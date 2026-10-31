//! `mythos`: el server de Mythos como biblioteca, para correrlo embebido en las apps (Tauri en
//! Windows y Android) además de como binario (`main.rs`, Docker).
//!
//! ```no_run
//! # async fn demo() -> anyhow::Result<()> {
//! let mut cfg = mythos::Config::local("/data/user/0/app/files/mythos");
//! cfg.remote_url = Some("https://server.tailnet.ts.net:7700".into());
//! let handle = mythos::run(cfg).await?;          // escucha en 127.0.0.1:<puerto libre>
//! println!("API local en {}", handle.base_url());
//! // ... la UI le habla a handle.base_url() ...
//! handle.shutdown().await?;                      // cierre ordenado
//! # Ok(()) }
//! ```
//!
//! La API pública es chica a propósito: `Config`/`Mode`, `run`, `Handle` y `cli` (comandos de
//! mantenimiento del binario). Todo lo demás es interno. Contrato HTTP en `docs/API.md`; modo
//! local en `docs/local-mode.md`.
mod admin;
mod auth;
mod avatars;
mod cache;
mod cinemeta;
mod config;
mod db;
mod error;
mod local;
mod metrics;
mod score;
mod state;
mod streams;
mod subtitles;
mod sync;
mod torrentio;
mod user_data;
mod web;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::Context;
use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderName, HeaderValue, Method},
    middleware,
    routing::get,
    Router,
};
use mythos_media::{Media, MediaConfig, MediaTools};
use rusqlite::params;
use serde_json::json;
use tokio::task::JoinHandle;
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};

pub use crate::config::{Config, Mode};
pub use mythos_media::HwAccel;
use crate::{
    db::{now_unix, Db},
    error::ApiError,
    metrics::Recorder,
    state::AppState,
};

/// Server andando. Soltarlo sin `shutdown` deja todo corriendo hasta que termine el runtime.
pub struct Handle {
    addr: SocketAddr,
    mode: Mode,
    tools: MediaTools,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    server: Option<JoinHandle<std::io::Result<()>>>,
    background: Vec<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
    state: Option<AppState>,
}

impl Handle {
    /// Dirección real (con `bind` en puerto 0, el puerto que tocó).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// `http://127.0.0.1:<puerto>` (lo que la UI usa como base de la API).
    pub fn base_url(&self) -> String {
        let host = if self.addr.ip().is_unspecified() { "127.0.0.1".to_string() } else { self.addr.ip().to_string() };
        let host = if host.contains(':') { format!("[{host}]") } else { host };
        format!("http://{host}:{}", self.addr.port())
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// ffmpeg/ffprobe encontrados (sin ffmpeg: solo reproducción directa).
    pub fn media_tools(&self) -> MediaTools {
        self.tools
    }

    /// Espera a que el server termine solo (error de E/S del listener). Normalmente no vuelve.
    pub async fn stopped(&mut self) -> anyhow::Result<()> {
        match self.server.as_mut() {
            Some(s) => {
                let r = s.await;
                self.server = None;
                Ok(r??)
            }
            None => Ok(()),
        }
    }

    /// Cierre ordenado: deja de aceptar conexiones, espera hasta 10 s a las abiertas (un
    /// `<video>` mantiene la suya), frena las tareas de fondo, guarda las métricas pendientes y
    /// apaga el motor de media (suelta puertos y archivos). La base queda cerrada al volver.
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        if let Some(tx) = self.stop.take() {
            tx.send(()).ok();
        }
        if let Some(mut s) = self.server.take() {
            if tokio::time::timeout(Duration::from_secs(10), &mut s).await.is_err() {
                tracing::warn!("quedaron conexiones abiertas; se cierran a la fuerza");
                s.abort();
            }
        }
        for t in self.background.drain(..) {
            t.abort();
            let _ = t.await;
        }
        if let Some(state) = self.state.take() {
            state.media.shutdown().await;
            // Sin el estado (y su Recorder) el canal de métricas se cierra y el escritor vacía lo
            // que tenga.
            drop(state);
        }
        if let Some(w) = self.writer.take() {
            if tokio::time::timeout(Duration::from_secs(5), w).await.is_err() {
                tracing::warn!("metrics: el escritor no terminó a tiempo");
            }
        }
        tracing::info!("Mythos apagado");
        Ok(())
    }
}

/// Arranca el server con `cfg`: abre/migra la base, el motor de media, las tareas de fondo y
/// empieza a escuchar. Vuelve en cuanto escucha (con `Handle::addr` real).
pub async fn run(cfg: Config) -> anyhow::Result<Handle> {
    std::fs::create_dir_all(&cfg.data_dir).with_context(|| format!("no se pudo crear {}", cfg.data_dir.display()))?;
    let db = Db::open(&cfg.data_dir.join("mythos.db"))?;
    if cfg.mode == Mode::Server {
        bootstrap_users(&db, &cfg).await?;
    }

    let media = Media::new(MediaConfig {
        data_dir: cfg.data_dir.clone(),
        cache_limit_bytes: cfg.cache_limit_bytes(),
        cache_max_age_secs: cfg.cache_max_age.map_or(0, |d| d.as_secs()),
        peer_port: cfg.peer_port,
        ffmpeg: cfg.ffmpeg.clone(),
        ffprobe: cfg.ffprobe.clone(),
        hwaccel: cfg.hwaccel,
        extra_trackers: cfg.extra_trackers,
    })
    .await
    .context("no se pudo iniciar el motor de media")?;
    let tools = media.tools();

    let (recorder, writer) = if cfg.metrics {
        let (r, w) = Recorder::start(db.clone());
        (r, Some(tokio::spawn(w)))
    } else {
        (Recorder::disabled(), None)
    };
    let local = match cfg.mode {
        Mode::Local => Some(Arc::new(local::LocalCtx::new(cfg.remote_url.as_deref())?)),
        Mode::Server => None,
    };
    let http = state::http_client()?;
    let state = AppState {
        auth: auth::Auth::with_recorder(db.clone(), recorder.clone()),
        db: db.clone(),
        media,
        cinemeta: Arc::new(cinemeta::Cinemeta::new(http.clone())),
        streams: Arc::new(streams::StreamCtx::new(http, cfg.torrentio_url.clone())),
        subs: Arc::new(subtitles::Subtitles::new()?),
        watching: Default::default(),
        recorder,
        changes: Default::default(),
        local,
        cfg: Arc::new(cfg.clone()),
    };
    let mut background = vec![tokio::spawn(cleanup_sessions(db))];
    if let (Mode::Local, Some(every)) = (cfg.mode, cfg.sync_interval) {
        background.push(tokio::spawn(local::sync_loop(state.clone(), every)));
    }

    let app = app(state.clone());
    let listener = tokio::net::TcpListener::bind(cfg.bind).await.with_context(|| format!("no se pudo escuchar en {}", cfg.bind))?;
    let addr = listener.local_addr()?;
    tracing::info!(
        "Mythos {} ({:?}) escuchando en http://{} (datos: {}, UI: {}, caché: {} GB, hwaccel: {:?}, ffmpeg: {})",
        env!("CARGO_PKG_VERSION"),
        cfg.mode,
        addr,
        cfg.data_dir.display(),
        cfg.web_dir.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "no".into()),
        cfg.cache_gb,
        cfg.hwaccel,
        tools.ffmpeg,
    );

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let server = axum::serve(listener, app.clone().into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(async {
        rx.await.ok();
    });
    let server = tokio::spawn(std::future::IntoFuture::into_future(server));
    // Mismas rutas en las direcciones extra (la red de casa). Se cortan junto con el resto de las
    // tareas de fondo al apagar.
    let mut background = background;
    for extra in &cfg.extra_binds {
        let l = tokio::net::TcpListener::bind(extra).await.with_context(|| format!("no se pudo escuchar en {extra}"))?;
        tracing::info!("también escuchando en http://{}", l.local_addr()?);
        let app = app.clone();
        background.push(tokio::spawn(async move {
            if let Err(e) = axum::serve(l, app.into_make_service_with_connect_info::<SocketAddr>()).await {
                tracing::warn!("listener extra: {e}");
            }
        }));
    }
    Ok(Handle { addr, mode: cfg.mode, tools, stop: Some(tx), server: Some(server), background, writer, state: Some(state) })
}

fn app(state: AppState) -> Router {
    let mode = state.cfg.mode;
    let tools = state.media.tools();
    let mut api = Router::new()
        .route(
            "/health",
            get(move || async move {
                axum::Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION"), "mode": mode, "transcode": tools.ffmpeg }))
            }),
        )
        .merge(cinemeta::routes())
        .merge(streams::routes())
        .merge(subtitles::routes())
        .merge(user_data::routes())
        .merge(sync::routes())
        .merge(avatars::routes());
    api = match mode {
        Mode::Server => api.merge(auth::routes()).merge(admin::routes()).merge(avatars::admin_routes()).merge(metrics::routes()),
        // Login/logout propios (online contra el remoto u offline), sin admin.
        Mode::Local => api.merge(auth::account_routes()).merge(local::routes()),
    };
    let api = api
        // Compresión solo para JSON (y la UI, más abajo): nunca para video ni segmentos HLS.
        .layer(CompressionLayer::new())
        // Ningún cuerpo de la API necesita más (settings: 64 KB, el resto es chico; /sync pone el
        // suyo). 413 si se pasa.
        .layer(DefaultBodyLimit::max(256 * 1024))
        .with_state(state.clone());

    let media = subtitles::media_routes()
        .merge(avatars::media_routes())
        .with_state(state.clone())
        .merge(state.media.router())
        .layer(middleware::from_fn_with_state(state.auth.clone(), auth::media_auth));

    let api = api
        .nest("/media", media)
        .fallback(|| async { ApiError::not_found("Unknown API route") });

    let mut root = Router::new()
        .nest("/api", api)
        // Después del ruteo: anota la plantilla de la ruta para las métricas.
        .route_layer(middleware::from_fn(metrics::note_route));
    if let Some(dir) = &state.cfg.web_dir {
        root = root.fallback_service(web::router(dir).layer(CompressionLayer::new()));
    }
    let mut root = root
        .layer(middleware::map_response(security_headers))
        .layer(middleware::from_fn_with_state(state.recorder.clone(), metrics::track));
    if mode == Mode::Local {
        root = root.layer(middleware::from_fn(local::host_guard));
    }
    root.layer(cors())
        // El span lleva solo el path: la query de /api/media trae el token (`?t=`).
        .layer(TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<_>| {
            tracing::debug_span!("request", method = %req.method(), path = %req.uri().path())
        }))
}

/// Headers de seguridad para todas las respuestas (la CSP la pone `web` en el HTML).
async fn security_headers(mut res: axum::response::Response) -> axum::response::Response {
    let h = res.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    // Solo el origen hacia afuera (YouTube lo necesita para los tráilers), nunca la URL con `?t=`.
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("strict-origin-when-cross-origin"));
    res
}

/// CORS para el dev server de Vite y la ventana de Tauri (ver docs/API.md). `Private Network
/// Access`: la ventana de Tauri (origen `tauri.localhost`) le habla al server local en 127.0.0.1.
fn cors() -> CorsLayer {
    let origins = ["http://localhost:5173", "tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"]
        .map(HeaderValue::from_static);
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::RANGE, header::CONTENT_TYPE])
        .expose_headers([header::CONTENT_RANGE, header::ACCEPT_RANGES, header::CONTENT_LENGTH, HeaderName::from_static("x-mythos-partial")])
        .allow_private_network(true)
        .max_age(Duration::from_secs(3600))
}

/// Primer arranque: crea el admin de `admin_user`/`admin_password` si no hay usuarios. Con
/// `dev_users` crea además `admin`/`admin` y `test`/`test` (solo para pruebas locales).
async fn bootstrap_users(db: &Db, cfg: &Config) -> anyhow::Result<()> {
    if cfg.dev_users {
        let created = db
            .call(|c| {
                let mut created = Vec::new();
                for (name, admin) in [("admin", true), ("test", false)] {
                    // Directo a la tabla: las contraseñas de prueba no cumplen el largo mínimo.
                    let n = c.execute(
                        "INSERT OR IGNORE INTO users (username, password_hash, is_admin) VALUES (?1, ?2, ?3)",
                        params![name, auth::hash_password(name)?, admin],
                    )?;
                    if n > 0 {
                        created.push(name);
                    }
                }
                Ok(created)
            })
            .await
            .map_err(|e| anyhow::anyhow!(e.message))?;
        tracing::warn!("==============================================================");
        tracing::warn!("  MODO DEV (MYTHOS_DEV=1): usuarios de prueba admin/admin y test/test");
        if !created.is_empty() {
            tracing::warn!("  creados ahora: {}", created.join(", "));
        }
        tracing::warn!("  ¡No usar en producción!");
        tracing::warn!("==============================================================");
    }
    let users: i64 = db
        .call(|c| Ok(c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?))
        .await
        .map_err(|e| anyhow::anyhow!(e.message))?;
    if users > 0 {
        return Ok(());
    }
    match (&cfg.admin_user, &cfg.admin_password) {
        (Some(u), Some(p)) => {
            let (u, p) = (u.clone(), p.clone());
            let user = db
                .call(move |c| auth::create_user(c, &u, &p, true))
                .await
                .map_err(|e| anyhow::anyhow!("no se pudo crear el admin inicial: {}", e.message))?;
            tracing::info!("admin inicial creado: {}", user.username);
        }
        _ => {
            tracing::warn!("no hay ningún usuario: definí MYTHOS_ADMIN_USER y MYTHOS_ADMIN_PASSWORD y reiniciá para crear el primer admin");
            tracing::warn!("(o: `mythos adduser <usuario> <contraseña> --admin`)");
        }
    }
    Ok(())
}

/// Comandos de mantenimiento del binario sobre la base de `cfg.data_dir`:
/// `adduser <usuario> <contraseña> [--admin]` y `passwd <usuario> <contraseña>`.
pub async fn cli(cfg: &Config, args: &[String]) -> anyhow::Result<()> {
    std::fs::create_dir_all(&cfg.data_dir).with_context(|| format!("no se pudo crear {}", cfg.data_dir.display()))?;
    let db = Db::open(&cfg.data_dir.join("mythos.db"))?;
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let res = match a.as_slice() {
        ["adduser", user, pass, rest @ ..] => {
            let admin = rest.contains(&"--admin");
            let (u, p) = (user.to_string(), pass.to_string());
            db.call(move |c| auth::create_user(c, &u, &p, admin))
                .await
                .map(|u| println!("usuario creado: {} (admin: {})", u.username, u.is_admin))
        }
        ["passwd", user, pass] => {
            let (u, p) = (user.to_string(), pass.to_string());
            db.call(move |c| {
                auth::validate_password(&p)?;
                let hash = auth::hash_password(&p)?;
                let n = c.execute("UPDATE users SET password_hash = ?2, disabled = 0 WHERE username = ?1", params![u, hash])?;
                if n == 0 {
                    return Err(ApiError::not_found("User not found"));
                }
                c.execute("DELETE FROM sessions WHERE user_id = (SELECT id FROM users WHERE username = ?1)", [u])?;
                Ok(())
            })
            .await
            .map(|_| println!("contraseña cambiada (y usuario habilitado)"))
        }
        _ => anyhow::bail!(
            "uso:\n  mythos                                    arranca el server\n  mythos adduser <usuario> <contraseña> [--admin]\n  mythos passwd <usuario> <contraseña>"
        ),
    };
    res.map_err(|e| anyhow::anyhow!(e.message))
}

/// Borra sesiones vencidas cada hora.
async fn cleanup_sessions(db: Db) {
    let mut tick = tokio::time::interval(Duration::from_secs(3600));
    loop {
        tick.tick().await;
        let r = db.call(|c| Ok(c.execute("DELETE FROM sessions WHERE expires_at < ?1", [now_unix()])?)).await;
        if let Ok(n @ 1..) = r {
            tracing::debug!("{n} sesiones vencidas borradas");
        }
    }
}
