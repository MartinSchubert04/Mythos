//! Sondeo sin ffprobe: lee las cabeceras de MP4 (`moov`) y Matroska/WebM (`Info` + `Tracks`) en
//! Rust puro. Lo usa el motor cuando ffprobe no está (modo local en Android/Windows sin ffmpeg) o
//! falló; si tampoco sirve, queda `probe::guess` (por el nombre).
//!
//! Es a propósito chico: solo lo que necesita `choose_mode` y el `PlayInfo` (contenedor, duración,
//! códecs, resolución, 10 bits / HDR, pistas de audio y subtítulos con idioma). Lee por el endpoint
//! interno con `Range`, así las piezas que pide (principio; en MP4 con el `moov` al final, el
//! final) se bajan con prioridad igual que con ffprobe.

use crate::probe::{AudioInfo, Probe, VideoInfo};
use crate::{util, Track};
use std::time::Duration;

/// Cuánto principio del archivo se lee de una (alcanza para `Info` + `Tracks` de casi todo MKV y
/// para el `moov` de un MP4 "faststart").
const HEAD: u64 = 2 << 20;
/// Tope del `moov` / `Tracks` que aceptamos leer.
const MAX_BOX: u64 = 32 << 20;

/// Sondea `url` (endpoint interno, acepta `Range`). `size` es el tamaño del archivo y `name` su
/// nombre real (para distinguir extensiones).
pub async fn run(http: &reqwest::Client, url: &str, size: u64, name: &str, timeout: Duration) -> anyhow::Result<Probe> {
    tokio::time::timeout(timeout, run_inner(http, url, size, name)).await?
}

async fn run_inner(http: &reqwest::Client, url: &str, size: u64, name: &str) -> anyhow::Result<Probe> {
    anyhow::ensure!(size > 16, "file too small");
    let head = read(http, url, 0, HEAD.min(size)).await?;
    let ext = util::ext_of(name);
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        let mut p = parse_mkv(&head, &ext);
        if p.video.is_none() && p.audio.is_empty() {
            // `Tracks` más allá del principio: la SeekHead dice dónde.
            if let Some(pos) = mkv_tracks_pos(&head) {
                let chunk = read(http, url, pos, (1 << 20).min(size.saturating_sub(pos))).await?;
                let mut q = Probe::default();
                mkv_tracks(&chunk, &mut q);
                p.video = q.video;
                p.audio = q.audio;
                p.subs = q.subs;
            }
        }
        anyhow::ensure!(p.video.is_some() || !p.audio.is_empty(), "matroska without tracks");
        return Ok(p);
    }
    if head.len() >= 8 && &head[4..8] == b"ftyp" {
        let mut off = 0u64;
        let mut brand = String::new();
        for _ in 0..64 {
            if off + 8 > size {
                break;
            }
            let hdr = if off + 16 <= head.len() as u64 {
                head[off as usize..off as usize + 16].to_vec()
            } else {
                read(http, url, off, 16.min(size - off)).await?
            };
            let Some((kind, box_len, hdr_len)) = box_header(&hdr, size - off) else { break };
            if kind == *b"ftyp" && hdr.len() >= 12 {
                brand = String::from_utf8_lossy(&hdr[8..12]).into_owned();
            }
            if kind == *b"moov" {
                anyhow::ensure!(box_len <= MAX_BOX, "moov too large");
                let body = if off + box_len <= head.len() as u64 {
                    head[(off + hdr_len) as usize..(off + box_len) as usize].to_vec()
                } else {
                    read(http, url, off + hdr_len, box_len - hdr_len).await?
                };
                let mut p = parse_moov(&body);
                p.container = Some(if brand.starts_with("qt") || ext == "mov" { "mov" } else { "mp4" }.into());
                return Ok(p);
            }
            off += box_len;
        }
        anyhow::bail!("mp4 without moov");
    }
    anyhow::bail!("unknown container")
}

async fn read(http: &reqwest::Client, url: &str, start: u64, len: u64) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(len > 0, "empty range");
    let r = http
        .get(url)
        .header(reqwest::header::RANGE, format!("bytes={}-{}", start, start + len - 1))
        .send()
        .await?
        .error_for_status()?;
    let b = r.bytes().await?;
    // Si el server ignorara el Range y mandara todo, nos quedamos con el pedazo pedido.
    Ok(b.into_iter().take(len as usize).collect())
}

// ---------- MP4 ----------

/// (tipo, largo total, largo de la cabecera) de una caja. `remaining` = bytes hasta el final del
/// contenedor (para `size == 0`, "hasta el final").
fn box_header(b: &[u8], remaining: u64) -> Option<([u8; 4], u64, u64)> {
    if b.len() < 8 {
        return None;
    }
    let size32 = u32::from_be_bytes(b[0..4].try_into().ok()?) as u64;
    let kind: [u8; 4] = b[4..8].try_into().ok()?;
    let (len, hdr) = match size32 {
        0 => (remaining, 8),
        1 => (u64::from_be_bytes(b.get(8..16)?.try_into().ok()?), 16),
        n => (n, 8),
    };
    (len >= hdr).then_some((kind, len, hdr))
}

/// Cajas hijas de `b`: (tipo, contenido).
fn boxes(b: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 8 <= b.len() {
        let Some((kind, len, hdr)) = box_header(&b[off..], (b.len() - off) as u64) else { break };
        let end = (off as u64 + len).min(b.len() as u64) as usize;
        out.push((kind, &b[off + hdr as usize..end]));
        if len == 0 {
            break;
        }
        off = end;
    }
    out
}

fn child<'a>(b: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(b).into_iter().find(|(k, _)| k == kind).map(|(_, v)| v)
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// Contenido de `moov` → `Probe` (sin `container`).
pub fn parse_moov(moov: &[u8]) -> Probe {
    let mut p = Probe::default();
    if let Some(mvhd) = child(moov, b"mvhd") {
        let (ts, dur) = if mvhd.first() == Some(&1) {
            (be32(mvhd, 20), be64(mvhd, 24))
        } else {
            (be32(mvhd, 12), be32(mvhd, 16).map(u64::from))
        };
        if let (Some(ts), Some(d)) = (ts, dur) {
            if ts > 0 && d > 0 && d != u32::MAX as u64 && d != u64::MAX {
                p.duration = Some(d as f64 / ts as f64);
            }
        }
    }
    for (index, trak) in boxes(moov).into_iter().filter(|(k, _)| k == b"trak").map(|(_, v)| v).enumerate() {
        let enabled = child(trak, b"tkhd").and_then(|t| t.get(3)).is_some_and(|f| f & 1 == 1);
        let Some(mdia) = child(trak, b"mdia") else { continue };
        let lang = child(mdia, b"mdhd").and_then(|m| {
            let at = if m.first() == Some(&1) { 32 } else { 20 };
            mp4_lang(be16(m, at)?)
        });
        let handler = child(mdia, b"hdlr").and_then(|h| h.get(8..12)).map(|h| [h[0], h[1], h[2], h[3]]);
        let Some(stsd) = child(mdia, b"minf").and_then(|m| child(m, b"stbl")).and_then(|s| child(s, b"stsd")) else { continue };
        // stsd: versión/flags (4) + cantidad (4) + entradas (cajas).
        let Some((fourcc, entry)) = stsd.get(8..).and_then(|e| boxes(e).into_iter().next()) else { continue };
        let codec = mp4_codec(&fourcc);
        let track = Track { index, codec: codec.clone(), lang, title: None, default: Some(enabled) };
        match handler.as_ref() {
            Some(b"vide") if p.video.is_none() => {
                // VisualSampleEntry: 78 bytes fijos y después cajas (avcC, hvcC, colr, dvcC...).
                let kids = entry.get(78..).map(boxes).unwrap_or_default();
                let mut v = VideoInfo {
                    index,
                    codec,
                    width: be16(entry, 24).map(u32::from).filter(|w| *w > 0),
                    height: be16(entry, 26).map(u32::from).filter(|h| *h > 0),
                    ..Default::default()
                };
                for (k, c) in kids {
                    match &k {
                        b"avcC" => set_avc_profile(&mut v, c.get(1).copied()),
                        b"hvcC" => set_hevc_profile(&mut v, c.get(1).map(|x| x & 0x1f)),
                        b"colr" if c.get(0..4) == Some(b"nclx") => set_transfer(&mut v, be16(c, 6).map(u64::from)),
                        b"dvcC" | b"dvvC" => v.hdr = true,
                        _ => {}
                    }
                }
                p.video = Some(v);
            }
            Some(b"soun") => {
                let channels = be16(entry, 16).map(u32::from).filter(|c| *c > 0).unwrap_or(2);
                p.audio.push(AudioInfo { track, channels });
            }
            Some(b"text" | b"sbtl" | b"subt") => p.subs.push(track),
            _ => {}
        }
    }
    p.real = p.video.is_some() || !p.audio.is_empty();
    p
}

fn mp4_lang(packed: u16) -> Option<String> {
    let c = |shift: u16| (((packed >> shift) & 0x1f) as u8 + 0x60) as char;
    let s: String = [c(10), c(5), c(0)].iter().collect();
    (s.chars().all(|c| c.is_ascii_lowercase()) && s != "und").then_some(s)
}

fn mp4_codec(fourcc: &[u8; 4]) -> String {
    match fourcc {
        b"avc1" | b"avc3" => "h264",
        b"hvc1" | b"hev1" | b"dvh1" | b"dvhe" => "hevc",
        b"av01" => "av1",
        b"vp09" => "vp9",
        b"vp08" => "vp8",
        b"mp4v" => "mpeg4",
        b"mp4a" => "aac",
        b"ac-3" => "ac3",
        b"ec-3" => "eac3",
        b"Opus" => "opus",
        b"fLaC" => "flac",
        b".mp3" => "mp3",
        b"dtsc" | b"dtsh" | b"dtsl" | b"dtse" => "dts",
        b"tx3g" => "mov_text",
        b"wvtt" => "webvtt",
        _ => return String::from_utf8_lossy(fourcc).trim().to_ascii_lowercase(),
    }
    .into()
}

fn set_avc_profile(v: &mut VideoInfo, profile_idc: Option<u8>) {
    // 110 High 10, 122 High 4:2:2, 244 High 4:4:4: el navegador no los decodifica.
    match profile_idc {
        Some(110) => {
            v.profile = Some("High 10".into());
            v.pix_fmt = Some("yuv420p10le".into());
        }
        Some(122) => v.profile = Some("High 4:2:2".into()),
        Some(244) => v.profile = Some("High 4:4:4 Predictive".into()),
        Some(_) => v.pix_fmt = Some("yuv420p".into()),
        None => {}
    }
}

fn set_hevc_profile(v: &mut VideoInfo, profile_idc: Option<u8>) {
    if profile_idc == Some(2) {
        v.profile = Some("Main 10".into());
        v.pix_fmt = Some("yuv420p10le".into());
    }
}

/// Transferencia (ITU-T H.273): 16 = PQ (HDR10/DV), 18 = HLG.
fn set_transfer(v: &mut VideoInfo, tc: Option<u64>) {
    match tc {
        Some(16) => {
            v.hdr = true;
            v.transfer = Some("smpte2084".into());
        }
        Some(18) => {
            v.hdr = true;
            v.transfer = Some("arib-std-b67".into());
        }
        _ => {}
    }
}

// ---------- Matroska / WebM ----------

const EBML_DOCTYPE: u64 = 0x4282;
const SEGMENT: u64 = 0x1853_8067;
const SEEKHEAD: u64 = 0x114D_9B74;
const SEEK: u64 = 0x4DBB;
const SEEK_ID: u64 = 0x53AB;
const SEEK_POS: u64 = 0x53AC;
const INFO: u64 = 0x1549_A966;
const TIMESCALE: u64 = 0x2AD7B1;
const DURATION: u64 = 0x4489;
const TRACKS: u64 = 0x1654_AE6B;
const TRACK_ENTRY: u64 = 0xAE;
const CLUSTER: u64 = 0x1F43_B675;

/// Lee un vint: (valor, largo). `keep_marker` para los IDs (que se comparan con el marcador).
fn vint(b: &[u8], keep_marker: bool) -> Option<(u64, usize)> {
    let first = *b.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || b.len() < len {
        return None;
    }
    let mut v = if keep_marker { first as u64 } else { (first as u64) & ((1u64 << (8 - len)) - 1) };
    for x in &b[1..len] {
        v = (v << 8) | *x as u64;
    }
    Some((v, len))
}

/// Un elemento: (id, contenido, largo total). Un tamaño "desconocido" (todos unos) se toma como
/// "hasta el final del buffer" (Segment y Cluster en vivo).
fn element(b: &[u8]) -> Option<(u64, &[u8], usize)> {
    let (id, il) = vint(b, true)?;
    let (size, sl) = vint(&b[il..], false)?;
    let unknown = size == (1u64 << (7 * sl)) - 1;
    let start = il + sl;
    let end = if unknown { b.len() } else { (start as u64).saturating_add(size).min(b.len() as u64) as usize };
    Some((id, &b[start..end], end))
}

fn elements(b: &[u8]) -> Vec<(u64, &[u8])> {
    let mut out = Vec::new();
    let mut off = 0;
    while off < b.len() {
        let Some((id, data, len)) = element(&b[off..]) else { break };
        out.push((id, data));
        if id == CLUSTER || len == 0 {
            break;
        }
        off += len;
    }
    out
}

fn uint(b: &[u8]) -> u64 {
    b.iter().take(8).fold(0, |acc, x| (acc << 8) | *x as u64)
}

fn float(b: &[u8]) -> Option<f64> {
    match b.len() {
        4 => Some(f32::from_be_bytes(b.try_into().ok()?) as f64),
        8 => Some(f64::from_be_bytes(b.try_into().ok()?)),
        _ => None,
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).trim_end_matches('\0').to_string()
}

/// Comienzo de los datos del Segment (las posiciones de la SeekHead son relativas a eso).
fn segment_of(buf: &[u8]) -> Option<(usize, &[u8])> {
    let mut off = 0;
    while off < buf.len() {
        let (id, il) = vint(&buf[off..], true)?;
        let (size, sl) = vint(&buf[off + il..], false)?;
        let start = off + il + sl;
        if id == SEGMENT {
            return Some((start, &buf[start..]));
        }
        off = start.checked_add(size as usize)?;
    }
    None
}

/// Principio de un Matroska → `Probe`. Si `Tracks` no entra en `buf`, devuelve lo que haya.
pub fn parse_mkv(buf: &[u8], ext: &str) -> Probe {
    let mut p = Probe { container: Some("mkv".into()), ..Default::default() };
    if let Some((_, header, _)) = element(buf) {
        if let Some((_, dt)) = elements(header).into_iter().find(|(id, _)| *id == EBML_DOCTYPE) {
            if text(dt) == "webm" || ext == "webm" {
                p.container = Some("webm".into());
            }
        }
    }
    let Some((_, seg)) = segment_of(buf) else { return p };
    for (id, data) in elements(seg) {
        match id {
            INFO => {
                let mut scale = 1_000_000u64;
                let mut dur = None;
                for (k, v) in elements(data) {
                    match k {
                        TIMESCALE => scale = uint(v).max(1),
                        DURATION => dur = float(v),
                        _ => {}
                    }
                }
                p.duration = dur.map(|d| d * scale as f64 / 1e9).filter(|d| d.is_finite() && *d > 0.0);
            }
            TRACKS => mkv_tracks_body(data, &mut p),
            _ => {}
        }
    }
    p.real = p.video.is_some() || !p.audio.is_empty();
    p
}

/// Posición absoluta (en el archivo) del elemento `Tracks`, según la SeekHead.
fn mkv_tracks_pos(buf: &[u8]) -> Option<u64> {
    let (seg_start, seg) = segment_of(buf)?;
    let (_, head) = elements(seg).into_iter().find(|(id, _)| *id == SEEKHEAD)?;
    for (id, seek) in elements(head) {
        if id != SEEK {
            continue;
        }
        let kids = elements(seek);
        let target = kids.iter().find(|(k, _)| *k == SEEK_ID).map(|(_, v)| uint(v));
        let pos = kids.iter().find(|(k, _)| *k == SEEK_POS).map(|(_, v)| uint(v));
        if let (Some(TRACKS), Some(pos)) = (target, pos) {
            return Some(seg_start as u64 + pos);
        }
    }
    None
}

/// `buf` empieza en el elemento `Tracks`.
fn mkv_tracks(buf: &[u8], p: &mut Probe) {
    if let Some((TRACKS, data, _)) = element(buf) {
        mkv_tracks_body(data, p);
    }
}

fn mkv_tracks_body(data: &[u8], p: &mut Probe) {
    for (index, (_, entry)) in elements(data).into_iter().filter(|(id, _)| *id == TRACK_ENTRY).enumerate() {
        let (mut kind, mut codec_id, mut lang, mut lang_bcp, mut name) = (0u64, String::new(), None, None, None);
        let mut default = true;
        let (mut width, mut height, mut transfer, mut channels) = (None, None, None, 2u32);
        let mut private: &[u8] = &[];
        for (k, v) in elements(entry) {
            match k {
                0x83 => kind = uint(v),
                0x86 => codec_id = text(v),
                0x22B59C => lang = Some(text(v)),
                0x22B59D => lang_bcp = Some(text(v)),
                0x536E => name = Some(text(v)).filter(|s| !s.is_empty()),
                0x88 => default = uint(v) == 1,
                0x63A2 => private = v,
                0xE0 => {
                    for (vk, vv) in elements(v) {
                        match vk {
                            0xB0 => width = Some(uint(vv) as u32),
                            0xBA => height = Some(uint(vv) as u32),
                            0x55B0 => {
                                transfer = elements(vv).into_iter().find(|(c, _)| *c == 0x55BA).map(|(_, x)| uint(x));
                            }
                            _ => {}
                        }
                    }
                }
                0xE1 => {
                    if let Some((_, c)) = elements(v).into_iter().find(|(c, _)| *c == 0x9F) {
                        channels = uint(c).max(1) as u32;
                    }
                }
                _ => {}
            }
        }
        let lang = lang_bcp.or(lang).filter(|l| !l.is_empty() && l != "und");
        let codec = mkv_codec(&codec_id);
        let track = Track { index, codec: codec.clone(), lang, title: name, default: Some(default) };
        match kind {
            1 if p.video.is_none() => {
                let mut v = VideoInfo { index, codec, width, height, ..Default::default() };
                match v.codec.as_str() {
                    "h264" => set_avc_profile(&mut v, private.get(1).copied()),
                    "hevc" => set_hevc_profile(&mut v, private.get(1).map(|x| x & 0x1f)),
                    _ => {}
                }
                set_transfer(&mut v, transfer);
                p.video = Some(v);
            }
            2 => p.audio.push(AudioInfo { track, channels }),
            17 => p.subs.push(track),
            _ => {}
        }
    }
}

fn mkv_codec(id: &str) -> String {
    let c = match id {
        "V_MPEG4/ISO/AVC" => "h264",
        "V_MPEGH/ISO/HEVC" => "hevc",
        "V_AV1" => "av1",
        "V_VP9" => "vp9",
        "V_VP8" => "vp8",
        "V_MPEG4/ISO/ASP" | "V_MPEG4/ISO/SP" | "V_MS/VFW/FOURCC" => "mpeg4",
        "V_MPEG2" => "mpeg2video",
        "A_AC3" => "ac3",
        "A_EAC3" => "eac3",
        "A_OPUS" => "opus",
        "A_FLAC" => "flac",
        "A_VORBIS" => "vorbis",
        "A_MPEG/L3" => "mp3",
        "A_TRUEHD" => "truehd",
        "S_TEXT/UTF8" => "subrip",
        "S_TEXT/ASS" | "S_TEXT/SSA" | "S_ASS" | "S_SSA" => "ass",
        "S_TEXT/WEBVTT" => "webvtt",
        "S_HDMV/PGS" => "hdmv_pgs_subtitle",
        "S_VOBSUB" => "dvd_subtitle",
        x if x.starts_with("A_AAC") => "aac",
        x if x.starts_with("A_DTS") => "dts",
        x => return x.to_ascii_lowercase(),
    };
    c.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Arma un elemento EBML con un ID (con marcador) y contenido.
    fn el(id: u64, data: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        let idb = id.to_be_bytes();
        let skip = idb.iter().position(|b| *b != 0).unwrap();
        v.extend_from_slice(&idb[skip..]);
        // Tamaño en 8 bytes (0x01 + 7 bytes).
        v.push(0x01);
        v.extend_from_slice(&(data.len() as u64).to_be_bytes()[1..]);
        v.extend_from_slice(data);
        v
    }

    fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    #[test]
    fn matroska_tracks() {
        let header = el(0x1A45_DFA3, &el(EBML_DOCTYPE, b"matroska"));
        let info = el(INFO, &cat(&[el(TIMESCALE, &[0x0F, 0x42, 0x40]), el(DURATION, &5_400_000f64.to_be_bytes())]));
        let video = el(
            TRACK_ENTRY,
            &cat(&[
                el(0xD7, &[1]),
                el(0x83, &[1]),
                el(0x86, b"V_MPEGH/ISO/HEVC"),
                el(0x63A2, &[1, 2, 0, 0]),
                el(0xE0, &cat(&[el(0xB0, &[0x0F, 0x00]), el(0xBA, &[0x08, 0x70]), el(0x55B0, &el(0x55BA, &[16]))])),
            ]),
        );
        let audio = el(
            TRACK_ENTRY,
            &cat(&[el(0x83, &[2]), el(0x86, b"A_EAC3"), el(0x22B59C, b"spa"), el(0x536E, b"Latino"), el(0xE1, &el(0x9F, &[6]))]),
        );
        let sub = el(TRACK_ENTRY, &cat(&[el(0x83, &[17]), el(0x86, b"S_TEXT/UTF8"), el(0x88, &[0])]));
        let tracks = el(TRACKS, &cat(&[video, audio, sub]));
        let file = cat(&[header, el(SEGMENT, &cat(&[info, tracks, el(CLUSTER, &[0; 16])]))]);
        let p = parse_mkv(&file, "mkv");
        assert_eq!(p.container.as_deref(), Some("mkv"));
        assert!((p.duration.unwrap() - 5400.0).abs() < 0.01);
        let v = p.video.as_ref().unwrap();
        assert_eq!((v.codec.as_str(), v.width, v.height, v.hdr), ("hevc", Some(3840), Some(2160), true));
        assert_eq!(v.pix_fmt.as_deref(), Some("yuv420p10le"));
        assert_eq!(p.audio.len(), 1);
        assert_eq!(p.audio[0].track.codec, "eac3");
        assert_eq!(p.audio[0].track.index, 1);
        assert_eq!(p.audio[0].track.lang.as_deref(), Some("spa"));
        assert_eq!(p.audio[0].channels, 6);
        assert_eq!(p.subs[0].codec, "subrip");
        assert_eq!(p.subs[0].default, Some(false));
        assert!(p.real);
    }

    fn mbox(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = ((data.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn mp4_moov() {
        let mut mvhd = vec![0u8; 100];
        mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
        mvhd[16..20].copy_from_slice(&7_200_000u32.to_be_bytes());
        let mut tkhd = vec![0u8; 84];
        tkhd[3] = 1;
        let mut mdhd = vec![0u8; 24];
        // "eng" empaquetado.
        let lang = ((b'e' - 0x60) as u16) << 10 | ((b'n' - 0x60) as u16) << 5 | (b'g' - 0x60) as u16;
        mdhd[20..22].copy_from_slice(&lang.to_be_bytes());
        let hdlr = |h: &[u8; 4]| {
            let mut x = vec![0u8; 24];
            x[8..12].copy_from_slice(h);
            mbox(b"hdlr", &x)
        };
        let mut vse = vec![0u8; 78];
        vse[24..26].copy_from_slice(&1920u16.to_be_bytes());
        vse[26..28].copy_from_slice(&1080u16.to_be_bytes());
        vse.extend(mbox(b"avcC", &[1, 100, 0, 40]));
        let stsd = |entry: Vec<u8>| {
            let mut s = vec![0, 0, 0, 0, 0, 0, 0, 1];
            s.extend(entry);
            mbox(b"minf", &mbox(b"stbl", &mbox(b"stsd", &s)))
        };
        let vtrak = mbox(b"trak", &cat(&[mbox(b"tkhd", &tkhd), mbox(b"mdia", &cat(&[mbox(b"mdhd", &mdhd), hdlr(b"vide"), stsd(mbox(b"avc1", &vse))]))]));
        let mut ase = vec![0u8; 28];
        ase[16..18].copy_from_slice(&2u16.to_be_bytes());
        let atrak = mbox(b"trak", &cat(&[mbox(b"tkhd", &tkhd), mbox(b"mdia", &cat(&[mbox(b"mdhd", &mdhd), hdlr(b"soun"), stsd(mbox(b"mp4a", &ase))]))]));
        let moov = cat(&[mbox(b"mvhd", &mvhd), vtrak, atrak]);
        let p = parse_moov(&moov);
        assert!((p.duration.unwrap() - 7200.0).abs() < 0.01);
        let v = p.video.as_ref().unwrap();
        assert_eq!((v.codec.as_str(), v.width, v.height), ("h264", Some(1920), Some(1080)));
        assert_eq!(v.pix_fmt.as_deref(), Some("yuv420p"));
        assert_eq!(p.audio[0].track.codec, "aac");
        assert_eq!(p.audio[0].track.lang.as_deref(), Some("eng"));
        assert_eq!(p.audio[0].track.index, 1);
    }

    #[test]
    fn garbage_does_not_panic() {
        for b in [&[][..], &[0x1A, 0x45, 0xDF, 0xA3, 0xFF][..], &[0, 0, 0, 1, b'f', b't', b'y', b'p'][..], &[0xFF; 64][..]] {
            let _ = parse_mkv(b, "mkv");
            let _ = parse_moov(b);
            let _ = mkv_tracks_pos(b);
        }
    }
}
