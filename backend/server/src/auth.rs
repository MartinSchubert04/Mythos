//! Autenticación: contraseñas argon2id, tokens de sesión aleatorios (en la DB solo su sha256),
//! extractores `AuthUser`/`AdminUser`, middleware para `/api/media/**` (Bearer o `?t=`), rate limit
//! de login por IP y las rutas `/api/auth/*` y `/api/me*`.
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
    routing::{get, post, put},
    Router,
};
use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    db::{now_unix, Db},
    error::{ApiError, ApiResult, Json},
};

/// Las sesiones duran 30 días desde el último uso.
const SESSION_TTL_S: i64 = 30 * 24 * 3600;
/// Para no escribir en la DB en cada request (los segmentos HLS llegan cada pocos segundos), la
/// renovación de la sesión se hace como mucho una vez por hora.
const RENEW_EVERY_S: i64 = 3600;
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
}

pub const USER_COLS: &str = "id, username, is_admin, disabled, created_at";

pub fn user_from_row(r: &rusqlite::Row) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(0)?,
        username: r.get(1)?,
        is_admin: r.get(2)?,
        disabled: r.get(3)?,
        created_at: r.get(4)?,
    })
}

/// Estado de auth: la DB, la caché de tokens y el rate limiter.
#[derive(Clone)]
pub struct Auth {
    pub db: Db,
    inner: Arc<AuthInner>,
}

struct AuthInner {
    cache: Mutex<HashMap<String, (User, Instant)>>,
    fails: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
    /// Hash de una contraseña cualquiera: si el usuario no existe se verifica contra este, así el
    /// tiempo de respuesta no revela qué usuarios existen.
    dummy_hash: String,
}

impl Auth {
    pub fn new(db: Db) -> Self {
        let dummy_hash = hash_password("mythos-dummy-password").expect("argon2");
        Self {
            db,
            inner: Arc::new(AuthInner {
                cache: Mutex::new(HashMap::new()),
                fails: Mutex::new(HashMap::new()),
                dummy_hash,
            }),
        }
    }

    /// Olvida todas las sesiones cacheadas (al deshabilitar, borrar o cambiar contraseñas).
    pub fn clear_cache(&self) {
        self.inner.cache.lock().unwrap().clear();
    }

    /// Devuelve el usuario dueño del token, o `None` si no es válido, expiró o está deshabilitado.
    pub async fn authenticate(&self, token: &str) -> ApiResult<Option<User>> {
        let hash = token_hash(token);
        if let Some((user, at)) = self.inner.cache.lock().unwrap().get(&hash) {
            if at.elapsed() < AUTH_CACHE_TTL {
                return Ok(Some(user.clone()));
            }
        }
        let h = hash.clone();
        let user = self
            .db
            .call(move |c| {
                let now = now_unix();
                let row = c
                    .query_row(
                        "SELECT u.id, u.username, u.is_admin, u.disabled, u.created_at, s.last_used
                         FROM sessions s JOIN users u ON u.id = s.user_id
                         WHERE s.token_hash = ?1 AND s.expires_at > ?2",
                        params![h, now],
                        |r| Ok((user_from_row(r)?, r.get::<_, i64>(5)?)),
                    )
                    .optional()?;
                let Some((user, last_used)) = row else { return Ok(None) };
                if user.disabled {
                    return Ok(None);
                }
                if now - last_used > RENEW_EVERY_S {
                    c.execute(
                        "UPDATE sessions SET last_used = ?2, expires_at = ?3 WHERE token_hash = ?1",
                        params![h, now, now + SESSION_TTL_S],
                    )?;
                }
                Ok(Some(user))
            })
            .await?;
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

    /// Verifica usuario y contraseña y abre una sesión nueva. Aplica el rate limit por IP.
    pub async fn login(&self, ip: IpAddr, username: &str, password: &str) -> ApiResult<(String, User)> {
        if let Some(wait) = self.blocked_for(ip) {
            let mins = wait.as_secs().div_ceil(60).max(1);
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                format!("Too many failed login attempts. Try again in {mins} minute(s)."),
            ));
        }
        let name = username.trim().to_string();
        let row = self
            .db
            .call(move |c| {
                Ok(c.query_row(
                    &format!("SELECT {USER_COLS}, password_hash FROM users WHERE username = ?1"),
                    [name],
                    |r| Ok((user_from_row(r)?, r.get::<_, String>(5)?)),
                )
                .optional()?)
            })
            .await?;
        let (user, stored) = match row {
            Some((u, h)) => (Some(u), h),
            None => (None, self.inner.dummy_hash.clone()),
        };
        let pw = password.to_string();
        let ok = tokio::task::spawn_blocking(move || verify_password(&pw, &stored))
            .await
            .map_err(ApiError::internal)?;
        let user = match (ok, user) {
            (true, Some(u)) => u,
            _ => {
                self.record_fail(ip);
                return Err(ApiError::unauthorized("Invalid username or password"));
            }
        };
        if user.disabled {
            return Err(ApiError::unauthorized("This account is disabled"));
        }
        self.inner.fails.lock().unwrap().remove(&ip);
        let token = new_token();
        let (h, uid) = (token_hash(&token), user.id);
        self.db
            .call(move |c| {
                let now = now_unix();
                c.execute(
                    "INSERT INTO sessions (token_hash, user_id, created_at, last_used, expires_at)
                     VALUES (?1, ?2, ?3, ?3, ?4)",
                    params![h, uid, now, now + SESSION_TTL_S],
                )?;
                Ok(())
            })
            .await?;
        tracing::info!(user = %user.username, %ip, "login");
        Ok((token, user))
    }

    pub async fn logout(&self, token_hash: String) -> ApiResult<()> {
        self.inner.cache.lock().unwrap().remove(&token_hash);
        self.db
            .call(move |c| {
                c.execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash])?;
                Ok(())
            })
            .await
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

    fn record_fail(&self, ip: IpAddr) {
        let mut fails = self.inner.fails.lock().unwrap();
        if fails.len() > 10_000 {
            fails.retain(|_, q| q.back().is_some_and(|t| t.elapsed() < LOGIN_WINDOW));
        }
        let q = fails.entry(ip).or_default();
        q.push_back(Instant::now());
        if q.len() > LOGIN_MAX_FAILS {
            q.pop_front();
        }
        tracing::warn!(%ip, fails = q.len(), "login fallido");
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

async fn resolve(auth: &Auth, token: Option<String>) -> Result<AuthUser, ApiError> {
    let token = token.ok_or_else(|| ApiError::unauthorized("Missing session token"))?;
    match auth.authenticate(&token).await? {
        Some(user) => Ok(AuthUser { user, token_hash: token_hash(&token) }),
        None => Err(ApiError::unauthorized("Session expired, please log in again")),
    }
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
        let u = resolve(&auth, bearer(&parts.headers).map(str::to_string)).await?;
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
    match resolve(&auth, token).await {
        Ok(u) => {
            req.extensions_mut().insert(u);
            next.run(req).await
        }
        Err(e) => e.into_response(),
    }
}

/// IP del cliente. Si la conexión viene de loopback o de una red privada (tailscale serve, la red
/// de Docker), se confía en el último valor de `X-Forwarded-For`, que es el que agregó el proxy.
pub struct ClientIp(pub IpAddr);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip())
            .unwrap_or(IpAddr::from([127, 0, 0, 1]));
        let trusted = match peer {
            IpAddr::V4(v4) => v4.is_loopback() || v4.is_private(),
            IpAddr::V6(v6) => v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v| v.is_loopback() || v.is_private()),
        };
        let forwarded = trusted
            .then(|| parts.headers.get("x-forwarded-for")?.to_str().ok()?.rsplit(',').next()?.trim().parse().ok())
            .flatten();
        Ok(ClientIp(forwarded.unwrap_or(peer)))
    }
}

// ---------- rutas ----------

pub fn routes<S>() -> Router<S>
where
    Auth: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/me", get(me))
        .route("/me/password", put(change_password))
}

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    user: User,
}

async fn login(State(auth): State<Auth>, ClientIp(ip): ClientIp, Json(b): Json<LoginBody>) -> ApiResult<Json<LoginResponse>> {
    let (token, user) = auth.login(ip, &b.username, &b.password).await?;
    Ok(Json(LoginResponse { token, user }))
}

async fn logout(State(auth): State<Auth>, u: AuthUser) -> ApiResult<StatusCode> {
    auth.logout(u.token_hash).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(u: AuthUser) -> Json<User> {
    Json(u.user)
}

#[derive(Deserialize)]
struct PasswordBody {
    current: String,
    new: String,
}

/// Cambia la contraseña propia y cierra las demás sesiones del usuario (la actual sigue).
async fn change_password(State(auth): State<Auth>, u: AuthUser, Json(b): Json<PasswordBody>) -> ApiResult<StatusCode> {
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
    Ok(StatusCode::NO_CONTENT)
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
}
