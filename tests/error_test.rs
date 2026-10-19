//! Integration tests for `src/error.rs` (migrated from the Java `UserNotFoundException`).

use std::error::Error as StdError;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

use migrated_service::config::Config;
use migrated_service::error::{anyhow_like, AppError, BoxError, UserNotFoundError};
use migrated_service::{app, AppState};

fn test_state() -> AppState {
    AppState {
        config: Arc::new(Config {
            port: 0,
            database_url: String::new(),
            secret_key: "test-secret".to_string(),
        }),
    }
}

async fn body_json(resp: Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("valid json")
}

/// The application router plus test routes whose handlers fail with
/// `UserNotFoundError` converted via `?`.
fn router_with_error_routes() -> Router {
    async fn no_message() -> Result<String, AppError> {
        Err(UserNotFoundError::new())?
    }
    async fn with_message() -> Result<String, AppError> {
        Err(UserNotFoundError::with_message("User not found"))?
    }
    async fn with_message_and_cause() -> Result<String, AppError> {
        Err(UserNotFoundError::with_message_and_cause(
            "missing",
            anyhow_like::Error("db down".into()),
        ))?
    }
    async fn with_cause() -> Result<String, AppError> {
        Err(UserNotFoundError::with_cause(anyhow_like::Error(
            "db down".into(),
        )))?
    }
    async fn ok() -> Result<String, AppError> {
        Ok("fine".to_string())
    }

    app(test_state()).merge(
        Router::new()
            .route("/unf/none", get(no_message))
            .route("/unf/message", get(with_message))
            .route("/unf/message-cause", get(with_message_and_cause))
            .route("/unf/cause", get(with_cause))
            .route("/ok", get(ok)),
    )
}

async fn get_req(router: Router, uri: &str) -> Response {
    router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .expect("oneshot")
}

fn assert_is_error<T: StdError + Send + Sync + 'static>() {}

// ---------- UserNotFoundException() ----------

#[test]
fn no_arg_constructor_has_no_message_and_no_cause() {
    let e = UserNotFoundError::new();
    assert!(e.message().is_none());
    assert!(e.source().is_none());
    // Display falls back to a generic text rather than panicking.
    assert_eq!(e.to_string(), "user not found");
}

#[test]
fn default_equals_new() {
    let e = UserNotFoundError::default();
    assert!(e.message().is_none());
    assert!(e.source().is_none());
}

#[test]
fn is_a_proper_error_type() {
    assert_is_error::<UserNotFoundError>();
    let boxed: BoxError = Box::new(UserNotFoundError::new());
    assert!(boxed.downcast_ref::<UserNotFoundError>().is_some());
}

// ---------- UserNotFoundException(String) ----------

#[test]
fn message_constructor_stores_message_exactly() {
    let e = UserNotFoundError::with_message("User not found");
    assert_eq!(e.message(), Some("User not found"));
    assert!(e.source().is_none());
    assert_eq!(e.to_string(), "User not found");
}

#[test]
fn message_constructor_accepts_owned_string() {
    let msg = String::from("User are not available");
    let e = UserNotFoundError::with_message(msg.clone());
    assert_eq!(e.message(), Some(msg.as_str()));
}

#[test]
fn null_message_equivalent_is_absent_message() {
    // Java `new UserNotFoundException((String) null)` -> Rust `new()`.
    let e = UserNotFoundError::new();
    assert_eq!(e.message(), None);
    assert!(e.source().is_none());
}

#[test]
fn empty_message_is_stored() {
    let e = UserNotFoundError::with_message("");
    assert_eq!(e.message(), Some(""));
}

// ---------- UserNotFoundException(String, Throwable) ----------

#[test]
fn message_and_cause_constructor_stores_both() {
    let e = UserNotFoundError::with_message_and_cause(
        "missing",
        anyhow_like::Error("runtime failure".into()),
    );
    assert_eq!(e.message(), Some("missing"));
    let src = e.source().expect("cause present");
    let cause = src
        .downcast_ref::<anyhow_like::Error>()
        .expect("same cause type");
    assert_eq!(cause.0, "runtime failure");
    // message is not derived from the cause
    assert_ne!(e.message(), Some("runtime failure"));
    assert_eq!(e.to_string(), "missing");
}

#[test]
fn message_and_cause_preserves_cause_identity() {
    let cause: BoxError = Box::new(anyhow_like::Error("x".into()));
    let ptr = cause.as_ref() as *const (dyn StdError + Send + Sync) as *const u8;
    let e = UserNotFoundError::with_message_and_cause("missing", cause);
    let src_ptr = e.source().unwrap() as *const dyn StdError as *const u8;
    assert_eq!(ptr, src_ptr);
}

#[test]
fn message_and_cause_accepts_string_cause() {
    let e = UserNotFoundError::with_message_and_cause("missing", "io broke");
    assert_eq!(e.message(), Some("missing"));
    assert_eq!(e.source().unwrap().to_string(), "io broke");
}

#[test]
fn message_with_null_cause_equivalent_has_no_source() {
    // Java `(message, null)` -> Rust `with_message(message)`.
    let e = UserNotFoundError::with_message("missing");
    assert_eq!(e.message(), Some("missing"));
    assert!(e.source().is_none());
}

#[test]
fn nested_cause_chain_is_walkable() {
    let inner = UserNotFoundError::with_message("inner");
    let outer = UserNotFoundError::with_message_and_cause("outer", inner);
    let first = outer.source().unwrap();
    assert_eq!(first.to_string(), "inner");
    assert!(first.source().is_none());
}

// ---------- UserNotFoundException(Throwable) ----------

#[test]
fn cause_constructor_derives_message_from_cause() {
    let e = UserNotFoundError::with_cause(anyhow_like::Error("db down".into()));
    assert_eq!(e.message(), Some("db down"));
    let src = e.source().expect("cause present");
    assert_eq!(src.to_string(), "db down");
    assert_eq!(
        src.downcast_ref::<anyhow_like::Error>().unwrap().0,
        "db down"
    );
    assert_eq!(e.to_string(), "db down");
}

#[test]
fn null_cause_equivalent_has_no_message_and_no_cause() {
    // Java `new UserNotFoundException((Throwable) null)` -> Rust `new()`.
    let e = UserNotFoundError::new();
    assert!(e.message().is_none());
    assert!(e.source().is_none());
}

// ---------- (message, cause, enableSuppression, writableStackTrace) ----------

#[test]
fn four_arg_constructor_collapses_into_message_and_cause() {
    // Flags have no Rust equivalent; behaviour must equal (message, cause).
    let e = UserNotFoundError::with_message_and_cause(
        "msg",
        anyhow_like::Error("cause".into()),
    );
    assert_eq!(e.message(), Some("msg"));
    assert_eq!(e.source().unwrap().to_string(), "cause");
}

// ---------- Conversion into AppError ----------

#[test]
fn converts_into_app_error_user_not_found() {
    let app_err: AppError = UserNotFoundError::with_message("User not found").into();
    match app_err {
        AppError::UserNotFound(m) => assert_eq!(m, "User not found"),
        other => panic!("unexpected variant {other:?}"),
    }
}

#[test]
fn missing_message_converts_to_empty_string() {
    let app_err: AppError = UserNotFoundError::new().into();
    match app_err {
        AppError::UserNotFound(m) => assert!(m.is_empty()),
        other => panic!("unexpected variant {other:?}"),
    }
}

#[test]
fn app_error_display() {
    assert_eq!(AppError::UserNotFound("abc".into()).to_string(), "abc");
    assert_eq!(AppError::NotFound.to_string(), "not found");
    assert_eq!(AppError::BadRequest("x".into()).to_string(), "bad request: x");
    assert_eq!(AppError::Unauthorized.to_string(), "unauthorized");
    let internal: AppError = anyhow_like::Error("boom".into()).into();
    assert_eq!(internal.to_string(), "boom");
}

// ---------- HTTP rendering ----------

#[tokio::test]
async fn user_not_found_renders_404_with_message() {
    let resp = AppError::UserNotFound("User are not available".into()).into_response();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(resp).await,
        json!({ "status": "NOT_FOUND", "message": "User are not available" })
    );
}

#[tokio::test]
async fn user_not_found_empty_message_renders_null() {
    let resp = AppError::UserNotFound(String::new()).into_response();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(resp).await,
        json!({ "status": "NOT_FOUND", "message": null })
    );
}

#[tokio::test]
async fn other_variants_render_expected_status_and_body() {
    let cases: Vec<(AppError, StatusCode, Value)> = vec![
        (AppError::NotFound, StatusCode::NOT_FOUND, json!({"error": "not found"})),
        (
            AppError::BadRequest("bad id".into()),
            StatusCode::BAD_REQUEST,
            json!({"error": "bad request: bad id"}),
        ),
        (
            AppError::Unauthorized,
            StatusCode::UNAUTHORIZED,
            json!({"error": "unauthorized"}),
        ),
        (
            AppError::Internal(anyhow_like::Error("secret detail".into())),
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": "internal server error"}),
        ),
    ];
    for (err, status, body) in cases {
        let resp = err.into_response();
        assert_eq!(resp.status(), status);
        assert_eq!(body_json(resp).await, body);
    }
}

// ---------- Through the Router ----------

#[tokio::test]
async fn health_still_works() {
    let resp = get_req(router_with_error_routes(), "/health").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await, json!({"status": "ok"}));
}

#[tokio::test]
async fn handler_ok_path_unaffected() {
    let resp = get_req(router_with_error_routes(), "/ok").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn handler_with_no_message_error_returns_404_null_message() {
    let resp = get_req(router_with_error_routes(), "/unf/none").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(resp).await,
        json!({ "status": "NOT_FOUND", "message": null })
    );
}

#[tokio::test]
async fn handler_with_message_error_returns_404_message() {
    let resp = get_req(router_with_error_routes(), "/unf/message").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(resp).await,
        json!({ "status": "NOT_FOUND", "message": "User not found" })
    );
}

#[tokio::test]
async fn handler_with_message_and_cause_uses_message_only() {
    let resp = get_req(router_with_error_routes(), "/unf/message-cause").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(resp).await,
        json!({ "status": "NOT_FOUND", "message": "missing" })
    );
}

#[tokio::test]
async fn handler_with_cause_uses_derived_message() {
    let resp = get_req(router_with_error_routes(), "/unf/cause").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(resp).await,
        json!({ "status": "NOT_FOUND", "message": "db down" })
    );
}