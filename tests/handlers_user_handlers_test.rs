//! Integration tests for `src/handlers/user_handlers.rs`.
//!
//! The router is driven in-process with `tower::ServiceExt::oneshot`.
//!
//! Some tests need a database. They run only when `DATABASE_URL` is set and
//! are skipped otherwise. Tests that fail before the service is reached run
//! unconditionally. Examples are bean validation, malformed JSON, missing
//! content type and non-numeric path ids.

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

use migrated_service::{app, AppState};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

async fn build_app() -> Router {
    let state = AppState::from_env()
        .await
        .expect("failed to build AppState from environment");
    app(state)
}

fn db_available() -> bool {
    std::env::var("DATABASE_URL").is_ok()
}

fn unique(prefix: &str) -> String {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}_{n}")
}

async fn send(
    app: Router,
    method: Method,
    uri: &str,
    body: Option<(String, Option<&str>)>,
) -> (StatusCode, Option<String>, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri.to_string());
    let req_body = match body {
        Some((b, ct)) => {
            if let Some(ct) = ct {
                builder = builder.header(header::CONTENT_TYPE, ct);
            }
            Body::from(b)
        }
        None => Body::empty(),
    };
    let resp = app
        .oneshot(builder.body(req_body).unwrap())
        .await
        .expect("request failed");
    let status = resp.status();
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap().to_string());
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap().to_vec();
    (status, ct, bytes)
}

async fn send_json(
    app: Router,
    method: Method,
    uri: &str,
    body: Value,
) -> (StatusCode, Option<String>, Vec<u8>) {
    send(
        app,
        method,
        uri,
        Some((body.to_string(), Some("application/json"))),
    )
    .await
}

fn as_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("response body is not valid JSON")
}

async fn list_users(app: Router) -> Vec<Value> {
    let (status, _, bytes) = send(app, Method::GET, "/get_user_data", None).await;
    assert_eq!(status, StatusCode::OK);
    as_json(&bytes).as_array().expect("expected JSON array").clone()
}

/// Saves a user through the HTTP API and returns its id, found by email.
async fn create_user(app: Router, name: &str, email: &str) -> i64 {
    let (status, _, bytes) = send_json(
        app.clone(),
        Method::POST,
        "/save_user_data",
        json!({"name": name, "email": email, "password": "pw", "role": "USER", "about": "a"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "save failed: {:?}", String::from_utf8_lossy(&bytes));
    let users = list_users(app).await;
    users
        .iter()
        .find(|u| u["email"] == json!(email))
        .expect("saved user not in list")["id"]
        .as_i64()
        .unwrap()
}

// ---------------------------------------------------------------------------
// saveUser
// ---------------------------------------------------------------------------

#[tokio::test]
async fn save_user_valid_body_returns_fixed_text() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let email = unique("save") + "@x.com";
    let (status, ct, bytes) = send_json(
        app.clone(),
        Method::POST,
        "/save_user_data",
        json!({"name": "Alice", "email": email.clone(), "password": "p", "role": "ADMIN", "about": "hi"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.unwrap_or_default().starts_with("text/plain"));
    assert_eq!(String::from_utf8(bytes).unwrap(), "User data saved successfully!");

    // Persisted
    let users = list_users(app).await;
    let saved = users.iter().find(|u| u["email"] == json!(email)).expect("not persisted");
    assert_eq!(saved["name"], json!("Alice"));
    assert_eq!(saved["role"], json!("ADMIN"));
}

#[tokio::test]
async fn save_user_missing_name_is_400_and_not_persisted() {
    let app = build_app().await;
    let email = unique("noname") + "@x.com";
    let (status, _, bytes) = send_json(
        app.clone(),
        Method::POST,
        "/save_user_data",
        json!({"email": email.clone()}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(bytes.is_empty());
    assert_ne!(String::from_utf8_lossy(&bytes), "User data saved successfully!");

    if db_available() {
        let users = list_users(app).await;
        assert!(users.iter().all(|u| u["email"] != json!(email)));
    }
}

#[tokio::test]
async fn save_user_blank_name_is_400() {
    let app = build_app().await;
    for name in ["", "   "] {
        let (status, _, bytes) = send_json(
            app.clone(),
            Method::POST,
            "/save_user_data",
            json!({"name": name, "email": "e@x.com"}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "name={name:?}");
        assert!(bytes.is_empty());
    }
}

#[tokio::test]
async fn save_user_malformed_json_is_400() {
    let app = build_app().await;
    let (status, _, bytes) = send(
        app,
        Method::POST,
        "/save_user_data",
        Some(("{\"name\": ".to_string(), Some("application/json"))),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(bytes.is_empty());
}

#[tokio::test]
async fn save_user_empty_json_body_is_400() {
    let app = build_app().await;
    let (status, _, _) = send(
        app,
        Method::POST,
        "/save_user_data",
        Some((String::new(), Some("application/json"))),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn save_user_missing_content_type_is_415() {
    let app = build_app().await;
    let (status, _, bytes) = send(
        app,
        Method::POST,
        "/save_user_data",
        Some((json!({"name": "x"}).to_string(), None)),
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(bytes.is_empty());
}

// ---------------------------------------------------------------------------
// fetchUserList
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_user_list_returns_json_array_containing_saved_users() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let e1 = unique("l1") + "@x.com";
    let e2 = unique("l2") + "@x.com";
    create_user(app.clone(), "L1", &e1).await;
    create_user(app.clone(), "L2", &e2).await;

    let (status, ct, bytes) = send(app, Method::GET, "/get_user_data", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.unwrap_or_default().starts_with("application/json"));
    let v = as_json(&bytes);
    let arr = v.as_array().expect("array");
    assert!(arr.iter().any(|u| u["email"] == json!(e1)));
    assert!(arr.iter().any(|u| u["email"] == json!(e2)));
    for u in arr {
        for k in ["id", "name", "email", "password", "role", "about"] {
            assert!(u.get(k).is_some(), "missing key {k}");
        }
    }
}

// ---------------------------------------------------------------------------
// fetchUserById
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_user_by_id_existing_returns_user() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let email = unique("byid") + "@x.com";
    let id = create_user(app.clone(), "ById", &email).await;

    let (status, _, bytes) = send(app, Method::GET, &format!("/get_user_data/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    let v = as_json(&bytes);
    assert_eq!(v["id"], json!(id));
    assert_eq!(v["name"], json!("ById"));
    assert_eq!(v["email"], json!(email));
}

#[tokio::test]
async fn fetch_user_by_id_missing_returns_404_error_message() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let (status, _, bytes) = send(app, Method::GET, "/get_user_data/2147483000", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let v = as_json(&bytes);
    assert_eq!(v["status"], json!("NOT_FOUND"));
    assert_eq!(v["message"], json!("User are not available"));
}

#[tokio::test]
async fn fetch_user_by_id_non_integer_is_400() {
    let app = build_app().await;
    for bad in ["abc", "1.5", "99999999999"] {
        let (status, _, bytes) =
            send(app.clone(), Method::GET, &format!("/get_user_data/{bad}"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "id={bad}");
        assert!(bytes.is_empty());
    }
}

// ---------------------------------------------------------------------------
// deleteUser
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_user_existing_returns_fixed_text_and_removes() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let email = unique("del") + "@x.com";
    let id = create_user(app.clone(), "Del", &email).await;

    let (status, ct, bytes) =
        send(app.clone(), Method::DELETE, &format!("/delete_user_data/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.unwrap_or_default().starts_with("text/plain"));
    assert_eq!(String::from_utf8(bytes).unwrap(), "user data deleted Successfully");

    let (status, _, _) = send(app.clone(), Method::GET, &format!("/get_user_data/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let users = list_users(app).await;
    assert!(users.iter().all(|u| u["email"] != json!(email)));
}

#[tokio::test]
async fn delete_user_missing_id_is_200_or_500() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let (status, _, bytes) =
        send(app, Method::DELETE, "/delete_user_data/2147483001", None).await;
    match status {
        StatusCode::OK => assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "user data deleted Successfully"
        ),
        StatusCode::INTERNAL_SERVER_ERROR => {
            let v = as_json(&bytes);
            assert_eq!(v["status"], json!(500));
            assert_eq!(v["error"], json!("Internal Server Error"));
            assert_eq!(v["path"], json!("/delete_user_data/2147483001"));
        }
        other => panic!("unexpected status {other}"),
    }
}

#[tokio::test]
async fn delete_user_non_integer_is_400() {
    let app = build_app().await;
    let (status, _, bytes) = send(app, Method::DELETE, "/delete_user_data/xyz", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(bytes.is_empty());
}

// ---------------------------------------------------------------------------
// updateUser
// ---------------------------------------------------------------------------

#[tokio::test]
async fn update_user_echoes_request_body_with_path_id() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let email = unique("upd") + "@x.com";
    let id = create_user(app.clone(), "Before", &email).await;

    let new_email = unique("upd2") + "@x.com";
    let body = json!({"id": 0, "name": "After", "email": new_email.clone(), "password": "np", "role": "R", "about": "ab"});
    let (status, _, bytes) = send_json(
        app.clone(),
        Method::PUT,
        &format!("/update_user_data/{id}"),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v = as_json(&bytes);
    assert_eq!(v["id"], json!(id));
    assert_eq!(v["name"], json!("After"));
    assert_eq!(v["email"], json!(new_email));
    assert_eq!(v["password"], json!("np"));
    assert_eq!(v["role"], json!("R"));
    assert_eq!(v["about"], json!("ab"));

    let (status, _, bytes) = send(app, Method::GET, &format!("/get_user_data/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(as_json(&bytes)["name"], json!("After"));
}

#[tokio::test]
async fn update_user_echo_preserves_null_fields_as_sent() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let email = unique("updnull") + "@x.com";
    let id = create_user(app.clone(), "N", &email).await;

    let (status, _, bytes) = send_json(
        app,
        Method::PUT,
        &format!("/update_user_data/{id}"),
        json!({"name": "OnlyName", "email": email.clone()}),
    )
    .await;
    // The echo must not be reloaded from the DB. Fields that were not sent come back null.
    if status == StatusCode::OK {
        let v = as_json(&bytes);
        assert_eq!(v["name"], json!("OnlyName"));
        assert!(v["password"].is_null());
        assert!(v["role"].is_null());
        assert!(v["about"].is_null());
    } else {
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}

#[tokio::test]
async fn update_user_invalid_body_not_rejected_by_controller() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let email = unique("updinv") + "@x.com";
    let id = create_user(app.clone(), "Inv", &email).await;

    let (status, _, bytes) = send_json(
        app,
        Method::PUT,
        &format!("/update_user_data/{id}"),
        json!({"name": "", "email": email}),
    )
    .await;
    // The controller does no validation. Any rejection comes from the service.
    // The service can return 200 (echo), 400 (its own validation) or 500.
    match status {
        StatusCode::OK => {
            let v = as_json(&bytes);
            assert_eq!(v["name"], json!(""));
            assert_eq!(v["id"], json!(id));
        }
        StatusCode::BAD_REQUEST | StatusCode::INTERNAL_SERVER_ERROR => {}
        other => panic!("unexpected status {other}"),
    }
}

#[tokio::test]
async fn update_user_nonexistent_id_is_200_echo_or_error() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let email = unique("updmiss") + "@x.com";
    let (status, _, bytes) = send_json(
        app,
        Method::PUT,
        "/update_user_data/2147483002",
        json!({"name": "Ghost", "email": email}),
    )
    .await;
    match status {
        StatusCode::OK => {
            let v = as_json(&bytes);
            assert_eq!(v["id"], json!(2147483002));
            assert_eq!(v["name"], json!("Ghost"));
        }
        StatusCode::INTERNAL_SERVER_ERROR | StatusCode::NOT_FOUND => {}
        other => panic!("unexpected status {other}"),
    }
}

#[tokio::test]
async fn update_user_non_integer_id_is_400() {
    let app = build_app().await;
    let (status, _, bytes) = send_json(
        app,
        Method::PUT,
        "/update_user_data/notanumber",
        json!({"name": "x"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(bytes.is_empty());
}

#[tokio::test]
async fn update_user_malformed_json_is_400() {
    let app = build_app().await;
    let (status, _, _) = send(
        app,
        Method::PUT,
        "/update_user_data/1",
        Some(("not json".to_string(), Some("application/json"))),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_user_missing_content_type_is_415() {
    let app = build_app().await;
    let (status, _, _) = send(
        app,
        Method::PUT,
        "/update_user_data/1",
        Some((json!({"name": "x"}).to_string(), None)),
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

// ---------------------------------------------------------------------------
// getUserNameByName
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_user_by_name_found_returns_user() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let name = unique("Named");
    let email = unique("named") + "@x.com";
    let id = create_user(app.clone(), &name, &email).await;

    let (status, _, bytes) =
        send(app, Method::GET, &format!("/get_user_name/name/{name}"), None).await;
    assert_eq!(status, StatusCode::OK);
    let v = as_json(&bytes);
    assert_eq!(v["id"], json!(id));
    assert_eq!(v["name"], json!(name));
    assert_eq!(v["email"], json!(email));
}

#[tokio::test]
async fn get_user_by_name_not_found_returns_200_empty_body() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let name = unique("Nobody");
    let (status, _, bytes) =
        send(app, Method::GET, &format!("/get_user_name/name/{name}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(bytes.is_empty());
}

// ---------------------------------------------------------------------------
// global invariants
// ---------------------------------------------------------------------------

#[tokio::test]
async fn routes_are_at_root_without_prefix() {
    let app = build_app().await;
    let (status, _, _) = send(app, Method::GET, "/api/get_user_data", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn success_uses_200_not_201_or_204() {
    if !db_available() {
        return;
    }
    let app = build_app().await;
    let email = unique("codes") + "@x.com";
    let id = create_user(app.clone(), "Codes", &email).await;
    let (s, _, _) = send(app.clone(), Method::DELETE, &format!("/delete_user_data/{id}"), None).await;
    assert_eq!(s, StatusCode::OK);
}