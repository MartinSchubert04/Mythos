//! Mide el arranque en frío contra el arranque con precarga (no es parte del server).
//!
//! Borra el torrent (desde cero), opcionalmente lo precarga (`--prefetch`, como la precarga del
//! próximo episodio o de la Biblioteca), opcionalmente reinicia el motor (`--restart`: los peers
//! se pierden, como cuando el torrent precargado quedó en pausa horas) y abre una sesión. Mide
//! cuánto tarda `open` (metadata + probe) y cuánto hasta `ready`.
//!
//! `--next <hash del episodio actual>`: en vez de la precarga de la ficha usa la del próximo
//! episodio (`prefetch_next`): abre una sesión con ese torrent (hace de episodio actual, se
//! "mira" leyéndolo entero), pide la precarga, espera a que quede `done` y mide cuánto bajó.
//! Con `--cancel` cierra la sesión actual sin abrir el siguiente y comprueba que lo precargado
//! se borre; si no, la cierra y abre el siguiente (reusa lo precargado) y mide.
//!
//! ```text
//! cargo run --release -p mythos-media --example warm -- --hash <info_hash> [--idx N]
//!     [--prefetch] [--next <hash>] [--cancel] [--restart] [--secs 120] [--port 42072]
//! ```
//! ## Mediciones (PC con fibra, sin puerto abierto, ~200 Mbit; 31/10/2026)
//!
//! | caso | open | ready | en disco antes de abrir |
//! |---|---|---|---|
//! | Big Buck Bunny (276 MB) en frío, antes / después | 3,2–4,1 / 4,5–6,6 s | 6,4–7,2 / 7,2–8,9 s | — |
//! | Tears of Steel (571 MB) en frío, antes / después | 4,2–6,8 / 2,8–3,0 s | 7,0–8,0 / 7,2–8,1 s | — |
//! | BBB con la precarga de la ficha (vieja) + reinicio | 0,1 s | 0,7 s | 276 MB (el archivo entero) |
//! | BBB con `--next` (precarga del próximo episodio) | 0,1 s | 0,7 s | 77 MB (52 pedidos) |
//! | ToS con `--next` | 0,1 s | 0,9 s | 108 MB (55 pedidos) |
//! | ToS con `--next --cancel` | — | — | torrent borrado a los 45 s |
//!
//! En frío no hay diferencia medible (lo que cambió —200 peers, más trackers— se nota con el
//! puerto abierto y swarms grandes, no con estos torrents de prueba de pocos peers).
//!
//! Imprime `RESULT {json}`. `MYTHOS_DATA` elige la carpeta de datos (por defecto
//! `./data/media-warm`).

use mythos_media::{ClientCaps, HwAccel, Media, MediaConfig, MediaState, Source};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1).cloned())
}

fn flag(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

fn cfg(port: u16) -> MediaConfig {
    MediaConfig {
        data_dir: PathBuf::from(std::env::var("MYTHOS_DATA").unwrap_or_else(|_| "./data/media-warm".into())),
        cache_limit_bytes: 0,
        cache_max_age_secs: 0,
        peer_port: port,
        ffmpeg: "ffmpeg".into(),
        ffprobe: "ffprobe".into(),
        hwaccel: HwAccel::None,
        extra_trackers: true,
    }
}

fn src(hash: &str, idx: Option<usize>) -> Source {
    Source::Torrent { info_hash: hash.to_string(), file_idx: idx, trackers: vec![] }
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "error".into()))
        .init();
    let hash = arg("--hash").unwrap_or_else(|| "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c".into());
    let idx: Option<usize> = arg("--idx").and_then(|s| s.parse().ok());
    let secs: f64 = arg("--secs").and_then(|s| s.parse().ok()).unwrap_or(120.0);
    let port: u16 = arg("--port").and_then(|s| s.parse().ok()).unwrap_or(42072);
    let parent_hash = arg("--next");

    let mut media = Media::new(cfg(port)).await?;
    let _ = media.remove_torrent(&hash, true).await;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut prefetch_s = None;
    let mut prefetched = 0u64;
    if let Some(ph) = &parent_hash {
        let caps = ClientCaps { h264: true, vp9: true, av1: true, aac: true, opus: true, mkv: true, ..Default::default() };
        let parent = media.open(src(ph, None), caps).await?;
        // "Mirar" el actual: un lector que se lleva todo (colchón = lo bajado por delante).
        let url = format!("http://127.0.0.1:{}/api/media/{}/file", serve(&media).await?, parent.session_id);
        tokio::spawn(async move {
            if let Ok(r) = reqwest::get(&url).await {
                let _ = r.bytes().await;
            }
        });
        let t = Instant::now();
        media.prefetch_next(&parent.session_id, src(&hash, idx))?;
        let mut last = None;
        while t.elapsed().as_secs_f64() < secs * 3.0 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let Some(ns) = media.next_status(&parent.session_id) else { break };
            if last != Some(ns.state) {
                eprintln!("t={:.1}s next {:?} {}/{} MB", t.elapsed().as_secs_f64(), ns.state, ns.have_bytes >> 20, ns.target_bytes >> 20);
                last = Some(ns.state);
            }
            if ns.state == mythos_media::NextState::Done {
                break;
            }
        }
        prefetch_s = Some(r1(t.elapsed().as_secs_f64()));
        tokio::time::sleep(Duration::from_secs(3)).await;
        prefetched = downloaded(&media, &hash).await;
        eprintln!("next prefetch {:?}s, {:.1} MB on disk", prefetch_s, prefetched as f64 / 1e6);
        media.close(&parent.session_id).await;
        if flag("--cancel") {
            tokio::time::sleep(Duration::from_secs(50)).await;
            let left = media.torrents().await.into_iter().any(|x| x.info_hash == hash);
            println!("RESULT {}", serde_json::json!({ "hash": hash, "cancel": true, "prefetch_s": prefetch_s, "prefetched_mb": prefetched / 1_000_000, "torrent_left": left }));
            media.shutdown().await;
            return Ok(());
        }
    } else if flag("--prefetch") {
        let t = Instant::now();
        media.prefetch(src(&hash, idx)).await?;
        // Terminó cuando lo bajado no cambia en 6 s (la precarga suelta sus fantasmas al
        // completarse; lo que siga bajando es el orden natural de librqbit, que no cuenta).
        let mut last = (0u64, Instant::now());
        while t.elapsed().as_secs_f64() < secs {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let b = media.torrents().await.into_iter().find(|x| x.info_hash == hash).map(|x| x.downloaded_bytes).unwrap_or(0);
            if b != last.0 {
                last = (b, Instant::now());
            } else if b > 0 && last.1.elapsed() > Duration::from_secs(6) {
                break;
            }
        }
        prefetch_s = Some(r1(last.1.duration_since(t).as_secs_f64()));
        prefetched = last.0;
        eprintln!("prefetch {:?}s {:.1} MB", prefetch_s, prefetched as f64 / 1e6);
    }
    if flag("--restart") {
        media.shutdown().await;
        drop(media);
        tokio::time::sleep(Duration::from_secs(2)).await;
        media = Media::new(cfg(port)).await?;
    }

    let caps = ClientCaps { h264: true, vp9: true, av1: true, aac: true, opus: true, mkv: true, ..Default::default() };
    let t0 = Instant::now();
    let info = media.open(src(&hash, idx), caps).await?;
    let open_s = t0.elapsed().as_secs_f64();
    let mut ready_s = None;
    let mut at_ready = None;
    let mut peers_max = 0;
    let mut down_max = 0u64;
    while t0.elapsed().as_secs_f64() < secs {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let Some(st) = media.status(&info.session_id) else { break };
        peers_max = peers_max.max(st.peers);
        down_max = down_max.max(st.down_bps);
        if st.state == MediaState::Ready && ready_s.is_none() {
            ready_s = Some(r1(t0.elapsed().as_secs_f64()));
            at_ready = Some(st.clone());
            if !flag("--stay") {
                break;
            }
        }
        if st.state == MediaState::Error {
            eprintln!("error: {:?}", st.error);
            break;
        }
    }
    let res = serde_json::json!({
        "hash": hash,
        "file": info.file_name,
        "size_mb": info.size_bytes / 1_000_000,
        "prefetch": flag("--prefetch"),
        "next_of": parent_hash,
        "restart": flag("--restart"),
        "prefetch_s": prefetch_s,
        "prefetched_mb": prefetched / 1_000_000,
        "open_s": r1(open_s),
        "ready_s": ready_s,
        "ahead_at_ready_s": at_ready.as_ref().and_then(|s| s.buffered_ahead_s).map(r1),
        "target_at_ready_s": at_ready.as_ref().and_then(|s| s.buffer_target_s).map(r1),
        "peers_max": peers_max,
        "down_max_mbit": r1(down_max as f64 * 8.0 / 1e6),
    });
    println!("RESULT {res}");
    media.close(&info.session_id).await;
    media.shutdown().await;
    Ok(())
}

async fn downloaded(media: &Media, hash: &str) -> u64 {
    media.torrents().await.into_iter().find(|x| x.info_hash == hash).map(|x| x.downloaded_bytes).unwrap_or(0)
}

/// Sirve el router de media en un puerto libre (para "mirar" la sesión actual).
async fn serve(media: &Media) -> anyhow::Result<u16> {
    let app = axum::Router::new().nest("/api/media", media.router());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = l.local_addr()?.port();
    tokio::spawn(async move { axum::serve(l, app).await });
    Ok(port)
}
