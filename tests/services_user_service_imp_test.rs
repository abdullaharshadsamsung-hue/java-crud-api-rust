//! Tests for `src/services/user_service_imp.rs` (`UserServiceImp`).
//!
//! These tests need a live PostgreSQL database reachable through the
//! `DATABASE_URL` environment variable. When it is not set, each test returns
//! early and passes without checking anything.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::Value;
use tower::ServiceExt;

use migrated_service::error::AppError;
use migrated_service::models::user::{User, UserRepository};
use migrated_service::services::user_service_imp::UserServiceImp;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Returns a suffix that is unique across test runs and across tests in this run.
fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("{prefix}_{nanos}_{n}")
}

/// Connects to `DATABASE_URL`, or returns `None` so the caller can skip.
async fn service() -> Option<Arc<UserServiceImp>> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("DATABASE_URL not set; skipping DB-backed test");
            return None;
        }
    };
    let pool = sqlx::PgPool::connect(&url).await.expect("connect to database");
    let repo = UserRepository::init(pool).await.expect("init schema");
    Some(Arc::new(UserServiceImp::new(repo)))
}

fn new_user(name: &str) -> User {
    User::builder()
        .name(name.to_string())
        .email(format!("{}@example.com", unique("mail")))
        .password("secret")
        .role("java developer")
        .about("Sr")
        .build()
}

/// Reads the response body and parses it as JSON (`Null` for an empty body).
async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    }
}

/// Small router that exposes the service so errors render through axum.
fn router(svc: Arc<UserServiceImp>, id: i32) -> Router {
    let s1 = svc.clone();
    let s2 = svc.clone();
    let s3 = svc.clone();
    Router::new()
        .route(
            "/user",
            get(move || {
                let s = s1.clone();
                async move { s.fetch_user_by_id(id).await.map(Json) }
            }),
        )
        .route(
            "/users",
            get(move || {
                let s = s2.clone();
                async move { s.fetch_user_list().await.map(Json) }
            }),
        )
        .route(
            "/delete",
            get(move || {
                let s = s3.clone();
                async move {
                    s.delete_user(id).await.map(|_| StatusCode::OK)
                }
            }),
        )
}

async fn call(app: Router, uri: &str) -> axum::response::Response {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

// ---------------------------------------------------------------- saveUser

#[tokio::test]
async fn save_user_returns_saved_entity_with_generated_id() {
    let Some(svc) = service().await else { return };
    let name = unique("save");
    let input = new_user(&name);
    let saved = svc.save_user(input.clone()).await.unwrap();
    assert_ne!(saved.id, 0);
    assert_eq!(saved.name, input.name);
    assert_eq!(saved.email, input.email);
    assert_eq!(saved.role, input.role);
    assert_eq!(saved.about, input.about);

    let stored = svc.fetch_user_by_id(saved.id).await.unwrap();
    assert_eq!(stored, saved);
}

#[tokio::test]
async fn save_user_with_blank_name_is_rejected() {
    let Some(svc) = service().await else { return };
    let err = svc.save_user(User::default()).await.unwrap_err();
    assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
}

// ----------------------------------------------------------- fetchUserList

#[tokio::test]
async fn fetch_user_list_contains_saved_users() {
    let Some(svc) = service().await else { return };
    let a = svc.save_user(new_user(&unique("list_a"))).await.unwrap();
    let b = svc.save_user(new_user(&unique("list_b"))).await.unwrap();
    let list = svc.fetch_user_list().await.unwrap();
    assert!(list.contains(&a));
    assert!(list.contains(&b));
}

#[tokio::test]
async fn fetch_user_list_is_read_only_and_served_as_json_array() {
    let Some(svc) = service().await else { return };
    let before = svc.fetch_user_list().await.unwrap();

    let resp = call(router(svc.clone(), 0), "/users").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json.is_array());

    let after = svc.fetch_user_list().await.unwrap();
    assert_eq!(before.len(), after.len());
}

#[tokio::test]
async fn fetch_user_list_empty_table_returns_empty_vec() {
    let Some(svc) = service().await else { return };
    // Work in a dedicated schema so we can see an empty table without
    // touching the shared data.
    let url = std::env::var("DATABASE_URL").unwrap();
    let schema = unique("empty").to_lowercase();
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let schema_for_conn = schema.clone();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .after_connect(move |conn, _| {
            let s = schema_for_conn.clone();
            Box::pin(async move {
                sqlx::query(&format!("SET search_path TO {s}"))
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    let empty_svc = UserServiceImp::new(UserRepository::new(pool.clone()));
    let list = empty_svc.fetch_user_list().await.unwrap();
    assert!(list.is_empty());
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    drop(svc);
}

// ----------------------------------------------------------- fetchUserById

#[tokio::test]
async fn fetch_user_by_id_returns_existing_user() {
    let Some(svc) = service().await else { return };
    let saved = svc.save_user(new_user(&unique("byid"))).await.unwrap();
    let found = svc.fetch_user_by_id(saved.id).await.unwrap();
    assert_eq!(found, saved);

    let resp = call(router(svc.clone(), saved.id), "/user").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["id"], saved.id);
    assert_eq!(json["name"], saved.name.clone().unwrap());
}

#[tokio::test]
async fn fetch_user_by_id_missing_is_not_found() {
    let Some(svc) = service().await else { return };
    let err = svc.fetch_user_by_id(-12345).await.unwrap_err();
    assert_eq!(err.status_code(), StatusCode::NOT_FOUND);
    assert!(matches!(err, AppError::NotFound(_)));

    let resp = call(router(svc.clone(), -12345), "/user").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "NOT_FOUND");
    assert_eq!(json["message"], "User are not available");
}

// --------------------------------------------------------------- deleteUser

#[tokio::test]
async fn delete_user_removes_existing_row() {
    let Some(svc) = service().await else { return };
    let saved = svc.save_user(new_user(&unique("del"))).await.unwrap();
    svc.delete_user(saved.id).await.unwrap();
    let err = svc.fetch_user_by_id(saved.id).await.unwrap_err();
    assert_eq!(err.status_code(), StatusCode::NOT_FOUND);
    assert!(!svc.user_dao().exists_by_id(saved.id).await.unwrap());
}

#[tokio::test]
async fn delete_user_missing_id_is_internal_error_not_not_found() {
    let Some(svc) = service().await else { return };
    let err = svc.delete_user(-99999).await.unwrap_err();
    assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!matches!(err, AppError::NotFound(_)));

    let resp = call(router(svc.clone(), -99999), "/delete").await;
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn delete_user_via_router_succeeds_for_existing() {
    let Some(svc) = service().await else { return };
    let saved = svc.save_user(new_user(&unique("del_r"))).await.unwrap();
    let resp = call(router(svc.clone(), saved.id), "/delete").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(!svc.user_dao().exists_by_id(saved.id).await.unwrap());
}

// --------------------------------------------------------------- updateUser

#[tokio::test]
async fn update_user_overrides_id_and_overwrites_row() {
    let Some(svc) = service().await else { return };
    let saved = svc.save_user(new_user(&unique("upd"))).await.unwrap();
    let new_name = unique("upd_new");
    let mut changed = new_user(&new_name);
    changed.id = 424_242; // must be ignored in favour of the id argument
    svc.update_user(saved.id, changed.clone()).await.unwrap();

    let stored = svc.fetch_user_by_id(saved.id).await.unwrap();
    assert_eq!(stored.id, saved.id);
    assert_eq!(stored.name.as_deref(), Some(new_name.as_str()));
    assert_eq!(stored.email, changed.email);
}

#[tokio::test]
async fn update_user_none_fields_overwrite_with_null() {
    let Some(svc) = service().await else { return };
    let saved = svc.save_user(new_user(&unique("upd_null"))).await.unwrap();
    let only_name = User::builder().name(unique("only")).build();
    svc.update_user(saved.id, only_name).await.unwrap();
    let stored = svc.fetch_user_by_id(saved.id).await.unwrap();
    assert!(stored.email.is_none());
    assert!(stored.about.is_none());
}

#[tokio::test]
async fn update_user_absent_id_inserts_without_existence_check() {
    let Some(svc) = service().await else { return };
    let before = svc.user_dao().count().await.unwrap();
    let name = unique("upsert");
    svc.update_user(-777_777, new_user(&name)).await.unwrap();
    let after = svc.user_dao().count().await.unwrap();
    assert_eq!(after, before + 1);
    let found = svc.get_user_name_by_name(&name).await.unwrap();
    assert!(found.is_some());
}

// -------------------------------------------------------- getUserNameByName

#[tokio::test]
async fn get_user_name_by_name_returns_matching_user() {
    let Some(svc) = service().await else { return };
    // Ported scenario: "hemraj". The name is made unique to avoid clashing
    // with other rows of the same name.
    let name = unique("hemraj");
    let user = User::builder()
        .name(name.clone())
        .email(format!("{}hemrajmalhi1234@gmail.com", unique("x")))
        .about("Sr")
        .role("java developer")
        .build();
    svc.save_user(user).await.unwrap();
    let found = svc.get_user_name_by_name(&name).await.unwrap().unwrap();
    assert_eq!(found.name.as_deref(), Some(name.as_str()));
    assert_eq!(found.role.as_deref(), Some("java developer"));
    assert_eq!(found.about.as_deref(), Some("Sr"));
}

#[tokio::test]
async fn get_user_name_by_name_absent_returns_none() {
    let Some(svc) = service().await else { return };
    let res = svc.get_user_name_by_name(&unique("nobody")).await.unwrap();
    assert!(res.is_none());
}

#[tokio::test]
async fn get_user_name_by_name_duplicate_is_internal_error() {
    let Some(svc) = service().await else { return };
    let name = unique("dup");
    svc.save_user(new_user(&name)).await.unwrap();
    svc.save_user(new_user(&name)).await.unwrap();
    let err = svc.get_user_name_by_name(&name).await.unwrap_err();
    assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
}