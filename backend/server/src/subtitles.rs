//! Subtítulos externos: lista del addon OpenSubtitles v3 (`/api/subtitles/{type}/{id}`) y la ruta
//! `/api/media/subs/external?u=` que baja el .srt (a veces gzip), lo convierte a WebVTT y lo
//! cachea. Solo acepta URLs de dominios de Stremio/OpenSubtitles para no ser un proxy abierto.
use std::{io::Read, sync::Arc, time::Duration};

use axum::{
    body::Bytes,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    auth::AuthUser,
    cache::TtlCache,
    cinemeta::{check_id, check_type, enc},
    error::{ApiError, ApiResult, Json, Path, Query},
    state::AppState,
};

const OPENSUBS: &str = "https://opensubtitles-v3.strem.io";
const MAX_SUB_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Serialize)]
pub struct ExternalSub {
    id: String,
    lang: String,
    url: String,
}

pub struct Subtitles {
    http: reqwest::Client,
    lists: TtlCache<String, Arc<Vec<ExternalSub>>>,
    files: TtlCache<String, Bytes>,
}

impl Subtitles {
    /// Cliente propio: solo sigue redirects a hosts permitidos (si no, un 302 lo convertiría en
    /// proxy abierto).
    pub fn new() -> anyhow::Result<Self> {
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
        Ok(Self {
            http,
            lists: TtlCache::new(Duration::from_secs(6 * 3600), 300),
            files: TtlCache::new(Duration::from_secs(3600), 60),
        })
    }
}

/// Rutas bajo `/api` (con `AuthUser`).
pub fn routes() -> Router<AppState> {
    Router::new().route("/subtitles/{type}/{id}", get(list))
}

/// Ruta bajo `/api/media` (la auth la pone el middleware de media).
pub fn media_routes() -> Router<AppState> {
    Router::new().route("/subs/external", get(external))
}

async fn list(State(st): State<AppState>, _u: AuthUser, Path((kind, id)): Path<(String, String)>) -> ApiResult<Json<Vec<ExternalSub>>> {
    check_type(&kind)?;
    check_id(&id)?;
    let key = format!("{kind}/{id}");
    if let Some(l) = st.subs.lists.get(&key) {
        return Ok(Json((*l).clone()));
    }
    let url = format!("{OPENSUBS}/subtitles/{kind}/{id}.json");
    let res = st.subs.http.get(&url).timeout(Duration::from_secs(12)).send().await.map_err(|e| {
        tracing::warn!("opensubtitles {key}: {e}");
        ApiError::bad_gateway("The subtitles service (OpenSubtitles) is unreachable")
    })?;
    if !res.status().is_success() {
        return Err(ApiError::bad_gateway(format!("The subtitles service answered {}", res.status())));
    }
    let v: Value = res.json().await.map_err(|_| ApiError::bad_gateway("The subtitles service sent an invalid response"))?;
    let subs: Vec<ExternalSub> = v["subtitles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let u = s["url"].as_str()?;
            allowed_url(u).then(|| ExternalSub {
                id: s["id"].as_str().map(str::to_string).unwrap_or_else(|| s["id"].to_string()),
                lang: s["lang"].as_str().unwrap_or("und").to_string(),
                url: format!("/api/media/subs/external?u={}", enc(u)),
            })
        })
        .collect();
    let subs = Arc::new(subs);
    st.subs.lists.insert(key, subs.clone());
    Ok(Json((*subs).clone()))
}

/// Solo https y hosts de Stremio u OpenSubtitles.
pub fn allowed_url(u: &str) -> bool {
    let Ok(url) = url::Url::parse(u) else { return false };
    let Some(host) = url.host_str() else { return false };
    url.scheme() == "https"
        && url.port().is_none()
        && ["strem.io", "opensubtitles.org", "opensubtitles.com"]
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

#[derive(Deserialize)]
struct ExternalQuery {
    u: String,
}

async fn external(State(st): State<AppState>, Query(q): Query<ExternalQuery>) -> ApiResult<Response> {
    if !allowed_url(&q.u) {
        return Err(ApiError::bad_request("Subtitle URL is not from an allowed host"));
    }
    let vtt = match st.subs.files.get(&q.u) {
        Some(v) => v,
        None => {
            let res = st.subs.http.get(&q.u).send().await.map_err(|e| {
                tracing::warn!("subtítulo {}: {e}", q.u);
                ApiError::bad_gateway("Could not download the subtitle")
            })?;
            if !res.status().is_success() {
                return Err(ApiError::bad_gateway(format!("Subtitle download failed ({})", res.status())));
            }
            if res.content_length().is_some_and(|l| l as usize > MAX_SUB_BYTES) {
                return Err(ApiError::bad_gateway("Subtitle file is too large"));
            }
            let raw = res.bytes().await.map_err(|_| ApiError::bad_gateway("Could not download the subtitle"))?;
            let text = decode(&raw)?;
            let vtt = Bytes::from(srt_to_vtt(&text));
            st.subs.files.insert(q.u.clone(), vtt.clone());
            vtt
        }
    };
    Ok((
        [(header::CONTENT_TYPE, "text/vtt; charset=utf-8"), (header::CACHE_CONTROL, "private, max-age=86400")],
        vtt,
    )
        .into_response())
}

/// Descomprime si es gzip y decodifica: UTF-8 si es válido, si no Latin-1/Windows-1252 (lo más
/// común en subtítulos viejos).
fn decode(raw: &[u8]) -> ApiResult<String> {
    let bytes = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(raw)
            .take(MAX_SUB_BYTES as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|_| ApiError::bad_gateway("Subtitle file is corrupted"))?;
        if out.len() > MAX_SUB_BYTES {
            return Err(ApiError::bad_gateway("Subtitle file is too large"));
        }
        out
    } else {
        raw.to_vec()
    };
    Ok(match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
    })
}

/// SRT → WebVTT: cabecera, comas → puntos en los tiempos y fuera los tags de posición de ASS
/// (`{\an8}`) que el navegador mostraría como texto.
pub fn srt_to_vtt(srt: &str) -> String {
    let s = srt.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    if s.trim_start().starts_with("WEBVTT") {
        return s;
    }
    let mut out = String::with_capacity(s.len() + 16);
    out.push_str("WEBVTT\n\n");
    for line in s.lines() {
        if line.contains("-->") {
            out.push_str(&line.replace(',', "."));
        } else {
            let mut l = line.to_string();
            while let (Some(a), Some(b)) = (l.find("{\\"), l.find('}')) {
                if b < a {
                    break;
                }
                l.replace_range(a..=b, "");
            }
            out.push_str(&l);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_srt() {
        let srt = "\u{feff}1\r\n00:00:01,000 --> 00:00:04,000\r\n{\\an8}Hello, world\r\n\r\n2\r\n00:00:05,500 --> 00:00:06,000\r\nBye\r\n";
        let vtt = srt_to_vtt(srt);
        assert!(vtt.starts_with("WEBVTT\n\n1\n00:00:01.000 --> 00:00:04.000\nHello, world\n"));
        assert!(vtt.contains("00:00:05.500 --> 00:00:06.000"));
    }

    #[test]
    fn decodes_gzip_and_latin1() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all("Hola ñandú".as_bytes()).unwrap();
        assert_eq!(decode(&gz.finish().unwrap()).unwrap(), "Hola ñandú");
        assert_eq!(decode(&[b'a', 0xf1, b'o']).unwrap(), "año");
    }

    #[test]
    fn only_allowed_hosts() {
        assert!(allowed_url("https://subs5.strem.io/en/download/subencoding-stremio-utf8/src-api/file/1952537628"));
        assert!(allowed_url("https://dl.opensubtitles.org/x.gz"));
        assert!(!allowed_url("http://subs5.strem.io/x"));
        assert!(!allowed_url("https://evil.com/strem.io"));
        assert!(!allowed_url("https://strem.io.evil.com/x"));
        assert!(!allowed_url("https://subs5.strem.io:8443/x"));
        assert!(!allowed_url("file:///etc/passwd"));
    }
}
