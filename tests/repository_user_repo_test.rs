//! Tests for src/repository/user_repo.rs (UserDao port).
//!
//! Database-backed tests need PostgreSQL via `DATABASE_URL`; when it is
//! unset they skip themselves. Pure logic (paging validation, error mapping,
//! serde shapes) runs without a database.

use migrated_service::error::AppError;
use migrated_service::models::user::{User, UserRepository};
use migrated_service::repository::user_repo::{
    Direction, Page, PageRequest, SortOrder, UserRepoError,
};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

async fn pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .ok()
}

/// Fresh repo with an empty `user` table. Tests share a DB, so serialize.
static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn fresh_repo() -> Option<UserRepository> {
    let pool = pool().await?;
    let repo = UserRepository::init(pool.clone()).await.ok()?;
    sqlx::query("TRUNCATE \"user\" RESTART IDENTITY")
        .execute(&pool)
        .await
        .ok()?;
    Some(repo)
}

fn hemraj() -> User {
    User::new(
        3,
        Some("hemraj".to_string()),
        Some("hemrajmalhi1234@gmail.com".to_string()),
        Some("root".to_string()),
        Some("java developer".to_string()),
        Some("Sr".to_string()),
    )
}

fn named(name: &str, email: &str) -> User {
    User::builder().name(name).email(email).build()
}

macro_rules! repo_or_skip {
    () => {
        match fresh_repo().await {
            Some(r) => r,
            None => {
                eprintln!("DATABASE_URL not set/unreachable; skipping");
                return;
            }
        }
    };
}

// ---------------- findByName (ported) ----------------

#[tokio::test]
async fn find_by_name_returns_matching_user() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    repo.save(&hemraj()).await.unwrap();
    let found = repo.find_by_name("hemraj").await.unwrap().expect("user");
    assert_eq!(found.name(), Some("hemraj"));
    assert_eq!(found.email(), Some("hemrajmalhi1234@gmail.com"));
    assert_eq!(found.about(), Some("Sr"));
    assert_eq!(found.password(), Some("root"));
    assert_eq!(found.role(), Some("java developer"));
}

#[tokio::test]
async fn find_by_name_missing_returns_none() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    repo.save(&hemraj()).await.unwrap();
    assert!(repo.find_by_name("nobody").await.unwrap().is_none());
    // exact match only
    assert!(repo.find_by_name("HEMRAJ").await.unwrap().is_none());
    assert!(repo.find_by_name("hem").await.unwrap().is_none());
}

#[tokio::test]
async fn find_by_name_non_unique_errors() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    repo.save(&named("dup", "a@x.com")).await.unwrap();
    repo.save(&named("dup", "b@x.com")).await.unwrap();
    let err = repo.find_by_name("dup").await.unwrap_err();
    assert!(matches!(err, UserRepoError::NonUniqueResult { ref name } if name == "dup"));
    let app: AppError = err.into();
    assert!(matches!(app, AppError::Internal(_)));
}

#[tokio::test]
async fn find_by_name_is_read_only() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    repo.save(&hemraj()).await.unwrap();
    let before = repo.find_all().await.unwrap();
    let _ = repo.find_by_name("hemraj").await.unwrap();
    let _ = repo.find_by_name("missing").await.unwrap();
    assert_eq!(repo.find_all().await.unwrap(), before);
}

// ---------------- save ----------------

#[tokio::test]
async fn save_new_user_inserts_and_assigns_id() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    let saved = repo.save(&named("alice", "alice@x.com")).await.unwrap();
    assert!(saved.id > 0);
    assert_eq!(saved.name(), Some("alice"));
    assert_eq!(repo.count().await.unwrap(), 1);
    let fetched = repo.find_by_id(saved.id).await.unwrap().unwrap();
    assert_eq!(fetched, saved);
}

#[tokio::test]
async fn save_existing_user_updates_row() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    let saved = repo.save(&named("bob", "bob@x.com")).await.unwrap();
    let mut changed = saved.clone();
    changed.set_name(Some("bobby".to_string()));
    changed.set_about(Some("updated".to_string()));
    let updated = repo.save(&changed).await.unwrap();
    assert_eq!(updated.id, saved.id);
    assert_eq!(updated.name(), Some("bobby"));
    assert_eq!(repo.count().await.unwrap(), 1);
    assert_eq!(repo.find_by_id(saved.id).await.unwrap().unwrap(), updated);
}

#[tokio::test]
async fn save_merge_overwrites_none_with_null() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    let saved = repo.save(&hemraj()).await.unwrap();
    let partial = User::builder().id(saved.id).name("hemraj").build();
    let updated = repo.save(&partial).await.unwrap();
    assert!(updated.email.is_none());
    assert!(updated.about.is_none());
}

#[tokio::test]
async fn save_with_unknown_id_inserts_new_row() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    let saved = repo
        .save(&User::builder().id(9999).name("ghost").build())
        .await
        .unwrap();
    assert!(saved.id > 0);
    assert_eq!(repo.count().await.unwrap(), 1);
    assert!(repo.exists_by_id(saved.id).await.unwrap());
}

#[tokio::test]
async fn save_invalid_user_is_validation_error() {
    // Closest Rust analogue of "entity is null": an empty (default) entity.
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    let err = repo.save(&User::default()).await.unwrap_err();
    assert!(matches!(err, UserRepoError::Validation(_)));
    let app: AppError = err.into();
    assert!(matches!(app, AppError::BadRequest(_)));
    let err = repo.save(&User::builder().name("   ").build()).await.unwrap_err();
    assert!(matches!(err, UserRepoError::Validation(_)));
    assert_eq!(repo.count().await.unwrap(), 0);
}

#[tokio::test]
async fn save_duplicate_email_is_database_error() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    repo.save(&named("a", "same@x.com")).await.unwrap();
    let err = repo.save(&named("b", "same@x.com")).await.unwrap_err();
    assert!(matches!(err, UserRepoError::Database(_)));
    let app: AppError = err.into();
    assert!(matches!(app, AppError::Internal(_)));
}

// ---------------- findById ----------------

#[tokio::test]
async fn find_by_id_existing_and_missing() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    let saved = repo.save(&hemraj()).await.unwrap();
    assert_eq!(repo.find_by_id(saved.id).await.unwrap(), Some(saved.clone()));
    assert_eq!(repo.find_by_id(saved.id + 1000).await.unwrap(), None);
    assert!(repo.exists_by_id(saved.id).await.unwrap());
    assert!(!repo.exists_by_id(saved.id + 1000).await.unwrap());
}

// ---------------- findAll ----------------

#[tokio::test]
async fn find_all_empty_and_populated() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    assert!(repo.find_all().await.unwrap().is_empty());
    assert!(repo.find_all_sorted(&[]).await.unwrap().is_empty());
    let a = repo.save(&named("b", "1@x")).await.unwrap();
    let b = repo.save(&named("a", "2@x")).await.unwrap();
    assert_eq!(repo.find_all().await.unwrap(), vec![a.clone(), b.clone()]);
    assert_eq!(repo.find_all_sorted(&[]).await.unwrap(), vec![a.clone(), b.clone()]);
    assert_eq!(
        repo.find_all_sorted(&[SortOrder::asc("name")]).await.unwrap(),
        vec![b.clone(), a.clone()]
    );
    assert_eq!(
        repo.find_all_sorted(&[SortOrder::desc("id")]).await.unwrap(),
        vec![b, a]
    );
}

#[tokio::test]
async fn find_all_sorted_rejects_unknown_property() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    let err = repo
        .find_all_sorted(&[SortOrder::asc("user_name")])
        .await
        .unwrap_err();
    assert!(matches!(err, UserRepoError::InvalidSortProperty(ref p) if p == "user_name"));
    let app: AppError = err.into();
    assert!(matches!(app, AppError::BadRequest(_)));
}

#[tokio::test]
async fn find_all_paged() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    let empty = repo.find_all_paged(&PageRequest::of(0, 2)).await.unwrap();
    assert!(empty.content.is_empty());
    assert_eq!(empty.total_elements, 0);
    assert_eq!(empty.total_pages, 0);

    let mut saved = Vec::new();
    for i in 0..5 {
        saved.push(repo.save(&named(&format!("u{i}"), &format!("u{i}@x"))).await.unwrap());
    }
    let p0 = repo.find_all_paged(&PageRequest::of(0, 2)).await.unwrap();
    assert_eq!(p0.content, saved[0..2].to_vec());
    assert_eq!(p0.total_elements, 5);
    assert_eq!(p0.total_pages, 3);
    assert_eq!((p0.number, p0.size), (0, 2));
    let p2 = repo.find_all_paged(&PageRequest::of(2, 2)).await.unwrap();
    assert_eq!(p2.content, saved[4..5].to_vec());
    let p9 = repo.find_all_paged(&PageRequest::of(9, 2)).await.unwrap();
    assert!(p9.content.is_empty());
    let desc = repo
        .find_all_paged(&PageRequest::of(0, 1).with_sort(vec![SortOrder::desc("name")]))
        .await
        .unwrap();
    assert_eq!(desc.content, vec![saved[4].clone()]);
}

#[tokio::test]
async fn find_all_paged_invalid_requests() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    let err = repo.find_all_paged(&PageRequest::of(0, 0)).await.unwrap_err();
    assert!(matches!(err, UserRepoError::InvalidPageRequest(_)));
    let app: AppError = err.into();
    assert!(matches!(app, AppError::BadRequest(_)));
    let err = repo
        .find_all_paged(&PageRequest::of(0, 1).with_sort(vec![SortOrder::asc("bogus")]))
        .await
        .unwrap_err();
    assert!(matches!(err, UserRepoError::InvalidSortProperty(_)));
}

// ---------------- deleteById ----------------

#[tokio::test]
async fn delete_by_id_removes_row() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    let saved = repo.save(&hemraj()).await.unwrap();
    assert!(repo.delete_by_id(saved.id).await.unwrap());
    assert!(repo.find_by_id(saved.id).await.unwrap().is_none());
    assert_eq!(repo.count().await.unwrap(), 0);
    // missing id: no error, returns false
    assert!(!repo.delete_by_id(saved.id).await.unwrap());
}

// ---------------- count ----------------

#[tokio::test]
async fn count_tracks_rows() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    assert_eq!(repo.count().await.unwrap(), 0);
    let a = repo.save(&named("a", "a@x")).await.unwrap();
    repo.save(&named("b", "b@x")).await.unwrap();
    assert_eq!(repo.count().await.unwrap(), 2);
    repo.delete_by_id(a.id).await.unwrap();
    assert_eq!(repo.count().await.unwrap(), 1);
    assert!(repo.count().await.unwrap() >= 0);
}

#[tokio::test]
async fn cloned_repo_shares_data() {
    let _g = LOCK.lock().await;
    let repo = repo_or_skip!();
    let clone = repo.clone();
    let saved = repo.save(&hemraj()).await.unwrap();
    assert_eq!(clone.find_by_id(saved.id).await.unwrap(), Some(saved));
}

// ---------------- pure logic ----------------

#[test]
fn page_request_builders_and_serde() {
    let r = PageRequest::of(1, 10).with_sort(vec![SortOrder::desc("email")]);
    assert_eq!(r.page, 1);
    assert_eq!(r.size, 10);
    assert_eq!(r.sort[0].direction, Direction::Desc);
    let parsed: PageRequest =
        serde_json::from_str(r#"{"page":0,"size":5,"sort":[{"property":"name"}]}"#).unwrap();
    assert_eq!(parsed.sort, vec![SortOrder::asc("name")]);
    let d: Direction = serde_json::from_str("\"DESC\"").unwrap();
    assert_eq!(d, Direction::Desc);
    assert_eq!(Direction::default(), Direction::Asc);
}

#[test]
fn page_serializes() {
    let p: Page<User> = Page {
        content: vec![],
        number: 0,
        size: 1,
        total_elements: 0,
        total_pages: 0,
    };
    let v = serde_json::to_value(&p).unwrap();
    assert!(v["content"].as_array().unwrap().is_empty());
    assert_eq!(v["total_elements"], 0);
}

#[test]
fn error_messages() {
    assert!(UserRepoError::NonUniqueResult { name: "x".into() }
        .to_string()
        .contains("'x'"));
    assert_eq!(
        UserRepoError::InvalidSortProperty("zz".into()).to_string(),
        "no property 'zz' found for type 'User'"
    );
}