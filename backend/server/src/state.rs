//! Estado compartido de la app (todo `Clone` barato: `Arc`s por dentro).
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::extract::FromRef;
use mythos_media::Media;
use serde::Serialize;

use crate::{
    auth::Auth, cinemeta::Cinemeta, config::Config, db::Db, local::LocalCtx, metrics::Recorder, streams::StreamCtx,
    subtitles::Subtitles,
};

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub db: Db,
    pub auth: Auth,
    pub media: Media,
    pub cinemeta: Arc<Cinemeta>,
    pub streams: Arc<StreamCtx>,
    pub subs: Arc<Subtitles>,
    /// Qué está viendo cada usuario (lo anota `/api/play` con `meta_id`), por id de usuario.
    pub watching: Arc<Mutex<HashMap<i64, Watching>>>,
    /// Métricas y auditoría (deshabilitado en modo local).
    pub recorder: Recorder,
    /// Se avisa en cada cambio sincronizable (progreso, biblioteca, settings): el modo local
    /// sincroniza poco después.
    pub changes: Arc<tokio::sync::Notify>,
    /// Solo en modo local.
    pub local: Option<Arc<LocalCtx>>,
}

impl AppState {
    /// Hubo un cambio sincronizable.
    pub fn changed(&self) {
        self.changes.notify_one();
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Watching {
    pub username: String,
    pub meta_id: String,
    pub stream_key: String,
    pub session_id: String,
    /// unix s
    pub started_at: i64,
}

impl FromRef<AppState> for Auth {
    fn from_ref(s: &AppState) -> Self {
        s.auth.clone()
    }
}

/// Cliente HTTP para los upstreams (Cinemeta, Torrentio, OpenSubtitles).
pub fn http_client() -> anyhow::Result<reqwest::Client> {
    Ok(mythos_media::http_client_builder()
        .user_agent(concat!("Mythos/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(6))
        .timeout(std::time::Duration::from_secs(25))
        .pool_idle_timeout(std::time::Duration::from_secs(60))
        .build()?)
}
