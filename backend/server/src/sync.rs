//! Sincronización de datos de usuario entre servidores (el remoto de casa y el local embebido en
//! cada app): settings (`ui`, calidad preferida, tamaño máximo; nunca la clave de RD), progreso y
//! biblioteca.
//!
//! Reglas:
//! - **Last-write-wins por registro** con `updated_ms` (reloj del dispositivo que hizo el cambio).
//!   Empate exacto: gana el borrado y, si no, el contenido "mayor" (comparación del JSON
//!   canónico). Es un orden total, así todas las réplicas terminan iguales sin importar en qué
//!   orden reciban los cambios.
//! - **Lápidas** para borrar (`deleted: true`): un borrado viaja como cualquier cambio.
//! - **Idempotente**: aplicar dos veces el mismo cambio no hace nada la segunda vez (no toma seq
//!   nuevo, así no rebota).
//! - **Incremental**: cada escritura toma un `seq` creciente; `GET /api/sync?since=<cursor>`
//!   devuelve lo que cambió después de ese cursor y el cursor nuevo. El cursor es del servidor
//!   que responde (no tiene relación con el reloj), por eso no le afecta el desfase de relojes.
//!
//! El mismo código lo usan las rutas (`GET`/`POST /api/sync`, en cualquier modo) y el cliente de
//! sincronización del modo local (`local.rs`).
use axum::{
    extract::{DefaultBodyLimit, State},
    routing::get,
    Router,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    auth::AuthUser,
    cinemeta::{check_id, check_type, MetaPreview},
    db::{current_seq, next_seq},
    error::{ApiError, ApiResult, Json, Query},
    state::AppState,
    user_data::{check_progress, Progress},
};

/// Máximo de registros por tipo en una respuesta de `GET /api/sync`.
pub const PAGE: usize = 1000;
/// Máximo de registros en un `POST /api/sync`.
pub const MAX_PUSH: usize = 5000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProgressRec {
    pub meta_id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub series_id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub episode_label: Option<String>,
    #[serde(default)]
    pub position_s: f64,
    #[serde(default)]
    pub duration_s: f64,
    #[serde(default)]
    pub stream_key: Option<String>,
    pub updated_ms: i64,
    #[serde(default)]
    pub deleted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibraryRec {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    /// `MetaPreview`; `null` en las lápidas.
    #[serde(default)]
    pub item: Option<Value>,
    pub updated_ms: i64,
    #[serde(default)]
    pub deleted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingsRec {
    #[serde(default)]
    pub ui: Map<String, Value>,
    #[serde(default)]
    pub preferred_quality: Option<String>,
    #[serde(default)]
    pub max_size_gb: Option<f64>,
    pub updated_ms: i64,
}

/// Un lote de cambios (cuerpo de `POST /api/sync` y parte de la respuesta de `GET`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Changes {
    #[serde(default)]
    pub settings: Option<SettingsRec>,
    #[serde(default)]
    pub progress: Vec<ProgressRec>,
    #[serde(default)]
    pub library: Vec<LibraryRec>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.settings.is_none() && self.progress.is_empty() && self.library.is_empty()
    }
    pub fn len(&self) -> usize {
        self.settings.is_some() as usize + self.progress.len() + self.library.len()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pull {
    #[serde(flatten)]
    pub changes: Changes,
    /// Pasar como `since` la próxima vez.
    pub cursor: i64,
    /// Hay más cambios: pedir de nuevo con el cursor nuevo.
    pub more: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PushResult {
    /// Registros que cambiaron algo acá.
    pub applied: usize,
    /// Registros viejos o repetidos (ya estaba eso o algo más nuevo).
    pub ignored: usize,
    /// Registros inválidos (se saltean, no frenan el lote).
    pub rejected: usize,
}

// ---------- comparación ----------

/// ¿`incoming` le gana a `current`? Orden total: (updated_ms, deleted, JSON canónico).
fn wins<T: Serialize>(inc_ms: i64, inc_del: bool, inc: &T, cur_ms: i64, cur_del: bool, cur: &T) -> bool {
    use std::cmp::Ordering::*;
    match (inc_ms, inc_del).cmp(&(cur_ms, cur_del)) {
        Greater => true,
        Less => false,
        Equal => serde_json::to_string(inc).unwrap_or_default() > serde_json::to_string(cur).unwrap_or_default(),
    }
}

/// Las lápidas se comparan sin el contenido (que no importa).
fn tombstone_progress(p: &ProgressRec) -> ProgressRec {
    ProgressRec {
        meta_id: p.meta_id.clone(),
        kind: String::new(),
        series_id: None,
        name: String::new(),
        poster: None,
        background: None,
        episode_label: None,
        position_s: 0.0,
        duration_s: 0.0,
        stream_key: None,
        updated_ms: p.updated_ms,
        deleted: true,
    }
}

fn norm_progress(p: &ProgressRec) -> ProgressRec {
    if p.deleted {
        tombstone_progress(p)
    } else {
        p.clone()
    }
}

fn norm_library(l: &LibraryRec) -> LibraryRec {
    if l.deleted {
        LibraryRec { item: None, ..l.clone() }
    } else {
        l.clone()
    }
}

// ---------- lectura ----------

const PCOLS: &str = "meta_id, type, series_id, name, poster, background, episode_label, position_s, duration_s, stream_key, updated_ms, deleted, seq";

fn progress_rec(r: &rusqlite::Row) -> rusqlite::Result<(ProgressRec, i64)> {
    Ok((
        ProgressRec {
            meta_id: r.get(0)?,
            kind: r.get(1)?,
            series_id: r.get(2)?,
            name: r.get(3)?,
            poster: r.get(4)?,
            background: r.get(5)?,
            episode_label: r.get(6)?,
            position_s: r.get(7)?,
            duration_s: r.get(8)?,
            stream_key: r.get(9)?,
            updated_ms: r.get(10)?,
            deleted: r.get(11)?,
        },
        r.get(12)?,
    ))
}

fn library_rec(r: &rusqlite::Row) -> rusqlite::Result<(LibraryRec, i64)> {
    let deleted: bool = r.get(4)?;
    let data: String = r.get(2)?;
    Ok((
        LibraryRec {
            kind: r.get(0)?,
            id: r.get(1)?,
            item: if deleted { None } else { serde_json::from_str(&data).ok() },
            updated_ms: r.get(3)?,
            deleted,
        },
        r.get(5)?,
    ))
}

fn settings_rec(c: &Connection, uid: i64, since: Option<i64>) -> rusqlite::Result<Option<(SettingsRec, i64)>> {
    c.query_row(
        "SELECT ui, preferred_quality, max_size_gb, updated_ms, seq FROM settings WHERE user_id = ?1 AND seq > ?2",
        params![uid, since.unwrap_or(i64::MIN)],
        |r| {
            let ui: String = r.get(0)?;
            Ok((
                SettingsRec {
                    ui: serde_json::from_str(&ui).unwrap_or_default(),
                    preferred_quality: r.get(1)?,
                    max_size_gb: r.get(2)?,
                    updated_ms: r.get(3)?,
                },
                r.get(4)?,
            ))
        },
    )
    .optional()
}

/// Cambios del usuario con `seq > since`, como mucho `limit` por tipo (ordenados por seq). Si se
/// cortó alguno, el cursor queda en el último seq entregado de forma completa y `more = true`.
pub fn collect(c: &Connection, uid: i64, since: i64, limit: usize) -> rusqlite::Result<Pull> {
    let lim = limit.max(1) as i64;
    let mut progress: Vec<(ProgressRec, i64)> = c
        .prepare_cached(&format!("SELECT {PCOLS} FROM progress WHERE user_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3"))?
        .query_map(params![uid, since, lim + 1], progress_rec)?
        .collect::<Result<_, _>>()?;
    let mut library: Vec<(LibraryRec, i64)> = c
        .prepare_cached("SELECT type, id, data, updated_ms, deleted, seq FROM library WHERE user_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3")?
        .query_map(params![uid, since, lim + 1], library_rec)?
        .collect::<Result<_, _>>()?;
    let mut settings = settings_rec(c, uid, Some(since))?;
    // Cursor: el seq global actual, salvo que algún tipo se haya cortado.
    let mut cursor = current_seq(c)?;
    let mut more = false;
    if progress.len() as i64 > lim {
        cursor = cursor.min(progress[lim as usize - 1].1);
        more = true;
    }
    if library.len() as i64 > lim {
        cursor = cursor.min(library[lim as usize - 1].1);
        more = true;
    }
    progress.retain(|(_, s)| *s <= cursor);
    library.retain(|(_, s)| *s <= cursor);
    if settings.as_ref().is_some_and(|(_, s)| *s > cursor) {
        settings = None;
    }
    Ok(Pull {
        changes: Changes {
            settings: settings.map(|(s, _)| s),
            progress: progress.into_iter().map(|(p, _)| p).collect(),
            library: library.into_iter().map(|(l, _)| l).collect(),
        },
        cursor,
        more,
    })
}

/// Cuántos registros del usuario cambiaron después de `since` (pendientes de subir).
pub fn pending(c: &Connection, uid: i64, since: i64) -> rusqlite::Result<i64> {
    c.query_row(
        "SELECT (SELECT COUNT(*) FROM progress WHERE user_id = ?1 AND seq > ?2)
              + (SELECT COUNT(*) FROM library WHERE user_id = ?1 AND seq > ?2)
              + (SELECT COUNT(*) FROM settings WHERE user_id = ?1 AND seq > ?2)",
        params![uid, since],
        |r| r.get(0),
    )
}

// ---------- escritura ----------

fn validate_progress(p: &ProgressRec) -> ApiResult<()> {
    if p.deleted {
        return check_id(&p.meta_id);
    }
    check_progress(&Progress {
        meta_id: p.meta_id.clone(),
        kind: p.kind.clone(),
        series_id: p.series_id.clone(),
        name: p.name.clone(),
        poster: p.poster.clone(),
        background: p.background.clone(),
        episode_label: p.episode_label.clone(),
        position_s: p.position_s,
        duration_s: p.duration_s,
        stream_key: p.stream_key.clone(),
        updated_at: String::new(),
    })
}

/// `MetaPreview` validado y normalizado (id/tipo del registro), en JSON.
fn library_data(l: &LibraryRec) -> ApiResult<String> {
    check_type(&l.kind)?;
    check_id(&l.id)?;
    if l.deleted {
        return Ok("{}".into());
    }
    let item = l.item.clone().ok_or_else(|| ApiError::bad_request("library item is required"))?;
    let mut m: MetaPreview = serde_json::from_value(item).map_err(|e| ApiError::bad_request(format!("invalid library item: {e}")))?;
    m.id = l.id.clone();
    m.kind = l.kind.clone();
    let data = serde_json::to_string(&m).map_err(ApiError::internal)?;
    if data.len() > 16 * 1024 {
        return Err(ApiError::bad_request("Library item is too large"));
    }
    Ok(data)
}

fn validate_settings(s: &SettingsRec) -> ApiResult<()> {
    if serde_json::to_string(&s.ui).map(|j| j.len()).unwrap_or(usize::MAX) > 64 * 1024 {
        return Err(ApiError::bad_request("ui settings are too large (max 64 KB)"));
    }
    if s.preferred_quality.as_deref().is_some_and(|q| !matches!(q, "2160p" | "1080p" | "720p")) {
        return Err(ApiError::bad_request("invalid preferred_quality"));
    }
    if s.max_size_gb.is_some_and(|m| !(m > 0.0 && m < 10_000.0)) {
        return Err(ApiError::bad_request("invalid max_size_gb"));
    }
    Ok(())
}

/// Aplica un lote con last-write-wins, en una transacción. Cada registro que cambia algo toma un
/// seq nuevo (así sigue viajando a otros dispositivos). Los inválidos se cuentan y se saltean.
pub fn apply(c: &mut Connection, uid: i64, ch: &Changes) -> rusqlite::Result<PushResult> {
    let tx = c.transaction()?;
    let mut r = PushResult::default();

    if let Some(s) = &ch.settings {
        if validate_settings(s).is_err() {
            r.rejected += 1;
        } else {
            let cur = settings_rec(&tx, uid, None)?.map(|(s, _)| s);
            let take = match &cur {
                None => true,
                Some(c) => wins(s.updated_ms, false, s, c.updated_ms, false, c),
            };
            if take {
                let seq = next_seq(&tx)?;
                let ui = serde_json::to_string(&s.ui).unwrap_or_else(|_| "{}".into());
                // La clave de RD de este dispositivo no se toca.
                tx.execute(
                    "INSERT INTO settings (user_id, ui, preferred_quality, max_size_gb, updated_ms, seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(user_id) DO UPDATE SET ui = ?2, preferred_quality = ?3, max_size_gb = ?4, updated_ms = ?5, seq = ?6",
                    params![uid, ui, s.preferred_quality, s.max_size_gb, s.updated_ms, seq],
                )?;
                r.applied += 1;
            } else {
                r.ignored += 1;
            }
        }
    }

    for p in &ch.progress {
        if validate_progress(p).is_err() {
            r.rejected += 1;
            continue;
        }
        let cur = tx
            .query_row(&format!("SELECT {PCOLS} FROM progress WHERE user_id = ?1 AND meta_id = ?2"), params![uid, p.meta_id], progress_rec)
            .optional()?
            .map(|(p, _)| p);
        let inc = norm_progress(p);
        let take = match &cur {
            None => true,
            Some(c) => {
                let c = norm_progress(c);
                wins(inc.updated_ms, inc.deleted, &inc, c.updated_ms, c.deleted, &c)
            }
        };
        if !take {
            r.ignored += 1;
            continue;
        }
        let seq = next_seq(&tx)?;
        if inc.deleted {
            match cur {
                // Lápida de algo que acá nunca existió: se guarda igual (por si llega tarde la
                // versión vieja desde otro lado).
                None => tx.execute(
                    "INSERT INTO progress (user_id, meta_id, type, name, position_s, duration_s, updated_ms, updated_at, deleted, seq)
                     VALUES (?1, ?2, 'movie', '', 0, 0, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', ?3 / 1000.0, 'unixepoch'), 1, ?4)",
                    params![uid, inc.meta_id, inc.updated_ms, seq],
                )?,
                Some(_) => tx.execute(
                    "UPDATE progress SET deleted = 1, updated_ms = ?3, seq = ?4,
                       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', ?3 / 1000.0, 'unixepoch')
                     WHERE user_id = ?1 AND meta_id = ?2",
                    params![uid, inc.meta_id, inc.updated_ms, seq],
                )?,
            };
        } else {
            tx.execute(
                "INSERT INTO progress (user_id, meta_id, type, series_id, name, poster, background, episode_label,
                                       position_s, duration_s, stream_key, updated_ms, updated_at, deleted, seq)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                         strftime('%Y-%m-%dT%H:%M:%fZ', ?12 / 1000.0, 'unixepoch'), 0, ?13)
                 ON CONFLICT(user_id, meta_id) DO UPDATE SET type = ?3, series_id = ?4, name = ?5, poster = ?6,
                   background = ?7, episode_label = ?8, position_s = ?9, duration_s = ?10, stream_key = ?11,
                   updated_ms = ?12, updated_at = excluded.updated_at, deleted = 0, seq = ?13",
                params![
                    uid, inc.meta_id, inc.kind, inc.series_id, inc.name, inc.poster, inc.background, inc.episode_label,
                    inc.position_s, inc.duration_s, inc.stream_key, inc.updated_ms, seq
                ],
            )?;
        }
        r.applied += 1;
    }

    for l in &ch.library {
        let Ok(data) = library_data(l) else {
            r.rejected += 1;
            continue;
        };
        let cur = tx
            .query_row(
                "SELECT type, id, data, updated_ms, deleted, seq FROM library WHERE user_id = ?1 AND type = ?2 AND id = ?3",
                params![uid, l.kind, l.id],
                library_rec,
            )
            .optional()?
            .map(|(l, _)| l);
        // Se compara con el ítem ya normalizado (como quedaría guardado).
        let mut inc = norm_library(l);
        if !inc.deleted {
            inc.item = serde_json::from_str(&data).ok();
        }
        let take = match &cur {
            None => true,
            Some(c) => {
                let c = norm_library(c);
                wins(inc.updated_ms, inc.deleted, &inc, c.updated_ms, c.deleted, &c)
            }
        };
        if !take {
            r.ignored += 1;
            continue;
        }
        let seq = next_seq(&tx)?;
        tx.execute(
            "INSERT INTO library (user_id, type, id, data, added_at, updated_ms, deleted, seq)
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', ?5 / 1000.0, 'unixepoch'), ?5, ?6, ?7)
             ON CONFLICT(user_id, type, id) DO UPDATE SET
               data = CASE WHEN ?6 = 1 THEN library.data ELSE ?4 END,
               added_at = CASE WHEN library.deleted = 1 AND ?6 = 0 THEN excluded.added_at ELSE library.added_at END,
               updated_ms = ?5, deleted = ?6, seq = ?7",
            params![uid, inc.kind, inc.id, data, inc.updated_ms, inc.deleted, seq],
        )?;
        r.applied += 1;
    }
    tx.commit()?;
    Ok(r)
}

// ---------- rutas ----------

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sync", get(pull).post(push))
        // Un lote de 5000 registros no entra en el límite general de 256 KB.
        .layer(DefaultBodyLimit::max(8 << 20))
}

#[derive(Deserialize)]
struct PullQuery {
    since: Option<i64>,
    limit: Option<usize>,
}

/// `GET /api/sync?since=<cursor>&limit=`: cambios del usuario después del cursor (sin `since`,
/// todo). Incluye lápidas.
async fn pull(State(st): State<AppState>, u: AuthUser, Query(q): Query<PullQuery>) -> ApiResult<Json<Pull>> {
    let (uid, since, limit) = (u.user.id, q.since.unwrap_or(0), q.limit.unwrap_or(PAGE).clamp(1, PAGE));
    Ok(Json(st.db.call(move |c| Ok(collect(c, uid, since, limit)?)).await?))
}

/// `POST /api/sync` con `Changes`: aplica con last-write-wins. Idempotente.
async fn push(State(st): State<AppState>, u: AuthUser, Json(ch): Json<Changes>) -> ApiResult<Json<PushResult>> {
    if ch.len() > MAX_PUSH {
        return Err(ApiError::bad_request(format!("Too many changes in one batch (max {MAX_PUSH})")));
    }
    let uid = u.user.id;
    let r = st.db.call(move |c| Ok(apply(c, uid, &ch)?)).await?;
    if r.applied > 0 {
        st.changed();
    }
    Ok(Json(r))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn db_with_user() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn.lock().unwrap().execute("INSERT INTO users (id, username, password_hash) VALUES (1, 'ana', 'x')", []).unwrap();
        db
    }

    fn prog(id: &str, pos: f64, ms: i64) -> ProgressRec {
        ProgressRec {
            meta_id: id.into(),
            kind: "movie".into(),
            series_id: None,
            name: "Inception".into(),
            poster: None,
            background: None,
            episode_label: None,
            position_s: pos,
            duration_s: 8880.0,
            stream_key: None,
            updated_ms: ms,
            deleted: false,
        }
    }

    fn lib(id: &str, ms: i64, deleted: bool) -> LibraryRec {
        LibraryRec {
            kind: "movie".into(),
            id: id.into(),
            item: (!deleted).then(|| serde_json::json!({"id": id, "type": "movie", "name": "Inception"})),
            updated_ms: ms,
            deleted,
        }
    }

    fn push(db: &Db, ch: Changes) -> PushResult {
        apply(&mut db.conn.lock().unwrap(), 1, &ch).unwrap()
    }

    fn pull(db: &Db, since: i64) -> Pull {
        collect(&db.conn.lock().unwrap(), 1, since, PAGE).unwrap()
    }

    #[test]
    fn last_write_wins_and_idempotent() {
        let db = db_with_user();
        let ch = Changes { progress: vec![prog("tt1375666", 100.0, 1000)], ..Default::default() };
        assert_eq!(push(&db, ch.clone()).applied, 1);
        // Repetido: no cambia nada (ni toma seq nuevo).
        let seq0 = current_seq(&db.conn.lock().unwrap()).unwrap();
        assert_eq!(push(&db, ch.clone()), PushResult { applied: 0, ignored: 1, rejected: 0 });
        assert_eq!(current_seq(&db.conn.lock().unwrap()).unwrap(), seq0);
        // Más viejo: pierde.
        assert_eq!(push(&db, Changes { progress: vec![prog("tt1375666", 50.0, 999)], ..Default::default() }).ignored, 1);
        // Más nuevo: gana.
        assert_eq!(push(&db, Changes { progress: vec![prog("tt1375666", 200.0, 1001)], ..Default::default() }).applied, 1);
        let p = pull(&db, 0);
        assert_eq!(p.changes.progress.len(), 1);
        assert_eq!(p.changes.progress[0].position_s, 200.0);
        assert!(!p.more);
    }

    #[test]
    fn ties_converge_regardless_of_order() {
        let a = prog("tt1", 10.0, 5000);
        let b = prog("tt1", 20.0, 5000);
        let (d1, d2) = (db_with_user(), db_with_user());
        push(&d1, Changes { progress: vec![a.clone()], ..Default::default() });
        push(&d1, Changes { progress: vec![b.clone()], ..Default::default() });
        push(&d2, Changes { progress: vec![b], ..Default::default() });
        push(&d2, Changes { progress: vec![a], ..Default::default() });
        assert_eq!(pull(&d1, 0).changes.progress, pull(&d2, 0).changes.progress);
    }

    #[test]
    fn tombstones() {
        let db = db_with_user();
        push(&db, Changes { library: vec![lib("tt1", 1000, false)], progress: vec![prog("tt2", 5.0, 1000)], ..Default::default() });
        // Borrado más nuevo: gana y viaja como lápida.
        let mut del = prog("tt2", 0.0, 2000);
        del.deleted = true;
        let r = push(&db, Changes { library: vec![lib("tt1", 2000, true)], progress: vec![del], ..Default::default() });
        assert_eq!(r.applied, 2);
        let p = pull(&db, 0);
        assert!(p.changes.library[0].deleted && p.changes.library[0].item.is_none());
        assert!(p.changes.progress[0].deleted);
        // La versión vieja que llega tarde no resucita nada.
        let r = push(&db, Changes { library: vec![lib("tt1", 1500, false)], progress: vec![prog("tt2", 9.0, 1500)], ..Default::default() });
        assert_eq!(r.ignored, 2);
        let c = db.conn.lock().unwrap();
        let live: i64 = c.query_row("SELECT COUNT(*) FROM library WHERE deleted = 0", [], |r| r.get(0)).unwrap();
        assert_eq!(live, 0);
        drop(c);
        // Volver a agregar después del borrado sí gana.
        assert_eq!(push(&db, Changes { library: vec![lib("tt1", 3000, false)], ..Default::default() }).applied, 1);
        // Lápida de algo que nunca existió acá: se guarda.
        let mut ghost = prog("tt9", 0.0, 10);
        ghost.deleted = true;
        assert_eq!(push(&db, Changes { progress: vec![ghost], ..Default::default() }).applied, 1);
        assert_eq!(push(&db, Changes { progress: vec![prog("tt9", 1.0, 5)], ..Default::default() }).ignored, 1);
    }

    #[test]
    fn since_cursor_and_paging() {
        let db = db_with_user();
        let many: Vec<ProgressRec> = (0..25).map(|i| prog(&format!("tt{i}"), i as f64, 1000 + i)).collect();
        push(&db, Changes { progress: many, ..Default::default() });
        // Páginas de 10.
        let mut since = 0;
        let mut got = 0;
        let mut rounds = 0;
        loop {
            let p = collect(&db.conn.lock().unwrap(), 1, since, 10).unwrap();
            got += p.changes.progress.len();
            since = p.cursor;
            rounds += 1;
            if !p.more {
                break;
            }
        }
        assert_eq!((got, rounds), (25, 3));
        // Nada nuevo.
        assert!(pull(&db, since).changes.is_empty());
        // Un cambio después del cursor aparece solo.
        push(&db, Changes { progress: vec![prog("tt3", 99.0, 9999)], ..Default::default() });
        let p = pull(&db, since);
        assert_eq!(p.changes.progress.len(), 1);
        assert_eq!(p.changes.progress[0].meta_id, "tt3");
        assert_eq!(pending(&db.conn.lock().unwrap(), 1, since).unwrap(), 1);
    }

    #[test]
    fn settings_keep_local_rd_key_and_validate() {
        let db = db_with_user();
        db.conn.lock().unwrap().execute("INSERT INTO settings (user_id, rd_key, updated_ms) VALUES (1, 'LOCALKEY', 10)", []).unwrap();
        let mut ui = Map::new();
        ui.insert("theme".into(), "dark".into());
        let s = SettingsRec { ui, preferred_quality: Some("720p".into()), max_size_gb: Some(4.0), updated_ms: 20 };
        assert_eq!(push(&db, Changes { settings: Some(s.clone()), ..Default::default() }).applied, 1);
        let key: String = db.conn.lock().unwrap().query_row("SELECT rd_key FROM settings WHERE user_id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(key, "LOCALKEY");
        let out = pull(&db, 0).changes.settings.unwrap();
        assert_eq!(out, s);
        // La clave nunca sale en el sync.
        assert!(!serde_json::to_string(&pull(&db, 0)).unwrap().contains("LOCALKEY"));
        let bad = SettingsRec { preferred_quality: Some("4k".into()), updated_ms: 99, ..s };
        assert_eq!(push(&db, Changes { settings: Some(bad), ..Default::default() }).rejected, 1);
    }

    #[test]
    fn invalid_records_are_skipped() {
        let db = db_with_user();
        let mut bad = prog("not-an-id!", 1.0, 1);
        bad.name = String::new();
        let r = push(&db, Changes { progress: vec![bad, prog("tt1", 1.0, 1)], library: vec![LibraryRec { item: None, ..lib("tt5", 1, false) }], ..Default::default() });
        assert_eq!(r, PushResult { applied: 1, ignored: 0, rejected: 2 });
    }
}
