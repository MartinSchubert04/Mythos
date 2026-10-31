//! Utilidades puras y chicas: parser de `Range`, tipo MIME por extensión, fechas RFC 3339,
//! percent-encoding mínimo e ids aleatorios.
//!
//! Por qué a mano: son pocas líneas y evitan sumar crates (chrono, mime, percent-encoding) al
//! binario; prioridad 2 del usuario es que todo sea liviano.

use std::time::{SystemTime, UNIX_EPOCH};

/// Rango de bytes pedido, ya resuelto contra el tamaño del archivo (fin inclusivo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }
}

/// Resultado de interpretar el header `Range`.
#[derive(Debug, PartialEq, Eq)]
pub enum RangeReq {
    /// Sin header (o uno que ignoramos, p. ej. multi-rango): 200 con el archivo entero.
    Full,
    /// 206 con este rango.
    Partial(ByteRange),
    /// 416: el rango no se puede satisfacer.
    Unsatisfiable,
}

/// Interpreta `Range: bytes=...` según RFC 9110. Multi-rango no lo pide ningún reproductor, así
/// que se trata como "archivo entero" (válido según la RFC).
pub fn parse_range(header: Option<&str>, size: u64) -> RangeReq {
    let Some(h) = header else { return RangeReq::Full };
    let h = h.trim();
    let Some(spec) = h.strip_prefix("bytes=") else { return RangeReq::Full };
    if spec.contains(',') {
        return RangeReq::Full;
    }
    let Some((a, b)) = spec.split_once('-') else { return RangeReq::Full };
    let (a, b) = (a.trim(), b.trim());
    if size == 0 {
        return RangeReq::Unsatisfiable;
    }
    if a.is_empty() {
        // Sufijo: los últimos N bytes.
        let Ok(n) = b.parse::<u64>() else { return RangeReq::Full };
        if n == 0 {
            return RangeReq::Unsatisfiable;
        }
        let n = n.min(size);
        return RangeReq::Partial(ByteRange { start: size - n, end: size - 1 });
    }
    let Ok(start) = a.parse::<u64>() else { return RangeReq::Full };
    if start >= size {
        return RangeReq::Unsatisfiable;
    }
    let end = if b.is_empty() {
        size - 1
    } else {
        match b.parse::<u64>() {
            Ok(e) if e >= start => e.min(size - 1),
            Ok(_) => return RangeReq::Full,
            Err(_) => return RangeReq::Full,
        }
    };
    RangeReq::Partial(ByteRange { start, end })
}

/// Content-Type por extensión (solo lo que puede aparecer en un torrent de video).
pub fn mime_for(name: &str) -> &'static str {
    match ext_of(name).as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "ts" | "m2ts" | "mts" => "video/mp2t",
        "wmv" => "video/x-ms-wmv",
        "flv" => "video/x-flv",
        "mpg" | "mpeg" => "video/mpeg",
        "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "srt" => "application/x-subrip",
        "vtt" => "text/vtt",
        "m3u8" => "application/vnd.apple.mpegurl",
        "m4s" => "video/iso.segment",
        _ => "application/octet-stream",
    }
}

pub fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

pub const VIDEO_EXTS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "webm", "mov", "ts", "m2ts", "mts", "wmv", "flv", "mpg", "mpeg",
    "ogv",
];

pub fn is_video_name(name: &str) -> bool {
    VIDEO_EXTS.contains(&ext_of(name).as_str())
}

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Segundos Unix → `YYYY-MM-DDTHH:MM:SSZ` (algoritmo "days from civil" de H. Hinnant).
pub fn rfc3339(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

/// Percent-encoding para meter URLs de trackers dentro de un magnet.
pub fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decodifica `%XX` (para sacar el nombre de archivo de una URL).
pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 128 bits aleatorios en hex (url-safe).
pub fn random_id() -> String {
    let v: u128 = rand::random();
    format!("{v:032x}")
}

/// `info_hash` válido: 40 hex. Se normaliza a minúsculas.
pub fn normalize_hash(h: &str) -> Option<String> {
    let h = h.trim().to_ascii_lowercase();
    (h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit())).then_some(h)
}

/// `tokio::process::Command` para ffmpeg/ffprobe. En Windows, sin ventana de consola: embebido en
/// la app de escritorio (que no tiene consola propia) cada proceso abriría una ventana negra.
pub fn tool_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_basic() {
        assert_eq!(parse_range(None, 100), RangeReq::Full);
        assert_eq!(parse_range(Some("bytes=0-"), 100), RangeReq::Partial(ByteRange { start: 0, end: 99 }));
        assert_eq!(parse_range(Some("bytes=10-19"), 100), RangeReq::Partial(ByteRange { start: 10, end: 19 }));
        assert_eq!(parse_range(Some("bytes=10-1000"), 100), RangeReq::Partial(ByteRange { start: 10, end: 99 }));
        assert_eq!(parse_range(Some("bytes=-10"), 100), RangeReq::Partial(ByteRange { start: 90, end: 99 }));
        assert_eq!(parse_range(Some("bytes=-1000"), 100), RangeReq::Partial(ByteRange { start: 0, end: 99 }));
        assert_eq!(parse_range(Some("bytes=100-"), 100), RangeReq::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 100), RangeReq::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=20-10"), 100), RangeReq::Full);
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), RangeReq::Full);
        assert_eq!(parse_range(Some("items=0-1"), 100), RangeReq::Full);
        assert_eq!(parse_range(Some("bytes=abc"), 100), RangeReq::Full);
        assert_eq!(parse_range(Some("bytes=0-"), 0), RangeReq::Unsatisfiable);
        assert_eq!(ByteRange { start: 10, end: 19 }.len(), 10);
    }

    #[test]
    fn dates() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn pct() {
        assert_eq!(pct_encode("udp://a.b:80/announce"), "udp%3A%2F%2Fa.b%3A80%2Fannounce");
        assert_eq!(pct_decode("Big%20Buck%20Bunny.mp4"), "Big Buck Bunny.mp4");
        assert_eq!(pct_decode("a%2"), "a%2");
        assert_eq!(pct_decode("100%"), "100%");
    }

    #[test]
    fn misc() {
        assert_eq!(mime_for("A.MKV"), "video/x-matroska");
        assert!(is_video_name("x.mp4"));
        assert!(!is_video_name("x.nfo"));
        assert_eq!(normalize_hash(" DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C ").as_deref(), Some("dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c"));
        assert_eq!(normalize_hash("xyz"), None);
        assert_eq!(random_id().len(), 32);
    }
}
