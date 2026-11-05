//! Rutas HTTP del crate.
//!
//! - `public_router`: lo que monta el server bajo `/api/media` (él pone la auth).
//! - `internal_router`: endpoint solo en 127.0.0.1 (puerto efímero) para que ffmpeg y ffprobe
//!   lean el archivo con la misma priorización de piezas que el reproductor.

use crate::engine::Engine;
use crate::stream::{self, coded_error, Role};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use std::collections::HashMap;
use std::sync::Arc;

type St = State<Arc<Engine>>;

pub fn public_router(engine: Arc<Engine>) -> Router {
    Router::new()
        .route("/{session}", axum::routing::delete(delete_session))
        .route("/{session}/file", get(file))
        .route("/{session}/status", get(status))
        .route("/{session}/hls/index.m3u8", get(hls_index))
        .route("/{session}/hls/{segment}", get(hls_segment))
        .route("/{session}/subs/{file}", get(subs))
        .route("/{session}/thumbs", get(thumbs_index))
        .route("/{session}/thumbs/{file}", get(thumbs_image))
        .with_state(engine)
}

pub fn internal_router(engine: Arc<Engine>) -> Router {
    Router::new().route("/s/{session}/file", get(internal_file)).with_state(engine)
}

fn not_found() -> Response {
    coded_error(StatusCode::NOT_FOUND, crate::Coded::new("media_session", "unknown or expired media session"))
}

fn range_of(h: &HeaderMap) -> Option<&str> {
    h.get(header::RANGE).and_then(|v| v.to_str().ok())
}

async fn file(State(e): St, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let Some(s) = e.session(&id) else { return not_found() };
    stream::serve_file(&e.api, &e.http, s, range_of(&headers), Role::Player).await
}

async fn internal_file(State(e): St, Path(id): Path<String>, Query(q): Query<HashMap<String, String>>, headers: HeaderMap) -> Response {
    let Some(s) = e.session(&id) else { return not_found() };
    let role = Role::parse(q.get("role").map(|s| s.as_str()));
    stream::serve_file(&e.api, &e.http, s, range_of(&headers), role).await
}

async fn status(State(e): St, Path(id): Path<String>) -> Response {
    match e.session(&id) {
        Some(s) => Json(s.status()).into_response(),
        None => not_found(),
    }
}

async fn delete_session(State(e): St, Path(id): Path<String>) -> Response {
    e.close_session(&id).await;
    StatusCode::NO_CONTENT.into_response()
}

/// HLS y subtítulos internos necesitan ffmpeg: sin él, 501 con un mensaje claro.
fn no_ffmpeg() -> Response {
    coded_error(
        StatusCode::NOT_IMPLEMENTED,
        crate::Coded::new("no_ffmpeg", "transcoding is not available on this device (ffmpeg not found); use direct play"),
    )
}

async fn hls_index(State(e): St, Path(id): Path<String>, Query(q): Query<HashMap<String, String>>) -> Response {
    if !e.tools.ffmpeg {
        return no_ffmpeg();
    }
    let Some(s) = e.session(&id) else { return not_found() };
    crate::hls::index(&e, s, &q).await
}

async fn hls_segment(State(e): St, Path((id, seg)): Path<(String, String)>, Query(q): Query<HashMap<String, String>>) -> Response {
    if !e.tools.ffmpeg {
        return no_ffmpeg();
    }
    let Some(s) = e.session(&id) else { return not_found() };
    crate::hls::segment(&e, s, &seg, &q).await
}

async fn subs(State(e): St, Path((id, file)): Path<(String, String)>) -> Response {
    if !e.tools.ffmpeg {
        return no_ffmpeg();
    }
    let Some(s)
 = e.session(&id) else { return not_found() };
    let Some(idx) = file.strip_suffix(".vtt").and_then(|x| x.parse::<usize>().ok()) else {
        return coded_error(StatusCode::NOT_FOUND, crate::Coded::new("sub_not_found", "subtitle not found"));
    };
    crate::subs::serve(&e, s, idx).await
}

/// Miniaturas para la vista previa al adelantar (`thumbs.rs`). Sin ffmpeg, 501: la UI sigue sin.
async fn thumbs_index(State(e): St, Path(id): Path<String>) -> Response {
    if !e.tools.ffmpeg {
        return no_ffmpeg();
    }
    let Some(s) = e.session(&id) else { return not_found() };
    crate::thumbs::index(&e, s)
}

async fn thumbs_image(State(e): St, Path((id, file)): Path<(String, String)>) -> Response {
    if !e.tools.ffmpeg {
        return no_ffmpeg();
    }
    let Some(s) = e.session(&id) else { return not_found() };
    crate::thumbs::image(&e, s, &file).await
}
