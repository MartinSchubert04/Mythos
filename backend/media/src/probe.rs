//! ffprobe y elección de modo (`direct` / `remux` / `transcode`).
//!
//! Anti-trabas: ffprobe corre con `-probesize`/`-analyzeduration` acotados y lee por el endpoint
//! local (rol `probe`), así las piezas que necesita (el principio y, en MP4 con el moov al final,
//! el final) se piden con prioridad. Si ffprobe falla o tarda demasiado, igual devolvemos un
//! `PlayInfo` con lo que se sepa, adivinando por la extensión: nunca dejamos al usuario sin poder
//! intentar reproducir.

use crate::util;
use crate::{ClientCaps, PlayMode, Track};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct VideoInfo {
    pub index: usize,
    pub codec: String,
    pub profile: Option<String>,
    pub pix_fmt: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub hdr: bool,
    /// `color_transfer` de ffprobe (smpte2084 = PQ, arib-std-b67 = HLG).
    pub transfer: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AudioInfo {
    pub track: Track,
    pub channels: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Probe {
    /// Contenedor normalizado: "mkv", "mp4", "webm", "avi", "ts"...
    pub container: Option<String>,
    pub duration: Option<f64>,
    /// bits/s
    pub bit_rate: Option<u64>,
    pub video: Option<VideoInfo>,
    pub audio: Vec<AudioInfo>,
    pub subs: Vec<Track>,
    /// `true` si salió de ffprobe; `false` si es adivinado.
    pub real: bool,
}

impl Probe {
    /// Pista de audio por defecto: la marcada como default, si no la primera.
    pub fn default_audio(&self) -> Option<&AudioInfo> {
        self.audio.iter().find(|a| a.track.default == Some(true)).or(self.audio.first())
    }
}

/// Corre ffprobe sobre `input` (URL del endpoint interno). `name` es el nombre real del archivo
/// (la URL interna no tiene extensión, y la extensión distingue webm de mkv).
pub async fn run(ffprobe: &Path, input: &str, name: &str, timeout: Duration) -> anyhow::Result<Probe> {
    let mut cmd = tokio::process::Command::new(ffprobe);
    cmd.args([
        "-v", "error",
        "-probesize", "10000000",
        "-analyzeduration", "5000000",
        "-show_format", "-show_streams",
        "-of", "json",
        input,
    ]);
    cmd.kill_on_drop(true).stdin(std::process::Stdio::null());
    let out = tokio::time::timeout(timeout, cmd.output()).await??;
    anyhow::ensure!(out.status.success(), "ffprobe failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    let v: Value = serde_json::from_slice(&out.stdout)?;
    Ok(parse(&v, name))
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string())
}

fn tag(v: &Value, k: &str) -> Option<String> {
    let tags = v.get("tags")?;
    tags.get(k).or_else(|| tags.get(k.to_uppercase())).and_then(|x| x.as_str()).map(|x| x.to_string())
}

pub fn normalize_container(format_name: &str, ext: &str) -> String {
    if format_name.contains("matroska") || format_name.contains("webm") {
        if ext == "webm" { "webm" } else { "mkv" }.into()
    } else if format_name.contains("mp4") || format_name.contains("mov") {
        if ext == "mov" { "mov" } else { "mp4" }.into()
    } else if format_name.contains("mpegts") {
        "ts".into()
    } else if format_name.contains("avi") {
        "avi".into()
    } else {
        format_name.split(',').next().unwrap_or(format_name).to_string()
    }
}

pub fn parse(v: &Value, name: &str) -> Probe {
    let fmt = v.get("format").cloned().unwrap_or(Value::Null);
    let ext = util::ext_of(name);
    let mut p = Probe {
        container: s(&fmt, "format_name").map(|f| normalize_container(&f, &ext)),
        duration: s(&fmt, "duration").and_then(|d| d.parse().ok()),
        bit_rate: s(&fmt, "bit_rate").and_then(|d| d.parse().ok()),
        real: true,
        ..Default::default()
    };
    for st in v.get("streams").and_then(|x| x.as_array()).into_iter().flatten() {
        let index = st.get("index").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
        let codec = s(st, "codec_name").unwrap_or_else(|| "unknown".into());
        let default = st.get("disposition").and_then(|d| d.get("default")).and_then(|x| x.as_i64()).map(|x| x == 1);
        let attached = st.get("disposition").and_then(|d| d.get("attached_pic")).and_then(|x| x.as_i64()) == Some(1);
        let track = Track { index, codec: codec.clone(), lang: tag(st, "language"), title: tag(st, "title"), default };
        match s(st, "codec_type").as_deref() {
            Some("video") if p.video.is_none() && !attached => {
                let trc = s(st, "color_transfer").unwrap_or_default();
                let dv = st
                    .get("side_data_list")
                    .and_then(|x| x.as_array())
                    .is_some_and(|l| l.iter().any(|d| s(d, "side_data_type").is_some_and(|t| t.contains("DOVI"))));
                p.video = Some(VideoInfo {
                    index,
                    codec,
                    profile: s(st, "profile"),
                    pix_fmt: s(st, "pix_fmt"),
                    width: st.get("width").and_then(|x| x.as_u64()).map(|x| x as u32),
                    height: st.get("height").and_then(|x| x.as_u64()).map(|x| x as u32),
                    hdr: trc == "smpte2084" || trc == "arib-std-b67" || dv,
                    transfer: (!trc.is_empty()).then_some(trc),
                });
            }
            Some("audio") => p.audio.push(AudioInfo {
                track,
                channels: st.get("channels").and_then(|x| x.as_u64()).unwrap_or(2) as u32,
            }),
            Some("subtitle") => p.subs.push(track),
            _ => {}
        }
    }
    p
}

/// Si ffprobe no anduvo: lo que se pueda deducir del nombre del archivo.
pub fn guess(name: &str) -> Probe {
    let ext = util::ext_of(name);
    let lower = name.to_ascii_lowercase();
    let vcodec = if lower.contains("x265") || lower.contains("hevc") || lower.contains("h265") || lower.contains("h.265") {
        "hevc"
    } else if lower.contains("av1") {
        "av1"
    } else if lower.contains("xvid") || lower.contains("divx") {
        "mpeg4"
    } else if ext == "webm" {
        "vp9"
    } else {
        "h264"
    };
    let container = match ext.as_str() {
        "m4v" | "mp4" => "mp4".to_string(),
        "" => "unknown".to_string(),
        e => e.to_string(),
    };
    let hdr = lower.contains("hdr") || lower.contains(".dv.") || lower.contains("dolby.vision");
    Probe {
        container: Some(container),
        video: Some(VideoInfo { codec: vcodec.into(), hdr, ..Default::default() }),
        real: false,
        ..Default::default()
    }
}

pub fn video_ok(v: &VideoInfo, caps: &ClientCaps) -> bool {
    let ten_bit = v.pix_fmt.as_deref().is_some_and(|f| f.contains("10") || f.contains("12"));
    match v.codec.as_str() {
        // Los navegadores no decodifican H.264 High 10 / 4:2:2 / 4:4:4.
        "h264" => {
            caps.h264 && !ten_bit && !v.profile.as_deref().is_some_and(|p| p.contains("10") || p.contains("4:2:2") || p.contains("4:4:4"))
        }
        "hevc" => caps.hevc,
        "av1" => caps.av1,
        "vp9" => caps.vp9,
        "vp8" => caps.vp9,
        _ => false,
    }
}

pub fn audio_ok(codec: &str, caps: &ClientCaps) -> bool {
    match codec {
        "aac" => caps.aac,
        "mp3" => true,
        "opus" => caps.opus,
        "vorbis" => caps.opus,
        "flac" => caps.flac,
        "ac3" => caps.ac3,
        "eac3" => caps.eac3,
        "dts" => caps.dts,
        _ => false,
    }
}

pub fn container_ok(c: &str, caps: &ClientCaps) -> bool {
    match c {
        "mp4" | "mov" | "webm" => true,
        "mkv" => caps.mkv,
        _ => false,
    }
}

/// `direct` si contenedor + video + audio andan en el cliente; `remux` si el video sí pero el
/// contenedor o el audio no; `transcode` si no.
pub fn choose_mode(p: &Probe, caps: &ClientCaps) -> PlayMode {
    let Some(v) = &p.video else {
        // Sin video (o desconocido): que el navegador lo intente.
        return PlayMode::Direct;
    };
    if !video_ok(v, caps) {
        return PlayMode::Transcode;
    }
    let a_ok = p.default_audio().is_none_or(|a| audio_ok(&a.track.codec, caps));
    let c_ok = p.container.as_deref().is_some_and(|c| container_ok(c, caps));
    // Un MP4 con un audio que el navegador no conoce pero que no trae audio compatible sería
    // mudo en directo: va por remux (copia el video y pasa el audio a AAC).
    if a_ok && c_ok {
        PlayMode::Direct
    } else {
        PlayMode::Remux
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps_chrome() -> ClientCaps {
        ClientCaps { h264: true, vp9: true, av1: true, aac: true, opus: true, flac: true, mkv: false, ..Default::default() }
    }

    fn probe(container: &str, v: &str, a: &str) -> Probe {
        Probe {
            container: Some(container.into()),
            video: Some(VideoInfo { codec: v.into(), pix_fmt: Some("yuv420p".into()), ..Default::default() }),
            audio: vec![AudioInfo { track: Track { index: 1, codec: a.into(), lang: None, title: None, default: Some(true) }, channels: 2 }],
            real: true,
            ..Default::default()
        }
    }

    #[test]
    fn modes() {
        let c = caps_chrome();
        assert_eq!(choose_mode(&probe("mp4", "h264", "aac"), &c), PlayMode::Direct);
        assert_eq!(choose_mode(&probe("mkv", "h264", "aac"), &c), PlayMode::Remux);
        assert_eq!(choose_mode(&probe("mp4", "h264", "ac3"), &c), PlayMode::Remux);
        assert_eq!(choose_mode(&probe("mkv", "hevc", "aac"), &c), PlayMode::Transcode);
        assert_eq!(choose_mode(&probe("avi", "mpeg4", "mp3"), &c), PlayMode::Transcode);
        let mut c2 = c.clone();
        c2.mkv = true;
        c2.hevc = true;
        c2.eac3 = true;
        assert_eq!(choose_mode(&probe("mkv", "hevc", "eac3"), &c2), PlayMode::Direct);
        assert_eq!(choose_mode(&probe("mkv", "hevc", "dts"), &c2), PlayMode::Remux);
        let mut hi10 = probe("mp4", "h264", "aac");
        hi10.video.as_mut().unwrap().pix_fmt = Some("yuv420p10le".into());
        assert_eq!(choose_mode(&hi10, &c), PlayMode::Transcode);
    }

    #[test]
    fn parse_ffprobe_json() {
        let v: Value = serde_json::json!({
            "format": {"filename": "http://127.0.0.1/x", "format_name": "matroska,webm", "duration": "888.0", "bit_rate": "5000000"},
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "hevc", "width": 3840, "height": 2160, "pix_fmt": "yuv420p10le", "color_transfer": "smpte2084", "profile": "Main 10"},
                {"index": 1, "codec_type": "audio", "codec_name": "eac3", "channels": 6, "tags": {"language": "eng"}, "disposition": {"default": 1}},
                {"index": 2, "codec_type": "audio", "codec_name": "aac", "channels": 2, "tags": {"language": "spa", "title": "Latino"}, "disposition": {"default": 0}},
                {"index": 3, "codec_type": "subtitle", "codec_name": "subrip", "tags": {"language": "spa"}},
                {"index": 4, "codec_type": "video", "codec_name": "mjpeg", "disposition": {"attached_pic": 1}}
            ]
        });
        let p = parse(&v, "Movie.mkv");
        assert_eq!(parse(&v, "Movie.webm").container.as_deref(), Some("webm"));
        assert_eq!(p.container.as_deref(), Some("mkv"));
        assert_eq!(p.duration, Some(888.0));
        let vi = p.video.as_ref().unwrap();
        assert!(vi.hdr);
        assert_eq!(vi.width, Some(3840));
        assert_eq!(p.audio.len(), 2);
        assert_eq!(p.default_audio().unwrap().track.index, 1);
        assert_eq!(p.audio[1].track.title.as_deref(), Some("Latino"));
        assert_eq!(p.subs.len(), 1);
        assert_eq!(p.subs[0].codec, "subrip");
    }

    #[test]
    fn guessing() {
        let g = guess("Movie.2010.2160p.HDR.x265-GRP.mkv");
        assert_eq!(g.container.as_deref(), Some("mkv"));
        assert_eq!(g.video.as_ref().unwrap().codec, "hevc");
        assert!(g.video.as_ref().unwrap().hdr);
        assert_eq!(guess("a.mp4").video.unwrap().codec, "h264");
        assert_eq!(normalize_container("mov,mp4,m4a,3gp,3g2,mj2", "mp4"), "mp4");
    }
}
