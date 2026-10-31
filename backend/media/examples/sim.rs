//! Simulador de reproducción para medir trabas (no es parte del server).
//!
//! Levanta `Media` con un tope de bajada (para emular un swarm o una conexión lenta), abre un
//! torrent desde cero y lo "mira" como lo haría un `<video>`: pide `/file` con `Range`, consume a
//! la velocidad de reproducción (bitrate = tamaño/duración), lee por delante hasta 60 s como un
//! navegador, arranca cuando la sesión está `ready` y tiene 2 s en su buffer, se traba si el
//! buffer llega a 0 (y sigue con 2 s), y hace saltos en los momentos pedidos.
//!
//! ```text
//! cargo run --release -p mythos-media --example sim -- --hash <info_hash> --limit-x 1.5 \
//!     --window 64 --seeks 60:0.5,120:0.8 --secs 200 [--bitrate <bytes/s>] [--no-focus] //!     [--max-windows 6] [--peer-limit 160] [--wait-ready] [--no-rescue] [--no-flush] [-v]
//! ```
//! Imprime una línea `RESULT {json}` con las mediciones. `MYTHOS_DATA` elige la carpeta de datos.

use futures::StreamExt;
use mythos_media::{ClientCaps, HwAccel, Media, MediaConfig, MediaState, Source, Tuning};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const START_BUF_S: f64 = 2.0;
static RECONNECTS: AtomicU64 = AtomicU64::new(0);
const CLIENT_AHEAD_S: f64 = 60.0;

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1).cloned())
}

/// Lector HTTP: baja desde `start` y publica hasta qué byte llegó; se frena solo si el
/// "reproductor" ya tiene 60 s por delante (como el backpressure de un navegador).
fn spawn_reader(url: String, start: u64, recv: Arc<AtomicU64>, play_byte: Arc<AtomicU64>, bitrate: f64, stop: Arc<AtomicBool>) {
    recv.store(start, Ordering::Relaxed);
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let Ok(resp) = client.get(&url).header("Range", format!("bytes={start}-")).send().await else { return };
        let s = resp.bytes_stream();
        let mut s = Box::pin(s);
        while !stop.load(Ordering::Relaxed) {
            let ahead = recv.load(Ordering::Relaxed).saturating_sub(play_byte.load(Ordering::Relaxed)) as f64 / bitrate;
            if ahead > CLIENT_AHEAD_S {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            match tokio::time::timeout(Duration::from_millis(200), s.next()).await {
                Ok(Some(Ok(b))) => {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    recv.fetch_add(b.len() as u64, Ordering::Relaxed);
                }
                Ok(other) => {
                    // El servidor cortó la respuesta antes del final: un `<video>` volvería a
                    // pedir desde donde iba; lo contamos para detectar cortes del lado del server.
                    if !stop.load(Ordering::Relaxed) {
                        eprintln!("READER ENDED at byte {} ({:?})", recv.load(Ordering::Relaxed), other.map(|r| r.map(|b| b.len())));
                        RECONNECTS.fetch_add(1, Ordering::Relaxed);
                        let pos = recv.load(Ordering::Relaxed);
                        match client.get(&url).header("Range", format!("bytes={pos}-")).send().await {
                            Ok(r) => s = Box::pin(r.bytes_stream()),
                            Err(_) => return,
                        }
                        continue;
                    }
                    return;
                }
                Err(_) => {}
            }
        }
    });
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "error".into()))
        .init();
    let hash = arg("--hash").unwrap_or_else(|| "209c8226b299b308beaf2b9cd3fb49212dbd13ec".into());
    let limit_x: f64 = arg("--limit-x").and_then(|s| s.parse().ok()).unwrap_or(1.5);
    let window: usize = arg("--window").and_then(|s| s.parse().ok()).unwrap_or(64);
    let max_windows: usize = arg("--max-windows").and_then(|s| s.parse().ok()).unwrap_or(6);
    let secs: f64 = arg("--secs").and_then(|s| s.parse().ok()).unwrap_or(200.0);
    let focus = !std::env::args().any(|a| a == "--no-focus");
    let peer_limit: usize = arg("--peer-limit").and_then(|s| s.parse().ok()).unwrap_or(Tuning::default().peer_limit);
    let verbose = std::env::args().any(|a| a == "-v");
    // `--wait-ready`: tras un salto o una traba, el reproductor espera `status == ready` (lo que
    // haría la UI) en vez de seguir apenas tiene 2 s (lo que hace un `<video>` solo).
    let wait_ready = std::env::args().any(|a| a == "--wait-ready");
    let seeks: Vec<(f64, f64)> = arg("--seeks")
        .unwrap_or_else(|| "60:0.5,120:0.8".into())
        .split(',')
        .filter_map(|p| p.split_once(':').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))))
        .collect();

    let cfg = MediaConfig {
        data_dir: PathBuf::from(std::env::var("MYTHOS_DATA").unwrap_or_else(|_| "./data/media-sim".into())),
        cache_limit_bytes: 0,
        peer_port: 42071,
        ffmpeg: "ffmpeg".into(),
        ffprobe: "ffprobe".into(),
        hwaccel: HwAccel::None,
        extra_trackers: true,
    };
    let rescue = !std::env::args().any(|a| a == "--no-rescue");
    let flush = !std::env::args().any(|a| a == "--no-flush");
    let tuning = Tuning { peer_request_window: window, max_windows, focus_when_slow: focus, peer_limit, rescue_stuck: rescue, flush_on_seek: flush, ..Default::default() };
    let media = Media::with_tuning(cfg, tuning).await?;
    // Desde cero: borra lo bajado (la metadata queda cacheada, así medimos la descarga y no el DHT).
    let _ = media.remove_torrent(&hash, true).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let app = axum::Router::new().nest("/api/media", media.router());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = l.local_addr()?.port();
    tokio::spawn(async move { axum::serve(l, app).await });

    // El tope hace falta antes de abrir; el bitrate lo sabemos recién después del probe, así que
    // arrancamos con un tope bajo y lo ajustamos apenas tenemos la duración.
    let pre: Option<f64> = arg("--bitrate").and_then(|s| s.parse().ok());
    // `--limit-x 0`: sin tope (para probar contra un swarm lento de verdad, ver `seed.rs`).
    let limit = |b: f64| (limit_x > 0.0).then_some((b * limit_x) as u32);
    media.set_download_limit(pre.map(limit).unwrap_or(Some(256 * 1024)));
    let t0 = Instant::now();
    let caps = ClientCaps { h264: true, vp9: true, av1: true, aac: true, opus: true, mkv: true, ..Default::default() };
    let info = media.open(Source::Torrent { info_hash: hash.clone(), file_idx: None, trackers: vec![] }, caps).await?;
    let t_open = t0.elapsed().as_secs_f64();
    let dur = info.duration_s.unwrap_or(600.0);
    let size = info.size_bytes;
    // Con `--bitrate` se usa ese (por si no hay ffprobe y la duración es adivinada).
    let bitrate = pre.unwrap_or(size as f64 / dur);
    let dur = size as f64 / bitrate;
    media.set_download_limit(limit(bitrate));
    let url = format!("http://127.0.0.1:{port}/api/media/{}/file", info.session_id);
    eprintln!("open {t_open:.1}s file={} bitrate={:.0} KB/s limit={:.0} KB/s", info.file_name, bitrate / 1024.0, bitrate * limit_x / 1024.0);

    let recv = Arc::new(AtomicU64::new(0));
    let play_byte = Arc::new(AtomicU64::new(0));
    let mut stop = Arc::new(AtomicBool::new(false));
    spawn_reader(url.clone(), 0, recv.clone(), play_byte.clone(), bitrate, stop.clone());

    let mut pos_s = 0.0f64; // posición de reproducción
    let mut playing = false;
    let mut t_ready = None;
    let mut t_play = None;
    let mut stalls: Vec<f64> = Vec::new();
    let mut stall_since: Option<Instant> = None;
    let mut seek_lat: Vec<f64> = Vec::new();
    let mut seek_since: Option<Instant> = None;
    let mut next_seek = 0usize;
    let mut last = Instant::now();
    let dt = Duration::from_millis(100);
    while t0.elapsed().as_secs_f64() < secs && pos_s < dur - 1.0 {
        tokio::time::sleep(dt).await;
        let now = Instant::now();
        let step = now.duration_since(last).as_secs_f64();
        last = now;
        let st = media.status(&info.session_id);
        if verbose && (t0.elapsed().as_millis() / 100).is_multiple_of(50) {
            if let Some(s) = &st {
                eprintln!(
                    "t={:.0}s {:?} ahead={:.1}s prog={:.3} down={:.0}KB/s peers={} pos={:.0}s recv={:.1}s playing={playing}",
                    t0.elapsed().as_secs_f64(),
                    s.state,
                    s.buffered_ahead_s.unwrap_or(0.0),
                    s.progress,
                    s.down_bps as f64 / 1024.0,
                    s.peers,
                    pos_s,
                    recv.load(Ordering::Relaxed) as f64 / bitrate
                );
            }
        }
        if t_ready.is_none() && st.as_ref().is_some_and(|s| s.state == MediaState::Ready) {
            t_ready = Some(t0.elapsed().as_secs_f64());
        }
        let buf_s = (recv.load(Ordering::Relaxed) as f64 / bitrate - pos_s).max(0.0);
        if playing {
            if buf_s <= 0.05 {
                playing = false;
                stall_since = Some(now);
            } else {
                pos_s += step.min(buf_s);
                play_byte.store((pos_s * bitrate) as u64, Ordering::Relaxed);
            }
        } else {
            let first = t_play.is_none();
            let is_ready = st.as_ref().is_some_and(|s| s.state == MediaState::Ready);
            let ok_state = if first { t_ready.is_some() } else { !wait_ready || is_ready };
            if ok_state && buf_s >= START_BUF_S {
                playing = true;
                if first {
                    t_play = Some(t0.elapsed().as_secs_f64());
                }
                if let Some(s) = seek_since.take() {
                    seek_lat.push(s.elapsed().as_secs_f64());
                } else if let Some(s) = stall_since.take() {
                    stalls.push(s.elapsed().as_secs_f64());
                }
            }
        }
        // Saltos: a los `t` segundos de reproducción, ir a `frac` de la película.
        if playing && next_seek < seeks.len() && t_play.is_some_and(|tp| t0.elapsed().as_secs_f64() - tp >= seeks[next_seek].0) {
            let frac = seeks[next_seek].1;
            next_seek += 1;
            stop.store(true, Ordering::Relaxed);
            stop = Arc::new(AtomicBool::new(false));
            pos_s = dur * frac;
            let b = (pos_s * bitrate) as u64;
            play_byte.store(b, Ordering::Relaxed);
            spawn_reader(url.clone(), b, recv.clone(), play_byte.clone(), bitrate, stop.clone());
            playing = false;
            stall_since = None;
            seek_since = Some(Instant::now());
        }
    }
    if let Some(s) = stall_since {
        stalls.push(s.elapsed().as_secs_f64());
    }
    let st = media.status(&info.session_id);
    let res = serde_json::json!({
        "rescue": rescue, "flush": flush, "window": window, "limit_x": limit_x, "peer_limit": peer_limit, "wait_ready": wait_ready, "focus": focus, "max_windows": max_windows,
        "open_s": (t_open * 10.0).round() / 10.0,
        "ready_s": t_ready.map(|x| (x * 10.0).round() / 10.0),
        "play_s": t_play.map(|x| (x * 10.0).round() / 10.0),
        "stalls": stalls.len(),
        "stall_total_s": (stalls.iter().sum::<f64>() * 10.0).round() / 10.0,
        "stall_max_s": (stalls.iter().cloned().fold(0.0, f64::max) * 10.0).round() / 10.0,
        "seek_latency_s": seek_lat.iter().map(|x| (x * 10.0).round() / 10.0).collect::<Vec<_>>(),
        "played_s": pos_s.round(),
        "reconnects": RECONNECTS.load(Ordering::Relaxed),
        "peers": st.as_ref().map(|s| s.peers),
        "progress": st.as_ref().map(|s| (s.progress * 1000.0).round() / 1000.0),
    });
    println!("RESULT {res}");
    media.close(&info.session_id).await;
    Ok(())
}
