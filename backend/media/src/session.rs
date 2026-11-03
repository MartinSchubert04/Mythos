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
    target_s: Option<f64>,
    ever_ready: bool,
    /// Desde cuándo se está esperando colchón (para relajar el objetivo si tarda mucho).
    waiting_since: Option<Instant>,
    /// Byte desde donde el reproductor quiere ver (inicio del último salto). El lector HTTP se
    /// lleva todo lo que hay apenas llega, así que el colchón no se puede medir desde el `head`:
    /// se mide desde acá, descontando lo que ya se habría reproducido.
    origin: u64,
    /// Segundos reproducidos (estimados) desde `origin` antes del último `playing_since`.
    played_base: f64,
    /// Desde cuándo suponemos que se está reproduciendo (desde el último `ready`).
    playing_since: Option<Instant>,
    ahead_s: Option<f64>,
    /// El archivo ya está entero en disco (lo anota la ventana).
    complete: bool,
}

/// Por debajo de esto (con `ready`) la sesión "pasa hambre": las precargas y los torrents sin
/// uso le ceden el ancho de banda (ver `starving`).
pub const STARVING_AHEAD_S: f64 = 20.0;

/// Un lector nuevo que arranca más de esto por delante del head (o antes del origen) es un salto.
const SEEK_GAP: u64 = 8 << 20;

pub struct PlaySession {
    pub id: String,
    pub backing: Backing,
    pub file_name: String,
    pub size: u64,
    pub info: Mutex<Option<PlayInfo>>,
    pub probe: Mutex<Option<crate::probe::Probe>>,
    pub caps: Mutex<crate::ClientCaps>,
    /// Bitrate estimado en bytes/s (tamaño/duración; sin probe, adivinado por el nombre).
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
    /// Última foto de las piezas que tenemos (la actualiza la ventana): el lector la usa para
    /// detectar si está esperando una pieza que en realidad ya llegó.
    pub haves: Mutex<Arc<Vec<bool>>>,
    pub hls: tokio::sync::Mutex<Option<crate::hls::HlsJob>>,
    pub subs: crate::subs::SubsState,
    pub tmp_dir: PathBuf,
}

impl PlaySession {
    pub fn new(id: String, backing: Backing, file_name: String, size: u64, tmp_dir: PathBuf) -> Self {
        let bitrate = guess_bitrate(&file_name, size);
        Self {
            id,
            backing,
            file_name,
            size,
            info: Mutex::new(None),
            probe: Mutex::new(None),
            caps: Mutex::new(Default::default()),
            bitrate: AtomicU64::new(bitrate),
            head: AtomicU64::new(0),
            reader_gen: AtomicU64::new(0),
            last_activity: Mutex::new(Instant::now()),
            closed: AtomicBool::new(false),
            buf: Mutex::new(BufState {
                state: MediaState::Metadata,
                error: None,
                target_s: None,
                ever_ready: false,
                waiting_since: None,
                origin: 0,
                played_base: 0.0,
                playing_since: None,
                ahead_s: None,
                complete: false,
            }),
            url_bytes: AtomicU64::new(0),
            url_speed: Mutex::new((Instant::now(), 0, 0)),
            hls_ahead_s: Mutex::new(None),
            haves: Mutex::new(Arc::new(Vec::new())),
            hls: tokio::sync::Mutex::new(None),
            subs: Default::default(),
            tmp_dir,
        }
    }

    /// ¿Ya tenemos el byte `pos` del archivo? (según la última foto de la ventana)
    pub fn have_at(&self, pos: u64) -> bool {
        match self.torrent() {
            Some(t) => self.haves.lock().unwrap().get(t.geom.piece_at(pos)).copied().unwrap_or(false),
            None => false,
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
        {
            let mut b = self.buf.lock().unwrap();
            let head = self.head();
            if g == 1 || start < b.origin || start > head + SEEK_GAP {
                // Salto: el colchón se vuelve a medir desde acá y hay que llegar a `ready` de nuevo.
                b.origin = start;
                b.played_base = 0.0;
                b.playing_since = None;
                b.waiting_since = None;
                b.ever_ready = false;
            }
        }
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
    /// Byte desde donde se mide el colchón (ver `BufState::origin`).
    pub fn origin(&self) -> u64 {
        self.buf.lock().unwrap().origin
    }

    /// Colchón estimado en segundos (lo que calculó la última vuelta de la ventana).
    pub fn ahead_s(&self) -> f64 {
        self.buf.lock().unwrap().ahead_s.unwrap_or(0.0)
    }

    /// Lo llama la ventana con los bytes contiguos bajados desde `origin`: estima el colchón
    /// restando lo que ya se habría reproducido y decide `ready`/`buffering` (umbral adaptativo).
    pub fn update_buffer(&self, from_origin: u64, complete: bool, burst_s: f64) {
        let br = self.bitrate().max(1.0);
        let down = self.down_bps() as f64;
        // En HLS cuenta también lo ya convertido por delante de lo que pidió el reproductor.
        let hls = self.hls_ahead_s.lock().unwrap().unwrap_or(0.0);
        let mut b = self.buf.lock().unwrap();
        let waited_s = b.waiting_since.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
        let target = window::buffer_target_s(br, down, waited_s, burst_s);
        let played = b.played_base + b.playing_since.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
        let ahead_s = (from_origin as f64 / br - played).max(0.0) + hls;
        b.ahead_s = Some(ahead_s);
        b.target_s = Some(target);
        b.complete = complete;
        if matches!(b.state, MediaState::Error | MediaState::Metadata | MediaState::Probing) {
            return;
        }
        let at_end = b.origin + from_origin >= self.size;
        if complete || at_end || ahead_s >= target {
            if b.state != MediaState::Ready || b.playing_since.is_none() {
                b.playing_since = Some(Instant::now());
            }
            b.state = MediaState::Ready;
            b.ever_ready = true;
            b.waiting_since = None;
        } else if !b.ever_ready || ahead_s < 1.5 {
            // Antes del primer `ready`, o si se vació el colchón (traba en camino): buffering, y
            // el reproductor se supone parado (deja de sumar tiempo reproducido).
            if let Some(t) = b.playing_since.take() {
                b.played_base += t.elapsed().as_secs_f64();
            }
            b.state = MediaState::Buffering;
            b.waiting_since.get_or_insert_with(Instant::now);
        }
    }

    /// La sesión necesita todo el ancho de banda: arrancando o tras un salto (`buffering`), o
    /// reproduciendo con menos de `STARVING_AHEAD_S` de colchón. Un torrent ya completo o un link
    /// directo (Real-Debrid, no comparte peers) nunca pasa hambre de esto.
    pub fn starving(&self) -> bool {
        if self.is_closed() || self.torrent().is_none() {
            return false;
        }
        let b = self.buf.lock().unwrap();
        !b.complete
            && match b.state {
                MediaState::Buffering => true,
                MediaState::Ready => b.ahead_s.unwrap_or(0.0) < STARVING_AHEAD_S,
                _ => false,
            }
    }

    /// Colchón real por delante del lector: (está bajado todo lo que falta hasta el final del
    /// archivo, segundos contiguos ya bajados desde el `head`). Para una sesión de link directo:
    /// `(true, ∞)` si está `ready` (no comparte peers; solo la bajada de la casa).
    pub fn cushion(&self) -> (bool, f64) {
        let state = self.buf.lock().unwrap().state;
        let Some(t) = self.torrent() else {
            return if state == MediaState::Ready { (true, f64::INFINITY) } else { (false, 0.0) };
        };
        if self.buf.lock().unwrap().complete {
            return (true, f64::INFINITY);
        }
        if state != MediaState::Ready {
            return (false, 0.0);
        }
        let hv = self.haves.lock().unwrap().clone();
        let have = |p: usize| hv.get(p).copied().unwrap_or(false);
        let head = self.head().min(t.geom.len);
        let ahead = window::contiguous_ahead(&t.geom, &have, head);
        (head + ahead >= t.geom.len, ahead as f64 / self.bitrate().max(1.0))
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
        let (state, error, buffered_ahead_s, target) = {
            let b = self.buf.lock().unwrap();
            (b.state, b.error.clone(), b.ahead_s, b.target_s)
        };
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
                let have_ranges = {
                    let hv = self.haves.lock().unwrap().clone();
                    (!hv.is_empty()).then(|| window::have_ranges(&t.geom, &|p| hv.get(p).copied().unwrap_or(false), 64))
                };
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
                    have_ranges,
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
                    have_ranges: None,
                }
            }
        }
    }
}

/// Bitrate (bytes/s) para cuando ffprobe no dio la duración: típico según la resolución del
/// nombre, acotado para que la duración implícita quede entre 10 min y 4 h. Medido: suponer
/// siempre 2 h daba 79 KB/s para un archivo de 778 KB/s, y el `ready` salía con 1 s de colchón.
pub fn guess_bitrate(name: &str, size: u64) -> u64 {
    let n = name.to_ascii_lowercase();
    let typical: u64 = if n.contains("2160p") || n.contains("4k") || n.contains("uhd") {
        2_500_000
    } else if n.contains("1080p") {
        750_000
    } else if n.contains("720p") {
        400_000
    } else {
        600_000
    };
    typical.clamp((size / (4 * 3600)).max(1), (size / 600).max(1))
}

#[cfg(test)]
mod tests {
    use super::guess_bitrate;

    #[test]
    fn bitrate_guess() {
        // 571 MB sin resolución en el nombre: 600 KB/s (≈ 16 min).
        assert_eq!(guess_bitrate("Tears of Steel.webm", 571_346_576), 600_000);
        // 40 GB 4K: el típico daría 4.4 h → se acota a 4 h.
        assert_eq!(guess_bitrate("Movie.2160p.mkv", 40_000_000_000), 40_000_000_000 / (4 * 3600));
        // Archivo chico: la duración implícita no baja de 10 min.
        assert_eq!(guess_bitrate("clip.1080p.mp4", 100_000_000), 100_000_000 / 600);
    }
}
