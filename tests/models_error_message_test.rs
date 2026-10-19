use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use tower::ServiceExt;

use migrated_service::models::error_message::{http_status_name, ErrorMessage};

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

// ---- no-args constructor ---------------------------------------------------

#[test]
fn new_has_both_fields_unset() {
    let e = ErrorMessage::new();
    assert_eq!(e.status(), None);
    assert_eq!(e.message(), None);
}

#[test]
fn default_equals_new() {
    assert_eq!(ErrorMessage::default(), ErrorMessage::new());
}

// ---- all-args constructor --------------------------------------------------

#[test]
fn with_stores_status_and_message_in_order() {
    let e = ErrorMessage::with("NOT_FOUND", "Contact not found");
    assert_eq!(e.status(), Some("NOT_FOUND"));
    assert_eq!(e.message(), Some("Contact not found"));
}

#[test]
fn with_stores_values_without_transformation() {
    let e = ErrorMessage::with("not_found ", "");
    assert_eq!(e.status(), Some("not_found "));
    assert_eq!(e.message(), Some(""));
}

#[test]
fn null_status_and_null_message_equivalent() {
    let mut e = ErrorMessage::with("X", "y");
    e.set_status(None);
    e.set_message(None);
    assert_eq!(e.status(), None);
    assert_eq!(e.message(), None);
    assert_eq!(e, ErrorMessage::new());
}

#[test]
fn from_status_code_maps_to_spring_names() {
    let e = ErrorMessage::from_status_code(StatusCode::NOT_FOUND, "Contact not found");
    assert_eq!(e, ErrorMessage::with("NOT_FOUND", "Contact not found"));
    assert_eq!(http_status_name(StatusCode::BAD_REQUEST), "BAD_REQUEST");
    assert_eq!(
        http_status_name(StatusCode::INTERNAL_SERVER_ERROR),
        "INTERNAL_SERVER_ERROR"
    );
    assert_eq!(http_status_name(StatusCode::OK), "OK");
    assert_eq!(http_status_name(StatusCode::UNAUTHORIZED), "UNAUTHORIZED");
}

// ---- status accessors ------------------------------------------------------

#[test]
fn set_status_then_get() {
    let mut e = ErrorMessage::new();
    e.set_status(Some("BAD_REQUEST".to_string()));
    assert_eq!(e.status(), Some("BAD_REQUEST"));
}

#[test]
fn set_status_none_then_get() {
    let mut e = ErrorMessage::with("NOT_FOUND", "m");
    e.set_status(None);
    assert_eq!(e.status(), None);
    assert_eq!(e.message(), Some("m"));
}

#[test]
fn status_getter_returns_last_value() {
    let mut e = ErrorMessage::with("NOT_FOUND", "m");
    e.set_status(Some("A".into()));
    e.set_status(Some("B".into()));
    assert_eq!(e.status(), Some("B"));
}

// ---- message accessors -----------------------------------------------------

#[test]
fn set_message_then_get() {
    let mut e = ErrorMessage::new();
    e.set_message(Some("Invalid input".to_string()));
    assert_eq!(e.message(), Some("Invalid input"));
    assert_eq!(e.status(), None);
}

#[test]
fn set_message_none_then_get() {
    let mut e = ErrorMessage::with("NOT_FOUND", "m");
    e.set_message(None);
    assert_eq!(e.message(), None);
}

// ---- equality / hash -------------------------------------------------------

#[test]
fn equal_values_are_equal_with_equal_hashes() {
    let a = ErrorMessage::with("NOT_FOUND", "x");
    let b = ErrorMessage::with("NOT_FOUND", "x");
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));

    let n1 = ErrorMessage::new();
    let n2 = ErrorMessage::new();
    assert_eq!(n1, n2);
    assert_eq!(hash_of(&n1), hash_of(&n2));
}

#[test]
fn differing_values_are_not_equal() {
    let a = ErrorMessage::with("NOT_FOUND", "x");
    assert_ne!(a, ErrorMessage::with("BAD_REQUEST", "x"));
    assert_ne!(a, ErrorMessage::with("NOT_FOUND", "y"));
    assert_ne!(a, ErrorMessage::new());
}

#[test]
fn not_equal_to_absent_value() {
    let a = Some(ErrorMessage::with("NOT_FOUND", "x"));
    assert_ne!(a, None);
}

#[test]
fn equality_reflexive_symmetric_transitive() {
    let a = ErrorMessage::with("S", "m");
    let b = a.clone();
    let c = ErrorMessage::with("S", "m");
    assert_eq!(a, a);
    assert_eq!(a == b, b == a);
    assert!(a == b && b == c && a == c);
}

// ---- toString --------------------------------------------------------------

#[test]
fn display_with_values() {
    let e = ErrorMessage::with("NOT_FOUND", "x");
    assert_eq!(e.to_string(), "ErrorMessage(status=NOT_FOUND, message=x)");
}

#[test]
fn display_with_nulls() {
    assert_eq!(
        ErrorMessage::new().to_string(),
        "ErrorMessage(status=null, message=null)"
    );
}

// ---- JSON ------------------------------------------------------------------

#[test]
fn serializes_exactly_two_properties() {
    let v = serde_json::to_value(ErrorMessage::with("NOT_FOUND", "x")).unwrap();
    let obj = v.as_object().unwrap();
    assert_eq!(obj.len(), 2);
    assert_eq!(v["status"], "NOT_FOUND");
    assert_eq!(v["message"], "x");
}

#[test]
fn serializes_unset_as_null() {
    let s = serde_json::to_string(&ErrorMessage::new()).unwrap();
    assert_eq!(s, r#"{"status":null,"message":null}"#);
}

#[test]
fn deserializes_full_partial_and_empty() {
    let e: ErrorMessage =
        serde_json::from_str(r#"{"status":"NOT_FOUND","message":"x"}"#).unwrap();
    assert_eq!(e, ErrorMessage::with("NOT_FOUND", "x"));

    let p: ErrorMessage = serde_json::from_str(r#"{"message":"only"}"#).unwrap();
    assert_eq!(p.status(), None);
    assert_eq!(p.message(), Some("only"));

    let n: ErrorMessage = serde_json::from_str(r#"{"status":null,"message":null}"#).unwrap();
    assert_eq!(n, ErrorMessage::new());

    let empty: ErrorMessage = serde_json::from_str("{}").unwrap();
    assert_eq!(empty, ErrorMessage::new());
}

#[test]
fn json_roundtrip() {
    let e = ErrorMessage::with("BAD_REQUEST", "Invalid input");
    let back: ErrorMessage = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
    assert_eq!(back, e);
}

// ---- HTTP-level behaviour via an axum Router -------------------------------

async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorMessage::from_status_code(
            StatusCode::NOT_FOUND,
            "Contact not found",
        )),
    )
}

async fn echo(Json(body): Json<ErrorMessage>) -> Json<ErrorMessage> {
    Json(body)
}

fn router() -> Router {
    Router::new()
        .route("/missing", get(not_found))
        .route("/echo", post(echo))
}

#[tokio::test]
async fn error_response_body_shape() {
    let resp = router()
        .oneshot(Request::builder().uri("/missing").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"status":"NOT_FOUND","message":"Contact not found"})
    );
    let e: ErrorMessage = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(e.status(), Some("NOT_FOUND"));
}

#[tokio::test]
async fn echo_roundtrip_with_nulls() {
    let resp = router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/echo")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"status":null,"message":"Invalid input"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let e: ErrorMessage = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(e.status(), None);
    assert_eq!(e.message(), Some("Invalid input"));
}