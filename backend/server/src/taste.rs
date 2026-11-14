//! Streams que aprenden qué release prefiere cada usuario (docs/API.md, "Learned preferences").
//!
//! Caso real: en Demon Slayer el usuario elegía a mano "Erai-raws" (subtítulos en español); la app
//! tenía que aprenderlo y ofrecerlo primero en los episodios siguientes sin perder la prioridad de
//! una buena conexión.
//!
//! - **Registro** (tabla `plays`): cada `POST /api/play` guarda el grupo del release (del nombre y
//!   el `bingeGroup` de Torrentio), calidad, tamaño, audio dual, los idiomas de audio y subtítulos
//!   REALES del archivo (los de ffprobe), si se eligió a mano y cuál ofrecía la app primero. El
//!   resultado se completa después: `finished` (el progreso pasó el 90 %), `switched` (a los pocos
//!   minutos se abrió otro stream del mismo episodio) o `stalled` (se cambió desde el cartel de
//!   "trabado").
//! - **Inferencia** (`infer`): por serie, el grupo de la última reproducción buena (confianza según
//!   cuántas lo confirman; uno que se trabó o se cambió enseguida no se prefiere); en todas las
//!   series, los grupos que el usuario repite y los idiomas de subtítulos que busca (el de
//!   Settings, o el que tienen de verdad al menos dos elecciones a mano buenas) y el audio dual si
//!   lo elige a mano.
//! - **Uso** (`bonus`, sumado en `score.rs`): acotado entre `MIN_BONUS` y `MAX_BONUS` para que la
//!   salud del torrent siga mandando (un grupo preferido casi muerto no le gana a uno sano). Sin
//!   historial, el bonus es 0 y todo queda como antes. Los motivos (`reasons`) van en cada
//!   `Stream` para que "Choose stream" diga por qué recomienda el primero.
use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection};

use crate::torrentio::Stream;

/// Lo que la reproducción de un stream suma o resta, como mucho.
pub const MAX_BONUS: f64 = 110.0;
pub const MIN_BONUS: f64 = -60.0;
/// Mismo grupo que lo que se vio de esta serie (con confianza completa). Probado con Demon Slayer
/// en la PC: con 60, un Erai-raws de 15 seeders elegido a mano y terminado quedaba sexto detrás de
/// un pack de 129; con 90 queda primero, y uno de menos de ~10 seeders (o con menos de 5, que
/// además resta 180) sigue perdiendo contra uno sano.
const SAME_GROUP: f64 = 90.0;
/// Un grupo que el usuario repite en otras series.
const LIKED_GROUP: f64 = 15.0;
/// El grupo de esta serie que se trabó o se cambió enseguida.
const BAD_GROUP: f64 = -40.0;
/// Subtítulos en el idioma que el usuario busca.
const SUBS: f64 = 50.0;
/// Audio dual, si lo elige.
const DUAL: f64 = 25.0;
/// Se mira hasta esta cantidad de reproducciones del usuario (las más nuevas).
pub const HISTORY: usize = 300;
/// Abrir otro stream del mismo episodio antes de esto (y habiendo visto menos de 5 min) cuenta
/// como "lo cambió".
pub const SWITCH_WINDOW_MS: i64 = 15 * 60 * 1000;

/// El grupo del release: `[Grupo] Nombre ...` o `Nombre...-GRUPO`. En minúsculas (para comparar).
pub fn release_group(title: &str) -> Option<String> {
    let t = title.trim();
    if let Some(rest) = t.strip_prefix('[') {
        let g = rest.split(']').next()?.trim();
        return valid_group(g);
    }
    // Sin la extensión ni lo que venga entre corchetes o paréntesis al final.
    let mut base = t;
    for ext in [".mkv", ".mp4", ".avi", ".webm", ".m4v", ".ts"] {
        let cut = base.len().saturating_sub(ext.len());
        if cut > 0 && base.get(cut..).is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
            base = &base[..cut];
        }
    }
    let base = base.split(['[', '(']).next().unwrap_or(base).trim_end();
    let g = base.rsplit_once('-')?.1.trim();
    valid_group(g)
}

fn valid_group(g: &str) -> Option<String> {
    let lower = g.trim().to_lowercase();
    if !(2..=24).contains(&lower.chars().count()) || lower.contains(' ') {
        return None;
    }
    if !lower.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.')) || !lower.chars().any(char::is_alphabetic) {
        return None;
    }
    // "1080p", "x264", "DL" (de WEB-DL) y parecidos no son grupos.
    const TECH: &[&str] = &["x264", "x265", "h264", "h265", "hevc", "avc", "aac", "ac3", "dts", "dl", "web", "webrip", "bluray", "hdr", "remux", "rip"];
    let resolution = lower.strip_suffix('p').is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
    (!resolution && !TECH.contains(&lower.as_str())).then_some(lower)
}

/// Normaliza un código de idioma (ffprobe da "spa", "es", "es-419"...) a 3 letras.
pub fn norm_lang(l: &str) -> Option<String> {
    let l = l.trim().to_ascii_lowercase();
    let l = l.split(['-', '_']).next().unwrap_or("");
    let three = match l {
        "en" | "eng" => "eng",
        "es" | "spa" | "esp" => "spa",
        "pt" | "por" | "pob" => "por",
        "fr" | "fre" | "fra" => "fre",
        "de" | "ger" | "deu" => "ger",
        "it" | "ita" => "ita",
        "ja" | "jpn" => "jpn",
        "ru" | "rus" => "rus",
        "" | "und" | "unk" => return None,
        other if other.len() == 3 => other,
        _ => return None,
    };
    Some(three.to_string())
}

/// Subtítulos que el nombre del release anuncia ("ESub", "Sub Esp", "Multi Subs"...). `multi`
/// sin saber cuáles.
pub fn name_sub_langs(text: &str) -> (Vec<String>, bool) {
    let h = format!(" {} ", text.to_ascii_lowercase().replace(['.', '_', '[', ']', '(', ')'], " "));
    let mut out = Vec::new();
    let mut add = |l: &str| {
        if !out.iter().any(|x: &String| x == l) {
            out.push(l.to_string());
        }
    };
    if [" esub", " eng sub", " engsub", " english sub"].iter().any(|k| h.contains(k)) {
        add("eng");
    }
    if [" sub esp", " subs esp", " spa sub", " sub español", " subtitulado", " subesp", " castellano sub", " sub spa", " vose "].iter().any(|k| h.contains(k)) {
        add("spa");
    }
    let multi = [" multi sub", " multisub", " multi-sub", " multi subs", "multi-subs"].iter().any(|k| h.contains(k));
    (out, multi)
}

/// Una reproducción guardada.
#[derive(Clone, Debug, Default)]
pub struct PlayRow {
    pub series_id: String,
    pub group: Option<String>,
    pub binge: Option<String>,
    pub dual_audio: bool,
    pub sub_langs: Vec<String>,
    pub manual: bool,
    pub outcome: Option<String>,
    pub watched_s: f64,
}

impl PlayRow {
    fn bad(&self) -> bool {
        matches!(self.outcome.as_deref(), Some("switched" | "stalled"))
    }
    /// Una reproducción que salió bien: terminada, o en curso elegida a mano o con más de 5 min.
    fn good(&self) -> bool {
        match self.outcome.as_deref() {
            Some("finished") => true,
            None => self.manual || self.watched_s >= 300.0,
            _ => false,
        }
    }
}

/// Lo aprendido de un usuario para una serie (o película).
#[derive(Clone, Debug, Default)]
pub struct Personal {
    /// Grupo (o bingeGroup) preferido en esta serie y su confianza (0..1).
    pub series_group: Option<(String, f64)>,
    /// Grupos de esta serie que se trabaron o se cambiaron enseguida.
    pub bad_groups: HashSet<String>,
    /// Grupos que el usuario repite en otras series.
    pub liked_groups: HashSet<String>,
    /// Subtítulos reales vistos por grupo (de cualquier serie): para adivinar los de un stream.
    pub group_subs: HashMap<String, HashSet<String>>,
    /// Idiomas de subtítulos que busca.
    pub sub_langs: Vec<String>,
    /// Elige audio dual.
    pub dual: bool,
}

impl Personal {
    pub fn is_empty(&self) -> bool {
        self.series_group.is_none() && self.bad_groups.is_empty() && self.liked_groups.is_empty() && self.sub_langs.is_empty() && !self.dual
    }
}

/// Identidad de un release para comparar entre episodios: el grupo del nombre o, si no hay, el
/// `bingeGroup` de Torrentio.
fn ident(group: &Option<String>, binge: &Option<String>) -> Option<String> {
    group.clone().or_else(|| binge.as_ref().map(|b| format!("binge:{}", b.to_ascii_lowercase())))
}

/// `rows`: las reproducciones del usuario, de la más nueva a la más vieja. `ui_sub_lang`: el
/// idioma de subtítulos de Settings (`subLang`).
pub fn infer(rows: &[PlayRow], series_id: &str, ui_sub_lang: Option<&str>) -> Personal {
    let mut p = Personal::default();
    // --- esta serie: la última buena manda; lo que se trabó o se cambió, no.
    let mine: Vec<&PlayRow> = rows.iter().filter(|r| r.series_id == series_id).take(20).collect();
    for r in &mine {
        let Some(id) = ident(&r.group, &r.binge) else { continue };
        if r.bad() {
            if p.series_group.as_ref().is_none_or(|(g, _)| *g != id) {
                p.bad_groups.insert(id);
            }
            continue;
        }
        if r.good() && p.series_group.is_none() && !p.bad_groups.contains(&id) {
            let same: Vec<&&PlayRow> = mine.iter().filter(|x| x.good() && ident(&x.group, &x.binge).as_deref() == Some(id.as_str())).collect();
            // Elegido a mano (en contra de lo que ofrecía la app) y visto: lo dijo el usuario, no
            // hace falta que se repita (y los episodios que siguieron solos con ese grupo no le
            // bajan la confianza).
            let conf = match same.len() {
                _ if same.iter().any(|x| x.manual) => 1.0,
                0 | 1 => 0.6,
                2 => 0.8,
                _ => 1.0,
            };
            p.series_group = Some((id, conf));
        }
    }
    // --- otras series: grupos repetidos (en al menos dos series distintas, o tres veces).
    let mut seen: HashMap<String, HashSet<&str>> = HashMap::new();
    for r in rows.iter().filter(|r| r.series_id != series_id && r.good()) {
        if let Some(g) = &r.group {
            seen.entry(g.clone()).or_default().insert(r.series_id.as_str());
        }
    }
    let mut count: HashMap<&str, usize> = HashMap::new();
    for r in rows.iter().filter(|r| r.series_id != series_id && r.good()) {
        if let Some(g) = &r.group {
            *count.entry(g.as_str()).or_default() += 1;
        }
    }
    for (g, series) in &seen {
        if series.len() >= 2 || count.get(g.as_str()).copied().unwrap_or(0) >= 3 {
            p.liked_groups.insert(g.clone());
        }
    }
    // --- subtítulos reales por grupo (para adivinar los de un stream que no se abrió).
    for r in rows {
        if let Some(g) = &r.group {
            if !r.sub_langs.is_empty() {
                p.group_subs.entry(g.clone()).or_default().extend(r.sub_langs.iter().cloned());
            }
        }
    }
    // --- idiomas de subtítulos: el de Settings, o el que tienen de verdad dos elecciones a mano
    // buenas (sin contar inglés, que traen casi todos y no distingue nada).
    if let Some(l) = ui_sub_lang.and_then(norm_lang) {
        p.sub_langs.push(l);
    }
    let mut by_lang: HashMap<&str, usize> = HashMap::new();
    for r in rows.iter().filter(|r| r.manual && r.good()).take(40) {
        for l in &r.sub_langs {
            *by_lang.entry(l.as_str()).or_default() += 1;
        }
    }
    let mut inferred: Vec<&str> = by_lang.iter().filter(|(l, n)| **n >= 2 && **l != "eng").map(|(l, _)| *l).collect();
    inferred.sort();
    for l in inferred {
        if !p.sub_langs.iter().any(|x| x == l) {
            p.sub_langs.push(l.to_string());
        }
    }
    // --- audio dual: dos elecciones a mano buenas con audio dual.
    p.dual = rows.iter().filter(|r| r.manual && r.good() && r.dual_audio).take(40).count() >= 2;
    p
}

/// Si el stream trae audio dual o multi según el nombre.
pub fn dual_audio(s: &Stream) -> bool {
    s.languages.iter().any(|l| l == "multi" || l == "dual") || {
        let t = s.title.to_ascii_lowercase();
        t.contains("dual audio") || t.contains("dual-audio") || t.contains("multi audio")
    }
}

/// Con "Multi Subs" y sin doblaje ni audio dual, las banderas de Torrentio son los subtítulos
/// (Erai-raws: `[ENG][POR-BR][SPA-LA]...` → 🇬🇧 🇧🇷 🇲🇽 ...). Con doblaje no se sabe si son audio.
pub fn flag_subs(s: &Stream) -> Vec<String> {
    let l = &s.languages;
    if !l.iter().any(|x| x == "multi-subs") || l.iter().any(|x| matches!(x.as_str(), "multi" | "dual" | "Dubbed")) {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for f in l {
        let lang = match f.as_str() {
            "🇪🇸" | "🇲🇽" => "spa",
            "🇬🇧" | "🇺🇸" => "eng",
            "🇧🇷" | "🇵🇹" => "por",
            "🇫🇷" => "fre",
            "🇩🇪" => "ger",
            "🇮🇹" => "ita",
            "🇷🇺" => "rus",
            _ => continue,
        };
        if !out.iter().any(|x| x == lang) {
            out.push(lang.to_string());
        }
    }
    out
}

/// Subtítulos que probablemente trae un stream: los del nombre, los de las banderas (con "Multi
/// Subs") y los que tuvo de verdad su grupo.
fn likely_subs(s: &Stream, p: &Personal) -> HashSet<String> {
    let (mut named, _) = name_sub_langs(&format!("{} {}", s.title, s.file_name.as_deref().unwrap_or("")));
    named.extend(flag_subs(s));
    if let Some(g) = &s.group {
        if let Some(seen) = p.group_subs.get(g) {
            named.extend(seen.iter().cloned());
        }
    }
    named.into_iter().collect()
}

/// Lo que suma o resta lo aprendido para este stream, con los motivos para la UI (códigos:
/// `same_group`, `liked_group`, `subs:<idioma>`, `dual_audio`).
pub fn bonus(s: &Stream, p: &Personal) -> (f64, Vec<String>) {
    if p.is_empty() {
        return (0.0, Vec::new());
    }
    let mut b = 0.0;
    let mut why = Vec::new();
    let id = ident(&s.group, &s.binge);
    match (&p.series_group, &id) {
        (Some((g, conf)), Some(i)) if g == i => {
            b += SAME_GROUP * conf;
            why.push("same_group".to_string());
        }
        _ => {
            if let Some(i) = &id {
                if p.bad_groups.contains(i) {
                    b += BAD_GROUP;
                } else if s.group.as_ref().is_some_and(|g| p.liked_groups.contains(g)) {
                    b += LIKED_GROUP;
                    why.push("liked_group".to_string());
                }
            }
        }
    }
    if !p.sub_langs.is_empty() {
        let subs = likely_subs(s, p);
        if let Some(l) = p.sub_langs.iter().find(|l| subs.contains(*l)) {
            b += SUBS;
            why.push(format!("subs:{l}"));
        }
    }
    if p.dual && dual_audio(s) {
        b += DUAL;
        why.push("dual_audio".to_string());
    }
    (b.clamp(MIN_BONUS, MAX_BONUS), why)
}

// ---- base

/// Lo que se guarda de una reproducción al abrirla.
pub struct NewPlay {
    pub user_id: i64,
    pub meta_id: String,
    pub series_id: String,
    pub stream_key: String,
    pub group: Option<String>,
    pub binge: Option<String>,
    pub quality: Option<String>,
    pub size_bytes: Option<u64>,
    pub dual_audio: bool,
    pub audio_langs: Vec<String>,
    pub sub_langs: Vec<String>,
    pub manual: bool,
    pub top_key: Option<String>,
    /// Se abrió desde el cartel de "trabado": la anterior del mismo episodio salió mal.
    pub after_stall: bool,
}

/// Guarda la reproducción y cierra la anterior del mismo episodio si se cambió enseguida.
pub fn record(c: &Connection, p: &NewPlay, now_ms: i64) -> rusqlite::Result<()> {
    let prev: Option<(i64, String, i64, f64)> = c
        .query_row(
            "SELECT id, stream_key, ts_ms, watched_s FROM plays WHERE user_id = ?1 AND meta_id = ?2 AND outcome IS NULL
             ORDER BY ts_ms DESC LIMIT 1",
            params![p.user_id, p.meta_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok();
    if let Some((id, key, ts, watched)) = prev {
        if key != p.stream_key {
            let outcome = if p.after_stall {
                Some("stalled")
            } else if now_ms - ts < SWITCH_WINDOW_MS && watched < 300.0 {
                Some("switched")
            } else {
                None
            };
            if let Some(o) = outcome {
                c.execute("UPDATE plays SET outcome = ?1 WHERE id = ?2", params![o, id])?;
            }
        } else if now_ms - ts < SWITCH_WINDOW_MS {
            // El mismo stream de nuevo (cambio de calidad del audio, reconexión): no es otra fila.
            return Ok(());
        }
    }
    c.execute(
        "INSERT INTO plays (user_id, meta_id, series_id, ts_ms, stream_key, grp, binge, quality, size_bytes, dual_audio,
                            audio_langs, sub_langs, manual, top_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            p.user_id,
            p.meta_id,
            p.series_id,
            now_ms,
            p.stream_key,
            p.group,
            p.binge,
            p.quality,
            p.size_bytes.map(|s| s as i64),
            p.dual_audio,
            p.audio_langs.join(","),
            p.sub_langs.join(","),
            p.manual,
            p.top_key,
        ],
    )?;
    // Solo lo reciente sirve: se recorta lo viejo de este usuario.
    c.execute(
        "DELETE FROM plays WHERE user_id = ?1 AND id NOT IN (SELECT id FROM plays WHERE user_id = ?1 ORDER BY ts_ms DESC LIMIT ?2)",
        params![p.user_id, (HISTORY * 2) as i64],
    )?;
    Ok(())
}

/// Al guardar el progreso: cuánto se vio en esta reproducción (desde donde arrancó: el primer
/// progreso, o más atrás si se volvió) y si se terminó (> 90 % del episodio).
pub fn progress(c: &Connection, user_id: i64, meta_id: &str, position_s: f64, duration_s: f64) -> rusqlite::Result<()> {
    let finished = duration_s > 60.0 && position_s >= duration_s * 0.9;
    c.execute(
        "UPDATE plays SET start_s = MIN(COALESCE(start_s, ?3), ?3),
                          watched_s = MAX(watched_s, ?3 - MIN(COALESCE(start_s, ?3), ?3)),
                          outcome = CASE WHEN ?4 AND outcome IS NULL THEN 'finished' ELSE outcome END
         WHERE id = (SELECT id FROM plays WHERE user_id = ?1 AND meta_id = ?2 ORDER BY ts_ms DESC LIMIT 1)",
        params![user_id, meta_id, position_s, finished],
    )?;
    Ok(())
}

/// Las últimas reproducciones del usuario (de la más nueva a la más vieja).
pub fn recent(c: &Connection, user_id: i64) -> rusqlite::Result<Vec<PlayRow>> {
    let mut stmt = c.prepare_cached(
        "SELECT series_id, grp, binge, dual_audio, sub_langs, manual, outcome, watched_s
         FROM plays WHERE user_id = ?1 ORDER BY ts_ms DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![user_id, HISTORY as i64], |r| {
        let subs: String = r.get(4)?;
        Ok(PlayRow {
            series_id: r.get(0)?,
            group: r.get(1)?,
            binge: r.get(2)?,
            dual_audio: r.get(3)?,
            sub_langs: subs.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect(),
            manual: r.get(5)?,
            outcome: r.get(6)?,
            watched_s: r.get(7)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups() {
        assert_eq!(release_group("[Erai-raws] Kimetsu no Yaiba - 05 [1080p][Multiple Subtitle]").as_deref(), Some("erai-raws"));
        assert_eq!(release_group("[SubsPlease] Demon Slayer - 05 (1080p) [ABCD1234].mkv").as_deref(), Some("subsplease"));
        assert_eq!(release_group("Inception.2010.1080p.BluRay.x264-REFiNED").as_deref(), Some("refined"));
        assert_eq!(release_group("Nocturne 2026 S01E01 720p WEB H264-SYLiX").as_deref(), Some("sylix"));
        assert_eq!(release_group("Show.S01E01.1080p.WEB-DL.DDP5.1.H.264-NTb.mkv").as_deref(), Some("ntb"));
        // Sin grupo reconocible.
        assert_eq!(release_group("Nocturne 2026 S01E01 Lazarus Come Forth 1080p"), None);
        assert_eq!(release_group("Movie 2020 1080p WEB-DL"), None);
        assert_eq!(release_group("Movie.2020.x264"), None);
        // Nombres con letras de varios bytes (no se corta en el medio de una).
        assert_eq!(release_group("Начало.2010.BDRip.мкв"), None);
        assert_eq!(release_group("Начало.2010.1080p-Кинозал").as_deref(), Some("кинозал"));
    }

    #[test]
    fn records_outcomes() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let c = db.conn.lock().unwrap();
        c.execute("INSERT INTO users (id, username, password_hash) VALUES (1, 'ana', 'h')", []).unwrap();
        let play = |key: &str, group: &str, manual: bool, after_stall: bool| NewPlay {
            user_id: 1,
            meta_id: "tt1:1:2".into(),
            series_id: "tt1".into(),
            stream_key: key.into(),
            group: Some(group.into()),
            binge: None,
            quality: Some("P1080".into()),
            size_bytes: Some(1 << 30),
            dual_audio: false,
            audio_langs: vec!["jpn".into()],
            sub_langs: vec!["eng".into(), "spa".into()],
            manual,
            top_key: Some("a".into()),
            after_stall,
        };
        // Auto, a los 2 minutos lo cambia a mano: el primero queda "switched".
        record(&c, &play("a", "subsplease", false, false), 1_000_000).unwrap();
        record(&c, &play("b", "erai-raws", true, false), 1_120_000).unwrap();
        // El mismo stream reabierto enseguida no es otra fila.
        record(&c, &play("b", "erai-raws", true, false), 1_200_000).unwrap();
        progress(&c, 1, "tt1:1:2", 1300.0, 1420.0).unwrap();
        let rows = recent(&c, 1).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].group.as_deref(), Some("erai-raws"));
        assert_eq!(rows[0].outcome.as_deref(), Some("finished"));
        assert_eq!(rows[0].sub_langs, ["eng", "spa"]);
        assert_eq!(rows[1].outcome.as_deref(), Some("switched"));
        // Elegido desde "trabado": el anterior queda "stalled".
        record(&c, &play("c", "cytox", true, false), 9_000_000).unwrap();
        record(&c, &play("d", "judas", true, true), 9_060_000).unwrap();
        let rows = recent(&c, 1).unwrap();
        assert_eq!(rows[1].outcome.as_deref(), Some("stalled"));
        // Lo visto es desde donde arrancó esa reproducción, no la posición: sigue el episodio por
        // los 20 min y al minuto lo cambia → "switched".
        let ep3 = |key: &str, group: &str| NewPlay { meta_id: "tt1:1:3".into(), ..play(key, group, false, false) };
        record(&c, &ep3("e", "dekinai"), 20_000_000).unwrap();
        progress(&c, 1, "tt1:1:3", 1200.0, 1420.0).unwrap();
        progress(&c, 1, "tt1:1:3", 1260.0, 1420.0).unwrap();
        let w: f64 = c.query_row("SELECT watched_s FROM plays WHERE stream_key = 'e'", [], |r| r.get(0)).unwrap();
        assert_eq!(w, 60.0);
        record(&c, &NewPlay { manual: true, ..ep3("f", "erai-raws") }, 20_070_000).unwrap();
        let rows = recent(&c, 1).unwrap();
        assert_eq!(rows[1].outcome.as_deref(), Some("switched"), "{rows:?}");
        // Volver atrás baja el inicio (empezar de nuevo cuenta desde ahí).
        progress(&c, 1, "tt1:1:3", 0.0, 1420.0).unwrap();
        progress(&c, 1, "tt1:1:3", 400.0, 1420.0).unwrap();
        assert_eq!(recent(&c, 1).unwrap()[0].watched_s, 400.0);
        let p = infer(&rows, "tt1", None);
        assert!(p.bad_groups.contains("cytox"));
        assert!(p.bad_groups.contains("subsplease"));
    }

    #[test]
    fn langs() {
        assert_eq!(norm_lang("es-419").as_deref(), Some("spa"));
        assert_eq!(norm_lang("ENG").as_deref(), Some("eng"));
        assert_eq!(norm_lang("und"), None);
        assert_eq!(name_sub_langs("Movie.2020.1080p.WEB.ESub.x264").0, vec!["eng"]);
        assert_eq!(name_sub_langs("Peli 2020 1080p Sub Esp").0, vec!["spa"]);
        assert!(name_sub_langs("[Erai-raws] Show - 01 [Multi-Subs]").1);
    }
}
