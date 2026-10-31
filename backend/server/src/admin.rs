//! Rutas de admin: usuarios (crear, editar, deshabilitar, borrar), torrents en caché y stats.
use axum::{
    extract::State,
    http::StatusCode,
    routing::{delete, get, patch},
    Router,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{create_user, hash_password, user_from_row, validate_password, AdminUser, ClientIp, User, USER_COLS},
    db::now_unix,
    error::{ApiError, ApiResult, Json, Path, Query},
    state::{AppState, Watching},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/users", get(list_users).post(add_user))
        .route("/admin/users/{id}", patch(edit_user).delete(remove_user))
        .route("/admin/torrents", get(torrents))
        .route("/admin/torrents/{info_hash}", delete(remove_torrent))
        .route("/admin/stats", get(stats))
}

async fn list_users(State(st): State<AppState>, _a: AdminUser) -> ApiResult<Json<Vec<User>>> {
    let users = st
        .db
        .call(|c| {
            let mut stmt = c.prepare_cached(&format!("SELECT {USER_COLS} FROM users ORDER BY id"))?;
            let rows = stmt.query_map([], user_from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(Json(users))
}

#[derive(Deserialize)]
struct NewUser {
    username: String,
    password: String,
    #[serde(default)]
    is_admin: bool,
}

async fn add_user(State(st): State<AppState>, ClientIp(ip): ClientIp, a: AdminUser, Json(b): Json<NewUser>) -> ApiResult<Json<User>> {
    let user = st.db.call(move |c| create_user(c, &b.username, &b.password, b.is_admin)).await?;
    tracing::info!(by = %a.0.user.username, user = %user.username, admin = user.is_admin, "usuario creado");
    st.recorder.audit(
        "user_created",
        Some(a.0.user.id),
        Some(&a.0.user.username),
        Some(ip),
        Some(format!("created {} (admin: {})", user.username, user.is_admin)),
    );
    Ok(Json(user))
}

#[derive(Deserialize)]
struct EditUser {
    password: Option<String>,
    is_admin: Option<bool>,
    disabled: Option<bool>,
}

fn get_user(c: &Connection, id: i64) -> ApiResult<User> {
    c.query_row(&format!("SELECT {USER_COLS} FROM users WHERE id = ?1"), [id], user_from_row)
        .optional()?
        .ok_or_else(|| ApiError::not_found("User not found"))
}

/// Admins habilitados que quedarían si `id` dejara de serlo.
fn other_active_admins(c: &Connection, id: i64) -> ApiResult<i64> {
    Ok(c.query_row("SELECT COUNT(*) FROM users WHERE is_admin = 1 AND disabled = 0 AND id <> ?1", [id], |r| r.get(0))?)
}

async fn edit_user(
    State(st): State<AppState>,
    ClientIp(ip): ClientIp,
    a: AdminUser,
    Path(id): Path<i64>,
    Json(b): Json<EditUser>,
) -> ApiResult<Json<User>> {
    // Para la auditoría: qué se cambió (nunca la contraseña en sí).
    let mut what = Vec::new();
    if b.password.is_some() {
        what.push("password".to_string());
    }
    if let Some(v) = b.is_admin {
        what.push(format!("is_admin={v}"));
    }
    if let Some(v) = b.disabled {
        what.push(format!("disabled={v}"));
    }
    let disabling = b.disabled == Some(true);
    if let Some(p) = &b.password {
        validate_password(p)?;
    }
    let me = a.0.user.id;
    let my_token = a.0.token_hash.clone();
    let user = st
        .db
        .call(move |c| {
            let current = get_user(c, id)?;
            let loses_admin = (b.is_admin == Some(false) && current.is_admin) || (b.disabled == Some(true) && current.is_admin);
            if id == me && b.disabled == Some(true) {
                return Err(ApiError::bad_request("You can't disable your own account"));
            }
            if id == me && b.is_admin == Some(false) {
                return Err(ApiError::bad_request("You can't remove your own admin rights"));
            }
            if loses_admin && other_active_admins(c, id)? == 0 {
                return Err(ApiError::bad_request("There must be at least one active admin"));
            }
            let tx = c.transaction()?;
            if let Some(p) = &b.password {
                tx.execute("UPDATE users SET password_hash = ?2 WHERE id = ?1", params![id, hash_password(p)?])?;
            }
            if let Some(v) = b.is_admin {
                tx.execute("UPDATE users SET is_admin = ?2 WHERE id = ?1", params![id, v])?;
            }
            if let Some(v) = b.disabled {
                tx.execute("UPDATE users SET disabled = ?2 WHERE id = ?1", params![id, v])?;
            }
            // Deshabilitar o cambiarle la contraseña cierra todas sus sesiones (si es uno mismo,
            // menos la actual).
            if b.disabled == Some(true) || b.password.is_some() {
                tx.execute("DELETE FROM sessions WHERE user_id = ?1 AND token_hash <> ?2", params![id, my_token])?;
            }
            tx.commit()?;
            get_user(c, id)
        })
        .await?;
    st.auth.clear_cache();
    if user.disabled {
        let w = st.watching.lock().unwrap().remove(&user.id);
        if let Some(w) = w {
            st.media.close(&w.session_id).await;
        }
    }
    tracing::info!(by = %a.0.user.username, user = %user.username, admin = user.is_admin, disabled = user.disabled, "usuario editado");
    let event = if disabling { "user_disabled" } else { "user_updated" };
    st.recorder.audit(event, Some(a.0.user.id), Some(&a.0.user.username), Some(ip), Some(format!("{}: {}", user.username, what.join(", "))));
    Ok(Json(user))
}

async fn remove_user(State(st): State<AppState>, ClientIp(ip): ClientIp, a: AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if id == a.0.user.id {
        return Err(ApiError::bad_request("You can't delete your own account"));
    }
    let user = st
        .db
        .call(move |c| {
            let u = get_user(c, id)?;
            if u.is_admin && !u.disabled && other_active_admins(c, id)? == 0 {
                return Err(ApiError::bad_request("You can't delete the last admin"));
            }
            c.execute("DELETE FROM users WHERE id = ?1", [id])?;
            Ok(u)
        })
        .await?;
    st.auth.clear_cache();
    let w = st.watching.lock().unwrap().remove(&id);
    if let Some(w) = w {
        st.media.close(&w.session_id).await;
    }
    tracing::info!(by = %a.0.user.username, user = %user.username, "usuario borrado");
    st.recorder.audit("user_deleted", Some(a.0.user.id), Some(&a.0.user.username), Some(ip), Some(format!("deleted {}", user.username)));
    Ok(StatusCode::NO_CONTENT)
}

async fn torrents(State(st): State<AppState>, _a: AdminUser) -> Json<Vec<mythos_media::TorrentStat>> {
    Json(st.media.torrents().await)
}

#[derive(Deserialize)]
struct RemoveQuery {
    #[serde(default)]
    files: bool,
}

async fn remove_torrent(
    State(st): State<AppState>,
    a: AdminUser,
    Path(info_hash): Path<String>,
    Query(q): Query<RemoveQuery>,
) -> ApiResult<StatusCode> {
    let h = info_hash.to_ascii_lowercase();
    if !crate::torrentio::is_info_hash(&h) {
        return Err(ApiError::bad_request("info_hash must be 40 hex characters"));
    }
    st.media.remove_torrent(&h, q.files).await.map_err(|e| ApiError::bad_request(format!("Could not remove torrent: {e}")))?;
    tracing::info!(by = %a.0.user.username, %h, files = q.files, "torrent borrado");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct Stats {
    cache_bytes: u64,
    cache_limit_bytes: u64,
    disk_free_bytes: u64,
    active_sessions: u32,
    users: i64,
    /// Qué está viendo cada usuario (último `/api/play` de las últimas 6 h).
    watching: Vec<Watching>,
}

async fn stats(State(st): State<AppState>, _a: AdminUser) -> ApiResult<Json<Stats>> {
    let m = st.media.stats().await;
    let users = st.db.call(|c| Ok(c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?)).await?;
    let cutoff = now_unix() - 6 * 3600;
    let mut watching: Vec<Watching> = {
        let mut w = st.watching.lock().unwrap();
        w.retain(|_, v| v.started_at > cutoff);
        w.values().cloned().collect()
    };
    watching.sort_by_key(|w| std::cmp::Reverse(w.started_at));
    Ok(Json(Stats {
        cache_bytes: m.cache_bytes,
        cache_limit_bytes: m.cache_limit_bytes,
        disk_free_bytes: m.disk_free_bytes,
        active_sessions: m.active_sessions,
        users,
        watching,
    }))
}
