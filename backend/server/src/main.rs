//! Binario `mythos`: arma el estado, las rutas (API, media, UI estática) y levanta el server.
//! También tiene comandos de mantenimiento:
//!   mythos adduser <usuario> <contraseña> [--admin]
//!   mythos passwd  <usuario> <contraseña>
//!   mythos health                 (para el healthcheck de Docker: GET /api/health local)
mod admin;
mod auth;
mod cache;
mod cinemeta;
mod config;
mod db;
mod error;
mod score;
mod state;
mod streams;
mod subtitles;
mod torrentio;
mod user_data;
mod web;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::Context;
use axum::{
    http::{header, HeaderName, HeaderValue, Method},
    middleware,
    routing::get,
    Router,
};
use mythos_media::{Media, MediaConfig};
use rusqlite::params;
use serde_json::json;
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

use crate::{
    config::Config,
    db::{now_unix, Db},
    error::ApiError,
    state::AppState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        // Sin colores cuando la salida no es una terminal (logs de Docker).
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "health") {
        return health_check().await;
    }
    let cfg = Config::from_env()?;
    std::fs::create_dir_all(&cfg.data_dir).with_context(|| format!("no se pudo crear {}", cfg.data_dir.display()))?;
    let db = Db::open(&cfg.data_dir.join("mythos.db"))?;

    if !args.is_empty() {
        return cli(&db, &args).await;
    }
    bootstrap_users(&db, &cfg).await?;

    let media = Media::new(MediaConfig {
        data_dir: cfg.data_dir.clone(),
        cache_limit_bytes: cfg.cache_limit_bytes(),
        peer_port: cfg.peer_port,
        ffmpeg: cfg.ffmpeg.clone(),
        ffprobe: cfg.ffprobe.clone(),
        hwaccel: cfg.hwaccel,
        extra_trackers: true,
    })
    .await
    .context("no se pudo iniciar el motor de media")?;

    let http = state::http_client()?;
    let state = AppState {
        auth: auth::Auth::new(db.clone()),
        db: db.clone(),
        media,
        cinemeta: Arc::new(cinemeta::Cinemeta::new(http.clone())),
        streams: Arc::new(streams::StreamCtx::new(http, cfg.torrentio_url.clone())),
        subs: Arc::new(subtitles::Subtitles::new()?),
        watching: Default::default(),
        cfg: Arc::new(cfg.clone()),
    };
    tokio::spawn(cleanup_sessions(db));

    let app = app(state);
    let listener = tokio::net::TcpListener::bind(cfg.bind).await.with_context(|| format!("no se pudo escuchar en {}", cfg.bind))?;
    tracing::info!(
        "Mythos {} escuchando en http://{} (datos: {}, UI: {}, caché: {} GB, hwaccel: {:?})",
        env!("CARGO_PKG_VERSION"),
        cfg.bind,
        cfg.data_dir.display(),
        cfg.web_dir.display(),
        cfg.cache_gb,
        cfg.hwaccel
    );

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let server = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            rx.await.ok();
        });
    let server = std::future::IntoFuture::into_future(server);
    tokio::pin!(server);
    tokio::select! {
        r = &mut server => r?,
        _ = shutdown_signal() => {
            tracing::info!("apagando…");
            tx.send(()).ok();
            // Un <video> abierto mantiene la conexión viva: como mucho se esperan 10 s.
            if tokio::time::timeout(Duration::from_secs(10), &mut server).await.is_err() {
                tracing::warn!("quedaron conexiones abiertas; se cierran a la fuerza");
            }
        }
    }
    Ok(())
}

fn app(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(|| async { axum::Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") })) }))
        .merge(auth::routes())
        .merge(cinemeta::routes())
        .merge(streams::routes())
        .merge(subtitles::routes())
        .merge(user_data::routes())
        .merge(admin::routes())
        // Compresión solo para JSON (y la UI, más abajo): nunca para video ni segmentos HLS.
        .layer(CompressionLayer::new())
        .with_state(state.clone());

    let media = subtitles::media_routes()
        .with_state(state.clone())
        .merge(state.media.router())
        .layer(middleware::from_fn_with_state(state.auth.clone(), auth::media_auth));

    let api = api
        .nest("/media", media)
        .fallback(|| async { ApiError::not_found("Unknown API route") });

    let web = web::router(&state.cfg.web_dir).layer(CompressionLayer::new());

    Router::new()
        .nest("/api", api)
        .fallback_service(web)
        .layer(cors())
        // El span lleva solo el path: la query de /api/media trae el token (`?t=`).
        .layer(TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<_>| {
            tracing::debug_span!("request", method = %req.method(), path = %req.uri().path())
        }))
}

/// CORS para el dev server de Vite y la ventana de Tauri (ver docs/API.md).
fn cors() -> CorsLayer {
    let origins = ["http://localhost:5173", "tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"]
        .map(HeaderValue::from_static);
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::RANGE, header::CONTENT_TYPE])
        .expose_headers([header::CONTENT_RANGE, header::ACCEPT_RANGES, header::CONTENT_LENGTH, HeaderName::from_static("x-mythos-partial")])
        .max_age(Duration::from_secs(3600))
}

/// Primer arranque: crea el admin de `MYTHOS_ADMIN_USER`/`MYTHOS_ADMIN_PASSWORD` si no hay usuarios.
/// Con `MYTHOS_DEV=1` crea además `admin`/`admin` y `test`/`test` (solo para pruebas locales).
async fn bootstrap_users(db: &Db, cfg: &Config) -> anyhow::Result<()> {
    let dev = std::env::var("MYTHOS_DEV").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    if dev {
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

async fn cli(db: &Db, args: &[String]) -> anyhow::Result<()> {
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
        _ => {
            eprintln!("uso:\n  mythos                                    arranca el server\n  mythos adduser <usuario> <contraseña> [--admin]\n  mythos passwd <usuario> <contraseña>");
            std::process::exit(2);
        }
    };
    res.map_err(|e| anyhow::anyhow!(e.message))
}

/// `mythos health`: pide `/api/health` al server local (sin curl en la imagen). Exit 1 si falla.
async fn health_check() -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let bind: SocketAddr = std::env::var("MYTHOS_BIND").unwrap_or_else(|_| "127.0.0.1:7700".into()).parse()?;
    let addr = SocketAddr::from(([127, 0, 0, 1], bind.port()));
    let check = async {
        let mut s = tokio::net::TcpStream::connect(addr).await?;
        s.write_all(b"GET /api/health HTTP/1.0
Host: localhost

").await?;
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await?;
        anyhow::Ok(buf.starts_with(b"HTTP/1.0 200") || buf.starts_with(b"HTTP/1.1 200"))
    };
    match tokio::time::timeout(Duration::from_secs(4), check).await {
        Ok(Ok(true)) => Ok(()),
        _ => {
            eprintln!("mythos no responde en {addr}");
            std::process::exit(1);
        }
    }
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

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}

