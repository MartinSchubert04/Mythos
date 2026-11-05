//! Catálogo: proxy a Cinemeta (el addon oficial de Stremio) normalizado a `MetaPreview`/`Meta` del
//! contrato. Las respuestas de Cinemeta son enormes (traen popularidades, links, episodios en los
//! catálogos), así que se recortan y se cachean ya serializadas: catálogos 6 h, metas 24 h,
//! búsquedas 1 h.
use std::{sync::Arc, time::Duration};

use axum::{
    body::Bytes,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    auth::AuthUser,
    cache::TtlCache,
    error::{ApiError, ApiResult, Path, Query},
    state::AppState,
};

const CINEMETA: &str = "https://v3-cinemeta.strem.io";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct MetaPreview {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imdb_rating: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genres: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Meta {
    #[serde(flatten)]
    pub preview: MetaPreview,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cast: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub director: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trailer_youtube_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub videos: Option<Vec<Episode>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Episode {
    pub id: String,
    pub season: u32,
    pub episode: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub released: Option<String>,
}

pub struct Cinemeta {
    http: reqwest::Client,
    lists: TtlCache<String, Bytes>,
    searches: TtlCache<String, Bytes>,
    metas: TtlCache<String, Arc<Meta>>,
}

impl Cinemeta {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            lists: TtlCache::new(Duration::from_secs(6 * 3600), 300),
            searches: TtlCache::new(Duration::from_secs(3600), 300),
            metas: TtlCache::new(Duration::from_secs(24 * 3600), 500),
        }
    }

    async fn fetch(&self, path: &str) -> ApiResult<Value> {
        let url = format!("{CINEMETA}{path}");
        let res = self.http.get(&url).timeout(Duration::from_secs(12)).send().await.map_err(|e| {
            tracing::warn!("cinemeta {url}: {e}");
            ApiError::bad_gateway("The catalog service (Cinemeta) is unreachable, try again in a moment").with_code("cinemeta_down")
        })?;
        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ApiError::not_found("Title not found").with_code("title_not_found"));
        }
        if !res.status().is_success() {
            tracing::warn!("cinemeta {url}: HTTP {}", res.status());
            return Err(ApiError::bad_gateway(format!("The catalog service (Cinemeta) answered {}", res.status()))
                .with_code("cinemeta_status")
                .with_param("status", res.status().as_u16()));
        }
        res.json().await.map_err(|e| {
            tracing::warn!("cinemeta {url}: JSON inválido: {e}");
            ApiError::bad_gateway("The catalog service (Cinemeta) sent an invalid response").with_code("cinemeta_bad")
        })
    }

    async fn list(&self, path: String) -> ApiResult<Vec<MetaPreview>> {
        let v = self.fetch(&path).await?;
        Ok(metas_from(&v))
    }

    pub async fn catalog(&self, kind: &str, catalog: &str, genre: Option<&str>, skip: Option<u32>) -> ApiResult<Bytes> {
        let mut extras = Vec::new();
        if let Some(g) = genre {
            extras.push(format!("genre={}", enc(g)));
        }
        if let Some(s) = skip.filter(|s| *s > 0) {
            extras.push(format!("skip={s}"));
        }
        let path = if extras.is_empty() {
            format!("/catalog/{kind}/{catalog}.json")
        } else {
            format!("/catalog/{kind}/{catalog}/{}.json", extras.join("&"))
        };
        if let Some(b) = self.lists.get(&path) {
            return Ok(b);
        }
        let list = match self.list(path.clone()).await {
            Ok(l) => l,
            Err(e) => return self.lists.get_stale(&path).ok_or(e),
        };
        let body = Bytes::from(serde_json::to_vec(&list).map_err(ApiError::internal)?);
        self.lists.insert(path, body.clone());
        Ok(body)
    }

    pub async fn search(&self, q: &str) -> ApiResult<Bytes> {
        let key = q.to_lowercase();
        if let Some(b) = self.searches.get(&key) {
            return Ok(b);
        }
        #[derive(Serialize)]
        struct Out {
            movies: Vec<MetaPreview>,
            series: Vec<MetaPreview>,
        }
        let (movies, series) = tokio::join!(
            self.list(format!("/catalog/movie/top/search={}.json", enc(q))),
            self.list(format!("/catalog/series/top/search={}.json", enc(q))),
        );
        // Si falla una de las dos se devuelve la otra (y no se cachea el resultado incompleto).
        let complete = movies.is_ok() && series.is_ok();
        let out = match (movies, series) {
            (Err(e), Err(_)) => return Err(e),
            (m, s) => Out { movies: m.unwrap_or_default(), series: s.unwrap_or_default() },
        };
        let body = Bytes::from(serde_json::to_vec(&out).map_err(ApiError::internal)?);
        if complete {
            self.searches.insert(key, body.clone());
        }
        Ok(body)
    }

    pub async fn genres(&self, kind: &str) -> ApiResult<Bytes> {
        let key = format!("genres:{kind}");
        if let Some(b) = self.lists.get(&key) {
            return Ok(b);
        }
        let manifest = match self.fetch("/manifest.json").await {
            Ok(m) => m,
            Err(e) => return self.lists.get_stale(&key).ok_or(e),
        };
        let genres: Vec<String> = manifest["catalogs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|c| c["type"] == kind && c["id"] == "top")
            .and_then(|c| c["genres"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|g| g.as_str().map(str::to_string))
            .collect();
        let body = Bytes::from(serde_json::to_vec(&genres).map_err(ApiError::internal)?);
        self.lists.insert(key, body.clone());
        Ok(body)
    }

    pub async fn meta(&self, kind: &str, id: &str) -> ApiResult<Arc<Meta>> {
        let key = format!("{kind}/{id}");
        if let Some(m) = self.metas.get(&key) {
            return Ok(m);
        }
        let v = match self.fetch(&format!("/meta/{kind}/{id}.json")).await {
            Ok(v) => v,
            Err(e) => return self.metas.get_stale(&key).ok_or(e),
        };
        let meta = meta_from(&v["meta"], kind).ok_or_else(|| ApiError::not_found("Title not found").with_code("title_not_found"))?;
        let meta = Arc::new(meta);
        self.metas.insert(key, meta.clone());
        Ok(meta)
    }
}

// ---------- normalización ----------

/// Texto no vacío de un campo que puede venir como string o número.
fn text(v: &Value, key: &str) -> Option<String> {
    match &v[key] {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn strings(v: &Value, key: &str) -> Option<Vec<String>> {
    let out: Vec<String> = v[key].as_array()?.iter().filter_map(|x| x.as_str()).map(str::to_string).collect();
    (!out.is_empty()).then_some(out)
}

fn metas_from(v: &Value) -> Vec<MetaPreview> {
    v["metas"].as_array().into_iter().flatten().filter_map(|m| preview_from(m, None)).collect()
}

pub fn preview_from(m: &Value, kind: Option<&str>) -> Option<MetaPreview> {
    let id = text(m, "id").or_else(|| text(m, "imdb_id"))?;
    let kind = text(m, "type").or_else(|| kind.map(str::to_string))?;
    Some(MetaPreview {
        id,
        kind,
        name: text(m, "name")?,
        poster: text(m, "poster"),
        background: text(m, "background"),
        logo: text(m, "logo"),
        // Cinemeta usa guion largo en los rangos ("2011–2019"); el contrato pide "2011-2019".
        year: text(m, "releaseInfo").or_else(|| text(m, "year")).map(|y| y.replace('–', "-")),
        imdb_rating: text(m, "imdbRating"),
        genres: strings(m, "genres").or_else(|| strings(m, "genre")),
        description: text(m, "description"),
    })
}

fn meta_from(m: &Value, kind: &str) -> Option<Meta> {
    let preview = preview_from(m, Some(kind))?;
    let trailer = m["trailerStreams"]
        .as_array()
        .and_then(|a| a.iter().find_map(|t| text(t, "ytId")))
        .or_else(|| m["trailers"].as_array().and_then(|a| a.iter().find_map(|t| text(t, "source"))));
    let videos = (preview.kind == "series").then(|| {
        let mut eps: Vec<Episode> = m["videos"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                Some(Episode {
                    id: text(e, "id")?,
                    season: e["season"].as_u64()? as u32,
                    episode: e["episode"].as_u64().or_else(|| e["number"].as_u64())? as u32,
                    name: text(e, "name").or_else(|| text(e, "title")),
                    overview: text(e, "overview").or_else(|| text(e, "description")),
                    thumbnail: text(e, "thumbnail"),
                    released: text(e, "released").or_else(|| text(e, "firstAired")),
                })
            })
            .collect();
        eps.sort_by_key(|e| (e.season, e.episode));
        eps
    });
    Some(Meta {
        runtime: text(m, "runtime"),
        cast: strings(m, "cast"),
        director: strings(m, "director"),
        trailer_youtube_id: trailer,
        videos,
        preview,
    })
}

/// Minutos de un runtime de Cinemeta: "148 min", "2h 28min", "57".
pub fn runtime_minutes(s: &str) -> Option<f64> {
    let s = s.to_ascii_lowercase();
    let mut total = 0.0;
    let mut num = String::new();
    let mut found = false;
    for ch in s.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() || ch == '.' {
            num.push(ch);
            continue;
        }
        if !num.is_empty() {
            let n: f64 = num.parse().ok()?;
            total += if ch == 'h' { n * 60.0 } else { n };
            found = true;
            num.clear();
        }
    }
    (found && total > 0.0).then_some(total)
}

/// Percent-encoding estricto para los "extras" de Stremio (todo menos alfanuméricos y `-_.~`).
pub fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn check_type(kind: &str) -> ApiResult<()> {
    match kind {
        "movie" | "series" => Ok(()),
        _ => Err(ApiError::bad_request("Type must be 'movie' or 'series'")),
    }
}

/// Ids de Cinemeta: `tt123`, `tt123:1:2`. Se valida para no reenviar basura al upstream.
pub fn check_id(id: &str) -> ApiResult<()> {
    let ok = (2..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'-' | b'.'));
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad_request("Invalid id"))
    }
}

// ---------- rutas ----------

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/catalog/{type}/{catalog}", get(catalog))
        .route("/genres/{type}", get(genres))
        .route("/search", get(search))
        .route("/meta/{type}/{id}", get(meta))
}

fn json_bytes(b: Bytes) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], b).into_response()
}

#[derive(Deserialize)]
struct CatalogQuery {
    genre: Option<String>,
    skip: Option<u32>,
}

async fn catalog(
    State(st): State<AppState>,
    _u: AuthUser,
    Path((kind, catalog)): Path<(String, String)>,
    Query(q): Query<CatalogQuery>,
) -> ApiResult<Response> {
    check_type(&kind)?;
    if !matches!(catalog.as_str(), "top" | "imdbRating" | "year") {
        return Err(ApiError::bad_request("Catalog must be 'top', 'imdbRating' or 'year'"));
    }
    let genre = q.genre.as_deref().map(str::trim).filter(|g| !g.is_empty());
    if genre.is_some_and(|g| g.len() > 64) {
        return Err(ApiError::bad_request("Genre is too long"));
    }
    Ok(json_bytes(st.cinemeta.catalog(&kind, &catalog, genre, q.skip).await?))
}

async fn genres(State(st): State<AppState>, _u: AuthUser, Path(kind): Path<String>) -> ApiResult<Response> {
    check_type(&kind)?;
    Ok(json_bytes(st.cinemeta.genres(&kind).await?))
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

async fn search(State(st): State<AppState>, _u: AuthUser, Query(q): Query<SearchQuery>) -> ApiResult<Response> {
    let q = q.q.trim();
    if q.is_empty() {
        return Ok(json_bytes(Bytes::from_static(br#"{"movies":[],"series":[]}"#)));
    }
    if q.chars().count() > 100 {
        return Err(ApiError::bad_request("Search query is too long").with_code("query_long"));
    }
    Ok(json_bytes(st.cinemeta.search(q).await?))
}

async fn meta(State(st): State<AppState>, _u: AuthUser, Path((kind, id)): Path<(String, String)>) -> ApiResult<Response> {
    check_type(&kind)?;
    check_id(&id)?;
    let m = st.cinemeta.meta(&kind, &id).await?;
    Ok(axum::Json(&*m).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_series_meta() {
        let v: Value = serde_json::from_str(
            r#"{"meta":{"id":"tt0944947","type":"series","name":"Game of Thrones","releaseInfo":"2011–2019",
            "imdbRating":"9.2","genre":["Drama"],"director":null,"runtime":"57 min",
            "trailerStreams":[{"title":"x","ytId":"KPLWWIOCOOQ"}],
            "videos":[{"id":"tt0944947:1:2","season":1,"number":2,"episode":2,"name":"The Kingsroad","firstAired":"2011-04-24"},
                      {"id":"tt0944947:1:1","season":1,"episode":1,"name":"Winter Is Coming","overview":"o","rating":"0"}]}}"#,
        )
        .unwrap();
        let m = meta_from(&v["meta"], "series").unwrap();
        assert_eq!(m.preview.year.as_deref(), Some("2011-2019"));
        assert_eq!(m.preview.genres, Some(vec!["Drama".to_string()]));
        assert_eq!(m.director, None);
        assert_eq!(m.trailer_youtube_id.as_deref(), Some("KPLWWIOCOOQ"));
        let eps = m.videos.unwrap();
        assert_eq!(eps[0].id, "tt0944947:1:1");
        assert_eq!(eps[1].released.as_deref(), Some("2011-04-24"));
        let json = serde_json::to_value(meta_from(&v["meta"], "series").unwrap()).unwrap();
        assert_eq!(json["type"], "series");
        assert_eq!(json["imdb_rating"], "9.2");
        assert!(json.get("cast").is_none());
    }

    #[test]
    fn runtime_parse() {
        assert_eq!(runtime_minutes("148 min"), Some(148.0));
        assert_eq!(runtime_minutes("2h 28min"), Some(148.0));
        assert_eq!(runtime_minutes("N/A"), None);
    }

    #[test]
    fn encoding() {
        assert_eq!(enc("star wars & co"), "star%20wars%20%26%20co");
        assert_eq!(enc("Sci-Fi"), "Sci-Fi");
    }
}
