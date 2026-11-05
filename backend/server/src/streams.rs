//! `/api/streams`, `/api/prefetch` y `/api/play`: pide los streams a Torrentio, los ordena con el
//! score anti-trabas y le pasa a media la fuente elegida (con los trackers que trajo Torrentio).
use std::{sync::Arc, time::Duration};

use axum::{extract::State, http::StatusCode, routing::{get, post}, Router};
use mythos_media::{ClientCaps, PlayInfo, Source};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    auth::{hex, AuthUser},
    cache::TtlCache,
    cinemeta::{check_id, check_type, runtime_minutes},
    db::now_unix,
    error::{ApiError, ApiResult, Json, Path, Query},
    score::{self, Prefs},
    taste::{self, NewPlay},
    state::{AppState, Watching},
    torrentio::{self, is_info_hash, Quality, Stream, RD_PLACEHOLDER},
    user_data::{self, UserPrefs},
};

/// Config de Torrentio: sin CAM, screeners ni calidad desconocida. El orden da igual, lo
/// reordenamos con nuestro score.
const TORRENTIO_BASE_CONFIG: &str = "qualityfilter=cam,scr,unknown";

/// Datos para reabrir un stream sabiendo solo su `stream_key`.
#[derive(Clone)]
struct StreamSource {
    info_hash: Option<String>,
    file_idx: Option<u32>,
    /// Link de Real-Debrid con la clave reemplazada por `RD_PLACEHOLDER`.
    url: Option<String>,
    trackers: Arc<Vec<String>>,
    /// Lo que se guarda de la reproducción (`taste.rs`).
    facts: Option<Arc<Facts>>,
}

/// Datos del release para el registro de reproducciones.
struct Facts {
    group: Option<String>,
    binge: Option<String>,
    quality: String,
    size_bytes: Option<u64>,
    dual_audio: bool,
}

pub struct StreamCtx {
    http: reqwest::Client,
    base: String,
    /// Respuestas de Torrentio ya parseadas (sin score), 10 min.
    lists: TtlCache<String, Arc<Vec<Stream>>>,
    /// stream_key → fuente (trackers, link), 6 h.
    sources: TtlCache<String, StreamSource>,
}

impl StreamCtx {
    pub fn new(http: reqwest::Client, base: String) -> Self {
        Self {
            http,
            base,
            lists: TtlCache::new(Duration::from_secs(600), 200),
            sources: TtlCache::new(Duration::from_secs(6 * 3600), 20_000),
        }
    }

    async fn fetch(&self, kind: &str, id: &str, rd_key: Option<&str>) -> ApiResult<torrentio::Parsed> {
        let config = match rd_key {
            Some(k) => format!("{TORRENTIO_BASE_CONFIG}|realdebrid={k}"),
            None => TORRENTIO_BASE_CONFIG.to_string(),
        };
        let url = format!("{}/{config}/stream/{kind}/{id}.json", self.base);
        // Torrentio a veces tarda (scrapea en el momento): 20 s antes de rendirse.
        let res = self.http.get(&url).timeout(Duration::from_secs(20)).send().await.map_err(|e| {
            // Sin la URL: con Real-Debrid lleva la clave adentro.
            tracing::warn!("torrentio {kind}/{id}: {}", e.without_url());
            ApiError::bad_gateway("Torrentio is unreachable right now, try again in a moment").with_code("torrentio_down")
        })?;
        if !res.status().is_success() {
            tracing::warn!("torrentio {kind}/{id}: HTTP {}", res.status());
            return Err(ApiError::bad_gateway(format!("Torrentio answered {}, try again in a moment", res.status()))
            .with_code("torrentio_status")
            .with_param("status", res.status().as_u16()));
        }
        let v: serde_json::Value = res
            .json()
            .await
            .map_err(|_| ApiError::bad_gateway("Torrentio sent an invalid response").with_code("torrentio_bad"))?;
        Ok(torrentio::parse_response(&v, rd_key))
    }

    /// Streams sin score, cacheados por título y por usuario de RD (la clave no se guarda en claro).
    async fn list(&self, kind: &str, id: &str, rd_key: Option<&str>) -> ApiResult<Arc<Vec<Stream>>> {
        let who = rd_key.map(|k| hex(&Sha256::digest(k.as_bytes()))[..12].to_string()).unwrap_or_default();
        let cache_key = format!("{kind}/{id}/{who}");
        if let Some(l) = self.lists.get(&cache_key) {
            return Ok(l);
        }
        let mut parsed = match self.fetch(kind, id, rd_key).await {
            Ok(p) => p,
            // Torrentio caído: mejor la lista vieja que nada.
            Err(e) => match self.lists.get_stale(&cache_key) {
                Some(old) => {
                    tracing::warn!("torrentio falló ({}); se usa la lista cacheada", e.message);
                    return Ok(old);
                }
                None => return Err(e),
            },
        };
        if let (Some(err), Some(_)) = (&parsed.debrid_error, rd_key) {
            // Clave de RD inválida o vencida: mejor torrents comunes que una lista vacía.
            tracing::warn!("Real-Debrid devolvió un error ({err}); se usan torrents comunes");
            parsed = self.fetch(kind, id, None).await?;
        }
        for s in &parsed.streams {
            self.sources.insert(
                s.key.clone(),
                StreamSource {
                    info_hash: s.info_hash.clone(),
                    file_idx: s.file_idx,
                    url: s.url.clone(),
                    trackers: Arc::new(s.trackers.clone()),
                    facts: Some(Arc::new(Facts {
                        group: s.group.clone(),
                        binge: s.binge.clone(),
                        quality: format!("{:?}", s.quality),
                        size_bytes: s.size_bytes,
                        dual_audio: taste::dual_audio(s),
                    })),
                },
            );
        }
        let list = Arc::new(parsed.streams);
        self.lists.insert(cache_key, list.clone());
        Ok(list)
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/streams/{type}/{id}", get(streams))
        .route("/prefetch", post(prefetch))
        .route("/play", post(play))
}

/// Lo que el score necesita de la meta de Cinemeta (casi siempre ya está en caché porque la UI
/// abrió la ficha): duración estimada en segundos (si no se sabe, 2 h para películas y 45 min
/// para episodios) y rango de años aceptable. Como mucho espera 2,5 s.
async fn meta_info(st: &AppState, kind: &str, id: &str) -> (f64, Option<(u16, u16)>) {
    let fallback = if kind == "series" { 2700.0 } else { 7200.0 };
    let base_id = id.split(':').next().unwrap_or(id);
    match tokio::time::timeout(Duration::from_millis(2500), st.cinemeta.meta(kind, base_id)).await {
        Ok(Ok(m)) => {
            let duration = m.runtime.as_deref().and_then(runtime_minutes).map(|m| m * 60.0).unwrap_or(fallback);
            let this_year = (1970 + now_unix() / 31_556_952) as u16;
            (duration, m.preview.year.as_deref().and_then(|y| score::year_range(y, this_year)))
        }
        _ => (fallback, None),
    }
}

#[derive(Deserialize)]
struct StreamsQuery {
    /// `ClientCaps` en JSON o lista "h264,hevc,aac,..." (ver `score::parse_caps`).
    caps: Option<String>,
}

async fn streams(
    State(st): State<AppState>,
    u: AuthUser,
    Path((kind, id)): Path<(String, String)>,
    Query(q): Query<StreamsQuery>,
) -> ApiResult<Json<Vec<Stream>>> {
    check_type(&kind)?;
    check_id(&id)?;
    let prefs = user_data::load_prefs(&st.db, u.user.id).await?;
    let uid = u.user.id;
    let history = st.db.call(move |c| Ok(taste::recent(c, uid)?));
    let (list, (duration_s, years), history) =
        tokio::join!(st.streams.list(&kind, &id, prefs.rd_key.as_deref()), meta_info(&st, &kind, &id), history);
    let mut list = (*list?).clone();
    // Lo aprendido de sus reproducciones (sin historial, nada cambia). Un error de la base no
    // deja sin streams.
    let sub_lang = prefs.ui.get("subLang").and_then(|v| v.as_str()).map(str::to_string);
    let personal = history
        .ok()
        .map(|rows| taste::infer(&rows, series_of(&id), sub_lang.as_deref()))
        .filter(|p| !p.is_empty());
    let p = Prefs {
        preferred: prefs.preferred_quality.as_deref().and_then(Quality::parse_pref).unwrap_or(Quality::P1080),
        max_size_bytes: prefs.max_size_gb.map(|g| (g * 1073741824.0) as u64),
        duration_s,
        caps: q.caps.as_deref().and_then(score::parse_caps),
        years,
        direct_only: !st.media.tools().ffmpeg,
        personal,
    };
    let media = &st.media;
    score::rank(
        &mut list,
        |s| s.info_hash.as_deref().is_some_and(|h| media.is_downloaded(h, s.file_idx.map(|i| i as usize))),
        &p,
    );
    Ok(Json(list))
}

/// La serie de un episodio (`tt123:1:2` → `tt123`); una película es su propio id.
fn series_of(id: &str) -> &str {
    id.split(':').next().unwrap_or(id)
}

/// Arma la `Source` de media a partir de lo que mandó la UI y lo que quedó cacheado del listado.
fn resolve_source(
    ctx: &StreamCtx,
    prefs: &UserPrefs,
    stream_key: Option<&str>,
    info_hash: Option<&str>,
    file_idx: Option<u32>,
    url: Option<&str>,
) -> ApiResult<Source> {
    let cached = stream_key.and_then(|k| ctx.sources.get(&k.to_string()));
    // Sin datos explícitos, una key "<hash>[:<idx>]" alcanza para abrir el torrent.
    let from_key = stream_key.and_then(|k| {
        let (h, i) = k.split_once(':').map(|(h, i)| (h, i.parse().ok())).unwrap_or((k, None));
        is_info_hash(h).then(|| (h.to_ascii_lowercase(), i))
    });
    let url = url
        .map(str::to_string)
        .or_else(|| cached.as_ref().and_then(|c| c.url.clone()))
        .filter(|u| !u.is_empty());
    if let Some(u) = url {
        let real = if u.contains(RD_PLACEHOLDER) {
            let key = prefs.rd_key.as_deref().ok_or_else(|| ApiError::bad_request("Real-Debrid key is not set").with_code("rd_missing"))?;
            u.replace(RD_PLACEHOLDER, key)
        } else {
            u
        };
        // Solo links que vienen de Torrentio: el server no es un proxy abierto.
        if !real.starts_with(&format!("{}/", ctx.base)) {
            return Err(ApiError::bad_request("Only Torrentio / Real-Debrid links can be played").with_code("only_torrentio"));
        }
        return Ok(Source::Url(real));
    }
    let hash = info_hash
        .map(str::to_ascii_lowercase)
        .or_else(|| cached.as_ref().and_then(|c| c.info_hash.clone()))
        .or_else(|| from_key.as_ref().map(|(h, _)| h.clone()))
        .ok_or_else(|| ApiError::bad_request("Unknown stream, reload the stream list").with_code("unknown_stream"))?;
    if !is_info_hash(&hash) {
        return Err(ApiError::bad_request("info_hash must be 40 hex characters"));
    }
    let file_idx = file_idx
        .or_else(|| cached.as_ref().and_then(|c| c.file_idx))
        .or_else(|| from_key.and_then(|(h, i)| (h == hash).then_some(i).flatten()));
    let trackers = cached.map(|c| (*c.trackers).clone()).unwrap_or_default();
    Ok(Source::Torrent { info_hash: hash, file_idx: file_idx.map(|i| i as usize), trackers })
}

/// Saca la clave de Real-Debrid de un texto (los errores de media pueden traer la URL).
fn redact(msg: &str, prefs: &UserPrefs) -> String {
    match prefs.rd_key.as_deref() {
        Some(k) if !k.is_empty() => msg.replace(k, RD_PLACEHOLDER),
        _ => msg.to_string(),
    }
}

#[derive(Deserialize)]
struct PrefetchBody {
    info_hash: Option<String>,
    file_idx: Option<u32>,
    url: Option<String>,
    stream_key: Option<String>,
    /// Precarga del próximo episodio: la sesión que se está viendo (ver `Media::prefetch_next`).
    session_id: Option<String>,
}

async fn prefetch(State(st): State<AppState>, u: AuthUser, Json(b): Json<PrefetchBody>) -> ApiResult<StatusCode> {
    let prefs = user_data::load_prefs(&st.db, u.user.id).await?;
    let src = resolve_source(&st.streams, &prefs, b.stream_key.as_deref(), b.info_hash.as_deref(), b.file_idx, b.url.as_deref())?;
    if let Some(sid) = b.session_id.as_deref().filter(|s| !s.is_empty()) {
        // Sin la sesión (venció o se cerró) no hay a quién atar la precarga: 404 como las rutas
        // de media.
        if st.media.status(sid).is_none() {
            return Err(ApiError::not_found("unknown or expired media session").with_code("media_session"));
        }
        st.media.prefetch_next(sid, src).map_err(|e| ApiError::bad_gateway(redact(&format!("Could not prefetch: {e}"), &prefs)))?;
        return Ok(StatusCode::ACCEPTED);
    }
    st.media.prefetch(src).await.map_err(|e| {
        tracing::warn!("prefetch: {}", redact(&format!("{e:#}"), &prefs));
        ApiError::bad_gateway(redact(&format!("Could not prefetch: {e}"), &prefs))
    })?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
struct PlayBody {
    stream_key: String,
    info_hash: Option<String>,
    file_idx: Option<u32>,
    url: Option<String>,
    #[serde(default)]
    caps: ClientCaps,
    meta_id: Option<String>,
    /// Elegido a mano (Choose stream, el panel de streams del reproductor). Para aprender qué
    /// release prefiere (`taste.rs`).
    #[serde(default)]
    manual: bool,
    /// El que la app ponía primero para este título.
    top_key: Option<String>,
    /// Se eligió desde el cartel de "trabado": el anterior del mismo episodio salió mal.
    #[serde(default)]
    after_stall: bool,
}

async fn play(State(st): State<AppState>, u: AuthUser, Json(b): Json<PlayBody>) -> ApiResult<Json<PlayInfo>> {
    let prefs = user_data::load_prefs(&st.db, u.user.id).await?;
    let src = resolve_source(&st.streams, &prefs, Some(&b.stream_key), b.info_hash.as_deref(), b.file_idx, b.url.as_deref())?;
    let info = st.media.open(src, b.caps).await.map_err(|e| {
        tracing::warn!(user = %u.user.username, key = %b.stream_key, "play: {}", redact(&format!("{e:#}"), &prefs));
        let detail = redact(&format!("{e}"), &prefs);
        let err = ApiError::bad_gateway(format!("Could not start playback: {detail}"));
        // Los de media que la UI conoce (sin peers, sin espacio) llevan su código; el resto, el
        // genérico con el detalle (en inglés) como dato.
        match e.downcast_ref::<mythos_media::Coded>() {
            Some(c) => {
                let mut err = err.with_code(c.code);
                err.params = c.params.clone();
                err
            }
            None => err.with_code("play_failed").with_param("e", detail),
        }
    })?;
    tracing::info!(user = %u.user.username, meta = ?b.meta_id, key = %b.stream_key, mode = ?info.mode, "play");
    if let Some(meta_id) = b.meta_id.filter(|m| !m.is_empty() && m.len() <= 64) {
        let previous = st.watching.lock().unwrap().insert(
            u.user.id,
            Watching {
                username: u.user.username.clone(),
                meta_id: meta_id.clone(),
                stream_key: b.stream_key.clone(),
                session_id: info.session_id.clone(),
                started_at: now_unix(),
            },
        );
        // Cambió de stream para el mismo título: se cierra la sesión vieja para que su ffmpeg y su
        // ventana de descarga no le roben ancho de banda a la nueva. (Con otro título no se toca:
        // puede estar viendo otra cosa en otro dispositivo.)
        if let Some(prev) = previous.filter(|p| p.meta_id == meta_id && p.session_id != info.session_id) {
            st.media.close(&prev.session_id).await;
        }
        // Registro para aprender qué release prefiere (de fondo: no demora el video).
        let facts = st.streams.sources.get(&b.stream_key).and_then(|s| s.facts);
        let langs = |t: &[mythos_media::Track]| {
            let mut v: Vec<String> = t.iter().filter_map(|x| x.lang.as_deref().and_then(taste::norm_lang)).collect();
            v.sort();
            v.dedup();
            v
        };
        let rec = NewPlay {
            user_id: u.user.id,
            series_id: series_of(&meta_id).to_string(),
            meta_id,
            stream_key: b.stream_key.clone(),
            group: facts.as_ref().and_then(|f| f.group.clone()),
            binge: facts.as_ref().and_then(|f| f.binge.clone()),
            quality: facts.as_ref().map(|f| f.quality.clone()),
            size_bytes: facts.as_ref().and_then(|f| f.size_bytes).or(Some(info.size_bytes)),
            dual_audio: facts.as_ref().is_some_and(|f| f.dual_audio) || langs(&info.audio_tracks).len() > 1,
            audio_langs: langs(&info.audio_tracks),
            sub_langs: langs(&info.subtitle_tracks),
            manual: b.manual,
            top_key: b.top_key.filter(|k| k.len() <= 128),
            after_stall: b.after_stall,
        };
        let db = st.db.clone();
        tokio::spawn(async move {
            if let Err(e) = db.call(move |c| Ok(taste::record(c, &rec, crate::db::now_ms())?)).await {
                tracing::warn!("plays: {}", e.message);
            }
        });
    }
    Ok(Json(info))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> StreamCtx {
        StreamCtx::new(reqwest::Client::new(), "https://torrentio.strem.fun".into())
    }

    const H: &str = "08ada5a7a6183aae1e09d831df6748d566095a10";

    #[test]
    fn torrent_from_key_and_cache() {
        let c = ctx();
        let p = UserPrefs::default();
        // Solo con la key "<hash>:<idx>".
        match resolve_source(&c, &p, Some(&format!("{H}:3")), None, None, None).unwrap() {
            Source::Torrent { info_hash, file_idx, trackers } => {
                assert_eq!(info_hash, H);
                assert_eq!(file_idx, Some(3));
                assert!(trackers.is_empty());
            }
            _ => panic!("esperaba torrent"),
        }
        // Con trackers cacheados del listado.
        c.sources.insert(
            H.to_string(),
            StreamSource { info_hash: Some(H.into()), file_idx: None, url: None, trackers: Arc::new(vec!["udp://t:1".into()]), facts: None },
        );
        match resolve_source(&c, &p, Some(H), None, None, None).unwrap() {
            Source::Torrent { trackers, file_idx, .. } => {
                assert_eq!(trackers, ["udp://t:1"]);
                assert_eq!(file_idx, None);
            }
            _ => panic!("esperaba torrent"),
        }
        assert!(matches!(resolve_source(&c, &p, Some("zzz"), None, None, None), Err(e) if e.status == StatusCode::BAD_REQUEST));
        assert!(resolve_source(&c, &p, Some("x"), Some("nothex"), None, None).is_err());
    }

    #[test]
    fn urls_only_from_torrentio_and_key_restored() {
        let c = ctx();
        let mut p = UserPrefs::default();
        let rd = format!("https://torrentio.strem.fun/resolve/realdebrid/{RD_PLACEHOLDER}/{H}/null/0/x.mkv");
        // Sin clave de RD guardada no se puede reponer.
        assert!(resolve_source(&c, &p, Some("url:abc"), None, None, Some(&rd)).is_err());
        p.rd_key = Some("SECRETKEY".into());
        match resolve_source(&c, &p, Some("url:abc"), None, None, Some(&rd)).unwrap() {
            Source::Url(u) => assert!(u.contains("/SECRETKEY/") && !u.contains(RD_PLACEHOLDER)),
            _ => panic!("esperaba url"),
        }
        for bad in ["https://evil.com/x.mkv", "https://torrentio.strem.fun.evil.com/x", "file:///etc/passwd"] {
            assert!(resolve_source(&c, &p, Some("url:abc"), None, None, Some(bad)).is_err(), "{bad}");
        }
        assert_eq!(redact("GET https://t/SECRETKEY/x failed", &p), format!("GET https://t/{RD_PLACEHOLDER}/x failed"));
    }
}
