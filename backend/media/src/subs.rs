//! Subtítulos internos de texto (subrip, ass, mov_text, webvtt) → WebVTT con ffmpeg.
//!
//! Los subtítulos de un MKV están intercalados en todo el archivo, así que extraerlos obliga a
//! recorrerlo entero. Para no robarle prioridad a la película, ffmpeg lee con el rol **pasivo**
//! (solo piezas que ya están en disco, sin pedir nada): avanza a medida que avanza la descarga.
//!
//! - Resultado cacheado en disco: `data_dir/subs/{hash}_{archivo}_{pista}.vtt` para torrents (sirve
//!   para siempre) o en la carpeta temporal de la sesión para links directos.
//! - Si la extracción no terminó en ~20 s se devuelve lo que haya hasta ahora (VTT válido, cubre
//!   desde el principio hasta donde está bajado) con `X-Mythos-Partial: true`; la UI puede volver
//!   a pedirlo más tarde para completar.

use crate::engine::Engine;
use crate::session::PlaySession;
use crate::stream::{json_error, Role};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TEXT_CODECS: &[&str] = &["subrip", "srt", "ass", "ssa", "mov_text", "webvtt", "text"];
const WAIT_FULL: Duration = Duration::from_secs(20);

struct Job {
    part: PathBuf,
    done: AtomicBool,
    error: Mutex<Option<String>>,
}

#[derive(Default)]
pub struct SubsState {
    jobs: Mutex<HashMap<usize, Arc<Job>>>,
}

fn cache_path(e: &Engine, s: &PlaySession, idx: usize) -> PathBuf {
    match s.torrent() {
        Some(t) => e.subs_dir.join(format!("{}_{}_{}.vtt", t.hash, t.idx, idx)),
        None => s.tmp_dir.join(format!("subs_{idx}.vtt")),
    }
}

fn vtt(body: Vec<u8>, partial: bool) -> Response {
    let mut r = body.into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/vtt; charset=utf-8"));
    if partial {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        h.insert("x-mythos-partial", HeaderValue::from_static("true"));
    } else {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=86400"));
    }
    r
}

pub async fn serve(e: &Engine, s: Arc<PlaySession>, idx: usize) -> Response {
    s.touch();
    let ok = s.probe.lock().unwrap().as_ref().is_some_and(|p| p.subs.iter().any(|t| t.index == idx && TEXT_CODECS.contains(&t.codec.as_str())));
    if !ok {
        return json_error(StatusCode::NOT_FOUND, "no such text subtitle track");
    }
    let out = cache_path(e, &s, idx);
    if let Ok(b) = tokio::fs::read(&out).await {
        return vtt(b, false);
    }
    let job = {
        let mut jobs = s.subs.jobs.lock().unwrap();
        match jobs.get(&idx) {
            Some(j) if j.error.lock().unwrap().is_none() => j.clone(),
            _ => match spawn(e, &s, idx, &out) {
                Ok(j) => {
                    jobs.insert(idx, j.clone());
                    j
                }
                Err(err) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("could not start ffmpeg: {err:#}")),
            },
        }
    };
    let t0 = Instant::now();
    while !job.done.load(Ordering::Relaxed) && t0.elapsed() < WAIT_FULL {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if let Some(err) = job.error.lock().unwrap().clone() {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, err);
    }
    if job.done.load(Ordering::Relaxed) {
        if let Ok(b) = tokio::fs::read(&out).await {
            return vtt(b, false);
        }
    }
    // Parcial: lo extraído hasta ahora (recortado al último bloque completo).
    let b = tokio::fs::read(&job.part).await.unwrap_or_default();
    vtt(complete_cues(&b), true)
}

/// Deja solo los bloques terminados (hasta la última línea en blanco) para no cortar un cue.
pub fn complete_cues(b: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(b);
    if !text.starts_with("WEBVTT") {
        return b"WEBVTT\n\n".to_vec();
    }
    match text.rfind("\n\n") {
        Some(i) => text[..i + 2].as_bytes().to_vec(),
        None => b"WEBVTT\n\n".to_vec(),
    }
}

fn spawn(e: &Engine, s: &Arc<PlaySession>, idx: usize, out: &Path) -> anyhow::Result<Arc<Job>> {
    if let Some(p) = out.parent() {
        std::fs::create_dir_all(p)?;
    }
    let part = out.with_extension("vtt.part");
    let input = e.internal_url(&s.id, Role::Passive);
    let mut child = crate::util::tool_command(&e.cfg.ffmpeg)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i", &input, "-map", &format!("0:{idx}"), "-c:s", "webvtt", "-flush_packets", "1", "-f", "webvtt"])
        .arg(&part)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let job = Arc::new(Job { part: part.clone(), done: AtomicBool::new(false), error: Mutex::new(None) });
    let j = job.clone();
    let out = out.to_path_buf();
    let sess = s.clone();
    let stderr = child.stderr.take();
    tokio::spawn(async move {
        let last_err = Arc::new(Mutex::new(String::new()));
        if let Some(err) = stderr {
            let le = last_err.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let mut lines = tokio::io::BufReader::new(err).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    *le.lock().unwrap() = l;
                }
            });
        }
        // Si la sesión se cierra a mitad de camino, se mata ffmpeg (el lector pasivo esperaría
        // piezas que quizá nunca lleguen).
        let status = loop {
            tokio::select! {
                st = child.wait() => break st,
                _ = tokio::time::sleep(Duration::from_secs(2)) => {
                    if sess.is_closed() {
                        let _ = child.start_kill();
                    }
                }
            }
        };
        match status {
            Ok(st) if st.success() => {
                let _ = tokio::fs::rename(&part, &out).await;
                j.done.store(true, Ordering::Relaxed);
            }
            Ok(_) => {
                let msg = last_err.lock().unwrap().clone();
                *j.error.lock().unwrap() = Some(format!("subtitle extraction failed: {msg}"));
                let _ = tokio::fs::remove_file(&part).await;
            }
            Err(err) => *j.error.lock().unwrap() = Some(format!("subtitle extraction failed: {err}")),
        }
    });
    Ok(job)
}

/// Al borrar un torrent con sus archivos, también sus subtítulos cacheados.
pub async fn forget_torrent(dir: &Path, hash: &str) {
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else { return };
    while let Ok(Some(ent)) = rd.next_entry().await {
        if ent.file_name().to_string_lossy().starts_with(hash) {
            let _ = tokio::fs::remove_file(ent.path()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_cues() {
        let b = b"WEBVTT\n\n00:00.000 --> 00:01.000\nHola\n\n00:02.000 --> 00:03.000\nCort";
        assert_eq!(complete_cues(b), b"WEBVTT\n\n00:00.000 --> 00:01.000\nHola\n\n".to_vec());
        assert_eq!(complete_cues(b""), b"WEBVTT\n\n".to_vec());
    }
}
