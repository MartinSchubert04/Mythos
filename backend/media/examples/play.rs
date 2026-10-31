//! Ejemplo de prueba manual: levanta `Media` + `router()` en 127.0.0.1:7711 (sin auth) y abre un
//! torrent. Por defecto Sintel (Blender, Creative Commons).
//!
//! ```text
//! cargo run -p mythos-media --example play -- [info_hash] [file_idx] [--caps=mkv,hevc,...]
//! curl http://127.0.0.1:7711/api/media/<session>/status
//! curl -r 0-1023 -o NUL -D - http://127.0.0.1:7711/api/media/<session>/file
//! ```
//! Variables: `MYTHOS_DATA` (carpeta de datos, por defecto `./data/media-example`, ignorada por git), `MYTHOS_HWACCEL`
//! (`nvenc`|`none`), `FFMPEG`/`FFPROBE` (rutas, por defecto se buscan en el PATH),
//! `MYTHOS_DOWN_LIMIT_KB` (tope de bajada) y `MYTHOS_CACHE_GB` (límite de caché, 0 = sin límite).
//! Además expone (para probar con curl) `GET /open?hash=..&idx=..` o `/open?url=..`,
//! `/prefetch?hash=..`, `/torrents`, `/stats`, `/cached?hash=..` y `/remove?hash=..&files=1`.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use mythos_media::{ClientCaps, HwAccel, Media, MediaConfig, Source};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

const SINTEL: &str = "08ada5a7a6183aae1e09d831df6748d566095a10";

fn caps_from(list: &str) -> ClientCaps {
    // Por defecto: lo que reproduce Chrome de escritorio.
    let mut c = ClientCaps { h264: true, vp9: true, av1: true, aac: true, opus: true, flac: true, ..Default::default() };
    for x in list.split(',') {
        match x {
            "mkv" => c.mkv = true,
            "hevc" => c.hevc = true,
            "ac3" => c.ac3 = true,
            "eac3" => c.eac3 = true,
            "dts" => c.dts = true,
            "noh264" => c.h264 = false,
            "noaac" => c.aac = false,
            _ => {}
        }
    }
    c
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,librqbit=warn,librqbit_dht=warn".into()))
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let caps = caps_from(args.iter().find_map(|a| a.strip_prefix("--caps=")).unwrap_or(""));
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let hash = pos.first().map(|s| s.as_str()).unwrap_or(SINTEL).to_string();
    let idx = pos.get(1).and_then(|s| s.parse().ok());

    let cfg = MediaConfig {
        data_dir: PathBuf::from(std::env::var("MYTHOS_DATA").unwrap_or_else(|_| "./data/media-example".into())),
        cache_limit_bytes: std::env::var("MYTHOS_CACHE_GB").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(20) << 30,
        peer_port: 42069,
        ffmpeg: std::env::var("FFMPEG").unwrap_or_else(|_| "ffmpeg".into()).into(),
        ffprobe: std::env::var("FFPROBE").unwrap_or_else(|_| "ffprobe".into()).into(),
        hwaccel: if std::env::var("MYTHOS_HWACCEL").as_deref() == Ok("nvenc") { HwAccel::Nvenc } else { HwAccel::None },
        extra_trackers: true,
    };
    let t0 = Instant::now();
    // `MYTHOS_DOWN_LIMIT_KB`: tope de bajada para pruebas (emular una conexión lenta).
    let tuning = mythos_media::Tuning {
        download_limit_bps: std::env::var("MYTHOS_DOWN_LIMIT_KB").ok().and_then(|v| v.parse::<u32>().ok()).map(|k| k * 1024),
        ..Default::default()
    };
    let media = Media::with_tuning(cfg, tuning).await?;
    println!("media up in {:?}", t0.elapsed());

    let opener = Router::new()
        .route("/open", get(open))
        .route("/prefetch", get(prefetch))
        .route("/torrents", get(|State((m, _)): State<(Media, ClientCaps)>| async move { Json(m.torrents().await) }))
        .route("/stats", get(|State((m, _)): State<(Media, ClientCaps)>| async move { Json(m.stats().await) }))
        .route("/cached", get(cached))
        .route("/remove", get(remove))
        .with_state((media.clone(), caps.clone()));
    let app = Router::new().nest("/api/media", media.router()).merge(opener);
    let l = tokio::net::TcpListener::bind("127.0.0.1:7711").await?;
    tokio::spawn(async move { axum::serve(l, app).await });

    let t1 = Instant::now();
    let info = media.open(Source::Torrent { info_hash: hash, file_idx: idx, trackers: vec![] }, caps).await?;
    println!("open in {:?}\n{}", t1.elapsed(), serde_json::to_string_pretty(&info)?);
    let id = info.session_id.clone();
    // Mide el tiempo hasta `ready` e imprime el estado cada 2 s.
    let mut ready_at = None;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let Some(st) = media.status(&id) else { break };
        if ready_at.is_none() && st.state == mythos_media::MediaState::Ready {
            ready_at = Some(t1.elapsed());
            println!("READY after {:?}", t1.elapsed());
        }
        println!(
            "{:?} down={:.2} MB/s peers={} {:.1}% ahead={:?}s target={:?}s",
            st.state,
            st.down_bps as f64 / 1e6,
            st.peers,
            st.progress * 100.0,
            st.buffered_ahead_s.map(|x| x.round()),
            st.buffer_target_s.map(|x| x.round())
        );
    }
    Ok(())
}

async fn open(State((media, caps)): State<(Media, ClientCaps)>, Query(q): Query<HashMap<String, String>>) -> Json<serde_json::Value> {
    let src = match q.get("url") {
        Some(u) => Source::Url(u.clone()),
        None => Source::Torrent {
            info_hash: q.get("hash").cloned().unwrap_or_default(),
            file_idx: q.get("idx").and_then(|s| s.parse().ok()),
            trackers: vec![],
        },
    };
    let caps = q.get("caps").map(|c| caps_from(c)).unwrap_or(caps);
    match media.open(src, caps).await {
        Ok(i) => Json(serde_json::to_value(i).unwrap_or_default()),
        Err(e) => Json(serde_json::json!({ "error": format!("{e:#}") })),
    }
}

async fn prefetch(State((media, _)): State<(Media, ClientCaps)>, Query(q): Query<HashMap<String, String>>) -> Json<serde_json::Value> {
    let src = Source::Torrent {
        info_hash: q.get("hash").cloned().unwrap_or_default(),
        file_idx: q.get("idx").and_then(|s| s.parse().ok()),
        trackers: vec![],
    };
    Json(serde_json::json!({ "ok": media.prefetch(src).await.is_ok() }))
}

async fn cached(State((media, _)): State<(Media, ClientCaps)>, Query(q): Query<HashMap<String, String>>) -> Json<bool> {
    Json(media.is_downloaded(q.get("hash").map(|s| s.as_str()).unwrap_or(""), q.get("idx").and_then(|s| s.parse().ok())))
}

async fn remove(State((media, _)): State<(Media, ClientCaps)>, Query(q): Query<HashMap<String, String>>) -> Json<serde_json::Value> {
    let r = media.remove_torrent(q.get("hash").map(|s| s.as_str()).unwrap_or(""), q.contains_key("files")).await;
    Json(serde_json::json!({ "ok": r.is_ok(), "error": r.err().map(|e| e.to_string()) }))
}
