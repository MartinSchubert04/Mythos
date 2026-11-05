//! Errores de la API: siempre status + `{"error": "..."}` en inglés y, en los que la UI muestra,
//! un `code` estable (y `params` con los datos que van en el texto) para que los traduzca sin
//! depender de la frase (docs/API.md, "Errores"). También envuelve los extractores de axum
//! (`Json`, `Path`, `Query`) para que sus rechazos salgan con ese formato.
use axum::{
    extract::{rejection::*, FromRequest, FromRequestParts},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
    /// Código estable (`snake_case`): la UI traduce por acá. `None` = solo el texto.
    pub code: Option<String>,
    /// Los datos variables del texto (`{"n": 3}`), para armarlo en el idioma de la UI.
    pub params: Map<String, Value>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, message: message.into(), code: None, params: Map::new() }
    }
    /// Le pone el código estable (ver docs/API.md, "Errores").
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
    /// Un dato del texto, para que la UI lo arme en su idioma.
    pub fn with_param(mut self, name: &str, v: impl Into<Value>) -> Self {
        self.params.insert(name.to_string(), v.into());
        self
    }
    pub fn bad_request(m: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, m)
    }
    pub fn unauthorized(m: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, m)
    }
    pub fn forbidden(m: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, m)
    }
    pub fn not_found(m: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, m)
    }
    pub fn conflict(m: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, m)
    }
    pub fn bad_gateway(m: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, m)
    }
    /// Error interno: se loguea el detalle y al cliente le llega un texto genérico.
    pub fn internal(e: impl std::fmt::Display) -> Self {
        tracing::error!("internal error: {e:#}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").with_code("internal")
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self::internal(format!("{e:#}"))
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        Self::internal(e)
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a str>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    params: &'a Map<String, Value>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody { error: &self.message, code: self.code.as_deref(), params: &self.params };
        (self.status, axum::Json(body)).into_response()
    }
}

impl From<JsonRejection> for ApiError {
    fn from(r: JsonRejection) -> Self {
        Self::new(r.status(), format!("Invalid JSON body: {}", r.body_text()))
    }
}
impl From<PathRejection> for ApiError {
    fn from(r: PathRejection) -> Self {
        Self::new(r.status(), format!("Invalid path: {}", r.body_text()))
    }
}
impl From<QueryRejection> for ApiError {
    fn from(r: QueryRejection) -> Self {
        Self::new(r.status(), format!("Invalid query: {}", r.body_text()))
    }
}

/// `axum::Json` con rechazo en formato API.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct Json<T>(pub T);

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// `axum::extract::Path` con rechazo en formato API.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct Path<T>(pub T);

/// `axum::extract::Query` con rechazo en formato API.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
pub struct Query<T>(pub T);
