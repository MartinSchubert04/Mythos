//! Parser de las respuestas de Torrentio. Cada stream trae la info en texto libre:
//!   name:  "Torrentio\n4k DV | HDR"   (con Real-Debrid: "[RD+] Torrentio\n1080p")
//!   title: "<release>\n[<archivo dentro del pack>]\n👤 291 💾 8.91 GB ⚙️ 1337x\n🇬🇧 / 🇮🇹"
//! y `behaviorHints.filename` / `bingeGroup` con más pistas de códec. De ahí sale el `Stream` del
//! contrato (calidad, tamaño, seeders, fuente, tags, idiomas).
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::auth::hex;

/// Lo que se pone en lugar de la clave de Real-Debrid en las URLs que viajan a la UI (la clave
/// nunca sale del server). `/api/play` la vuelve a reemplazar.
pub const RD_PLACEHOLDER: &str = "_RD_";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Quality {
    #[serde(rename = "CAM")]
    Cam,
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "SD")]
    Sd,
    #[serde(rename = "480p")]
    P480,
    #[serde(rename = "720p")]
    P720,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "2160p")]
    P2160,
}

impl Quality {
    pub fn parse_pref(s: &str) -> Option<Self> {
        match s {
            "2160p" => Some(Self::P2160),
            "1080p" => Some(Self::P1080),
            "720p" => Some(Self::P720),
            _ => None,
        }
    }

    /// Escalón de resolución (para comparar contra la calidad preferida).
    pub fn rank(self) -> Option<f64> {
        match self {
            Self::P2160 => Some(4.0),
            Self::P1080 => Some(3.0),
            Self::P720 => Some(2.0),
            Self::P480 => Some(1.0),
            Self::Sd => Some(0.5),
            Self::Unknown | Self::Cam => None,
        }
    }

    fn from_height(h: u32) -> Self {
        match h {
            1500.. => Self::P2160,
            900.. => Self::P1080,
            650.. => Self::P720,
            400.. => Self::P480,
            _ => Self::Sd,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Stream {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_idx: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    pub quality: Quality,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seeders: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub languages: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached: Option<bool>,
    pub score: f64,
    /// Trackers (`sources` de Torrentio sin el prefijo `tracker:`), para pasarle a media.
    #[serde(skip)]
    pub trackers: Vec<String>,
    /// `Some(cacheado)` si es un link de Real-Debrid.
    #[serde(skip)]
    pub debrid: Option<bool>,
    /// Lo que se pudo deducir del texto para estimar si el cliente lo reproduce directo.
    #[serde(skip)]
    pub media: MediaGuess,
    /// El nombre del release está en cirílico (release ruso: el audio por defecto suele ser doblaje).
    #[serde(skip)]
    pub cyrillic: bool,
    /// Años que aparecen en el release o en el nombre del archivo (para detectar otra película
    /// con el mismo nombre: remakes, parodias...).
    #[serde(skip)]
    pub years: Vec<u16>,
    /// Grupo del release en minúsculas (`[Erai-raws] ...`, `...-NTb`; ver `taste::release_group`).
    #[serde(skip)]
    pub group: Option<String>,
    /// `behaviorHints.bingeGroup` de Torrentio: el mismo release en los distintos episodios.
    #[serde(skip)]
    pub binge: Option<String>,
    /// Por qué lo recomienda lo aprendido del usuario (`taste::bonus`): `same_group`,
    /// `liked_group`, `subs:<idioma>`, `dual_audio`. Vacío sin historial.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

/// Números de 4 cifras entre 1900 y 2099 que estén sueltos ("1920x800" no cuenta).
pub fn years_in(text: &str) -> Vec<u16> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= b.len() {
        let w = &b[i..i + 4];
        let before_ok = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        let after_ok = i + 4 == b.len() || !b[i + 4].is_ascii_alphanumeric();
        let century_ok = (w[0] == b'1' && w[1] == b'9') || (w[0] == b'2' && w[1] == b'0');
        if before_ok && after_ok && century_ok && w.iter().all(u8::is_ascii_digit) {
            let y = w.iter().fold(0u16, |acc, d| acc * 10 + (d - b'0') as u16);
            if !out.contains(&y) {
                out.push(y);
            }
            i += 4;
        } else {
            i += 1;
        }
    }
    out
}

/// Contenedor y códecs adivinados del nombre del release/archivo (`None` = no se sabe).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaGuess {
    pub container: Option<&'static str>,
    pub video: Option<&'static str>,
    pub audio: Vec<&'static str>,
}

fn guess_media(hay: &str, file_name: Option<&str>) -> MediaGuess {
    let any = |ns: &[&str]| ns.iter().any(|n| has(hay, n));
    let ext = file_name.and_then(|f| f.rsplit_once('.')).map(|(_, e)| e.to_ascii_lowercase());
    let container = match ext.as_deref() {
        Some("mp4" | "m4v") => Some("mp4"),
        Some("mkv") => Some("mkv"),
        Some("webm") => Some("webm"),
        Some("avi") => Some("avi"),
        Some("ts" | "m2ts") => Some("ts"),
        _ => None,
    };
    let video = if any(&["av1"]) {
        Some("av1")
    } else if any(&["hevc", "x265", "h265", "h.265", "h 265"]) {
        Some("hevc")
    } else if any(&["x264", "h264", "h.264", "h 264", "avc"]) {
        Some("h264")
    } else if any(&["xvid", "divx"]) {
        Some("mpeg4")
    } else if any(&["vc-1", "vc1"]) {
        Some("vc1")
    } else {
        None
    };
    // Los códecs de audio suelen venir pegados a los canales ("DDP5.1", "AAC2.0"), por eso además
    // de la palabra exacta se aceptan esas variantes.
    let glued = |n: &str| ["", "2.0", "5.1", "7.1", "2", "5", "7"].iter().any(|s| has(hay, &format!("{n}{s}")));
    let mut audio = Vec::new();
    if glued("truehd") {
        audio.push("truehd");
    }
    if glued("dts") || any(&["dts-hd", "dts-x", "dtsx"]) {
        audio.push("dts");
    }
    if glued("ddp") || glued("dd+") || glued("eac3") || any(&["e-ac3"]) {
        audio.push("eac3");
    } else if glued("ac3") || glued("dd") {
        audio.push("ac3");
    }
    if glued("aac") || glued("he.aac") {
        audio.push("aac");
    }
    if glued("opus") {
        audio.push("opus");
    }
    if glued("flac") {
        audio.push("flac");
    }
    MediaGuess { container, video, audio }
}

/// Resultado de parsear una respuesta entera.
pub struct Parsed {
    pub streams: Vec<Stream>,
    /// Torrentio avisa los errores de Real-Debrid (clave inválida, etc.) como un stream falso.
    pub debrid_error: Option<String>,
}

pub fn parse_response(v: &Value, rd_key: Option<&str>) -> Parsed {
    let mut streams = Vec::new();
    let mut debrid_error = None;
    for raw in v["streams"].as_array().into_iter().flatten() {
        let name = raw["name"].as_str().unwrap_or_default();
        if name.contains("RD error") || (raw["url"].as_str().is_some_and(|u| u.contains("/videos/")) && raw["infoHash"].is_null()) {
            debrid_error = Some(raw["title"].as_str().unwrap_or("Real-Debrid error").replace('\n', " "));
            continue;
        }
        // Torrentio a veces repite el mismo torrent (mismo hash y archivo): la UI usa la key como
        // clave de la lista y con dos iguales no la puede dibujar. Queda el primero.
        if let Some(s) = parse_stream(raw, rd_key).filter(|s| !streams.iter().any(|x: &Stream| x.key == s.key)) {
            streams.push(s);
        }
    }
    Parsed { streams, debrid_error }
}

pub fn parse_stream(raw: &Value, rd_key: Option<&str>) -> Option<Stream> {
    let name = raw["name"].as_str().unwrap_or_default();
    let title = raw["title"].as_str().or_else(|| raw["description"].as_str()).unwrap_or_default();
    let hints = &raw["behaviorHints"];

    // --- name: marcador de debrid + "calidad [| tags]"
    let mut name_lines = name.lines();
    let head = name_lines.next().unwrap_or_default();
    let debrid = if head.contains("[RD+]") {
        Some(true)
    } else if head.contains("[RD") {
        Some(false)
    } else {
        None
    };
    let qline = name_lines.collect::<Vec<_>>().join(" ");
    let mut qparts = qline.split('|').map(str::trim);
    let first = qparts.next().unwrap_or_default();
    let (qword, qrest) = first.split_once(' ').unwrap_or((first, ""));
    let name_tags: Vec<&str> = std::iter::once(qrest.trim()).chain(qparts).filter(|t| !t.is_empty()).collect();

    // --- title: release, archivo, estadísticas, idiomas
    let lines: Vec<&str> = title.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let stats_idx = lines.iter().position(|l| l.contains('👤') || l.contains('💾'));
    let release = lines.first().copied().unwrap_or(name).to_string();
    let stats = stats_idx.map(|i| lines[i]).unwrap_or_default();
    let inner_file = match stats_idx {
        Some(i) if i > 1 => Some(lines[1..i].join(" ")),
        _ => None,
    };
    let languages: Vec<String> = stats_idx
        .map(|i| &lines[i + 1..])
        .unwrap_or_default()
        .iter()
        .flat_map(|l| l.split('/'))
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| match l {
            "Multi Audio" => "multi".to_string(),
            "Dual Audio" => "dual".to_string(),
            "Multi Subs" => "multi-subs".to_string(),
            other => other.to_string(),
        })
        .collect();
    let file_name = hints["filename"]
        .as_str()
        .map(str::to_string)
        .or_else(|| inner_file.as_deref().map(|f| f.rsplit('/').next().unwrap_or(f).to_string()));

    // --- identificadores
    let info_hash = raw["infoHash"]
        .as_str()
        .map(str::to_ascii_lowercase)
        .or_else(|| raw["url"].as_str().and_then(hash_in_url))
        .filter(|h| is_info_hash(h));
    let file_idx = raw["fileIdx"].as_u64().map(|i| i as u32);
    let url = raw["url"].as_str().map(|u| match rd_key {
        Some(k) if !k.is_empty() => u.replace(k, RD_PLACEHOLDER),
        _ => u.to_string(),
    });
    let key = match (&url, &info_hash) {
        (Some(u), _) => format!("url:{}", &hex(&Sha256::digest(u.as_bytes()))[..16]),
        (None, Some(h)) => match file_idx {
            Some(i) => format!("{h}:{i}"),
            None => h.clone(),
        },
        (None, None) => return None,
    };
    let trackers = raw["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str()?.strip_prefix("tracker:").map(str::to_string))
        .collect();

    // --- calidad y tags: se busca en todo el texto disponible
    let hay = format!(
        " {} {} {} {} ",
        release,
        inner_file.as_deref().unwrap_or_default(),
        file_name.as_deref().unwrap_or_default(),
        hints["bingeGroup"].as_str().unwrap_or_default().replace('|', " "),
    )
    .to_ascii_lowercase();
    let quality = quality_of(qword, &hay);
    let media = guess_media(&hay, file_name.as_deref());
    let cyrillic = release.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
    let years = years_in(&format!(
        "{release} {} {}",
        inner_file.as_deref().unwrap_or_default(),
        file_name.as_deref().unwrap_or_default()
    ));
    let mut tags = Vec::new();
    for t in &name_tags {
        let t = if t.contains("3D") { "3D" } else { t };
        push_unique(&mut tags, t);
    }
    tags_from_text(&hay, &mut tags);

    let group = crate::taste::release_group(&release).or_else(|| file_name.as_deref().and_then(crate::taste::release_group));
    Some(Stream {
        key,
        info_hash,
        file_idx,
        url,
        title: release,
        file_name,
        quality,
        size_bytes: parse_size(stats),
        seeders: after(stats, '👤').and_then(|s| s.split_whitespace().next()?.parse().ok()),
        source: after(stats, '⚙').map(|s| s.trim_start_matches('\u{fe0f}').trim().to_string()).filter(|s| !s.is_empty()),
        tags,
        languages,
        cached: debrid.filter(|c| *c),
        score: 0.0,
        trackers,
        debrid,
        media,
        cyrillic,
        years,
        group,
        binge: hints["bingeGroup"].as_str().map(str::trim).filter(|b| b.matches('|').count() >= 2).map(str::to_string),
        reasons: Vec::new(),
    })
}

fn after(s: &str, marker: char) -> Option<&str> {
    let i = s.find(marker)?;
    let rest = &s[i + marker.len_utf8()..];
    // Corta en el próximo marcador conocido.
    let end = rest.find(['👤', '💾', '⚙']).unwrap_or(rest.len());
    Some(rest[..end].trim())
}

fn parse_size(stats: &str) -> Option<u64> {
    let s = after(stats, '💾')?;
    let mut it = s.split_whitespace();
    let n: f64 = it.next()?.replace(',', ".").parse().ok()?;
    let mult = match it.next()?.to_ascii_uppercase().as_str() {
        "B" => 1.0,
        "KB" => 1024.0,
        "MB" => 1024.0 * 1024.0,
        "GB" => 1024.0 * 1024.0 * 1024.0,
        "TB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((n * mult) as u64)
}

pub fn is_info_hash(h: &str) -> bool {
    h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit())
}

fn hash_in_url(u: &str) -> Option<String> {
    u.split('/').find(|s| is_info_hash(s)).map(str::to_ascii_lowercase)
}

/// `needle` aparece en `hay` como palabra (sin letras/dígitos pegados a los costados).
fn has(hay: &str, needle: &str) -> bool {
    let b = hay.as_bytes();
    let mut from = 0;
    while let Some(pos) = hay[from..].find(needle) {
        let start = from + pos;
        let end = start + needle.len();
        let before_ok = start == 0 || !b[start - 1].is_ascii_alphanumeric();
        let after_ok = end >= b.len() || !b[end].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// Canales de audio ("5.1", "DDP5.1", "DD 5 1"): solo exige que no haya dígitos pegados.
fn has_channels(hay: &str, ch: &str) -> bool {
    let b = hay.as_bytes();
    [ch.to_string(), ch.replace('.', " ")].iter().any(|n| {
        hay.match_indices(n.as_str()).any(|(i, _)| {
            let end = i + n.len();
            (i == 0 || !b[i - 1].is_ascii_digit()) && (end >= b.len() || !b[end].is_ascii_digit())
        })
    })
}

fn quality_of(qword: &str, hay: &str) -> Quality {
    let q = qword.to_ascii_lowercase();
    if q == "4k" {
        return Quality::P2160;
    }
    if let Some(h) = q.strip_suffix('p').and_then(|n| n.parse::<u32>().ok()) {
        return Quality::from_height(h);
    }
    if matches!(q.as_str(), "cam" | "telesync" | "telecine" | "scr" | "screener" | "ts" | "tc")
        || ["camrip", "hdcam", "hdts", "telesync", "telecine", "hdtc"].iter().any(|n| has(hay, n))
    {
        return Quality::Cam;
    }
    // "BDRip", "HDTV", "DVD", etc.: si el texto dice la resolución se usa, si no es SD.
    let from_text = if has(hay, "2160p") || has(hay, "4k") || has(hay, "uhd") {
        Some(Quality::P2160)
    } else if has(hay, "1080p") || has(hay, "1080i") {
        Some(Quality::P1080)
    } else if has(hay, "720p") {
        Some(Quality::P720)
    } else if has(hay, "480p") || has(hay, "576p") {
        Some(Quality::P480)
    } else {
        None
    };
    match (from_text, q.is_empty() || q == "torrentio") {
        (Some(x), _) => x,
        (None, false) => Quality::Sd,
        (None, true) => Quality::Unknown,
    }
}

fn push_unique(tags: &mut Vec<String>, t: &str) {
    if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
        tags.push(t.to_string());
    }
}

fn tags_from_text(hay: &str, tags: &mut Vec<String>) {
    let any = |ns: &[&str]| ns.iter().any(|n| has(hay, n));
    if any(&["hdr10+", "hdr10plus"]) || hay.contains("hdr10+") {
        push_unique(tags, "HDR10+");
    }
    if any(&["hdr", "hdr10", "hdr10+", "hdr10plus"]) || hay.contains("hdr10+") {
        push_unique(tags, "HDR");
    }
    if any(&["dv", "dovi", "dolby vision", "dolby.vision"]) {
        push_unique(tags, "DV");
    }
    if any(&["hevc", "x265", "h265", "h.265", "h 265"]) {
        push_unique(tags, "HEVC");
    }
    if any(&["x265"]) {
        push_unique(tags, "x265");
    }
    if any(&["av1"]) {
        push_unique(tags, "AV1");
    }
    if any(&["x264", "h264", "h.264", "h 264", "avc"]) {
        push_unique(tags, "x264");
    }
    if any(&["10bit", "10-bit", "10 bit"]) {
        push_unique(tags, "10bit");
    }
    if any(&["remux", "bdremux"]) {
        push_unique(tags, "REMUX");
    }
    if any(&["web-dl", "webdl", "web dl"]) {
        push_unique(tags, "WEB-DL");
    } else if any(&["webrip", "web-rip", "web"]) {
        push_unique(tags, "WEBRip");
    }
    if any(&["bluray", "blu-ray", "bdrip", "brrip", "bdremux"]) {
        push_unique(tags, "BluRay");
    }
    if any(&["atmos"]) {
        push_unique(tags, "Atmos");
    }
    if has_channels(hay, "7.1") {
        push_unique(tags, "7.1");
    } else if has_channels(hay, "5.1") || any(&["6ch"]) {
        push_unique(tags, "5.1");
    }
    if any(&["3d", "sbs", "half-sbs", "hsbs"]) {
        push_unique(tags, "3D");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn tag(s: &Stream, t: &str) -> bool {
        s.tags.iter().any(|x| x == t)
    }

    #[test]
    fn repeated_torrent_kept_once() {
        // Así llegó "Demon Slayer: Infinity Castle" (tt32820897): el mismo torrent dos veces.
        let raw = r#"{"streams":[
          {"name":"Torrentio\n1080p","title":"[Cytox] Infinity Castle 2025 REPACK 1080p\n👤 40 💾 3 GB ⚙️ NyaaSi","infoHash":"9ede7c67f9ab4860aed64aa64f288fe20d10d151","fileIdx":0},
          {"name":"Torrentio\n1080p","title":"[Cytox] Infinity Castle 2025 REPACK 1080p\n👤 12 💾 3 GB ⚙️ TorrentGalaxy","infoHash":"9EDE7C67F9AB4860AED64AA64F288FE20D10D151","fileIdx":0},
          {"name":"Torrentio\n1080p","title":"Otro\n👤 5 💾 2 GB","infoHash":"9ede7c67f9ab4860aed64aa64f288fe20d10d151","fileIdx":1}
        ]}"#;
        let p = parse_response(&serde_json::from_str(raw).unwrap(), None);
        let keys: Vec<&str> = p.streams.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["9ede7c67f9ab4860aed64aa64f288fe20d10d151:0", "9ede7c67f9ab4860aed64aa64f288fe20d10d151:1"]);
        assert_eq!(p.streams[0].seeders, Some(40));
    }

    #[test]
    fn inception_movie() {
        let p = parse_response(&fixture("torrentio_tt1375666.json"), None);
        assert!(p.debrid_error.is_none());
        assert_eq!(p.streams.len(), 10);
        let s = &p.streams;

        // 4K AV1 HDR con banderas.
        assert_eq!(s[0].key, "2849211f150e966ef7924f43aec22727a644e10d:0");
        assert_eq!(s[0].quality, Quality::P2160);
        assert_eq!(s[0].seeders, Some(291));
        assert_eq!(s[0].size_bytes, Some((8.91 * 1073741824.0) as u64));
        assert_eq!(s[0].source.as_deref(), Some("1337x"));
        assert_eq!(s[0].languages, ["🇬🇧", "🇮🇹", "🇪🇸", "🇫🇷", "🇩🇪"]);
        assert!(tag(&s[0], "HDR") && tag(&s[0], "AV1") && tag(&s[0], "5.1") && tag(&s[0], "10bit"));
        assert_eq!(s[0].file_name.as_deref(), Some("Inception.2010.PROPER.Bluray.2160p.AV1.HDR10.OPUS.5.1-UH.mkv"));

        // Release ruso con trackers en `sources`.
        assert!(tag(&s[1], "DV") && tag(&s[1], "REMUX"));
        assert_eq!(s[1].trackers.len(), 3); // el 4º es "dht:..."
        assert!(s[1].trackers[0].starts_with("udp://"));

        // YTS: el códec solo viene en bingeGroup.
        assert_eq!(s[2].source.as_deref(), Some("YTS"));
        assert!(tag(&s[2], "x265") && tag(&s[2], "HEVC"));

        // Sin fileIdx: la key es solo el hash.
        assert_eq!(s[3].file_idx, None);
        assert_eq!(s[3].key, "a75b82ad8477187109f80e7d3fee845923b78d7c");

        // YIFY 1080p x264 en mp4: alcanza para saber que va directo.
        assert_eq!(s[4].media, MediaGuess { container: Some("mp4"), video: Some("h264"), audio: vec![] });
        assert_eq!(s[0].media.video, Some("av1"));
        assert_eq!(s[0].media.audio, ["opus"]);
        assert!(s[1].cyrillic && !s[0].cyrillic);
        assert_eq!(s[1].media.audio, ["dts"]);
        assert_eq!(s[4].quality, Quality::P1080);
        assert_eq!(s[4].seeders, Some(890));
        assert!(tag(&s[4], "x264"));
        assert!(!tag(&s[4], "HEVC"));

        // Remux 1080p.
        assert!(tag(&s[5], "REMUX"));

        // Pack: el título es el nombre del pack y el archivo sale de la segunda línea.
        assert_eq!(s[6].title, "41 Movies To Watch While Stoned (all High IMDB) jZQ");
        assert_eq!(s[6].file_idx, Some(107));
        assert_eq!(s[6].file_name.as_deref(), Some("Inception.2010.1080p.BluRay.x265-RARBG.mp4"));

        assert_eq!(s[7].quality, Quality::P720);
        // "HDRip" sin resolución en el texto → SD.
        assert_eq!(s[8].quality, Quality::Sd);
        assert_eq!(s[8].languages, ["🇷🇺"]);
        // DVD de un pack español.
        assert_eq!(s[9].quality, Quality::Sd);
        assert_eq!(s[9].source.as_deref(), Some("Wolfmax4k"));
        assert_eq!(s[9].languages, ["🇪🇸"]);
    }

    #[test]
    fn got_episode() {
        let p = parse_response(&fixture("torrentio_tt0944947_1_1.json"), None);
        let s = &p.streams;
        assert_eq!(s.len(), 6);
        assert_eq!(s[0].quality, Quality::P2160);
        assert_eq!(s[0].file_name.as_deref(), Some("Игра престолов.S01E01.WEB-DL.2160p.mkv"));
        assert_eq!(s[0].size_bytes, Some((9.52 * 1073741824.0) as u64));
        assert!(tag(&s[0], "DV") && tag(&s[0], "WEB-DL"));
        assert_eq!(s[1].key, "30eaa43096f11a553ce182f97963e5384a4ac781:0");
        assert_eq!(s[1].file_name.as_deref(), Some("Game of Thrones S01E01 Winter is Coming.mkv"));
        assert!(tag(&s[1], "HDR") && tag(&s[1], "x265") && tag(&s[1], "10bit") && tag(&s[1], "5.1"));
        assert_eq!(s[2].size_bytes, Some((984.22 * 1048576.0) as u64));
        assert_eq!(s[3].languages, ["🇬🇧", "🇵🇹"]);
        assert!(tag(&s[3], "x264") && tag(&s[3], "WEB-DL"));
        // "HDTV" sin resolución → SD; con 720p en el título → 720p.
        assert_eq!(s[4].quality, Quality::P720);
        assert_eq!(s[5].quality, Quality::Sd);
    }

    #[test]
    fn real_debrid_stream() {
        let key = "ABCDEF123456";
        let v: Value = serde_json::from_str(&format!(
            r#"{{"streams":[
              {{"name":"[RD+] Torrentio\n1080p","title":"Inception.2010.1080p.BluRay.x264-REFiNED\n👤 10 💾 10.93 GB ⚙️ RARBG",
                "url":"https://torrentio.strem.fun/resolve/realdebrid/{key}/c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00/null/2/Inception.mkv",
                "behaviorHints":{{"bingeGroup":"torrentio|1080p|BluRay|x264","filename":"Inception.mkv"}}}},
              {{"name":"[RD download] Torrentio\n720p","title":"Inception 720p\n👤 3 💾 1 GB ⚙️ YTS",
                "url":"https://torrentio.strem.fun/resolve/realdebrid/{key}/c0ffee00c0ffee00c0ffee00c0ffee00c0ffee01/null/0/x.mkv"}}
            ]}}"#
        ))
        .unwrap();
        let p = parse_response(&v, Some(key));
        assert_eq!(p.streams.len(), 2);
        let s = &p.streams[0];
        assert!(s.key.starts_with("url:"));
        assert_eq!(s.cached, Some(true));
        assert_eq!(s.debrid, Some(true));
        assert_eq!(s.info_hash.as_deref(), Some("c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00"));
        let u = s.url.as_deref().unwrap();
        assert!(!u.contains(key), "la clave de RD no puede salir del server");
        assert!(u.contains(RD_PLACEHOLDER));
        assert_eq!(p.streams[1].cached, None);
        assert_eq!(p.streams[1].debrid, Some(false));
    }

    #[test]
    fn debrid_error_detected() {
        let v: Value = serde_json::from_str(
            r#"{"streams":[{"name":"Torrentio\nRD error","title":"Invalid RealDebrid ApiKey/Token!","url":"https://torrentio.strem.fun/videos/failed_access_v3.mp4"}]}"#,
        )
        .unwrap();
        let p = parse_response(&v, Some("bad"));
        assert!(p.streams.is_empty());
        assert_eq!(p.debrid_error.as_deref(), Some("Invalid RealDebrid ApiKey/Token!"));
    }

    #[test]
    fn years() {
        assert_eq!(years_in("Night of the Living Dead 1968 1080p BluRay"), [1968]);
        assert_eq!(years_in("La noche de los muertos vivientes Estallido zombi (2026) [Bluray 720p][Esp]"), [2026]);
        assert_eq!(years_in("Filmografi 2002 - 2021 Inception[2010]DvDrip"), [2002, 2021, 2010]);
        assert!(years_in("1920x800 2160p 1080p x2010 20100").is_empty());
        let p = parse_response(&fixture("torrentio_tt1375666.json"), None);
        // Pack sin año en el nombre: el año sale del archivo de adentro.
        assert_eq!(p.streams[6].years, [2010]);
    }

    #[test]
    fn word_boundaries() {
        assert!(has(" dts-hd.ma.5.1-x ", "dts-hd"));
        assert!(!has(" hdr10 ", "hdr"));
        assert!(has(" x265-e ", "x265"));
        assert!(has_channels(" ddp5.1 ", "5.1"));
        assert!(has_channels(" dd 5 1 x264 ", "5.1"));
        assert!(!has_channels(" s05 1080p ", "5.1"));
    }
}
