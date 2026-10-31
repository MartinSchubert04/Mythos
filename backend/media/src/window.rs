//! Ventana de prioridad deslizante: lo más importante para que la película no se trabe.
//!
//! Cómo prioriza librqbit (leído de su código, v9.1): cada `FileStream` vivo aporta una ventana fija
//! de 32 MB desde su posición, y cuando un peer pide trabajo se le da la primera pieza que falte de
//! esas ventanas (intercaladas entre streams); si no hay, sigue el orden natural del archivo
//! (primera pieza, última, y después de principio a fin). Problemas de eso para ver una película:
//! - 32 MB son 2–8 piezas en torrents grandes: con 50 peers la mayoría queda bajando en orden
//!   natural, que después de un salto es *detrás* del espectador.
//! - Si el lector ya tiene sus 32 MB, nadie empuja lo que viene después.
//!
//! Lo que hacemos (cada segundo, en `run`):
//! 1. Calculamos la "frontera": la primera pieza que falta desde la posición de lectura.
//! 2. Ponemos streams "fantasma" (no leen nada, solo existen para dar prioridad) en la frontera, y
//!    sumamos ventanas de 32 MB consecutivas solo mientras haya más peers que piezas faltantes
//!    cubiertas (`plan_windows`). Con pocos peers queda una sola ventana pegada a la frontera, así
//!    no se diluye la prioridad de lo inmediato; con muchos, todos bajan por delante del lector.
//! 3. La frontera siempre existe mientras falte algo después del lector: el resto del archivo se
//!    sigue bajando en orden hacia adelante en segundo plano, nunca "se frena" la ventana.
//! 4. Al abrir, un fantasma en el final del archivo (moov de MP4 / cues de MKV) hasta tenerlo.
//! 5. El buffer inicial es adaptativo (`buffer_target_s`): si la bajada es mucho más rápida que el
//!    bitrate alcanza con pocos segundos; si es más lenta, se espera más.
//!
//! Endgame: librqbit no expone "deadline" por pieza; lo que tiene es robo de piezas a peers 3x/10x
//! más lentos, que se activa justamente con la pieza que el lector espera porque queda primera en
//! todas las ventanas. Además bajamos `max_request_window` (ver `engine`) para que una pieza nueva
//! se pida antes tras un salto.

use crate::session::PlaySession;
use librqbit::{api::TorrentIdOrHash, Api, ManagedTorrent};
use std::io::SeekFrom;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncSeekExt;

/// Ventana fija por stream dentro de librqbit (`PER_STREAM_BUF_DEFAULT`).
pub const LIB_WINDOW: u64 = 32 * 1024 * 1024;
/// Máximo de fantasmas de ventana por sesión (cada stream ocupa un permiso del semáforo de
/// librqbit; ver `engine::BLOCKING_PERMITS`).
pub const MAX_WINDOWS: usize = 6;

/// Geometría del archivo dentro del torrent.
#[derive(Clone, Copy, Debug)]
pub struct FileGeom {
    /// Offset del archivo dentro del torrent.
    pub offset: u64,
    pub len: u64,
    pub piece_len: u64,
}

impl FileGeom {
    /// Pieza que contiene el byte `pos` del archivo.
    pub fn piece_at(&self, pos: u64) -> usize {
        ((self.offset + pos.min(self.len.saturating_sub(1))) / self.piece_len) as usize
    }
    pub fn last_piece(&self) -> usize {
        self.piece_at(self.len.saturating_sub(1))
    }
    /// Offset (relativo al archivo) donde empieza la pieza, recortado a 0.
    pub fn piece_start(&self, piece: usize) -> u64 {
        (piece as u64 * self.piece_len).saturating_sub(self.offset).min(self.len)
    }
    /// Offset (relativo al archivo) donde termina la pieza (exclusivo), recortado al largo.
    pub fn piece_end(&self, piece: usize) -> u64 {
        ((piece as u64 + 1) * self.piece_len).saturating_sub(self.offset).min(self.len)
    }
}

/// Bytes contiguos ya bajados desde `pos`.
pub fn contiguous_ahead(g: &FileGeom, have: &dyn Fn(usize) -> bool, pos: u64) -> u64 {
    if pos >= g.len {
        return 0;
    }
    let mut p = g.piece_at(pos);
    let last = g.last_piece();
    while p <= last && have(p) {
        p += 1;
    }
    if p == g.piece_at(pos) {
        return 0;
    }
    g.piece_end(p - 1).saturating_sub(pos)
}

/// Primera pieza que falta desde `pos` hasta el final del archivo.
pub fn first_missing_from(g: &FileGeom, have: &dyn Fn(usize) -> bool, pos: u64) -> Option<usize> {
    if pos >= g.len {
        return None;
    }
    (g.piece_at(pos)..=g.last_piece()).find(|&p| !have(p))
}

/// Piezas que faltan dentro de `[from, from+len)` del archivo.
pub fn missing_in(g: &FileGeom, have: &dyn Fn(usize) -> bool, from: u64, len: u64) -> usize {
    if from >= g.len || len == 0 {
        return 0;
    }
    let end = (from + len).min(g.len);
    (g.piece_at(from)..=g.piece_at(end - 1)).filter(|&p| !have(p)).count()
}

/// Posiciones (relativas al archivo) de los fantasmas de ventana desde la frontera.
/// Se suma una ventana más solo si hay más peers que piezas faltantes en las anteriores.
pub fn plan_windows(g: &FileGeom, have: &dyn Fn(usize) -> bool, frontier: u64, peers: u32, max: usize) -> Vec<u64> {
    let mut out = Vec::new();
    if frontier >= g.len || max == 0 {
        return out;
    }
    let mut pos = frontier;
    let mut covered = 0usize;
    while out.len() < max && pos < g.len {
        out.push(pos);
        covered += missing_in(g, have, pos, LIB_WINDOW);
        if (peers as usize) <= covered {
            break;
        }
        pos += LIB_WINDOW;
    }
    out
}

/// Cuánto final del archivo bajar al abrir (índices de MP4, cues de MKV).
pub fn tail_bytes(len: u64) -> u64 {
    (len / 500).clamp(4 << 20, 16 << 20).min(len)
}

/// Bytes por delante que queremos como colchón: 120 s de video, al menos 64 MB.
pub fn readahead_target_bytes(bitrate: f64) -> u64 {
    ((bitrate.max(0.0) * 120.0) as u64).max(64 << 20)
}

/// Segundos de buffer antes de pasar a `ready`, según velocidad vs. bitrate (ambos en bytes/s) y
/// cuánto hace que se espera (para no esperar para siempre con un torrent lento).
pub fn buffer_target_s(bitrate: f64, down: f64, waited_s: f64) -> f64 {
    let base = if bitrate <= 0.0 {
        8.0
    } else {
        let ratio = down / bitrate;
        if ratio >= 2.0 {
            4.0
        } else if ratio >= 1.3 {
            8.0
        } else if ratio >= 1.0 {
            15.0
        } else {
            // Bajando más lento que lo que se consume: cuanto más lento, más colchón (hasta 60 s).
            (15.0 + 45.0 * (1.0 - ratio)).min(60.0)
        }
    };
    // Tras 45 s esperando, se va relajando hasta un mínimo de 3 s.
    if waited_s > 45.0 {
        (base - (waited_s - 45.0) / 2.0).max(3.0)
    } else {
        base
    }
}

type ManagedTorrentHandle = Arc<ManagedTorrent>;

/// Stream "fantasma": un `FileStream` de librqbit que nunca se lee; solo existe para que su
/// ventana de 32 MB tenga prioridad. (El tipo `FileStream` no es nombrable fuera de librqbit, por
/// eso va en una caja y llevamos la posición nosotros.)
pub struct Phantom {
    s: Box<dyn tokio::io::AsyncSeek + Unpin + Send>,
    pos: u64,
}

impl Phantom {
    pub async fn open(h: &ManagedTorrentHandle, idx: usize, pos: u64) -> Option<Phantom> {
        let mut s = h.clone().stream(idx).await.ok()?;
        s.seek(SeekFrom::Start(pos)).await.ok()?;
        Some(Phantom { s: Box::new(s), pos })
    }

    pub async fn move_to(&mut self, pos: u64) {
        if self.pos != pos && self.s.seek(SeekFrom::Start(pos)).await.is_ok() {
            self.pos = pos;
        }
    }
}

/// Lee el bitfield de piezas que ya tenemos (copia chica: piezas/8 bytes).
pub fn haves(api: &Api, hash: librqbit::dht::Id20) -> Option<Vec<bool>> {
    let (bf, len) = api.api_dump_haves(TorrentIdOrHash::Hash(hash)).ok()?;
    Some((0..len as usize).map(|i| bf.get(i).map(|b| *b).unwrap_or(false)).collect())
}

/// Bucle por sesión de torrent: reubica fantasmas y actualiza el buffer de la sesión.
pub async fn run(api: Api, handle: ManagedTorrentHandle, file_idx: usize, g: FileGeom, sess: Arc<PlaySession>) {
    let hash = handle.info_hash();
    let tail_pos = g.len - tail_bytes(g.len);
    let mut tail: Option<Phantom> = None;
    let mut tail_done = false;
    let mut windows: Vec<Phantom> = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_millis(700));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if sess.is_closed() {
            break;
        }
        let Some(hv) = haves(&api, hash) else {
            if let Some(err) = handle.stats().error {
                sess.set_error(format!("torrent error: {err}"));
            }
            continue;
        };
        let have = |p: usize| hv.get(p).copied().unwrap_or(false);
        let head = sess.head();

        // Final del archivo: se pide al abrir y se suelta cuando está completo.
        if !tail_done {
            if missing_in(&g, &have, tail_pos, g.len - tail_pos) == 0 {
                tail_done = true;
                tail = None;
            } else if tail.is_none() {
                tail = Phantom::open(&handle, file_idx, tail_pos).await;
            }
        }

        let ahead = contiguous_ahead(&g, &have, head);
        let peers = handle.live().map(|l| l.stats_snapshot().peer_stats.live).unwrap_or(0);
        // Por debajo del colchón objetivo: todas las ventanas que los peers puedan llenar.
        // Con el colchón cubierto: una sola (sigue bajando en orden hacia adelante y deja que el
        // resto de los peers rellene huecos de atrás, útil si el usuario vuelve para atrás).
        let max_w = if ahead < readahead_target_bytes(sess.bitrate()) { MAX_WINDOWS } else { 1 };
        let plan = match first_missing_from(&g, &have, head) {
            Some(p) => plan_windows(&g, &have, g.piece_start(p).max(head.min(g.len)), peers.max(1), max_w),
            None => Vec::new(),
        };
        windows.truncate(plan.len());
        for (i, pos) in plan.iter().enumerate() {
            match windows.get_mut(i) {
                Some(w) => w.move_to(*pos).await,
                None => match Phantom::open(&handle, file_idx, *pos).await {
                    Some(w) => windows.push(w),
                    None => break,
                },
            }
        }

        let complete = first_missing_from(&g, &have, 0).is_none();
        sess.update_buffer(ahead, complete);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1 << 20;

    fn geom() -> FileGeom {
        // Archivo de 100 MB que empieza a mitad de la pieza 0, piezas de 4 MB.
        FileGeom { offset: 2 * MB, len: 100 * MB, piece_len: 4 * MB }
    }

    #[test]
    fn geometry() {
        let g = geom();
        assert_eq!(g.piece_at(0), 0);
        assert_eq!(g.piece_at(2 * MB), 1);
        assert_eq!(g.last_piece(), (102 * MB - 1) as usize / (4 * MB) as usize);
        assert_eq!(g.piece_start(0), 0);
        assert_eq!(g.piece_start(1), 2 * MB);
        assert_eq!(g.piece_end(0), 2 * MB);
        assert_eq!(g.piece_end(g.last_piece()), 100 * MB);
    }

    #[test]
    fn contiguous_and_frontier() {
        let g = geom();
        let have = |p: usize| p < 5 || p == 9;
        // Piezas 0..=4 → hasta el final de la pieza 4 = 20 MB - 2 MB = 18 MB del archivo.
        assert_eq!(contiguous_ahead(&g, &have, 0), 18 * MB);
        assert_eq!(contiguous_ahead(&g, &have, 10 * MB), 8 * MB);
        assert_eq!(contiguous_ahead(&g, &have, 30 * MB), 0);
        assert_eq!(first_missing_from(&g, &have, 0), Some(5));
        assert_eq!(first_missing_from(&g, &have, 34 * MB), Some(10));
        let all = |_: usize| true;
        assert_eq!(first_missing_from(&g, &all, 0), None);
        assert_eq!(contiguous_ahead(&g, &all, 99 * MB), MB);
    }

    #[test]
    fn windows_grow_with_peers() {
        let g = FileGeom { offset: 0, len: 1000 * MB, piece_len: 16 * MB };
        let none = |_: usize| false;
        // 1 peer: una sola ventana (no diluir lo inmediato).
        assert_eq!(plan_windows(&g, &none, 0, 1, MAX_WINDOWS), vec![0]);
        // Ventana de 32 MB = 2 piezas: con 5 peers hacen falta 3 ventanas.
        assert_eq!(plan_windows(&g, &none, 0, 5, MAX_WINDOWS), vec![0, 32 * MB, 64 * MB]);
        // Muchos peers: tope.
        assert_eq!(plan_windows(&g, &none, 0, 500, MAX_WINDOWS).len(), MAX_WINDOWS);
        // Cerca del final no se pasa del archivo.
        assert_eq!(plan_windows(&g, &none, 990 * MB, 500, MAX_WINDOWS), vec![990 * MB]);
        assert!(plan_windows(&g, &none, 1000 * MB, 5, MAX_WINDOWS).is_empty());
    }

    #[test]
    fn buffer_target_adapts() {
        let br = 1_000_000.0;
        assert_eq!(buffer_target_s(br, 5.0 * br, 0.0), 4.0);
        assert_eq!(buffer_target_s(br, 1.5 * br, 0.0), 8.0);
        assert_eq!(buffer_target_s(br, 1.1 * br, 0.0), 15.0);
        assert!((buffer_target_s(br, 0.5 * br, 0.0) - 37.5).abs() < 1e-9);
        assert_eq!(buffer_target_s(br, 0.0, 0.0), 60.0);
        assert_eq!(buffer_target_s(br, 0.0, 500.0), 3.0);
        assert_eq!(buffer_target_s(0.0, 0.0, 0.0), 8.0);
    }

    #[test]
    fn readahead_and_tail() {
        assert_eq!(readahead_target_bytes(0.0), 64 * MB);
        assert_eq!(readahead_target_bytes(2.0 * MB as f64), 240 * MB);
        assert_eq!(tail_bytes(100 * MB), 4 * MB);
        assert_eq!(tail_bytes(20_000 * MB), 16 * MB);
        assert_eq!(tail_bytes(MB), MB);
    }
}
