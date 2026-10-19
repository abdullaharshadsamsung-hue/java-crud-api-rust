//! One application error type, convertible into an HTTP response.
//!
//! Handlers return `Result<T, AppError>` so that `?` works throughout and
//! no request path needs `unwrap()`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error(transparent)]
    Internal(#[from] anyhow_like::Error),
}

/// Minimal stand-in so this file compiles without pulling in `anyhow`.
/// Replace with a concrete error type as real ones are introduced.
pub mod anyhow_like {
    #[derive(Debug, thiserror::Error)]
    #[error("{0}")]
    pub struct Error(pub String);
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error".to_string(),
            ),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}
