//! HLS en vivo con ffmpeg, para cuando el cliente no puede reproducir el archivo tal cual.
//!
//! - **Remux** (`-c:v copy`): el video ya lo soporta el cliente y solo cambia el contenedor (y el
//!   audio, si hace falta, a AAC). Casi no usa CPU.
//! - **Transcode**: `h264_nvenc` (preset rápido, `-tune ll`) o `libx264 veryfast` si no hay GPU;
//!   4K → 1080p y HDR → SDR con tonemap (zscale + hable) porque el cliente no tiene HEVC/HDR.
//! - Audio AAC estéreo, o 5.1 si la pista es multicanal; la pista se elige con `?audio=`.
//! - fMP4 (`.m4s`) de 4 s con keyframes forzados cada 4 s, playlist tipo EVENT que crece.
//!
//! Anti-trabas:
//! - ffmpeg lee por el endpoint interno con rol `hls`, así sus lecturas mueven la ventana de
//!   prioridad igual que el reproductor directo.
//! - Pedir un segmento (o la playlist) que todavía no existe **espera** a que exista (con
//!   timeout) en vez de dar 404, que haría que hls.js lo dé por perdido.
//! - `-hls_flags temp_file`: ffmpeg escribe a `.tmp` y renombra, así nunca servimos un segmento
//!   a medio escribir.
//! - Si ffmpeg va más de `MAX_AHEAD_S` por delante de lo que pidió el reproductor, el endpoint
//!   interno deja de darle datos (backpressure): no llena el disco con la película entera ni
//!   compite por ancho de banda más allá de lo útil.
//! - `?start=<s>` arranca un ffmpeg nuevo desde ese segundo (saltos en modo transcode). Cada
//!   arranque tiene su "generación" en el nombre de los segmentos, para que pedidos viejos no
//!   reciban segmentos de otra posición.
//! - Sin pedidos por 60 s, o con `DELETE`, se mata ffmpeg y se borra su carpeta.

use crate::engine::Engine;
use crate::probe::{self, Probe};
use crate::session::PlaySession;
use crate::stream::{json_error, Role};
use crate::{ClientCaps, HwAccel, PlayMode};
use axum::body::Body;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncBufReadExt;

pub const SEGMENT_S: f64 = 4.0;
/// Cuánto puede adelantarse ffmpeg a lo que pidió el reproductor.
pub const MAX_AHEAD_S: f64 = 240.0;
const IDLE_KILL: Duration = Duration::from_secs(60);
const PLAYLIST_WAIT: Duration = Duration::from_secs(120);
const SEGMENT_WAIT: Duration = Duration::from_secs(90);

static GEN: AtomicU64 = AtomicU64::new(1);

pub struct Job {
    pub gen: u64,
    start: f64,
    audio: Option<usize>,
    attempt: u32,
    dir: PathBuf,
    child: Mutex<Option<tokio::process::Child>>,
    last_touch: Mutex<Instant>,
    /// Último segmento pedido por el reproductor (-1: ninguno).
    last_req: AtomicI64,
    produced: AtomicU64,
    exited: AtomicBool,
    /// ffmpeg terminó bien (playlist completa con ENDLIST): se reutiliza tal cual.
    finished_ok: AtomicBool,
    stderr: Mutex<VecDeque<String>>,
}

pub type HlsJob = Arc<Job>;

impl Job {
    fn touch(&self) {
        *self.last_touch.lock().unwrap() = Instant::now();
    }

    fn ahead_s(&self) -> f64 {
        let produced = self.produced.load(Ordering::Relaxed) as i64;
        let next = self.last_req.load(Ordering::Relaxed) + 1;
        (produced - next).max(0) as f64 * SEGMENT_S
    }

    fn kill(&self) {
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.start_kill();
        }
    }

    fn error_text(&self) -> String {
        let e = self.stderr.lock().unwrap();
        let tail: Vec<&str> = e.iter().map(|s| s.as_str()).collect();
        if tail.is_empty() {
            "ffmpeg exited".into()
        } else {
            format!("ffmpeg: {}", tail.join(" | "))
        }
    }
}

/// Lo que va a hacer ffmpeg (separado de los argumentos para poder testearlo).
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub copy_video: bool,
    pub video_index: Option<usize>,
    pub audio_index: Option<usize>,
    pub copy_audio: bool,
    pub audio_channels: u32,
    pub downscale: bool,
    pub tonemap: bool,
    /// Curva de entrada para el tonemap (si el archivo no la declara, se asume PQ).
    pub transfer: String,
    pub ten_bit: bool,
    pub hevc_tag: bool,
    /// Encoder: NVENC o libx264.
    pub hw: HwAccel,
    /// Decodificar con NVDEC (`-hwaccel cuda`).
    pub hw_decode: bool,
}

/// Si ffmpeg muere antes de producir nada, se reintenta con un plan más conservador: un remux que
/// falla pasa a transcode; un transcode, sin decodificar en GPU, después con libx264, y por
/// último sin tonemap (por si a ese ffmpeg le falta zimg). Mejor colores lavados que no ver nada.
pub fn degrade(pl: &Plan, attempt: u32) -> Option<Plan> {
    if pl.copy_video && attempt > 0 {
        let mut t = pl.clone();
        t.copy_video = false;
        t.hevc_tag = false;
        t.hw_decode = t.hw == HwAccel::Nvenc;
        return degrade(&t, attempt - 1);
    }
    let mut p = pl.clone();
    match attempt {
        0 => {}
        1 if p.hw_decode => p.hw_decode = false,
        1 | 2 if p.hw == HwAccel::Nvenc => {
            p.hw_decode = false;
            p.hw = HwAccel::None;
        }
        1..=3 if p.tonemap => {
            p.hw_decode = false;
            p.hw = HwAccel::None;
            p.tonemap = false;
        }
        _ => return None,
    }
    if attempt > 0 && p == *pl {
        return None;
    }
    Some(p)
}

pub fn plan(p: &Probe, caps: &ClientCaps, mode: PlayMode, audio_req: Option<usize>, hw: HwAccel) -> Plan {
    let v = p.video.as_ref();
    let copy_video = mode != PlayMode::Transcode && v.is_some_and(|v| probe::video_ok(v, caps));
    let audio = audio_req.and_then(|i| p.audio.iter().find(|a| a.track.index == i)).or(p.default_audio());
    let copy_audio = audio.is_some_and(|a| matches!(a.track.codec.as_str(), "aac" | "ac3" | "eac3") && probe::audio_ok(&a.track.codec, caps));
    Plan {
        copy_video,
        video_index: v.map(|v| v.index),
        audio_index: audio.map(|a| a.track.index),
        copy_audio,
        audio_channels: if audio.is_some_and(|a| a.channels > 2) { 6 } else { 2 },
        // Estos dos solo se aplican si se transcodifica (también si el remux falla y se cae a
        // transcode).
        downscale: v.is_some_and(|v| v.width.unwrap_or(0) > 1920 || v.height.unwrap_or(0) > 1080),
        tonemap: v.is_some_and(|v| v.hdr),
        transfer: v
            .and_then(|v| v.transfer.clone())
            .filter(|t| t == "smpte2084" || t == "arib-std-b67")
            .unwrap_or_else(|| "smpte2084".into()),
        ten_bit: v.and_then(|v| v.pix_fmt.as_deref()).is_some_and(|f| f.contains("10") || f.contains("12")),
        hevc_tag: copy_video && v.is_some_and(|v| v.codec == "hevc"),
        hw,
        hw_decode: !copy_video && hw == HwAccel::Nvenc,
    }
}

/// Las rutas de salida son relativas: ffmpeg corre con la carpeta del trabajo como directorio
/// actual (con rutas absolutas de Windows, `\`, ffmpeg calcula mal dónde va el init de fMP4).
pub fn args(pl: &Plan, input: &str, start: f64, gen: u64) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-nostdin", "-loglevel", "error", "-nostats"].iter().map(|s| s.to_string()).collect();
    let mut push = |xs: &[&str]| a.extend(xs.iter().map(|s| s.to_string()));
    if !pl.copy_video && pl.hw_decode {
        // Decodificar en la GPU (vuelve a RAM sola si hay filtros de CPU; si NVDEC no conoce el
        // códec, ffmpeg cae a software).
        push(&["-hwaccel", "cuda"]);
    }
    push(&["-probesize", "10000000", "-analyzeduration", "5000000", "-fflags", "+genpts"]);
    if start > 0.0 {
        push(&["-ss", &format!("{start:.3}")]);
    }
    push(&["-i", input]);
    match pl.video_index {
        Some(i) => push(&["-map", &format!("0:{i}")]),
        None => push(&["-map", "0:v:0?"]),
    }
    if let Some(i) = pl.audio_index {
        push(&["-map", &format!("0:{i}")]);
    }
    push(&["-sn", "-dn", "-map_metadata", "-1", "-map_chapters", "-1"]);
    if pl.copy_video {
        push(&["-c:v", "copy"]);
        if pl.hevc_tag {
            push(&["-tag:v", "hvc1"]);
        }
    } else {
        let mut vf: Vec<String> = Vec::new();
        if pl.downscale {
            vf.push("scale=w='min(1920,iw)':h=-2".into());
        }
        if pl.tonemap {
            // `setparams` primero: muchos archivos HDR no declaran primarios/curva y sin eso
            // zscale falla con "no path between colorspaces".
            vf.push(format!(
                "setparams=color_primaries=bt2020:color_trc={}:colorspace=bt2020nc,zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv",
                pl.transfer
            ));
        }
        vf.push("format=yuv420p".into());
        push(&["-vf", &vf.join(",")]);
        match pl.hw {
            HwAccel::Nvenc => push(&[
                "-c:v", "h264_nvenc", "-preset", "p3", "-tune", "ll", "-rc", "vbr", "-cq", "23", "-b:v", "8M", "-maxrate", "12M",
                "-bufsize", "16M", "-profile:v", "high", "-forced-idr", "1",
            ]),
            HwAccel::None => push(&["-c:v", "libx264", "-preset", "veryfast", "-crf", "22", "-maxrate", "10M", "-bufsize", "16M", "-profile:v", "high"]),
        }
        push(&["-force_key_frames", "expr:gte(t,n_forced*4)"]);
    }
    if pl.audio_index.is_some() {
        if pl.copy_audio {
            push(&["-c:a", "copy"]);
        } else if pl.audio_channels == 6 {
            push(&["-c:a", "aac", "-ac", "6", "-b:a", "384k"]);
        } else {
            push(&["-c:a", "aac", "-ac", "2", "-b:a", "192k"]);
        }
    }
    let seg = format!("g{gen}_%05d.m4s");
    push(&[
        "-max_muxing_queue_size", "4096",
        "-f", "hls",
        "-hls_time", "4",
        "-hls_playlist_type", "event",
        "-hls_list_size", "0",
        "-hls_segment_type", "fmp4",
        "-hls_fmp4_init_filename", &format!("g{gen}_init.mp4"),
        "-hls_flags", "temp_file+independent_segments",
        "-start_number", "0",
        "-hls_segment_filename", &seg,
        "index.m3u8",
    ]);
    a
}

/// Agrega `?t=<token>` a cada URI de la playlist (hls.js no hereda el query de la playlist).
pub fn rewrite_playlist(text: &str, token: Option<&str>) -> String {
    let Some(t) = token.filter(|t| !t.is_empty()) else { return text.to_string() };
    let q = format!("?t={}", crate::util::pct_encode(t));
    let mut out = String::with_capacity(text.len() + 64);
    for line in text.lines() {
        if line.starts_with("#EXT-X-MAP:") {
            if let Some((a, b)) = line.split_once("URI=\"") {
                if let Some((uri, rest)) = b.split_once('"') {
                    out.push_str(&format!("{a}URI=\"{uri}{q}\"{rest}\n"));
                    continue;
                }
            }
            out.push_str(line);
        } else if !line.is_empty() && !line.starts_with('#') {
            out.push_str(line);
            out.push_str(&q);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// Generación y número de un segmento: `g12_00034.m4s` → (12, Some(34)); `g12_init.mp4` → (12, None).
pub fn parse_segment(name: &str) -> Option<(u64, Option<i64>)> {
    let rest = name.strip_prefix('g')?;
    let (gen, tail) = rest.split_once('_')?;
    let gen = gen.parse().ok()?;
    if tail == "init.mp4" {
        return Some((gen, None));
    }
    let n = tail.strip_suffix(".m4s")?.parse().ok()?;
    Some((gen, Some(n)))
}

async fn start_job(e: &Engine, s: &Arc<PlaySession>, start: f64, audio: Option<usize>, attempt: u32) -> anyhow::Result<HlsJob> {
    let gen = GEN.fetch_add(1, Ordering::Relaxed);
    let dir = s.tmp_dir.join(format!("g{gen}"));
    tokio::fs::create_dir_all(&dir).await?;
    let (p, caps, mode) = {
        let p = s.probe.lock().unwrap().clone().unwrap_or_else(|| probe::guess(&s.file_name));
        let caps = s.caps.lock().unwrap().clone();
        let mode = s.info.lock().unwrap().as_ref().map(|i| i.mode).unwrap_or(PlayMode::Transcode);
        (p, caps, mode)
    };
    let pl = degrade(&plan(&p, &caps, mode, audio, e.cfg.hwaccel), attempt).ok_or_else(|| anyhow::anyhow!("no more fallbacks"))?;
    let input = e.internal_url(&s.id, Role::Hls);
    let argv = args(&pl, &input, start, gen);
    tracing::info!(session = %s.id, gen, start, ?pl, "starting ffmpeg (hls)");
    let mut child = tokio::process::Command::new(&e.cfg.ffmpeg)
        .args(&argv)
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stderr = child.stderr.take();
    let job = Arc::new(Job {
        gen,
        start,
        audio,
        attempt,
        dir: dir.clone(),
        child: Mutex::new(Some(child)),
        last_touch: Mutex::new(Instant::now()),
        last_req: AtomicI64::new(-1),
        produced: AtomicU64::new(0),
        exited: AtomicBool::new(false),
        finished_ok: AtomicBool::new(false),
        stderr: Mutex::new(VecDeque::new()),
    });
    if let Some(err) = stderr {
        let j = job.clone();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(err).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let mut q = j.stderr.lock().unwrap();
                if q.len() >= 12 {
                    q.pop_front();
                }
                q.push_back(l);
            }
        });
    }
    // Monitor: cuenta segmentos producidos, publica el colchón y detecta la salida de ffmpeg.
    let j = job.clone();
    let sess = s.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        loop {
            tick.tick().await;
            if let Ok(t) = tokio::fs::read_to_string(j.dir.join("index.m3u8")).await {
                j.produced.store(t.matches("#EXTINF").count() as u64, Ordering::Relaxed);
            }
            let done = {
                let mut c = j.child.lock().unwrap();
                match c.as_mut().map(|ch| ch.try_wait()) {
                    Some(Ok(None)) => false,
                    Some(Ok(Some(st))) => {
                        j.finished_ok.store(st.success(), Ordering::Relaxed);
                        true
                    }
                    _ => true,
                }
            };
            let current = sess.hls.lock().await.as_ref().is_some_and(|x| x.gen == j.gen);
            if current {
                *sess.hls_ahead_s.lock().unwrap() = Some(j.ahead_s());
            }
            if done {
                j.exited.store(true, Ordering::Relaxed);
                break;
            }
        }
    });
    Ok(job)
}

/// El endpoint interno llama esto antes de darle más datos a ffmpeg (backpressure).
pub fn too_far_ahead(s: &PlaySession) -> bool {
    s.hls_ahead_s.lock().unwrap().is_some_and(|a| a > MAX_AHEAD_S)
}

pub async fn stop(s: &PlaySession) {
    if let Some(j) = s.hls.lock().await.take() {
        j.kill();
        let _ = tokio::fs::remove_dir_all(&j.dir).await;
    }
    *s.hls_ahead_s.lock().unwrap() = None;
}

pub async fn reap_idle(s: &PlaySession) {
    let idle = s.hls.lock().await.as_ref().is_some_and(|j| j.last_touch.lock().unwrap().elapsed() > IDLE_KILL);
    if idle {
        tracing::info!(session = %s.id, "hls idle: stopping ffmpeg");
        stop(s).await;
    }
}

fn no_cache(mut r: Response, ct: &'static str) -> Response {
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r
}

pub async fn index(e: &Engine, s: Arc<PlaySession>, q: &HashMap<String, String>) -> Response {
    let start = q.get("start").and_then(|x| x.parse::<f64>().ok()).filter(|x| x.is_finite() && *x >= 0.0).unwrap_or(0.0);
    let audio = q.get("audio").and_then(|x| x.parse::<usize>().ok());
    s.touch();
    let job = {
        let mut g = s.hls.lock().await;
        let reuse = g.as_ref().is_some_and(|j| {
            (j.start - start).abs() < 0.5 && j.audio == audio && (!j.exited.load(Ordering::Relaxed) || j.finished_ok.load(Ordering::Relaxed))
        });
        if !reuse {
            if let Some(old) = g.take() {
                old.kill();
                let d = old.dir.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    let _ = tokio::fs::remove_dir_all(d).await;
                });
            }
            match start_job(e, &s, start, audio, 0).await {
                Ok(j) => *g = Some(j),
                Err(err) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("could not start ffmpeg: {err:#}")),
            }
        }
        g.as_ref().cloned()
    };
    let Some(mut job) = job else { return json_error(StatusCode::INTERNAL_SERVER_ERROR, "hls job missing") };
    job.touch();
    let path = job.dir.join("index.m3u8");
    let t0 = Instant::now();
    loop {
        if let Ok(t) = tokio::fs::read_to_string(&path).await {
            if t.contains("#EXTINF") {
                let body = rewrite_playlist(&t, q.get("t").map(|s| s.as_str()));
                return no_cache(body.into_response(), "application/vnd.apple.mpegurl");
            }
        }
        if job.exited.load(Ordering::Relaxed) && !job.finished_ok.load(Ordering::Relaxed) {
            // Murió sin producir nada: reintentar con un plan más conservador.
            let mut g = s.hls.lock().await;
            let same = g.as_ref().is_some_and(|j| j.gen == job.gen);
            if same && job.produced.load(Ordering::Relaxed) == 0 {
                tracing::warn!(session = %s.id, attempt = job.attempt, "ffmpeg failed early: {}", job.error_text());
                match start_job(e, &s, start, audio, job.attempt + 1).await {
                    Ok(j) => {
                        *g = Some(j.clone());
                        job = j;
                        continue;
                    }
                    Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, job.error_text()),
                }
            }
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, job.error_text());
        }
        if t0.elapsed() > PLAYLIST_WAIT {
            return json_error(StatusCode::GATEWAY_TIMEOUT, "stream is still buffering, try again");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub async fn segment(_e: &Engine, s: Arc<PlaySession>, seg: &str, _q: &HashMap<String, String>) -> Response {
    let Some((gen, n)) = parse_segment(seg) else { return json_error(StatusCode::NOT_FOUND, "bad segment name") };
    let job = s.hls.lock().await.as_ref().cloned();
    let Some(job) = job.filter(|j| j.gen == gen) else {
        return json_error(StatusCode::NOT_FOUND, "segment from an old playlist");
    };
    s.touch();
    job.touch();
    if let Some(n) = n {
        job.last_req.store(n, Ordering::Relaxed);
        *s.hls_ahead_s.lock().unwrap() = Some(job.ahead_s());
    }
    let path = job.dir.join(seg);
    let t0 = Instant::now();
    loop {
        // El init se escribe sin temp_file: lo damos por completo cuando ya hay un segmento.
        let ready = match n {
            Some(_) => tokio::fs::try_exists(&path).await.unwrap_or(false),
            None => job.produced.load(Ordering::Relaxed) > 0 && tokio::fs::try_exists(&path).await.unwrap_or(false),
        };
        if ready {
            if let Ok(f) = tokio::fs::File::open(&path).await {
                let len = f.metadata().await.map(|m| m.len()).unwrap_or(0);
                let body = Body::from_stream(tokio_util::io::ReaderStream::with_capacity(f, crate::stream::CHUNK));
                let mut r = no_cache(body.into_response(), "video/mp4");
                r.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(len));
                return r;
            }
        }
        if job.exited.load(Ordering::Relaxed) && !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return json_error(StatusCode::NOT_FOUND, "segment not available");
        }
        if t0.elapsed() > SEGMENT_WAIT {
            return json_error(StatusCode::GATEWAY_TIMEOUT, "segment not ready yet");
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::{AudioInfo, VideoInfo};
    use crate::Track;

    fn probe_of(codec: &str, w: u32, h: u32, hdr: bool, audio: &str, ch: u32) -> Probe {
        Probe {
            container: Some("mkv".into()),
            video: Some(VideoInfo { index: 0, codec: codec.into(), width: Some(w), height: Some(h), hdr, pix_fmt: Some(if hdr { "yuv420p10le" } else { "yuv420p" }.into()), ..Default::default() }),
            audio: vec![
                AudioInfo { track: Track { index: 1, codec: audio.into(), default: Some(true), ..Default::default() }, channels: ch },
                AudioInfo { track: Track { index: 2, codec: "aac".into(), ..Default::default() }, channels: 2 },
            ],
            real: true,
            ..Default::default()
        }
    }

    fn chrome() -> ClientCaps {
        ClientCaps { h264: true, vp9: true, av1: true, aac: true, opus: true, ..Default::default() }
    }

    #[test]
    fn remux_copies_video_and_converts_audio() {
        let p = probe_of("h264", 1920, 1080, false, "eac3", 6);
        let pl = plan(&p, &chrome(), PlayMode::Remux, None, HwAccel::Nvenc);
        assert!(pl.copy_video && !pl.copy_audio && !pl.downscale && !pl.tonemap && !pl.hw_decode);
        assert_eq!(pl.audio_index, Some(1));
        assert_eq!(pl.audio_channels, 6);
        let a = args(&pl, "http://x", 0.0, 3).join(" ");
        assert!(a.contains("-c:v copy"));
        assert!(a.contains("-c:a aac -ac 6"));
        assert!(!a.contains("-hwaccel"));
        assert!(!a.contains("-ss"));
        assert!(a.contains("g3_init.mp4"));
        // Pista elegida a mano.
        let pl2 = plan(&p, &chrome(), PlayMode::Remux, Some(2), HwAccel::Nvenc);
        assert_eq!(pl2.audio_index, Some(2));
        assert!(pl2.copy_audio);
    }

    #[test]
    fn transcode_4k_hdr_to_1080p_sdr() {
        let p = probe_of("hevc", 3840, 2160, true, "aac", 2);
        let pl = plan(&p, &chrome(), PlayMode::Transcode, None, HwAccel::Nvenc);
        assert!(!pl.copy_video && pl.downscale && pl.tonemap && pl.copy_audio);
        let a = args(&pl, "http://x", 125.5, 1).join(" ");
        assert!(a.contains("-hwaccel cuda"));
        assert!(a.contains("h264_nvenc"));
        assert!(a.contains("tonemap=tonemap=hable"));
        assert!(a.contains("-ss 125.500"));
        assert!(a.contains("expr:gte(t,n_forced*4)"));
        let sw = plan(&p, &chrome(), PlayMode::Transcode, None, HwAccel::None);
        assert!(args(&sw, "http://x", 0.0, 1).join(" ").contains("libx264 -preset veryfast"));
    }

    #[test]
    fn fallbacks() {
        let p = probe_of("hevc", 3840, 2160, true, "aac", 2);
        let pl = plan(&p, &chrome(), PlayMode::Transcode, None, HwAccel::Nvenc);
        assert!(pl.hw_decode);
        let a1 = degrade(&pl, 1).unwrap();
        assert!(!a1.hw_decode && a1.hw == HwAccel::Nvenc);
        let a2 = degrade(&pl, 2).unwrap();
        assert_eq!(a2.hw, HwAccel::None);
        let a3 = degrade(&pl, 3).unwrap();
        assert!(!a3.tonemap);
        assert!(degrade(&pl, 4).is_none());
        // Un remux que falla pasa a transcode, y después se degrada igual.
        let r = plan(&probe_of("h264", 1920, 1080, false, "aac", 2), &chrome(), PlayMode::Remux, None, HwAccel::Nvenc);
        assert!(degrade(&r, 0).unwrap().copy_video);
        let r1 = degrade(&r, 1).unwrap();
        assert!(!r1.copy_video && r1.hw_decode);
        assert_eq!(degrade(&r, 3).unwrap().hw, HwAccel::None);
        assert!(degrade(&r, 4).is_none());
    }

    #[test]
    fn hevc_copy_gets_hvc1_tag() {
        let mut caps = chrome();
        caps.hevc = true;
        let pl = plan(&probe_of("hevc", 3840, 2160, true, "aac", 2), &caps, PlayMode::Remux, None, HwAccel::Nvenc);
        assert!(pl.copy_video && pl.hevc_tag);
        let a = args(&pl, "http://x", 0.0, 1).join(" ");
        assert!(a.contains("-tag:v hvc1") && !a.contains("tonemap") && !a.contains("scale="));
    }

    #[test]
    fn playlist_rewrite_and_names() {
        let src = "#EXTM3U\n#EXT-X-MAP:URI=\"g1_init.mp4\"\n#EXTINF:4.0,\ng1_00000.m4s\n";
        let out = rewrite_playlist(src, Some("ab c"));
        assert!(out.contains("URI=\"g1_init.mp4?t=ab%20c\""));
        assert!(out.contains("g1_00000.m4s?t=ab%20c\n"));
        assert_eq!(rewrite_playlist(src, None), src);
        assert_eq!(parse_segment("g12_00034.m4s"), Some((12, Some(34))));
        assert_eq!(parse_segment("g12_init.mp4"), Some((12, None)));
        assert_eq!(parse_segment("index.m3u8"), None);
        assert_eq!(parse_segment("g1_x.m4s"), None);
    }
}
