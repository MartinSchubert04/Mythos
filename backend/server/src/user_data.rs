//! Datos por usuario: settings (`/api/me/settings`), progreso (`/api/progress*`) y biblioteca
//! (`/api/library*`).
use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, put},
    Router,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::{
    auth::AuthUser,
    cinemeta::{check_id, check_type, MetaPreview},
    db::{next_seq, now_ms, Db},
    error::{ApiError, ApiResult, Json, Path, Query},
    state::AppState,
};

// Toda escritura de progreso, biblioteca o settings toma un `seq` nuevo (cursor de la
// sincronización) y un `updated_ms` = ahora, pero nunca menor al que ya tenía + 1: así un cambio
// hecho acá siempre le gana (last-write-wins) a lo que había, aunque eso viniera de un
// dispositivo con el reloj adelantado. Los borrados dejan una lápida (`deleted = 1`) para que la
// sincronización los propague. Ver `sync.rs`.

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/me/settings", get(get_settings).put(put_settings))
        .route("/progress", get(series_progress).put(put_progress))
        .route("/progress/continue", get(continue_watching))
        .route("/progress/recent", get(recent))
        .route("/progress/{meta_id}", get(get_progress).delete(delete_progress))
        .route("/library", get(library))
        .route("/library/{type}/{id}", put(library_add).delete(library_remove))
}

// ---------- settings ----------

/// Lo que el server necesita de los settings (incluye la clave de RD, que nunca sale).
#[derive(Default)]
pub struct UserPrefs {
    pub ui: Map<String, Value>,
    pub preferred_quality: Option<String>,
    pub max_size_gb: Option<f64>,
    pub rd_key: Option<String>,
}

pub async fn load_prefs(db: &Db, uid: i64) -> ApiResult<UserPrefs> {
    db.call(move |c| {
        let row = c
            .query_row(
                "SELECT ui, preferred_quality, max_size_gb, rd_key FROM settings WHERE user_id = ?1",
                [uid],
                |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get::<_, Option<String>>(3)?)),
            )
            .optional()?;
        Ok(match row {
            Some((ui, q, max, rd)) => UserPrefs {
                ui: serde_json::from_str(&ui).unwrap_or_default(),
                preferred_quality: q,
                max_size_gb: max,
                rd_key: rd.filter(|k| !k.is_empty()),
            },
            None => UserPrefs::default(),
        })
    })
    .await
}

#[derive(Serialize)]
struct SettingsOut {
    ui: Map<String, Value>,
    has_rd_key: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    preferred_quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_size_gb: Option<f64>,
}

impl From<UserPrefs> for SettingsOut {
    fn from(p: UserPrefs) -> Self {
        Self { ui: p.ui, has_rd_key: p.rd_key.is_some(), preferred_quality: p.preferred_quality, max_size_gb: p.max_size_gb }
    }
}

/// Distingue "campo ausente" (`None`) de "campo en null" (`Some(None)`).
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
struct SettingsIn {
    ui: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "nullable")]
    preferred_quality: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    max_size_gb: Option<Option<f64>>,
    rd_key: Option<String>,
}

async fn get_settings(State(st): State<AppState>, u: AuthUser) -> ApiResult<Json<SettingsOut>> {
    Ok(Json(load_prefs(&st.db, u.user.id).await?.into()))
}

/// Actualización parcial: `ui` se mezcla por clave (una clave en `null` se borra);
/// `preferred_quality`/`max_size_gb` en `null` se borran; `rd_key` vacío borra la clave.
async fn put_settings(State(st): State<AppState>, u: AuthUser, Json(b): Json<SettingsIn>) -> ApiResult<Json<SettingsOut>> {
    let mut p = load_prefs(&st.db, u.user.id).await?;
    let ui_changed = b.ui.as_ref().is_some_and(|m| !m.is_empty());
    let q_changed = b.preferred_quality.is_some();
    let max_changed = b.max_size_gb.is_some();
    if let Some(ui) = b.ui {
        for (k, v) in ui {
            if v.is_null() {
                p.ui.remove(&k);
            } else {
                p.ui.insert(k, v);
            }
        }
    }
    let ui_json = serde_json::to_string(&p.ui).map_err(ApiError::internal)?;
    if ui_json.len() > 64 * 1024 {
        return Err(ApiError::bad_request("ui settings are too large (max 64 KB)"));
    }
    if let Some(q) = b.preferred_quality {
        if let Some(q) = &q {
            if !matches!(q.as_str(), "2160p" | "1080p" | "720p") {
                return Err(ApiError::bad_request("preferred_quality must be '2160p', '1080p' or '720p'"));
            }
        }
        p.preferred_quality = q;
    }
    if let Some(m) = b.max_size_gb {
        if m.is_some_and(|m| !(m > 0.0 && m < 10_000.0)) {
            return Err(ApiError::bad_request("max_size_gb must be a positive number"));
        }
        p.max_size_gb = m;
    }
    if let Some(k) = b.rd_key {
        let k = k.trim().to_string();
        if k.len() > 200 || !k.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(ApiError::bad_request("Invalid Real-Debrid API key"));
        }
        p.rd_key = (!k.is_empty()).then_some(k);
    }
    let (uid, q, max, rd) = (u.user.id, p.preferred_quality.clone(), p.max_size_gb, p.rd_key.clone());
    // Solo la clave de RD no se sincroniza: si fue lo único que cambió, no hace falta un seq nuevo.
    let synced_change = ui_changed || q_changed || max_changed;
    st.db
        .call(move |c| {
            let tx = c.transaction()?;
            if synced_change {
                let seq = next_seq(&tx)?;
                tx.execute(
                    "INSERT INTO settings (user_id, ui, preferred_quality, max_size_gb, rd_key, updated_ms, seq)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(user_id) DO UPDATE SET ui = ?2, preferred_quality = ?3, max_size_gb = ?4, rd_key = ?5,
                       updated_ms = MAX(?6, settings.updated_ms + 1), seq = ?7",
                    params![uid, ui_json, q, max, rd, now_ms(), seq],
                )?;
            } else {
                tx.execute(
                    "INSERT INTO settings (user_id, ui, preferred_quality, max_size_gb, rd_key) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(user_id) DO UPDATE SET rd_key = ?5",
                    params![uid, ui_json, q, max, rd],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .await?;
    if synced_change {
        st.changed();
    }
    Ok(Json(p.into()))
}

// ---------- progreso ----------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub meta_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_label: Option<String>,
    pub position_s: f64,
    pub duration_s: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_key: Option<String>,
    #[serde(default, skip_deserializing)]
    pub updated_at: String,
}

pub const PROGRESS_COLS: &str =
    "meta_id, type, series_id, name, poster, background, episode_label, position_s, duration_s, stream_key, updated_at";

pub fn progress_from_row(r: &rusqlite::Row) -> rusqlite::Result<Progress> {
    Ok(Progress {
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
        updated_at: r.get(10)?,
    })
}

/// Validación común (API y sincronización).
pub fn check_progress(p: &Progress) -> ApiResult<()> {
    check_type(&p.kind)?;
    check_id(&p.meta_id)?;
    if let Some(s) = &p.series_id {
        check_id(s)?;
    }
    if !(p.position_s.is_finite() && p.duration_s.is_finite() && p.position_s >= 0.0 && p.duration_s >= 0.0) {
        return Err(ApiError::bad_request("position_s and duration_s must be non-negative numbers"));
    }
    if p.name.trim().is_empty() || p.name.len() > 500 {
        return Err(ApiError::bad_request("name is required"));
    }
    let long = [&p.poster, &p.background, &p.episode_label, &p.stream_key].iter().any(|f| f.as_ref().is_some_and(|s| s.len() > 2000));
    if long {
        return Err(ApiError::bad_request("Progress field is too long"));
    }
    Ok(())
}

async fn put_progress(State(st): State<AppState>, u: AuthUser, Json(p): Json<Progress>) -> ApiResult<StatusCode> {
    check_progress(&p)?;
    let uid = u.user.id;
    st.db
        .call(move |c| {
            let tx = c.transaction()?;
            let seq = next_seq(&tx)?;
            tx.execute(
                "INSERT INTO progress (user_id, meta_id, type, series_id, name, poster, background, episode_label,
                                       position_s, duration_s, stream_key, updated_ms, updated_at, deleted, seq)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                         strftime('%Y-%m-%dT%H:%M:%fZ', ?12 / 1000.0, 'unixepoch'), 0, ?13)
                 ON CONFLICT(user_id, meta_id) DO UPDATE SET type = ?3, series_id = ?4, name = ?5, poster = ?6,
                   background = ?7, episode_label = ?8, position_s = ?9, duration_s = ?10, stream_key = ?11,
                   updated_ms = MAX(?12, progress.updated_ms + 1),
                   updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', MAX(?12, progress.updated_ms + 1) / 1000.0, 'unixepoch'),
                   deleted = 0, seq = ?13",
                params![
                    uid, p.meta_id, p.kind, p.series_id, p.name, p.poster, p.background, p.episode_label,
                    p.position_s, p.duration_s, p.stream_key, now_ms(), seq
                ],
            )?;
            // Cuánto se vio y si se terminó la reproducción en curso (taste.rs).
            crate::taste::progress(&tx, uid, &p.meta_id, p.position_s, p.duration_s)?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    st.changed();
    Ok(StatusCode::NO_CONTENT)
}

/// "Seguir viendo": los últimos 20, sin los terminados (> 92 %) y uno solo por serie (el más
/// reciente).
async fn continue_watching(State(st): State<AppState>, u: AuthUser) -> ApiResult<Json<Vec<Progress>>> {
    let uid = u.user.id;
    let rows = st
        .db
        .call(move |c| {
            let mut stmt = c.prepare_cached(&format!(
                "SELECT {PROGRESS_COLS} FROM progress
                 WHERE user_id = ?1 AND deleted = 0 AND (duration_s <= 0 OR position_s / duration_s <= 0.92)
                 ORDER BY updated_at DESC LIMIT 200"
            ))?;
            let rows = stmt.query_map([uid], progress_from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    let mut seen = std::collections::HashSet::new();
    let out = rows
        .into_iter()
        .filter(|p| seen.insert(p.series_id.clone().unwrap_or_else(|| p.meta_id.clone())))
        .take(20)
        .collect();
    Ok(Json(out))
}

/// "Visto hace poco" (la Library): los últimos 30, terminados incluidos, uno solo por serie (el
/// episodio más reciente).
async fn recent(State(st): State<AppState>, u: AuthUser) -> ApiResult<Json<Vec<Progress>>> {
    let uid = u.user.id;
    let rows = st
        .db
        .call(move |c| {
            let mut stmt = c.prepare_cached(&format!(
                "SELECT {PROGRESS_COLS} FROM progress WHERE user_id = ?1 AND deleted = 0 ORDER BY updated_at DESC LIMIT 400"
            ))?;
            let rows = stmt.query_map([uid], progress_from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    let mut seen = std::collections::HashSet::new();
    let out = rows
        .into_iter()
        .filter(|p| seen.insert(p.series_id.clone().unwrap_or_else(|| p.meta_id.clone())))
        .take(30)
        .collect();
    Ok(Json(out))
}

#[derive(Deserialize)]
struct SeriesQuery {
    series_id: Option<String>,
}

/// Todos los episodios con progreso de una serie (también los terminados, para marcarlos como
/// vistos), del más reciente al más viejo.
async fn series_progress(State(st): State<AppState>, u: AuthUser, Query(q): Query<SeriesQuery>) -> ApiResult<Json<Vec<Progress>>> {
    let series_id = q.series_id.filter(|s| !s.is_empty()).ok_or_else(|| ApiError::bad_request("series_id is required"))?;
    check_id(&series_id)?;
    let uid = u.user.id;
    let rows = st
        .db
        .call(move |c| {
            let mut stmt = c.prepare_cached(&format!(
                "SELECT {PROGRESS_COLS} FROM progress WHERE user_id = ?1 AND series_id = ?2 AND deleted = 0 ORDER BY updated_at DESC"
            ))?;
            let rows = stmt.query_map(params![uid, series_id], progress_from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(Json(rows))
}

async fn get_progress(State(st): State<AppState>, u: AuthUser, Path(meta_id): Path<String>) -> ApiResult<Json<Progress>> {
    let uid = u.user.id;
    st.db
        .call(move |c| {
            Ok(c.query_row(
                &format!("SELECT {PROGRESS_COLS} FROM progress WHERE user_id = ?1 AND meta_id = ?2 AND deleted = 0"),
                params![uid, meta_id],
                progress_from_row,
            )
            .optional()?)
        })
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("No progress for this title"))
}

/// Deja una lápida (para que el borrado se sincronice).
async fn delete_progress(State(st): State<AppState>, u: AuthUser, Path(meta_id): Path<String>) -> ApiResult<StatusCode> {
    let uid = u.user.id;
    let n = st
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let seq = next_seq(&tx)?;
            let n = tx.execute(
                "UPDATE progress SET deleted = 1, seq = ?3, updated_ms = MAX(?4, updated_ms + 1),
                   updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', MAX(?4, updated_ms + 1) / 1000.0, 'unixepoch')
                 WHERE user_id = ?1 AND meta_id = ?2 AND deleted = 0",
                params![uid, meta_id, seq, now_ms()],
            )?;
            tx.commit()?;
            Ok(n)
        })
        .await?;
    if n > 0 {
        st.changed();
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------- biblioteca ----------

async fn library(State(st): State<AppState>, u: AuthUser) -> ApiResult<Json<Vec<MetaPreview>>> {
    let uid = u.user.id;
    let items = st
        .db
        .call(move |c| {
            let mut stmt = c.prepare_cached("SELECT data FROM library WHERE user_id = ?1 AND deleted = 0 ORDER BY added_at DESC")?;
            let rows = stmt.query_map([uid], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows.iter().filter_map(|d| serde_json::from_str(d).ok()).collect())
        })
        .await?;
    Ok(Json(items))
}

async fn library_add(
    State(st): State<AppState>,
    u: AuthUser,
    Path((kind, id)): Path<(String, String)>,
    Json(mut m): Json<MetaPreview>,
) -> ApiResult<StatusCode> {
    check_type(&kind)?;
    check_id(&id)?;
    m.id = id.clone();
    m.kind = kind.clone();
    let data = serde_json::to_string(&m).map_err(ApiError::internal)?;
    if data.len() > 16 * 1024 {
        return Err(ApiError::bad_request("Library item is too large"));
    }
    let uid = u.user.id;
    st.db
        .call(move |c| {
            let tx = c.transaction()?;
            let seq = next_seq(&tx)?;
            // Volver a agregar algo borrado lo pone primero (added_at nuevo).
            tx.execute(
                "INSERT INTO library (user_id, type, id, data, updated_ms, deleted, seq) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)
                 ON CONFLICT(user_id, type, id) DO UPDATE SET data = ?4,
                   added_at = CASE WHEN library.deleted = 1 THEN excluded.added_at ELSE library.added_at END,
                   updated_ms = MAX(?5, library.updated_ms + 1), deleted = 0, seq = ?6",
                params![uid, kind, id, data, now_ms(), seq],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    st.changed();
    Ok(StatusCode::NO_CONTENT)
}

/// Deja una lápida (para que el borrado se sincronice).
async fn library_remove(State(st): State<AppState>, u: AuthUser, Path((kind, id)): Path<(String, String)>) -> ApiResult<StatusCode> {
    let uid = u.user.id;
    let n = st
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let seq = next_seq(&tx)?;
            let n = tx.execute(
                "UPDATE library SET deleted = 1, seq = ?4, updated_ms = MAX(?5, updated_ms + 1)
                 WHERE user_id = ?1 AND type = ?2 AND id = ?3 AND deleted = 0",
                params![uid, kind, id, seq, now_ms()],
            )?;
            tx.commit()?;
            Ok(n)
        })
        .await?;
    if n > 0 {
        st.changed();
    }
    Ok(StatusCode::NO_CONTENT)
}

