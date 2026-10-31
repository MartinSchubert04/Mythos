//! UI estática (`MYTHOS_WEB_DIR`): archivos de Vite con fallback SPA a `index.html`.
//! `/assets/*` lleva hash en el nombre → caché de un año; el resto (index.html) → `no-cache` para
//! que una versión nueva se vea al recargar. Las páginas HTML llevan Content-Security-Policy: el
//! script inline de `index.html` (el que aplica el tema antes de pintar) se permite por su sha256,
//! que se recalcula si el archivo cambia, así no hace falta `'unsafe-inline'` para scripts.
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    Router,
};
use base64::Engine;
use sha2::{Digest, Sha256};
use tower_http::services::{ServeDir, ServeFile};

pub fn router(web_dir: &Path) -> Router {
    let index = web_dir.join("index.html");
    if !index.exists() {
        tracing::warn!("no está {} — la UI no se va a servir (¿falta compilar apps/web?)", index.display());
    }
    let csp = Arc::new(Csp { index: index.clone(), cached: Mutex::new(None) });
    let serve = ServeDir::new(web_dir)
        .precompressed_br()
        .precompressed_gzip()
        .fallback(ServeFile::new(index));
    Router::new().fallback_service(serve).layer(middleware::from_fn_with_state(csp, ui_headers))
}

/// CSP calculada a partir de `index.html` (cacheada por fecha de modificación).
struct Csp {
    index: PathBuf,
    cached: Mutex<Option<(SystemTime, HeaderValue)>>,
}

impl Csp {
    fn header(&self) -> HeaderValue {
        let mtime = std::fs::metadata(&self.index).and_then(|m| m.modified()).ok();
        let mut cached = self.cached.lock().unwrap();
        if let (Some(m), Some((at, v))) = (mtime, cached.as_ref()) {
            if *at == m {
                return v.clone();
            }
        }
        let html = std::fs::read_to_string(&self.index).unwrap_or_default();
        let value = HeaderValue::from_str(&build_csp(&html)).unwrap_or_else(|_| HeaderValue::from_static("default-src 'self'"));
        if let Some(m) = mtime {
            *cached = Some((m, value.clone()));
        }
        value
    }
}

/// Hashes (`'sha256-…'`) de los `<script>` inline (sin `src`) de un HTML.
fn inline_script_hashes(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<script") {
        rest = &rest[start..];
        let Some(tag_end) = rest.find('>') else { break };
        let tag = &rest[..tag_end];
        let body = &rest[tag_end + 1..];
        let Some(close) = body.find("</script>") else { break };
        if !tag.contains("src=") {
            let digest = Sha256::digest(&body.as_bytes()[..close]);
            out.push(format!("'sha256-{}'", base64::engine::general_purpose::STANDARD.encode(digest)));
        }
        rest = &body[close..];
    }
    out
}

/// Política para la UI:
/// - imágenes de cualquier https (los pósters de Cinemeta vienen de metahub, amazon, etc.);
/// - video/HLS solo del propio server (y `blob:` para Media Source de hls.js), workers `blob:`;
/// - el tráiler en un iframe de youtube-nocookie;
/// - nadie puede meter la UI en un iframe (`frame-ancestors 'none'`).
fn build_csp(index_html: &str) -> String {
    let mut script = String::from("'self'");
    for h in inline_script_hashes(index_html) {
        script.push(' ');
        script.push_str(&h);
    }
    format!(
        "default-src 'self'; script-src {script}; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob: https:; font-src 'self' data:; media-src 'self' blob:; \
         connect-src 'self'; worker-src 'self' blob:; \
         frame-src https://www.youtube-nocookie.com https://www.youtube.com; \
         object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'"
    )
}

async fn ui_headers(State(csp): State<Arc<Csp>>, req: Request, next: Next) -> Response {
    let immutable = req.uri().path().starts_with("/assets/");
    let mut res = next.run(req).await;
    let is_html = res.headers().get(header::CONTENT_TYPE).is_some_and(|v| v.as_bytes().starts_with(b"text/html"));
    // Un asset que no existe no debe recibir index.html (el fallback SPA) con caché de un año.
    if immutable && is_html {
        *res.status_mut() = StatusCode::NOT_FOUND;
        *res.body_mut() = Body::from("Not found");
        res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
        res.headers_mut().remove(header::CONTENT_LENGTH);
    } else if is_html {
        let h = res.headers_mut();
        h.insert(header::CONTENT_SECURITY_POLICY, csp.header());
        h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    }
    let value = if immutable && res.status().is_success() {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_hashes_inline_scripts_only() {
        let html = "<head><script>\n  var a = 1;\n</script><script type=\"module\" crossorigin src=\"/assets/i.js\"></script></head>";
        let hashes = inline_script_hashes(html);
        assert_eq!(hashes.len(), 1);
        let expected = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(b"\n  var a = 1;\n"));
        assert_eq!(hashes[0], format!("'sha256-{expected}'"));
        let csp = build_csp(html);
        assert!(csp.contains(&hashes[0]));
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(!csp.contains("script-src 'self' 'unsafe-inline'"));
    }
}
