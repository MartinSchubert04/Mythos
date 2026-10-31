//! Sesión de reproducción: un archivo (de torrent o de URL) abierto para un cliente.
//!
//! Guarda la posición de lectura ("head") que alimenta la ventana de prioridad, el estado del
//! buffer (para `ready` y `buffered_ahead_s`), la info del probe y el trabajo HLS. Todo con
//! atómicos o locks cortos: la consulta de `/status` es frecuente y no debe frenar a nadie.

use crate::window::{self, FileGeom};
use crate::{MediaState, MediaStatus, PlayInfo};
use librqbit::ManagedTorrent;
use std::sync::Arc;

type ManagedTorrentHandle = Arc<ManagedTorrent>;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

pub struct TorrentFile {
    pub handle: ManagedTorrentHandle,
    pub hash: String,
    pub idx: usize,
    pub geom: FileGeom,
    /// Ruta en disco (para leer sin prioridad, p. ej. al extraer subtítulos).
    pub path: PathBuf,
}

pub enum Backing {
    Torrent(TorrentFile),
    Url(String),
}

struct BufState {
    state: MediaState,
    error: Option<String>,
    ahead_bytes: Option<u64>,
    target_s: Option<f64>,
    ever_ready: bool,
    /// Desde cuándo se está esperando colchón (para relajar el objetivo si tarda mucho).
    waiting_since: Option<Instant>,
}

pub struct PlaySession {
    pub id: String,
    pub backing: Backing,
    pub file_name: String,
    pub size: u64,
    pub info: Mutex<Option<PlayInfo>>,
    pub probe: Mutex<Option<crate::probe::Probe>>,
    pub caps: Mutex<crate::ClientCaps>,
    /// Bitrate estimado en bytes/s (tamaño/duración; antes del probe, suponiendo 2 h).
    bitrate: AtomicU64,
    head: AtomicU64,
    /// Generación del lector más nuevo: solo ese mueve el `head` (los viejos se están cerrando).
    reader_gen: AtomicU64,
    last_activity: Mutex<Instant>,
    closed: AtomicBool,
    buf: Mutex<BufState>,
    /// Para URL: bytes traídos del origen y medición de velocidad.
    url_bytes: AtomicU64,
    url_speed: Mutex<(Instant, u64, u64)>,
    /// Segundos de HLS ya generados por delante del último segmento pedido (lo llena `hls`).
    pub hls_ahead_s: Mutex<Option<f64>>,
    pub hls: tokio::sync::Mutex<Option<crate::hls::HlsJob>>,
    pub subs: crate::subs::SubsState,
    pub tmp_dir: PathBuf,
}

impl PlaySession {
    pub fn new(id: String, backing: Backing, file_name: String, size: u64, tmp_dir: PathBuf) -> Self {
        Self {
            id,
            backing,
            file_name,
            size,
            info: Mutex::new(None),
            probe: Mutex::new(None),
            caps: Mutex::new(Default::default()),
            bitrate: AtomicU64::new((size / 7200).max(1)),
            head: AtomicU64::new(0),
            reader_gen: AtomicU64::new(0),
            last_activity: Mutex::new(Instant::now()),
            closed: AtomicBool::new(false),
            buf: Mutex::new(BufState {
                state: MediaState::Metadata,
                error: None,
                ahead_bytes: None,
                target_s: None,
                ever_ready: false,
                waiting_since: None,
            }),
            url_bytes: AtomicU64::new(0),
            url_speed: Mutex::new((Instant::now(), 0, 0)),
            hls_ahead_s: Mutex::new(None),
            hls: tokio::sync::Mutex::new(None),
            subs: Default::default(),
            tmp_dir,
        }
    }

    pub fn torrent(&self) -> Option<&TorrentFile> {
        match &self.backing {
            Backing::Torrent(t) => Some(t),
            Backing::Url(_) => None,
        }
    }

    pub fn bitrate(&self) -> f64 {
        self.bitrate.load(Ordering::Relaxed) as f64
    }

    pub fn set_duration(&self, d: Option<f64>, bit_rate_bps: Option<u64>) {
        let br = match (bit_rate_bps, d) {
            (Some(b), _) if b > 0 => b / 8,
            (_, Some(d)) if d > 1.0 => (self.size as f64 / d) as u64,
            _ => return,
        };
        self.bitrate.store(br.max(1), Ordering::Relaxed);
    }

    pub fn head(&self) -> u64 {
        self.head.load(Ordering::Relaxed)
    }

    /// Un lector nuevo (petición HTTP) se registra y recibe su generación.
    pub fn new_reader(&self, start: u64) -> u64 {
        let g = self.reader_gen.fetch_add(1, Ordering::Relaxed) + 1;
        self.head.store(start, Ordering::Relaxed);
        self.touch();
        g
    }

    pub fn advance(&self, gen: u64, pos: u64) {
        if self.reader_gen.load(Ordering::Relaxed) == gen {
            self.head.store(pos, Ordering::Relaxed);
        }
    }

    pub fn touch(&self) {
        *self.last_activity.lock().unwrap() = Instant::now();
    }

    pub fn idle_secs(&self) -> u64 {
        self.last_activity.lock().unwrap().elapsed().as_secs()
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    pub fn set_state(&self, s: MediaState) {
        let mut b = self.buf.lock().unwrap();
        if b.state != MediaState::Error {
            b.state = s;
        }
    }

    pub fn set_error(&self, e: String) {
        let mut b = self.buf.lock().unwrap();
        b.state = MediaState::Error;
        b.error = Some(e);
    }

    /// Lo llama la ventana: decide `ready`/`buffering` según el colchón adaptativo.
    pub fn update_buffer(&self, ahead: u64, complete: bool) {
        let br = self.bitrate();
        let down = self.down_bps() as f64;
        let waited_s = self.buf.lock().unwrap().waiting_since.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
        let target = window::buffer_target_s(br, down, waited_s);
        // En HLS cuenta también lo ya convertido por delante de lo que pidió el reproductor.
        let hls = self.hls_ahead_s.lock().unwrap().unwrap_or(0.0);
        let ahead_s = ahead as f64 / br.max(1.0) + hls;
        let mut b = self.buf.lock().unwrap();
        b.ahead_bytes = Some(ahead);
        b.target_s = Some(target);
        if matches!(b.state, MediaState::Error | MediaState::Metadata | MediaState::Probing) {
            return;
        }
        let at_end = self.head() + ahead >= self.size;
        if complete || at_end || ahead_s >= target {
            b.state = MediaState::Ready;
            b.ever_ready = true;
            b.waiting_since = None;
        } else if !b.ever_ready || ahead_s < 1.5 {
            // Antes del primer `ready`, o si se vació el colchón (traba en camino): buffering.
            b.state = MediaState::Buffering;
            b.waiting_since.get_or_insert_with(Instant::now);
        }
    }

    /// Para URL: contabiliza bytes traídos del origen.
    pub fn add_url_bytes(&self, n: u64) {
        self.url_bytes.fetch_add(n, Ordering::Relaxed);
    }

    fn down_bps(&self) -> u64 {
        match &self.backing {
            Backing::Torrent(t) => t.handle.live().map(|l| l.down_speed_estimator().bps()).unwrap_or(0),
            Backing::Url(_) => {
                let total = self.url_bytes.load(Ordering::Relaxed);
                let mut g = self.url_speed.lock().unwrap();
                let el = g.0.elapsed().as_secs_f64();
                if el >= 2.0 {
                    g.2 = ((total - g.1) as f64 / el) as u64;
                    *g = (Instant::now(), total, g.2);
                }
                g.2
            }
        }
    }

    pub fn status(&self) -> MediaStatus {
        let (state, error, ahead, target) = {
            let b = self.buf.lock().unwrap();
            (b.state, b.error.clone(), b.ahead_bytes, b.target_s)
        };
        let br = self.bitrate().max(1.0);
        let hls_ahead = *self.hls_ahead_s.lock().unwrap();
        let mut buffered_ahead_s = ahead.map(|a| a as f64 / br);
        if let Some(h) = hls_ahead {
            buffered_ahead_s = Some(h + buffered_ahead_s.unwrap_or(0.0));
        }
        let down_bps = self.down_bps();
        match &self.backing {
            Backing::Torrent(t) => {
                let st = t.handle.stats();
                let downloaded = st.file_progress.get(t.idx).copied().unwrap_or(0).min(self.size);
                let (up, peers) = t
                    .handle
                    .live()
                    .map(|l| (l.up_speed_estimator().bps(), l.stats_snapshot().peer_stats.live))
                    .unwrap_or((0, 0));
                MediaStatus {
                    state,
                    error,
                    down_bps,
                    up_bps: up,
                    peers,
                    downloaded_bytes: downloaded,
                    total_bytes: self.size,
                    progress: if self.size > 0 { downloaded as f64 / self.size as f64 } else { 0.0 },
                    buffered_ahead_s,
                    buffer_target_s: target,
                }
            }
            Backing::Url(_) => {
                let pos = self.head().min(self.size);
                MediaStatus {
                    state,
                    error,
                    down_bps,
                    up_bps: 0,
                    peers: 0,
                    downloaded_bytes: pos,
                    total_bytes: self.size,
                    progress: if self.size > 0 { pos as f64 / self.size as f64 } else { 0.0 },
                    buffered_ahead_s,
                    buffer_target_s: None,
                }
            }
        }
    }
}
