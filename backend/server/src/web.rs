//! UI estática (`MYTHOS_WEB_DIR`): archivos de Vite con fallback SPA a `index.html`.
//! `/assets/*` lleva hash en el nombre → caché de un año; el resto (index.html) → `no-cache` para
//! que una versión nueva se vea al recargar.
use std::path::Path;

use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    Router,
};
use tower_http::services::{ServeDir, ServeFile};

pub fn router(web_dir: &Path) -> Router {
    let index = web_dir.join("index.html");
    if !index.exists() {
        tracing::warn!("no está {} — la UI no se va a servir (¿falta compilar apps/web?)", index.display());
    }
    let serve = ServeDir::new(web_dir)
        .precompressed_br()
        .precompressed_gzip()
        .fallback(ServeFile::new(index));
    Router::new().fallback_service(serve).layer(middleware::from_fn(cache_headers))
}

async fn cache_headers(req: Request, next: Next) -> Response {
    let immutable = req.uri().path().starts_with("/assets/");
    let mut res = next.run(req).await;
    // Un asset que no existe no debe recibir index.html (el fallback SPA) con caché de un año.
    let is_html = res.headers().get(header::CONTENT_TYPE).is_some_and(|v| v.as_bytes().starts_with(b"text/html"));
    if immutable && is_html {
        *res.status_mut() = StatusCode::NOT_FOUND;
        *res.body_mut() = Body::from("Not found");
        res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
        res.headers_mut().remove(header::CONTENT_LENGTH);
    }
    let value = if immutable && res.status().is_success() {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    res
}
