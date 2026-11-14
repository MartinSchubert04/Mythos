//! Score "anti-trabas" de los streams: decide qué stream se ofrece primero.
//!
//! La prioridad número uno del usuario es que la película no se trabe, así que el score premia lo
//! que hace que un torrent baje rápido y se reproduzca sin convertir, y castiga lo que lo arriesga:
//!
//! 1. **Ya descargado en el server** (+600) o **en caché de Real-Debrid** (+400): arranca al
//!    instante y no depende de peers. Un link de RD que *no* está en caché (−200) obliga a RD a
//!    bajarlo primero, así que no sirve para ver ya.
//! 2. **Seeders**, en escala logarítmica (90·log10(s+1): 10 → 94, 100 → 180, 1000 → 270): de 5 a
//!    50 seeders la diferencia es enorme, de 500 a 1000 casi no importa. Con menos de 5 seeders la
//!    descarga suele no alcanzar al bitrate: penalización fuerte (−180, y −350 con 0).
//! 3. **Calidad**: la preferida del dispositivo (`?quality=`; si no, la de la cuenta o 1080p) suma
//!    100. Por debajo pierde 120 por escalón: con 45 un 1080p con el doble de seeders y que va
//!    directo le ganaba a un 4K sano y la tele con 4K preferido arrancaba en 1080p. Por encima
//!    pierde 70 por escalón porque "pasarse" significa archivos más grandes, más bitrate y más
//!    chances de tener que transcodificar (4K HEVC/HDR en un navegador).
//! 4. **Bitrate estimado** = tamaño / duración (la de Cinemeta, o 2 h / 45 min si no se sabe). Un
//!    buen encode necesita ~25 Mb/s en 4K, 10 en 1080p, 5 en 720p y 2,5 en SD; cada 100 % de exceso
//!    resta 100 (hasta −320): un remux de 80 GB necesita 70 Mb/s sostenidos del enjambre, y uno de
//!    20 GB en 1080p, el doble que un encode normal. Los archivos ridículamente chicos para su
//!    calidad también restan (suelen ser fakes o ilegibles).
//! 5. **Idioma**: Torrentio marca con banderas los idiomas que no son el original. Si las banderas
//!    incluyen inglés o español (latino suma un poco más) suma; si *solo* hay otros idiomas es casi
//!    seguro un doblaje (−140, −50 si además dice multi/dual). Un release con nombre en cirílico
//!    suele tener el doblaje ruso como pista por defecto (−60).
//! 6. **Compatibilidad**: si la UI manda `caps` (lo que su `<video>` reproduce), se estima si el
//!    stream va directo (+45), con remux barato (+10) o con transcodificación de video (−45, −90
//!    si es 4K o HDR, que cuesta mucho más en vivo). Sin `caps` se usa una regla general: x264
//!    suma, AV1 resta. Dolby Vision sin capa HDR resta (colores rotos o tone-mapping), 3D resta
//!    mucho y REMUX resta (además del bitrate).
//! 7. **CAM/TS**: −500. **Más grande que `max_size_gb`**: −1000 (queda al fondo pero visible por si
//!    es lo único que hay).
//! 8. **Otra película con el mismo nombre** (remake, parodia): si el release o su archivo traen
//!    años y ninguno cae en el de Cinemeta (±1; en series, su rango), −600. No se descarta del
//!    todo por si el año del release está mal, pero queda al fondo.
//! 9. **Lo aprendido del usuario** (`taste.rs`): el grupo que eligió en esta serie, los que repite,
//!    los subtítulos que busca y el audio dual, acotado entre −60 y +110: la salud del torrent
//!    sigue mandando (un grupo preferido casi muerto no le gana a uno sano).
use mythos_media::ClientCaps;

use crate::taste::{self, Personal};
use crate::torrentio::{Quality, Stream};

#[derive(Clone, Debug)]
pub struct Prefs {
    pub preferred: Quality,
    pub max_size_bytes: Option<u64>,
    /// Duración estimada del contenido en segundos.
    pub duration_s: f64,
    /// Lo que reproduce el cliente, si lo mandó (`?caps=`).
    pub caps: Option<ClientCaps>,
    /// Años aceptables según Cinemeta (película: año ±1; serie: su rango ±1).
    pub years: Option<(u16, u16)>,
    /// El server no tiene ffmpeg (modo local sin ffmpeg): lo que no se reproduce directo no se
    /// puede convertir, así que se hunde en la lista.
    pub direct_only: bool,
    /// Lo aprendido de las reproducciones del usuario (`taste.rs`): un bonus acotado.
    pub personal: Option<Personal>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { preferred: Quality::P1080, max_size_bytes: None, duration_s: 7200.0, caps: None, years: None, direct_only: false, personal: None }
    }
}

/// Lo que pierde cada escalón por debajo de la calidad preferida.
const BELOW_PREFERRED: f64 = 120.0;

/// Bitrate de un buen encode (Mb/s) para cada calidad.
fn sane_mbps(q: Quality) -> f64 {
    match q {
        Quality::P2160 => 25.0,
        Quality::P1080 | Quality::Unknown => 10.0,
        Quality::P720 => 5.0,
        Quality::P480 | Quality::Sd | Quality::Cam => 2.5,
    }
}

/// Cómo lo reproduciría el cliente.
#[derive(Debug, PartialEq)]
pub enum Playback {
    Direct,
    Remux,
    Transcode,
    Unknown,
}

/// Estimación a partir del contenedor y los códecs deducidos del nombre. Lo desconocido se asume
/// favorable (lo más común es h264 + aac/ac3) pero sin llegar a "directo" seguro.
pub fn playback(s: &Stream, caps: &ClientCaps) -> Playback {
    let m = &s.media;
    let video_ok = match m.video {
        Some("h264") => Some(caps.h264),
        Some("hevc") => Some(caps.hevc),
        Some("av1") => Some(caps.av1),
        Some(_) => Some(false), // xvid, vc-1...
        None => None,
    };
    let container_ok = match m.container {
        Some("mp4") => Some(true),
        Some("mkv") => Some(caps.mkv),
        Some("webm") => Some(caps.vp9 || caps.av1),
        Some(_) => Some(false),
        None => None,
    };
    let audio_ok = if m.audio.is_empty() {
        None
    } else {
        Some(m.audio.iter().any(|a| match *a {
            "aac" => caps.aac,
            "ac3" => caps.ac3,
            "eac3" => caps.eac3,
            "dts" => caps.dts,
            "opus" => caps.opus,
            "flac" => caps.flac,
            _ => false,
        }))
    };
    match (video_ok, container_ok, audio_ok) {
        (Some(false), _, _) => Playback::Transcode,
        (Some(true), Some(true), Some(true)) => Playback::Direct,
        (Some(true), Some(true), None) if caps.aac => Playback::Direct,
        (Some(true), _, _) => Playback::Remux,
        (None, _, _) => Playback::Unknown,
    }
}

fn is_en_or_es(flag: &str) -> bool {
    matches!(flag, "🇬🇧" | "🇺🇸" | "🇪🇸" | "🇲🇽" | "🇦🇷")
}

fn language_score(s: &Stream) -> f64 {
    let flags: Vec<&str> = s.languages.iter().map(String::as_str).filter(|l| !l.is_ascii()).collect();
    let multi = s.languages.iter().any(|l| l == "multi" || l == "dual");
    let mut score = if flags.is_empty() {
        0.0 // sin banderas: idioma original (casi siempre inglés en Cinemeta)
    } else if flags.iter().any(|f| is_en_or_es(f)) {
        10.0 + if flags.contains(&"🇲🇽") { 10.0 } else { 0.0 }
    } else if multi {
        -50.0
    } else {
        -140.0
    };
    if s.cyrillic {
        score -= 60.0;
    }
    score
}

/// El score con lo aprendido del usuario (9.) y los motivos para la UI.
pub fn score_with(s: &Stream, downloaded: bool, p: &Prefs) -> (f64, Vec<String>) {
    let base = score(s, downloaded, p);
    match &p.personal {
        Some(me) => {
            let (b, why) = taste::bonus(s, me);
            (((base + b) * 10.0).round() / 10.0, why)
        }
        None => (base, Vec::new()),
    }
}

/// El score sin lo aprendido del usuario (1. a 8.).
pub fn score(s: &Stream, downloaded: bool, p: &Prefs) -> f64 {
    let has = |t: &str| s.tags.iter().any(|x| x == t);
    let mut score = 0.0;

    // 1. Disponibilidad inmediata.
    if downloaded {
        score += 600.0;
    }
    match s.debrid {
        Some(true) => score += 400.0,
        Some(false) => score -= 200.0,
        None => {}
    }

    // 2. Seeders (no importan si ya está todo bajado o si lo sirve RD desde su caché).
    if !downloaded && s.debrid != Some(true) {
        let seeds = s.seeders.unwrap_or(0) as f64;
        score += 90.0 * (seeds + 1.0).log10();
        score -= match s.seeders {
            None | Some(0) => 350.0,
            Some(1..=4) => 180.0,
            Some(5..=14) => 40.0,
            _ => 0.0,
        };
    }

    // 3. Calidad contra la preferida.
    match (s.quality.rank(), p.preferred.rank()) {
        (Some(q), Some(pref)) if q > pref => score += 100.0 - 70.0 * (q - pref),
        (Some(q), Some(pref)) => score += 100.0 - BELOW_PREFERRED * (pref - q),
        _ if s.quality == Quality::Cam => score -= 500.0,
        _ => score += 20.0, // calidad desconocida
    }

    // 4. Bitrate estimado.
    if let Some(size) = s.size_bytes {
        let mbps = size as f64 * 8.0 / p.duration_s.max(600.0) / 1e6;
        let sane = sane_mbps(s.quality);
        if mbps > sane {
            score -= (100.0 * (mbps / sane - 1.0)).min(320.0);
        } else if mbps < sane / 10.0 {
            score -= 40.0;
        }
        if p.max_size_bytes.is_some_and(|max| size > max) {
            score -= 1000.0;
        }
    }

    // 5. Idioma.
    score += language_score(s);

    // 6. Compatibilidad / riesgo de transcodificar.
    match &p.caps {
        Some(caps) => {
            score += match playback(s, caps) {
                // Sin ffmpeg: remux/transcode no existen; desconocido es un riesgo.
                Playback::Remux if p.direct_only => -250.0,
                Playback::Transcode if p.direct_only => -400.0,
                Playback::Unknown if p.direct_only => -30.0,
                Playback::Direct => 45.0,
                Playback::Remux => 10.0,
                Playback::Transcode if s.quality == Quality::P2160 || has("HDR") || has("DV") => -90.0,
                Playback::Transcode => -45.0,
                Playback::Unknown => 0.0,
            }
        }
        None => {
            if has("x264") && !has("HEVC") {
                score += 15.0;
            }
            if has("AV1") {
                score -= 15.0;
            }
        }
    }
    if has("DV") && !has("HDR") {
        score -= 40.0;
    }
    if has("REMUX") {
        score -= 20.0;
    }
    if has("3D") {
        score -= 150.0;
    }

    // 8. Año que no coincide: casi seguro es otra película.
    if let Some((from, to)) = p.years {
        if !s.years.is_empty() && !s.years.iter().any(|y| (from..=to).contains(y)) {
            score -= 600.0;
        }
    }

    (score * 10.0).round() / 10.0
}

/// Calcula el score de cada stream y ordena de mayor a menor (desempata por seeders).
pub fn rank(streams: &mut [Stream], downloaded: impl Fn(&Stream) -> bool, p: &Prefs) {
    for s in streams.iter_mut() {
        let d = downloaded(s);
        if d {
            s.cached = Some(true);
        } else if s.cached.is_none() && s.info_hash.is_some() && s.url.is_none() {
            s.cached = Some(false);
        }
        (s.score, s.reasons) = score_with(s, d, p);
    }
    streams.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.seeders.cmp(&a.seeders)));
}

/// Rango de años aceptable a partir del `year` de Cinemeta ("1968", "2008-2013", "2022-"), con ±1
/// de tolerancia. `current_year` cierra los rangos abiertos de series en emisión.
pub fn year_range(year: &str, current_year: u16) -> Option<(u16, u16)> {
    let mut parts = year.split('-').map(str::trim);
    let from: u16 = parts.next()?.get(..4)?.parse().ok()?;
    let to: u16 = match parts.next() {
        None => from,
        Some("") => current_year.max(from),
        Some(t) => t.get(..4).and_then(|t| t.parse().ok()).unwrap_or(current_year),
    };
    Some((from - 1, to + 1))
}

/// `?caps=` de `/api/streams`: un `ClientCaps` en JSON (`{"h264":true,...}`) o una lista separada
/// por comas (`h264,hevc,aac,mkv`). Nombres desconocidos se ignoran.
pub fn parse_caps(s: &str) -> Option<ClientCaps> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.starts_with('{') {
        return serde_json::from_str(s).ok();
    }
    let mut c = ClientCaps::default();
    for name in s.split(',').map(|n| n.trim().to_ascii_lowercase()) {
        match name.as_str() {
            "h264" | "avc" => c.h264 = true,
            "hevc" | "h265" => c.hevc = true,
            "av1" => c.av1 = true,
            "vp9" => c.vp9 = true,
            "mkv" => c.mkv = true,
            "aac" => c.aac = true,
            "ac3" => c.ac3 = true,
            "eac3" => c.eac3 = true,
            "dts" => c.dts = true,
            "opus" => c.opus = true,
            "flac" => c.flac = true,
            _ => {}
        }
    }
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::torrentio::MediaGuess;

    const GB: u64 = 1 << 30;

    fn st(q: Quality, seeders: u32, size_gb: f64, tags: &[&str]) -> Stream {
        Stream {
            key: format!("{q:?}-{seeders}-{size_gb}"),
            info_hash: Some("0".repeat(40)),
            file_idx: Some(0),
            url: None,
            title: String::new(),
            file_name: None,
            quality: q,
            size_bytes: Some((size_gb * GB as f64) as u64),
            seeders: Some(seeders),
            source: None,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            languages: vec![],
            cached: None,
            score: 0.0,
            trackers: vec![],
            debrid: None,
            media: MediaGuess::default(),
            cyrillic: false,
            years: vec![],
            group: None,
            binge: None,
            reasons: vec![],
        }
    }

    fn sc(s: &Stream) -> f64 {
        score(s, false, &Prefs::default())
    }

    #[test]
    fn seeders_dominate() {
        let healthy = st(Quality::P1080, 200, 2.5, &["x264"]);
        let dead = st(Quality::P1080, 2, 2.5, &["x264"]);
        assert!(sc(&healthy) > sc(&dead) + 200.0);
        // Un 720p sano le gana a un 1080p casi muerto.
        assert!(sc(&st(Quality::P720, 150, 1.2, &[])) > sc(&st(Quality::P1080, 3, 2.5, &[])));
        // La curva es logarítmica: de 500 a 1000 seeders casi no cambia.
        let d = sc(&st(Quality::P1080, 1000, 2.5, &[])) - sc(&st(Quality::P1080, 500, 2.5, &[]));
        assert!(d < 30.0);
    }

    #[test]
    fn preferred_quality_without_overshooting() {
        let p1080 = st(Quality::P1080, 100, 3.0, &[]);
        let p2160 = st(Quality::P2160, 100, 15.0, &[]);
        let p720 = st(Quality::P720, 100, 1.5, &[]);
        assert!(sc(&p1080) > sc(&p2160));
        assert!(sc(&p1080) > sc(&p720));
        // Si el usuario prefiere 4K, gana el 4K.
        let pref4k = Prefs { preferred: Quality::P2160, ..Prefs::default() };
        assert!(score(&p2160, false, &pref4k) > score(&p1080, false, &pref4k));
        // Caso real (Saving Private Ryan en la tele, 169 min): el YIFY 1080p en mp4 con 290 seeders
        // le ganaba a un 4K HEVC de 8 GB con 135 (remux en la tele, sin mkv).
        let tv = Prefs { preferred: Quality::P2160, duration_s: 169.0 * 60.0, caps: parse_caps("h264,hevc,aac"), ..Prefs::default() };
        let mut yify = st(Quality::P1080, 290, 2.2, &["x264"]);
        yify.media = MediaGuess { container: Some("mp4"), video: Some("h264"), audio: vec!["aac"] };
        let mut uhd = st(Quality::P2160, 135, 8.0, &["HEVC"]);
        uhd.media = MediaGuess { container: Some("mkv"), video: Some("hevc"), audio: vec!["aac"] };
        assert!(score(&uhd, false, &tv) > score(&yify, false, &tv));
        // Pero un 4K casi muerto no le gana a un 1080p sano.
        let dying = st(Quality::P2160, 3, 8.0, &["HEVC"]);
        assert!(score(&yify, false, &tv) > score(&dying, false, &tv));
    }

    #[test]
    fn absurd_bitrate_penalized() {
        let encode = st(Quality::P2160, 80, 15.0, &["HEVC"]);
        let remux = st(Quality::P2160, 80, 80.0, &["HEVC", "REMUX"]);
        assert!(sc(&encode) > sc(&remux) + 100.0);
        // Caso real de Inception (148 min): 21 GB 1080p con 190 seeders no le gana a 2,9 GB con 100.
        let inception = Prefs { duration_s: 148.0 * 60.0, ..Prefs::default() };
        let huge = st(Quality::P1080, 190, 20.9, &["x264"]);
        let normal = st(Quality::P1080, 100, 2.9, &["x264"]);
        assert!(score(&normal, false, &inception) > score(&huge, false, &inception) + 50.0);
        // Con la duración real (episodio de 50 min) 9 GB es demasiado para 1080p.
        let ep = Prefs { duration_s: 3000.0, ..Prefs::default() };
        assert!(score(&st(Quality::P1080, 50, 2.0, &[]), false, &ep) > score(&st(Quality::P1080, 50, 9.0, &[]), false, &ep));
    }

    #[test]
    fn language_preference() {
        let original = st(Quality::P1080, 100, 2.0, &[]);
        let mut dub = original.clone();
        dub.languages = vec!["🇷🇺".into()];
        let mut with_en = original.clone();
        with_en.languages = vec!["🇬🇧".into(), "🇷🇺".into()];
        let mut latino = original.clone();
        latino.languages = vec!["🇲🇽".into()];
        assert!(sc(&original) > sc(&dub) + 100.0);
        assert!(sc(&with_en) > sc(&dub));
        assert!(sc(&latino) > sc(&with_en));
        let mut russian = with_en.clone();
        russian.cyrillic = true;
        assert!(sc(&with_en) > sc(&russian));
    }

    #[test]
    fn direct_only_sinks_non_direct() {
        let caps = parse_caps("h264,aac,opus").unwrap();
        let mut mp4 = st(Quality::P1080, 100, 2.0, &["x264"]);
        mp4.media = MediaGuess { container: Some("mp4"), video: Some("h264"), audio: vec!["aac"] };
        let mut mkv = mp4.clone();
        mkv.media.container = Some("mkv");
        let with = Prefs { caps: Some(caps.clone()), direct_only: true, ..Prefs::default() };
        let without = Prefs { caps: Some(caps), ..Prefs::default() };
        let gap_direct_only = score(&mp4, false, &with) - score(&mkv, false, &with);
        let gap_normal = score(&mp4, false, &without) - score(&mkv, false, &without);
        assert!(gap_direct_only > gap_normal + 200.0, "{gap_direct_only} vs {gap_normal}");
    }

    #[test]
    fn caps_reward_direct_play() {
        let browser = parse_caps("h264,aac,opus").unwrap();
        let p = Prefs { caps: Some(browser.clone()), ..Prefs::default() };
        let mut mp4 = st(Quality::P1080, 100, 2.0, &["x264"]);
        mp4.media = MediaGuess { container: Some("mp4"), video: Some("h264"), audio: vec!["aac"] };
        let mut hevc = st(Quality::P1080, 100, 2.0, &["HEVC"]);
        hevc.media = MediaGuess { container: Some("mkv"), video: Some("hevc"), audio: vec!["eac3"] };
        assert_eq!(playback(&mp4, &browser), Playback::Direct);
        assert_eq!(playback(&hevc, &browser), Playback::Transcode);
        assert!(score(&mp4, false, &p) > score(&hevc, false, &p) + 60.0);
        // Un cliente que reproduce HEVC en mkv (desktop) no lo castiga.
        let desktop = parse_caps(r#"{"h264":true,"hevc":true,"mkv":true,"aac":true,"eac3":true}"#).unwrap();
        assert_eq!(playback(&hevc, &desktop), Playback::Direct);
        // h264 en mkv con un navegador sin mkv: remux.
        let mut mkv = mp4.clone();
        mkv.media.container = Some("mkv");
        assert_eq!(playback(&mkv, &browser), Playback::Remux);
        assert!(parse_caps("").is_none());
    }

    #[test]
    fn wrong_year_sinks() {
        // Caso real: buscando "Night of the Living Dead" (1968) aparece una parodia de 2026.
        let p = Prefs { years: year_range("1968", 2026), ..Prefs::default() };
        let mut original = st(Quality::P720, 10, 1.0, &[]);
        original.years = vec![1968];
        let mut other = st(Quality::P720, 40, 2.4, &[]);
        other.years = vec![2026];
        let no_year = st(Quality::P720, 10, 1.0, &[]);
        let mut pack = st(Quality::P720, 10, 1.0, &[]);
        pack.years = vec![2025, 1968]; // colección de 2025 con el archivo de 1968
        assert!(score(&original, false, &p) > score(&other, false, &p) + 400.0);
        assert_eq!(score(&no_year, false, &p), score(&original, false, &p));
        assert_eq!(score(&pack, false, &p), score(&original, false, &p));
        // ±1 de tolerancia y rangos de series.
        assert_eq!(year_range("1968", 2026), Some((1967, 1969)));
        assert_eq!(year_range("2008-2013", 2026), Some((2007, 2014)));
        assert_eq!(year_range("2022-", 2026), Some((2021, 2027)));
        assert_eq!(year_range("", 2026), None);
    }

    #[test]
    fn downloaded_and_debrid_win() {
        let local = st(Quality::P1080, 0, 3.0, &[]);
        let popular = st(Quality::P1080, 2000, 3.0, &[]);
        assert!(score(&local, true, &Prefs::default()) > sc(&popular));
        let mut rd = st(Quality::P1080, 1, 3.0, &[]);
        rd.debrid = Some(true);
        assert!(sc(&rd) > sc(&popular));
        rd.debrid = Some(false);
        assert!(sc(&rd) < sc(&popular));
    }

    #[test]
    fn cam_and_max_size() {
        assert!(sc(&st(Quality::Cam, 3000, 1.5, &[])) < sc(&st(Quality::P720, 20, 1.0, &[])));
        let p = Prefs { max_size_bytes: Some(4 * GB), ..Prefs::default() };
        assert!(score(&st(Quality::P1080, 500, 6.0, &[]), false, &p) < score(&st(Quality::P720, 30, 1.0, &[]), false, &p));
    }

    /// Un episodio de anime: release con grupo y nombre.
    fn anime(title: &str, seeders: u32) -> Stream {
        let mut s = st(Quality::P1080, seeders, 1.4, &[]);
        s.key = title.into();
        s.title = title.into();
        s.group = taste::release_group(title);
        s
    }

    fn ranked(list: &[Stream], p: &Prefs) -> Vec<Stream> {
        let mut v = list.to_vec();
        rank(&mut v, |_| false, p);
        v
    }

    /// Caso real (Demon Slayer): el usuario elegía a mano Erai-raws (trae subtítulos en español).
    #[test]
    fn learns_the_preferred_release() {
        use crate::taste::{infer, PlayRow};
        let erai = |series: &str, manual: bool, outcome: Option<&str>| PlayRow {
            series_id: series.into(),
            group: Some("erai-raws".into()),
            sub_langs: vec!["eng".into(), "spa".into()],
            manual,
            outcome: outcome.map(str::to_string),
            watched_s: 1400.0,
            ..Default::default()
        };
        let subsplease = PlayRow { series_id: "tt9335498".into(), group: Some("subsplease".into()), outcome: Some("switched".into()), ..Default::default() };
        let history = vec![erai("tt9335498", true, Some("finished")), subsplease, erai("tt9335498", true, Some("finished"))];
        let me = infer(&history, "tt9335498", None);
        let p = Prefs { duration_s: 1440.0, personal: Some(me.clone()), ..Prefs::default() };

        // Sin historial, todo como antes: gana el más sano.
        let list = [anime("[SubsPlease] Kimetsu no Yaiba - 06 (1080p)", 250), anime("[Erai-raws] Kimetsu no Yaiba - 06 [1080p]", 60)];
        let none = Prefs { duration_s: 1440.0, ..Prefs::default() };
        let v = ranked(&list, &none);
        assert_eq!(v[0].group.as_deref(), Some("subsplease"));
        assert!(v[0].reasons.is_empty());
        assert!(infer(&[], "tt9335498", None).is_empty());

        // Con historial: el preferido sano gana, y dice por qué.
        let v = ranked(&list, &p);
        assert_eq!(v[0].group.as_deref(), Some("erai-raws"), "{v:?}");
        assert_eq!(v[0].reasons, ["same_group", "subs:spa"]);
        assert!(me.sub_langs.contains(&"spa".to_string()));

        // El preferido casi muerto pierde contra uno sano con subtítulos en español.
        let list = [anime("[Erai-raws] Kimetsu no Yaiba - 07 [1080p]", 2), anime("Kimetsu no Yaiba 07 1080p Sub Esp-Cytox", 180)];
        let v = ranked(&list, &p);
        assert_eq!(v[0].group.as_deref(), Some("cytox"), "{v:?}");
        assert_eq!(v[0].reasons, ["subs:spa"]);

        // Lo de una serie pesa poco en otra: en otra serie el grupo solo no da vuelta la salud.
        let other = Prefs { duration_s: 1440.0, personal: Some(infer(&history, "tt0388629", None)), ..Prefs::default() };
        let list = [anime("[Erai-raws] One Piece - 1100 [1080p]", 40), anime("[SubsPlease] One Piece - 1100 (1080p)", 400)];
        let v = ranked(&list, &other);
        assert_eq!(v[0].group.as_deref(), Some("subsplease"));

        // El bonus está acotado.
        let (b, _) = taste::bonus(&anime("[Erai-raws] Kimetsu no Yaiba - 08 Dual Audio Sub Esp", 50), &me);
        assert!(b <= taste::MAX_BONUS);
    }

    /// Probado en la PC (Demon Slayer, seeders reales): una sola elección a mano terminada alcanza
    /// para que el grupo venga primero contra un pack mucho más sano, pero no si casi no tiene
    /// seeders; terminado sin elegirlo a mano pesa menos.
    #[test]
    fn one_manual_pick_is_enough() {
        use crate::taste::{infer, PlayRow};
        let e1 = |manual: bool| PlayRow {
            series_id: "tt9335498".into(),
            group: Some("erai-raws".into()),
            sub_langs: vec!["eng".into(), "spa".into()],
            manual,
            outcome: Some("finished".into()),
            watched_s: 1420.0,
            ..Default::default()
        };
        let p = |manual: bool| Prefs { duration_s: 1440.0, personal: Some(infer(&[e1(manual)], "tt9335498", None)), ..Prefs::default() };
        let pack = anime("[Tenrai-Sensei] Demon Slayer: Kimetsu no Yaiba S1+S2+S3 [BD][1080p]", 129);
        let list = [pack.clone(), anime("[Erai-raws] Kimetsu no Yaiba - 01 ~ 26 [1080p][HEVC][BATCH]", 15)];
        let v = ranked(&list, &p(true));
        assert_eq!(v[0].group.as_deref(), Some("erai-raws"), "{v:?}");
        assert_eq!(v[0].reasons, ["same_group"]);
        assert_eq!(ranked(&list, &p(false))[0].group.as_deref(), Some("tenrai-sensei"), "sin elegirlo a mano, menos confianza");
        // El episodio siguiente arrancó solo con el mismo grupo (sin elegirlo): sigue con confianza completa.
        let e2 = PlayRow { manual: false, outcome: None, watched_s: 300.0, ..e1(true) };
        let me = infer(&[e2, e1(true)], "tt9335498", None);
        assert_eq!(me.series_group, Some(("erai-raws".to_string(), 1.0)));
        let weak = [pack, anime("[Erai-raws] Kimetsu no Yaiba - 01 ~ 26 [1080p][Multiple Subtitle]", 7)];
        assert_eq!(ranked(&weak, &p(true))[0].group.as_deref(), Some("tenrai-sensei"), "con 7 seeders no le gana a 129");
    }

    /// Las banderas de Torrentio con "Multi Subs" son los subtítulos (caso real: Erai-raws en
    /// Demon Slayer, `[ENG][POR-BR][SPA-LA]...`).
    #[test]
    fn flags_with_multi_subs_are_subtitles() {
        use crate::taste::flag_subs;
        let with = |langs: &[&str]| Stream { languages: langs.iter().map(|x| x.to_string()).collect(), ..st(Quality::P1080, 20, 0.5, &[]) };
        assert_eq!(flag_subs(&with(&["multi-subs", "🇬🇧", "🇷🇺", "🇧🇷", "🇪🇸", "🇲🇽"])), ["eng", "rus", "por", "spa"]);
        assert!(flag_subs(&with(&["Dubbed", "multi-subs", "dual", "🇪🇸"])).is_empty(), "con doblaje pueden ser audio");
        assert!(flag_subs(&with(&["🇪🇸"])).is_empty(), "sin Multi Subs es el audio");
        // Con subLang en español, sube aunque nunca se haya abierto ese grupo.
        let me = crate::taste::infer(&[], "tt1", Some("spa"));
        let (b, why) = crate::taste::bonus(&with(&["multi-subs", "🇬🇧", "🇲🇽"]), &me);
        assert!(b > 0.0);
        assert_eq!(why, ["subs:spa"]);
    }

    #[test]
    fn rank_sorts_desc_and_marks_cached() {
        let mut v = vec![st(Quality::P1080, 3, 2.0, &[]), st(Quality::P1080, 300, 2.0, &[]), st(Quality::P720, 50, 1.0, &[])];
        v[2].info_hash = Some("a".repeat(40));
        rank(&mut v, |s| s.info_hash.as_deref() == Some(&"a".repeat(40)), &Prefs::default());
        assert_eq!(v[0].seeders, Some(50));
        assert_eq!(v[0].cached, Some(true));
        assert_eq!(v[1].seeders, Some(300));
        assert_eq!(v[2].cached, Some(false));
        assert!(v.windows(2).all(|w| w[0].score >= w[1].score));
    }
}
