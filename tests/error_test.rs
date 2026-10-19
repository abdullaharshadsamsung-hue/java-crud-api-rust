use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde_json::Value;
use tower::ServiceExt;

use migrated_service::config::Config;
use migrated_service::error::{AppError, BootErrorBody, UserNotFoundError};
use migrated_service::{app, AppState};

fn state() -> AppState {
    AppState {
        config: Arc::new(Config {
            port: 0,
            database_url: String::new(),
            secret_key: "test".to_string(),
        }),
    }
}

async fn not_found_msg() -> Result<String, AppError> {
    Err(UserNotFoundError::new("User not found with id 5").into())
}
async fn not_found_null() -> Result<String, AppError> {
    Err(UserNotFoundError::default().into())
}
async fn not_found_empty() -> Result<String, AppError> {
    Err(UserNotFoundError::new("").into())
}
async fn bad_request() -> Result<String, AppError> {
    Err(AppError::BadRequest(String::new()))
}
async fn unsupported() -> Result<String, AppError> {
    Err(AppError::UnsupportedMediaType)
}
async fn not_allowed() -> Result<String, AppError> {
    Err(AppError::MethodNotAllowed)
}
async fn not_acceptable() -> Result<String, AppError> {
    Err(AppError::NotAcceptable)
}
async fn internal() -> Result<String, AppError> {
    Err(AppError::internal("db exploded").with_path("/boom"))
}
async fn sqlx_fail() -> Result<String, AppError> {
    Err(AppError::from(sqlx::Error::RowNotFound))
}

fn router() -> Router {
    let extra: Router = Router::new()
        .route("/users/five", get(not_found_msg).post(not_found_msg).put(not_found_msg))
        .route("/contacts/five", get(not_found_msg).delete(not_found_msg))
        .route("/users/null", get(not_found_null))
        .route("/users/empty", get(not_found_empty))
        .route("/bad", get(bad_request).post(bad_request))
        .route("/unsupported", get(unsupported).post(unsupported))
        .route("/notallowed", get(not_allowed))
        .route("/notacceptable", get(not_acceptable))
        .route("/boom", get(internal))
        .route("/sqlx", get(sqlx_fail))
        .route("/only-get", get(|| async { "ok" }));
    app(state()).merge(extra)
}

async fn send(method: Method, uri: &str) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = router().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, bytes.to_vec())
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("valid json body")
}

#[tokio::test]
async fn user_not_found_with_message_returns_404_error_message() {
    let (status, body) = send(Method::GET, "/users/five").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let v = json(&body);
    assert_eq!(v["status"], "NOT_FOUND");
    assert_eq!(v["message"], "User not found with id 5");
    assert_eq!(v.as_object().unwrap().len(), 2);
}

#[tokio::test]
async fn user_not_found_null_message_serializes_as_null() {
    let (status, body) = send(Method::GET, "/users/null").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let v = json(&body);
    assert_eq!(v["status"], "NOT_FOUND");
    assert!(v.as_object().unwrap().contains_key("message"));
    assert!(v["message"].is_null());
}

#[tokio::test]
async fn user_not_found_empty_message_is_empty_string() {
    let (status, body) = send(Method::GET, "/users/empty").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let v = json(&body);
    assert_eq!(v["status"], "NOT_FOUND");
    assert_eq!(v["message"], "");
}

#[tokio::test]
async fn user_not_found_identical_across_paths_and_methods() {
    let cases = vec![
        (Method::GET, "/users/five"),
        (Method::POST, "/users/five"),
        (Method::PUT, "/users/five"),
        (Method::GET, "/contacts/five"),
        (Method::DELETE, "/contacts/five"),
        (Method::GET, "/users/five?x=1&y=2"),
    ];
    let (_, reference) = send(Method::GET, "/users/five").await;
    let reference = json(&reference);
    for (m, uri) in cases {
        let (status, body) = send(m.clone(), uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{m} {uri}");
        assert_eq!(json(&body), reference, "{m} {uri}");
    }
}

#[tokio::test]
async fn user_not_found_never_becomes_500() {
    let err: AppError = UserNotFoundError::new("x").into();
    assert_eq!(err.status_code(), StatusCode::NOT_FOUND);
    let resp: Response = AppError::from(UserNotFoundError::new("x")).into_response();
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    // with_path leaves NotFound unchanged
    let e = AppError::from(UserNotFoundError::new("y")).with_path("/p");
    match e {
        AppError::NotFound(inner) => assert_eq!(inner.message(), Some("y")),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn bad_request_returns_400_empty_body() {
    for m in [Method::GET, Method::POST] {
        let (status, body) = send(m, "/bad").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.is_empty());
    }
}

#[tokio::test]
async fn unsupported_media_type_returns_415_empty_body() {
    let (status, body) = send(Method::POST, "/unsupported").await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(body.is_empty());
}

#[tokio::test]
async fn method_not_allowed_returns_405() {
    let (status, body) = send(Method::GET, "/notallowed").await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert!(body.is_empty());
    let (status, _) = send(Method::DELETE, "/only-get").await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn not_acceptable_returns_406_empty_body() {
    let (status, body) = send(Method::GET, "/notacceptable").await;
    assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
    assert!(body.is_empty());
}

#[tokio::test]
async fn framework_errors_are_4xx_not_500() {
    for e in [
        AppError::BadRequest(String::new()),
        AppError::UnsupportedMediaType,
        AppError::MethodNotAllowed,
        AppError::NotAcceptable,
    ] {
        assert!(e.status_code().is_client_error(), "{e}");
    }
}

#[tokio::test]
async fn unhandled_errors_fall_through_to_boot_style_500() {
    let (status, body) = send(Method::GET, "/boom").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let b: BootErrorBody = serde_json::from_slice(&body).unwrap();
    assert_eq!(b.status, 500);
    assert_eq!(b.error, "Internal Server Error");
    assert_eq!(b.path, "/boom");
    assert!(!String::from_utf8_lossy(&body).contains("db exploded"));
    let ts = b.timestamp;
    assert_eq!(ts.len(), 29, "{ts}");
    assert!(ts.ends_with("+00:00"));
    assert_eq!(&ts[4..5], "-");
    assert_eq!(&ts[10..11], "T");
    assert_eq!(&ts[19..20], ".");
    let year: i32 = ts[0..4].parse().unwrap();
    assert!(year >= 2024);
}

#[tokio::test]
async fn sqlx_error_maps_to_500() {
    let (status, body) = send(Method::GET, "/sqlx").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let v = json(&body);
    assert_eq!(v["status"], 500);
    assert_eq!(v["path"], "");
}

#[tokio::test]
async fn box_error_maps_to_internal() {
    let boxed: migrated_service::error::BoxError = "oops".into();
    let e = AppError::from(boxed);
    assert_eq!(e.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn health_still_works_alongside_errors() {
    let (status, body) = send(Method::GET, "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["status"], "ok");
}

#[test]
fn user_not_found_error_display_and_accessors() {
    let e = UserNotFoundError::new("msg");
    assert_eq!(e.message(), Some("msg"));
    assert_eq!(e.to_string(), "msg");
    let n = UserNotFoundError::default();
    assert_eq!(n.message(), None);
    assert_eq!(n.to_string(), "UserNotFoundException");
    assert_eq!(AppError::from(e).to_string(), "not found: msg");
    assert_eq!(AppError::BadRequest(String::new()).to_string(), "bad request");
    assert_eq!(
        AppError::internal("cause").with_path("/x").to_string(),
        "internal server error"
    );
}