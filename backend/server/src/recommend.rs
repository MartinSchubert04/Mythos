//! Recomendaciones por usuario: "Para vos" en el inicio y "Similares" en la ficha (docs/API.md,
//! "Recomendaciones").
//!
//! - **Gustos** ([`Profile`]): de lo que el usuario vio y de su biblioteca. No hay calificaciones
//!   propias en Mythos, así que las señales son: películas vistas (≥ 60 % pesa 1; entre 25 y 60 %,
//!   0,5; menos no dice nada), series (1 por el primer episodio visto, +0,15 por cada otro, hasta
//!   2; dos episodios empezados sin terminar, 0,5) y la biblioteca (0,6; +0,2 si además la vio).
//!   Cada título pesa menos con el tiempo (vida media de 90 días, nunca menos de la cuarta parte).
//!   Los géneros y el año salen de los datos de IMDb (`imdb.rs`); si un título no está ahí, los
//!   géneros que guardó la biblioteca. El perfil es por cuenta (cada perfil de la tele es una
//!   cuenta) y mezcla películas y series: quien solo ve series policiales recibe películas
//!   policiales.
//! - **Candidatos**: lo bien calificado de IMDb (rating ≥ 7.0 y los votos por defecto del Top rated:
//!   25 mil en películas, 10 mil en series), sin nada que el usuario ya tenga en el progreso (visto,
//!   terminado, en "Seguir viendo" o sacado de ahí) ni en la biblioteca.
//! - **Puntaje** = afinidad^1,5 × calidad × época. Afinidad: el promedio, en los géneros del
//!   candidato, de cuánto más que lo normal ve el usuario ese género ("lift": su parte en lo que vio
//!   contra su parte entre los candidatos, suavizado y con tope 4). Así Drama, que está en casi
//!   todo, pesa poco, y lo distintivo (Animación, Western) pesa mucho. Calidad: rating y, la mitad,
//!   votos (los de rating alto con pocos votos suelen estar inflados). Época: cerca del año
//!   promedio de lo que vio suma hasta un 25 %. Animación y documental son "formato": si casi no
//!   los ve (< 10 %), lo que los tiene vale un 30 %.
//! - **Rotación**: los 120 mejores puntajes y de ahí 20 por muestreo ponderado sin reemplazo
//!   (Efraimidis–Spirakis: cada uno saca `ln(u) / puntaje²` y quedan los más altos), con `u` derivado
//!   de (usuario, tipo, bloque de 6 h, id). Sin estado: dentro del bloque la fila es la misma (volver
//!   al inicio no la baraja de nuevo), y cada 6 h sale otra; los de más puntaje aparecen más seguido
//!   pero no siempre, y el orden también cambia. Ver algo nuevo cambia el perfil y la fila enseguida.
//! - **Sin gustos suficientes** (menos de [`MIN_TITLES`] títulos con señal): 20 al azar (misma
//!   semilla) de los 300 mejores del Top rated de ese tipo, sin lo ya visto.
//! - **Similares** (`?like=`): parecido en géneros (Jaccard ≥ 0,3, y animación o documental solo
//!   si el título lo es) con la misma calidad y época relativa a él; si hay gustos, acomodan ±25 %
//!   sin tapar el parecido.
//! - Sin datos de IMDb (bajando, modo local) responde `state` como `/api/top` y sin títulos: la UI
//!   usa lo de antes (Cinemeta).
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use axum::{
    extract::{FromRef, State},
    routing::get,
    Router,
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{Auth, AuthUser},
    cinemeta::{check_id, check_type, Cinemeta, MetaPreview},
    db::{now_ms, now_unix, Db},
    error::{ApiResult, Json, Path, Query},
    imdb::{genre_bit, Dataset, Imdb, Kind, Title, TopState, ADULT_BIT, DEFAULT_VOTES_MOVIE, DEFAULT_VOTES_SERIES, GENRES},
    state::AppState,
};

/// Con menos títulos con señal no hay gustos: al azar del Top rated.
pub const MIN_TITLES: usize = 3;
/// Rating mínimo (× 10) de lo recomendado.
const MIN_RATING: u8 = 70;
/// Entre cuántos de los mejores puntajes se sortea.
const POOL: usize = 120;
const LIKE_POOL: usize = 60;
/// Sin gustos: al azar de los N mejores del Top rated.
pub const TOP_FALLBACK: usize = 300;
/// Cada cuánto cambia la selección.
pub const ROTATE_S: i64 = 6 * 3600;
const LIMIT_DEFAULT: usize = 20;
const LIMIT_MAX: usize = 40;
/// Los primeros N salen con descripción y logo de Cinemeta (el destacado del inicio).
const HERO: usize = 4;
/// Lo que se espera a Cinemeta para eso; lo que no llegó sigue bajando y queda en su caché.
const DESCRIBE_WAIT: Duration = Duration::from_secs(2);
/// Cuántas filas de progreso se miran (las más nuevas).
const HISTORY_ROWS: usize = 3000;
/// Visto "de verdad" (fracción) y visto "un poco".
const WATCHED: f64 = 0.6;
const PARTIAL: f64 = 0.25;
const LIBRARY: f64 = 0.6;
const HALF_LIFE_DAYS: f64 = 90.0;
const DECAY_FLOOR: f64 = 0.25;
/// Suavizado del lift (un género nunca visto no queda en cero del todo) y su tope.
const SMOOTH: f64 = 0.15;
const MAX_LIFT: f64 = 4.0;
const NG: usize = GENRES.len();

// ---------- historial ----------

/// Un título (película o serie) con señal de gusto.
#[derive(Clone, Debug, PartialEq)]
pub struct Watched {
    pub id: String,
    /// 0..2,2: cuánto dice de los gustos (antes del paso del tiempo).
    pub engage: f64,
    /// unix ms de lo último (progreso o agregado a la biblioteca).
    pub ts_ms: i64,
    /// Géneros de la biblioteca (si el título no está en los datos de IMDb).
    pub genres: u32,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    pub watched: Vec<Watched>,
    /// Lo que no se recomienda: todo lo que tiene progreso (también borrado de "Seguir viendo") y
    /// la biblioteca. Ids de película o de serie.
    pub seen: HashSet<String>,
}

#[derive(Default)]
struct Agg {
    movie: bool,
    best: f64,
    done: u32,
    started: u32,
    ts: i64,
}

fn fraction(pos: f64, dur: f64) -> f64 {
    if dur > 0.0 {
        (pos / dur).clamp(0.0, 1.0)
    } else {
        // Sin duración conocida: una hora y media es "entera".
        (pos / 5400.0).clamp(0.0, 1.0)
    }
}

/// El historial del usuario (progreso y biblioteca).
pub fn load_history(c: &Connection, uid: i64) -> rusqlite::Result<History> {
    let mut h = History::default();
    let mut agg: HashMap<String, Agg> = HashMap::new();
    let mut stmt = c.prepare_cached(
        "SELECT meta_id, type, series_id, position_s, duration_s, updated_ms, deleted FROM progress
         WHERE user_id = ?1 ORDER BY updated_ms DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![uid, HISTORY_ROWS as i64], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, f64>(3)?,
            r.get::<_, f64>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, bool>(6)?,
        ))
    })?;
    for row in rows {
        let (meta_id, kind, series_id, pos, dur, ts, deleted) = row?;
        let movie = kind == "movie";
        let id = match series_id {
            Some(s) if !movie => s,
            _ => meta_id.split(':').next().unwrap_or(&meta_id).to_string(),
        };
        h.seen.insert(id.clone());
        // Lo sacado de "Seguir viendo" no se recomienda, pero tampoco dice que le guste.
        if deleted {
            continue;
        }
        let f = fraction(pos, dur);
        let a = agg.entry(id).or_default();
        a.movie = movie;
        a.best = a.best.max(f);
        a.started += 1;
        a.done += u32::from(f >= WATCHED);
        a.ts = a.ts.max(ts);
    }
    let mut by_id: HashMap<String, Watched> = HashMap::new();
    for (id, a) in agg {
        let engage = if a.movie {
            if a.best >= WATCHED {
                1.0
            } else if a.best >= PARTIAL {
                0.5
            } else {
                0.0
            }
        } else if a.done >= 1 {
            (1.0 + 0.15 * (a.done - 1) as f64).min(2.0)
        } else if a.started >= 2 {
            0.5
        } else if a.best >= PARTIAL {
            0.4
        } else {
            0.0
        };
        if engage > 0.0 {
            by_id.insert(id.clone(), Watched { id, engage, ts_ms: a.ts, genres: 0 });
        }
    }

    let mut stmt = c.prepare_cached("SELECT id, data, updated_ms FROM library WHERE user_id = ?1 AND deleted = 0")?;
    let rows = stmt.query_map([uid], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?;
    for row in rows {
        let (id, data, ts) = row?;
        h.seen.insert(id.clone());
        let genres = serde_json::from_str::<MetaPreview>(&data)
            .ok()
            .and_then(|m| m.genres)
            .unwrap_or_default()
            .iter()
            .filter_map(|g| genre_bit(g))
            .fold(0, |m, b| m | b);
        by_id
            .entry(id.clone())
            .and_modify(|w| {
                w.engage = w.engage.max(LIBRARY) + 0.2;
                w.ts_ms = w.ts_ms.max(ts);
                w.genres = genres;
            })
            .or_insert(Watched { id, engage: LIBRARY, ts_ms: ts, genres });
    }
    h.watched = by_id.into_values().collect();
    h.watched.sort_by(|a, b| b.ts_ms.cmp(&a.ts_ms).then(a.id.cmp(&b.id)));
    Ok(h)
}

// ---------- gustos ----------

/// Lo que se sabe de los gustos del usuario.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// Peso de cada género (suma de los pesos de los títulos que lo tienen).
    genre_w: [f64; NG],
    /// Suma de los pesos de los títulos.
    total: f64,
    /// Títulos con señal y géneros conocidos.
    pub titles: usize,
    /// Año promedio de lo visto y su desvío (ponderados).
    year: Option<(f64, f64)>,
}

impl Profile {
    pub fn build(h: &History, d: &Dataset, now_ms: i64) -> Self {
        let mut genre_w = [0.0; NG];
        let (mut total, mut titles) = (0.0, 0);
        let (mut ysum, mut y2sum, mut ywsum) = (0.0, 0.0, 0.0);
        for w in &h.watched {
            let found = d.find(&w.id);
            let genres = found.map(|(_, t)| t.genres).filter(|&g| g != 0).unwrap_or(w.genres) & !ADULT_BIT;
            if genres == 0 {
                continue;
            }
            let age_days = (now_ms - w.ts_ms).max(0) as f64 / 86_400_000.0;
            let weight = w.engage * 0.5f64.powf(age_days / HALF_LIFE_DAYS).max(DECAY_FLOOR);
            if w.engage >= 0.5 {
                titles += 1;
            }
            total += weight;
            for (i, g) in genre_w.iter_mut().enumerate() {
                if genres & (1 << i) != 0 {
                    *g += weight;
                }
            }
            if let Some((_, t)) = found.filter(|(_, t)| t.start > 0) {
                let y = t.start as f64;
                ysum += weight * y;
                y2sum += weight * y * y;
                ywsum += weight;
            }
        }
        let year = (ywsum > 0.0).then(|| {
            let mean = ysum / ywsum;
            let sd = (y2sum / ywsum - mean * mean).max(0.0).sqrt();
            (mean, sd.max(12.0))
        });
        Profile { genre_w, total, titles, year }
    }

    /// Hay gustos suficientes para personalizar.
    pub fn ready(&self) -> bool {
        self.titles >= MIN_TITLES && self.total > 0.0
    }

    /// Qué parte de lo visto tiene ese género.
    fn share(&self, i: usize) -> f64 {
        if self.total > 0.0 {
            self.genre_w[i] / self.total
        } else {
            0.0
        }
    }

    /// Cuánto encaja un candidato (≈ 1 = como cualquiera; más = lo que más ve).
    fn affinity(&self, genres: u32, base: &[f64; NG]) -> f64 {
        let (mut sum, mut n) = (0.0, 0);
        for (i, &b) in base.iter().enumerate() {
            if genres & (1 << i) == 0 {
                continue;
            }
            let b = b.max(0.01);
            sum += ((self.share(i) + SMOOTH * b) / ((1.0 + SMOOTH) * b)).min(MAX_LIFT);
            n += 1;
        }
        if n == 0 {
            0.3
        } else {
            sum / n as f64
        }
    }

    /// Cerca de la época que suele ver: hasta +25 %.
    fn era(&self, start: u16) -> f64 {
        match self.year {
            Some((mean, sd)) if start > 0 => era(start, mean, sd),
            _ => 0.85,
        }
    }

    /// Los géneros que más pesan (para mostrar), de mayor a menor.
    pub fn top_genres(&self, n: usize) -> Vec<String> {
        let mut v: Vec<(usize, f64)> = (0..NG).map(|i| (i, self.share(i))).filter(|&(_, s)| s >= 0.2).collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().take(n).map(|(i, _)| GENRES[i].to_string()).collect()
    }
}

fn era(start: u16, mean: f64, sd: f64) -> f64 {
    let z = (start as f64 - mean) / sd;
    0.75 + 0.25 * (-z * z / 2.0).exp()
}

/// Rating y, un poco, votos: 0..1.
fn quality(t: &Title, min_votes: u32) -> f64 {
    let r = ((t.rating as f64 - 65.0) / 30.0).clamp(0.05, 1.0);
    let lo = (min_votes.max(1) as f64).ln();
    let v = (((t.votes.max(1) as f64).ln() - lo) / (1_000_000f64.ln() - lo)).clamp(0.0, 1.0);
    // Los votos pesan la mitad: con pocos votos (25 mil) y rating alto suelen ser títulos
    // inflados por campañas; probado con los datos reales, sin esto la fila se llenaba de esos.
    r * (0.5 + 0.5 * v)
}

/// Géneros de "formato" (animación, documental): o se ven o no se ven, más allá del tema.
fn format_mask() -> u32 {
    ["Animation", "Documentary"].iter().filter_map(|g| genre_bit(g)).fold(0, |m, b| m | b)
}

// ---------- azar con semilla ----------

/// splitmix64.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// FNV-1a (estable entre versiones de Rust, a diferencia de `DefaultHasher`).
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Semilla de una fila: usuario, tipo, bloque de rotación y el título de "Similares".
pub fn seed(uid: i64, kind: Kind, block: i64, like: Option<&str>) -> u64 {
    mix(mix(uid as u64) ^ mix(block as u64).rotate_left(17) ^ fnv(kind.api()) ^ like.map_or(0, fnv).rotate_left(31))
}

/// El bloque de rotación de un instante (unix s).
pub fn block_of(unix_s: i64) -> i64 {
    unix_s.div_euclid(ROTATE_S)
}

/// Uniforme en (0, 1), fijo para (semilla, id).
fn unit(seed: u64, id: &str) -> f64 {
    ((mix(seed ^ fnv(id)) >> 11) as f64 + 0.5) / (1u64 << 53) as f64
}

/// `n` de `items` por muestreo ponderado sin reemplazo (Efraimidis–Spirakis), en el orden del
/// sorteo. El peso es el puntaje al cuadrado: con el puntaje solo (probado con los datos reales)
/// uno de rating alto y pocos votos salía primero casi tan seguido como The Dark Knight. Con todos
/// iguales es un sorteo uniforme.
fn sample<'a>(items: &[(&'a Title, f64)], n: usize, seed: u64) -> Vec<&'a Title> {
    let mut keyed: Vec<(f64, &Title)> = items.iter().map(|&(t, w)| (unit(seed, &t.id).ln() / (w * w).max(1e-12), t)).collect();
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    keyed.into_iter().take(n).map(|(_, t)| t).collect()
}

// ---------- la selección ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Basis {
    /// Por los gustos del usuario.
    Taste,
    /// Sin gustos suficientes: al azar de los mejores del Top rated.
    Top,
    /// Parecidos a un título.
    Like,
}

/// El título de "Similares": sus géneros y su año.
#[derive(Clone, Debug)]
pub struct Like {
    pub id: String,
    pub genres: u32,
    pub start: u16,
}

pub fn min_votes(kind: Kind) -> u32 {
    match kind {
        Kind::Movie => DEFAULT_VOTES_MOVIE,
        Kind::Series => DEFAULT_VOTES_SERIES,
    }
}

/// La fila: hasta `limit` títulos de `kind` y en base a qué se eligieron.
pub fn pick<'a>(d: &'a Dataset, kind: Kind, h: &History, p: &Profile, like: Option<&Like>, seed: u64, limit: usize) -> (Basis, Vec<&'a Title>) {
    let min_votes = min_votes(kind);
    let list = d.list(kind);
    let fresh = |t: &Title| !h.seen.contains(&*t.id) && like.is_none_or(|l| *t.id != *l.id);
    let pool: Vec<&Title> = list.iter().filter(|t| t.votes >= min_votes && t.rating >= MIN_RATING && fresh(t)).collect();

    let fmt = format_mask();
    if let Some(l) = like.filter(|l| l.genres != 0) {
        let base = base_shares(&pool);
        let mut scored: Vec<(&Title, f64)> = pool
            .iter()
            .filter_map(|&t| {
                // Animado con animado, documental con documental (y nada de eso si el título no lo es).
                if (t.genres ^ l.genres) & fmt != 0 {
                    return None;
                }
                let jac = (t.genres & l.genres).count_ones() as f64 / (t.genres | l.genres).count_ones() as f64;
                if jac < 0.3 {
                    return None;
                }
                let era = if l.start > 0 && t.start > 0 { era(t.start, l.start as f64, 15.0) } else { 0.85 };
                // Los gustos acomodan un poco (±25 %), sin tapar el parecido: probado con Toy Story y
                // un usuario de policiales, con más peso se colaba The Big Lebowski.
                let taste = if p.ready() { p.affinity(t.genres, &base).clamp(0.5, 2.0).powf(0.3) } else { 1.0 };
                Some((t, (0.1 + jac).powi(2) * quality(t, min_votes) * era * taste))
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.id.cmp(&b.0.id)));
        scored.truncate(LIKE_POOL);
        return (Basis::Like, sample(&scored, limit, seed));
    }

    let mut out = Vec::new();
    let mut basis = Basis::Top;
    if p.ready() {
        basis = Basis::Taste;
        let base = base_shares(&pool);
        // Animación o documental que casi no ve: mucho menos (probado: a quien ve policiales le
        // llegaban Perfect Blue e Inside Job entre los primeros).
        let seen_fmt = (0..NG).filter(|&i| p.share(i) >= 0.1).fold(0, |m, i| m | (1 << i)) & fmt;
        let mut scored: Vec<(&Title, f64)> = pool
            .iter()
            .map(|&t| {
                let format = if t.genres & fmt & !seen_fmt != 0 { 0.3 } else { 1.0 };
                (t, p.affinity(t.genres, &base).powf(1.5) * quality(t, min_votes) * p.era(t.start) * format)
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.id.cmp(&b.0.id)));
        scored.truncate(POOL);
        out = sample(&scored, limit, seed);
    }
    if out.len() < limit {
        // Sin gustos (o no alcanzó): al azar de los 300 mejores del Top rated (ya ordenado por rating
        // y votos), sin lo ya visto ni lo ya elegido.
        let chosen: HashSet<&str> = out.iter().map(|t| &*t.id).collect();
        let top: Vec<(&Title, f64)> = list
            .iter()
            .filter(|t| t.votes >= min_votes)
            .take(TOP_FALLBACK)
            .filter(|t| fresh(t) && !chosen.contains(&*t.id))
            .map(|t| (t, 1.0))
            .collect();
        out.extend(sample(&top, limit - out.len(), seed));
    }
    (basis, out)
}

/// Qué parte de los candidatos tiene cada género.
fn base_shares(pool: &[&Title]) -> [f64; NG] {
    let mut base = [0.0; NG];
    for t in pool {
        for (i, b) in base.iter_mut().enumerate() {
            if t.genres & (1 << i) != 0 {
                *b += 1.0;
            }
        }
    }
    let n = pool.len().max(1) as f64;
    base.map(|c| c / n)
}

// ---------- ruta ----------

#[derive(Clone)]
pub struct RecCtx {
    pub db: Db,
    pub imdb: Imdb,
    /// Para la descripción y el logo del destacado (None en pruebas).
    pub cinemeta: Option<Arc<Cinemeta>>,
}

impl FromRef<AppState> for RecCtx {
    fn from_ref(s: &AppState) -> Self {
        RecCtx { db: s.db.clone(), imdb: s.imdb.clone(), cinemeta: Some(s.cinemeta.clone()) }
    }
}

pub fn routes<S>() -> Router<S>
where
    RecCtx: FromRef<S>,
    Auth: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/recommended/{type}", get(recommended::<S>))
}

#[derive(Deserialize)]
struct RecQuery {
    limit: Option<usize>,
    /// "Similares": id de IMDb del título.
    like: Option<String>,
    /// Sus géneros (por si no está en los datos de IMDb), separados por coma.
    genres: Option<String>,
}

#[derive(Serialize)]
struct RecBody {
    state: TopState,
    basis: Option<Basis>,
    items: Vec<MetaPreview>,
    /// Los géneros que más pesan en los gustos (vacío sin gustos).
    genres: Vec<String>,
    /// unix s en que cambia la selección.
    rotates_at: i64,
}

async fn recommended<S>(State(ctx): State<RecCtx>, u: AuthUser, Path(kind): Path<String>, Query(q): Query<RecQuery>) -> ApiResult<Json<RecBody>>
where
    RecCtx: FromRef<S>,
    Auth: FromRef<S>,
    S: Send + Sync,
{
    check_type(&kind)?;
    let kind = Kind::from_api(&kind);
    let like_id = q.like.filter(|s| !s.is_empty());
    if let Some(id) = &like_id {
        check_id(id)?;
    }
    let limit = q.limit.unwrap_or(LIMIT_DEFAULT).clamp(1, LIMIT_MAX);
    let now = now_unix();
    let block = block_of(now);
    let rotates_at = (block + 1) * ROTATE_S;
    let d = match ctx.imdb.dataset() {
        Ok(d) => d,
        Err(state) => return Ok(Json(RecBody { state, basis: None, items: vec![], genres: vec![], rotates_at })),
    };
    let uid = u.user.id;
    let h = ctx.db.call(move |c| Ok(load_history(c, uid)?)).await?;
    let p = Profile::build(&h, &d, now_ms());
    let like = like_id.map(|id| {
        let hint = q.genres.as_deref().unwrap_or("").split(',').filter_map(|g| genre_bit(g.trim())).fold(0, |m, b| m | b);
        match d.find(&id) {
            Some((_, t)) => Like { genres: if t.genres != 0 { t.genres } else { hint }, start: t.start, id },
            None => Like { genres: hint, start: 0, id },
        }
    });
    let (basis, picks) = pick(&d, kind, &h, &p, like.as_ref(), seed(uid, kind, block, like.as_ref().map(|l| l.id.as_str())), limit);
    let mut items: Vec<MetaPreview> = picks.iter().map(|t| t.preview(kind)).collect();
    if basis != Basis::Like {
        if let Some(c) = &ctx.cinemeta {
            describe(c, &mut items).await;
        }
    }
    let genres = if p.ready() { p.top_genres(5) } else { vec![] };
    Ok(Json(RecBody { state: TopState::Ready, basis: Some(basis), items, genres, rotates_at }))
}

/// Descripción y logo de Cinemeta para los primeros (el destacado del inicio los muestra). Se
/// espera hasta `DESCRIBE_WAIT`; lo que no llegó sigue bajando y queda en la caché de Cinemeta
/// para la próxima vez.
async fn describe(c: &Arc<Cinemeta>, items: &mut [MetaPreview]) {
    let n = items.len().min(HERO);
    if n == 0 {
        return;
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel(n);
    for (i, m) in items.iter().take(n).enumerate() {
        let (c, tx, kind, id) = (c.clone(), tx.clone(), m.kind.clone(), m.id.clone());
        tokio::spawn(async move {
            if let Ok(meta) = c.meta(&kind, &id).await {
                tx.send((i, meta)).await.ok();
            }
        });
    }
    drop(tx);
    let _ = tokio::time::timeout(DESCRIBE_WAIT, async {
        while let Some((i, meta)) = rx.recv().await {
            let m = &mut items[i];
            m.description = meta.preview.description.clone().or(m.description.take());
            m.logo = meta.preview.logo.clone().or(m.logo.take());
            if m.background.is_none() {
                m.background = meta.preview.background.clone();
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{create_user, routes as auth_routes};
    use axum::{body::Body, http::Request, http::StatusCode};
    use tower::ServiceExt;

    fn mask(gs: &[&str]) -> u32 {
        gs.iter().map(|g| genre_bit(g).unwrap()).fold(0, |m, b| m | b)
    }

    fn title(id: String, genres: u32, rating: u8, votes: u32, start: u16) -> Title {
        Title { name: format!("T {id}").into(), id: id.into(), start, end: 0, rating, votes, genres }
    }

    /// 400 películas y 120 series: de cada grupo de géneros, ratings de 7.0 a 9.4 con muchos votos,
    /// más algunas flojas o con pocos votos que nunca tienen que salir.
    fn dataset() -> Dataset {
        let groups = [
            mask(&["Crime", "Drama"]),
            mask(&["Crime", "Thriller"]),
            mask(&["Comedy"]),
            mask(&["Comedy", "Romance"]),
            mask(&["Animation", "Family"]),
            mask(&["Horror"]),
            mask(&["Drama", "Romance"]),
            mask(&["Action", "Sci-Fi"]),
        ];
        let mut movies = Vec::new();
        for i in 0..400u32 {
            let g = groups[i as usize % groups.len()];
            movies.push(title(format!("tt1{i:06}"), g, 70 + (i % 25) as u8, 30_000 + i * 2_000, 1960 + (i % 60) as u16));
        }
        // Flojas o con pocos votos.
        for i in 0..40u32 {
            movies.push(title(format!("tt2{i:06}"), groups[0], 60, 500_000, 2000));
            movies.push(title(format!("tt3{i:06}"), groups[0], 95, 5_000, 2000));
        }
        let mut series = Vec::new();
        for i in 0..120u32 {
            let g = groups[i as usize % groups.len()];
            series.push(title(format!("tt4{i:06}"), g, 70 + (i % 25) as u8, 20_000 + i * 1_000, 1990 + (i % 30) as u16));
        }
        Dataset::from_lists(movies, series, 1_700_000_000)
    }

    const DAY: i64 = 86_400_000;
    const NOW: i64 = 1_800_000_000_000;

    fn db_with_user() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn.lock().unwrap().execute("INSERT INTO users (id, username, password_hash) VALUES (1, 'ana', 'h')", []).unwrap();
        db
    }

    fn progress(db: &Db, meta_id: &str, kind: &str, series_id: Option<&str>, (pos, dur): (f64, f64), ts: i64, deleted: bool) {
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO progress (user_id, meta_id, type, series_id, name, position_s, duration_s, updated_at, updated_ms, deleted)
                 VALUES (1, ?1, ?2, ?3, 'x', ?4, ?5, '2026-01-01T00:00:00Z', ?6, ?7)",
                params![meta_id, kind, series_id, pos, dur, ts, deleted],
            )
            .unwrap();
    }

    fn library(db: &Db, kind: &str, id: &str, genres: &[&str], ts: i64) {
        let data = serde_json::json!({"id": id, "type": kind, "name": "x", "genres": genres}).to_string();
        db.conn
            .lock()
            .unwrap()
            .execute("INSERT INTO library (user_id, type, id, data, updated_ms) VALUES (1, ?1, ?2, ?3, ?4)", params![kind, id, data, ts])
            .unwrap();
    }

    fn history(db: &Db) -> History {
        load_history(&db.conn.lock().unwrap(), 1).unwrap()
    }

    /// Ids de las películas de un grupo de géneros (del dataset de prueba).
    fn of_group(d: &Dataset, g: u32, n: usize) -> Vec<String> {
        d.movies.iter().filter(|t| t.genres == g && t.votes >= DEFAULT_VOTES_MOVIE).take(n).map(|t| t.id.to_string()).collect()
    }

    #[test]
    fn history_signals() {
        let db = db_with_user();
        // Terminada, a medias, apenas empezada, sacada de "Seguir viendo".
        progress(&db, "tt1", "movie", None, (5000.0, 6000.0), NOW - DAY, false);
        progress(&db, "tt2", "movie", None, (2000.0, 6000.0), NOW - DAY, false);
        progress(&db, "tt3", "movie", None, (60.0, 6000.0), NOW - DAY, false);
        progress(&db, "tt4", "movie", None, (5900.0, 6000.0), NOW - DAY, true);
        // Una serie con tres episodios vistos y otra con uno empezado.
        for e in 1..=3 {
            progress(&db, &format!("tt5:1:{e}"), "series", Some("tt5"), (2500.0, 2600.0), NOW - e * DAY, false);
        }
        progress(&db, "tt6:1:1", "series", Some("tt6"), (100.0, 2600.0), NOW, false);
        // En la biblioteca: una no vista (con sus géneros) y una vista.
        library(&db, "movie", "tt7", &["Western"], NOW);
        library(&db, "movie", "tt1", &["Drama"], NOW);

        let h = history(&db);
        let get = |id: &str| h.watched.iter().find(|w| w.id == id).cloned();
        assert_eq!(get("tt1").map(|w| w.engage), Some(1.2), "vista + biblioteca");
        assert_eq!(get("tt2").map(|w| w.engage), Some(0.5));
        assert_eq!(get("tt3"), None, "apenas empezada no dice nada");
        assert_eq!(get("tt4"), None, "sacada de Seguir viendo");
        assert_eq!(get("tt5").map(|w| (w.engage * 100.0).round() / 100.0), Some(1.3));
        assert_eq!(get("tt5").map(|w| w.ts_ms), Some(NOW - DAY), "lo último de la serie");
        assert_eq!(get("tt6"), None);
        let w7 = get("tt7").unwrap();
        assert_eq!((w7.engage, w7.genres), (LIBRARY, mask(&["Western"])));
        // Todo lo que tiene progreso (también lo sacado) o está en la biblioteca no se recomienda.
        for id in ["tt1", "tt2", "tt3", "tt4", "tt5", "tt6", "tt7"] {
            assert!(h.seen.contains(id), "{id}");
        }
        assert!(!h.seen.contains("tt5:1:1"));
    }

    #[test]
    fn profile_weights_genres_and_recency() {
        let d = dataset();
        let crime = of_group(&d, mask(&["Crime", "Drama"]), 3);
        let comedy = of_group(&d, mask(&["Comedy"]), 3);
        let db = db_with_user();
        // Tres policiales vistos hace poco y tres comedias hace dos años.
        for id in &crime {
            progress(&db, id, "movie", None, (5000.0, 6000.0), NOW - 3 * DAY, false);
        }
        for id in &comedy {
            progress(&db, id, "movie", None, (5000.0, 6000.0), NOW - 730 * DAY, false);
        }
        let p = Profile::build(&history(&db), &d, NOW);
        assert!(p.ready());
        assert_eq!(p.titles, 6);
        let crime_i = GENRES.iter().position(|g| *g == "Crime").unwrap();
        let comedy_i = GENRES.iter().position(|g| *g == "Comedy").unwrap();
        // Lo viejo pesa la cuarta parte (el piso): ~0,98 contra 0,25 por título.
        assert!(p.share(crime_i) > 0.75, "{}", p.share(crime_i));
        assert!(p.share(comedy_i) < 0.25, "{}", p.share(comedy_i));
        assert_eq!(p.top_genres(5), ["Crime", "Drama", "Comedy"], "por peso, lo viejo al final");
        // Afinidad: un policial encaja más que una comedia, y una de terror (nunca vista) casi nada.
        let base = base_shares(&d.movies.iter().collect::<Vec<_>>());
        let a = |g: &[&str]| p.affinity(mask(g), &base);
        assert!(a(&["Crime", "Thriller"]) > a(&["Comedy"]));
        assert!(a(&["Comedy"]) > a(&["Horror"]));
        assert!(a(&["Horror"]) < 0.3);

        // Dos títulos no alcanzan para tener gustos.
        let db = db_with_user();
        for id in &crime[..2] {
            progress(&db, id, "movie", None, (5000.0, 6000.0), NOW, false);
        }
        assert!(!Profile::build(&history(&db), &d, NOW).ready());
    }

    #[test]
    fn taste_picks_match_and_skip_seen() {
        let d = dataset();
        let crime = of_group(&d, mask(&["Crime", "Drama"]), 4);
        let db = db_with_user();
        for id in &crime[..3] {
            progress(&db, id, "movie", None, (5000.0, 6000.0), NOW, false);
        }
        // En "Seguir viendo" (a medias) y en la biblioteca: no se recomiendan.
        progress(&db, &crime[3], "movie", None, (600.0, 6000.0), NOW, false);
        let thriller = of_group(&d, mask(&["Crime", "Thriller"]), 1);
        library(&db, "movie", &thriller[0], &["Crime", "Thriller"], NOW);
        let h = history(&db);
        let p = Profile::build(&h, &d, NOW);
        let (basis, picks) = pick(&d, Kind::Movie, &h, &p, None, seed(1, Kind::Movie, 7, None), 20);
        assert_eq!(basis, Basis::Taste);
        assert_eq!(picks.len(), 20);
        let crime_bit = genre_bit("Crime").unwrap();
        let crime_n = picks.iter().filter(|t| t.genres & crime_bit != 0).count();
        // Un cuarto de los candidatos es policial: tiene que ser la gran mayoría.
        assert!(crime_n >= 14, "policiales: {crime_n} de 20 ({:?})", picks.iter().map(|t| crate::imdb::genre_names(t.genres)).collect::<Vec<_>>());
        for t in &picks {
            assert!(!h.seen.contains(&*t.id), "{} ya visto", t.id);
            assert!(t.rating >= MIN_RATING && t.votes >= DEFAULT_VOTES_MOVIE, "{t:?}");
        }
        let ids: HashSet<&str> = picks.iter().map(|t| &*t.id).collect();
        assert_eq!(ids.len(), 20, "sin repetidos");

        // Las series salen por los mismos gustos (vio películas policiales).
        let (basis, series) = pick(&d, Kind::Series, &h, &p, None, seed(1, Kind::Series, 7, None), 20);
        assert_eq!(basis, Basis::Taste);
        assert!(series.iter().filter(|t| t.genres & crime_bit != 0).count() >= 12);
    }

    #[test]
    fn rotates_by_block_and_is_stable_inside_one() {
        let d = dataset();
        let db = db_with_user();
        for id in of_group(&d, mask(&["Animation", "Family"]), 3) {
            progress(&db, &id, "movie", None, (5000.0, 6000.0), NOW, false);
        }
        let h = history(&db);
        let p = Profile::build(&h, &d, NOW);
        let ids = |block| -> Vec<String> {
            pick(&d, Kind::Movie, &h, &p, None, seed(1, Kind::Movie, block, None), 20).1.iter().map(|t| t.id.to_string()).collect()
        };
        assert_eq!(ids(100), ids(100), "mismo bloque, misma fila");
        let (a, b) = (ids(100), ids(101));
        assert_ne!(a, b);
        let common = a.iter().filter(|x| b.contains(x)).count();
        assert!(common < 16, "casi la misma fila de un bloque al otro: {common} de 20");
        // Otro usuario con los mismos gustos ve otra cosa.
        let other: Vec<String> = pick(&d, Kind::Movie, &h, &p, None, seed(2, Kind::Movie, 100, None), 20).1.iter().map(|t| t.id.to_string()).collect();
        assert_ne!(a, other);
        // El bloque cambia cada 6 h.
        assert_eq!(block_of(ROTATE_S * 10), block_of(ROTATE_S * 11 - 1));
        assert_ne!(block_of(ROTATE_S * 10), block_of(ROTATE_S * 11));
    }

    #[test]
    fn cold_start_is_random_from_top_300() {
        let d = dataset();
        let top300: Vec<&str> = d.movies.iter().filter(|t| t.votes >= DEFAULT_VOTES_MOVIE).take(TOP_FALLBACK).map(|t| &*t.id).collect();
        let db = db_with_user();
        // Dos vistas: no alcanza.
        let seen: Vec<String> = top300[..2].iter().map(|s| s.to_string()).collect();
        for id in &seen {
            progress(&db, id, "movie", None, (5000.0, 6000.0), NOW, false);
        }
        let h = history(&db);
        let p = Profile::build(&h, &d, NOW);
        assert!(!p.ready());
        let run = |block| pick(&d, Kind::Movie, &h, &p, None, seed(1, Kind::Movie, block, None), 20);
        let (basis, picks) = run(5);
        assert_eq!(basis, Basis::Top);
        assert_eq!(picks.len(), 20);
        for t in &picks {
            assert!(top300.contains(&&*t.id), "{} no está entre los 300 mejores", t.id);
            assert!(!seen.contains(&t.id.to_string()));
        }
        // Al azar: no son los primeros 20 ni en orden de rating, y cambia con el bloque.
        let first: Vec<&str> = top300[2..22].to_vec();
        assert_ne!(picks.iter().map(|t| &*t.id).collect::<Vec<_>>(), first);
        assert_ne!(run(5).1.iter().map(|t| t.id.clone()).collect::<Vec<_>>(), run(6).1.iter().map(|t| t.id.clone()).collect::<Vec<_>>());
        // Sin nada visto, igual (usuario nuevo).
        let (basis, picks) = pick(&d, Kind::Series, &History::default(), &Profile::build(&History::default(), &d, NOW), None, 9, 20);
        assert_eq!((basis, picks.len()), (Basis::Top, 20));
    }

    #[test]
    fn like_finds_similar() {
        let d = dataset();
        let src = of_group(&d, mask(&["Animation", "Family"]), 1).remove(0);
        let (_, t) = d.find(&src).unwrap();
        let like = Like { id: src.clone(), genres: t.genres, start: t.start };
        let h = History::default();
        let p = Profile::build(&h, &d, NOW);
        let (basis, picks) = pick(&d, Kind::Movie, &h, &p, Some(&like), seed(1, Kind::Movie, 1, Some(&src)), 20);
        assert_eq!(basis, Basis::Like);
        assert_eq!(picks.len(), 20);
        let anim = genre_bit("Animation").unwrap();
        assert!(picks.iter().all(|t| t.genres & anim != 0 && *t.id != *src), "solo animadas");
        // Y a una que no es animada no le tocan animadas.
        let crime = of_group(&d, mask(&["Crime", "Drama"]), 1).remove(0);
        let like = Like { id: crime.clone(), genres: mask(&["Crime", "Drama"]), start: 2000 };
        let (_, picks) = pick(&d, Kind::Movie, &h, &p, Some(&like), 3, 20);
        assert!(!picks.is_empty() && picks.iter().all(|t| t.genres & anim == 0 && t.genres & like.genres != 0));
        // Sin géneros conocidos: la fila normal.
        let unknown = Like { id: "tt9".into(), genres: 0, start: 0 };
        assert_eq!(pick(&d, Kind::Movie, &h, &p, Some(&unknown), 1, 20).0, Basis::Top);
    }

    /// Para afinar con los datos reales: `MYTHOS_REAL_IMDB=<data>/imdb/top-v1.tsv.gz cargo test -p
    /// mythos --lib real_data -- --ignored --nocapture`. Muestra la fila de alguien que vio tres
    /// policiales y Breaking Bad en cuatro bloques seguidos y cuánto se repite de uno al otro.
    #[test]
    #[ignore]
    fn real_data() {
        let path = std::env::var("MYTHOS_REAL_IMDB").expect("MYTHOS_REAL_IMDB");
        let d = Dataset::read_from(std::fs::File::open(path).unwrap()).unwrap();
        let db = db_with_user();
        for id in ["tt0068646", "tt0099685", "tt0110912"] {
            progress(&db, id, "movie", None, (5000.0, 5200.0), NOW - 2 * DAY, false);
        }
        progress(&db, "tt0903747:1:1", "series", Some("tt0903747"), (3000.0, 3100.0), NOW, false);
        let h = history(&db);
        let p = Profile::build(&h, &d, NOW);
        for kind in [Kind::Movie, Kind::Series] {
            let mut prev: Option<Vec<String>> = None;
            for block in 0..4 {
                let (basis, picks) = pick(&d, kind, &h, &p, None, seed(1, kind, block, None), 20);
                let ids: Vec<String> = picks.iter().map(|t| t.id.to_string()).collect();
                let common = prev.as_ref().map_or(0, |p| ids.iter().filter(|x| p.contains(x)).count());
                println!("{kind:?} bloque {block} ({basis:?}, {common} repetidos del anterior):");
                for t in &picks {
                    println!("  {} {} {} {} [{}]", t.id, t.rating, t.votes, t.name, crate::imdb::genre_names(t.genres).join(","));
                }
                prev = Some(ids);
            }
        }
    }

    // ---------- la ruta ----------

    #[derive(Clone)]
    struct TestState {
        auth: Auth,
        rec: RecCtx,
    }
    impl FromRef<TestState> for Auth {
        fn from_ref(s: &TestState) -> Self {
            s.auth.clone()
        }
    }
    impl FromRef<TestState> for RecCtx {
        fn from_ref(s: &TestState) -> Self {
            s.rec.clone()
        }
    }

    async fn setup(imdb: Imdb) -> (Router, String, Db) {
        let db = Db::open_in_memory().unwrap();
        db.call(|c| create_user(c, "bob", "hunter22", false).map(|_| ())).await.unwrap();
        let st = TestState { auth: Auth::new(db.clone()), rec: RecCtx { db: db.clone(), imdb, cinemeta: None } };
        let app = Router::new().nest("/api", auth_routes::<TestState>().merge(routes()).with_state(st));
        let req = Request::post("/api/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"username":"bob","password":"hunter22"}"#))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
        (app, v["token"].as_str().unwrap().to_string(), db)
    }

    async fn get(app: &Router, uri: &str, token: Option<&str>) -> (StatusCode, serde_json::Value) {
        let mut b = Request::get(uri);
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        let res = app.clone().oneshot(b.body(Body::empty()).unwrap()).await.unwrap();
        let st = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        (st, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    #[tokio::test]
    async fn route() {
        let d = dataset();
        let crime = of_group(&d, mask(&["Crime", "Drama"]), 3);
        let (app, token, db) = setup(Imdb::with_data(d)).await;
        let t = Some(token.as_str());
        let (st, _) = get(&app, "/api/recommended/movie", None).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);

        // Usuario nuevo: del Top rated.
        let (st, v) = get(&app, "/api/recommended/movie", t).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert_eq!((v["state"].as_str(), v["basis"].as_str()), (Some("ready"), Some("top")));
        assert_eq!(v["items"].as_array().unwrap().len(), LIMIT_DEFAULT);
        assert_eq!(v["genres"], serde_json::json!([]));
        assert!(v["rotates_at"].as_i64().unwrap() > now_unix());
        let item = &v["items"][0];
        assert_eq!(item["type"], "movie");
        assert!(item["poster"].as_str().unwrap().contains("metahub"));

        // Con tres vistas: por gustos.
        let uid = db.call(|c| Ok(c.query_row("SELECT id FROM users WHERE username = 'bob'", [], |r| r.get::<_, i64>(0))?)).await.unwrap();
        for id in &crime {
            let id = id.clone();
            db.call(move |c| {
                Ok(c.execute(
                    "INSERT INTO progress (user_id, meta_id, type, name, position_s, duration_s, updated_at, updated_ms)
                     VALUES (?1, ?2, 'movie', 'x', 5000, 6000, '2026-01-01T00:00:00Z', ?3)",
                    params![uid, id, now_ms()],
                )?)
            })
            .await
            .unwrap();
        }
        let (_, v) = get(&app, "/api/recommended/movie?limit=5", t).await;
        assert_eq!(v["basis"], "taste");
        assert_eq!(v["items"].as_array().unwrap().len(), 5);
        assert_eq!(v["genres"], serde_json::json!(["Crime", "Drama"]));
        let got: Vec<&str> = v["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
        assert!(got.iter().all(|id| !crime.iter().any(|c| c == id)), "recomendó algo ya visto");
        let (_, v) = get(&app, "/api/recommended/movie?limit=999", t).await;
        assert_eq!(v["items"].as_array().unwrap().len(), LIMIT_MAX);

        // Similares.
        let (_, v) = get(&app, &format!("/api/recommended/movie?like={}", crime[0]), t).await;
        assert_eq!(v["basis"], "like");
        let (st, _) = get(&app, "/api/recommended/movie?like=no%20valido!", t).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        let (st, _) = get(&app, "/api/recommended/anime", t).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);

        // Sin datos de IMDb (modo local): responde igual, sin títulos.
        let (app, token, _) = setup(Imdb::new(std::path::Path::new("."), reqwest::Client::new(), false)).await;
        let (st, v) = get(&app, "/api/recommended/series", Some(&token)).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!((v["state"].as_str(), v["basis"].is_null()), (Some("unavailable"), true));
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
    }
}
