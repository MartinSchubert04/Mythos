//! Modo local (server embebido en la app desktop/Android). Ver `docs/local-mode.md`.
//!
//! - **Login** (`POST /api/auth/login`): si hay server remoto, el login se hace contra él (el
//!   local hace de intermediario). Si el remoto acepta, la cuenta local queda "enrolada": mismo
//!   nombre, y una verificación **propia** de la contraseña (argon2id con sal nueva, calculada acá
//!   con la contraseña que acaba de validar el remoto). El hash del server nunca sale del server.
//!   La respuesta trae el token local y también el del remoto, para que la UI le hable directo
//!   al server mientras se lo alcance. Si el remoto no responde, se valida contra la verificación
//!   local (login offline). Si el remoto rechaza la contraseña, no se prueba la local.
//! - **Sincronización**: el server local guarda una sesión del remoto por cuenta (`local_link`) y
//!   sube/baja cambios solo (cada `sync_interval`, y unos segundos después de cada cambio local),
//!   o a pedido con `POST /api/local/sync`. La UI no implementa nada de la lógica.
//! - Sin admin (403), sin cambio de contraseña (se cambia en el server), y solo se aceptan
//!   requests con `Host` de loopback (contra DNS rebinding desde una página web).
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{any, get, post, put},
    Router,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    auth::{hash_password, user_from_row, AuthUser, ClientIp, SessionMeta, User, USER_COLS},
    db::{current_seq, now_ms},
    error::{ApiError, ApiResult, Json},
    state::AppState,
    sync::{self, Pull, PushResult},
};

/// Subida en lotes de este tamaño.
const PUSH_BATCH: usize = 500;
/// Cuánto vale una comprobación de "¿se alcanza el remoto?".
const REACH_TTL: Duration = Duration::from_secs(15);

/// Estado del modo local.
pub struct LocalCtx {
    http: reqwest::Client,
    /// Remoto por defecto (de la config), normalizado.
    pub default_remote: Option<String>,
    reach: Mutex<HashMap<String, (bool, Instant)>>,
    /// Una sincronización a la vez.
    sync_lock: tokio::sync::Mutex<()>,
}

impl LocalCtx {
    pub fn new(default_remote: Option<&str>) -> anyhow::Result<Self> {
        let http = mythos_media::http_client_builder()
            .user_agent(concat!("Mythos-local/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            default_remote: default_remote.and_then(|u| normalize_url(u).ok()),
            reach: Mutex::new(HashMap::new()),
            sync_lock: tokio::sync::Mutex::new(()),
        })
    }

    fn set_reachable(&self, url: &str, ok: bool) {
        self.reach.lock().unwrap().insert(url.to_string(), (ok, Instant::now()));
    }

    /// Último resultado conocido (sin comprobar).
    fn known_reachable(&self, url: &str) -> Option<bool> {
        self.reach.lock().unwrap().get(url).map(|(ok, _)| *ok)
    }

    /// `GET /api/health` del remoto (3 s), cacheado `REACH_TTL`.
    pub async fn reachable(&self, url: &str) -> bool {
        if let Some((ok, at)) = self.reach.lock().unwrap().get(url) {
            if at.elapsed() < REACH_TTL {
                return *ok;
            }
        }
        let ok = matches!(
            self.http.get(format!("{url}/api/health")).timeout(Duration::from_secs(3)).send().await,
            Ok(r) if r.status().is_success()
        );
        self.set_reachable(url, ok);
        ok
    }

    /// `server_id` del remoto en esa dirección (`GET /api/health`, 3 s). `None` si no contesta o
    /// es un server viejo que no lo manda.
    async fn remote_server_id(&self, url: &str) -> Option<String> {
        let res = self.http.get(format!("{url}/api/health")).timeout(Duration::from_secs(3)).send().await.ok()?;
        let v: Value = res.error_for_status().ok()?.json().await.ok()?;
        v.get("server_id").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
    }
}

/// `https://host:7700/` → `https://host:7700`. Solo http(s), sin query ni credenciales.
pub fn normalize_url(u: &str) -> ApiResult<String> {
    let parsed = url::Url::parse(u.trim()).map_err(|_| ApiError::bad_request("Invalid server URL").with_code("bad_server_url"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ApiError::bad_request("Server URL must be http(s)://host[:port]").with_code("server_url"));
    }
    let mut s = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or_default());
    if let Some(p) = parsed.port() {
        s.push_str(&format!(":{p}"));
    }
    let path = parsed.path().trim_end_matches('/');
    s.push_str(path);
    Ok(s)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/me/password", put(no_password_change))
        .route("/me/username", put(no_rename))
        .route("/local/status", get(status))
        .route("/local/sync", post(sync_now))
        .route("/admin", any(no_admin))
        .route("/admin/{*rest}", any(no_admin))
}

async fn no_admin() -> ApiError {
    ApiError::forbidden("Admin is not available in local mode").with_code("admin_local")
}

async fn no_password_change() -> ApiError {
    ApiError::forbidden("Change your password on the server; this device picks it up on the next online sign-in").with_code("pw_on_server")
}

async fn no_rename() -> ApiError {
    ApiError::forbidden("Change your username on the server; this device picks it up when it syncs").with_code("rename_on_server")
}

/// La cuenta local vinculada a esa cuenta del remoto toma su nombre (si lo cambiaron en el server).
/// Si otra cuenta local vieja tiene ese nombre (no vinculada a esta), se le corre el suyo para que
/// no choque. Devuelve si cambio algo.
fn follow_remote_name(c: &rusqlite::Connection, uid: i64, name: &str) -> ApiResult<bool> {
    let cur: String = c.query_row("SELECT username FROM users WHERE id = ?1", [uid], |r| r.get(0))?;
    if cur == name {
        return Ok(false);
    }
    c.execute(
        "UPDATE users SET username = username || '~' || id WHERE username = ?1 AND id <> ?2",
        params![name, uid],
    )?;
    c.execute("UPDATE users SET username = ?2 WHERE id = ?1", params![uid, name])?;
    Ok(true)
}

/// Solo `Host` de loopback (127.0.0.1, localhost, [::1]): una página web que apunte su dominio a
/// 127.0.0.1 (DNS rebinding) no puede hablarle a la API local.
pub async fn host_guard(req: Request, next: Next) -> Response {
    let ok = match req.headers().get(header::HOST).and_then(|h| h.to_str().ok()) {
        None => true,
        Some(h) => {
            let host = if let Some(rest) = h.strip_prefix('[') {
                rest.split(']').next().unwrap_or("")
            } else {
                h.rsplit_once(':').map(|(a, _)| a).unwrap_or(h)
            };
            matches!(host.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1")
        }
    };
    if ok {
        next.run(req).await
    } else {
        ApiError::forbidden("Forbidden host").into_response()
    }
}

// ---------- cliente del remoto ----------

#[derive(Debug)]
pub enum RemoteErr {
    /// No se llegó (red, timeout, 5xx): se puede seguir offline.
    Unreachable(String),
    /// El remoto respondió que no (401, 403, 429...): status, texto y su código, si mandó uno.
    Rejected(StatusCode, String, Option<String>),
}

impl std::fmt::Display for RemoteErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteErr::Unreachable(e) => write!(f, "server unreachable: {e}"),
            RemoteErr::Rejected(s, m, _) => write!(f, "server answered {}: {m}", s.as_u16()),
        }
    }
}

#[derive(Deserialize)]
struct ErrBody {
    error: String,
    code: Option<String>,
}

async fn remote_call(
    http: &reqwest::Client,
    method: reqwest::Method,
    url: String,
    token: Option<&str>,
    body: Option<&impl Serialize>,
) -> Result<reqwest::Response, RemoteErr> {
    let mut req = http.request(method, url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    if let Some(b) = body {
        req = req.json(b);
    }
    // Sin la URL en el error (no tiene secretos, pero es ruido).
    let res = req.send().await.map_err(|e| RemoteErr::Unreachable(e.without_url().to_string()))?;
    let st = res.status();
    if st.is_success() {
        return Ok(res);
    }
    if st.is_server_error() {
        return Err(RemoteErr::Unreachable(format!("HTTP {st}")));
    }
    let (msg, code) = res.json::<ErrBody>().await.map(|b| (b.error, b.code)).unwrap_or_else(|_| (format!("HTTP {st}"), None));
    Err(RemoteErr::Rejected(StatusCode::from_u16(st.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY), msg, code))
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct RemoteUser {
    pub id: i64,
    pub username: String,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, Value>,
}

#[derive(Deserialize)]
struct RemoteLogin {
    token: String,
    user: RemoteUser,
}

async fn remote_login(ctx: &LocalCtx, url: &str, username: &str, password: &str, device: Option<&str>) -> Result<RemoteLogin, RemoteErr> {
    let body = serde_json::json!({ "username": username, "password": password, "device": device });
    let res = remote_call(&ctx.http, reqwest::Method::POST, format!("{url}/api/auth/login"), None, Some(&body)).await?;
    res.json().await.map_err(|e| RemoteErr::Unreachable(format!("invalid login response: {e}")))
}

async fn remote_me(ctx: &LocalCtx, url: &str, token: &str) -> Result<RemoteUser, RemoteErr> {
    let res = remote_call(&ctx.http, reqwest::Method::GET, format!("{url}/api/me"), Some(token), None::<&()>).await?;
    res.json().await.map_err(|e| RemoteErr::Unreachable(format!("invalid response: {e}")))
}

// ---------- vínculo local ↔ remoto ----------

#[derive(Clone, Debug)]
struct Link {
    remote_url: String,
    remote_token: Option<String>,
    remote_cursor: i64,
    pushed_seq: i64,
    last_sync_ms: Option<i64>,
    last_error: Option<String>,
}

/// El id de la cuenta del remoto vinculada a esta cuenta local, si hay.
async fn load_link_remote_id(st: &AppState, uid: i64) -> ApiResult<Option<i64>> {
    st.db
        .call(move |c| {
            Ok(c.query_row("SELECT remote_user_id FROM local_link WHERE user_id = ?1", [uid], |r| r.get::<_, Option<i64>>(0))
                .optional()?
                .flatten())
        })
        .await
}

async fn load_link(st: &AppState, uid: i64) -> ApiResult<Option<Link>> {
    st.db
        .call(move |c| {
            Ok(c.query_row(
                "SELECT remote_url, remote_token, remote_cursor, pushed_seq, last_sync_ms, last_error FROM local_link WHERE user_id = ?1",
                [uid],
                |r| {
                    Ok(Link {
                        remote_url: r.get(0)?,
                        remote_token: r.get(1)?,
                        remote_cursor: r.get(2)?,
                        pushed_seq: r.get(3)?,
                        last_sync_ms: r.get(4)?,
                        last_error: r.get(5)?,
                    })
                },
            )
            .optional()?)
        })
        .await
}

/// Guarda (o actualiza) el vínculo. Si cambia el server o la cuenta remota, los cursores vuelven
/// a 0 (sincronización completa, que por last-write-wins no pisa nada más nuevo). Otra dirección
/// del mismo server (mismo `server_id`: Tailscale o la red de casa) no cuenta como cambio.
async fn save_link(st: &AppState, uid: i64, url: &str, token: &str, remote_user_id: i64, server_id: Option<String>) -> ApiResult<()> {
    let (url, token) = (url.to_string(), token.to_string());
    let old = st
        .db
        .call(move |c| {
            let old: Option<(String, Option<String>, Option<String>)> = c
                .query_row("SELECT remote_url, remote_token, remote_server_id FROM local_link WHERE user_id = ?1", [uid], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .optional()?;
            // La sesión anterior es del mismo server: se cierra (en la dirección nueva).
            let old = old.and_then(|(u, t, sid)| t.filter(|_| u == url || (sid.is_some() && sid == server_id)));
            c.execute(
                "INSERT INTO local_link (user_id, remote_url, remote_token, remote_user_id, remote_server_id) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(user_id) DO UPDATE SET
                   remote_cursor = CASE WHEN (local_link.remote_url = excluded.remote_url
                                              OR local_link.remote_server_id = excluded.remote_server_id)
                                         AND local_link.remote_user_id IS excluded.remote_user_id
                                        THEN local_link.remote_cursor ELSE 0 END,
                   pushed_seq = CASE WHEN (local_link.remote_url = excluded.remote_url
                                           OR local_link.remote_server_id = excluded.remote_server_id)
                                      AND local_link.remote_user_id IS excluded.remote_user_id
                                     THEN local_link.pushed_seq ELSE 0 END,
                   remote_server_id = COALESCE(excluded.remote_server_id,
                                               CASE WHEN local_link.remote_url = excluded.remote_url THEN local_link.remote_server_id END),
                   remote_url = excluded.remote_url, remote_token = excluded.remote_token,
                   remote_user_id = excluded.remote_user_id, last_error = NULL",
                params![uid, url, token, remote_user_id, server_id],
            )?;
            Ok(old.filter(|o| *o != token).map(|o| (url, o)))
        })
        .await?;
    // La sesión remota anterior ya no se usa: se cierra (sin esperar).
    if let (Some((url, old)), Some(ctx)) = (old, st.local.clone()) {
        tokio::spawn(async move {
            let _ = remote_call(&ctx.http, reqwest::Method::POST, format!("{url}/api/auth/logout"), Some(&old), None::<&()>).await;
        });
    }
    Ok(())
}

/// Cuenta local con la contraseña ya validada por el remoto: verificación propia (sal nueva). Si
/// esa cuenta del remoto ya estaba vinculada a una local, es esa (aunque le hayan cambiado el
/// nombre en el server: toma el nuevo); si no, la del mismo nombre o una nueva.
async fn enroll(st: &AppState, url: &str, server_id: Option<&str>, remote: &RemoteUser, password: &str) -> ApiResult<User> {
    let (name, pw, url, rid) = (remote.username.clone(), password.to_string(), url.to_string(), remote.id);
    let sid = server_id.map(str::to_string);
    st.db
        .call(move |c| {
            let hash = hash_password(&pw)?;
            // El mismo server por cualquiera de sus direcciones.
            let linked: Option<i64> = c
                .query_row(
                    "SELECT user_id FROM local_link WHERE (remote_url = ?1 OR remote_server_id = ?3) AND remote_user_id = ?2
                     ORDER BY remote_url = ?1 DESC LIMIT 1",
                    params![url, rid, sid],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(uid) = linked {
                follow_remote_name(c, uid, &name)?;
                c.execute("UPDATE users SET password_hash = ?2, disabled = 0 WHERE id = ?1", params![uid, hash])?;
                return Ok(c.query_row(&format!("SELECT {USER_COLS} FROM users WHERE id = ?1"), [uid], user_from_row)?);
            }
            Ok(c.query_row(
                &format!(
                    "INSERT INTO users (username, password_hash, is_admin) VALUES (?1, ?2, 0)
                     ON CONFLICT(username) DO UPDATE SET username = excluded.username, password_hash = excluded.password_hash, disabled = 0
                     RETURNING {USER_COLS}"
                ),
                params![name, hash],
                user_from_row,
            )?)
        })
        .await
}

// ---------- login / logout ----------

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
    device: Option<String>,
    /// Server remoto a usar (si no, el del vínculo de esa cuenta o el de la config).
    remote_url: Option<String>,
    /// No intentar el remoto (modo "solo local").
    #[serde(default)]
    offline: bool,
}

#[derive(Serialize)]
struct RemoteSession {
    url: String,
    token: String,
    user: RemoteUser,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    user: User,
    /// `true` = validado contra la verificación local (el server no se alcanzó).
    offline: bool,
    /// Sesión del server remoto (para que la UI le hable directo). `null` si fue offline.
    remote: Option<RemoteSession>,
}

/// Remoto de una cuenta ya vinculada (por nombre).
async fn linked_remote(st: &AppState, username: &str) -> ApiResult<Option<String>> {
    let name = username.to_string();
    st.db
        .call(move |c| {
            Ok(c.query_row(
                "SELECT l.remote_url FROM local_link l JOIN users u ON u.id = l.user_id WHERE u.username = ?1",
                [name],
                |r| r.get(0),
            )
            .optional()?)
        })
        .await
}

async fn login(State(st): State<AppState>, ClientIp(ip): ClientIp, headers: HeaderMap, Json(b): Json<LoginBody>) -> ApiResult<Json<LoginResponse>> {
    let ctx = st.local.clone().ok_or_else(|| ApiError::internal("local mode without context"))?;
    st.auth.check_rate(ip)?;
    let username = b.username.trim().to_string();
    let meta = SessionMeta::new(b.device.as_deref(), &headers);
    let remote = match b.remote_url.as_deref().filter(|u| !u.trim().is_empty()) {
        Some(u) => Some(normalize_url(u)?),
        None => linked_remote(&st, &username).await?.or_else(|| ctx.default_remote.clone()),
    };
    if let (Some(url), false) = (remote.as_deref(), b.offline) {
        match remote_login(&ctx, url, &username, &b.password, meta.device.as_deref()).await {
            Ok(r) => {
                ctx.set_reachable(url, true);
                let server_id = ctx.remote_server_id(url).await;
                let user = enroll(&st, url, server_id.as_deref(), &r.user, &b.password).await?;
                save_link(&st, user.id, url, &r.token, r.user.id, server_id).await?;
                let token = st.auth.open_session(&user, ip, meta).await?;
                // Primera sincronización enseguida.
                let (st2, uid) = (st.clone(), user.id);
                tokio::spawn(async move {
                    if let Err(e) = sync_user(&st2, uid).await {
                        tracing::warn!("sync after login: {e}");
                    }
                });
                return Ok(Json(LoginResponse {
                    token,
                    user,
                    offline: false,
                    remote: Some(RemoteSession { url: url.to_string(), token: r.token, user: r.user }),
                }));
            }
            Err(RemoteErr::Rejected(status, msg, code)) => {
                ctx.set_reachable(url, true);
                st.auth.record_fail(ip, &username, Some("rejected by server"));
                // (Los servers viejos no mandan el código: también por el texto.)
                if code.as_deref() == Some("disabled") || msg == "This account is disabled" {
                    // Deshabilitada en el server: también acá (y fuera sus sesiones).
                    let name = username.clone();
                    st.db
                        .call(move |c| {
                            c.execute("UPDATE users SET disabled = 1 WHERE username = ?1", [&name])?;
                            c.execute("DELETE FROM sessions WHERE user_id IN (SELECT id FROM users WHERE username = ?1)", [&name])?;
                            Ok(())
                        })
                        .await?;
                    st.auth.clear_cache();
                }
                let status = if status == StatusCode::TOO_MANY_REQUESTS { status } else { StatusCode::UNAUTHORIZED };
                let mut e = ApiError::new(status, msg);
                e.code = code;
                return Err(e);
            }
            Err(RemoteErr::Unreachable(e)) => {
                ctx.set_reachable(url, false);
                tracing::info!("server unreachable ({e}); trying offline sign-in");
            }
        }
    }
    let (token, user) = st.auth.login(ip, &username, &b.password, meta).await.map_err(|e| {
        if e.code.as_deref() == Some("bad_login") {
            ApiError::unauthorized("Invalid username or password (offline: sign in once while the server is reachable)").with_code("bad_login_offline")
        } else {
            e
        }
    })?;
    Ok(Json(LoginResponse { token, user, offline: true, remote: None }))
}

/// Cierra la sesión local y, si era la última de esa cuenta en el dispositivo, también la del
/// remoto (deja de sincronizar hasta el próximo login online).
async fn logout(State(st): State<AppState>, ClientIp(ip): ClientIp, u: AuthUser) -> ApiResult<StatusCode> {
    st.auth.logout(&u, ip).await?;
    let uid = u.user.id;
    let remote = st
        .db
        .call(move |c| {
            let left: i64 = c.query_row("SELECT COUNT(*) FROM sessions WHERE user_id = ?1", [uid], |r| r.get(0))?;
            if left > 0 {
                return Ok(None);
            }
            let link: Option<(String, Option<String>)> = c
                .query_row("SELECT remote_url, remote_token FROM local_link WHERE user_id = ?1", [uid], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            c.execute("UPDATE local_link SET remote_token = NULL WHERE user_id = ?1", [uid])?;
            Ok(link.and_then(|(u, t)| t.map(|t| (u, t))))
        })
        .await?;
    if let (Some((url, token)), Some(ctx)) = (remote, st.local.clone()) {
        tokio::spawn(async move {
            let _ = remote_call(&ctx.http, reqwest::Method::POST, format!("{url}/api/auth/logout"), Some(&token), None::<&()>).await;
        });
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------- sincronización ----------

#[derive(Serialize, Debug, Default, Clone)]
pub struct SyncReport {
    /// Registros subidos al remoto.
    pub pushed: usize,
    /// Registros del remoto que cambiaron algo acá.
    pub pulled: usize,
    /// ms unix
    pub last_sync_at: i64,
}

#[derive(Debug)]
pub enum SyncError {
    NotLinked,
    SignedOut,
    Remote(RemoteErr),
    Local(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::NotLinked => write!(f, "this account was never signed in online on this device"),
            SyncError::SignedOut => write!(f, "signed out from the server: sign in again while online"),
            SyncError::Remote(e) => write!(f, "{e}"),
            SyncError::Local(e) => write!(f, "{e}"),
        }
    }
}

impl SyncError {
    /// Código estable para la UI (docs/API.md, "Errores").
    pub fn code(&self) -> &'static str {
        match self {
            SyncError::NotLinked => "sync_not_linked",
            SyncError::SignedOut => "sync_signed_out",
            SyncError::Remote(RemoteErr::Unreachable(_)) => "sync_unreachable",
            SyncError::Remote(RemoteErr::Rejected(_, _, Some(c))) if c == "sync_unsupported" => "sync_unsupported",
            _ => "sync_failed",
        }
    }
}

/// El código de un error de sincronización guardado como texto en `local_link.last_error` (lo
/// arma `SyncError`, en este archivo: ver `code`).
fn stored_sync_code(e: &str) -> &'static str {
    if e == SyncError::NotLinked.to_string() {
        "sync_not_linked"
    } else if e == SyncError::SignedOut.to_string() {
        "sync_signed_out"
    } else if e.starts_with("server unreachable") {
        "sync_unreachable"
    } else if e.ends_with("does not support sync yet (update it)") {
        "sync_unsupported"
    } else {
        "sync_failed"
    }
}

impl From<ApiError> for SyncError {
    fn from(e: ApiError) -> Self {
        SyncError::Local(e.message)
    }
}

/// Sube los cambios locales pendientes y baja los del remoto. Guarda el resultado en el vínculo.
pub async fn sync_user(st: &AppState, uid: i64) -> Result<SyncReport, SyncError> {
    let ctx = st.local.clone().ok_or_else(|| SyncError::Local("not in local mode".into()))?;
    let _guard = ctx.sync_lock.lock().await;
    let res = sync_inner(st, &ctx, uid).await;
    let (ok_ms, err) = match &res {
        Ok(r) => (Some(r.last_sync_at), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let signed_out = matches!(res, Err(SyncError::SignedOut));
    let _ = st
        .db
        .call(move |c| {
            c.execute(
                "UPDATE local_link SET last_sync_ms = COALESCE(?2, last_sync_ms), last_error = ?3,
                   remote_token = CASE WHEN ?4 THEN NULL ELSE remote_token END
                 WHERE user_id = ?1",
                params![uid, ok_ms, err, signed_out],
            )?;
            Ok(())
        })
        .await;
    res
}

fn remote_err(e: RemoteErr) -> SyncError {
    match e {
        RemoteErr::Rejected(StatusCode::UNAUTHORIZED, ..) => SyncError::SignedOut,
        RemoteErr::Rejected(StatusCode::NOT_FOUND, ..) => SyncError::Remote(RemoteErr::Rejected(
            StatusCode::NOT_FOUND,
            "the server does not support sync yet (update it)".into(),
            Some("sync_unsupported".into()),
        )),
        e => SyncError::Remote(e),
    }
}

async fn sync_inner(st: &AppState, ctx: &LocalCtx, uid: i64) -> Result<SyncReport, SyncError> {
    let link = load_link(st, uid).await?.ok_or(SyncError::NotLinked)?;
    let token = link.remote_token.clone().ok_or(SyncError::SignedOut)?;
    let url = link.remote_url.clone();
    let mut report = SyncReport::default();

    // 0. El nombre de la cuenta: si lo cambiaron en el server, el dispositivo lo toma (y el login
    // sin conexion acepta el nuevo).
    let me = remote_me(ctx, &url, &token).await.map_err(|e| {
        if matches!(e, RemoteErr::Unreachable(_)) {
            ctx.set_reachable(&url, false);
        }
        remote_err(e)
    })?;
    let name = me.username.clone();
    if st.db.call(move |c| follow_remote_name(c, uid, &name)).await? {
        st.auth.clear_cache();
        tracing::info!(user = uid, name = %me.username, "nombre tomado del server");
    }

    // 1. Subir.
    let mut since = link.pushed_seq;
    loop {
        let pull = st.db.call(move |c| Ok(sync::collect(c, uid, since, PUSH_BATCH)?)).await?;
        if !pull.changes.is_empty() {
            let res = remote_call(&ctx.http, reqwest::Method::POST, format!("{url}/api/sync"), Some(&token), Some(&pull.changes))
                .await
                .map_err(|e| {
                    if matches!(e, RemoteErr::Unreachable(_)) {
                        ctx.set_reachable(&url, false);
                    }
                    remote_err(e)
                })?;
            let _: PushResult = res.json().await.map_err(|e| SyncError::Remote(RemoteErr::Unreachable(format!("invalid sync response: {e}"))))?;
            report.pushed += pull.changes.len();
        }
        let cursor = pull.cursor;
        st.db
            .call(move |c| {
                c.execute("UPDATE local_link SET pushed_seq = MAX(pushed_seq, ?2) WHERE user_id = ?1", params![uid, cursor])?;
                Ok(())
            })
            .await?;
        since = cursor;
        if !pull.more {
            break;
        }
    }
    ctx.set_reachable(&url, true);

    // 2. Bajar. Si no hay nada local pendiente, lo bajado no se vuelve a subir (no rebota).
    let mut cursor = link.remote_cursor;
    loop {
        let res = remote_call(
            &ctx.http,
            reqwest::Method::GET,
            format!("{url}/api/sync?since={cursor}&limit={}", sync::PAGE),
            Some(&token),
            None::<&()>,
        )
        .await
        .map_err(remote_err)?;
        let pull: Pull = res.json().await.map_err(|e| SyncError::Remote(RemoteErr::Unreachable(format!("invalid sync response: {e}"))))?;
        let (changes, next, more) = (pull.changes, pull.cursor, pull.more);
        let applied = st
            .db
            .call(move |c| {
                let pushed: i64 = c.query_row("SELECT pushed_seq FROM local_link WHERE user_id = ?1", [uid], |r| r.get(0))?;
                let clean = sync::pending(c, uid, pushed)? == 0;
                let r = sync::apply(c, uid, &changes)?;
                let now_seq = current_seq(c)?;
                c.execute(
                    "UPDATE local_link SET remote_cursor = ?2, pushed_seq = CASE WHEN ?3 THEN ?4 ELSE pushed_seq END WHERE user_id = ?1",
                    params![uid, next, clean, now_seq],
                )?;
                Ok(r.applied)
            })
            .await?;
        report.pulled += applied;
        cursor = next;
        if !more {
            break;
        }
    }
    report.last_sync_at = now_ms();
    tracing::info!(user = uid, pushed = report.pushed, pulled = report.pulled, "sync ok");
    Ok(report)
}

/// Sincroniza todas las cuentas vinculadas cuyo remoto se alcance.
pub async fn sync_all(st: &AppState) {
    let Some(ctx) = st.local.clone() else { return };
    let links: Vec<(i64, String)> = st
        .db
        .call(|c| {
            let mut s = c.prepare("SELECT user_id, remote_url FROM local_link WHERE remote_token IS NOT NULL")?;
            let rows = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .unwrap_or_default();
    for (uid, url) in links {
        if !ctx.reachable(&url).await {
            continue;
        }
        if let Err(e) = sync_user(st, uid).await {
            tracing::warn!(user = uid, "sync failed: {e}");
        }
    }
}

/// Tarea de fondo: sincroniza cada `interval` y unos segundos después de cada cambio local.
pub async fn sync_loop(st: AppState, interval: Duration) {
    tokio::time::sleep(Duration::from_secs(2)).await;
    loop {
        sync_all(&st).await;
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = st.changes.notified() => {
                // Junta varios cambios seguidos (p. ej. progreso cada 10 s) en una sola subida.
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

// ---------- rutas de estado y sincronización ----------

#[derive(Deserialize, Default)]
struct SyncBody {
    remote_url: Option<String>,
    /// Token de una sesión del remoto (p. ej. de un login hecho directo contra el server). Se
    /// verifica que sea de la misma cuenta.
    token: Option<String>,
}

async fn sync_now(State(st): State<AppState>, u: AuthUser, body: axum::body::Bytes) -> ApiResult<Json<SyncReport>> {
    let ctx = st.local.clone().ok_or_else(|| ApiError::internal("local mode without context"))?;
    // El cuerpo es opcional (sin cuerpo = sincronizar con lo guardado).
    let b: SyncBody = if body.iter().all(u8::is_ascii_whitespace) {
        SyncBody::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| ApiError::bad_request(format!("Invalid JSON body: {e}")))?
    };
    if let Some(token) = b.token.filter(|t| !t.is_empty()) {
        let url = match b.remote_url.as_deref().filter(|u| !u.is_empty()) {
            Some(u) => normalize_url(u)?,
            None => load_link(&st, u.user.id)
                .await?
                .map(|l| l.remote_url)
                .or_else(|| ctx.default_remote.clone())
                .ok_or_else(|| ApiError::bad_request("remote_url is required"))?,
        };
        let remote = remote_me(&ctx, &url, &token).await.map_err(|e| match e {
            RemoteErr::Rejected(..) => ApiError::unauthorized("The server rejected that token").with_code("bad_token"),
            e => ApiError::bad_gateway(e.to_string()),
        })?;
        // La misma cuenta: por el vinculo (id del remoto) o, sin vinculo, por el nombre.
        let linked_id = load_link_remote_id(&st, u.user.id).await?;
        let same = match linked_id {
            Some(id) => id == remote.id,
            None => remote.username.eq_ignore_ascii_case(&u.user.username),
        };
        if !same {
            return Err(ApiError::conflict("That server session belongs to a different account").with_code("other_account"));
        }
        let server_id = ctx.remote_server_id(&url).await;
        save_link(&st, u.user.id, &url, &token, remote.id, server_id).await?;
    } else if b.remote_url.is_some() {
        return Err(ApiError::bad_request("token is required to change the server"));
    }
    match sync_user(&st, u.user.id).await {
        Ok(r) => Ok(Json(r)),
        Err(SyncError::NotLinked) => Err(ApiError::conflict(SyncError::NotLinked.to_string()).with_code("sync_not_linked")),
        Err(SyncError::SignedOut) => Err(ApiError::unauthorized(SyncError::SignedOut.to_string()).with_code("sync_signed_out")),
        Err(e) => {
            let msg = format!("Sync failed: {e}");
            Err(ApiError::bad_gateway(msg).with_code(e.code()).with_param("e", e.to_string()))
        }
    }
}

#[derive(Serialize)]
struct Status {
    mode: &'static str,
    username: String,
    /// Server remoto vinculado (o el de la config si todavía no hubo login online).
    remote_url: Option<String>,
    /// Hay una sesión del remoto guardada (si no, hace falta un login online para sincronizar).
    remote_signed_in: bool,
    /// `null` = no hay remoto configurado.
    remote_reachable: Option<bool>,
    /// ms unix de la última sincronización buena.
    last_sync_at: Option<i64>,
    last_sync_error: Option<String>,
    /// Código de `last_sync_error` (docs/API.md, "Errores").
    last_sync_error_code: Option<&'static str>,
    /// Cambios locales que todavía no subieron.
    pending_changes: i64,
    /// ffmpeg disponible (si no, solo reproducción directa).
    transcode_available: bool,
    sync_interval_s: Option<u64>,
    version: &'static str,
}

async fn status(State(st): State<AppState>, u: AuthUser) -> ApiResult<Json<Status>> {
    let ctx = st.local.clone().ok_or_else(|| ApiError::internal("local mode without context"))?;
    let link = load_link(&st, u.user.id).await?;
    let pushed = link.as_ref().map(|l| l.pushed_seq).unwrap_or(0);
    let uid = u.user.id;
    let pending = st.db.call(move |c| Ok(sync::pending(c, uid, pushed)?)).await?;
    let remote_url = link.as_ref().map(|l| l.remote_url.clone()).or_else(|| ctx.default_remote.clone());
    let remote_reachable = match &remote_url {
        Some(url) => Some(match ctx.known_reachable(url) {
            Some(ok) if ctx.reach.lock().unwrap().get(url).is_some_and(|(_, at)| at.elapsed() < REACH_TTL) => ok,
            _ => ctx.reachable(url).await,
        }),
        None => None,
    };
    Ok(Json(Status {
        mode: "local",
        username: u.user.username,
        remote_url,
        remote_signed_in: link.as_ref().is_some_and(|l| l.remote_token.is_some()),
        remote_reachable,
        last_sync_at: link.as_ref().and_then(|l| l.last_sync_ms),
        last_sync_error_code: link.as_ref().and_then(|l| l.last_error.as_deref()).map(stored_sync_code),
        last_sync_error: link.and_then(|l| l.last_error),
        pending_changes: pending,
        transcode_available: st.media.tools().ffmpeg,
        sync_interval_s: st.cfg.sync_interval.map(|d| d.as_secs()),
        version: env!("CARGO_PKG_VERSION"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(normalize_url("https://srv.ts.net:7700/").unwrap(), "https://srv.ts.net:7700");
        assert_eq!(normalize_url(" http://10.0.0.2:7700 ").unwrap(), "http://10.0.0.2:7700");
        assert_eq!(normalize_url("https://h/mythos/").unwrap(), "https://h/mythos");
        for bad in ["ftp://x", "nope", "https://user:pw@h", "file:///etc"] {
            assert!(normalize_url(bad).is_err(), "{bad}");
        }
    }
}
