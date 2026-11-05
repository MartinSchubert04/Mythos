//! Miniaturas para la vista previa al adelantar (`/{session}/thumbs`, ver `docs/API.md`).
//!
//! Una miniatura cada `interval_s` segundos (a lo sumo `MAX_COUNT`, nunca menos de `MIN_INTERVAL`
//! entre una y otra), de `WIDTH` px de ancho, en JPEG. Cada una sale de un ffmpeg corto que salta a
//! ese momento y decodifica solo el cuadro clave más cercano (`-skip_frame nokey`,
//! `-noaccurate_seek`): unos pocos MB leídos y una fracción de segundo de CPU por miniatura.
//!
//! Para no competir con la reproducción:
//! - Empieza recién cuando el cliente pide el índice y la sesión ya está reproduciendo; se frena
//!   mientras arranca o tras un salto (`buffering`).
//! - ffmpeg lee con el rol pasivo (solo piezas que ya están en disco, sin pedir nada) y solo se
//!   lanza para momentos cuyo tramo del archivo ya está bajado; las que faltan se intentan en la
//!   siguiente pasada, a medida que avanza la descarga.
//! - Un ffmpeg a la vez en todo el server (`SLOTS`), con un hilo, sin NVENC y con la prioridad
//!   más baja del sistema; cada uno con tope de tiempo (`TIMEOUT`).
//! - Orden por pasadas (una cada 16, después cada 8, ...): enseguida hay una vista gruesa de toda
//!   la parte bajada y después se completa.
//!
//! Cache en disco: `data_dir/thumbs/{hash}_{archivo}_{ancho}x{alto}_{intervalo}/{i}.jpg` para
//! torrents (sirve para siempre; se borra con el torrent) o en la carpeta temporal de la sesión
//! para links directos.

use crate::engine::Engine;
use crate::session::PlaySession;
use crate::stream::{json_error, Role};
use crate::MediaState;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::Semaphore;

/// Ancho de cada miniatura (px). El alto sale de la relación de aspecto del video.
pub const WIDTH: u32 = 240;
/// Como mucho tantas miniaturas por archivo (una película de 2 h: una cada 24 s).
pub const MAX_COUNT: usize = 300;
/// Nunca más seguidas que esto (un episodio de 20 min: una cada 5 s).
pub const MIN_INTERVAL: f64 = 5.0;
/// Tope de un ffmpeg (si el tramo no estaba de verdad, el lector pasivo se queda esperando).
const TIMEOUT: Duration = Duration::from_secs(15);
/// Fallos seguidos con el tramo bajado: el archivo no se deja decodificar así; se abandona.
const MAX_FAILS: u32 = 8;
/// Alrededor del byte estimado de un momento, lo que tiene que estar bajado (el cuadro clave
/// puede estar unos segundos antes y la estimación por bitrate medio no es exacta): tantos
/// segundos de video antes y después, con un mínimo y un máximo en bytes.
const BEFORE_S: f64 = 8.0;
const AFTER_S: f64 = 12.0;
const MIN_WINDOW: u64 = 2 << 20;
const MAX_WINDOW: u64 = 24 << 20;

/// Un ffmpeg de miniaturas a la vez en todo el server.
fn slots() -> &'static Semaphore {
    static S: OnceLock<Semaphore> = OnceLock::new();
    S.get_or_init(|| Semaphore::new(1))
}

/// Intervalo y cantidad para una duración (segundos enteros: URLs y cache estables).
pub fn plan(duration: f64) -> (f64, usize) {
    let interval = (duration / MAX_COUNT as f64).ceil().max(MIN_INTERVAL);
    let count = ((duration / interval).ceil() as usize).max(1);
    (interval, count)
}

/// Momento que representa la miniatura `i` (el medio de su tramo). La última se aleja un poco del
/// final: pegado al final, saltar al cuadro clave anterior a veces no devuelve nada.
pub fn time_of(i: usize, interval: f64, duration: f64) -> f64 {
    (i as f64 * interval + interval / 2.0).min((duration - 2.0).max(0.0))
}

/// Alto para `WIDTH` de ancho con esa relación de aspecto (par, para el escalado); 16:9 si no se sabe.
pub fn height_for(w: Option<u32>, h: Option<u32>) -> u32 {
    let (w, h) = match (w, h) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (w as f64, h as f64),
        _ => (16.0, 9.0),
    };
    let v = (WIDTH as f64 * h / w / 2.0).round() as u32 * 2;
    v.clamp(2, WIDTH * 2)
}

/// Orden de generación por pasadas: una cada 16, después las que faltan cada 8, cada 4, 2 y 1.
pub fn order(count: usize) -> Vec<usize> {
    let mut out = Vec::with_capacity(count);
    let mut seen = vec![false; count];
    for step in [16, 8, 4, 2, 1] {
        for i in (0..count).step_by(step) {
            if !seen[i] {
                seen[i] = true;
                out.push(i);
            }
        }
    }
    out
}

/// Tramo de bytes que tiene que estar bajado para el momento `t` (estimado con el bitrate medio).
pub fn byte_window(t: f64, duration: f64, size: u64) -> (u64, u64) {
    if duration <= 0.0 {
        return (0, size);
    }
    let bps = size as f64 / duration;
    let span = |secs: f64| ((bps * secs) as u64).clamp(MIN_WINDOW, MAX_WINDOW);
    let at = (size as f64 * (t / duration).clamp(0.0, 1.0)) as u64;
    (at.saturating_sub(span(BEFORE_S)), (at + span(AFTER_S)).min(size))
}

struct Job {
    dir: PathBuf,
    interval: f64,
    count: usize,
    width: u32,
    height: u32,
    ready: Mutex<Vec<bool>>,
    /// No queda nada por hacer (todas listas, o el archivo no se deja).
    done: AtomicBool,
}

#[derive(Default)]
pub struct ThumbsState {
    job: Mutex<Option<Arc<Job>>>,
}

/// Lo que devuelve `GET /{session}/thumbs`.
#[derive(Serialize)]
pub struct ThumbsIndex {
    pub interval_s: f64,
    pub count: usize,
    pub width: u32,
    pub height: u32,
    /// Un carácter por miniatura: `1` lista, `0` todavía no (o no se pudo).
    pub ready: String,
    /// Ya no van a aparecer más (todas listas, o se dejó de intentar).
    pub done: bool,
}

fn dir_for(e: &Engine, s: &PlaySession, w: u32, h: u32, interval: f64) -> PathBuf {
    let name = format!("{w}x{h}_{}", interval as u64);
    match s.torrent() {
        Some(t) => e.cfg.data_dir.join("thumbs").join(format!("{}_{}_{name}", t.hash, t.idx)),
        None => s.tmp_dir.join(format!("thumbs_{name}")),
    }
}

/// El trabajo de la sesión (lo crea y lo lanza la primera vez). `None` si todavía no se sabe la
/// duración (el cliente vuelve a preguntar).
fn job(e: &Arc<Engine>, s: &Arc<PlaySession>) -> Option<Arc<Job>> {
    let mut slot = s.thumbs.job.lock().unwrap();
    if let Some(j) = slot.as_ref() {
        return Some(j.clone());
    }
    let (duration, w, h) = {
        let info = s.info.lock().unwrap();
        let i = info.as_ref()?;
        (i.duration_s.filter(|d| *d > 1.0)?, i.width, i.height)
    };
    let (interval, count) = plan(duration);
    let height = height_for(w, h);
    let dir = dir_for(e, s, WIDTH, height, interval);
    let _ = std::fs::create_dir_all(&dir);
    let ready: Vec<bool> = (0..count).map(|i| dir.join(format!("{i}.jpg")).is_file()).collect();
    let all = ready.iter().all(|r| *r);
    let j = Arc::new(Job { dir, interval, count, width: WIDTH, height, ready: Mutex::new(ready), done: AtomicBool::new(all) });
    *slot = Some(j.clone());
    if !all {
        tokio::spawn(run(e.clone(), s.clone(), j.clone(), duration));
    }
    Some(j)
}

pub fn index(e: &Arc<Engine>, s: Arc<PlaySession>) -> Response {
    s.touch();
    let Some(j) = job(e, &s) else {
        return json_error(StatusCode::NOT_FOUND, "duration not known yet");
    };
    let ready: String = j.ready.lock().unwrap().iter().map(|r| if *r { '1' } else { '0' }).collect();
    let mut r = axum::Json(ThumbsIndex {
        interval_s: j.interval,
        count: j.count,
        width: j.width,
        height: j.height,
        ready,
        done: j.done.load(Ordering::Relaxed),
    })
    .into_response();
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

pub async fn image(e: &Arc<Engine>, s: Arc<PlaySession>, file: &str) -> Response {
    let Some(i) = file.strip_suffix(".jpg").and_then(|x| x.parse::<usize>().ok()) else {
        return json_error(StatusCode::NOT_FOUND, "no such thumbnail");
    };
    let Some(j) = job(e, &s) else {
        return json_error(StatusCode::NOT_FOUND, "duration not known yet");
    };
    if i >= j.count || !j.ready.lock().unwrap()[i] {
        return json_error(StatusCode::NOT_FOUND, "thumbnail not ready");
    }
    match tokio::fs::read(j.dir.join(file)).await {
        Ok(b) => {
            let mut r = b.into_response();
            let h = r.headers_mut();
            h.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=86400"));
            r
        }
        Err(_) => json_error(StatusCode::NOT_FOUND, "thumbnail not ready"),
    }
}

/// ¿Está bajado el tramo del archivo de ese momento? (Links directos: siempre se puede leer.)
fn available(e: &Engine, s: &PlaySession, t: f64, duration: f64) -> bool {
    let Some(tf) = s.torrent() else { return true };
    let (from, to) = byte_window(t, duration, tf.geom.len);
    let Some(h) = crate::window::haves(&e.api, tf.handle.info_hash()) else { return false };
    (tf.geom.piece_at(from)..=tf.geom.piece_at(to.saturating_sub(1))).all(|p| h.get(p).copied().unwrap_or(false))
}

/// Se puede trabajar: la sesión ya reproduce (no está arrancando ni recién saltó). En un torrent
/// las miniaturas solo leen lo que ya está en disco (no usan ancho de banda), así que alcanza con
/// eso aunque el colchón sea corto; un link directo, en cambio, baja del origen lo que lee: solo
/// si a la sesión no le falta nada.
fn calm(s: &PlaySession) -> bool {
    s.state() == MediaState::Ready && (s.torrent().is_some() || !s.starving())
}

async fn run(e: Arc<Engine>, s: Arc<PlaySession>, j: Arc<Job>, duration: f64) {
    let order = order(j.count);
    let mut fails = 0u32;
    loop {
        let mut pending = false;
        let mut made = false;
        for &i in &order {
            if j.ready.lock().unwrap()[i] {
                continue;
            }
            // Esperar a que la reproducción no necesite nada.
            loop {
                if s.is_closed() {
                    return;
                }
                if calm(&s) {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            let t = time_of(i, j.interval, duration);
            if !available(&e, &s, t, duration) {
                pending = true;
                continue;
            }
            let _slot = slots().acquire().await;
            if s.is_closed() {
                return;
            }
            match extract(&e, &s, &j, i, t).await {
                Ok(()) => {
                    j.ready.lock().unwrap()[i] = true;
                    made = true;
                    fails = 0;
                }
                Err(Fail::Wait) => pending = true,
                Err(Fail::Error(err)) => {
                    pending = true;
                    fails += 1;
                    tracing::debug!(session = %s.id, i, "thumbnail failed: {err}");
                    if fails >= MAX_FAILS {
                        tracing::info!(session = %s.id, "giving up on seek thumbnails for this file");
                        j.done.store(true, Ordering::Relaxed);
                        return;
                    }
                }
            }
        }
        if !pending {
            j.done.store(true, Ordering::Relaxed);
            return;
        }
        // Lo que falta todavía no bajó: otra pasada en un rato.
        tokio::time::sleep(Duration::from_secs(if made { 5 } else { 20 })).await;
        if s.is_closed() {
            return;
        }
    }
}

enum Fail {
    /// Se quedó esperando piezas (la estimación del tramo falló, o se borró algo): otra pasada.
    Wait,
    /// ffmpeg no pudo (cuenta para abandonar).
    Error(anyhow::Error),
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fail::Wait => write!(f, "timed out waiting for pieces"),
            Fail::Error(e) => write!(f, "{e:#}"),
        }
    }
}

async fn extract(e: &Engine, s: &PlaySession, j: &Job, i: usize, t: f64) -> Result<(), Fail> {
    let tmp = j.dir.join(format!("tmp_{i}.jpg"));
    let out = j.dir.join(format!("{i}.jpg"));
    let input = e.internal_url(&s.id, Role::Passive);
    let mut cmd = crate::util::tool_command(&e.cfg.ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-threads", "1", "-skip_frame", "nokey", "-noaccurate_seek", "-ss"])
        .arg(format!("{t:.3}"))
        .args(["-i", &input, "-map", "0:v:0", "-an", "-sn", "-dn", "-frames:v", "1", "-vf"])
        // Rango completo para JPEG: con el de video (limitado) el codificador de ffmpeg se niega en
        // algunos cuadros.
        .arg(format!("scale={}:{}:flags=fast_bilinear:out_range=full,format=yuvj420p", j.width, j.height))
        .args(["-q:v", "6", "-f", "image2"])
        .arg(&tmp)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    lowest_priority(&mut cmd);
    let child = cmd.spawn().map_err(|e| Fail::Error(e.into()))?;
    let res = tokio::time::timeout(TIMEOUT, child.wait_with_output()).await;
    let ok = matches!(&res, Ok(Ok(o)) if o.status.success());
    let size = tokio::fs::metadata(&tmp).await.map(|m| m.len()).unwrap_or(0);
    if !ok || size == 0 {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(match res {
            Err(_) => Fail::Wait,
            Ok(Err(err)) => Fail::Error(err.into()),
            Ok(Ok(o)) => Fail::Error(anyhow::anyhow!("ffmpeg: {}", String::from_utf8_lossy(&o.stderr).trim())),
        });
    }
    tokio::fs::rename(&tmp, &out).await.map_err(|e| Fail::Error(e.into()))?;
    Ok(())
}

/// Las miniaturas nunca le sacan CPU a nadie: prioridad más baja del sistema.
fn lowest_priority(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW | IDLE_PRIORITY_CLASS
        cmd.creation_flags(0x0800_0000 | 0x0000_0040);
    }
    #[cfg(unix)]
    {
        // SAFETY: setpriority es async-signal-safe y no toca memoria del padre.
        unsafe {
            cmd.pre_exec(|| {
                libc::setpriority(libc::PRIO_PROCESS, 0, 19);
                Ok(())
            });
        }
    }
    #[cfg(not(any(windows, unix)))]
    let _ = cmd;
}

/// Al borrar un torrent con sus archivos, también sus miniaturas.
pub async fn forget_torrent(data_dir: &Path, hash: &str) {
    let Ok(mut rd) = tokio::fs::read_dir(data_dir.join("thumbs")).await else { return };
    while let Ok(Some(ent)) = rd.next_entry().await {
        if ent.file_name().to_string_lossy().starts_with(hash) {
            let _ = tokio::fs::remove_dir_all(ent.path()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_limits() {
        // 20 min: cada 5 s (el mínimo), 240 miniaturas.
        assert_eq!(plan(1200.0), (5.0, 240));
        // 2 h: tope de 300 → cada 24 s.
        assert_eq!(plan(7200.0), (24.0, 300));
        // 2 h 10 min: 26 s, sin pasarse de 300.
        let (i, n) = plan(7800.0);
        assert_eq!(i, 26.0);
        assert!(n <= MAX_COUNT);
        // Muy corto: al menos una.
        assert_eq!(plan(3.0), (5.0, 1));
    }

    #[test]
    fn times_inside_the_file() {
        let (iv, n) = plan(1203.0);
        assert_eq!(time_of(0, iv, 1203.0), 2.5);
        assert!(time_of(n - 1, iv, 1203.0) <= 1201.0);
    }

    #[test]
    fn heights() {
        assert_eq!(height_for(Some(1920), Some(1080)), 136);
        assert_eq!(height_for(Some(3840), Some(1600)), 100);
        assert_eq!(height_for(None, None), 136);
        assert_eq!(height_for(Some(1440), Some(1080)), 180);
    }

    #[test]
    fn order_is_coarse_first_and_complete() {
        let o = order(40);
        assert_eq!(&o[..3], &[0, 16, 32]);
        assert_eq!(&o[3..5], &[8, 24]);
        let mut s = o.clone();
        s.sort();
        assert_eq!(s, (0..40).collect::<Vec<_>>());
    }

    #[test]
    fn window_clamped() {
        // 1 GB en 100 s (10 MB/s): 8 s antes y 12 s después pasan el tope.
        assert_eq!(byte_window(0.0, 100.0, 1 << 30), (0, MAX_WINDOW));
        let (a, b) = byte_window(100.0, 100.0, 1 << 30);
        assert_eq!(b, 1 << 30);
        assert_eq!(a, (1 << 30) - MAX_WINDOW);
        // 129 MB en 888 s (~145 KB/s): el mínimo.
        let (a, b) = byte_window(444.0, 888.0, 129_000_000);
        assert_eq!((b - a), MIN_WINDOW * 2);
        // 4 GB en 2 h (~556 KB/s): 8 y 12 s de video.
        let (a, b) = byte_window(3600.0, 7200.0, 4_000_000_000);
        assert_eq!(2_000_000_000 - a, 4_444_444);
        assert_eq!(b - 2_000_000_000, 6_666_666);
        // Sin duración: todo el archivo.
        assert_eq!(byte_window(5.0, 0.0, 1000), (0, 1000));
    }
}
