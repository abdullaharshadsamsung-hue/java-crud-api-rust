//! Application-wide error type. This is the port of
//! `com.smartContact.error.UserNotFoundException` together with the
//! `@ControllerAdvice` class `RestResponseEntityExceptionHandling`.
//!
//! Spring's global exception handling becomes a single [`AppError`] enum
//! that implements [`IntoResponse`]. Handlers return
//! `Result<T, AppError>`, and axum turns the error variant into the HTTP
//! response:
//!
//! | Spring source                                    | `AppError` variant      | Response                     |
//! |--------------------------------------------------|-------------------------|------------------------------|
//! | `@ExceptionHandler(UserNotFoundException.class)` | `NotFound`              | 404 + `ErrorMessage` JSON    |
//! | Inherited: `MethodArgumentNotValidException`,    | `BadRequest`            | 400, empty body              |
//! | `HttpMessageNotReadableException`,               |                         |                              |
//! | `TypeMismatchException`, missing params          |                         |                              |
//! | Inherited: `HttpMediaTypeNotSupportedException`  | `UnsupportedMediaType`  | 415, empty body              |
//! | Inherited: `HttpRequestMethodNotSupported`       | `MethodNotAllowed`      | 405, empty body              |
//! | Inherited: `HttpMediaTypeNotAcceptable`          | `NotAcceptable`         | 406, empty body              |
//! | Anything else (e.g. `DataAccessException`)       | `Internal { path }`     | 500 + Boot-style error JSON  |
//!
//! MIGRATION_NOTE: `ResponseEntityExceptionHandler` handles about 15
//! framework exceptions. Only the ones this service can actually produce
//! have variants here. Axum extractor rejections are mapped to these
//! variants by the custom extractors in `src/extract.rs`.
//!
//! MIGRATION_NOTE: `crate::models::error_message::ErrorMessage` keeps its
//! fields private, so it cannot be built with a struct literal here. The
//! 404 body is serialized through [`NotFoundBody`], a module-local struct
//! with exactly the same JSON shape (`{"status":..., "message":...}`, both
//! nullable). The wire format is identical to Jackson serializing
//! `new ErrorMessage(HttpStatus.NOT_FOUND, exception.getMessage())`.

use std::error::Error as StdError;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

/// Boxed, thread-safe error used for opaque internal failures.
pub type BoxError = Box<dyn StdError + Send + Sync + 'static>;

/// Port of `UserNotFoundException extends Exception`.
///
/// `message` is `Option` because Java's no-arg constructor leaves
/// `getMessage()` as `null`. That `null` is serialized as JSON `null`, and
/// it is never converted to `""`. An empty string stays `""`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserNotFoundError {
    pub message: Option<String>,
}

impl UserNotFoundError {
    /// Equivalent of `new UserNotFoundException(message)`.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
        }
    }

    /// Equivalent of `exception.getMessage()`.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl fmt::Display for UserNotFoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(m) => write!(f, "{m}"),
            None => write!(f, "UserNotFoundException"),
        }
    }
}

impl StdError for UserNotFoundError {}

/// The single error type returned by every handler.
#[derive(Debug)]
pub enum AppError {
    /// `UserNotFoundException`: 404 with an `ErrorMessage` body.
    NotFound(UserNotFoundError),
    /// Validation failure, malformed JSON, or path/query type mismatch: 400 with an empty body.
    BadRequest,
    /// Missing or unsupported `Content-Type`: 415 with an empty body.
    UnsupportedMediaType,
    /// Unsupported HTTP method: 405 with an empty body.
    MethodNotAllowed,
    /// The requested media type cannot be produced: 406 with an empty body.
    NotAcceptable,
    /// Unexpected server failure: 500 with a Boot-style JSON body.
    /// The underlying cause is logged when the error is created and is never
    /// sent to the client.
    Internal { path: String },
}

impl AppError {
    /// Builds an internal error. The cause is logged and then dropped.
    pub fn internal<E: fmt::Display>(cause: E) -> Self {
        tracing::error!(error = %cause, "internal server error");
        AppError::Internal {
            path: String::new(),
        }
    }

    /// Attaches the request path to an `Internal` error so it appears in the
    /// Boot-style body. Other variants are returned unchanged.
    pub fn with_path(self, request_path: impl Into<String>) -> Self {
        match self {
            AppError::Internal { .. } => AppError::Internal {
                path: request_path.into(),
            },
            other => other,
        }
    }

    /// HTTP status the variant maps to.
    pub fn status_code(&self) -> StatusCode {
        match self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::BadRequest => StatusCode::BAD_REQUEST,
            AppError::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            AppError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            AppError::NotAcceptable => StatusCode::NOT_ACCEPTABLE,
            AppError::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NotFound(e) => write!(f, "not found: {e}"),
            AppError::BadRequest => write!(f, "bad request"),
            AppError::UnsupportedMediaType => write!(f, "unsupported media type"),
            AppError::MethodNotAllowed => write!(f, "method not allowed"),
            AppError::NotAcceptable => write!(f, "not acceptable"),
            AppError::Internal { .. } => write!(f, "internal server error"),
        }
    }
}

impl StdError for AppError {}

impl From<UserNotFoundError> for AppError {
    fn from(e: UserNotFoundError) -> Self {
        AppError::NotFound(e)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::internal(e)
    }
}

impl From<BoxError> for AppError {
    fn from(e: BoxError) -> Self {
        AppError::internal(e)
    }
}

/// Explicit lookup table for Spring's `HttpStatus.name()`, covering the
/// statuses this service emits. Jackson serializes `HttpStatus` as its enum
/// constant name, so this table gives the wire value.
pub fn http_status_name(status: StatusCode) -> Option<&'static str> {
    match status.as_u16() {
        400 => Some("BAD_REQUEST"),
        404 => Some("NOT_FOUND"),
        405 => Some("METHOD_NOT_ALLOWED"),
        406 => Some("NOT_ACCEPTABLE"),
        415 => Some("UNSUPPORTED_MEDIA_TYPE"),
        500 => Some("INTERNAL_SERVER_ERROR"),
        _ => None,
    }
}

/// JSON body of the `UserNotFoundException` handler. It has the same wire
/// shape as `com.smartContact.model.ErrorMessage`: `{"status":"NOT_FOUND",
/// "message":...}`. `None` serializes as `null`, as Jackson does.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotFoundBody {
    pub status: Option<String>,
    pub message: Option<String>,
}

/// Spring Boot `DefaultErrorAttributes` body used for 500 responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootErrorBody {
    pub timestamp: String,
    pub status: u16,
    pub error: String,
    pub path: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            // @ExceptionHandler(UserNotFoundException.class):
            // new ErrorMessage(HttpStatus.NOT_FOUND, exception.getMessage())
            AppError::NotFound(e) => {
                let body = NotFoundBody {
                    status: http_status_name(StatusCode::NOT_FOUND).map(str::to_string),
                    message: e.message,
                };
                (StatusCode::NOT_FOUND, Json(body)).into_response()
            }
            // ResponseEntityExceptionHandler returns these statuses with a null body.
            AppError::BadRequest => StatusCode::BAD_REQUEST.into_response(),
            AppError::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response(),
            AppError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED.into_response(),
            AppError::NotAcceptable => StatusCode::NOT_ACCEPTABLE.into_response(),
            AppError::Internal { path } => {
                let body = BootErrorBody {
                    timestamp: boot_timestamp_now(),
                    status: 500,
                    error: "Internal Server Error".to_string(),
                    path,
                };
                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
            }
        }
    }
}

/// Current UTC time in Boot/Jackson format, e.g.
/// `2024-01-31T12:34:56.789+00:00`.
fn boot_timestamp_now() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs() as i64;
    let millis = dur.subsec_millis();
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}+00:00",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        millis
    )
}

/// Converts days since 1970-01-01 to (year, month, day), using Howard
/// Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }

    #[test]
    fn statuses() {
        assert_eq!(
            AppError::from(UserNotFoundError::new("x")).status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(AppError::BadRequest.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(
            AppError::UnsupportedMediaType.status_code(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
    }

    #[test]
    fn status_names() {
        assert_eq!(http_status_name(StatusCode::NOT_FOUND), Some("NOT_FOUND"));
        assert_eq!(http_status_name(StatusCode::IM_A_TEAPOT), None);
    }

    #[test]
    fn not_found_response_status() {
        let resp = AppError::from(UserNotFoundError::new("User are not available")).into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let resp = AppError::from(UserNotFoundError::default()).into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
