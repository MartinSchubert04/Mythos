//! Avatares (docs/API.md, "Avatares"): una galería de imágenes que sube el admin; cada usuario
//! elige la suya y se ve en la barra lateral, en Settings y en el selector de perfiles de la TV.
//!
//! - Archivos en `<data_dir>/avatars/<id>.<ext>`; la tabla `avatars` guarda nombre y tipo. El
//!   nombre del archivo es el id (nunca lo que mandó el cliente): no hay path traversal posible.
//! - Solo PNG, JPEG, WebP o GIF, reconocidos por el contenido (no por el nombre ni el
//!   Content-Type), y como mucho 2 MB. Nada de SVG (podría llevar scripts).
//! - La imagen se sirve en `/api/media/avatars/{id}` (con login: Bearer o `?t=`, como el video,
//!   porque `<img>` no manda headers) con caché larga: los ids no se reusan (AUTOINCREMENT).
//! - Borrar un avatar deja sin avatar (la inicial) a quien lo tenía (`ON DELETE SET NULL`).
//! - Modo local: la galería es la del server de casa; el embebido no tiene (la lista vacía y el
//!   usuario sin avatar: la interfaz muestra la inicial).
use std::path::PathBuf;

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, FromRef, FromRequest, Multipart, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Router,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{AdminUser, Auth, AuthUser, ClientIp},
    db::{now_unix, Db},
    error::{ApiError, ApiResult, Json, Path, Query},
    state::AppState,
};

/// Tope de cada imagen.
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
/// Tope de la galería (para que una subida en loop no llene el disco).
const MAX_AVATARS: i64 = 200;

/// Lo que necesitan las rutas: la base y la carpeta de las imágenes.
#[derive(Clone)]
pub struct Avatars {
    pub db: Db,
    pub dir: PathBuf,
}

impl FromRef<AppState> for Avatars {
    fn from_ref(s: &AppState) -> Self {
        Avatars { db: s.db.clone(), dir: s.cfg.data_dir.join("avatars") }
    }
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Avatar {
    pub id: i64,
    pub name: String,
    /// unix s
    pub created_at: i64,
}

/// Tipo de imagen por los primeros bytes: (mime, extensión). `None` si no es una de las que se
/// aceptan.
pub fn sniff(b: &[u8]) -> Option<(&'static str, &'static str)> {
    if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(("image/png", "png"))
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("image/jpeg", "jpg"))
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else {
        None
    }
}

/// Los avatares que trae el server de fábrica (dibujos propios: `assets/avatars/make.py`).
const DEFAULTS: &[(&str, &[u8])] = &[
    ("Smile", include_bytes!("../assets/avatars/01-smile.png")),
    ("Wink", include_bytes!("../assets/avatars/02-wink.png")),
    ("Cat", include_bytes!("../assets/avatars/03-cat.png")),
    ("Robot", include_bytes!("../assets/avatars/04-robot.png")),
    ("Ghost", include_bytes!("../assets/avatars/05-ghost.png")),
    ("Alien", include_bytes!("../assets/avatars/06-alien.png")),
    ("Bear", include_bytes!("../assets/avatars/07-bear.png")),
    ("Shades", include_bytes!("../assets/avatars/08-shades.png")),
    ("Sleepy", include_bytes!("../assets/avatars/09-sleepy.png")),
    ("Monster", include_bytes!("../assets/avatars/10-monster.png")),
    ("Panda", include_bytes!("../assets/avatars/11-panda.png")),
    ("Star", include_bytes!("../assets/avatars/12-star.png")),
];

/// Suma los avatares de fábrica a la galería una sola vez por base (marca `default_avatars` en
/// `server_meta`): si el admin borra alguno, no vuelve. Se agregan después de los que ya hubiera.
pub async fn seed_defaults(db: &Db, dir: PathBuf) -> ApiResult<usize> {
    db.call(move |c| {
        let done = c.query_row("SELECT 1 FROM server_meta WHERE key = 'default_avatars'", [], |_| Ok(())).optional()?.is_some();
        if done {
            return Ok(0);
        }
        std::fs::create_dir_all(&dir).map_err(ApiError::internal)?;
        let tx = c.transaction()?;
        let now = now_unix();
        for (name, bytes) in DEFAULTS {
            let (mime, ext) = sniff(bytes).ok_or_else(|| ApiError::internal("avatar de fábrica inválido"))?;
            tx.execute("INSERT INTO avatars (name, mime, ext, created_at) VALUES (?1, ?2, ?3, ?4)", params![name, mime, ext, now])?;
            let id = tx.last_insert_rowid();
            let tmp = dir.join(format!("{id}.{ext}.tmp"));
            std::fs::write(&tmp, bytes).map_err(ApiError::internal)?;
            std::fs::rename(&tmp, dir.join(format!("{id}.{ext}"))).map_err(ApiError::internal)?;
        }
        tx.execute("INSERT INTO server_meta (key, value) VALUES ('default_avatars', ?1)", [now.to_string()])?;
        tx.commit()?;
        Ok(DEFAULTS.len())
    })
    .await
}

/// Nombre para mostrar: recortado, sin caracteres de control.
fn clean_name(s: &str) -> String {
    let s: String = s.trim().chars().filter(|c| !c.is_control()).take(60).collect();
    if s.is_empty() {
        "Avatar".into()
    } else {
        s
    }
}

/// Las rutas con login (los dos modos).
pub fn routes<S>() -> Router<S>
where
    Avatars: FromRef<S>,
    Auth: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/avatars", get(list))
}

/// Las del admin (solo modo server).
pub fn admin_routes<S>() -> Router<S>
where
    Avatars: FromRef<S>,
    Auth: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        // La API entera tiene un tope de 256 KB: esta ruta pone el suyo.
        // (algo más que la imagen: los encabezados del formulario multipart).
        .route("/admin/avatars", post(upload).layer(DefaultBodyLimit::max(MAX_BYTES + 16 * 1024)))
        .route("/admin/avatars/{id}", delete(remove))
}

/// La imagen (va bajo `/api/media`, con su middleware de login que acepta `?t=`).
pub fn media_routes<S>() -> Router<S>
where
    Avatars: FromRef<S>,
    Auth: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/avatars/{id}", get(image))
}

async fn list(State(av): State<Avatars>, _u: AuthUser) -> ApiResult<Json<Vec<Avatar>>> {
    let rows = av
        .db
        .call(|c| {
            let mut st = c.prepare_cached("SELECT id, name, created_at FROM avatars ORDER BY id")?;
            let rows = st
                .query_map([], |r| Ok(Avatar { id: r.get(0)?, name: r.get(1)?, created_at: r.get(2)? }))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
struct UploadQuery {
    name: Option<String>,
}

/// Lo que mandó el cliente: la imagen y el nombre que vino en el formulario (si vino).
async fn read_upload(req: Request) -> ApiResult<(Bytes, Option<String>)> {
    let multipart = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("multipart/form-data"));
    let too_big = || ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "Images can be up to 2 MB");
    if !multipart {
        return Bytes::from_request(req, &()).await.map(|b| (b, None)).map_err(|_| too_big());
    }
    // multipart/form-data: el campo `file` (la imagen) y opcional `name`. El tope del cuerpo lo
    // pone DefaultBodyLimit (más de eso corta la lectura).
    let mut form = Multipart::from_request(req, &()).await.map_err(|e| ApiError::bad_request(e.body_text()))?;
    let (mut file, mut name) = (None, None);
    while let Some(field) = form.next_field().await.map_err(|_| too_big())? {
        match field.name() {
            Some("file") if file.is_none() => file = Some(field.bytes().await.map_err(|_| too_big())?),
            Some("name") => name = Some(field.text().await.map_err(|_| ApiError::bad_request("Bad name field"))?),
            _ => {}
        }
    }
    let file = file.ok_or_else(|| ApiError::bad_request("Send the image in a `file` field"))?;
    Ok((file, name))
}

/// `POST /api/admin/avatars`: una imagen por pedido, en un formulario multipart (campo `file`,
/// opcional `name`) o tal cual en el cuerpo (`?name=`).
async fn upload(State(av): State<Avatars>, State(auth): State<Auth>, ClientIp(ip): ClientIp, a: AdminUser, Query(q): Query<UploadQuery>, req: Request) -> ApiResult<(StatusCode, Json<Avatar>)> {
    let (body, form_name) = read_upload(req).await?;
    if body.is_empty() {
        return Err(ApiError::bad_request("Send the image in the request body"));
    }
    if body.len() > MAX_BYTES {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "Images can be up to 2 MB"));
    }
    let Some((mime, ext)) = sniff(&body) else {
        return Err(ApiError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Use a PNG, JPEG, WebP or GIF image"));
    };
    let name = clean_name(form_name.as_deref().or(q.name.as_deref()).unwrap_or(""));
    let dir = av.dir.clone();
    let avatar = av
        .db
        .call(move |c| {
            let n: i64 = c.query_row("SELECT COUNT(*) FROM avatars", [], |r| r.get(0))?;
            if n >= MAX_AVATARS {
                return Err(ApiError::conflict(format!("The gallery is full ({MAX_AVATARS} images). Delete some first.")));
            }
            std::fs::create_dir_all(&dir).map_err(ApiError::internal)?;
            let tx = c.transaction()?;
            let now = now_unix();
            tx.execute("INSERT INTO avatars (name, mime, ext, created_at) VALUES (?1, ?2, ?3, ?4)", params![name, mime, ext, now])?;
            let id = tx.last_insert_rowid();
            // Primero el archivo (a uno temporal y renombrado): si falla, la fila no queda.
            let tmp = dir.join(format!("{id}.{ext}.tmp"));
            std::fs::write(&tmp, &body).map_err(ApiError::internal)?;
            std::fs::rename(&tmp, dir.join(format!("{id}.{ext}"))).map_err(ApiError::internal)?;
            tx.commit()?;
            Ok(Avatar { id, name, created_at: now })
        })
        .await?;
    auth.recorder.audit("avatar_added", Some(a.0.user.id), Some(&a.0.user.username), Some(ip), Some(format!("avatar {}", avatar.id)));
    Ok((StatusCode::CREATED, Json(avatar)))
}

async fn remove(State(av): State<Avatars>, State(auth): State<Auth>, ClientIp(ip): ClientIp, a: AdminUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let dir = av.dir.clone();
    av.db
        .call(move |c| {
            let ext: Option<String> = c.query_row("SELECT ext FROM avatars WHERE id = ?1", [id], |r| r.get(0)).optional()?;
            let Some(ext) = ext else { return Err(ApiError::not_found("Avatar not found")) };
            c.execute("DELETE FROM avatars WHERE id = ?1", [id])?;
            let _ = std::fs::remove_file(dir.join(format!("{id}.{ext}")));
            Ok(())
        })
        .await?;
    // Quien lo tenia queda sin avatar: el usuario cacheado por token tiene que releerse.
    auth.clear_cache();
    auth.recorder.audit("avatar_removed", Some(a.0.user.id), Some(&a.0.user.username), Some(ip), Some(format!("avatar {id}")));
    Ok(StatusCode::NO_CONTENT)
}

async fn image(State(av): State<Avatars>, _u: AuthUser, Path(id): Path<i64>) -> ApiResult<Response> {
    let dir = av.dir.clone();
    let found = av
        .db
        .call(move |c| Ok(c.query_row("SELECT mime, ext FROM avatars WHERE id = ?1", [id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).optional()?))
        .await?;
    let Some((mime, ext)) = found else { return Err(ApiError::not_found("Avatar not found")) };
    let bytes = tokio::fs::read(dir.join(format!("{id}.{ext}"))).await.map_err(|_| ApiError::not_found("Avatar not found"))?;
    let mut res = bytes.into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap_or(HeaderValue::from_static("application/octet-stream")));
    // El id nunca se reusa: la imagen de un id no cambia.
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=31536000, immutable"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(res)
}

#[derive(Deserialize)]
pub struct UpdateMe {
    /// `null` saca el avatar; si falta, no cambia.
    #[serde(default, deserialize_with = "present")]
    pub avatar_id: Option<Option<i64>>,
}

fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<i64>>, D::Error> {
    Option::<i64>::deserialize(d).map(Some)
}

/// `PATCH /api/me`: lo que cada usuario cambia de su propia cuenta (por ahora, el avatar).
pub async fn update_me(State(auth): State<Auth>, u: AuthUser, Json(b): Json<UpdateMe>) -> ApiResult<Json<crate::auth::User>> {
    let uid = u.user.id;
    let user = auth
        .db
        .call(move |c| {
            if let Some(avatar) = b.avatar_id {
                if let Some(id) = avatar {
                    let ok = c.query_row("SELECT 1 FROM avatars WHERE id = ?1", [id], |_| Ok(())).optional()?.is_some();
                    if !ok {
                        return Err(ApiError::not_found("Avatar not found"));
                    }
                }
                c.execute("UPDATE users SET avatar_id = ?1 WHERE id = ?2", params![avatar, uid])?;
            }
            Ok(c.query_row(&format!("SELECT {} FROM users WHERE id = ?1", crate::auth::USER_COLS), [uid], crate::auth::user_from_row)?)
        })
        .await?;
    auth.clear_cache();
    Ok(Json(user))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{create_user, routes as auth_routes, Auth};
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[derive(Clone)]
    struct TestState {
        auth: Auth,
        avatars: Avatars,
    }
    impl FromRef<TestState> for Auth {
        fn from_ref(s: &TestState) -> Self {
            s.auth.clone()
        }
    }
    impl FromRef<TestState> for Avatars {
        fn from_ref(s: &TestState) -> Self {
            s.avatars.clone()
        }
    }

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];

    async fn setup() -> (Router, PathBuf) {
        let db = Db::open_in_memory().unwrap();
        db.call(|c| {
            create_user(c, "admin", "secret123", true)?;
            create_user(c, "bob", "hunter22", false)?;
            Ok(())
        })
        .await
        .unwrap();
        let dir = std::env::temp_dir().join(format!("mythos-avatars-{}", crate::db::now_ms()));
        let auth = Auth::new(db.clone());
        let st = TestState { auth: auth.clone(), avatars: Avatars { db, dir: dir.clone() } };
        // Como en lib.rs: /api/media con su middleware (acepta ?t=).
        let media = media_routes::<TestState>().with_state(st.clone()).layer(axum::middleware::from_fn_with_state(auth, crate::auth::media_auth));
        let app = Router::new().nest("/api", auth_routes::<TestState>().merge(routes()).merge(admin_routes()).with_state(st).nest("/media", media));
        (app, dir)
    }

    async fn send(app: &Router, method: &str, uri: &str, token: Option<&str>, ctype: &str, body: Vec<u8>) -> (StatusCode, Vec<u8>, Option<String>) {
        let mut b = Request::builder().method(method).uri(uri).header("content-type", ctype);
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        let res = app.clone().oneshot(b.body(Body::from(body)).unwrap()).await.unwrap();
        let status = res.status();
        let ct = res.headers().get(header::CONTENT_TYPE).map(|v| v.to_str().unwrap().to_string());
        (status, axum::body::to_bytes(res.into_body(), 4 << 20).await.unwrap().to_vec(), ct)
    }

    #[tokio::test]
    async fn defaults_seeded_once() {
        let db = Db::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("mythos-avatars-seed-{}", crate::db::now_ms()));
        assert_eq!(seed_defaults(&db, dir.clone()).await.unwrap(), DEFAULTS.len());
        let count = |db: &Db| {
            let db = db.clone();
            async move { db.call(|c| Ok(c.query_row("SELECT COUNT(*) FROM avatars", [], |r| r.get::<_, i64>(0))?)).await.unwrap() }
        };
        assert_eq!(count(&db).await, DEFAULTS.len() as i64);
        assert!(dir.join("1.png").exists(), "cada avatar queda como <id>.png");
        // Si el admin borra uno, no vuelve: la galería se siembra una sola vez.
        db.call(|c| Ok(c.execute("DELETE FROM avatars WHERE id = 1", [])?)).await.unwrap();
        assert_eq!(seed_defaults(&db, dir.clone()).await.unwrap(), 0);
        assert_eq!(count(&db).await, DEFAULTS.len() as i64 - 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    async fn login(app: &Router, user: &str, pass: &str) -> String {
        let body = format!(r#"{{"username":"{user}","password":"{pass}"}}"#);
        let (st, b, _) = send(app, "POST", "/api/auth/login", None, "application/json", body.into_bytes()).await;
        assert_eq!(st, StatusCode::OK);
        serde_json::from_slice::<serde_json::Value>(&b).unwrap()["token"].as_str().unwrap().to_string()
    }

    fn json(b: &[u8]) -> serde_json::Value {
        serde_json::from_slice(b).unwrap()
    }

    #[test]
    fn sniffs_by_content() {
        assert_eq!(sniff(PNG), Some(("image/png", "png")));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(("image/jpeg", "jpg")));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(("image/webp", "webp")));
        assert_eq!(sniff(b"GIF89a..."), Some(("image/gif", "gif")));
        assert_eq!(sniff(b"<svg xmlns='http://www.w3.org/2000/svg'><script/></svg>"), None);
        assert_eq!(sniff(b"<html>"), None);
        assert_eq!(sniff(b""), None);
    }

    #[tokio::test]
    async fn gallery_upload_pick_and_delete() {
        let (app, dir) = setup().await;
        let admin = login(&app, "admin", "secret123").await;
        let bob = login(&app, "bob", "hunter22").await;

        // Solo el admin sube, solo imagenes, con tope.
        let (st, _, _) = send(&app, "POST", "/api/admin/avatars?name=Cat", Some(&bob), "image/png", PNG.to_vec()).await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        let (st, _, _) = send(&app, "POST", "/api/admin/avatars", Some(&admin), "image/png", b"<svg/>".to_vec()).await;
        assert_eq!(st, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let mut big = PNG.to_vec();
        big.resize(MAX_BYTES + 10, 0);
        let (st, _, _) = send(&app, "POST", "/api/admin/avatars", Some(&admin), "image/png", big).await;
        assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
        let (st, b, _) = send(&app, "POST", "/api/admin/avatars?name=..%2F..%2Fetc%2Fpasswd", Some(&admin), "application/octet-stream", PNG.to_vec()).await;
        assert_eq!(st, StatusCode::CREATED, "{}", String::from_utf8_lossy(&b));
        let id = json(&b)["id"].as_i64().unwrap();
        // El nombre es solo para mostrar: el archivo es el id.
        assert!(dir.join(format!("{id}.png")).is_file());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        // Formulario multipart (lo que manda la interfaz), más grande que el tope general de la API
        // (256 KB): el nombre del archivo del cliente no se usa para nada.
        let mut img = PNG.to_vec();
        img.resize(1024 * 1024, 7);
        let form = |field: &str, data: &[u8]| {
            let mut b = Vec::new();
            b.extend_from_slice(b"--XyZ\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nFox\r\n");
            b.extend_from_slice(format!("--XyZ\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"../../evil.svg\"\r\nContent-Type: image/svg+xml\r\n\r\n").as_bytes());
            b.extend_from_slice(data);
            b.extend_from_slice(b"\r\n--XyZ--\r\n");
            b
        };
        let ct = "multipart/form-data; boundary=XyZ";
        let (st, b, _) = send(&app, "POST", "/api/admin/avatars", Some(&admin), ct, form("file", &img)).await;
        assert_eq!(st, StatusCode::CREATED, "{}", String::from_utf8_lossy(&b));
        let big_id = json(&b)["id"].as_i64().unwrap();
        assert_eq!(json(&b)["name"], "Fox");
        assert_eq!(std::fs::read(dir.join(format!("{big_id}.png"))).unwrap(), img);
        let (st, _, _) = send(&app, "POST", "/api/admin/avatars", Some(&admin), ct, form("file", b"<svg onload=alert(1)/>")).await;
        assert_eq!(st, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let (st, _, _) = send(&app, "POST", "/api/admin/avatars", Some(&admin), ct, form("other", PNG)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        let mut huge = PNG.to_vec();
        huge.resize(MAX_BYTES + 100, 0);
        let (st, _, _) = send(&app, "POST", "/api/admin/avatars", Some(&admin), ct, form("file", &huge)).await;
        assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
        let (st, _, _) = send(&app, "DELETE", &format!("/api/admin/avatars/{big_id}"), Some(&admin), "", vec![]).await;
        assert_eq!(st, StatusCode::NO_CONTENT);

        // Cualquiera con login lista y ve la imagen (tambien con ?t=, para <img>).
        let (st, b, _) = send(&app, "GET", "/api/avatars", Some(&bob), "application/json", vec![]).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(json(&b).as_array().unwrap().len(), 1);
        let (st, _, _) = send(&app, "GET", &format!("/api/media/avatars/{id}"), None, "", vec![]).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
        let (st, img, ct) = send(&app, "GET", &format!("/api/media/avatars/{id}?t={bob}"), None, "", vec![]).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(img, PNG);
        assert_eq!(ct.as_deref(), Some("image/png"));

        // Bob elige el suyo; uno que no existe, 404; /me lo devuelve enseguida.
        let (st, _, _) = send(&app, "PATCH", "/api/me", Some(&bob), "application/json", br#"{"avatar_id":999}"#.to_vec()).await;
        assert_eq!(st, StatusCode::NOT_FOUND);
        let (st, b, _) = send(&app, "PATCH", "/api/me", Some(&bob), "application/json", format!(r#"{{"avatar_id":{id}}}"#).into_bytes()).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(json(&b)["avatar_id"], id);
        let (_, b, _) = send(&app, "GET", "/api/me", Some(&bob), "", vec![]).await;
        assert_eq!(json(&b)["avatar_id"], id);
        // Sin el campo no cambia nada.
        let (_, b, _) = send(&app, "PATCH", "/api/me", Some(&bob), "application/json", b"{}".to_vec()).await;
        assert_eq!(json(&b)["avatar_id"], id);

        // Borrarlo: el archivo se va y bob queda sin avatar.
        let (st, _, _) = send(&app, "DELETE", &format!("/api/admin/avatars/{id}"), Some(&bob), "", vec![]).await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        let (st, _, _) = send(&app, "DELETE", &format!("/api/admin/avatars/{id}"), Some(&admin), "", vec![]).await;
        assert_eq!(st, StatusCode::NO_CONTENT);
        assert!(!dir.join(format!("{id}.png")).exists());
        let (_, b, _) = send(&app, "GET", "/api/me", Some(&bob), "", vec![]).await;
        assert_eq!(json(&b)["avatar_id"], serde_json::Value::Null);
        let (st, _, _) = send(&app, "GET", &format!("/api/media/avatars/{id}?t={bob}"), None, "", vec![]).await;
        assert_eq!(st, StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
