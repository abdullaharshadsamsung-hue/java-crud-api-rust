//! `User` entity — port of the JPA `com.smartContact.model.User`.
//!
//! The physical table is `"user"` (quoted: `user` is a reserved word in
//! PostgreSQL) with snake_case `user_*` columns. Queries alias the columns
//! back to the struct field names (`user_id AS id`, ...) so `sqlx::FromRow`
//! maps them directly.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Message carried by the source's `@NotBlank` constraint on `name`.
pub const NAME_NOT_BLANK_MESSAGE: &str = "please Add the department Name";

/// DDL replacing Hibernate `ddl-auto=update`.
/// `about` keeps the JPA `length = 500`; `email` keeps its unique constraint.
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

/// Column list aliased to the struct's field names, for use in SELECTs.
pub const USER_SELECT_COLUMNS: &str = "user_id AS id, user_name AS name, user_email AS email, \
     user_password AS password, user_role AS role, user_about AS about";

/// Creates the `user` table if it doesn't exist. Call this once at startup.
pub async fn ensure_user_schema(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
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

    /// `@NotBlank` on `name`: fails when `name` is absent, empty or
    /// whitespace-only. Call before persisting (the `@Valid` / pre-persist
    /// equivalent).
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
    fn json_shape() {
        let u: User = serde_json::from_str(r#"{"name":"x","extra":1}"#).unwrap();
        assert_eq!(u.id, 0);
        let v = serde_json::to_value(&u).unwrap();
        assert!(v["email"].is_null());
    }
}
