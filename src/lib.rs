//! The application as a library: every module, the shared state and
//! the router. main.rs only starts the server, so the integration tests in
//! tests/ can build the same Router (`use migrated_service::{app, AppState}`)
//! and drive it without a network listener.

pub mod config;

/// Application-wide error type. This is the port of
/// `com.smartContact.error.UserNotFoundException` together with the
/// `@ControllerAdvice` class `RestResponseEntityExceptionHandling`.
///
/// It is defined inline here so that its shape matches every consumer:
/// `AppError::BadRequest(String)`, `AppError::Internal(anyhow_like::Error)`,
/// `UserNotFoundError::with_message`, and the `anyhow_like` module.
///
/// | Spring source                                    | `AppError` variant      | Response                     |
/// |--------------------------------------------------|-------------------------|------------------------------|
/// | `@ExceptionHandler(UserNotFoundException.class)` | `NotFound`              | 404 + `ErrorMessage` JSON    |
/// | Validation / unreadable body / type mismatch     | `BadRequest`            | 400, empty body              |
/// | `HttpMediaTypeNotSupportedException`             | `UnsupportedMediaType`  | 415, empty body              |
/// | `HttpRequestMethodNotSupported`                  | `MethodNotAllowed`      | 405, empty body              |
/// | `HttpMediaTypeNotAcceptable`                     | `NotAcceptable`         | 406, empty body              |
/// | Anything else (e.g. `DataAccessException`)       | `Internal`/`InternalAt` | 500 + Boot-style error JSON  |
pub mod error {
    use std::error::Error as StdError;
    use std::fmt;
    use std::time::{SystemTime, UNIX_EPOCH};

    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
    use axum::Json;
    use serde::{Deserialize, Serialize};

    /// Boxed, thread-safe error used for opaque internal failures.
    pub type BoxError = Box<dyn StdError + Send + Sync + 'static>;

    /// Minimal opaque error carrying only a message.
    pub mod anyhow_like {
        use std::fmt;

        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct Error(pub String);

        impl fmt::Display for Error {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::error::Error for Error {}
    }

    /// Port of `UserNotFoundException extends Exception`.
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

        /// Alias of [`UserNotFoundError::new`].
        pub fn with_message(message: impl Into<String>) -> Self {
            Self::new(message)
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
        /// 400 with an empty body. The message is only logged.
        BadRequest(String),
        /// 415 with an empty body.
        UnsupportedMediaType,
        /// 405 with an empty body.
        MethodNotAllowed,
        /// 406 with an empty body.
        NotAcceptable,
        /// Unexpected server failure (no request path attached yet): 500.
        Internal(anyhow_like::Error),
        /// Unexpected server failure with the request path attached: 500.
        InternalAt {
            cause: anyhow_like::Error,
            path: String,
        },
    }

    impl AppError {
        /// Builds an internal error from any displayable cause.
        pub fn internal<E: fmt::Display>(cause: E) -> Self {
            AppError::Internal(anyhow_like::Error(cause.to_string()))
        }

        /// Attaches the request path to an internal error so it appears in
        /// the Boot-style body. Other variants are returned unchanged.
        pub fn with_path(self, request_path: impl Into<String>) -> Self {
            match self {
                AppError::Internal(cause) | AppError::InternalAt { cause, .. } => {
                    AppError::InternalAt {
                        cause,
                        path: request_path.into(),
                    }
                }
                other => other,
            }
        }

        /// HTTP status the variant maps to.
        pub fn status_code(&self) -> StatusCode {
            match self {
                AppError::NotFound(_) => StatusCode::NOT_FOUND,
                AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
                AppError::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
                AppError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
                AppError::NotAcceptable => StatusCode::NOT_ACCEPTABLE,
                AppError::Internal(_) | AppError::InternalAt { .. } => {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }
        }
    }

    impl fmt::Display for AppError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                AppError::NotFound(e) => write!(f, "not found: {e}"),
                AppError::BadRequest(m) if m.is_empty() => write!(f, "bad request"),
                AppError::BadRequest(m) => write!(f, "bad request: {m}"),
                AppError::UnsupportedMediaType => write!(f, "unsupported media type"),
                AppError::MethodNotAllowed => write!(f, "method not allowed"),
                AppError::NotAcceptable => write!(f, "not acceptable"),
                AppError::Internal(_) | AppError::InternalAt { .. } => {
                    write!(f, "internal server error")
                }
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

    /// Spring's `HttpStatus.name()` for the statuses this service emits.
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

    /// JSON body of the `UserNotFoundException` handler.
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

    fn internal_response(cause: &anyhow_like::Error, path: String) -> Response {
        tracing::error!(error = %cause, path = %path, "internal server error");
        let body = BootErrorBody {
            timestamp: boot_timestamp_now(),
            status: 500,
            error: "Internal Server Error".to_string(),
            path,
        };
        (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
    }

    impl IntoResponse for AppError {
        fn into_response(self) -> Response {
            match self {
                AppError::NotFound(e) => {
                    let body = NotFoundBody {
                        status: http_status_name(StatusCode::NOT_FOUND).map(str::to_string),
                        message: e.message,
                    };
                    (StatusCode::NOT_FOUND, Json(body)).into_response()
                }
                AppError::BadRequest(msg) => {
                    tracing::debug!(reason = %msg, "bad request");
                    StatusCode::BAD_REQUEST.into_response()
                }
                AppError::UnsupportedMediaType => {
                    StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response()
                }
                AppError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED.into_response(),
                AppError::NotAcceptable => StatusCode::NOT_ACCEPTABLE.into_response(),
                AppError::Internal(cause) => internal_response(&cause, String::new()),
                AppError::InternalAt { cause, path } => internal_response(&cause, path),
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

    /// Converts days since 1970-01-01 to (year, month, day).
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
            assert_eq!(
                AppError::BadRequest(String::new()).status_code(),
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                AppError::internal("boom").with_path("/x").status_code(),
                StatusCode::INTERNAL_SERVER_ERROR
            );
        }
    }
}

pub mod models {
    pub mod error_message;
    pub mod user;
}

pub mod repository {
    pub mod user_repo;
}

pub mod services {
    pub mod user_service;
    pub mod user_service_imp;
}

pub mod handlers {
    pub mod user_handlers;
}

use std::sync::Arc;

use axum::{routing::get, Json, Router};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::CorsLayer;

use crate::config::Config;
use crate::models::user::UserRepository;
use crate::services::user_service_imp::UserServiceImp;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Builds the user service from the configured database URL (DATABASE_URL).
/// The pool connects lazily, so no connection is opened until first use.
fn build_user_service(config: &Config) -> UserServiceImp {
    let pool = PgPoolOptions::new()
        .connect_lazy(&config.database_url)
        .expect("DATABASE_URL must be a valid postgres connection string");
    UserServiceImp::new(UserRepository::new(pool))
}

/// Every route the service exposes.
pub fn app(state: AppState) -> Router {
    let service = build_user_service(&state.config);
    Router::new()
        .route("/health", get(health))
        .with_state(state)
        .merge(handlers::user_handlers::user_routes(service))
        .layer(CorsLayer::permissive())
}