//! Métricas de requests y auditoría (solo admin).
//!
//! Camino caliente barato: el middleware `track` anota lo mínimo (método, plantilla de ruta,
//! status, ms hasta los headers, IP, usuario) y lo manda por un canal acotado con `try_send` (si
//! está lleno se descarta y se cuenta: nunca frena un request). Los bytes enviados los cuenta un
//! envoltorio del cuerpo (un `u64` por respuesta, sin atómicos) y el evento sale cuando el cuerpo
//! termina o se corta (en un `<video>`, al cerrar la conexión). Una tarea aparte junta eventos y
//! los escribe en SQLite por lotes (una transacción cada ≤ 2 s): crudos en `request_log` (~14
//! días, tope de filas) y sumados por hora en `request_hourly` (~90 días).
//!
//! Nunca se guarda la URL real: solo la plantilla de axum (`/api/media/{session}/file`), así el
//! `?t=<token>` y cualquier query quedan afuera. Tampoco contraseñas ni claves (los eventos de
//! auditoría llevan solo nombre de usuario e IP).
//!
//! IP: la del socket. Solo si el socket es loopback (`tailscale serve`, el proxy de Vite) se
//! confía en `X-Forwarded-For` (el último valor, el que agregó el proxy) y en
//! `Tailscale-User-Login`/`Tailscale-User-Name`; si no, esos headers se ignoran.
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};

use axum::{
    body::{Body, Bytes, HttpBody},
    extract::{ConnectInfo, MatchedPath, Request, State},
    http::{HeaderMap, Method},
    middleware::Next,
    response::Response,
    routing::get,
    Router,
};
use http_body::Frame;
use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{
    auth::AdminUser,
    db::{now_ms, Db},
    error::{ApiError, ApiResult, Json, Query},
    state::AppState,
};

/// Límites superiores (ms) de los baldes del histograma de latencia; el último es "más".
pub const BUCKETS_MS: [i64; 12] = [5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000, i64::MAX];
const CHANNEL: usize = 20_000;
const BATCH: usize = 2_000;
const RAW_KEEP_MS: i64 = 14 * 24 * 3600 * 1000;
const RAW_MAX_ROWS: i64 = 1_000_000;
const HOURLY_KEEP_MS: i64 = 90 * 24 * 3600 * 1000;
const AUDIT_KEEP_MS: i64 = 365 * 24 * 3600 * 1000;
const HOUR_MS: i64 = 3600 * 1000;

#[derive(Debug)]
pub struct RequestEvent {
    pub ts_ms: i64,
    pub ip: Option<IpAddr>,
    pub ts_login: Option<String>,
    pub user_id: Option<i64>,
    pub method: String,
    pub route: String,
    pub status: u16,
    pub dur_ms: i64,
    pub bytes: u64,
}

#[derive(Debug)]
pub struct AuditEvent {
    pub ts_ms: i64,
    pub event: &'static str,
    pub user_id: Option<i64>,
    pub username: Option<String>,
    pub ip: Option<IpAddr>,
    pub detail: Option<String>,
}

#[derive(Debug)]
pub enum Event {
    Request(RequestEvent),
    Audit(AuditEvent),
}

/// Manija para registrar eventos. `Recorder::disabled()` (modo local, tests) no hace nada.
#[derive(Clone, Default)]
pub struct Recorder {
    tx: Option<mpsc::Sender<Event>>,
    dropped: Arc<AtomicU64>,
}

impl Recorder {
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Recorder + la tarea que escribe (hay que lanzarla con `tokio::spawn`).
    pub fn start(db: Db) -> (Self, impl std::future::Future<Output = ()>) {
        let (tx, rx) = mpsc::channel(CHANNEL);
        (Self { tx: Some(tx), dropped: Arc::default() }, writer(db, rx))
    }

    pub fn enabled(&self) -> bool {
        self.tx.is_some()
    }

    fn send(&self, e: Event) {
        if let Some(tx) = &self.tx {
            if tx.try_send(e).is_err() {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Evento de auditoría (login, logout, usuarios...). `detail` nunca lleva secretos.
    pub fn audit(&self, event: &'static str, user_id: Option<i64>, username: Option<&str>, ip: Option<IpAddr>, detail: Option<String>) {
        if self.tx.is_none() {
            return;
        }
        self.send(Event::Audit(AuditEvent {
            ts_ms: now_ms(),
            event,
            user_id,
            username: username.map(|u| u.chars().take(64).collect()),
            ip,
            detail: detail.map(|d| d.chars().take(200).collect()),
        }));
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

// ---------- middleware ----------

/// Lo que se va sabiendo de un request mientras pasa por el router (ruta matcheada, usuario).
#[derive(Clone, Default)]
pub struct ReqSlot(Arc<Mutex<SlotData>>);

#[derive(Default)]
struct SlotData {
    route: Option<String>,
    user_id: Option<i64>,
}

impl ReqSlot {
    pub fn set_user(&self, id: i64) {
        self.0.lock().unwrap().user_id = Some(id);
    }
}

/// IP real del cliente y login de Tailscale (ver la cabecera del módulo).
pub fn client_ip(peer: Option<IpAddr>, headers: &HeaderMap) -> (IpAddr, Option<String>) {
    let peer = peer.unwrap_or(IpAddr::from([127, 0, 0, 1]));
    let loopback = match peer {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v| v.is_loopback()),
    };
    if !loopback {
        return (peer, None);
    }
    let fwd = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .and_then(|v| v.trim().parse().ok());
    let login = ["tailscale-user-login", "tailscale-user-name"]
        .iter()
        .find_map(|h| headers.get(*h).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty()))
        .map(|v| v.chars().filter(|c| !c.is_control()).take(100).collect());
    (fwd.unwrap_or(peer), login)
}

/// `route_layer`: corre después del ruteo y anota la plantilla de la ruta.
pub async fn note_route(req: Request, next: Next) -> Response {
    if let (Some(slot), Some(p)) = (req.extensions().get::<ReqSlot>(), req.extensions().get::<MatchedPath>()) {
        slot.0.lock().unwrap().route = Some(p.as_str().to_string());
    }
    next.run(req).await
}

/// Middleware externo: mide y, al terminar el cuerpo, manda el evento.
pub async fn track(State(rec): State<Recorder>, mut req: Request, next: Next) -> Response {
    if !rec.enabled() {
        return next.run(req).await;
    }
    let t0 = Instant::now();
    let ts_ms = now_ms();
    let method = req.method().clone();
    let is_api = req.uri().path().starts_with("/api/") || req.uri().path() == "/api";
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    let (ip, ts_login) = client_ip(peer, req.headers());
    let slot = ReqSlot::default();
    req.extensions_mut().insert(slot.clone());
    let res = next.run(req).await;
    let dur_ms = t0.elapsed().as_millis() as i64;
    let (route, user_id) = {
        let s = slot.0.lock().unwrap();
        (s.route.clone(), s.user_id)
    };
    let route = route.unwrap_or_else(|| if is_api { "/api/{unknown}".into() } else { "ui".into() });
    let ev = RequestEvent {
        ts_ms,
        ip: Some(ip),
        ts_login,
        user_id,
        method: method_name(&method),
        route,
        status: res.status().as_u16(),
        dur_ms,
        bytes: 0,
    };
    let (parts, body) = res.into_parts();
    Response::from_parts(parts, Body::new(Counted { inner: body, ev: Some(ev), rec }))
}

fn method_name(m: &Method) -> String {
    // Métodos raros no inflan la tabla.
    match *m {
        Method::GET | Method::POST | Method::PUT | Method::PATCH | Method::DELETE | Method::HEAD | Method::OPTIONS => m.as_str().into(),
        _ => "OTHER".into(),
    }
}

/// Cuerpo que cuenta bytes y manda el evento al terminar (o al soltarse a medias).
struct Counted {
    inner: Body,
    ev: Option<RequestEvent>,
    rec: Recorder,
}

impl HttpBody for Counted {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        let r = Pin::new(&mut self.inner).poll_frame(cx);
        if let Poll::Ready(Some(Ok(f))) = &r {
            if let (Some(d), Some(ev)) = (f.data_ref(), self.ev.as_mut()) {
                ev.bytes += d.len() as u64;
            }
        }
        r
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        if let Some(ev) = self.ev.take() {
            self.rec.send(Event::Request(ev));
        }
    }
}

// ---------- escritura ----------

async fn writer(db: Db, mut rx: mpsc::Receiver<Event>) {
    let mut last_prune = Instant::now() - Duration::from_secs(3600);
    loop {
        let first = tokio::select! {
            e = rx.recv() => e,
            _ = tokio::time::sleep(Duration::from_secs(600)) => {
                prune_if_due(&db, &mut last_prune).await;
                continue;
            }
        };
        let Some(first) = first else { break };
        let mut batch = vec![first];
        // Junta lo que llegue en los próximos 2 s (o hasta llenar el lote).
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while batch.len() < BATCH {
            match tokio::time::timeout_at(deadline, rx.recv()).await {
                Ok(Some(e)) => batch.push(e),
                Ok(None) | Err(_) => break,
            }
        }
        if let Err(e) = db.call(move |c| Ok(write_batch(c, &batch)?)).await {
            tracing::warn!("metrics: no se pudo guardar un lote: {}", e.message);
        }
        prune_if_due(&db, &mut last_prune).await;
    }
}

async fn prune_if_due(db: &Db, last: &mut Instant) {
    if last.elapsed() < Duration::from_secs(3600) {
        return;
    }
    *last = Instant::now();
    let r = db
        .call(|c| {
            let now = now_ms();
            c.execute("DELETE FROM request_log WHERE ts_ms < ?1", [now - RAW_KEEP_MS])?;
            c.execute("DELETE FROM request_log WHERE id <= (SELECT MAX(id) FROM request_log) - ?1", [RAW_MAX_ROWS])?;
            c.execute("DELETE FROM request_hourly WHERE hour < ?1", [now - HOURLY_KEEP_MS])?;
            c.execute("DELETE FROM audit WHERE ts_ms < ?1", [now - AUDIT_KEEP_MS])?;
            Ok(())
        })
        .await;
    if let Err(e) = r {
        tracing::warn!("metrics: limpieza falló: {}", e.message);
    }
}

fn bucket(ms: i64) -> usize {
    BUCKETS_MS.iter().position(|b| ms <= *b).unwrap_or(BUCKETS_MS.len() - 1)
}

type HourKey = (i64, String, String, u16, i64, String, String);

#[derive(Default)]
struct HourAgg {
    count: i64,
    bytes: i64,
    dur: i64,
    h: [i64; 12],
}

/// Escribe un lote en una transacción (público para los tests).
pub fn write_batch(c: &mut Connection, batch: &[Event]) -> rusqlite::Result<()> {
    let tx = c.transaction()?;
    let mut hourly: HashMap<HourKey, HourAgg> = HashMap::new();
    {
        let mut raw = tx.prepare_cached(
            "INSERT INTO request_log (ts_ms, ip, ts_login, user_id, method, route, status, dur_ms, bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        let mut audit = tx.prepare_cached("INSERT INTO audit (ts_ms, event, user_id, username, ip, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
        for e in batch {
            match e {
                Event::Request(r) => {
                    let ip = r.ip.map(|i| i.to_string());
                    raw.execute(params![r.ts_ms, ip, r.ts_login, r.user_id, r.method, r.route, r.status, r.dur_ms, r.bytes as i64])?;
                    let key = (
                        r.ts_ms - r.ts_ms.rem_euclid(HOUR_MS),
                        r.method.clone(),
                        r.route.clone(),
                        r.status,
                        r.user_id.unwrap_or(0),
                        ip.unwrap_or_default(),
                        r.ts_login.clone().unwrap_or_default(),
                    );
                    let a = hourly.entry(key).or_default();
                    a.count += 1;
                    a.bytes += r.bytes as i64;
                    a.dur += r.dur_ms;
                    a.h[bucket(r.dur_ms)] += 1;
                }
                Event::Audit(a) => {
                    audit.execute(params![a.ts_ms, a.event, a.user_id, a.username, a.ip.map(|i| i.to_string()), a.detail])?;
                }
            }
        }
        let cols: Vec<String> = (0..12).map(|i| format!("h{i}")).collect();
        let sql = format!(
            "INSERT INTO request_hourly (hour, method, route, status, user_id, ip, ts_login, count, bytes, dur_sum, {})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, {})
             ON CONFLICT(hour, method, route, status, user_id, ip, ts_login) DO UPDATE SET
               count = count + excluded.count, bytes = bytes + excluded.bytes, dur_sum = dur_sum + excluded.dur_sum, {}",
            cols.join(", "),
            (11..23).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", "),
            cols.iter().map(|c| format!("{c} = {c} + excluded.{c}")).collect::<Vec<_>>().join(", "),
        );
        let mut up = tx.prepare_cached(&sql)?;
        for ((hour, method, route, status, uid, ip, login), a) in &hourly {
            let mut p: Vec<rusqlite::types::Value> = vec![
                (*hour).into(),
                method.clone().into(),
                route.clone().into(),
                (*status as i64).into(),
                (*uid).into(),
                ip.clone().into(),
                login.clone().into(),
                a.count.into(),
                a.bytes.into(),
                a.dur.into(),
            ];
            p.extend(a.h.iter().map(|x| rusqlite::types::Value::from(*x)));
            up.execute(params_from_iter(p))?;
        }
    }
    tx.commit()
}

// ---------- rutas de admin ----------

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/metrics", get(metrics))
        .route("/admin/logs", get(logs))
        .route("/admin/audit", get(audit))
}

#[derive(Deserialize)]
struct RangeQuery {
    range: Option<String>,
}

#[derive(Serialize, Default, Debug)]
pub struct Totals {
    pub requests: i64,
    /// status ≥ 500
    pub errors: i64,
    /// 400..499
    pub client_errors: i64,
    pub bytes_out: i64,
    /// Aproximados (histograma: el límite superior del balde).
    pub p50_ms: Option<i64>,
    pub p95_ms: Option<i64>,
}

#[derive(Serialize, Debug)]
pub struct HourPoint {
    pub hour_ms: i64,
    pub requests: i64,
    pub errors: i64,
    pub bytes_out: i64,
    pub p95_ms: Option<i64>,
}

#[derive(Serialize, Debug)]
pub struct IpStat {
    pub ip: String,
    pub requests: i64,
    pub bytes_out: i64,
    pub users: Vec<String>,
    pub tailscale_logins: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct UserStat {
    pub user_id: i64,
    pub username: Option<String>,
    pub requests: i64,
    pub bytes_out: i64,
}

#[derive(Serialize, Debug)]
pub struct RouteStat {
    pub method: String,
    pub route: String,
    pub requests: i64,
    pub errors: i64,
    pub bytes_out: i64,
    pub p50_ms: Option<i64>,
    pub p95_ms: Option<i64>,
}

#[derive(Serialize, Debug)]
pub struct StatusStat {
    pub status: u16,
    pub requests: i64,
}

#[derive(Serialize, Debug)]
pub struct FailedLogins {
    pub ip: String,
    pub count: i64,
    pub last_at_ms: i64,
    /// Nombres probados (hasta 5).
    pub usernames: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct MetricsOut {
    pub range: String,
    pub from_ms: i64,
    pub to_ms: i64,
    pub totals: Totals,
    pub series: Vec<HourPoint>,
    pub top_ips: Vec<IpStat>,
    pub top_users: Vec<UserStat>,
    pub top_routes: Vec<RouteStat>,
    pub status: Vec<StatusStat>,
    pub failed_logins: Vec<FailedLogins>,
    /// Eventos descartados desde que arrancó el server (canal lleno).
    pub dropped_events: u64,
}

fn percentile(h: &[i64; 12], p: f64) -> Option<i64> {
    let total: i64 = h.iter().sum();
    if total == 0 {
        return None;
    }
    let want = (total as f64 * p).ceil().max(1.0) as i64;
    let mut acc = 0;
    for (i, n) in h.iter().enumerate() {
        acc += n;
        if acc >= want {
            // El último balde no tiene techo: se informa su piso.
            return Some(if i == 11 { BUCKETS_MS[10] } else { BUCKETS_MS[i] });
        }
    }
    None
}

fn add_hist(acc: &mut [i64; 12], r: &rusqlite::Row, from: usize) -> rusqlite::Result<()> {
    for (i, a) in acc.iter_mut().enumerate() {
        *a += r.get::<_, i64>(from + i)?;
    }
    Ok(())
}

const HSUM: &str = "SUM(h0), SUM(h1), SUM(h2), SUM(h3), SUM(h4), SUM(h5), SUM(h6), SUM(h7), SUM(h8), SUM(h9), SUM(h10), SUM(h11)";

/// Arma el resumen de métricas desde `from_ms` (público para los tests).
pub fn summarize(c: &Connection, range: &str, from_ms: i64, to_ms: i64) -> rusqlite::Result<MetricsOut> {
    let from_hour = from_ms - from_ms.rem_euclid(HOUR_MS);
    // Series y totales.
    let mut series = Vec::new();
    let mut totals = Totals::default();
    let mut all_h = [0i64; 12];
    {
        let mut st = c.prepare(&format!(
            "SELECT hour, SUM(count), SUM(CASE WHEN status >= 500 THEN count ELSE 0 END),
                    SUM(CASE WHEN status >= 400 AND status < 500 THEN count ELSE 0 END), SUM(bytes), {HSUM}
             FROM request_hourly WHERE hour >= ?1 GROUP BY hour ORDER BY hour"
        ))?;
        let mut rows = st.query([from_hour])?;
        while let Some(r) = rows.next()? {
            let mut h = [0i64; 12];
            add_hist(&mut h, r, 5)?;
            let (req, err, cerr, bytes): (i64, i64, i64, i64) = (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
            totals.requests += req;
            totals.errors += err;
            totals.client_errors += cerr;
            totals.bytes_out += bytes;
            for (a, b) in all_h.iter_mut().zip(h.iter()) {
                *a += b;
            }
            series.push(HourPoint { hour_ms: r.get(0)?, requests: req, errors: err, bytes_out: bytes, p95_ms: percentile(&h, 0.95) });
        }
    }
    totals.p50_ms = percentile(&all_h, 0.5);
    totals.p95_ms = percentile(&all_h, 0.95);

    let usernames: HashMap<i64, String> = {
        let mut st = c.prepare("SELECT id, username FROM users")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };

    // Top IPs (con usuarios y logins de Tailscale vistos en esa IP).
    let mut ips: BTreeMap<String, (i64, i64, BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    {
        let mut st = c.prepare(
            "SELECT ip, user_id, ts_login, SUM(count), SUM(bytes) FROM request_hourly WHERE hour >= ?1 GROUP BY ip, user_id, ts_login",
        )?;
        let mut rows = st.query([from_hour])?;
        while let Some(r) = rows.next()? {
            let (ip, uid, login, n, b): (String, i64, String, i64, i64) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
            let e = ips.entry(ip).or_default();
            e.0 += n;
            e.1 += b;
            if let Some(u) = usernames.get(&uid) {
                e.2.insert(u.clone());
            }
            if !login.is_empty() {
                e.3.insert(login);
            }
        }
    }
    let mut top_ips: Vec<IpStat> = ips
        .into_iter()
        .map(|(ip, (requests, bytes_out, users, logins))| IpStat {
            ip,
            requests,
            bytes_out,
            users: users.into_iter().collect(),
            tailscale_logins: logins.into_iter().collect(),
        })
        .collect();
    top_ips.sort_by_key(|x| std::cmp::Reverse(x.requests));
    top_ips.truncate(20);

    let top_users = {
        let mut st = c.prepare(
            "SELECT user_id, SUM(count) AS n, SUM(bytes) FROM request_hourly WHERE hour >= ?1 AND user_id <> 0
             GROUP BY user_id ORDER BY n DESC LIMIT 20",
        )?;
        let rows = st.query_map([from_hour], |r| {
            let uid: i64 = r.get(0)?;
            Ok(UserStat { user_id: uid, username: usernames.get(&uid).cloned(), requests: r.get(1)?, bytes_out: r.get(2)? })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let top_routes = {
        let mut st = c.prepare(&format!(
            "SELECT method, route, SUM(count) AS n, SUM(CASE WHEN status >= 500 THEN count ELSE 0 END), SUM(bytes), {HSUM}
             FROM request_hourly WHERE hour >= ?1 GROUP BY method, route ORDER BY n DESC LIMIT 30"
        ))?;
        let rows = st.query_map([from_hour], |r| {
            let mut h = [0i64; 12];
            add_hist(&mut h, r, 5)?;
            Ok(RouteStat {
                method: r.get(0)?,
                route: r.get(1)?,
                requests: r.get(2)?,
                errors: r.get(3)?,
                bytes_out: r.get(4)?,
                p50_ms: percentile(&h, 0.5),
                p95_ms: percentile(&h, 0.95),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let status = {
        let mut st = c.prepare("SELECT status, SUM(count) AS n FROM request_hourly WHERE hour >= ?1 GROUP BY status ORDER BY status")?;
        let rows = st.query_map([from_hour], |r| Ok(StatusStat { status: r.get::<_, i64>(0)? as u16, requests: r.get(1)? }))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let failed_logins = {
        let mut st = c.prepare(
            "SELECT COALESCE(ip, ''), COUNT(*) AS n, MAX(ts_ms), GROUP_CONCAT(DISTINCT username) FROM audit
             WHERE event = 'login_fail' AND ts_ms >= ?1 GROUP BY ip ORDER BY n DESC LIMIT 20",
        )?;
        let rows = st.query_map([from_ms], |r| {
            let names: Option<String> = r.get(3)?;
            Ok(FailedLogins {
                ip: r.get(0)?,
                count: r.get(1)?,
                last_at_ms: r.get(2)?,
                usernames: names.map(|n| n.split(',').take(5).map(str::to_string).collect()).unwrap_or_default(),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    Ok(MetricsOut {
        range: range.to_string(),
        from_ms,
        to_ms,
        totals,
        series,
        top_ips,
        top_users,
        top_routes,
        status,
        failed_logins,
        dropped_events: 0,
    })
}

async fn metrics(State(st): State<AppState>, _a: AdminUser, Query(q): Query<RangeQuery>) -> ApiResult<Json<MetricsOut>> {
    let range = q.range.unwrap_or_else(|| "24h".into());
    let span_h = match range.as_str() {
        "24h" => 24,
        "7d" => 7 * 24,
        "30d" => 30 * 24,
        "90d" => 90 * 24,
        _ => return Err(ApiError::bad_request("range must be 24h, 7d, 30d or 90d")),
    };
    let to = now_ms();
    let from = to - span_h * HOUR_MS;
    let mut out = st.db.call(move |c| Ok(summarize(c, &range, from, to)?)).await?;
    out.dropped_events = st.recorder.dropped();
    Ok(Json(out))
}

#[derive(Deserialize)]
struct LogsQuery {
    limit: Option<i64>,
    before: Option<i64>,
    /// nombre de usuario o id
    user: Option<String>,
    ip: Option<String>,
    /// "404" o una clase "5xx"
    status: Option<String>,
    /// parte de la plantilla de ruta
    route: Option<String>,
}

#[derive(Serialize)]
struct LogRow {
    id: i64,
    ts_ms: i64,
    ip: Option<String>,
    tailscale_login: Option<String>,
    user_id: Option<i64>,
    username: Option<String>,
    method: String,
    route: String,
    status: i64,
    dur_ms: i64,
    bytes: i64,
}

#[derive(Serialize)]
struct Page<T> {
    items: Vec<T>,
    /// Pasar como `before` para la página siguiente (`null` = no hay más).
    next_before: Option<i64>,
}

fn resolve_user(c: &Connection, user: &str) -> rusqlite::Result<i64> {
    if let Ok(id) = user.parse::<i64>() {
        return Ok(id);
    }
    use rusqlite::OptionalExtension;
    Ok(c.query_row("SELECT id FROM users WHERE username = ?1", [user], |r| r.get(0)).optional()?.unwrap_or(-1))
}

async fn logs(State(st): State<AppState>, _a: AdminUser, Query(q): Query<LogsQuery>) -> ApiResult<Json<Page<LogRow>>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let status = match q.status.as_deref().filter(|s| !s.is_empty()) {
        None => None,
        Some(s) if s.len() == 3 && s.ends_with("xx") => {
            let d: i64 = s[..1].parse().map_err(|_| ApiError::bad_request("Invalid status filter"))?;
            Some((d * 100, d * 100 + 99))
        }
        Some(s) => {
            let n: i64 = s.parse().map_err(|_| ApiError::bad_request("Invalid status filter"))?;
            Some((n, n))
        }
    };
    let page = st
        .db
        .call(move |c| {
            let mut sql = String::from(
                "SELECT l.id, l.ts_ms, l.ip, l.ts_login, l.user_id, u.username, l.method, l.route, l.status, l.dur_ms, l.bytes
                 FROM request_log l LEFT JOIN users u ON u.id = l.user_id WHERE 1 = 1",
            );
            let mut p: Vec<rusqlite::types::Value> = Vec::new();
            if let Some(b) = q.before {
                p.push(b.into());
                sql += &format!(" AND l.id < ?{}", p.len());
            }
            if let Some(u) = q.user.as_deref().filter(|s| !s.is_empty()) {
                p.push(resolve_user(c, u)?.into());
                sql += &format!(" AND l.user_id = ?{}", p.len());
            }
            if let Some(ip) = q.ip.filter(|s| !s.is_empty()) {
                p.push(ip.into());
                sql += &format!(" AND l.ip = ?{}", p.len());
            }
            if let Some((lo, hi)) = status {
                p.push(lo.into());
                p.push(hi.into());
                sql += &format!(" AND l.status BETWEEN ?{} AND ?{}", p.len() - 1, p.len());
            }
            if let Some(r) = q.route.filter(|s| !s.is_empty()) {
                p.push(r.into());
                sql += &format!(" AND instr(l.route, ?{}) > 0", p.len());
            }
            p.push((limit + 1).into());
            sql += &format!(" ORDER BY l.id DESC LIMIT ?{}", p.len());
            let mut stmt = c.prepare(&sql)?;
            let mut items = stmt
                .query_map(params_from_iter(p), |r| {
                    Ok(LogRow {
                        id: r.get(0)?,
                        ts_ms: r.get(1)?,
                        ip: r.get(2)?,
                        tailscale_login: r.get(3)?,
                        user_id: r.get(4)?,
                        username: r.get(5)?,
                        method: r.get(6)?,
                        route: r.get(7)?,
                        status: r.get(8)?,
                        dur_ms: r.get(9)?,
                        bytes: r.get(10)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let more = items.len() as i64 > limit;
            items.truncate(limit as usize);
            let next_before = if more { items.last().map(|i| i.id) } else { None };
            Ok(Page { items, next_before })
        })
        .await?;
    Ok(Json(page))
}

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
    before: Option<i64>,
    event: Option<String>,
    user: Option<String>,
}

#[derive(Serialize)]
struct AuditRow {
    id: i64,
    ts_ms: i64,
    event: String,
    user_id: Option<i64>,
    username: Option<String>,
    ip: Option<String>,
    detail: Option<String>,
}

async fn audit(State(st): State<AppState>, _a: AdminUser, Query(q): Query<AuditQuery>) -> ApiResult<Json<Page<AuditRow>>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let page = st
        .db
        .call(move |c| {
            let mut sql = String::from("SELECT id, ts_ms, event, user_id, username, ip, detail FROM audit WHERE 1 = 1");
            let mut p: Vec<rusqlite::types::Value> = Vec::new();
            if let Some(b) = q.before {
                p.push(b.into());
                sql += &format!(" AND id < ?{}", p.len());
            }
            if let Some(e) = q.event.filter(|s| !s.is_empty()) {
                p.push(e.into());
                sql += &format!(" AND event = ?{}", p.len());
            }
            if let Some(u) = q.user.filter(|s| !s.is_empty()) {
                p.push(u.into());
                sql += &format!(" AND username = ?{} COLLATE NOCASE", p.len());
            }
            p.push((limit + 1).into());
            sql += &format!(" ORDER BY id DESC LIMIT ?{}", p.len());
            let mut stmt = c.prepare(&sql)?;
            let mut items = stmt
                .query_map(params_from_iter(p), |r| {
                    Ok(AuditRow {
                        id: r.get(0)?,
                        ts_ms: r.get(1)?,
                        event: r.get(2)?,
                        user_id: r.get(3)?,
                        username: r.get(4)?,
                        ip: r.get(5)?,
                        detail: r.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let more = items.len() as i64 > limit;
            items.truncate(limit as usize);
            let next_before = if more { items.last().map(|i| i.id) } else { None };
            Ok(Page { items, next_before })
        })
        .await?;
    Ok(Json(page))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(ts: i64, route: &str, status: u16, dur: i64, bytes: u64, uid: Option<i64>, ip: &str) -> Event {
        Event::Request(RequestEvent {
            ts_ms: ts,
            ip: Some(ip.parse().unwrap()),
            ts_login: None,
            user_id: uid,
            method: "GET".into(),
            route: route.into(),
            status,
            dur_ms: dur,
            bytes,
        })
    }

    #[test]
    fn percentiles_from_histogram() {
        let mut h = [0i64; 12];
        h[0] = 50; // ≤ 5 ms
        h[4] = 45; // ≤ 100 ms
        h[9] = 5; // ≤ 5000 ms
        assert_eq!(percentile(&h, 0.5), Some(5));
        assert_eq!(percentile(&h, 0.95), Some(100));
        assert_eq!(percentile(&h, 0.99), Some(5000));
        assert_eq!(percentile(&[0; 12], 0.5), None);
    }

    #[test]
    fn client_ip_trusts_headers_only_from_loopback() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "1.2.3.4, 100.64.0.7".parse().unwrap());
        h.insert("tailscale-user-login", "martin@github".parse().unwrap());
        let (ip, login) = client_ip(Some("127.0.0.1".parse().unwrap()), &h);
        assert_eq!(ip.to_string(), "100.64.0.7");
        assert_eq!(login.as_deref(), Some("martin@github"));
        let (ip, login) = client_ip(Some("192.168.1.9".parse().unwrap()), &h);
        assert_eq!(ip.to_string(), "192.168.1.9");
        assert!(login.is_none());
    }

    #[test]
    fn aggregation() {
        let db = Db::open_in_memory().unwrap();
        let mut c = db.conn.lock().unwrap();
        c.execute("INSERT INTO users (id, username, password_hash) VALUES (7, 'ana', 'x')", []).unwrap();
        let now = now_ms();
        let hour = now - now.rem_euclid(HOUR_MS);
        let batch = vec![
            ev(hour + 1, "/api/meta/{type}/{id}", 200, 3, 1000, Some(7), "100.64.0.1"),
            ev(hour + 2, "/api/meta/{type}/{id}", 200, 80, 500, Some(7), "100.64.0.1"),
            ev(hour + 3, "/api/streams/{type}/{id}", 502, 2000, 50, Some(7), "100.64.0.2"),
            ev(hour - HOUR_MS + 5, "/api/health", 200, 1, 10, None, "127.0.0.1"),
            Event::Audit(AuditEvent { ts_ms: now, event: "login_fail", user_id: None, username: Some("bob".into()), ip: Some("100.64.0.9".parse().unwrap()), detail: None }),
        ];
        write_batch(&mut c, &batch).unwrap();
        // Un segundo lote suma sobre las mismas filas por hora.
        write_batch(&mut c, &[ev(hour + 9, "/api/meta/{type}/{id}", 404, 7, 20, Some(7), "100.64.0.1")]).unwrap();
        let raw: i64 = c.query_row("SELECT COUNT(*) FROM request_log", [], |r| r.get(0)).unwrap();
        assert_eq!(raw, 5);
        let m = summarize(&c, "24h", now - 24 * HOUR_MS, now).unwrap();
        assert_eq!(m.totals.requests, 5);
        assert_eq!(m.totals.errors, 1);
        assert_eq!(m.totals.client_errors, 1);
        assert_eq!(m.totals.bytes_out, 1580);
        assert_eq!(m.series.len(), 2);
        assert_eq!(m.top_routes[0].route, "/api/meta/{type}/{id}");
        assert_eq!(m.top_routes[0].requests, 3);
        assert_eq!(m.top_ips[0].ip, "100.64.0.1");
        assert_eq!(m.top_ips[0].users, vec!["ana".to_string()]);
        assert_eq!(m.top_users[0].username.as_deref(), Some("ana"));
        assert_eq!(m.top_users[0].requests, 4);
        assert_eq!(m.status.iter().find(|s| s.status == 502).unwrap().requests, 1);
        assert_eq!(m.failed_logins[0].ip, "100.64.0.9");
        assert_eq!(m.failed_logins[0].usernames, vec!["bob".to_string()]);
    }
}
