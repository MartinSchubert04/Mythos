//! STUB: lo reemplaza el agente de media. Solo fija la API publica de docs/API.md para que
//! el server compile mientras tanto.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HwAccel { Nvenc, None }

#[derive(Clone, Debug)]
pub struct MediaConfig {
    pub data_dir: PathBuf,
    pub cache_limit_bytes: u64,
    pub peer_port: u16,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub hwaccel: HwAccel,
    pub extra_trackers: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ClientCaps {
    #[serde(default)] pub h264: bool, #[serde(default)] pub hevc: bool,
    #[serde(default)] pub av1: bool, #[serde(default)] pub vp9: bool,
    #[serde(default)] pub mkv: bool, #[serde(default)] pub aac: bool,
    #[serde(default)] pub ac3: bool, #[serde(default)] pub eac3: bool,
    #[serde(default)] pub dts: bool, #[serde(default)] pub opus: bool,
    #[serde(default)] pub flac: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track { pub index: usize, pub codec: String, pub lang: Option<String>, pub title: Option<String>, pub default: Option<bool> }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayMode { Direct, Remux, Transcode }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayInfo {
    pub session_id: String, pub mode: PlayMode,
    pub direct_url: String, pub hls_url: Option<String>,
    pub file_name: String, pub size_bytes: u64, pub duration_s: Option<f64>,
    pub container: Option<String>, pub video_codec: Option<String>,
    pub width: Option<u32>, pub height: Option<u32>, pub hdr: Option<bool>,
    pub audio_tracks: Vec<Track>, pub subtitle_tracks: Vec<Track>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TorrentStat {
    pub info_hash: String, pub name: String, pub size_bytes: u64, pub downloaded_bytes: u64,
    pub down_bps: u64, pub up_bps: u64, pub peers: u32, pub state: String, pub last_used_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MediaStats { pub cache_bytes: u64, pub cache_limit_bytes: u64, pub disk_free_bytes: u64, pub active_sessions: u32 }

pub enum Source { Torrent { info_hash: String, file_idx: Option<usize>, trackers: Vec<String> }, Url(String) }

#[derive(Clone)]
pub struct Media { cfg: std::sync::Arc<MediaConfig> }

impl Media {
    pub async fn new(cfg: MediaConfig) -> anyhow::Result<Self> { Ok(Self { cfg: std::sync::Arc::new(cfg) }) }
    pub async fn open(&self, _src: Source, _caps: ClientCaps) -> anyhow::Result<PlayInfo> { anyhow::bail!("media stub") }
    pub fn router(&self) -> axum::Router { axum::Router::new() }
    pub async fn torrents(&self) -> Vec<TorrentStat> { vec![] }
    pub async fn remove_torrent(&self, _info_hash: &str, _delete_files: bool) -> anyhow::Result<()> { Ok(()) }
    pub async fn stats(&self) -> MediaStats { MediaStats { cache_limit_bytes: self.cfg.cache_limit_bytes, ..Default::default() } }
    pub async fn prefetch(&self, _src: Source) -> anyhow::Result<()> { Ok(()) }
    pub fn is_downloaded(&self, _info_hash: &str, _file_idx: Option<usize>) -> bool { false }
}
