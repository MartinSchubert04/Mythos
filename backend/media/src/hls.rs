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
//! - `?start=<s>` arranca un trabajo nuevo desde ese segundo (saltos en modo transcode). Cada
//!   (start, audio) tiene su "clave" en el nombre de los segmentos (`s125000ad_00012.m4s`), así un
//!   pedido viejo de otra posición da 404 en vez de recibir un segmento equivocado.
//! - **Reanudar**: sin pedidos por 60 s se mata ffmpeg pero se guardan los segmentos. El próximo
//!   pedido de playlist (o de un segmento que todavía no existe) relanza ffmpeg solo, desde el
//!   final de lo ya generado (`-ss` + `-output_ts_offset`, numeración continua): la playlist sigue
//!   igual para el cliente, que no tiene que adivinar nada. Lo mismo si ffmpeg se cae a mitad de
//!   camino. Si se cae sin producir nada, se reintenta con un plan más conservador (`degrade`).
//! - La playlist la armamos nosotros juntando las de cada corrida de ffmpeg (un `EXT-X-MAP` por
//!   corrida, porque cada una escribe su propio init).
//! - `DELETE` (o cerrar la sesión) mata ffmpeg y borra la carpeta.

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
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncBufReadExt;

pub const SEGMENT_S: f64 = 4.0;
/// Cuánto puede adelantarse ffmpeg a lo que pidió el reproductor.
pub const MAX_AHEAD_S: f64 = 240.0;
const IDLE_KILL: Duration = Duration::from_secs(60);
const PLAYLIST_WAIT: Duration = Duration::from_secs(120);
const SEGMENT_WAIT: Duration = Duration::from_secs(90);
/// Margen al reanudar un remux (más que un frame, menos que un GOP).
const RESUME_EPS_S: f64 = 0.2;

/// Un segmento ya generado.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub dur: f64,
    pub uri: String,
    /// Init (`EXT-X-MAP`) que le corresponde: cada corrida de ffmpeg escribe el suyo.
    pub map: String,
}

/// Una corrida de ffmpeg dentro de un trabajo.
struct Run {
    /// `None` si lo matamos nosotros por inactividad (no cuenta como falla).
    child: Option<tokio::process::Child>,
    /// Número del primer segmento de esta corrida.
    base: u64,
}

/// Un trabajo HLS: una posición de inicio + pista de audio. Puede tener varias corridas de ffmpeg
/// (si se mató por inactividad o se cayó) que comparten carpeta y numeración.
pub struct Job {
    key: String,
    start: f64,
    audio: Option<usize>,
    dir: PathBuf,
    /// Nivel de degradación (ver `degrade`); sube si una corrida muere sin producir nada.
    attempt: AtomicU32,
    run: Mutex<Option<Run>>,
    /// Segmentos de corridas terminadas, y los de la corrida actual.
    done: Mutex<Vec<Entry>>,
    current: Mutex<Vec<Entry>>,
    last_touch: Mutex<Instant>,
    /// Último segmento pedido por el reproductor (-1: ninguno).
    last_req: AtomicI64,
    /// No hay ffmpeg corriendo (terminó, se cayó o se mató por inactividad).
    exited: AtomicBool,
    /// ffmpeg llegó al final del archivo: playlist completa, no hay nada que reanudar.
    finished_ok: AtomicBool,
    stderr: Mutex<VecDeque<String>>,
}

pub type HlsJob = Arc<Job>;

/// Clave de un trabajo: `s<ms de inicio>a<pista|d>`.
pub fn job_key(start: f64, audio: Option<usize>) -> String {
    format!("s{}a{}", (start * 1000.0).round() as u64, audio.map(|a| a.to_string()).unwrap_or_else(|| "d".into()))
}

impl Job {
    fn new(s: &PlaySession, start: f64, audio: Option<usize>) -> HlsJob {
        let key = job_key(start, audio);
        Arc::new(Job {
            dir: s.tmp_dir.join(&key),
            key,
            start,
            audio,
            attempt: AtomicU32::new(0),
            run: Mutex::new(None),
            done: Mutex::new(Vec::new()),
            current: Mutex::new(Vec::new()),
            last_touch: Mutex::new(Instant::now()),
            last_req: AtomicI64::new(-1),
            exited: AtomicBool::new(true),
            finished_ok: AtomicBool::new(false),
            stderr: Mutex::new(VecDeque::new()),
        })
    }

    fn touch(&self) {
        *self.last_touch.lock().unwrap() = Instant::now();
    }

    fn entries(&self) -> Vec<Entry> {
        let mut v = self.done.lock().unwrap().clone();
        v.extend(self.current.lock().unwrap().iter().cloned());
        v
    }

    fn ahead_s(&self) -> f64 {
        let next = (self.last_req.load(Ordering::Relaxed) + 1).max(0) as usize;
        self.entries().iter().skip(next).map(|e| e.dur).sum()
    }

    /// Hay que relanzar ffmpeg (no corre y no terminó el archivo).
    fn needs_run(&self) -> bool {
        self.exited.load(Ordering::Relaxed) && !self.finished_ok.load(Ordering::Relaxed)
    }

    fn kill(&self) {
        if let Some(c) = self.run.lock().unwrap().as_mut().and_then(|r| r.child.as_mut()) {
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
        t.copy_audio = false;
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
    // En transcode el audio se recodifica siempre (AAC es barato): con `-ss` el audio copiado no
    // se recorta y arrancaba hasta un GOP antes que el video (medido: 10 s de solapamiento).
    let copy_audio = copy_video && audio.is_some_and(|a| matches!(a.track.codec.as_str(), "aac" | "ac3" | "eac3") && probe::audio_ok(&a.track.codec, caps));
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

/// Argumentos de ffmpeg. Las rutas de salida son relativas: ffmpeg corre con la carpeta del
/// trabajo como directorio actual (con rutas absolutas de Windows, `\`, ffmpeg calcula mal dónde
/// va el init de fMP4). `offset` y `base`: al reanudar, ffmpeg arranca `offset` segundos más
/// adelante, corre los timestamps esa cantidad y numera los segmentos desde `base`.
pub fn args(pl: &Plan, input: &str, start: f64, offset: f64, base: u64, key: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-nostdin", "-loglevel", "error", "-nostats"].iter().map(|s| s.to_string()).collect();
    let mut push = |xs: &[&str]| a.extend(xs.iter().map(|s| s.to_string()));
    if !pl.copy_video && pl.hw_decode {
        // Decodificar en la GPU (vuelve a RAM sola si hay filtros de CPU; si NVDEC no conoce el
        // códec, ffmpeg cae a software).
        push(&["-hwaccel", "cuda"]);
    }
    push(&["-probesize", "5000000", "-analyzeduration", "3000000", "-fflags", "+genpts"]);
    let from = start + offset;
    if from > 0.0 {
        push(&["-ss", &format!("{from:.3}")]);
    }
    // (No `-copyts`: con timestamps absolutos el muxer HLS corta mal el primer segmento —medido:
    // un primer segmento de 100 s con `start=100`—. Para reanudar se usa `-output_ts_offset`.)
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
    if offset > 0.0 {
        push(&["-output_ts_offset", &format!("{offset:.3}")]);
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
    let seg = format!("{key}_%05d.m4s");
    let init = init_name(key, base);
    let playlist = run_playlist(base);
    let base = base.to_string();
    push(&[
        "-max_muxing_queue_size", "4096",
        "-f", "hls",
        "-hls_time", "4",
        "-hls_playlist_type", "event",
        "-hls_list_size", "0",
        "-hls_segment_type", "fmp4",
        "-hls_fmp4_init_filename", &init,
        "-hls_flags", "temp_file+independent_segments",
        "-start_number", &base,
        "-hls_segment_filename", &seg,
        &playlist,
    ]);
    a
}

fn init_name(key: &str, base: u64) -> String {
    format!("{key}_r{base}_init.mp4")
}

fn run_playlist(base: u64) -> String {
    format!("run{base}.m3u8")
}

/// Lee los segmentos de la playlist que escribe una corrida de ffmpeg.
pub fn parse_run_playlist(text: &str, default_map: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut map = default_map.to_string();
    let mut dur = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            if let Some((_, b)) = rest.split_once("URI=\"") {
                if let Some((uri, _)) = b.split_once('"') {
                    map = uri.to_string();
                }
            }
        } else if let Some(rest) = line.strip_prefix("#EXTINF:") {
            dur = rest.split(',').next().and_then(|d| d.trim().parse::<f64>().ok());
        } else if !line.is_empty() && !line.starts_with('#') {
            out.push(Entry { dur: dur.take().unwrap_or(SEGMENT_S), uri: line.to_string(), map: map.clone() });
        }
    }
    out
}

/// Playlist EVENT con todos los segmentos de todas las corridas (un `EXT-X-MAP` cada vez que
/// cambia el init) y `ENDLIST` si ffmpeg llegó al final.
pub fn build_playlist(entries: &[Entry], ended: bool) -> String {
    let target = entries.iter().map(|e| e.dur.ceil() as u64).max().unwrap_or(4).max(4);
    let mut s = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:{target}\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:EVENT\n#EXT-X-INDEPENDENT-SEGMENTS\n"
    );
    let mut map = "";
    for e in entries {
        if e.map != map {
            map = &e.map;
            s.push_str(&format!("#EXT-X-MAP:URI=\"{map}\"\n"));
        }
        s.push_str(&format!("#EXTINF:{:.6},\n{}\n", e.dur, e.uri));
    }
    if ended {
        s.push_str("#EXT-X-ENDLIST\n");
    }
    s
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

/// Clave y número de un segmento: `s0ad_00034.m4s` → ("s0ad", Some(34));
/// `s0ad_r12_init.mp4` → ("s0ad", None).
pub fn parse_segment(name: &str) -> Option<(String, Option<u64>)> {
    if let Some(rest) = name.strip_suffix("_init.mp4") {
        let (key, run) = rest.rsplit_once('_')?;
        run.strip_prefix('r')?.parse::<u64>().ok()?;
        return Some((key.to_string(), None));
    }
    let (key, tail) = name.rsplit_once('_')?;
    let n = tail.strip_suffix(".m4s")?.parse().ok()?;
    (!key.is_empty()).then(|| (key.to_string(), Some(n)))
}

/// Lanza (o relanza) ffmpeg para el trabajo, desde el final de lo ya generado. Se llama con el
/// lock `s.hls` tomado, así no hay dos lanzamientos a la vez.
async fn launch(e: &Engine, s: &Arc<PlaySession>, job: &HlsJob) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&job.dir).await?;
    let (p, caps, mode) = {
        let p = s.probe.lock().unwrap().clone().unwrap_or_else(|| probe::guess(&s.file_name));
        let caps = s.caps.lock().unwrap().clone();
        let mode = s.info.lock().unwrap().as_ref().map(|i| i.mode).unwrap_or(PlayMode::Transcode);
        (p, caps, mode)
    };
    let attempt = job.attempt.load(Ordering::Relaxed);
    let Some(pl) = degrade(&plan(&p, &caps, mode, job.audio, e.cfg.hwaccel), attempt) else {
        anyhow::bail!("{}", job.error_text());
    };
    // Lo de la corrida anterior pasa a "hecho"; la nueva sigue la numeración y el tiempo.
    {
        let mut cur = job.current.lock().unwrap();
        job.done.lock().unwrap().append(&mut cur);
    }
    let (base, first, last, sum) = {
        let d = job.done.lock().unwrap();
        (d.len() as u64, d.first().cloned(), d.last().cloned(), d.iter().map(|e| e.dur).sum::<f64>())
    };
    let mut offset = 0.0;
    if let (Some(first), Some(last)) = (first, last) {
        // Cuánto se generó de verdad: fin del último segmento menos inicio del primero, medidos
        // con ffprobe (ffmpeg no arranca exactamente en 0: medido ~1 s de corrimiento en
        // transcode, y en remux el primer segmento arranca en un keyframe). Si falla, la suma de
        // las duraciones de la playlist.
        let span = match (segment_bounds(&e.cfg.ffprobe, &job.dir, &first).await, segment_bounds(&e.cfg.ffprobe, &job.dir, &last).await) {
            (Some((f, _)), Some((_, l))) if l > f => l - f,
            _ => sum,
        };
        offset = span.max(0.0);
        if pl.copy_video {
            // En remux `-ss` (antes de `-i`) va al keyframe anterior: sin este margen, por
            // redondeo cae en el keyframe del último segmento y lo repite (medido: duplicaba 8 s).
            offset += RESUME_EPS_S;
        }
    }
    let input = e.internal_url(&s.id, Role::Hls);
    let argv = args(&pl, &input, job.start, offset, base, &job.key);
    tracing::info!(session = %s.id, key = %job.key, base, offset, attempt, ?pl, "starting ffmpeg (hls)");
    let mut child = tokio::process::Command::new(&e.cfg.ffmpeg)
        .args(&argv)
        .current_dir(&job.dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stderr = child.stderr.take();
    job.stderr.lock().unwrap().clear();
    *job.run.lock().unwrap() = Some(Run { child: Some(child), base });
    job.exited.store(false, Ordering::Relaxed);
    job.finished_ok.store(false, Ordering::Relaxed);
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
    tokio::spawn(monitor(job.clone(), s.clone(), base));
    Ok(())
}

/// Inicio y fin (timestamps de salida, en s) de un segmento ya escrito: ffprobe sobre init + segmento.
async fn segment_bounds(ffprobe: &std::path::Path, dir: &std::path::Path, e: &Entry) -> Option<(f64, f64)> {
    let input = format!("concat:{}|{}", e.map, e.uri);
    let out = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(ffprobe)
            .args(["-v", "error", "-show_entries", "format=start_time,duration", "-of", "csv=p=0", &input])
            .current_dir(dir)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut it = text.trim().split(',').map(|x| x.trim().parse::<f64>().ok());
    let (start, dur) = (it.next()??, it.next()??);
    Some((start, fragment_end(start, dur)))
}

/// En un fragmento fMP4 suelto ffprobe informa como `duration` el timestamp final (cuenta desde
/// 0), no la duración del fragmento (medido). Si fuera una duración real (< start), se suma.
pub fn fragment_end(start: f64, dur: f64) -> f64 {
    if dur >= start {
        dur
    } else {
        start + dur
    }
}

/// Vigila una corrida: lee su playlist, publica el colchón y detecta cuándo termina.
async fn monitor(j: HlsJob, sess: Arc<PlaySession>, base: u64) {
    let playlist = j.dir.join(run_playlist(base));
    let init = init_name(&j.key, base);
    let read = || async {
        if let Ok(t) = tokio::fs::read_to_string(&playlist).await {
            *j.current.lock().unwrap() = parse_run_playlist(&t, &init);
        }
    };
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    loop {
        tick.tick().await;
        read().await;
        // `Some(ok)` cuando la corrida terminó; `killed` si la matamos por inactividad.
        let (ended, killed) = {
            let mut r = j.run.lock().unwrap();
            match r.as_mut().filter(|r| r.base == base) {
                None => (Some(false), true),
                Some(run) => match run.child.as_mut().map(|c| c.try_wait()) {
                    Some(Ok(None)) => (None, false),
                    Some(Ok(Some(st))) => (Some(st.success()), false),
                    Some(Err(_)) => (Some(false), false),
                    None => (Some(false), true),
                },
            }
        };
        if sess.hls.lock().await.as_ref().is_some_and(|x| Arc::ptr_eq(x, &j)) {
            *sess.hls_ahead_s.lock().unwrap() = Some(j.ahead_s());
        }
        if let Some(ok) = ended {
            read().await;
            if !ok && !killed && j.current.lock().unwrap().is_empty() {
                // Murió sin producir nada: la próxima corrida, con un plan más conservador.
                j.attempt.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(session = %sess.id, key = %j.key, "ffmpeg failed without output: {}", j.error_text());
            }
            j.finished_ok.store(ok, Ordering::Relaxed);
            j.exited.store(true, Ordering::Relaxed);
            return;
        }
    }
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

/// Sin pedidos por un rato: se mata ffmpeg pero se conservan los segmentos para reanudar.
pub async fn reap_idle(s: &PlaySession) {
    let g = s.hls.lock().await;
    let Some(j) = g.as_ref() else { return };
    if !j.exited.load(Ordering::Relaxed) && j.last_touch.lock().unwrap().elapsed() > IDLE_KILL {
        tracing::info!(session = %s.id, key = %j.key, "hls idle: stopping ffmpeg (segments kept)");
        if let Some(mut c) = j.run.lock().unwrap().as_mut().and_then(|r| r.child.take()) {
            let _ = c.start_kill();
        }
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
    // Sin `?audio=` = la pista por defecto, con su índice explícito (así `?audio=1` y nada, si
    // la 1 es la de por defecto, son el mismo trabajo y no se transcodifica dos veces).
    let audio = q
        .get("audio")
        .and_then(|x| x.parse::<usize>().ok())
        .or_else(|| s.probe.lock().unwrap().as_ref().and_then(|p| p.default_audio().map(|a| a.track.index)));
    let key = job_key(start, audio);
    s.touch();
    let job = {
        let mut g = s.hls.lock().await;
        if let Some(old) = g.take_if(|j| j.key != key) {
            // Otra posición u otra pista: el trabajo viejo ya no sirve.
            old.kill();
            let d = old.dir.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let _ = tokio::fs::remove_dir_all(d).await;
            });
        }
        let job = g.get_or_insert_with(|| Job::new(&s, start, audio)).clone();
        if job.needs_run() {
            if let Err(err) = launch(e, &s, &job).await {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("could not start ffmpeg: {err:#}"));
            }
        }
        job
    };
    job.touch();
    let t0 = Instant::now();
    loop {
        let entries = job.entries();
        if !entries.is_empty() {
            let body = build_playlist(&entries, job.finished_ok.load(Ordering::Relaxed));
            let body = rewrite_playlist(&body, q.get("t").map(|s| s.as_str()));
            return no_cache(body.into_response(), "application/vnd.apple.mpegurl");
        }
        if job.needs_run() {
            // Murió sin producir nada: reintentar (el monitor ya subió el nivel de degradación).
            let _g = s.hls.lock().await;
            if job.needs_run() {
                if let Err(err) = launch(e, &s, &job).await {
                    return json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{err:#}"));
                }
            }
        }
        if t0.elapsed() > PLAYLIST_WAIT {
            return json_error(StatusCode::GATEWAY_TIMEOUT, "stream is still buffering, try again");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub async fn segment(e: &Engine, s: Arc<PlaySession>, seg: &str, _q: &HashMap<String, String>) -> Response {
    let Some((key, n)) = parse_segment(seg) else { return json_error(StatusCode::NOT_FOUND, "bad segment name") };
    let job = s.hls.lock().await.as_ref().cloned();
    let Some(job) = job.filter(|j| j.key == key) else {
        return json_error(StatusCode::NOT_FOUND, "segment from an old playlist");
    };
    s.touch();
    job.touch();
    if let Some(n) = n {
        job.last_req.store(n as i64, Ordering::Relaxed);
        *s.hls_ahead_s.lock().unwrap() = Some(job.ahead_s());
    }
    let path = job.dir.join(seg);
    let t0 = Instant::now();
    loop {
        // El init se escribe sin temp_file: lo damos por completo cuando su corrida ya tiene un
        // segmento.
        let ready = match n {
            Some(_) => tokio::fs::try_exists(&path).await.unwrap_or(false),
            None => job.entries().iter().any(|en| en.map == seg) && tokio::fs::try_exists(&path).await.unwrap_or(false),
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
        if job.finished_ok.load(Ordering::Relaxed) {
            return json_error(StatusCode::NOT_FOUND, "segment not available");
        }
        if job.needs_run() {
            // ffmpeg no está (inactividad o caída): relanzar desde donde quedó.
            let _g = s.hls.lock().await;
            if job.needs_run() {
                if let Err(err) = launch(e, &s, &job).await {
                    return json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{err:#}"));
                }
            }
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
        let a = args(&pl, "http://x", 0.0, 0.0, 0, "s0ad").join(" ");
        assert!(a.contains("-c:v copy"));
        assert!(a.contains("-c:a aac -ac 6"));
        assert!(!a.contains("-hwaccel"));
        assert!(!a.contains("-ss"));
        assert!(a.contains("s0ad_r0_init.mp4") && a.contains("-start_number 0") && a.contains("run0.m3u8"));
        // Pista elegida a mano.
        let pl2 = plan(&p, &chrome(), PlayMode::Remux, Some(2), HwAccel::Nvenc);
        assert_eq!(pl2.audio_index, Some(2));
        assert!(pl2.copy_audio);
        assert!(args(&pl2, "http://x", 0.0, 0.0, 0, "s0a2").join(" ").contains("-map 0:2"));
    }

    #[test]
    fn transcode_4k_hdr_to_1080p_sdr() {
        let p = probe_of("hevc", 3840, 2160, true, "aac", 2);
        let pl = plan(&p, &chrome(), PlayMode::Transcode, None, HwAccel::Nvenc);
        assert!(!pl.copy_video && pl.downscale && pl.tonemap && !pl.copy_audio);
        let a = args(&pl, "http://x", 125.5, 0.0, 0, "s125500ad").join(" ");
        assert!(a.contains("-hwaccel cuda"));
        assert!(a.contains("h264_nvenc"));
        assert!(a.contains("setparams=color_primaries=bt2020:color_trc=smpte2084"));
        assert!(a.contains("tonemap=tonemap=hable"));
        assert!(a.contains("-ss 125.500"));
        assert!(a.contains("expr:gte(t,n_forced*4)"));
        let sw = plan(&p, &chrome(), PlayMode::Transcode, None, HwAccel::None);
        assert!(args(&sw, "http://x", 0.0, 0.0, 0, "s0ad").join(" ").contains("libx264 -preset veryfast"));
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
        let a = args(&pl, "http://x", 0.0, 0.0, 0, "s0ad").join(" ");
        assert!(a.contains("-tag:v hvc1") && !a.contains("tonemap") && !a.contains("scale="));
    }

    #[test]
    fn fragment_end_semantics() {
        assert_eq!(fragment_end(487.99, 492.01), 492.01);
        assert_eq!(fragment_end(0.0, 4.0), 4.0);
        assert_eq!(fragment_end(100.0, 4.0), 104.0);
    }

    #[test]
    fn playlist_rewrite_and_names() {
        let src = "#EXTM3U\n#EXT-X-MAP:URI=\"s0ad_r0_init.mp4\"\n#EXTINF:4.0,\ns0ad_00000.m4s\n";
        let out = rewrite_playlist(src, Some("ab c"));
        assert!(out.contains("URI=\"s0ad_r0_init.mp4?t=ab%20c\""));
        assert!(out.contains("s0ad_00000.m4s?t=ab%20c\n"));
        assert_eq!(rewrite_playlist(src, None), src);
        assert_eq!(job_key(125.5, None), "s125500ad");
        assert_eq!(job_key(0.0, Some(2)), "s0a2");
        assert_eq!(parse_segment("s0ad_00034.m4s"), Some(("s0ad".into(), Some(34))));
        assert_eq!(parse_segment("s0a2_r12_init.mp4"), Some(("s0a2".into(), None)));
        assert_eq!(parse_segment("index.m3u8"), None);
        assert_eq!(parse_segment("s0ad_x.m4s"), None);
        assert_eq!(parse_segment("s0ad_rx_init.mp4"), None);
    }

    #[test]
    fn resume_keeps_numbering_and_time() {
        let pl = plan(&probe_of("hevc", 1920, 1080, false, "aac", 2), &chrome(), PlayMode::Transcode, None, HwAccel::Nvenc);
        let a = args(&pl, "http://x", 100.0, 48.0, 12, "s100000ad").join(" ");
        assert!(a.contains("-ss 148.000"));
        assert!(a.contains("-output_ts_offset 48.000"));
        assert!(a.contains("-start_number 12"));
        assert!(a.contains("s100000ad_r12_init.mp4") && a.contains("run12.m3u8"));
        // Playlist unida de dos corridas: un MAP por corrida, numeración continua.
        let r0 = parse_run_playlist("#EXT-X-MAP:URI=\"k_r0_init.mp4\"\n#EXTINF:4.000000,\nk_00000.m4s\n#EXTINF:3.5,\nk_00001.m4s\n", "x");
        let r1 = parse_run_playlist("#EXTINF:4.0,\nk_00002.m4s\n", "k_r2_init.mp4");
        assert_eq!(r0.len(), 2);
        assert_eq!(r0[1].dur, 3.5);
        let all: Vec<Entry> = r0.into_iter().chain(r1).collect();
        let p = build_playlist(&all, true);
        assert_eq!(p.matches("#EXT-X-MAP").count(), 2);
        assert!(p.contains("#EXT-X-MAP:URI=\"k_r2_init.mp4\"\n#EXTINF:4.000000,\nk_00002.m4s"));
        assert!(p.ends_with("#EXT-X-ENDLIST\n"));
        assert!(p.contains("#EXT-X-TARGETDURATION:4"));
        assert!(!build_playlist(&all, false).contains("ENDLIST"));
    }
}
