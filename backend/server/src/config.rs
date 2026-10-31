//! Configuración del server, independiente de las variables de entorno: el binario `mythos` la
//! arma desde el entorno (ver `main.rs` y `deploy/.env.example`); las apps (Tauri) la arman en
//! código con `Config::local(...)` para el modo embebido.
use std::{net::SocketAddr, path::PathBuf, time::Duration};

use mythos_media::HwAccel;

/// Cómo corre el server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// El server de casa (Docker): usuarios, admin, métricas, UI.
    Server,
    /// Embebido en la app (desktop/Android): cuentas locales espejo de las del server remoto,
    /// sin admin, login offline y sincronización con el remoto. Ver `docs/local-mode.md`.
    Local,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    /// `127.0.0.1:0` = puerto libre cualquiera (el real lo da `Handle::addr`).
    pub bind: SocketAddr,
    /// Base de datos (`mythos.db`) y caché de torrents.
    pub data_dir: PathBuf,
    /// UI compilada para servir en `/`. `None` = no se sirve (la app trae la UI adentro).
    pub web_dir: Option<PathBuf>,
    pub cache_gb: f64,
    /// Puerto de peers de BitTorrent. 0 = efímero.
    pub peer_port: u16,
    pub hwaccel: HwAccel,
    /// Primer admin si la base está vacía (solo modo server).
    pub admin_user: Option<String>,
    pub admin_password: Option<String>,
    /// Usuarios de prueba admin/admin y test/test (`MYTHOS_DEV=1`, solo modo server).
    pub dev_users: bool,
    /// Sin barra final, p. ej. `https://torrentio.strem.fun`.
    pub torrentio_url: String,
    /// Ruta vacía = sin ffmpeg/ffprobe (solo reproducción directa).
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    /// Bajar la lista de trackers públicos al arrancar.
    pub extra_trackers: bool,
    /// Métricas de requests y auditoría (`/api/admin/metrics`, `/logs`, `/audit`).
    pub metrics: bool,
    /// Modo local: URL del server remoto por defecto (login online y sincronización). Se puede
    /// cambiar en caliente con `POST /api/local/sync` o en el login.
    pub remote_url: Option<String>,
    /// Modo local: cada cuánto sincronizar solo con el remoto. `None` = solo a pedido
    /// (`POST /api/local/sync`).
    pub sync_interval: Option<Duration>,
}

impl Config {
    /// Valores por defecto del server de casa (los mismos que sin variables de entorno).
    pub fn server(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            mode: Mode::Server,
            bind: SocketAddr::from(([127, 0, 0, 1], 7700)),
            data_dir: data_dir.into(),
            web_dir: Some("./web".into()),
            cache_gb: 60.0,
            peer_port: 42069,
            hwaccel: HwAccel::None,
            admin_user: None,
            admin_password: None,
            dev_users: false,
            torrentio_url: "https://torrentio.strem.fun".into(),
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            extra_trackers: true,
            metrics: true,
            remote_url: None,
            sync_interval: None,
        }
    }

    /// Valores para el modo embebido: solo en 127.0.0.1 con puerto libre, sin UI, sin métricas,
    /// caché de 20 GB, puerto de peers efímero y sincronización cada 60 s si hay remoto.
    /// ffmpeg/ffprobe se buscan en el PATH; si no están, solo reproducción directa.
    pub fn local(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            mode: Mode::Local,
            bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            web_dir: None,
            cache_gb: 20.0,
            peer_port: 0,
            metrics: false,
            sync_interval: Some(Duration::from_secs(60)),
            ..Self::server(data_dir)
        }
    }

    pub fn cache_limit_bytes(&self) -> u64 {
        (self.cache_gb.max(1.0) * 1024.0 * 1024.0 * 1024.0) as u64
    }

    pub fn is_local(&self) -> bool {
        self.mode == Mode::Local
    }
}
