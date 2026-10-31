//! Binario `mythos` (Docker / server de casa): lee la configuración del entorno (ver
//! `deploy/.env.example`), arranca la biblioteca (`mythos::run`) y espera Ctrl+C / SIGTERM.
//! También tiene comandos de mantenimiento:
//!   mythos adduser <usuario> <contraseña> [--admin]
//!   mythos passwd  <usuario> <contraseña>
//!   mythos health                 (para el healthcheck de Docker: GET /api/health local)
use std::{net::SocketAddr, time::Duration};

use anyhow::Context;
use mythos::{Config, HwAccel};
use tracing_subscriber::EnvFilter;

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
    let cfg = config_from_env()?;
    if !args.is_empty() {
        if let Err(e) = mythos::cli(&cfg, &args).await {
            eprintln!("{e:#}");
            std::process::exit(2);
        }
        return Ok(());
    }

    let mut handle = mythos::run(cfg).await?;
    let stopped_alone = tokio::select! {
        r = handle.stopped() => Some(r),
        _ = shutdown_signal() => None,
    };
    match stopped_alone {
        Some(r) => r,
        None => {
            tracing::info!("apagando…");
            handle.shutdown().await
        }
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// La configuración desde las variables `MYTHOS_*` (los valores por defecto son los de
/// `Config::server`).
fn config_from_env() -> anyhow::Result<Config> {
    let mut cfg = Config::server(var("MYTHOS_DATA").unwrap_or_else(|| "./data".into()));
    let bind = var("MYTHOS_BIND").unwrap_or_else(|| "127.0.0.1:7700".into());
    cfg.bind = bind.parse().with_context(|| format!("MYTHOS_BIND inválido: {bind:?}"))?;
    // Ademas de MYTHOS_BIND (p. ej. la IP de casa: `192.168.0.100:7700`). Separadas por coma.
    if let Some(v) = var("MYTHOS_EXTRA_BIND") {
        for a in v.split(',').map(str::trim).filter(|a| !a.is_empty()) {
            cfg.extra_binds.push(a.parse().with_context(|| format!("MYTHOS_EXTRA_BIND inválido: {a:?}"))?);
        }
    }
    cfg.hwaccel = match var("MYTHOS_HWACCEL").as_deref().map(str::to_ascii_lowercase).as_deref() {
        None | Some("none") | Some("cpu") => HwAccel::None,
        Some("nvenc") | Some("nvidia") => HwAccel::Nvenc,
        Some(other) => anyhow::bail!("MYTHOS_HWACCEL inválido: {other:?} (usar nvenc o none)"),
    };
    if let Some(v) = var("MYTHOS_CACHE_GB") {
        let gb: f64 = v.parse().with_context(|| format!("MYTHOS_CACHE_GB inválido: {v:?}"))?;
        cfg.cache_gb = gb.max(1.0);
    }
    if let Some(v) = var("MYTHOS_PEER_PORT") {
        cfg.peer_port = v.parse().with_context(|| format!("MYTHOS_PEER_PORT inválido: {v:?}"))?;
    }
    cfg.web_dir = Some(var("MYTHOS_WEB_DIR").unwrap_or_else(|| "./web".into()).into());
    cfg.admin_user = var("MYTHOS_ADMIN_USER");
    cfg.admin_password = var("MYTHOS_ADMIN_PASSWORD");
    cfg.dev_users = var("MYTHOS_DEV").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    if let Some(t) = var("MYTHOS_TORRENTIO_URL") {
        cfg.torrentio_url = t.trim_end_matches('/').to_string();
    }
    if let Some(f) = var("MYTHOS_FFMPEG") {
        cfg.ffmpeg = f.into();
    }
    if let Some(f) = var("MYTHOS_FFPROBE") {
        cfg.ffprobe = f.into();
    }
    // `MYTHOS_METRICS=0` apaga las métricas de requests (por defecto prendidas).
    cfg.metrics = !var("MYTHOS_METRICS").is_some_and(|v| v == "0" || v.eq_ignore_ascii_case("false"));
    // `MYTHOS_IMDB=0` apaga "Top rated" (no baja los datasets de IMDb).
    cfg.imdb_top = !var("MYTHOS_IMDB").is_some_and(|v| v == "0" || v.eq_ignore_ascii_case("false"));
    Ok(cfg)
}

/// `mythos health`: pide `/api/health` al server local (sin curl en la imagen). Exit 1 si falla.
async fn health_check() -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let bind: SocketAddr = std::env::var("MYTHOS_BIND").unwrap_or_else(|_| "127.0.0.1:7700".into()).parse()?;
    let addr = SocketAddr::from(([127, 0, 0, 1], bind.port()));
    let check = async {
        let mut s = tokio::net::TcpStream::connect(addr).await?;
        s.write_all(b"GET /api/health HTTP/1.0\r\nHost: localhost\r\n\r\n").await?;
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
