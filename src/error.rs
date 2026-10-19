//! One application error type, convertible into an HTTP response.
//!
//! Handlers return `Result<T, AppError>` so that `?` works throughout and
//! no request path needs `unwrap()`.
//!
//! Migrated from `com.smartContact.error.UserNotFoundException` (a checked
//! exception) and its `@ControllerAdvice` handler, which turned it into a
//! `404` with body `{"status":"NOT_FOUND","message":"<msg>"}`.

use std::error::Error as StdError;
use std::fmt;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Boxed "cause" type — the Rust analogue of Java's `Throwable cause`.
pub type BoxError = Box<dyn StdError + Send + Sync + 'static>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,

    /// The requested user does not exist (Java `UserNotFoundException`).
    /// Rendered as 404 `{"status":"NOT_FOUND","message":"<msg>"}`.
    #[error("{0}")]
    UserNotFound(String),

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

/// Standalone "user not found" error mirroring every constructor of the Java
/// `UserNotFoundException`: optional detail message and optional cause.
///
/// Converts into [`AppError::UserNotFound`] via `?` / `From`.
#[derive(Debug, Default)]
pub struct UserNotFoundError {
    message: Option<String>,
    cause: Option<BoxError>,
}

impl UserNotFoundError {
    /// `new UserNotFoundException()` — no detail message, no cause.
    pub fn new() -> Self {
        Self::default()
    }

    /// `new UserNotFoundException(message)` — detail message, no cause.
    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            cause: None,
        }
    }

    /// `new UserNotFoundException(message, cause)`.
    pub fn with_message_and_cause(message: impl Into<String>, cause: impl Into<BoxError>) -> Self {
        Self {
            message: Some(message.into()),
            cause: Some(cause.into()),
        }
    }

    /// `new UserNotFoundException(cause)` — like Java, the detail message is
    /// derived from the cause (`cause.toString()`).
    pub fn with_cause(cause: impl Into<BoxError>) -> Self {
        let cause = cause.into();
        Self {
            message: Some(cause.to_string()),
            cause: Some(cause),
        }
    }

    // MIGRATION_NOTE: the protected Java constructor
    // `(message, cause, enableSuppression, writableStackTrace)` has no Rust
    // equivalent — Rust errors have no suppressed-exception list and no
    // captured stack trace to toggle. It is collapsed into
    // `with_message_and_cause`; the two boolean flags are intentionally dropped.

    /// Equivalent of `Throwable.getMessage()` (may be absent).
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl fmt::Display for UserNotFoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(m) => f.write_str(m),
            None => f.write_str("user not found"),
        }
    }
}

impl StdError for UserNotFoundError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.cause
            .as_ref()
            .map(|c| c.as_ref() as &(dyn StdError + 'static))
    }
}

impl From<UserNotFoundError> for AppError {
    fn from(err: UserNotFoundError) -> Self {
        // A missing Java message (null) is carried as an empty string and
        // rendered as JSON `null` in the response.
        AppError::UserNotFound(err.message.unwrap_or_default())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::UserNotFound(message) => {
                let message = if message.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(message)
                };
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "status": "NOT_FOUND", "message": message })),
                )
                    .into_response()
            }
            other => {
                let (status, message) = match &other {
                    AppError::NotFound => (StatusCode::NOT_FOUND, other.to_string()),
                    AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, other.to_string()),
                    AppError::Unauthorized => (StatusCode::UNAUTHORIZED, other.to_string()),
                    AppError::Internal(_) | AppError::UserNotFound(_) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal server error".to_string(),
                    ),
                };
                (status, Json(json!({ "error": message }))).into_response()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_mirror_java() {
        assert!(UserNotFoundError::new().message().is_none());
        assert!(UserNotFoundError::new().source().is_none());

        let e = UserNotFoundError::with_message("User are not available");
        assert_eq!(e.message(), Some("User are not available"));
        assert!(e.source().is_none());

        let cause = anyhow_like::Error("db down".into());
        let e = UserNotFoundError::with_message_and_cause("missing", cause);
        assert_eq!(e.message(), Some("missing"));
        assert!(e.source().is_some());

        let e = UserNotFoundError::with_cause(anyhow_like::Error("db down".into()));
        assert_eq!(e.message(), Some("db down"));
        assert!(e.source().is_some());
    }

    #[test]
    fn user_not_found_is_404() {
        let resp = AppError::UserNotFound("User are not available".into()).into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
