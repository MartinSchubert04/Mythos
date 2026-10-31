//! Autenticación: contraseñas argon2id, tokens de sesión aleatorios (en la DB solo su sha256),
//! extractores `AuthUser`/`AdminUser`, middleware para `/api/media/**` (Bearer o `?t=`), rate limit
//! de login por IP y las rutas `/api/auth/*` y `/api/me*` (incluidas las sesiones abiertas).
//!
//! Sesiones largas: el login es una puerta de entrada de una sola vez. Una sesión dura 180 días
//! desde el último uso (se renueva sola) y para no escribir en la DB en cada request la renovación
//! se guarda como mucho una vez por día; el "último uso" fino (y la IP) se lleva en memoria y se
//! mezcla al listar las sesiones. El logout revoca la sesión.
//!
//! Sesiones que no vencen (`persistent`, las pide la app de TV en el login): `expires_at` queda en
//! `NEVER_EXPIRES` y no se renueva; solo se anota el último uso (una vez por día). Se revocan igual
//! que las demás (logout, `/api/me/sessions`, cambio de contraseña, el admin).
use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Argon2,
};
use axum::{
    extract::{ConnectInfo, FromRef, FromRequestParts, Request, State},
    http::{header, request::Parts, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Router,
};
use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    db::{now_unix, Db},
    error::{ApiError, ApiResult, Json, Path},
    metrics::{client_ip, Recorder, ReqSlot},
};

/// Las sesiones duran 180 días desde el último uso.
pub const SESSION_TTL_S: i64 = 180 * 24 * 3600;
/// La renovación (y `last_used`/`last_ip` en la DB) se escribe como mucho una vez por día: es
/// decir, cuando al vencimiento le queda menos que `TTL − 1 día`.
const RENEW_EVERY_S: i64 = 24 * 3600;
/// Vencimiento de las sesiones que no vencen: 9999-12-31T23:59:59Z (un número que cualquier
/// cliente sabe leer y que pasa todos los filtros `expires_at > ahora`).
pub const NEVER_EXPIRES: i64 = 253_402_300_799;
/// Caché en memoria token → usuario, para no ir a SQLite en cada request de media.
const AUTH_CACHE_TTL: Duration = Duration::from_secs(60);
/// Rate limit de login: 10 intentos fallidos por IP cada 5 minutos.
const LOGIN_MAX_FAILS: usize = 10;
const LOGIN_WINDOW: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    pub disabled: bool,
    pub created_at: String,
    /// El avatar que eligio de la galeria (avatars.rs); `null` = la inicial.
    pub avatar_id: Option<i64>,
}

pub const USER_COLS: &str = "id, username, is_admin, disabled, created_at, avatar_id";

pub fn user_from_row(r: &rusqlite::Row) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(0)?,
        username: r.get(1)?,
        is_admin: r.get(2)?,
        disabled: r.get(3)?,
        created_at: r.get(4)?,
        avatar_id: r.get(5)?,
    })
}

/// De qué dispositivo es una sesión (lo que se ve en `GET /api/me/sessions`).
#[derive(Clone, Debug, Default)]
pub struct SessionMeta {
    /// Etiqueta que manda la UI en el login ("Pixel 8", "Desktop"...).
    pub device: Option<String>,
    pub user_agent: Option<String>,
    /// La sesión no vence (la app de TV: el login es de una sola vez para siempre).
    pub persistent: bool,
}

impl SessionMeta {
    pub fn new(device: Option<&str>, headers: &HeaderMap) -> Self {
        Self {
            device: device.and_then(|d| clean(d, 64)),
            user_agent: headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).and_then(|v| clean(v, 200)),
            persistent: false,
        }
    }
}

/// Recorta y saca caracteres de control (texto que viene del cliente y se muestra o loguea).
fn clean(s: &str, max: usize) -> Option<String> {
    let s: String = s.trim().chars().filter(|c| !c.is_control()).take(max).collect();
    (!s.is_empty()).then_some(s)
}

/// Estado de auth: la DB, la caché de tokens, el rate limiter y el registro de auditoría.
#[derive(Clone)]
pub struct Auth {
    pub db: Db,
    pub recorder: Recorder,
    inner: Arc<AuthInner>,
}

struct AuthInner {
    cache: Mutex<HashMap<String, (User, Instant)>>,
    fails: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
    /// token_hash → (último uso en unix s, IP): fino, en memoria (la DB se actualiza 1 vez/día).
    seen: Mutex<HashMap<String, (i64, IpAddr)>>,
    /// Hash de una contraseña cualquiera: si el usuario no existe se verifica contra este, así el
    /// tiempo de respuesta no revela qué usuarios existen.
    dummy_hash: String,
}

impl Auth {
    #[cfg(test)]
    pub fn new(db: Db) -> Self {
        Self::with_recorder(db, Recorder::disabled())
    }

    pub fn with_recorder(db: Db, recorder: Recorder) -> Self {
        let dummy_hash = hash_password("mythos-dummy-password").expect("argon2");
        Self {
            db,
            recorder,
            inner: Arc::new(AuthInner {
                cache: Mutex::new(HashMap::new()),
                fails: Mutex::new(HashMap::new()),
                seen: Mutex::new(HashMap::new()),
                dummy_hash,
            }),
        }
    }

    /// Olvida todas las sesiones cacheadas (al deshabilitar, borrar o cambiar contraseñas).
    pub fn clear_cache(&self) {
        self.inner.cache.lock().unwrap().clear();
    }

    fn mark_seen(&self, hash: &str, ip: Option<IpAddr>) {
        let Some(ip) = ip else { return };
        let mut seen = self.inner.seen.lock().unwrap();
        if seen.len() > 5000 {
            seen.clear();
        }
        seen.insert(hash.to_string(), (now_unix(), ip));
    }

    /// Devuelve el usuario dueño del token, o `None` si no es válido, expiró o está deshabilitado.
    pub async fn authenticate(&self, token: &str, ip: Option<IpAddr>) -> ApiResult<Option<User>> {
        let hash = token_hash(token);
        if let Some((user, at)) = self.inner.cache.lock().unwrap().get(&hash) {
            if at.elapsed() < AUTH_CACHE_TTL {
                let user = user.clone();
                self.mark_seen(&hash, ip);
                return Ok(Some(user));
            }
        }
        let h = hash.clone();
        let ip_s = ip.map(|i| i.to_string());
        let user = self
            .db
            .call(move |c| {
                let now = now_unix();
                let row = c
                    .query_row(
                        "SELECT u.id, u.username, u.is_admin, u.disabled, u.created_at, u.avatar_id, s.expires_at, s.persistent, s.last_used
                         FROM sessions s JOIN users u ON u.id = s.user_id
                         WHERE s.token_hash = ?1 AND s.expires_at > ?2",
                        params![h, now],
                        |r| Ok((user_from_row(r)?, r.get::<_, i64>(6)?, r.get::<_, bool>(7)?, r.get::<_, i64>(8)?)),
                    )
                    .optional()?;
                let Some((user, expires_at, persistent, last_used)) = row else { return Ok(None) };
                if user.disabled {
                    return Ok(None);
                }
                if persistent {
                    // No vence: solo el último uso, una vez por día.
                    if now - last_used >= RENEW_EVERY_S {
                        c.execute(
                            "UPDATE sessions SET last_used = ?2, last_ip = COALESCE(?3, last_ip) WHERE token_hash = ?1",
                            params![h, now, ip_s],
                        )?;
                    }
                } else if expires_at - now < SESSION_TTL_S - RENEW_EVERY_S {
                    // Renovar solo si al vencimiento le queda menos que TTL − 1 día.
                    c.execute(
                        "UPDATE sessions SET last_used = ?2, expires_at = ?3, last_ip = COALESCE(?4, last_ip) WHERE token_hash = ?1",
                        params![h, now, now + SESSION_TTL_S, ip_s],
                    )?;
                }
                Ok(Some(user))
            })
            .await?;
        if user.is_some() {
            self.mark_seen(&hash, ip);
        }
        let mut cache = self.inner.cache.lock().unwrap();
        if cache.len() > 2000 {
            cache.clear();
        }
        match &user {
            Some(u) => {
                cache.insert(hash, (u.clone(), Instant::now()));
            }
            None => {
                cache.remove(&hash);
            }
        }
        Ok(user)
    }

    /// Verifica usuario y contraseña contra la DB (sin rate limit ni sesión). `Ok(None)` = no.
    pub async fn verify(&self, username: &str, password: &str) -> ApiResult<Option<User>> {
        let name = username.trim().to_string();
        let row = self
            .db
            .call(move |c| {
                Ok(c.query_row(
                    &format!("SELECT {USER_COLS}, password_hash FROM users WHERE username = ?1"),
                    [name],
                    |r| Ok((user_from_row(r)?, r.get::<_, String>(6)?)),
                )
                .optional()?)
            })
            .await?;
        let (user, stored) = match row {
            Some((u, h)) => (Some(u), h),
            None => (None, self.inner.dummy_hash.clone()),
        };
        let pw = password.to_string();
        let ok = tokio::task::spawn_blocking(move || verify_password(&pw, &stored)).await.map_err(ApiError::internal)?;
        Ok(if ok { user } else { None })
    }

    /// Verifica usuario y contraseña y abre una sesión nueva. Aplica el rate limit por IP.
    pub async fn login(&self, ip: IpAddr, username: &str, password: &str, meta: SessionMeta) -> ApiResult<(String, User)> {
        self.check_rate(ip)?;
        // Para el log de intentos fallidos: recortado (y con `?` en el log, escapado) para que un
        // nombre raro no ensucie los logs.
        let name_for_log: String = username.trim().chars().take(64).collect();
        let Some(user) = self.verify(username, password).await? else {
            self.record_fail(ip, &name_for_log, None);
            return Err(ApiError::unauthorized("Invalid username or password"));
        };
        if user.disabled {
            self.record_fail(ip, &name_for_log, Some("disabled"));
            return Err(ApiError::unauthorized("This account is disabled"));
        }
        self.inner.fails.lock().unwrap().remove(&ip);
        let token = self.open_session(&user, ip, meta).await?;
        Ok((token, user))
    }

    /// Error 429 si la IP está bloqueada por intentos fallidos.
    pub fn check_rate(&self, ip: IpAddr) -> ApiResult<()> {
        match self.blocked_for(ip) {
            Some(wait) => {
                let mins = wait.as_secs().div_ceil(60).max(1);
                Err(ApiError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    format!("Too many failed login attempts. Try again in {mins} minute(s)."),
                ))
            }
            None => Ok(()),
        }
    }

    /// Crea una sesión para `user` (ya autenticado) y devuelve el token.
    pub async fn open_session(&self, user: &User, ip: IpAddr, meta: SessionMeta) -> ApiResult<String> {
        let token = new_token();
        let (h, uid) = (token_hash(&token), user.id);
        let device = meta.device.clone();
        let ip_s = ip.to_string();
        self.db
            .call(move |c| {
                let now = now_unix();
                let expires = if meta.persistent { NEVER_EXPIRES } else { now + SESSION_TTL_S };
                c.execute(
                    "INSERT INTO sessions (token_hash, user_id, created_at, last_used, expires_at, device, user_agent, last_ip, persistent)
                     VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![h, uid, now, expires, meta.device, meta.user_agent, ip_s, meta.persistent],
                )?;
                Ok(())
            })
            .await?;
        tracing::info!(user = %user.username, %ip, device = ?device, persistent = meta.persistent, "login");
        self.recorder.audit("login_ok", Some(user.id), Some(&user.username), Some(ip), device);
        Ok(token)
    }

    pub async fn logout(&self, u: &AuthUser, ip: IpAddr) -> ApiResult<()> {
        let token_hash = u.token_hash.clone();
        self.inner.cache.lock().unwrap().remove(&token_hash);
        self.inner.seen.lock().unwrap().remove(&token_hash);
        self.db
            .call(move |c| {
                c.execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash])?;
                Ok(())
            })
            .await?;
        self.recorder.audit("logout", Some(u.user.id), Some(&u.user.username), Some(ip), None);
        Ok(())
    }

    /// Si la IP está bloqueada devuelve cuánto falta.
    fn blocked_for(&self, ip: IpAddr) -> Option<Duration> {
        let mut fails = self.inner.fails.lock().unwrap();
        let q = fails.get_mut(&ip)?;
        while q.front().is_some_and(|t| t.elapsed() > LOGIN_WINDOW) {
            q.pop_front();
        }
        if q.len() >= LOGIN_MAX_FAILS {
            q.front().map(|t| LOGIN_WINDOW.saturating_sub(t.elapsed()))
        } else {
            None
        }
    }

    /// Anota un login fallido (rate limit + auditoría).
    pub fn record_fail(&self, ip: IpAddr, username: &str, why: Option<&str>) {
        let n = {
            let mut fails = self.inner.fails.lock().unwrap();
            if fails.len() > 10_000 {
                fails.retain(|_, q| q.back().is_some_and(|t| t.elapsed() < LOGIN_WINDOW));
            }
            let q = fails.entry(ip).or_default();
            q.push_back(Instant::now());
            if q.len() > LOGIN_MAX_FAILS {
                q.pop_front();
            }
            q.len()
        };
        tracing::warn!(%ip, user = ?username, fails = n, "login fallido");
        self.recorder.audit("login_fail", None, Some(username), Some(ip), why.map(str::to_string));
    }
}

// ---------- primitivas ----------

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("argon2: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
        .unwrap_or(false)
}

/// 32 bytes aleatorios en base64url sin padding.
pub fn new_token() -> String {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).expect("getrandom");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

pub fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

pub fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 15) as usize] as char);
    }
    s
}

pub fn validate_username(u: &str) -> ApiResult<()> {
    let ok = (1..=32).contains(&u.chars().count())
        && u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad_request("Username must be 1-32 characters: letters, numbers, '_', '-' or '.'"))
    }
}

pub fn validate_password(p: &str) -> ApiResult<()> {
    match p.chars().count() {
        0..6 => Err(ApiError::bad_request("Password must be at least 6 characters")),
        6..=256 => Ok(()),
        _ => Err(ApiError::bad_request("Password is too long")),
    }
}

/// Cambia el nombre de un usuario (mismas reglas que al crear; único sin distinguir mayúsculas).
/// Devuelve el nombre viejo y el usuario. 409 si otro ya lo tiene. Las sesiones siguen valiendo:
/// van por `user_id`.
pub fn rename_user(c: &Connection, id: i64, new_name: &str) -> ApiResult<(String, User)> {
    let name = new_name.trim();
    validate_username(name)?;
    let old: String = c
        .query_row("SELECT username FROM users WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| ApiError::not_found("User not found"))?;
    if old != name {
        match c.execute("UPDATE users SET username = ?2 WHERE id = ?1", params![id, name]) {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
                return Err(ApiError::conflict("Username already exists"));
            }
            Err(e) => return Err(e.into()),
        }
    }
    let user = c.query_row(&format!("SELECT {USER_COLS} FROM users WHERE id = ?1"), [id], user_from_row)?;
    Ok((old, user))
}

/// Crea un usuario (bloqueante: hashea con argon2). 409 si el nombre ya existe.
pub fn create_user(c: &Connection, username: &str, password: &str, is_admin: bool) -> ApiResult<User> {
    let username = username.trim();
    validate_username(username)?;
    validate_password(password)?;
    let hash = hash_password(password)?;
    let res = c.query_row(
        &format!("INSERT INTO users (username, password_hash, is_admin) VALUES (?1, ?2, ?3) RETURNING {USER_COLS}"),
        params![username, hash, is_admin],
        user_from_row,
    );
    match res {
        Ok(u) => Ok(u),
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
            Err(ApiError::conflict("Username already exists"))
        }
        Err(e) => Err(e.into()),
    }
}

// ---------- extractores ----------

/// Usuario logueado (header `Authorization: Bearer`). 401 si no hay token válido.
#[derive(Clone, Debug)]
pub struct AuthUser {
    pub user: User,
    pub token_hash: String,
}

/// Usuario logueado y admin. 403 si no es admin.
pub struct AdminUser(pub AuthUser);

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let v = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = v.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim()).filter(|t| !t.is_empty())
}

fn query_token(parts_query: Option<&str>) -> Option<String> {
    url::form_urlencoded::parse(parts_query?.as_bytes())
        .find(|(k, _)| k == "t")
        .map(|(_, v)| v.into_owned())
        .filter(|t| !t.is_empty())
}

async fn resolve(auth: &Auth, token: Option<String>, ip: Option<IpAddr>) -> Result<AuthUser, ApiError> {
    let token = token.ok_or_else(|| ApiError::unauthorized("Missing session token"))?;
    match auth.authenticate(&token, ip).await? {
        Some(user) => Ok(AuthUser { user, token_hash: token_hash(&token) }),
        None => Err(ApiError::unauthorized("Session expired, please log in again")),
    }
}

fn request_ip(ext: &axum::http::Extensions, headers: &HeaderMap) -> IpAddr {
    client_ip(ext.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip()), headers).0
}

impl<S> FromRequestParts<S> for AuthUser
where
    Auth: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if let Some(u) = parts.extensions.get::<AuthUser>() {
            return Ok(u.clone());
        }
        let auth = Auth::from_ref(state);
        let ip = request_ip(&parts.extensions, &parts.headers);
        let u = resolve(&auth, bearer(&parts.headers).map(str::to_string), Some(ip)).await?;
        if let Some(slot) = parts.extensions.get::<ReqSlot>() {
            slot.set_user(u.user.id);
        }
        parts.extensions.insert(u.clone());
        Ok(u)
    }
}

impl<S> FromRequestParts<S> for AdminUser
where
    Auth: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let u = AuthUser::from_request_parts(parts, state).await?;
        if !u.user.is_admin {
            return Err(ApiError::forbidden("Admin access required"));
        }
        Ok(AdminUser(u))
    }
}

/// Middleware de `/api/media/**`: acepta Bearer o `?t=` (el `<video>` no manda headers).
pub async fn media_auth(State(auth): State<Auth>, mut req: Request, next: Next) -> Response {
    let token = bearer(req.headers()).map(str::to_string).or_else(|| query_token(req.uri().query()));
    let ip = request_ip(req.extensions(), req.headers());
    match resolve(&auth, token, Some(ip)).await {
        Ok(u) => {
            if let Some(slot) = req.extensions().get::<ReqSlot>() {
                slot.set_user(u.user.id);
            }
            req.extensions_mut().insert(u);
            next.run(req).await
        }
        Err(e) => e.into_response(),
    }
}

/// IP del cliente. Solo si la conexión viene de loopback (tailscale serve, el proxy de Vite) se
/// confía en el último valor de `X-Forwarded-For`, que es el que agregó el proxy.
pub struct ClientIp(pub IpAddr);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(request_ip(&parts.extensions, &parts.headers)))
    }
}

// ---------- rutas ----------

/// Todas las rutas de cuenta del modo server: login, logout, `/me`, sesiones y contraseña.
pub fn routes<S>() -> Router<S>
where
    Auth: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/me/password", put(change_password))
        .route("/me/username", put(change_username))
        .merge(account_routes())
}

/// `/me` y las sesiones abiertas (comunes a los dos modos). En modo local, login, logout y
/// cambio de contraseña los pone `local.rs`.
pub fn account_routes<S>() -> Router<S>
where
    Auth: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/me", get(me).patch(crate::avatars::update_me))
        .route("/me/sessions", get(list_sessions).delete(revoke_other_sessions))
        .route("/me/sessions/{id}", delete(revoke_session))
}

#[derive(Deserialize)]
pub struct LoginBody {
    pub username: String,
    pub password: String,
    /// Etiqueta del dispositivo para la lista de sesiones (opcional).
    pub device: Option<String>,
    /// Sesión que no vence (la app de TV). Por defecto, la de siempre (180 días desde el último uso).
    #[serde(default)]
    pub persistent: bool,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: User,
}

async fn login(State(auth): State<Auth>, ClientIp(ip): ClientIp, headers: HeaderMap, Json(b): Json<LoginBody>) -> ApiResult<Json<LoginResponse>> {
    let meta = SessionMeta { persistent: b.persistent, ..SessionMeta::new(b.device.as_deref(), &headers) };
    let (token, user) = auth.login(ip, &b.username, &b.password, meta).await?;
    Ok(Json(LoginResponse { token, user }))
}

async fn logout(State(auth): State<Auth>, ClientIp(ip): ClientIp, u: AuthUser) -> ApiResult<StatusCode> {
    auth.logout(&u, ip).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(u: AuthUser) -> Json<User> {
    Json(u.user)
}

/// Una sesión abierta (nunca el token).
#[derive(Serialize, Debug)]
pub struct SessionInfo {
    pub id: i64,
    pub device: Option<String>,
    pub user_agent: Option<String>,
    pub last_ip: Option<String>,
    /// unix s
    pub created_at: i64,
    pub last_used_at: i64,
    /// `NEVER_EXPIRES` si no vence.
    pub expires_at: i64,
    /// No vence (la app de TV).
    pub persistent: bool,
    /// Es la sesión de este request.
    pub current: bool,
}

async fn list_sessions(State(auth): State<Auth>, u: AuthUser) -> ApiResult<Json<Vec<SessionInfo>>> {
    Ok(Json(sessions_of(&auth, u.user.id, &u.token_hash).await?))
}

/// Las sesiones abiertas de un usuario, la más usada primero (`current`: la de `current_hash`).
/// También la usa el admin (`/api/admin/users/{id}/sessions`).
pub async fn sessions_of(auth: &Auth, uid: i64, current_hash: &str) -> ApiResult<Vec<SessionInfo>> {
    let rows = auth
        .db
        .call(move |c| {
            let mut st = c.prepare(
                "SELECT rowid, token_hash, device, user_agent, last_ip, created_at, last_used, expires_at, persistent
                 FROM sessions WHERE user_id = ?1 AND expires_at > ?2",
            )?;
            let rows = st
                .query_map(params![uid, now_unix()], |r| {
                    Ok((
                        r.get::<_, String>(1)?,
                        SessionInfo {
                            id: r.get(0)?,
                            device: r.get(2)?,
                            user_agent: r.get(3)?,
                            last_ip: r.get(4)?,
                            created_at: r.get(5)?,
                            last_used_at: r.get(6)?,
                            expires_at: r.get(7)?,
                            persistent: r.get(8)?,
                            current: false,
                        },
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    let seen = auth.inner.seen.lock().unwrap().clone();
    let mut out: Vec<SessionInfo> = rows
        .into_iter()
        .map(|(hash, mut s)| {
            if let Some((at, ip)) = seen.get(&hash) {
                if *at >= s.last_used_at {
                    s.last_used_at = *at;
                    s.last_ip = Some(ip.to_string());
                }
            }
            s.current = hash == current_hash;
            s
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.last_used_at));
    Ok(out)
}

async fn revoke_session(State(auth): State<Auth>, ClientIp(ip): ClientIp, u: AuthUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let uid = u.user.id;
    let n = auth
        .db
        .call(move |c| Ok(c.execute("DELETE FROM sessions WHERE rowid = ?1 AND user_id = ?2", params![id, uid])?))
        .await?;
    if n == 0 {
        return Err(ApiError::not_found("Session not found"));
    }
    auth.clear_cache();
    auth.recorder.audit("session_revoked", Some(uid), Some(&u.user.username), Some(ip), Some(format!("session {id}")));
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct Revoked {
    revoked: usize,
}

/// "Cerrar sesión en los demás dispositivos": borra todas menos la actual.
async fn revoke_other_sessions(State(auth): State<Auth>, ClientIp(ip): ClientIp, u: AuthUser) -> ApiResult<Json<Revoked>> {
    let (uid, keep) = (u.user.id, u.token_hash.clone());
    let n = auth
        .db
        .call(move |c| Ok(c.execute("DELETE FROM sessions WHERE user_id = ?1 AND token_hash <> ?2", params![uid, keep])?))
        .await?;
    auth.clear_cache();
    auth.recorder.audit("session_revoked", Some(uid), Some(&u.user.username), Some(ip), Some(format!("{n} other sessions")));
    Ok(Json(Revoked { revoked: n }))
}

#[derive(Deserialize)]
struct PasswordBody {
    current: String,
    new: String,
}

/// Cambia la contraseña propia y cierra las demás sesiones del usuario (la actual sigue).
async fn change_password(State(auth): State<Auth>, ClientIp(ip): ClientIp, u: AuthUser, Json(b): Json<PasswordBody>) -> ApiResult<StatusCode> {
    validate_password(&b.new)?;
    let uid = u.user.id;
    let keep = u.token_hash.clone();
    auth.db
        .call(move |c| {
            let stored: String = c.query_row("SELECT password_hash FROM users WHERE id = ?1", [uid], |r| r.get(0))?;
            if !verify_password(&b.current, &stored) {
                return Err(ApiError::bad_request("Current password is incorrect"));
            }
            let hash = hash_password(&b.new)?;
            c.execute("UPDATE users SET password_hash = ?2 WHERE id = ?1", params![uid, hash])?;
            c.execute("DELETE FROM sessions WHERE user_id = ?1 AND token_hash <> ?2", params![uid, keep])?;
            Ok(())
        })
        .await?;
    auth.clear_cache();
    auth.recorder.audit("password_changed", Some(uid), Some(&u.user.username), Some(ip), None);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct UsernameBody {
    username: String,
}

/// `PUT /api/me/username`: cada uno cambia su nombre (en modo local, 403: se cambia en el server y
/// el dispositivo lo toma al sincronizar).
async fn change_username(State(auth): State<Auth>, ClientIp(ip): ClientIp, u: AuthUser, Json(b): Json<UsernameBody>) -> ApiResult<Json<User>> {
    let uid = u.user.id;
    let (old, user) = auth.db.call(move |c| rename_user(c, uid, &b.username)).await?;
    auth.clear_cache();
    if old != user.username {
        auth.recorder.audit("user_renamed", Some(uid), Some(&user.username), Some(ip), Some(format!("{old} → {}", user.username)));
    }
    Ok(Json(user))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    async fn setup() -> (Auth, Router) {
        let db = Db::open_in_memory().unwrap();
        db.call(|c| {
            create_user(c, "admin", "secret123", true)?;
            create_user(c, "bob", "hunter22", false)?;
            Ok(())
        })
        .await
        .unwrap();
        let auth = Auth::new(db);
        let app = Router::new().nest("/api", routes::<Auth>()).with_state(auth.clone());
        (auth, app)
    }

    async fn call(app: &Router, method: &str, uri: &str, token: Option<&str>, body: Option<&str>) -> (StatusCode, serde_json::Value) {
        let mut b = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        if body.is_some() {
            b = b.header("content-type", "application/json");
        }
        let req = b.body(Body::from(body.unwrap_or("").to_string())).unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    async fn login_ok(app: &Router, user: &str, pass: &str) -> String {
        let body = format!(r#"{{"username":"{user}","password":"{pass}"}}"#);
        let (st, v) = call(app, "POST", "/api/auth/login", None, Some(&body)).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert!(v["user"]["username"].as_str().unwrap().eq_ignore_ascii_case(user));
        v["token"].as_str().unwrap().to_string()
    }

    #[test]
    fn password_hash_roundtrip() {
        let h = hash_password("correct horse").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("x", "not a hash"));
    }

    #[test]
    fn token_shape() {
        let t = new_token();
        assert_eq!(t.len(), 43); // 32 bytes en base64url sin padding
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(t, new_token());
        assert_eq!(token_hash("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[tokio::test]
    async fn login_me_logout() {
        let (_auth, app) = setup().await;
        let (st, v) = call(&app, "POST", "/api/auth/login", None, Some(r#"{"username":"bob","password":"nope"}"#)).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
        assert_eq!(v["error"], "Invalid username or password");
        let (st, _) = call(&app, "POST", "/api/auth/login", None, Some(r#"{"username":"ghost","password":"nope"}"#)).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);

        // El nombre de usuario no distingue mayúsculas.
        let token = login_ok(&app, "BOB", "hunter22").await;
        let (st, v) = call(&app, "GET", "/api/me", Some(&token), None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["username"], "bob");
        assert_eq!(v["is_admin"], false);

        let (st, v) = call(&app, "GET", "/api/me", None, None).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
        assert!(v["error"].is_string());
        let (st, _) = call(&app, "GET", "/api/me", Some("garbage"), None).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);

        let (st, _) = call(&app, "POST", "/api/auth/logout", Some(&token), None).await;
        assert_eq!(st, StatusCode::NO_CONTENT);
        let (st, _) = call(&app, "GET", "/api/me", Some(&token), None).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn disabled_user_loses_sessions() {
        let (auth, app) = setup().await;
        let token = login_ok(&app, "bob", "hunter22").await;
        assert_eq!(call(&app, "GET", "/api/me", Some(&token), None).await.0, StatusCode::OK);
        // Lo mismo que hace PATCH /api/admin/users/{id} con disabled=true.
        auth.db
            .call(|c| {
                c.execute("UPDATE users SET disabled = 1 WHERE username = 'bob'", [])?;
                c.execute("DELETE FROM sessions WHERE user_id = (SELECT id FROM users WHERE username = 'bob')", [])?;
                Ok(())
            })
            .await
            .unwrap();
        auth.clear_cache();
        assert_eq!(call(&app, "GET", "/api/me", Some(&token), None).await.0, StatusCode::UNAUTHORIZED);
        let (st, v) = call(&app, "POST", "/api/auth/login", None, Some(r#"{"username":"bob","password":"hunter22"}"#)).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
        assert_eq!(v["error"], "This account is disabled");
    }

    #[tokio::test]
    async fn change_password_flow() {
        let (_auth, app) = setup().await;
        let t1 = login_ok(&app, "bob", "hunter22").await;
        let t2 = login_ok(&app, "bob", "hunter22").await;
        let (st, v) = call(&app, "PUT", "/api/me/password", Some(&t1), Some(r#"{"current":"bad","new":"newpass1"}"#)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
        let (st, _) = call(&app, "PUT", "/api/me/password", Some(&t1), Some(r#"{"current":"hunter22","new":"newpass1"}"#)).await;
        assert_eq!(st, StatusCode::NO_CONTENT);
        assert_eq!(call(&app, "GET", "/api/me", Some(&t1), None).await.0, StatusCode::OK);
        assert_eq!(call(&app, "GET", "/api/me", Some(&t2), None).await.0, StatusCode::UNAUTHORIZED);
        login_ok(&app, "bob", "newpass1").await;
    }

    #[tokio::test]
    async fn login_rate_limit() {
        let (_auth, app) = setup().await;
        for _ in 0..LOGIN_MAX_FAILS {
            let (st, _) = call(&app, "POST", "/api/auth/login", None, Some(r#"{"username":"bob","password":"x"}"#)).await;
            assert_eq!(st, StatusCode::UNAUTHORIZED);
        }
        let (st, v) = call(&app, "POST", "/api/auth/login", None, Some(r#"{"username":"bob","password":"hunter22"}"#)).await;
        assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "{v}");
    }

    #[tokio::test]
    async fn bad_json_is_api_error() {
        let (_auth, app) = setup().await;
        let (st, v) = call(&app, "POST", "/api/auth/login", None, Some("{nope")).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert!(v["error"].as_str().unwrap().starts_with("Invalid JSON body"));
    }

    #[test]
    fn duplicate_username() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        create_user(&c, "ana", "secret123", false).unwrap();
        let e = create_user(&c, "ANA", "secret123", false).unwrap_err();
        assert_eq!(e.status, StatusCode::CONFLICT);
        assert_eq!(create_user(&c, "bad name", "secret123", false).unwrap_err().status, StatusCode::BAD_REQUEST);
        assert_eq!(create_user(&c, "short", "123", false).unwrap_err().status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn sessions_list_and_revoke() {
        let (_auth, app) = setup().await;
        let (st, v) = call(&app, "POST", "/api/auth/login", None, Some(r#"{"username":"bob","password":"hunter22","device":"Pixel 8"}"#)).await;
        assert_eq!(st, StatusCode::OK);
        let phone = v["token"].as_str().unwrap().to_string();
        let desk = login_ok(&app, "bob", "hunter22").await;
        let other = login_ok(&app, "admin", "secret123").await;

        let (st, v) = call(&app, "GET", "/api/me/sessions", Some(&desk), None).await;
        assert_eq!(st, StatusCode::OK);
        let list = v.as_array().unwrap();
        assert_eq!(list.len(), 2, "solo las de bob: {v}");
        assert!(list.iter().any(|s| s["device"] == "Pixel 8" && s["current"] == false));
        assert!(list.iter().any(|s| s["current"] == true));
        // Nunca el token ni su hash.
        let text = v.to_string();
        assert!(!text.contains(&phone) && !text.contains(&token_hash(&phone)) && !text.contains("token"));
        let exp = list[0]["expires_at"].as_i64().unwrap() - now_unix();
        assert!(exp > 170 * 24 * 3600, "sesiones largas");

        // Borrar la del teléfono por id.
        let id = list.iter().find(|s| s["device"] == "Pixel 8").unwrap()["id"].as_i64().unwrap();
        let (st, _) = call(&app, "DELETE", &format!("/api/me/sessions/{id}"), Some(&other), None).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "no se puede borrar una sesión ajena");
        let (st, _) = call(&app, "DELETE", &format!("/api/me/sessions/{id}"), Some(&desk), None).await;
        assert_eq!(st, StatusCode::NO_CONTENT);
        assert_eq!(call(&app, "GET", "/api/me", Some(&phone), None).await.0, StatusCode::UNAUTHORIZED);

        // "Cerrar las demás".
        let t3 = login_ok(&app, "bob", "hunter22").await;
        let (st, v) = call(&app, "DELETE", "/api/me/sessions", Some(&desk), None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["revoked"], 1);
        assert_eq!(call(&app, "GET", "/api/me", Some(&t3), None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(call(&app, "GET", "/api/me", Some(&desk), None).await.0, StatusCode::OK);
        assert_eq!(call(&app, "GET", "/api/me", Some(&other), None).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn rename_self_keeps_sessions() {
        let (_auth, app) = setup().await;
        let t = login_ok(&app, "bob", "hunter22").await;
        let (st, _) = call(&app, "PUT", "/api/me/username", Some(&t), Some(r#"{"username":"bad name"}"#)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        let (st, _) = call(&app, "PUT", "/api/me/username", Some(&t), Some(r#"{"username":"ADMIN"}"#)).await;
        assert_eq!(st, StatusCode::CONFLICT, "unico sin distinguir mayusculas");
        let (st, v) = call(&app, "PUT", "/api/me/username", Some(&t), Some(r#"{"username":" robert "}"#)).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert_eq!(v["username"], "robert");
        // La sesion sigue (va por user_id) y /me ya dice el nombre nuevo.
        let (st, v) = call(&app, "GET", "/api/me", Some(&t), None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["username"], "robert");
        // Entra con el nombre nuevo, no con el viejo.
        login_ok(&app, "robert", "hunter22").await;
        let (st, _) = call(&app, "POST", "/api/auth/login", None, Some(r#"{"username":"bob","password":"hunter22"}"#)).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
        // Solo mayusculas: vale (es el mismo).
        let (st, v) = call(&app, "PUT", "/api/me/username", Some(&t), Some(r#"{"username":"Robert"}"#)).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["username"], "Robert");
    }

    #[tokio::test]
    async fn persistent_sessions_never_expire_but_revoke() {
        let (auth, app) = setup().await;
        let (st, v) = call(&app, "POST", "/api/auth/login", None, Some(r#"{"username":"bob","password":"hunter22","device":"Mythos TV","persistent":true}"#)).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        let tv = v["token"].as_str().unwrap().to_string();
        let phone = login_ok(&app, "bob", "hunter22").await;

        let (_, v) = call(&app, "GET", "/api/me/sessions", Some(&phone), None).await;
        let list = v.as_array().unwrap();
        let tv_s = list.iter().find(|s| s["device"] == "Mythos TV").unwrap();
        assert_eq!(tv_s["persistent"], true);
        assert_eq!(tv_s["expires_at"].as_i64().unwrap(), NEVER_EXPIRES);
        let phone_s = list.iter().find(|s| s["current"] == true).unwrap();
        assert_eq!(phone_s["persistent"], false, "las de siempre no cambian");
        assert!(phone_s["expires_at"].as_i64().unwrap() < now_unix() + SESSION_TTL_S + 10);

        // Años sin usarla: sigue valiendo y no se "renueva" (el vencimiento no se toca); el
        // ultimo uso si se anota.
        let h = token_hash(&tv);
        auth.db
            .call(move |c| Ok(c.execute("UPDATE sessions SET last_used = 1, created_at = 1 WHERE token_hash = ?1", [h])?))
            .await
            .unwrap();
        auth.clear_cache();
        assert_eq!(call(&app, "GET", "/api/me", Some(&tv), None).await.0, StatusCode::OK);
        let h = token_hash(&tv);
        let (exp, used): (i64, i64) = auth
            .db
            .call(move |c| Ok(c.query_row("SELECT expires_at, last_used FROM sessions WHERE token_hash = ?1", [h], |r| Ok((r.get(0)?, r.get(1)?)))?))
            .await
            .unwrap();
        assert_eq!(exp, NEVER_EXPIRES);
        assert!(used > 1);

        // La limpieza de vencidas no la toca.
        auth.db.call(|c| Ok(c.execute("DELETE FROM sessions WHERE expires_at < ?1", [now_unix()])?)).await.unwrap();
        auth.clear_cache();
        assert_eq!(call(&app, "GET", "/api/me", Some(&tv), None).await.0, StatusCode::OK);

        // Se revoca como cualquier otra: desde otro dispositivo.
        let id = tv_s["id"].as_i64().unwrap();
        assert_eq!(call(&app, "DELETE", &format!("/api/me/sessions/{id}"), Some(&phone), None).await.0, StatusCode::NO_CONTENT);
        assert_eq!(call(&app, "GET", "/api/me", Some(&tv), None).await.0, StatusCode::UNAUTHORIZED);
    }
}
