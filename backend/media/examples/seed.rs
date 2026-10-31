//! Herramienta de prueba: crea un torrent a partir de un archivo local y lo siembra con una
//! velocidad de subida limitada, para probar `mythos-media` contra un torrent propio (p. ej. un
//! MKV de Sintel con subtítulos internos) sin depender de swarms públicos.
//!
//! ```text
//! cargo run --release -p mythos-media --example seed -- <archivo> <carpeta de datos de Media> [KB/s] [puerto]
//! ```
//! Imprime el info_hash, deja el `.torrent` en `<datos>/meta/` (así `Media` no necesita resolver
//! el magnet) y queda sembrando hasta que se lo corta. `Media` encuentra al sembrador por LSD
//! (multicast local), sin DHT ni trackers. Varios sembradores del mismo archivo (cada uno en su
//! puerto y con su tope) arman un "swarm lento" de prueba: el torrent sale igual en todos porque
//! se crea con los mismos parámetros.

use librqbit::{CreateTorrentOptions, ListenerMode, ListenerOptions, Session, SessionOptions};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let file = PathBuf::from(args.first().expect("falta el archivo"));
    let media_data = PathBuf::from(args.get(1).expect("falta la carpeta de datos de Media"));
    let kbps: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1500);
    let port: u16 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(42090);
    let work = std::env::temp_dir().join(format!("mythos-seed-{}", std::process::id()));
    std::fs::create_dir_all(&work)?;
    let opts = SessionOptions {
        dht: None,
        listen: Some(ListenerOptions {
            mode: ListenerMode::TcpOnly,
            listen_addr: SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
            ipv4_only: true,
            ..Default::default()
        }),
        ipv4_only: true,
        ratelimits: librqbit::limits::LimitsConfig { upload_bps: std::num::NonZeroU32::new(kbps * 1024), download_bps: None },
        ..Default::default()
    };
    let session = Session::new_with_opts(work, opts).await?;
    let (torrent, _handle) = session
        .create_and_serve_torrent(&file, CreateTorrentOptions { name: None, trackers: vec![], piece_length: Some(1 << 20) })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let hash = torrent.info_hash().as_string();
    let meta = media_data.join("meta");
    std::fs::create_dir_all(&meta)?;
    std::fs::write(meta.join(format!("{hash}.torrent")), torrent.as_bytes()?)?;
    println!("SEEDING {hash} at {kbps} KB/s on port {port}");
    tokio::signal::ctrl_c().await?;
    Ok(())
}
