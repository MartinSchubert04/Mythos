//! "Top rated": lo mejor calificado de IMDb por tipo, con uno o más géneros a la vez (todos
//! tienen que coincidir) y un mínimo de votos (docs/API.md, "Top rated").
//!
//! **De dónde sale.** De los datasets públicos de IMDb (`datasets.imdbws.com`, uso personal y no
//! comercial, sin clave): `title.ratings.tsv.gz` (~9 MB: rating y votos de cada título) y
//! `title.basics.tsv.gz` (~230 MB: tipo, nombre, años y hasta 3 géneros). Se eligieron porque
//! el catálogo `imdbRating` de Cinemeta no está ordenado por rating (se parece a "popular") y
//! acepta un solo género; los addons de terceros con "IMDb Top" dependen de un server ajeno.
//!
//! **Cómo.** Solo en modo server (el embebido de las apps no baja 230 MB: responde
//! `unavailable` y la UI usa el server de casa). Al arrancar se lee lo guardado; si no hay nada
//! se baja a los ~20 s (o enseguida si alguien pide la lista), y después cada 7 días (si falla,
//! se reintenta a las 6 h y se sigue con lo que había). La bajada se procesa en streaming (la
//! descompresión en un hilo bloqueante que lee de un canal acotado): nunca se guarda ni se tiene
//! en memoria el archivo grande. Quedan solo películas (`movie`) y series (`tvSeries`,
//! `tvMiniSeries`) no adultas con ≥ [`FLOOR_VOTES`] votos (~50 mil títulos, ~1,5 MB) en
//! `<data_dir>/imdb/top-v1.tsv.gz`. Pósters y fondos: los de metahub por id de IMDb (los mismos
//! que usa Cinemeta). Las páginas ya serializadas se cachean 1 h.
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path as FsPath, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use anyhow::Context;
use axum::{
    body::Bytes,
    extract::{FromRef, State},
    http::header,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{Auth, AuthUser},
    cache::TtlCache,
    cinemeta::{check_type, MetaPreview},
    db::now_unix,
    error::{ApiError, ApiResult, Path, Query},
};

const DATASETS: &str = "https://datasets.imdbws.com";
/// Lo mínimo que se guarda (y el piso de `min_votes`).
pub const FLOOR_VOTES: u32 = 1000;
/// `min_votes` por defecto (el de la Top 250 de IMDb para películas).
pub const DEFAULT_VOTES_MOVIE: u32 = 25_000;
pub const DEFAULT_VOTES_SERIES: u32 = 10_000;
/// IMDb da como mucho 3 géneros por título: más no puede coincidir nunca.
pub const MAX_GENRES: usize = 3;
const PAGE_DEFAULT: usize = 50;
const PAGE_MAX: usize = 100;
const SKIP_MAX: usize = 20_000;
const REFRESH_EVERY: Duration = Duration::from_secs(7 * 24 * 3600);
const RETRY_AFTER: Duration = Duration::from_secs(6 * 3600);
const FILE: &str = "top-v1.tsv.gz";
const MAGIC: &str = "mythos-imdb";

/// Géneros de IMDb (el bit de cada uno es su posición). "Adult" no se ofrece: esos títulos se
/// descartan al bajar.
pub const GENRES: [&str; 28] = [
    "Action", "Adult", "Adventure", "Animation", "Biography", "Comedy", "Crime", "Documentary", "Drama", "Family", "Fantasy",
    "Film-Noir", "Game-Show", "History", "Horror", "Music", "Musical", "Mystery", "News", "Reality-TV", "Romance", "Sci-Fi",
    "Short", "Sport", "Talk-Show", "Thriller", "War", "Western",
];
const ADULT_BIT: u32 = 1 << 1;

fn genre_bit(name: &str) -> Option<u32> {
    GENRES.iter().position(|g| g.eq_ignore_ascii_case(name)).map(|i| 1 << i)
}

fn genre_names(mask: u32) -> Vec<String> {
    GENRES.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, g)| g.to_string()).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Movie,
    Series,
}

impl Kind {
    fn api(self) -> &'static str {
        match self {
            Kind::Movie => "movie",
            Kind::Series => "series",
        }
    }
    fn code(self) -> &'static str {
        match self {
            Kind::Movie => "m",
            Kind::Series => "s",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Title {
    pub id: Box<str>,
    pub name: Box<str>,
    pub start: u16,
    /// 0 = sigue (series) o no aplica (películas).
    pub end: u16,
    /// Rating × 10 (87 = 8.7).
    pub rating: u8,
    pub votes: u32,
    pub genres: u32,
}

impl Title {
    fn year(&self, kind: Kind) -> Option<String> {
        if self.start == 0 {
            return None;
        }
        Some(match kind {
            Kind::Movie => self.start.to_string(),
            Kind::Series if self.end == self.start => self.start.to_string(),
            Kind::Series if self.end == 0 => format!("{}-", self.start),
            Kind::Series => format!("{}-{}", self.start, self.end),
        })
    }

    fn preview(&self, kind: Kind) -> MetaPreview {
        let id = &*self.id;
        MetaPreview {
            id: id.to_string(),
            kind: kind.api().to_string(),
            name: self.name.to_string(),
            poster: Some(format!("https://images.metahub.space/poster/small/{id}/img")),
            background: Some(format!("https://images.metahub.space/background/medium/{id}/img")),
            year: self.year(kind),
            imdb_rating: Some(format!("{}.{}", self.rating / 10, self.rating % 10)),
            genres: Some(genre_names(self.genres)),
            ..Default::default()
        }
    }
}

/// Lo guardado: cada lista ordenada por rating y después por votos (de mayor a menor).
#[derive(Debug, Default, PartialEq)]
pub struct Dataset {
    pub movies: Vec<Title>,
    pub series: Vec<Title>,
    /// unix s de la bajada.
    pub updated_at: i64,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct GenreCount {
    pub name: String,
    /// Cuántos títulos quedarían agregando este género al filtro (los elegidos: el total).
    pub count: usize,
}

#[derive(Debug)]
pub struct Page {
    pub items: Vec<MetaPreview>,
    pub total: usize,
    pub genres: Vec<GenreCount>,
}

impl Dataset {
    fn list(&self, kind: Kind) -> &[Title] {
        match kind {
            Kind::Movie => &self.movies,
            Kind::Series => &self.series,
        }
    }

    fn sort(&mut self) {
        for l in [&mut self.movies, &mut self.series] {
            l.sort_by(|a, b| b.rating.cmp(&a.rating).then(b.votes.cmp(&a.votes)).then(a.id.cmp(&b.id)));
        }
    }

    /// Una página de `kind` con todos los géneros de `mask` y al menos `min_votes` votos.
    pub fn query(&self, kind: Kind, mask: u32, min_votes: u32, skip: usize, limit: usize) -> Page {
        let mut counts = [0usize; GENRES.len()];
        let mut present = 0u32;
        let mut total = 0;
        let mut items = Vec::new();
        for t in self.list(kind) {
            if t.votes < min_votes {
                continue;
            }
            // Los géneros que existen con ese mínimo de votos (los demás no se ofrecen).
            present |= t.genres;
            if t.genres & mask != mask {
                continue;
            }
            if total >= skip && items.len() < limit {
                items.push(t.preview(kind));
            }
            total += 1;
            for (i, c) in counts.iter_mut().enumerate() {
                if t.genres & (1 << i) != 0 {
                    *c += 1;
                }
            }
        }
        let genres = GENRES
            .iter()
            .enumerate()
            .filter(|(i, _)| (present | mask) & (1 << i) != 0 && (1 << i) != ADULT_BIT)
            .map(|(i, g)| GenreCount { name: g.to_string(), count: counts[i] })
            .collect();
        Page { items, total, genres }
    }

    /// Formato compacto (TSV con gzip): una cabecera y una línea por título.
    pub fn write_to(&self, w: impl Write) -> anyhow::Result<()> {
        let mut gz = flate2::write::GzEncoder::new(std::io::BufWriter::new(w), flate2::Compression::default());
        writeln!(gz, "{MAGIC}\t1\t{}", self.updated_at)?;
        for (kind, list) in [(Kind::Movie, &self.movies), (Kind::Series, &self.series)] {
            for t in list {
                writeln!(gz, "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", kind.code(), t.id, t.rating, t.votes, t.start, t.end, t.genres, t.name)?;
            }
        }
        gz.finish()?.flush()?;
        Ok(())
    }

    pub fn read_from(r: impl Read) -> anyhow::Result<Self> {
        let r = BufReader::new(flate2::read::GzDecoder::new(r));
        let mut lines = r.lines();
        let head = lines.next().context("archivo vacío")??;
        let mut h = head.split('\t');
        anyhow::ensure!(h.next() == Some(MAGIC) && h.next() == Some("1"), "cabecera desconocida");
        let mut d = Dataset { updated_at: h.next().and_then(|s| s.parse().ok()).unwrap_or(0), ..Default::default() };
        for line in lines {
            let line = line?;
            let f: Vec<&str> = line.splitn(8, '\t').collect();
            let [kind, id, rating, votes, start, end, genres, name] = f[..] else {
                anyhow::bail!("línea inválida: {line:?}");
            };
            let t = Title {
                id: id.into(),
                name: name.into(),
                start: start.parse()?,
                end: end.parse()?,
                rating: rating.parse()?,
                votes: votes.parse()?,
                genres: genres.parse()?,
            };
            match kind {
                "m" => d.movies.push(t),
                "s" => d.series.push(t),
                _ => anyhow::bail!("tipo inválido: {kind:?}"),
            }
        }
        d.sort();
        Ok(d)
    }
}

// ---------- los datasets de IMDb ----------

/// Rating × 10 y votos de cada título con al menos `min_votes` (de `title.ratings.tsv`:
/// `tconst  averageRating  numVotes`).
pub fn parse_ratings(r: impl BufRead, min_votes: u32) -> anyhow::Result<HashMap<Box<str>, (u8, u32)>> {
    let mut out = HashMap::new();
    for line in r.lines().skip(1) {
        let line = line?;
        let mut f = line.split('\t');
        let (Some(id), Some(rating), Some(votes)) = (f.next(), f.next(), f.next()) else { continue };
        let Ok(votes) = votes.trim().parse::<u32>() else { continue };
        if votes < min_votes {
            continue;
        }
        let Ok(rating) = rating.parse::<f32>() else { continue };
        out.insert(id.into(), ((rating * 10.0).round().clamp(0.0, 100.0) as u8, votes));
    }
    Ok(out)
}

/// Cruza `title.basics.tsv` (`tconst titleType primaryTitle originalTitle isAdult startYear
/// endYear runtimeMinutes genres`) con los ratings. Las líneas sin rating se saltean sin
/// partirlas (son la enorme mayoría: episodios, cortos, videos).
pub fn parse_basics(mut r: impl BufRead, ratings: &HashMap<Box<str>, (u8, u32)>) -> anyhow::Result<Dataset> {
    let mut d = Dataset::default();
    let mut buf = Vec::with_capacity(512);
    loop {
        buf.clear();
        if r.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        let line = buf.strip_suffix(b"\n").unwrap_or(&buf);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(tab) = line.iter().position(|&b| b == b'\t') else { continue };
        let Ok(id) = std::str::from_utf8(&line[..tab]) else { continue };
        let Some(&(rating, votes)) = ratings.get(id) else { continue };
        let text = String::from_utf8_lossy(line);
        let f: Vec<&str> = text.split('\t').collect();
        if f.len() < 9 {
            continue;
        }
        let kind = match f[1] {
            "movie" => Kind::Movie,
            "tvSeries" | "tvMiniSeries" => Kind::Series,
            _ => continue,
        };
        let genres = f[8].split(',').filter_map(genre_bit).fold(0, |m, b| m | b);
        if f[4] == "1" || genres & ADULT_BIT != 0 {
            continue;
        }
        let year = |s: &str| s.parse::<u16>().unwrap_or(0);
        let start = year(f[5]);
        let mut end = year(f[6]);
        // Una miniserie sin año de fin es de un solo año (no "sigue").
        if kind == Kind::Series && end == 0 && f[1] == "tvMiniSeries" {
            end = start;
        }
        let name = f[2].trim();
        if name.is_empty() {
            continue;
        }
        let t = Title { id: id.into(), name: name.replace(['\t', '\n'], " ").into(), start, end, rating, votes, genres };
        match kind {
            Kind::Movie => d.movies.push(t),
            Kind::Series => d.series.push(t),
        }
    }
    d.sort();
    Ok(d)
}

/// `Read` sobre los pedazos que llegan por un canal (la bajada async → el hilo que descomprime).
struct ChanReader {
    rx: tokio::sync::mpsc::Receiver<Bytes>,
    cur: Bytes,
}

impl Read for ChanReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.cur.is_empty() {
            match self.rx.blocking_recv() {
                Some(b) => self.cur = b,
                None => return Ok(0),
            }
        }
        let n = buf.len().min(self.cur.len());
        buf[..n].copy_from_slice(&self.cur[..n]);
        self.cur = self.cur.slice(n..);
        Ok(n)
    }
}

/// Baja un `.tsv.gz` y lo procesa con `f` a medida que llega (sin archivo temporal).
async fn stream_gz<T: Send + 'static>(
    http: &reqwest::Client,
    url: &str,
    f: impl FnOnce(&mut dyn BufRead) -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    let mut res = http
        .get(url)
        // El cliente tiene 25 s de tope por pedido; esto son 230 MB.
        .timeout(Duration::from_secs(40 * 60))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("GET {url}"))?;
    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(64);
    let job = tokio::task::spawn_blocking(move || {
        let mut r = BufReader::with_capacity(256 * 1024, flate2::read::MultiGzDecoder::new(ChanReader { rx, cur: Bytes::new() }));
        f(&mut r)
    });
    while let Some(chunk) = res.chunk().await.with_context(|| format!("bajando {url}"))? {
        // Si el lector ya terminó (error al procesar) no tiene sentido seguir bajando.
        if tx.send(chunk).await.is_err() {
            break;
        }
    }
    drop(tx);
    job.await.context("el hilo que procesa la bajada se cayó")?
}

async fn download(http: &reqwest::Client) -> anyhow::Result<Dataset> {
    let ratings = stream_gz(http, &format!("{DATASETS}/title.ratings.tsv.gz"), |r| parse_ratings(r, FLOOR_VOTES)).await?;
    anyhow::ensure!(ratings.len() > 1000, "title.ratings trajo muy pocos títulos ({})", ratings.len());
    let ratings = Arc::new(ratings);
    let mut d = stream_gz(http, &format!("{DATASETS}/title.basics.tsv.gz"), move |r| parse_basics(r, &ratings)).await?;
    anyhow::ensure!(d.movies.len() > 1000, "title.basics trajo muy pocas películas ({})", d.movies.len());
    d.updated_at = now_unix();
    Ok(d)
}

// ---------- estado ----------

#[derive(Clone)]
pub struct Imdb {
    inner: Arc<Inner>,
}

struct Inner {
    enabled: bool,
    path: PathBuf,
    http: reqwest::Client,
    data: RwLock<Option<Arc<Dataset>>>,
    busy: AtomicBool,
    /// Alguien pidió la lista sin datos todavía: bajar ya.
    wake: tokio::sync::Notify,
    pages: TtlCache<String, Bytes>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TopState {
    Ready,
    /// Todavía no hay datos (la primera bajada está en camino o se reintenta).
    Preparing,
    /// Apagado (modo local o `MYTHOS_IMDB=0`).
    Unavailable,
}

#[derive(Serialize)]
struct TopBody<'a> {
    state: TopState,
    items: &'a [MetaPreview],
    total: usize,
    more: bool,
    genres: &'a [GenreCount],
    min_votes: u32,
    updated_at: Option<i64>,
}

impl Imdb {
    pub fn new(data_dir: &FsPath, http: reqwest::Client, enabled: bool) -> Self {
        Self {
            inner: Arc::new(Inner {
                enabled,
                path: data_dir.join("imdb").join(FILE),
                http,
                data: RwLock::new(None),
                busy: AtomicBool::new(false),
                wake: tokio::sync::Notify::new(),
                pages: TtlCache::new(Duration::from_secs(3600), 200),
            }),
        }
    }

    /// Con datos ya cargados (pruebas).
    #[cfg(test)]
    pub fn with_data(d: Dataset) -> Self {
        let s = Self::new(FsPath::new("."), reqwest::Client::new(), true);
        s.set(d);
        s
    }

    fn current(&self) -> Option<Arc<Dataset>> {
        self.inner.data.read().unwrap().clone()
    }

    fn set(&self, d: Dataset) {
        *self.inner.data.write().unwrap() = Some(Arc::new(d));
    }

    fn state(&self) -> TopState {
        if !self.inner.enabled {
            TopState::Unavailable
        } else if self.current().is_some() {
            TopState::Ready
        } else {
            TopState::Preparing
        }
    }

    /// Lo guardado en disco (si hay).
    async fn load(&self) -> anyhow::Result<bool> {
        let path = self.inner.path.clone();
        if !path.exists() {
            return Ok(false);
        }
        let d = tokio::task::spawn_blocking(move || Dataset::read_from(std::fs::File::open(&path)?)).await??;
        tracing::info!("imdb: {} películas y {} series cargadas de disco", d.movies.len(), d.series.len());
        self.set(d);
        Ok(true)
    }

    /// Baja todo de nuevo, lo guarda y lo pone en uso.
    async fn refresh(&self) -> anyhow::Result<()> {
        if self.inner.busy.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let r = async {
            let started = std::time::Instant::now();
            tracing::info!("imdb: bajando los datasets (title.ratings + title.basics)…");
            let d = download(&self.inner.http).await?;
            let path = self.inner.path.clone();
            let d = tokio::task::spawn_blocking(move || -> anyhow::Result<Dataset> {
                std::fs::create_dir_all(path.parent().unwrap())?;
                let tmp = path.with_extension("tmp");
                d.write_to(std::fs::File::create(&tmp)?)?;
                std::fs::rename(&tmp, &path)?;
                Ok(d)
            })
            .await??;
            tracing::info!(
                "imdb: {} películas y {} series con ≥ {FLOOR_VOTES} votos ({} s)",
                d.movies.len(),
                d.series.len(),
                started.elapsed().as_secs()
            );
            self.set(d);
            anyhow::Ok(())
        }
        .await;
        self.inner.busy.store(false, Ordering::SeqCst);
        r
    }

    /// Tarea de fondo (solo con `enabled`): carga lo de disco y lo mantiene al día.
    pub async fn keep_fresh(self) {
        if let Err(e) = self.load().await {
            tracing::warn!("imdb: no se pudo leer lo guardado ({e:#}); se baja de nuevo");
        }
        let age = self.current().map(|d| Duration::from_secs((now_unix() - d.updated_at).max(0) as u64));
        let mut wait = match age {
            None => Duration::from_secs(20),
            Some(a) if a >= REFRESH_EVERY => Duration::from_secs(5 * 60),
            Some(a) => REFRESH_EVERY - a,
        };
        loop {
            // Sin datos, un pedido a la lista adelanta la bajada.
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = self.inner.wake.notified(), if self.current().is_none() => {}
            }
            wait = match self.refresh().await {
                Ok(()) => REFRESH_EVERY,
                Err(e) => {
                    tracing::warn!("imdb: falló la bajada ({e:#}); se reintenta en 6 h");
                    RETRY_AFTER
                }
            };
        }
    }

    /// La página ya serializada (`TopBody`).
    fn page(&self, kind: Kind, mask: u32, min_votes: u32, skip: usize, limit: usize) -> ApiResult<Bytes> {
        let state = self.state();
        let Some(d) = self.current().filter(|_| state == TopState::Ready) else {
            if state == TopState::Preparing {
                self.inner.wake.notify_one();
            }
            let body = TopBody { state, items: &[], total: 0, more: false, genres: &[], min_votes, updated_at: None };
            return Ok(Bytes::from(serde_json::to_vec(&body).map_err(ApiError::internal)?));
        };
        let key = format!("{}:{}:{mask}:{min_votes}:{skip}:{limit}", d.updated_at, kind.code());
        if let Some(b) = self.inner.pages.get(&key) {
            return Ok(b);
        }
        let p = d.query(kind, mask, min_votes, skip, limit);
        let body = TopBody {
            state,
            more: skip + p.items.len() < p.total,
            items: &p.items,
            total: p.total,
            genres: &p.genres,
            min_votes,
            updated_at: Some(d.updated_at),
        };
        let b = Bytes::from(serde_json::to_vec(&body).map_err(ApiError::internal)?);
        self.inner.pages.insert(key, b.clone());
        Ok(b)
    }
}

// ---------- rutas ----------

pub fn routes<S>() -> Router<S>
where
    Imdb: FromRef<S>,
    Auth: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/top/{type}", get(top::<S>))
}

#[derive(Deserialize)]
struct TopQuery {
    /// Separados por coma: "Drama,Crime".
    genres: Option<String>,
    min_votes: Option<u32>,
    skip: Option<usize>,
    limit: Option<usize>,
}

/// Los géneros pedidos como máscara (400 si alguno no existe o son demasiados).
fn parse_genres(s: Option<&str>) -> ApiResult<u32> {
    let mut mask = 0;
    let mut n = 0;
    for g in s.unwrap_or("").split(',').map(str::trim).filter(|g| !g.is_empty()) {
        let bit = genre_bit(g).filter(|&b| b != ADULT_BIT).ok_or_else(|| {
            let g: String = g.chars().take(40).collect();
            ApiError::bad_request(format!("Unknown genre \"{g}\""))
        })?;
        if mask & bit == 0 {
            n += 1;
        }
        mask |= bit;
    }
    if n > MAX_GENRES {
        return Err(ApiError::bad_request(format!("Pick up to {MAX_GENRES} genres (IMDb lists at most {MAX_GENRES} per title)")));
    }
    Ok(mask)
}

async fn top<S>(State(imdb): State<Imdb>, _u: AuthUser, Path(kind): Path<String>, Query(q): Query<TopQuery>) -> ApiResult<Response>
where
    Imdb: FromRef<S>,
    Auth: FromRef<S>,
    S: Send + Sync,
{
    check_type(&kind)?;
    let kind = if kind == "movie" { Kind::Movie } else { Kind::Series };
    let mask = parse_genres(q.genres.as_deref())?;
    let default = if kind == Kind::Movie { DEFAULT_VOTES_MOVIE } else { DEFAULT_VOTES_SERIES };
    let min_votes = q.min_votes.unwrap_or(default).max(FLOOR_VOTES);
    let skip = q.skip.unwrap_or(0).min(SKIP_MAX);
    let limit = q.limit.unwrap_or(PAGE_DEFAULT).clamp(1, PAGE_MAX);
    let body = imdb.page(kind, mask, min_votes, skip, limit)?;
    Ok(([(header::CONTENT_TYPE, "application/json")], body).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth::{create_user, routes as auth_routes},
        db::Db,
    };
    use axum::{body::Body, http::Request, http::StatusCode};
    use tower::ServiceExt;

    const RATINGS: &str = "tconst\taverageRating\tnumVotes\n\
        tt0111161\t9.3\t3100000\n\
        tt0068646\t9.2\t2100000\n\
        tt0050083\t9.0\t930000\n\
        tt0903747\t9.5\t2300000\n\
        tt0944947\t9.2\t2400000\n\
        tt7000001\t9.9\t900\n\
        tt7000002\t8.0\t5000\n\
        tt7000003\t8.8\t40000\n\
        tt7000004\t8.1\t30000\n\
        tt7000005\t7.7\t20000\n";

    const BASICS: &str = "tconst\ttitleType\tprimaryTitle\toriginalTitle\tisAdult\tstartYear\tendYear\truntimeMinutes\tgenres\n\
        tt0111161\tmovie\tThe Shawshank Redemption\tThe Shawshank Redemption\t0\t1994\t\\N\t142\tDrama\n\
        tt0068646\tmovie\tThe Godfather\tThe Godfather\t0\t1972\t\\N\t175\tCrime,Drama\n\
        tt0050083\tmovie\t12 Angry Men\t12 Angry Men\t0\t1957\t\\N\t96\tCrime,Drama\n\
        tt0903747\ttvSeries\tBreaking Bad\tBreaking Bad\t0\t2008\t2013\t49\tCrime,Drama,Thriller\n\
        tt0944947\ttvSeries\tGame of Thrones\tGame of Thrones\t0\t2011\t2019\t57\tAction,Adventure,Drama\n\
        tt7000001\tmovie\tFew Votes\tFew Votes\t0\t2020\t\\N\t90\tDrama\n\
        tt7000002\tmovie\tAdult Thing\tAdult Thing\t1\t2020\t\\N\t90\tDrama\n\
        tt7000003\ttvEpisode\tSome Episode\tSome Episode\t0\t2020\t\\N\t40\tDrama\n\
        tt7000004\ttvMiniSeries\tA Mini\tA Mini\t0\t2016\t\\N\t60\tCrime,Mystery\n\
        tt7000005\ttvSeries\tStill Running\tStill Running\t0\t2021\t\\N\t30\tComedy\n\
        tt9999999\tmovie\tNo Rating\tNo Rating\t0\t2001\t\\N\t90\tDrama\n";

    fn sample() -> Dataset {
        let ratings = parse_ratings(RATINGS.as_bytes(), FLOOR_VOTES).unwrap();
        let mut d = parse_basics(BASICS.as_bytes(), &ratings).unwrap();
        d.updated_at = 1_700_000_000;
        d
    }

    fn mask(gs: &[&str]) -> u32 {
        gs.iter().map(|g| genre_bit(g).unwrap()).fold(0, |m, b| m | b)
    }

    #[test]
    fn parses_and_filters() {
        let ratings = parse_ratings(RATINGS.as_bytes(), FLOOR_VOTES).unwrap();
        assert_eq!(ratings.get("tt0111161"), Some(&(93, 3_100_000)));
        assert!(!ratings.contains_key("tt7000001"), "menos votos que el piso");
        let d = sample();
        // Sin adultos, episodios ni títulos sin rating; ordenado por rating y después votos.
        let ids: Vec<&str> = d.movies.iter().map(|t| &*t.id).collect();
        assert_eq!(ids, ["tt0111161", "tt0068646", "tt0050083"]);
        let ids: Vec<&str> = d.series.iter().map(|t| &*t.id).collect();
        assert_eq!(ids, ["tt0903747", "tt0944947", "tt7000004", "tt7000005"]);
        let mini = d.series.iter().find(|t| &*t.id == "tt7000004").unwrap();
        assert_eq!(mini.year(Kind::Series).as_deref(), Some("2016"));
        let running = d.series.iter().find(|t| &*t.id == "tt7000005").unwrap();
        assert_eq!(running.year(Kind::Series).as_deref(), Some("2021-"));
        assert_eq!(d.series[0].year(Kind::Series).as_deref(), Some("2008-2013"));
    }

    #[test]
    fn query_all_genres_must_match() {
        let d = sample();
        let p = d.query(Kind::Movie, mask(&["Crime", "Drama"]), FLOOR_VOTES, 0, 10);
        let ids: Vec<&str> = p.items.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["tt0068646", "tt0050083"]);
        assert_eq!(p.total, 2);
        let m = &p.items[0];
        assert_eq!(m.kind, "movie");
        assert_eq!(m.year.as_deref(), Some("1972"));
        assert_eq!(m.imdb_rating.as_deref(), Some("9.2"));
        assert_eq!(m.poster.as_deref(), Some("https://images.metahub.space/poster/small/tt0068646/img"));
        assert_eq!(m.genres.as_deref(), Some(&["Crime".to_string(), "Drama".to_string()][..]));

        // Conteos: cuántos quedarían sumando cada género. Los que no están en el tipo no salen.
        let p = d.query(Kind::Series, mask(&["Drama"]), FLOOR_VOTES, 0, 10);
        assert_eq!(p.total, 2);
        let count = |g: &str| p.genres.iter().find(|x| x.name == g).map(|x| x.count);
        assert_eq!(count("Drama"), Some(2));
        assert_eq!(count("Crime"), Some(1));
        assert_eq!(count("Comedy"), Some(0));
        assert_eq!(count("Western"), None);
        assert_eq!(count("Adult"), None);
        // Con más votos, la comedia (20 000) ya no está: no se ofrece.
        let p = d.query(Kind::Series, 0, 25_000, 0, 10);
        assert!(p.genres.iter().all(|g| g.name != "Comedy"));
        assert!(p.genres.iter().any(|g| g.name == "Mystery"));

        // Mínimo de votos y paginado.
        let p = d.query(Kind::Series, 0, 1_000_000, 0, 10);
        assert_eq!(p.total, 2);
        let p = d.query(Kind::Series, 0, FLOOR_VOTES, 1, 2);
        assert_eq!(p.total, 4);
        assert_eq!(p.items.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["tt0944947", "tt7000004"]);
    }

    #[test]
    fn compact_file_roundtrip() {
        let d = sample();
        let mut buf = Vec::new();
        d.write_to(&mut buf).unwrap();
        assert_eq!(Dataset::read_from(&buf[..]).unwrap(), d);
        assert!(Dataset::read_from(&b"basura"[..]).is_err());
    }

    #[test]
    fn genre_params() {
        assert_eq!(parse_genres(None).unwrap(), 0);
        assert_eq!(parse_genres(Some(" drama , Sci-Fi,")).unwrap(), mask(&["Drama", "Sci-Fi"]));
        assert_eq!(parse_genres(Some("Drama,Drama,drama")).unwrap(), mask(&["Drama"]));
        assert!(parse_genres(Some("Adult")).is_err());
        assert!(parse_genres(Some("Nope")).is_err());
        assert!(parse_genres(Some("Action,Comedy,Drama,Horror")).is_err());
    }

    #[tokio::test]
    async fn streams_a_gzip_through_the_channel() {
        // El mismo camino que la bajada: pedazos chicos por el canal → hilo que descomprime.
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(RATINGS.as_bytes()).unwrap();
        let data = Bytes::from(gz.finish().unwrap());
        let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(2);
        let job = tokio::task::spawn_blocking(move || {
            let r = BufReader::new(flate2::read::MultiGzDecoder::new(ChanReader { rx, cur: Bytes::new() }));
            parse_ratings(r, FLOOR_VOTES)
        });
        for i in (0..data.len()).step_by(7) {
            tx.send(data.slice(i..(i + 7).min(data.len()))).await.unwrap();
        }
        drop(tx);
        assert_eq!(job.await.unwrap().unwrap().len(), 9);
    }

    #[derive(Clone)]
    struct TestState {
        auth: Auth,
        imdb: Imdb,
    }
    impl FromRef<TestState> for Auth {
        fn from_ref(s: &TestState) -> Self {
            s.auth.clone()
        }
    }
    impl FromRef<TestState> for Imdb {
        fn from_ref(s: &TestState) -> Self {
            s.imdb.clone()
        }
    }

    async fn setup(imdb: Imdb) -> (Router, String) {
        let db = Db::open_in_memory().unwrap();
        db.call(|c| create_user(c, "bob", "hunter22", false).map(|_| ())).await.unwrap();
        let st = TestState { auth: Auth::new(db), imdb };
        let app = Router::new().nest("/api", auth_routes::<TestState>().merge(routes()).with_state(st));
        let req = Request::post("/api/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"username":"bob","password":"hunter22"}"#))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
        (app, v["token"].as_str().unwrap().to_string())
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
        let (app, token) = setup(Imdb::with_data(sample())).await;
        let t = Some(token.as_str());
        let (st, _) = get(&app, "/api/top/movie", None).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);

        let (st, v) = get(&app, "/api/top/movie?genres=Crime,Drama&min_votes=1000", t).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert_eq!(v["state"], "ready");
        assert_eq!(v["total"], 2);
        assert_eq!(v["more"], false);
        assert_eq!(v["min_votes"], 1000);
        assert_eq!(v["updated_at"], 1_700_000_000);
        assert_eq!(v["items"][0]["name"], "The Godfather");
        assert_eq!(v["items"][0]["type"], "movie");

        // Por defecto 25 000 votos en películas; menos del piso se sube al piso.
        let (_, v) = get(&app, "/api/top/movie", t).await;
        assert_eq!(v["min_votes"], DEFAULT_VOTES_MOVIE);
        let (_, v) = get(&app, "/api/top/series?min_votes=3", t).await;
        assert_eq!(v["min_votes"], FLOOR_VOTES);
        assert_eq!(v["total"], 4);
        let (_, v) = get(&app, "/api/top/series?limit=1&skip=1&min_votes=1000", t).await;
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["more"], true);

        let (st, v) = get(&app, "/api/top/movie?genres=Nope", t).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert_eq!(v["error"], "Unknown genre \"Nope\"");
        let (st, _) = get(&app, "/api/top/anime", t).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);

        // Apagado (modo local): responde igual, sin títulos.
        let (app, token) = setup(Imdb::new(FsPath::new("."), reqwest::Client::new(), false)).await;
        let (st, v) = get(&app, "/api/top/movie", Some(&token)).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["state"], "unavailable");
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
    }
}
