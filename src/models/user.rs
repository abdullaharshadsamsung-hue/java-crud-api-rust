//! `User` entity — port of the JPA `com.smartContact.model.User`.
//!
//! The physical table is `"user"`, quoted because `user` is a reserved word
//! in PostgreSQL. Its columns are snake_case `user_*`. Spring Boot's default
//! `SpringPhysicalNamingStrategy` lower-cases and snake-cases `@Table(name =
//! "USER")` and `@Column(name = "User_Email")`, so these are the same names
//! Hibernate created. `application.properties` sets no custom naming
//! strategy.
//!
//! Queries alias the columns back to the struct field names (`user_id AS id`,
//! ...) so `sqlx::FromRow` maps them directly.
//!
//! # Schema creation (replaces `spring.jpa.hibernate.ddl-auto=update`)
//!
//! Every query against `"user"` goes through [`UserRepository`]. Each
//! repository method first awaits [`UserRepository::ensure_schema`], which
//! runs `CREATE TABLE IF NOT EXISTS` exactly once per repository (and its
//! clones). The table therefore always exists before the first read or write,
//! even on a fresh database. Use [`UserRepository::init`] at boot to create it
//! eagerly, so a schema failure stops startup instead of failing the first
//! request.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::OnceCell;

use crate::error::AppError;

/// Message carried by the source's `@NotBlank` constraint on `name`.
pub const NAME_NOT_BLANK_MESSAGE: &str = "please Add the department Name";

/// DDL replacing Hibernate `ddl-auto=update`.
/// `about` keeps the JPA `length = 500`; `email` keeps its unique constraint.
// MIGRATION_NOTE: the source targeted MySQL (`MySQL8Dialect`). This port uses
// PostgreSQL (`SERIAL` stands in for `GenerationType.AUTO`), like the rest of
// the migrated service. Change the DDL and pool type if MySQL must be kept.
pub const CREATE_USER_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS "user" (
    user_id       SERIAL PRIMARY KEY,
    user_name     VARCHAR(255),
    user_email    VARCHAR(255) UNIQUE,
    user_password VARCHAR(255),
    user_role     VARCHAR(255),
    user_about    VARCHAR(500)
)
"#;

/// Column list aliased to the struct's field names, for use in SELECT / RETURNING.
pub const USER_SELECT_COLUMNS: &str = "user_id AS id, user_name AS name, user_email AS email, \
     user_password AS password, user_role AS role, user_about AS about";

/// Runs the `user` DDL unconditionally. The statement is idempotent
/// (`IF NOT EXISTS`). Prefer [`UserRepository`], which calls this for you.
pub async fn ensure_user_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(CREATE_USER_TABLE_SQL).execute(pool).await?;
    Ok(())
}

/// A user record. `Default` is the no-args constructor (id = 0, all
/// strings `None`/null), matching Lombok's `@NoArgsConstructor`.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::FromRow,
)]
pub struct User {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub about: Option<String>,
}

/// Raised when `User::validate` fails (Bean Validation equivalent).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{field}: {message}")]
pub struct UserValidationError {
    pub field: &'static str,
    pub message: &'static str,
}

impl From<UserValidationError> for AppError {
    fn from(e: UserValidationError) -> Self {
        AppError::BadRequest(e.to_string())
    }
}

/// Errors from [`UserRepository`] write operations.
#[derive(Debug, thiserror::Error)]
pub enum UserStoreError {
    /// `@NotBlank` failed (Hibernate's pre-persist validation equivalent).
    #[error(transparent)]
    Validation(#[from] UserValidationError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl User {
    /// All-args constructor (Lombok `@AllArgsConstructor`), field order
    /// id, name, email, password, role, about.
    pub fn new(
        id: i32,
        name: Option<String>,
        email: Option<String>,
        password: Option<String>,
        role: Option<String>,
        about: Option<String>,
    ) -> Self {
        Self { id, name, email, password, role, about }
    }

    /// Fluent builder (Lombok `@Builder`).
    pub fn builder() -> UserBuilder {
        UserBuilder::default()
    }

    /// `@NotBlank` on `name` fails when `name` is absent, empty or
    /// whitespace-only.
    pub fn validate(&self) -> Result<(), UserValidationError> {
        match self.name.as_deref() {
            Some(n) if !n.trim().is_empty() => Ok(()),
            _ => Err(UserValidationError {
                field: "name",
                message: NAME_NOT_BLANK_MESSAGE,
            }),
        }
    }

    // ---- Lombok @Data accessors -------------------------------------------

    pub fn id(&self) -> i32 {
        self.id
    }
    pub fn set_id(&mut self, id: i32) {
        self.id = id;
    }
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    pub fn set_name(&mut self, name: Option<String>) {
        self.name = name;
    }
    pub fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }
    pub fn set_email(&mut self, email: Option<String>) {
        self.email = email;
    }
    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }
    pub fn set_password(&mut self, password: Option<String>) {
        self.password = password;
    }
    pub fn role(&self) -> Option<&str> {
        self.role.as_deref()
    }
    pub fn set_role(&mut self, role: Option<String>) {
        self.role = role;
    }
    pub fn about(&self) -> Option<&str> {
        self.about.as_deref()
    }
    pub fn set_about(&mut self, about: Option<String>) {
        self.about = about;
    }
}

/// Lombok-style `toString`: `User(id=1, name=..., ...)`, nulls as `null`.
impl fmt::Display for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn s(v: &Option<String>) -> &str {
            v.as_deref().unwrap_or("null")
        }
        write!(
            f,
            "User(id={}, name={}, email={}, password={}, role={}, about={})",
            self.id,
            s(&self.name),
            s(&self.email),
            s(&self.password),
            s(&self.role),
            s(&self.about)
        )
    }
}

/// Builder for [`User`]; unset fields take their default values.
#[derive(Debug, Clone, Default)]
pub struct UserBuilder {
    inner: User,
}

impl UserBuilder {
    pub fn id(mut self, id: i32) -> Self {
        self.inner.id = id;
        self
    }
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.inner.name = Some(name.into());
        self
    }
    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.inner.email = Some(email.into());
        self
    }
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.inner.password = Some(password.into());
        self
    }
    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.inner.role = Some(role.into());
        self
    }
    pub fn about(mut self, about: impl Into<String>) -> Self {
        self.inner.about = Some(about.into());
        self
    }
    pub fn build(self) -> User {
        self.inner
    }
}

/// Data access for `User`, replacing the JPA `EntityManager` /
/// Spring Data repository.
///
/// Every method awaits [`Self::ensure_schema`] before it touches the table,
/// so the DDL is always applied before the first query. The once-guard sits
/// behind an `Arc`, so cloning the repository into per-request state does not
/// re-run the DDL. There is no process-global state; each pool has its own
/// guard.
#[derive(Debug, Clone)]
pub struct UserRepository {
    pool: PgPool,
    schema_ready: Arc<OnceCell<()>>,
}

impl UserRepository {
    /// Lazy constructor. The table is created on the first repository call.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            schema_ready: Arc::new(OnceCell::new()),
        }
    }

    /// Eager constructor for application boot. It creates the `user` table
    /// now and fails startup if that is impossible.
    pub async fn init(pool: PgPool) -> Result<Self, sqlx::Error> {
        let repo = Self::new(pool);
        repo.ensure_schema().await?;
        Ok(repo)
    }

    /// Applies [`CREATE_USER_TABLE_SQL`] once per repository instance.
    /// If it fails, the next call retries.
    pub async fn ensure_schema(&self) -> Result<(), sqlx::Error> {
        self.schema_ready
            .get_or_try_init(|| ensure_user_schema(&self.pool))
            .await?;
        Ok(())
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn find_all(&self) -> Result<Vec<User>, sqlx::Error> {
        self.ensure_schema().await?;
        let sql = format!("SELECT {USER_SELECT_COLUMNS} FROM \"user\" ORDER BY user_id");
        sqlx::query_as::<_, User>(&sql).fetch_all(&self.pool).await
    }

    pub async fn find_by_id(&self, id: i32) -> Result<Option<User>, sqlx::Error> {
        self.ensure_schema().await?;
        let sql = format!("SELECT {USER_SELECT_COLUMNS} FROM \"user\" WHERE user_id = $1");
        sqlx::query_as::<_, User>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn find_by_email(&self, email: &str) -> Result<Option<User>, sqlx::Error> {
        self.ensure_schema().await?;
        let sql = format!("SELECT {USER_SELECT_COLUMNS} FROM \"user\" WHERE user_email = $1");
        sqlx::query_as::<_, User>(&sql)
            .bind(email)
            .fetch_optional(&self.pool)
            .await
    }

    /// Inserts a new user and returns it with its generated id.
    /// `user.id` is ignored. The DB assigns it, as `@GeneratedValue` did.
    pub async fn insert(&self, user: &User) -> Result<User, UserStoreError> {
        user.validate()?;
        self.ensure_schema().await?;
        let sql = format!(
            "INSERT INTO \"user\" (user_name, user_email, user_password, user_role, user_about) \
             VALUES ($1, $2, $3, $4, $5) RETURNING {USER_SELECT_COLUMNS}"
        );
        let saved = sqlx::query_as::<_, User>(&sql)
            .bind(user.name.clone())
            .bind(user.email.clone())
            .bind(user.password.clone())
            .bind(user.role.clone())
            .bind(user.about.clone())
            .fetch_one(&self.pool)
            .await?;
        Ok(saved)
    }

    /// Updates every column of the row with `user.id`. Returns `None` if no
    /// such row exists.
    pub async fn update(&self, user: &User) -> Result<Option<User>, UserStoreError> {
        user.validate()?;
        self.ensure_schema().await?;
        let sql = format!(
            "UPDATE \"user\" SET user_name = $1, user_email = $2, user_password = $3, \
             user_role = $4, user_about = $5 WHERE user_id = $6 RETURNING {USER_SELECT_COLUMNS}"
        );
        let updated = sqlx::query_as::<_, User>(&sql)
            .bind(user.name.clone())
            .bind(user.email.clone())
            .bind(user.password.clone())
            .bind(user.role.clone())
            .bind(user.about.clone())
            .bind(user.id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(updated)
    }

    /// Deletes by id. Returns `true` if a row was removed.
    pub async fn delete(&self, id: i32) -> Result<bool, sqlx::Error> {
        self.ensure_schema().await?;
        let result = sqlx::query("DELETE FROM \"user\" WHERE user_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        let u = User::default();
        assert_eq!(u.id, 0);
        assert!(u.name.is_none());
    }

    #[test]
    fn builder_and_validation() {
        let u = User::builder().id(1).name("a").email("e").build();
        assert_eq!(u.name(), Some("a"));
        assert!(u.validate().is_ok());
        assert!(User::builder().name("  ").build().validate().is_err());
        assert!(User::default().validate().is_err());
    }

    #[test]
    fn validation_message_preserved() {
        let err = User::default().validate().unwrap_err();
        assert_eq!(err.message, NAME_NOT_BLANK_MESSAGE);
        assert_eq!(err.field, "name");
    }

    #[test]
    fn json_shape() {
        let u: User = serde_json::from_str(r#"{"name":"x","extra":1}"#).unwrap();
        assert_eq!(u.id, 0);
        let v = serde_json::to_value(&u).unwrap();
        assert!(v["email"].is_null());
    }

    #[test]
    fn ddl_matches_columns() {
        for col in ["user_id", "user_name", "user_email", "user_password", "user_role", "user_about"] {
            assert!(CREATE_USER_TABLE_SQL.contains(col));
            assert!(USER_SELECT_COLUMNS.contains(col));
        }
        assert!(CREATE_USER_TABLE_SQL.contains("VARCHAR(500)"));
        assert!(CREATE_USER_TABLE_SQL.contains("UNIQUE"));
    }
}
