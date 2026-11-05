//! Imágenes achicadas: `GET /api/media/img?u=<url>&w=<ancho>` baja un fondo o una miniatura de
//! metahub, la achica al ancho pedido (JPEG) y la guarda en disco (`<data>/img-cache`). Existe
//! porque metahub solo tiene fondos de 480 px o de 1920 px (~800 KB, ~8 MB decodificado): para
//! una tarjeta de 320 px o la TV box, 1920 es mucho bajar y mucho decodificar. Los pósters no
//! pasan por acá (metahub ya los da chicos en WebP).
//!
//! - Solo https de `*.metahub.space` (no es un proxy abierto; los redirects también se revisan).
//! - Anchos fijos (`WIDTHS`): se usa el primero que alcanza, así la caché no se llena de variantes.
//! - Si la original ya es chica, tiene transparencia o no se puede decodificar, se sirve tal cual.
//! - Caché con tope (se borran las menos usadas) y `Cache-Control` largo: la URL no cambia nunca.
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, SystemTime},
};

use axum::{
    body::Bytes,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use image::{codecs::jpeg::JpegEncoder, imageops::FilterType, ImageReader};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

use crate::{
    config::Mode,
    error::{ApiError, ApiResult, Query},
    state::AppState,
};

/// Anchos que se generan. 1280 es el tope: más que eso, la original de 1920.
pub const WIDTHS: [u32; 5] = [320, 480, 640, 960, 1280];
const MAX_SOURCE_BYTES: usize = 12 * 1024 * 1024;
const JPEG_QUALITY: u8 = 80;
const CACHE_CONTROL: &str = "private, max-age=2592000, immutable";

pub struct Images {
    http: reqwest::Client,
    dir: PathBuf,
    max_bytes: u64,
    /// Achicar es CPU pura: pocas a la vez, para no competir con el resto del server.
    work: Semaphore,
    writes: AtomicU32,
}

impl Images {
    pub fn new(data_dir: &Path, mode: Mode) -> anyhow::Result<Arc<Self>> {
        let http = mythos_media::http_client_builder()
            .user_agent(concat!("Mythos/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(6))
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::custom(|a| {
                if a.previous().len() < 5 && allowed_url(a.url().as_str()) {
                    a.follow()
                } else {
                    a.stop()
                }
            }))
            .build()?;
        let (max_mb, workers) = match mode {
            Mode::Server => (512, 2),
            // En el celular o la TV box: menos disco y de a una.
            Mode::Local => (96, 1),
        };
        Ok(Arc::new(Self {
            http,
            dir: data_dir.join("img-cache"),
            max_bytes: max_mb * 1024 * 1024,
            work: Semaphore::new(workers),
            writes: AtomicU32::new(0),
        }))
    }
}

pub fn media_routes() -> Router<AppState> {
    Router::new().route("/img", get(resized))
}

/// Solo https de metahub (fondos, logos y miniaturas de episodios de Cinemeta).
pub fn allowed_url(u: &str) -> bool {
    let Ok(url) = url::Url::parse(u) else { return false };
    let Some(host) = url.host_str() else { return false };
    url.scheme() == "https" && url.port().is_none() && (host == "metahub.space" || host.ends_with(".metahub.space"))
}

/// El primer ancho fijo que alcanza para `w` (o el más grande).
pub fn snap_width(w: u32) -> u32 {
    WIDTHS.iter().copied().find(|&x| x >= w).unwrap_or(WIDTHS[WIDTHS.len() - 1])
}

#[derive(Deserialize)]
struct ImgQuery {
    u: String,
    w: u32,
}

async fn resized(State(st): State<AppState>, Query(q): Query<ImgQuery>) -> ApiResult<Response> {
    if !allowed_url(&q.u) {
        return Err(ApiError::bad_request("Image URL is not from an allowed host"));
    }
    let img = st.images.clone();
    let w = snap_width(q.w);
    let file = img.dir.join(format!("{}-{w}", hex32(&q.u)));
    if let Ok(bytes) = tokio::fs::read(&file).await {
        // Tocar la fecha: la limpieza borra primero las que hace más que no se piden.
        let f = file.clone();
        tokio::task::spawn_blocking(move || touch(&f));
        return Ok(reply(Bytes::from(bytes)));
    }
    let res = img.http.get(&q.u).send().await.map_err(|e| {
        tracing::warn!("imagen {}: {e}", q.u);
        ApiError::bad_gateway("Could not download the image").with_code("img_download")
    })?;
    if !res.status().is_success() {
        return Err(ApiError::bad_gateway(format!("Image download failed ({})", res.status())).with_code("img_download"));
    }
    if res.content_length().is_some_and(|l| l as usize > MAX_SOURCE_BYTES) {
        return Err(ApiError::bad_gateway("Image is too large").with_code("img_too_large"));
    }
    let raw = res.bytes().await.map_err(|_| ApiError::bad_gateway("Could not download the image").with_code("img_download"))?;
    if raw.len() > MAX_SOURCE_BYTES {
        return Err(ApiError::bad_gateway("Image is too large").with_code("img_too_large"));
    }
    let _permit = img.work.acquire().await.map_err(ApiError::internal)?;
    let out = {
        let raw = raw.clone();
        tokio::task::spawn_blocking(move || shrink(&raw, w)).await.map_err(ApiError::internal)?
    };
    drop(_permit);
    // Si no se pudo (o no hacía falta) achicar, va la original: también se guarda, así la
    // próxima vez no se baja de nuevo.
    let bytes = out.map(Bytes::from).unwrap_or(raw);
    let (b, imgs) = (bytes.clone(), img.clone());
    tokio::task::spawn_blocking(move || imgs.store(&file, &b));
    Ok(reply(bytes))
}

fn reply(bytes: Bytes) -> Response {
    // El tipo sale de los primeros bytes (JPEG achicado, o la original si se sirvió tal cual).
    let ct = match bytes.as_ref() {
        [0xff, 0xd8, ..] => "image/jpeg",
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        _ => "application/octet-stream",
    };
    ([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, CACHE_CONTROL)], bytes).into_response()
}

/// JPEG de ancho `w`, o `None` si conviene la original (ya es chica, tiene transparencia o no se
/// pudo leer).
pub fn shrink(raw: &[u8], w: u32) -> Option<Vec<u8>> {
    let mut reader = ImageReader::new(Cursor::new(raw)).with_guessed_format().ok()?;
    // Tope de memoria al decodificar (una imagen rara no se come la RAM de la TV box).
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let src = reader.decode().ok()?;
    if src.width() <= w || src.color().has_alpha() {
        return None;
    }
    let h = ((src.height() as u64 * w as u64) / src.width() as u64).max(1) as u32;
    let small = src.resize_exact(w, h, FilterType::Triangle).to_rgb8();
    let mut out = Vec::with_capacity(64 * 1024);
    JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY).encode_image(&small).ok()?;
    // Por las dudas: si el JPEG chico pesa más que la original, la original.
    (out.len() < raw.len()).then_some(out)
}

impl Images {
    /// Guarda en la caché (escritura atómica) y cada tanto la recorta al tope.
    fn store(&self, file: &Path, bytes: &[u8]) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let tmp = file.with_extension("tmp");
        if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, file).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        if self.writes.fetch_add(1, Ordering::Relaxed).is_multiple_of(50) {
            self.trim();
        }
    }

    /// Borra las menos usadas hasta quedar en el 80 % del tope.
    fn trim(&self) {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return };
        let mut files: Vec<(SystemTime, u64, PathBuf)> = rd
            .flatten()
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                m.is_file().then(|| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len(), e.path()))
            })
            .collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        if total <= self.max_bytes {
            return;
        }
        files.sort_by_key(|f| f.0);
        for (_, len, path) in files {
            if total <= self.max_bytes / 10 * 8 {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }
}

fn touch(f: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(f) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// 32 hex del sha256 de la URL (nombre del archivo en la caché).
fn hex32(s: &str) -> String {
    Sha256::digest(s.as_bytes())[..16].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_metahub() {
        assert!(allowed_url("https://images.metahub.space/background/medium/tt0111161/img"));
        assert!(allowed_url("https://episodes.metahub.space/tt0944947/1/1/w780.jpg"));
        assert!(!allowed_url("http://images.metahub.space/x"));
        assert!(!allowed_url("https://metahub.space.evil.com/x"));
        assert!(!allowed_url("https://evil.com/metahub.space"));
        assert!(!allowed_url("https://images.metahub.space:8443/x"));
        assert!(!allowed_url("file:///etc/passwd"));
    }

    #[test]
    fn snaps_widths() {
        assert_eq!(snap_width(1), 320);
        assert_eq!(snap_width(320), 320);
        assert_eq!(snap_width(321), 480);
        assert_eq!(snap_width(700), 960);
        assert_eq!(snap_width(5000), 1280);
    }

    #[test]
    fn shrinks_big_jpeg_and_keeps_small_or_alpha() {
        let mut big = image::RgbImage::new(1920, 1080);
        for (x, y, p) in big.enumerate_pixels_mut() {
            *p = image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x * y) % 256) as u8]);
        }
        let mut raw = Vec::new();
        JpegEncoder::new_with_quality(&mut raw, 95).encode_image(&big).unwrap();
        let out = shrink(&raw, 640).expect("se achica");
        let back = image::load_from_memory(&out).unwrap();
        assert_eq!((back.width(), back.height()), (640, 360));
        // Ya es más chica que lo pedido: la original.
        assert!(shrink(&raw, 1920).is_none());
        // Con transparencia (logos PNG): la original.
        let rgba = image::RgbaImage::new(800, 300);
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(rgba).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        assert!(shrink(&png, 320).is_none());
        // Basura: la original.
        assert!(shrink(b"not an image", 320).is_none());
    }
}
