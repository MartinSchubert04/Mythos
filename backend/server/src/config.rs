//! Configuración del server leída de variables de entorno (ver `deploy/.env.example`).
use std::{net::SocketAddr, path::PathBuf};

use anyhow::Context;
use mythos_media::HwAccel;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub web_dir: PathBuf,
    pub cache_gb: f64,
    pub peer_port: u16,
    pub hwaccel: HwAccel,
    pub admin_user: Option<String>,
    pub admin_password: Option<String>,
    /// Sin barra final, p. ej. `https://torrentio.strem.fun`.
    pub torrentio_url: String,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind = var("MYTHOS_BIND").unwrap_or_else(|| "127.0.0.1:7700".into());
        let hwaccel = match var("MYTHOS_HWACCEL").as_deref().map(str::to_ascii_lowercase).as_deref() {
            None | Some("none") | Some("cpu") => HwAccel::None,
            Some("nvenc") | Some("nvidia") => HwAccel::Nvenc,
            Some(other) => anyhow::bail!("MYTHOS_HWACCEL inválido: {other:?} (usar nvenc o none)"),
        };
        let cache_gb: f64 = match var("MYTHOS_CACHE_GB") {
            Some(v) => v.parse().with_context(|| format!("MYTHOS_CACHE_GB inválido: {v:?}"))?,
            None => 60.0,
        };
        let peer_port: u16 = match var("MYTHOS_PEER_PORT") {
            Some(v) => v.parse().with_context(|| format!("MYTHOS_PEER_PORT inválido: {v:?}"))?,
            None => 42069,
        };
        Ok(Self {
            bind: bind.parse().with_context(|| format!("MYTHOS_BIND inválido: {bind:?}"))?,
            data_dir: var("MYTHOS_DATA").unwrap_or_else(|| "./data".into()).into(),
            web_dir: var("MYTHOS_WEB_DIR").unwrap_or_else(|| "./web".into()).into(),
            cache_gb: cache_gb.max(1.0),
            peer_port,
            hwaccel,
            admin_user: var("MYTHOS_ADMIN_USER"),
            admin_password: var("MYTHOS_ADMIN_PASSWORD"),
            torrentio_url: var("MYTHOS_TORRENTIO_URL")
                .unwrap_or_else(|| "https://torrentio.strem.fun".into())
                .trim_end_matches('/')
                .to_string(),
            ffmpeg: var("MYTHOS_FFMPEG").unwrap_or_else(|| "ffmpeg".into()).into(),
            ffprobe: var("MYTHOS_FFPROBE").unwrap_or_else(|| "ffprobe".into()).into(),
        })
    }

    pub fn cache_limit_bytes(&self) -> u64 {
        (self.cache_gb * 1024.0 * 1024.0 * 1024.0) as u64
    }
}
