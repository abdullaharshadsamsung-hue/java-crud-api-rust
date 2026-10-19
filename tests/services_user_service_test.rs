//! Tests for `src/services/user_service.rs`.
//!
//! The service functions use a real PostgreSQL database. Set `DATABASE_URL`
//! to run them. If it is unset, each test returns early without checking
//! anything.
//!
//! Each test runs in its own freshly created schema, so the tests are
//! isolated from each other and can run in parallel.
//!
//! Error-to-HTTP mapping is checked by turning the returned `AppError` into an
//! axum `Response`. The tests then assert on the status code and the
//! deserialized JSON body.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool};

use migrated_service::error::AppError;
use migrated_service::models::user::{User, UserRepository, NAME_NOT_BLANK_MESSAGE};
use migrated_service::services::user_service::{
    delete_user, fetch_user_by_id, fetch_user_list, get_user_name_by_name, save_user,
    update_user, USER_NOT_AVAILABLE_MESSAGE,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_schema() -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("us_test_{}_{}_{}", std::process::id(), nanos, n)
}

/// Builds a repository whose connections all use a brand-new, empty schema.
async fn repo() -> Option<UserRepository> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let schema = unique_schema();

    let admin = PgPool::connect(&url).await.expect("connect admin pool");
    admin
        .execute(format!("CREATE SCHEMA \"{schema}\"").as_str())
        .await
        .expect("create schema");
    admin.close().await;

    let s = schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .after_connect(move |conn, _meta| {
            let s = s.clone();
            Box::pin(async move {
                conn.execute(format!("SET search_path TO \"{s}\"").as_str())
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .expect("connect test pool");

    Some(UserRepository::new(pool))
}

fn sample(name: &str, email: &str) -> User {
    User::builder()
        .name(name)
        .email(email)
        .password("secret")
        .role("ROLE_USER")
        .about("about me")
        .build()
}

async fn body_json(resp: Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("json body")
}

#[allow(dead_code)]
fn _body_type_check(_: Body) {}

// ---------------------------------------------------------------- saveUser

#[tokio::test]
async fn save_user_new_assigns_id_and_keeps_fields() {
    let Some(repo) = repo().await else { return };
    let input = sample("alice", "alice@example.com");

    let saved = save_user(&repo, input.clone()).await.expect("save");

    assert_ne!(saved.id, 0);
    assert_eq!(saved.name, input.name);
    assert_eq!(saved.email, input.email);
    assert_eq!(saved.password, input.password);
    assert_eq!(saved.role, input.role);
    assert_eq!(saved.about, input.about);

    // Invariant: fetchUserById(returned.id) yields an equivalent user.
    let fetched = fetch_user_by_id(&repo, saved.id).await.expect("fetch");
    assert_eq!(fetched, saved);
    assert_eq!(fetch_user_list(&repo).await.unwrap().len(), 1);
}

#[tokio::test]
async fn save_user_existing_id_overwrites_record() {
    let Some(repo) = repo().await else { return };
    let saved = save_user(&repo, sample("bob", "bob@example.com"))
        .await
        .unwrap();

    let mut changed = sample("bobby", "bobby@example.com");
    changed.id = saved.id;
    changed.about = None;
    let resaved = save_user(&repo, changed.clone()).await.expect("resave");

    assert_eq!(resaved, changed);
    let all = fetch_user_list(&repo).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0], changed);
    assert_eq!(fetch_user_by_id(&repo, saved.id).await.unwrap(), changed);
}

#[tokio::test]
async fn save_user_blank_name_is_400_validation_error() {
    let Some(repo) = repo().await else { return };
    let err = save_user(&repo, sample("   ", "x@example.com"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::BadRequest(ref m) if m.contains(NAME_NOT_BLANK_MESSAGE)));

    let resp = err.into_response();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert!(body["error"].as_str().unwrap().contains(NAME_NOT_BLANK_MESSAGE));

    let err = save_user(&repo, User::default()).await.unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)));
    assert!(fetch_user_list(&repo).await.unwrap().is_empty());
}

#[tokio::test]
async fn save_user_constraint_violation_is_500() {
    let Some(repo) = repo().await else { return };
    save_user(&repo, sample("c1", "dup@example.com")).await.unwrap();
    let err = save_user(&repo, sample("c2", "dup@example.com"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Internal(_)));

    let resp = err.into_response();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_json(resp).await;
    assert_eq!(body["error"], "internal server error");
    assert_eq!(fetch_user_list(&repo).await.unwrap().len(), 1);
}

// ----------------------------------------------------------- fetchUserList

#[tokio::test]
async fn fetch_user_list_empty_returns_empty_vec() {
    let Some(repo) = repo().await else { return };
    let list = fetch_user_list(&repo).await.expect("list");
    assert!(list.is_empty());
}

#[tokio::test]
async fn fetch_user_list_returns_every_user_and_is_read_only() {
    let Some(repo) = repo().await else { return };
    let a = save_user(&repo, sample("a", "a@example.com")).await.unwrap();
    let b = save_user(&repo, sample("b", "b@example.com")).await.unwrap();
    let c = save_user(&repo, sample("c", "c@example.com")).await.unwrap();

    let list = fetch_user_list(&repo).await.unwrap();
    assert_eq!(list, vec![a.clone(), b.clone(), c.clone()]);

    let again = fetch_user_list(&repo).await.unwrap();
    assert_eq!(again, list);
}

// ----------------------------------------------------------- fetchUserById

#[tokio::test]
async fn fetch_user_by_id_existing_returns_user() {
    let Some(repo) = repo().await else { return };
    let saved = save_user(&repo, sample("dave", "dave@example.com"))
        .await
        .unwrap();
    let fetched = fetch_user_by_id(&repo, saved.id).await.expect("fetch");
    assert_eq!(fetched, saved);
    // Read-only.
    assert_eq!(fetch_user_list(&repo).await.unwrap(), vec![saved]);
}

#[tokio::test]
async fn fetch_user_by_id_missing_is_user_not_found_404() {
    let Some(repo) = repo().await else { return };
    let err = fetch_user_by_id(&repo, 424242).await.unwrap_err();
    assert!(matches!(err, AppError::UserNotFound(ref m) if m == USER_NOT_AVAILABLE_MESSAGE));
    assert_eq!(USER_NOT_AVAILABLE_MESSAGE, "User are not available");

    let resp = err.into_response();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(
        body,
        serde_json::json!({"status": "NOT_FOUND", "message": "User are not available"})
    );
}

// --------------------------------------------------------------- deleteUser

#[tokio::test]
async fn delete_user_existing_removes_only_that_user() {
    let Some(repo) = repo().await else { return };
    let keep = save_user(&repo, sample("keep", "keep@example.com"))
        .await
        .unwrap();
    let gone = save_user(&repo, sample("gone", "gone@example.com"))
        .await
        .unwrap();

    delete_user(&repo, gone.id).await.expect("delete");

    let err = fetch_user_by_id(&repo, gone.id).await.unwrap_err();
    assert!(matches!(err, AppError::UserNotFound(_)));
    assert_eq!(fetch_user_by_id(&repo, keep.id).await.unwrap(), keep);
    assert_eq!(fetch_user_list(&repo).await.unwrap(), vec![keep]);
}

#[tokio::test]
async fn delete_user_missing_id_is_silently_ignored() {
    let Some(repo) = repo().await else { return };
    let keep = save_user(&repo, sample("k", "k@example.com")).await.unwrap();
    delete_user(&repo, 999_999).await.expect("missing id ignored");
    assert_eq!(fetch_user_list(&repo).await.unwrap(), vec![keep]);
}

// --------------------------------------------------------------- updateUser

#[tokio::test]
async fn update_user_existing_id_applies_new_values() {
    let Some(repo) = repo().await else { return };
    let target = save_user(&repo, sample("old", "old@example.com"))
        .await
        .unwrap();
    let other = save_user(&repo, sample("other", "other@example.com"))
        .await
        .unwrap();

    // The id in the body is ignored. The path id wins.
    let mut patch = sample("new", "new@example.com");
    patch.id = 12345;
    update_user(&repo, target.id, patch.clone()).await.expect("update");

    let stored = fetch_user_by_id(&repo, target.id).await.unwrap();
    assert_eq!(stored.id, target.id);
    assert_eq!(stored.name.as_deref(), Some("new"));
    assert_eq!(stored.email.as_deref(), Some("new@example.com"));
    assert_eq!(fetch_user_by_id(&repo, other.id).await.unwrap(), other);
    assert_eq!(fetch_user_list(&repo).await.unwrap().len(), 2);
}

#[tokio::test]
async fn update_user_none_fields_overwrite_with_null() {
    let Some(repo) = repo().await else { return };
    let target = save_user(&repo, sample("n", "n@example.com")).await.unwrap();
    let patch = User::builder().name("only-name").build();
    update_user(&repo, target.id, patch).await.unwrap();

    let stored = fetch_user_by_id(&repo, target.id).await.unwrap();
    assert_eq!(stored.name.as_deref(), Some("only-name"));
    assert!(stored.email.is_none());
    assert!(stored.about.is_none());
}

#[tokio::test]
async fn update_user_missing_id_inserts_new_record() {
    let Some(repo) = repo().await else { return };
    update_user(&repo, 777_777, sample("fresh", "fresh@example.com"))
        .await
        .expect("merge inserts");
    let list = fetch_user_list(&repo).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name.as_deref(), Some("fresh"));
    assert_ne!(list[0].id, 0);
}

#[tokio::test]
async fn update_user_blank_name_is_400_and_leaves_row_unchanged() {
    let Some(repo) = repo().await else { return };
    let target = save_user(&repo, sample("v", "v@example.com")).await.unwrap();
    let err = update_user(&repo, target.id, sample("", "z@example.com"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)));
    assert_eq!(err.into_response().status(), StatusCode::BAD_REQUEST);
    assert_eq!(fetch_user_by_id(&repo, target.id).await.unwrap(), target);
}

// -------------------------------------------------------- getUserNameByName

#[tokio::test]
async fn get_user_name_by_name_match_returns_user() {
    let Some(repo) = repo().await else { return };
    let saved = save_user(&repo, sample("erin", "erin@example.com"))
        .await
        .unwrap();
    save_user(&repo, sample("frank", "frank@example.com")).await.unwrap();

    let found = get_user_name_by_name(&repo, "erin").await.unwrap();
    let found = found.expect("user present");
    assert_eq!(found, saved);
    assert_eq!(found.name.as_deref(), Some("erin"));
    assert_eq!(fetch_user_list(&repo).await.unwrap().len(), 2);
}

#[tokio::test]
async fn get_user_name_by_name_no_match_returns_none() {
    let Some(repo) = repo().await else { return };
    save_user(&repo, sample("gina", "gina@example.com")).await.unwrap();
    assert!(get_user_name_by_name(&repo, "nobody").await.unwrap().is_none());
    // Exact match only, so a different case does not match.
    assert!(get_user_name_by_name(&repo, "GINA").await.unwrap().is_none());
}

#[tokio::test]
async fn get_user_name_by_name_duplicates_is_500() {
    let Some(repo) = repo().await else { return };
    save_user(&repo, sample("twin", "t1@example.com")).await.unwrap();
    save_user(&repo, sample("twin", "t2@example.com")).await.unwrap();
    let err = get_user_name_by_name(&repo, "twin").await.unwrap_err();
    assert!(matches!(err, AppError::Internal(_)));
    let resp = err.into_response();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body_json(resp).await["error"], "internal server error");
}