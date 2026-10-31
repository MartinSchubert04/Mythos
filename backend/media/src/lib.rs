//! `mythos-media`: motor de reproducción del server.
//!
//! Qué hace: baja torrents con librqbit (solo el archivo elegido), los entrega por HTTP con `Range`
//! mientras se bajan, mira los códecs con ffprobe y, si el cliente no puede reproducir el formato,
//! los pasa a HLS en vivo con ffmpeg (remux o transcode con NVENC). También acepta links HTTP
//! directos (Real-Debrid) con el mismo flujo.
//!
//! La API pública de este archivo es el contrato con `backend/server` (ver `docs/API.md`): se puede
//! agregar, nunca sacar ni renombrar.
//!
//! Decisiones anti-trabas (detalle en cada módulo):
//! - `window`: ventana de prioridad deslizante y adaptativa desde la posición de lectura, más
//!   descarga secuencial en segundo plano desde ahí hasta el final.
//! - `engine`: principio y final del archivo primero (moov/cues), DHT y sesión persistentes,
//!   trackers extra, puerto fijo TCP+uTP, UPnP, torrents vivos un rato tras cerrar.
//! - `stream`: ffmpeg/ffprobe leen por un endpoint local, así la prioridad de piezas los alcanza.
//! - `hls`: segmentos que esperan a existir en vez de dar 404.

mod cache;
mod engine;
mod hls;
mod http;
mod probe;
mod session;
mod stream;
mod subs;
mod util;
mod window;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HwAccel {
    Nvenc,
    None,
}

#[derive(Clone, Debug)]
pub struct MediaConfig {
    /// torrents en `data_dir/torrents`, HLS temporal en `data_dir/hls`.
    pub data_dir: PathBuf,
    /// LRU: se borran los torrents menos usados que no estén en uso.
    pub cache_limit_bytes: u64,
    /// Puerto de peers (TCP y uTP/UDP).
    pub peer_port: u16,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub hwaccel: HwAccel,
    /// Bajar la lista de trackers públicos al arrancar.
    pub extra_trackers: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ClientCaps {
    #[serde(default)]
    pub h264: bool,
    #[serde(default)]
    pub hevc: bool,
    #[serde(default)]
    pub av1: bool,
    #[serde(default)]
    pub vp9: bool,
    #[serde(default)]
    pub mkv: bool,
    #[serde(default)]
    pub aac: bool,
    #[serde(default)]
    pub ac3: bool,
    #[serde(default)]
    pub eac3: bool,
    #[serde(default)]
    pub dts: bool,
    #[serde(default)]
    pub opus: bool,
    #[serde(default)]
    pub flac: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Track {
    pub index: usize,
    pub codec: String,
    pub lang: Option<String>,
    pub title: Option<String>,
    pub default: Option<bool>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlayMode {
    Direct,
    Remux,
    Transcode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayInfo {
    pub session_id: String,
    pub mode: PlayMode,
    pub direct_url: String,
    pub hls_url: Option<String>,
    pub file_name: String,
    pub size_bytes: u64,
    pub duration_s: Option<f64>,
    pub container: Option<String>,
    pub video_codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub hdr: Option<bool>,
    pub audio_tracks: Vec<Track>,
    pub subtitle_tracks: Vec<Track>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TorrentStat {
    pub info_hash: String,
    pub name: String,
    pub size_bytes: u64,
    pub downloaded_bytes: u64,
    pub down_bps: u64,
    pub up_bps: u64,
    pub peers: u32,
    pub state: String,
    pub last_used_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MediaStats {
    pub cache_bytes: u64,
    pub cache_limit_bytes: u64,
    pub disk_free_bytes: u64,
    pub active_sessions: u32,
}

/// Estado de una sesión de reproducción (`GET /api/media/{session}/status`).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MediaState {
    Metadata,
    Probing,
    Buffering,
    Ready,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaStatus {
    pub state: MediaState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub down_bps: u64,
    pub up_bps: u64,
    /// Peers conectados.
    pub peers: u32,
    /// Del archivo elegido.
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    /// 0..1 del archivo.
    pub progress: f64,
    /// Segundos listos después de la posición de lectura actual, si se sabe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffered_ahead_s: Option<f64>,
    /// Extra (no está en el contrato original): cuántos segundos de buffer se esperan antes de
    /// pasar a `ready`, para que la UI pueda mostrar una barra de "cargando".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_target_s: Option<f64>,
}

pub enum Source {
    Torrent {
        info_hash: String,
        file_idx: Option<usize>,
        trackers: Vec<String>,
    },
    Url(String),
}

/// Fachada barata de clonar (todo vive en un `Arc`).
#[derive(Clone)]
pub struct Media {
    inner: Arc<engine::Engine>,
}

impl Media {
    pub async fn new(cfg: MediaConfig) -> anyhow::Result<Self> {
        Ok(Self {
            inner: engine::Engine::start(cfg).await?,
        })
    }

    pub async fn open(&self, src: Source, caps: ClientCaps) -> anyhow::Result<PlayInfo> {
        self.inner.open(src, caps).await
    }

    /// Rutas relativas a `/api/media`, SIN auth (la pone el server).
    pub fn router(&self) -> axum::Router {
        http::public_router(self.inner.clone())
    }

    pub async fn torrents(&self) -> Vec<TorrentStat> {
        self.inner.torrents()
    }

    pub async fn remove_torrent(&self, info_hash: &str, delete_files: bool) -> anyhow::Result<()> {
        self.inner.remove_torrent(info_hash, delete_files).await
    }

    pub async fn stats(&self) -> MediaStats {
        self.inner.stats()
    }

    /// No bloquea: dispara la precarga (principio y final del archivo) y vuelve.
    pub async fn prefetch(&self, src: Source) -> anyhow::Result<()> {
        self.inner.prefetch(src)
    }

    /// Para `Stream.cached`: el archivo ya está completo en disco.
    pub fn is_downloaded(&self, info_hash: &str, file_idx: Option<usize>) -> bool {
        self.inner.is_downloaded(info_hash, file_idx)
    }

    /// Extra: estado de una sesión (lo mismo que sirve `/{session}/status`).
    pub fn status(&self, session_id: &str) -> Option<MediaStatus> {
        self.inner.session(session_id).map(|s| s.status())
    }

    /// Extra: cierra una sesión (lo mismo que `DELETE /{session}`).
    pub async fn close(&self, session_id: &str) {
        self.inner.close_session(session_id).await
    }
}
