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
//!    Si la bajada no llega a 1.5× el bitrate (o el colchón ya pasa los 120 s), una sola ventana.
//! 3. La frontera siempre existe mientras falte algo después del lector: el resto del archivo se
//!    sigue bajando en orden hacia adelante en segundo plano, nunca "se frena" la ventana.
//! 4. Al abrir, un fantasma en el final del archivo (moov de MP4 / cues de MKV) hasta tenerlo.
//! 5. El buffer inicial es adaptativo (`buffer_target_s`): si la bajada es mucho más rápida que el
//!    bitrate alcanza con pocos segundos; si es más lenta, se espera más.
//!
//! 6. El colchón se mide desde el **origen** (donde arrancó la reproducción o el último salto), no
//!    desde el lector: un `<video>` se lleva todo lo que hay apenas llega, así que medido desde el
//!    lector el colchón siempre daba 0. Se descuenta lo que se supone ya reproducido.
//! 7. **Ráfagas** (`burst_s`): cada peer trabaja en una pieza distinta y, si el cuello de botella es
//!    nuestra bajada, todas terminan juntas; el colchón objetivo cubre al menos una ráfaga.
//! 8. **Rescate de pieza trabada** (`should_rescue`): si la pieza que espera el reproductor no
//!    llega en 12 s mientras llegan otras y el colchón se acaba, se pausa y reanuda el torrent:
//!    las piezas en vuelo de peers lentos vuelven a la cola (librqbit solo roba piezas a peers 3x
//!    más lentos que el promedio, y con la bajada saturada el promedio es enorme).
//!
//! Endgame: librqbit no expone "deadline" por pieza; además del rescate, la pieza que espera el
//! lector queda primera en todas las ventanas y `max_request_window` acotado hace que se pida
//! antes tras un salto.
//!
//! ## Mediciones (`examples/sim.rs`, Tears of Steel, 571 MB, 778 KB/s de bitrate, piezas de 512 KB)
//!
//! El simulador consume a 1× como un `<video>` (lee hasta 60 s por delante), salta al 50 % y al
//! 80 %, y espera `ready` tras cada salto (`--wait-ready`, lo que debería hacer la UI). 150 s por
//! caso. "Salto" = tiempo hasta poder seguir. Valores separados por "/" = corridas repetidas.
//!
//! **Swarm lento** (sembradores locales con tope de subida, `examples/seed.rs`; es el caso típico
//! de un torrent con pocos seeders):
//!
//! | caso | ready | trabas | salto |
//! |---|---|---|---|
//! | 1.5× (4×292 KB/s), cola 16 / 32 / 64 | 9.9–10 s | 0 | 9–12 / 12–13 / 11–13 s |
//! | 1.5×, cola 128 | 10 s | 0 | 15–16 s (pedidos viejos delante de los nuevos) |
//! | 1.5×, con descarte al saltar (`flush_on_seek`) | 9.9 s | 0 | **7.3 / 7.3 s** (sin: 12 / 12 s) |
//! | 1.5×, 8 sembradores × 146 KB/s | 13.5 s | 0 | 12–20 s |
//! | 1.5×, sin esperar `ready` tras saltar | 9.9 s | 0 | 8–9 s |
//! | 0.8× (4×155 KB/s) | 33–35 s | 0 en 150 s | 34–40 s |
//! | 0.8×, sin esperar `ready` | 33 s | 5 cortas (7 s en total) | 19 s |
//!
//! **Bajada propia saturada** (contenedor Linux con `tc` policing de ingreso contra el swarm
//! público; TCP/uTP se adaptan como en una conexión lenta de verdad):
//!
//! | caso | ready | trabas (máx) | salto |
//! |---|---|---|---|
//! | 9.3 Mbit (1.2×), cola 64 | 37–55 s | 1–2 (11–34 s) | 28–56 s |
//! | 9.3 Mbit, cola 16 | 25–30 s | 1 (6–25 s) | 27–37 s |
//! | 9.3 Mbit, sin rescate | 76 s | 1 de 53 s | — |
//! | 18.7 Mbit (2.4×) | 19–28 s | 1–2 (31–38 s) | 10–19 s |
//! | 5 Mbit (0.65×) | 106 s (el probe solo tardó 50 s) | 1 (24 s) | — |
//!
//! Conclusiones aplicadas a los valores por defecto (`Tuning::default`):
//! - Cola por peer **32**: con la bajada saturada 16 fue mejor que 64 (lo pedido y no recibido
//!   tapa lo urgente); con peers lentos dio igual; 128 empeora siempre. 32 deja 512 KB por RTT
//!   (≈ 2.5 MB/s por peer a 200 ms), para no frenar a un único seeder rápido y lejano.
//! - **Rescate** activado (sin él, `ready` a 76 s y una traba de 53 s) y **descarte al saltar**
//!   activado (saltos 40 % más rápidos con peers lentos; con la bajada saturada, neutro).
//! - Colchón con **ráfaga**, medido desde el origen.
//! - Con la bajada propia saturada las trabas no desaparecen: librqbit reparte la bajada entre
//!   una pieza por peer y todas llegan juntas. Bajar `peer_limit` lo mejoraría en ese caso pero
//!   empeora los swarms lentos (menos fuentes), así que queda en 160.
//! - Errores encontrados al medir: el colchón medido desde el lector siempre daba 0 (el lector se
//!   lleva todo apenas llega); sin ffprobe el bitrate supuesto (2 h) era 10× menor que el real y
//!   `ready` salía con 1 s de colchón (ahora se estima por el nombre, `guess_bitrate`).
//! - El tope de bajada interno de librqbit (`Tuning::download_limit_bps`) **no** sirve para
//!   emular: su limitador deja esperando a algunos peers muchísimo y daba `ready` a 54 s aun con
//!   3× del bitrate. Por eso las mediciones usan sembradores lentos y `tc`.

use crate::session::PlaySession;
use crate::engine::Engine;
use librqbit::{api::TorrentIdOrHash, Api, ManagedTorrent};
use std::io::SeekFrom;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::io::AsyncSeekExt;

/// Ventana fija por stream dentro de librqbit (`PER_STREAM_BUF_DEFAULT`).
pub const LIB_WINDOW: u64 = 32 * 1024 * 1024;

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

/// Tramos del archivo ya bajados, como fracciones `[desde, hasta)` de 0 a 1 (para dibujarlos en la
/// barra). Si quedan más de `max`, se juntan los separados por los huecos más chicos.
pub fn have_ranges(g: &FileGeom, have: &dyn Fn(usize) -> bool, max: usize) -> Vec<(f64, f64)> {
    if g.len == 0 {
        return Vec::new();
    }
    let mut out: Vec<(u64, u64)> = Vec::new();
    for p in g.piece_at(0)..=g.last_piece() {
        if !have(p) {
            continue;
        }
        let (a, b) = (g.piece_start(p), g.piece_end(p));
        match out.last_mut() {
            Some(last) if last.1 >= a => last.1 = b,
            _ => out.push((a, b)),
        }
    }
    while out.len() > max.max(1) {
        // Junta el par con el hueco más chico.
        let i = (1..out.len()).min_by_key(|&i| out[i].0 - out[i - 1].1).unwrap_or(1);
        out[i - 1].1 = out[i].1;
        out.remove(i);
    }
    let f = |x: u64| (x as f64 / g.len as f64 * 10_000.0).round() / 10_000.0;
    out.into_iter().map(|(a, b)| (f(a), f(b))).collect()
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

/// Segundos de buffer antes de pasar a `ready`, según velocidad vs. bitrate (ambos en bytes/s),
/// la "ráfaga" (`burst_s`, ver `burst_s`) y cuánto hace que se espera (para no esperar para
/// siempre con un torrent lento).
pub fn buffer_target_s(bitrate: f64, down: f64, waited_s: f64, burst_s: f64) -> f64 {
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
    // Si las piezas llegan en ráfagas cada `burst_s`, el colchón tiene que cubrir al menos una.
    let base = base.max((burst_s * 1.25).min(60.0));
    // Tras 45 s esperando, se va relajando hasta un mínimo de 3 s.
    if waited_s > 45.0 {
        (base - (waited_s - 45.0) / 2.0).max(3.0)
    } else {
        base
    }
}

/// Cada peer trabaja en una pieza distinta y, si el cuello de botella es nuestra bajada, todas
/// avanzan a la vez y terminan juntas: la pieza que sigue al lector tarda ≈ peers × pieza / bajada.
/// Esa "ráfaga" es lo mínimo que tiene que aguantar el colchón (medido: con 30 peers, piezas de
/// 1 MB y 1.1 MB/s son ~27 s, y un colchón de 8 s se trababa en cada ráfaga).
pub fn burst_s(peers: u32, piece_len: u64, down: f64) -> f64 {
    if down <= 0.0 {
        return 0.0;
    }
    (peers.min(64) as f64 * piece_len as f64 / down).min(48.0)
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

/// Mensaje legible para un torrent en error (el de librqbit es un `Debug` largo).
pub fn friendly_error(err: &str) -> String {
    let e = err.to_ascii_lowercase();
    if e.contains("no space") || e.contains("enospc") || e.contains("os error 28") || e.contains("os error 112") {
        crate::engine::DISK_FULL_MSG.to_string()
    } else {
        let first = err.lines().next().unwrap_or(err);
        format!("torrent error: {}", first.chars().take(200).collect::<String>())
    }
}

/// Lee el bitfield de piezas que ya tenemos (copia chica: piezas/8 bytes).
pub fn haves(api: &Api, hash: librqbit::dht::Id20) -> Option<Vec<bool>> {
    let (bf, len) = api.api_dump_haves(TorrentIdOrHash::Hash(hash)).ok()?;
    Some((0..len as usize).map(|i| bf.get(i).map(|b| *b).unwrap_or(false)).collect())
}

/// Bucle por sesión de torrent: reubica fantasmas y actualiza el buffer de la sesión.
pub async fn run(engine: Weak<Engine>, handle: ManagedTorrentHandle, file_idx: usize, g: FileGeom, sess: Arc<PlaySession>) {
    let Some(e) = engine.upgrade() else { return };
    let api = e.api.clone();
    let tuning = e.tuning.clone();
    drop(e);
    let hash = handle.info_hash();
    let tail_pos = g.len - tail_bytes(g.len);
    let mut tail: Option<Phantom> = None;
    let mut tail_done = false;
    let mut windows: Vec<Phantom> = Vec::new();
    let mut stuck: Option<Stuck> = None;
    let mut last_rescue: Option<tokio::time::Instant> = None;
    let mut last_origin = sess.origin();
    let mut tick = tokio::time::interval(Duration::from_millis(700));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if sess.is_closed() {
            break;
        }
        match engine.upgrade() {
            Some(e) => e.guard_disk().await,
            None => break,
        }
        let Some(hv) = haves(&api, hash) else {
            tracing::debug!(session = %sess.id, "no piece bitfield yet");
            if let Some(err) = handle.stats().error {
                sess.set_error(friendly_error(&err));
            }
            continue;
        };
        let hv = Arc::new(hv);
        *sess.haves.lock().unwrap() = hv.clone();
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

        let peers = handle.live().map(|l| l.stats_snapshot().peer_stats.live).unwrap_or(0);
        // Por debajo del colchón objetivo: todas las ventanas que los peers puedan llenar.
        // Con el colchón cubierto: una sola (sigue bajando en orden hacia adelante y deja que el
        // resto de los peers rellene huecos de atrás, útil si el usuario vuelve para atrás).
        let br = sess.bitrate();
        let down = handle.live().map(|l| l.down_speed_estimator().bps()).unwrap_or(0) as f64;
        let slow = tuning.focus_when_slow && down < 1.5 * br;
        let max_w = if sess.ahead_s() * br < readahead_target_bytes(br) as f64 && !slow { tuning.max_windows } else { 1 };
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

        tracing::debug!(
            session = %sess.id,
            head,
            first_missing = ?first_missing_from(&g, &have, sess.origin()),
            missing_32mb = missing_in(&g, &have, sess.origin(), LIB_WINDOW),
            piece_len = g.piece_len,
            ahead_s = sess.ahead_s(),
            peers,
            down,
            windows = ?plan,
            tail = tail.is_some(),
            "priority windows"
        );
        let complete = first_missing_from(&g, &have, 0).is_none();
        sess.update_buffer(contiguous_ahead(&g, &have, sess.origin()), complete, burst_s(peers, g.piece_len, down));

        // Salto: con la bajada justa, los pedidos en vuelo de la posición vieja (cada peer tiene
        // una pieza reservada y hasta 1 MB pedido) demoran lo nuevo; se descartan.
        let origin = sess.origin();
        if origin != last_origin {
            last_origin = origin;
            let can = last_rescue.is_none_or(|t| t.elapsed() > Duration::from_secs(10));
            if tuning.flush_on_seek && can && down < 3.0 * br && !have(g.piece_at(origin)) && handle.live().is_some() {
                if let Some(e) = engine.upgrade() {
                    tracing::info!(session = %sess.id, origin, "seek: flushing in-flight requests");
                    e.rescue(&handle).await;
                    last_rescue = Some(tokio::time::Instant::now());
                    stuck = None;
                }
            }
        }

        // Rescate: la pieza que espera el reproductor no llega mientras otras sí.
        if tuning.rescue_stuck {
            let have_count = hv.iter().filter(|x| **x).count();
            let blocking = first_missing_from(&g, &have, sess.origin());
            stuck = match (stuck, blocking) {
                (Some(st), Some(p)) if st.piece == p => Some(st),
                (_, Some(p)) => Some(Stuck { piece: p, since: tokio::time::Instant::now(), have_count }),
                (_, None) => None,
            };
            if let Some(st) = stuck {
                let waited = st.since.elapsed().as_secs_f64();
                let others = have_count.saturating_sub(st.have_count) as u64 * g.piece_len;
                let can = last_rescue.is_none_or(|t| t.elapsed() > RESCUE_EVERY);
                if should_rescue(waited, others, sess.ahead_s(), g.piece_len) && can && handle.live().is_some() {
                    if let Some(e) = engine.upgrade() {
                        tracing::info!(session = %sess.id, piece = st.piece, waited, others, "head piece stuck: restarting peer connections");
                        e.rescue(&handle).await;
                        last_rescue = Some(tokio::time::Instant::now());
                        stuck = None;
                    }
                }
            }
        }
    }
}

/// Estado de la pieza que bloquea al reproductor.
#[derive(Clone, Copy)]
struct Stuck {
    piece: usize,
    since: tokio::time::Instant,
    have_count: usize,
}

const RESCUE_EVERY: Duration = Duration::from_secs(45);

/// ¿Vale la pena el rescate? La pieza bloqueante lleva `waited` s sin llegar, mientras llegaron
/// `others` bytes de otras piezas, y al reproductor le quedan `ahead_s` s. Solo si la descarga
/// anda (llegaron al menos 8 piezas o 8 MB: no es que no haya peers), la espera ya es larga
/// (12 s) y el colchón está por acabarse.
pub fn should_rescue(waited: f64, others: u64, ahead_s: f64, piece_len: u64) -> bool {
    waited >= 12.0 && others >= (8 * piece_len).min(8 << 20) && ahead_s < 5.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1 << 20;
    const MAX_WINDOWS: usize = 6;

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
    fn downloaded_ranges() {
        let g = FileGeom { offset: 0, len: 100 * MB, piece_len: 10 * MB };
        let have = |p: usize| p < 3 || p == 5 || p == 6 || p >= 8;
        assert_eq!(have_ranges(&g, &have, 64), vec![(0.0, 0.3), (0.5, 0.7), (0.8, 1.0)]);
        // Tope de 2: se junta el hueco más chico (0.7–0.8).
        assert_eq!(have_ranges(&g, &have, 2), vec![(0.0, 0.3), (0.5, 1.0)]);
        assert_eq!(have_ranges(&g, &|_| true, 64), vec![(0.0, 1.0)]);
        assert!(have_ranges(&g, &|_| false, 64).is_empty());
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
        assert_eq!(buffer_target_s(br, 5.0 * br, 0.0, 0.0), 4.0);
        assert_eq!(buffer_target_s(br, 1.5 * br, 0.0, 0.0), 8.0);
        assert_eq!(buffer_target_s(br, 1.1 * br, 0.0, 0.0), 15.0);
        assert!((buffer_target_s(br, 0.5 * br, 0.0, 0.0) - 37.5).abs() < 1e-9);
        assert_eq!(buffer_target_s(br, 0.0, 0.0, 0.0), 60.0);
        assert_eq!(buffer_target_s(br, 0.0, 500.0, 0.0), 3.0);
        assert_eq!(buffer_target_s(0.0, 0.0, 0.0, 0.0), 8.0);
        // Ráfagas de 20 s: aunque la bajada sobre, hay que aguantar 25 s.
        assert_eq!(buffer_target_s(br, 5.0 * br, 0.0, 20.0), 25.0);
        // 30 peers × 1 MB a 1.1 MB/s ≈ 27 s; tope 48 s.
        assert!((burst_s(30, MB, 1.1 * MB as f64) - 27.27).abs() < 0.1);
        assert_eq!(burst_s(500, 16 * MB, 1.0), 48.0);
        assert_eq!(burst_s(30, MB, 0.0), 0.0);
    }

    #[test]
    fn rescue_rules() {
        assert!(should_rescue(15.0, 10 * MB, 1.0, MB));
        assert!(!should_rescue(5.0, 10 * MB, 1.0, MB), "todavía no esperó bastante");
        assert!(!should_rescue(15.0, MB, 1.0, MB), "no llegó nada más: no hay peers, no es una pieza trabada");
        assert!(!should_rescue(15.0, 10 * MB, 30.0, MB), "hay colchón: no vale la pena cortar conexiones");
        // Piezas de 16 MB: alcanza con 8 MB de otras piezas.
        assert!(should_rescue(15.0, 8 * MB, 0.0, 16 * MB));
    }

    #[test]
    fn errors_are_readable() {
        assert!(friendly_error("FATAL: error writing chunk: pwritev: ENOSPC").contains("disk space"));
        assert!(friendly_error("No space left on device (os error 28)").contains("disk space"));
        assert_eq!(friendly_error("boom
details"), "torrent error: boom");
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
