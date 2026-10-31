//! Entrega del archivo por HTTP con `Range`, en streaming (nunca se carga el archivo en RAM: se
//! manda de a bloques de 256 KB con backpressure del socket).
//!
//! Tres formas de leer:
//! - **Torrent con prioridad** (`FileStream` de librqbit): lo usan el reproductor, ffprobe y
//!   ffmpeg. Cada lector registra su posición en la sesión (el "head"), que es lo que mueve la
//!   ventana de prioridad: por eso ffmpeg/ffprobe NO leen el archivo del disco directamente sino
//!   por el endpoint interno, y así esperan las piezas con máxima prioridad en vez de leer basura.
//! - **Torrent pasivo**: lee del disco solo piezas que ya están, sin pedir prioridad. Para
//!   extraer subtítulos (tiene que recorrer todo el archivo) sin robarle ancho de banda a la
//!   película.
//! - **URL** (Real-Debrid): proxy con `Range` al origen, y si la conexión se corta a mitad de
//!   camino se reconecta sola desde el byte donde iba (el reproductor ni se entera).

use crate::session::{PlaySession, TorrentFile};
use crate::util::{self, ByteRange, RangeReq};
use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use librqbit::Api;
use std::io::SeekFrom;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, ReadBuf};

pub const CHUNK: usize = 256 * 1024;

/// Quién lee: decide si mueve el head y si pide prioridad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Reproductor directo: prioridad y mueve el head.
    Player,
    /// ffmpeg de HLS: como `Player`, pero frenado si ya va muy adelantado (ver `hls`).
    Hls,
    /// ffprobe: prioridad, pero no mueve el head (salta al final para leer índices).
    Probe,
    /// Extracción de subtítulos: sin prioridad.
    Passive,
}

impl Role {
    pub fn parse(s: Option<&str>) -> Role {
        match s {
            Some("probe") => Role::Probe,
            Some("subs") => Role::Passive,
            Some("hls") => Role::Hls,
            _ => Role::Player,
        }
    }
}

pub fn json_error(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, axum::Json(serde_json::json!({ "error": msg.into() }))).into_response()
}

/// Lector que avisa a la sesión dónde va (para la ventana de prioridad).
struct Tracked<R> {
    inner: R,
    sess: Arc<PlaySession>,
    gen: u64,
    pos: u64,
    track: bool,
}

impl<R: AsyncRead + Unpin> AsyncRead for Tracked<R> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let r = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &r {
            let n = (buf.filled().len() - before) as u64;
            if n > 0 {
                self.pos += n;
                if self.track {
                    self.sess.advance(self.gen, self.pos);
                }
                self.sess.touch();
            }
        }
        r
    }
}

/// Arma la respuesta 200/206/416 alrededor de un body ya posicionado en `range.start`.
fn ranged_response(name: &str, size: u64, req: &RangeReq, body: Body) -> Response {
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(util::mime_for(name)));
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    let (status, len) = match req {
        RangeReq::Partial(r) => {
            if let Ok(v) = HeaderValue::from_str(&format!("bytes {}-{}/{}", r.start, r.end, size)) {
                h.insert(header::CONTENT_RANGE, v);
            }
            (StatusCode::PARTIAL_CONTENT, r.len())
        }
        _ => (StatusCode::OK, size),
    };
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    (status, h, body).into_response()
}

fn unsatisfiable(size: u64) -> Response {
    let mut r = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("bytes */{size}")) {
        r.headers_mut().insert(header::CONTENT_RANGE, v);
    }
    r
}

/// `GET .../file` para cualquier sesión.
pub async fn serve_file(api: &Api, http: &reqwest::Client, sess: Arc<PlaySession>, range: Option<&str>, role: Role) -> Response {
    let size = sess.size;
    let req = util::parse_range(range, size);
    let r = match &req {
        RangeReq::Unsatisfiable => return unsatisfiable(size),
        RangeReq::Full => ByteRange { start: 0, end: size.saturating_sub(1) },
        RangeReq::Partial(r) => *r,
    };
    if size == 0 {
        return ranged_response(&sess.file_name, 0, &RangeReq::Full, Body::empty());
    }
    let gate = (role == Role::Hls).then(|| sess.clone());
    let body = match &sess.backing {
        crate::session::Backing::Torrent(t) if role == Role::Passive => passive_body(api.clone(), t, r, sess.clone()),
        crate::session::Backing::Torrent(t) => match torrent_body(t, sess.clone(), r, role).await {
            Ok(b) => b,
            Err(e) => return json_error(StatusCode::SERVICE_UNAVAILABLE, format!("stream error: {e:#}")),
        },
        crate::session::Backing::Url(u) => url_body(http.clone(), u.clone(), r, sess.clone(), role),
    };
    let body = match gate {
        Some(s) => throttled(body, s),
        None => body,
    };
    ranged_response(&sess.file_name, size, &req, body)
}

/// Backpressure para ffmpeg: si ya generó demasiado HLS por delante del reproductor, se le dejan
/// de dar bytes hasta que el reproductor avance.
fn throttled(body: Body, sess: Arc<PlaySession>) -> Body {
    use futures::StreamExt;
    let s = body.into_data_stream().then(move |chunk| {
        let sess = sess.clone();
        async move {
            while crate::hls::too_far_ahead(&sess) && !sess.is_closed() {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            chunk
        }
    });
    Body::from_stream(s)
}

async fn torrent_body(t: &TorrentFile, sess: Arc<PlaySession>, r: ByteRange, role: Role) -> anyhow::Result<Body> {
    let mut fs = t.handle.clone().stream(t.idx).await?;
    fs.seek(SeekFrom::Start(r.start)).await?;
    let track = matches!(role, Role::Player | Role::Hls);
    let gen = if track { sess.new_reader(r.start) } else { 0 };
    let reader = Tracked { inner: fs, sess, gen, pos: r.start, track }.take(r.len());
    Ok(Body::from_stream(tokio_util::io::ReaderStream::with_capacity(reader, CHUNK)))
}

/// Lectura sin prioridad: espera a que cada pieza esté y la lee del disco.
fn passive_body(api: Api, t: &TorrentFile, r: ByteRange, sess: Arc<PlaySession>) -> Body {
    struct St {
        api: Api,
        hash: librqbit::dht::Id20,
        geom: crate::window::FileGeom,
        path: std::path::PathBuf,
        file: Option<tokio::fs::File>,
        pos: u64,
        end: u64,
        sess: Arc<PlaySession>,
    }
    let st = St {
        sess,
        api,
        hash: t.handle.info_hash(),
        geom: t.geom,
        path: t.path.clone(),
        file: None,
        pos: r.start,
        end: r.end + 1,
    };
    let s = futures::stream::unfold(st, |mut st| async move {
        if st.pos >= st.end {
            return None;
        }
        let piece = st.geom.piece_at(st.pos);
        // Espera (sin pedir nada) a que la pieza esté verificada.
        loop {
            let ok = crate::window::haves(&st.api, st.hash).and_then(|h| h.get(piece).copied()).unwrap_or(false);
            if ok {
                break;
            }
            if st.sess.is_closed() {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
        if st.file.is_none() {
            match tokio::fs::File::open(&st.path).await {
                Ok(f) => st.file = Some(f),
                Err(e) => return Some((Err(e), st)),
            }
        }
        let n = (st.geom.piece_end(piece).min(st.end) - st.pos).min(CHUNK as u64) as usize;
        let f = st.file.as_mut()?;
        let mut buf = vec![0u8; n];
        let res = async {
            f.seek(SeekFrom::Start(st.pos)).await?;
            f.read_exact(&mut buf).await?;
            Ok::<_, std::io::Error>(())
        }
        .await;
        match res {
            Ok(()) => {
                st.pos += n as u64;
                Some((Ok(Bytes::from(buf)), st))
            }
            Err(e) => Some((Err(e), st)),
        }
    });
    Body::from_stream(s)
}

/// Proxy al origen HTTP con reconexión automática desde el último byte entregado.
fn url_body(http: reqwest::Client, url: String, r: ByteRange, sess: Arc<PlaySession>, role: Role) -> Body {
    struct St {
        http: reqwest::Client,
        url: String,
        pos: u64,
        end: u64,
        resp: Option<reqwest::Response>,
        fails: u32,
        sess: Arc<PlaySession>,
        gen: u64,
        track: bool,
    }
    let track = matches!(role, Role::Player | Role::Hls);
    let gen = if track { sess.new_reader(r.start) } else { 0 };
    let st = St { http, url, pos: r.start, end: r.end, resp: None, fails: 0, sess, gen, track };
    let s = futures::stream::unfold(st, |mut st| async move {
        loop {
            if st.pos > st.end {
                return None;
            }
            if st.resp.is_none() {
                let res = st
                    .http
                    .get(&st.url)
                    .header(header::RANGE, format!("bytes={}-{}", st.pos, st.end))
                    .timeout(Duration::from_secs(3600 * 6))
                    .send()
                    .await;
                match res {
                    Ok(resp) if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT => st.resp = Some(resp),
                    Ok(resp) if resp.status().is_success() && st.pos == 0 => st.resp = Some(resp),
                    other => {
                        st.fails += 1;
                        if st.fails > 6 {
                            let msg = match other {
                                Ok(r) => format!("upstream status {}", r.status()),
                                Err(e) => format!("upstream error: {e}"),
                            };
                            return Some((Err(std::io::Error::other(msg)), st));
                        }
                        tokio::time::sleep(Duration::from_millis(400 * (1 << st.fails.min(4)))).await;
                        continue;
                    }
                }
            }
            let resp = st.resp.as_mut()?;
            match tokio::time::timeout(Duration::from_secs(30), resp.chunk()).await {
                Ok(Ok(Some(mut b))) => {
                    let left = st.end - st.pos + 1;
                    if b.len() as u64 > left {
                        b.truncate(left as usize);
                    }
                    st.pos += b.len() as u64;
                    st.fails = 0;
                    st.sess.add_url_bytes(b.len() as u64);
                    if st.track {
                        st.sess.advance(st.gen, st.pos);
                    }
                    st.sess.touch();
                    return Some((Ok(b), st));
                }
                // Fin prematuro, error o 30 s sin datos: reconectar desde donde íbamos.
                _ => {
                    st.resp = None;
                    st.fails += 1;
                    if st.fails > 6 {
                        return Some((Err(std::io::Error::other("upstream connection lost")), st));
                    }
                }
            }
        }
    });
    Body::from_stream(s)
}

/// Tamaño y nombre de un link directo: `GET` con `Range: bytes=0-0` (algunos CDNs no responden HEAD).
pub async fn url_meta(http: &reqwest::Client, url: &str) -> anyhow::Result<(u64, String)> {
    let resp = http
        .get(url)
        .header(header::RANGE, "bytes=0-0")
        .timeout(Duration::from_secs(20))
        .send()
        .await?;
    let st = resp.status();
    anyhow::ensure!(st.is_success(), "upstream status {st}");
    let h = resp.headers();
    let size = h
        .get(header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .or_else(|| {
            if st == reqwest::StatusCode::OK {
                resp.content_length()
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("upstream did not report a size"))?;
    let name = h
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(filename_from_disposition)
        .unwrap_or_else(|| filename_from_url(url));
    Ok((size, name))
}

pub fn filename_from_disposition(v: &str) -> Option<String> {
    for part in v.split(';') {
        let p = part.trim();
        if let Some(x) = p.strip_prefix("filename*=") {
            let x = x.trim_matches('"');
            let x = x.split_once("''").map(|(_, n)| n).unwrap_or(x);
            return Some(util::pct_decode(x));
        }
    }
    for part in v.split(';') {
        if let Some(x) = part.trim().strip_prefix("filename=") {
            return Some(x.trim_matches('"').to_string());
        }
    }
    None
}

pub fn filename_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.rsplit('/').next().unwrap_or("");
    let n = util::pct_decode(last);
    if n.is_empty() {
        "video".into()
    } else {
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(filename_from_disposition("attachment; filename=\"Movie.mkv\"").as_deref(), Some("Movie.mkv"));
        assert_eq!(filename_from_disposition("attachment; filename*=UTF-8''Pel%C3%ADcula.mp4").as_deref(), Some("Película.mp4"));
        assert_eq!(filename_from_disposition("inline"), None);
        assert_eq!(filename_from_url("https://x.example/d/ABC/Big%20Buck.mp4?x=1"), "Big Buck.mp4");
        assert_eq!(filename_from_url("https://x.example/"), "video");
        assert_eq!(Role::parse(Some("probe")), Role::Probe);
        assert_eq!(Role::parse(None), Role::Player);
    }
}
