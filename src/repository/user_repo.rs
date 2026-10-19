//! Port of the Spring Data JPA `UserDao extends JpaRepository<User, Integer>`.
//!
//! The concrete data-access type, [`UserRepository`], already lives in
//! `crate::models::user`. It covers `findAll`, `findById`, insert, update
//! and delete. This module adds the rest of the `UserDao` contract as a
//! second inherent `impl` block on that same type, so nothing is declared
//! twice:
//!
//! * `findByName` (derived query `WHERE name = ?`) -> [`UserRepository::find_by_name`]
//! * `save` (persist if new, merge otherwise) -> [`UserRepository::save`]
//! * `existsById` -> [`UserRepository::exists_by_id`]
//! * `count` -> [`UserRepository::count`]
//! * `deleteById` -> [`UserRepository::delete_by_id`]
//! * `findAll(Sort)` / `findAll(Pageable)` -> [`UserRepository::find_all_sorted`] /
//!   [`UserRepository::find_all_paged`]
//!
//! All SQL is runtime-checked (`sqlx::query*`) and targets PostgreSQL with
//! `$n` placeholders. The `user` table is always double-quoted because `user`
//! is a reserved word in PostgreSQL.

use serde::{Deserialize, Serialize};

use crate::error::{anyhow_like, AppError};
use crate::models::user::{User, UserRepository, UserStoreError, UserValidationError, USER_SELECT_COLUMNS};

/// Errors raised by the `UserDao`-level operations.
///
/// This stands in for Spring's `DataAccessException` hierarchy, which
/// `@Repository` exception translation produced.
#[derive(Debug, thiserror::Error)]
pub enum UserRepoError {
    /// `findByName` matched more than one row. Spring Data raises
    /// `IncorrectResultSizeDataAccessException` for a single-result query
    /// in that case.
    #[error("query did not return a unique result: more than one user named '{name}'")]
    NonUniqueResult { name: String },

    /// The sort property is not a mapped `User` attribute. This mirrors
    /// Spring's `PropertyReferenceException`.
    #[error("no property '{0}' found for type 'User'")]
    InvalidSortProperty(String),

    /// The page request was invalid (size < 1). This mirrors
    /// `PageRequest.of` throwing `IllegalArgumentException`.
    #[error("invalid page request: {0}")]
    InvalidPageRequest(String),

    /// Bean Validation (`@NotBlank` on `name`) failed before persisting.
    #[error(transparent)]
    Validation(#[from] UserValidationError),

    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<UserStoreError> for UserRepoError {
    fn from(e: UserStoreError) -> Self {
        match e {
            UserStoreError::Validation(v) => UserRepoError::Validation(v),
            UserStoreError::Database(d) => UserRepoError::Database(d),
        }
    }
}

impl From<UserRepoError> for AppError {
    fn from(e: UserRepoError) -> Self {
        match e {
            UserRepoError::Validation(v) => v.into(),
            UserRepoError::InvalidSortProperty(_) | UserRepoError::InvalidPageRequest(_) => {
                AppError::BadRequest(e.to_string())
            }
            // Everything else is an unexpected data-access failure, so it
            // becomes a 500, as an untranslated DataAccessException did in Spring.
            other => AppError::Internal(anyhow_like::Error(other.to_string())),
        }
    }
}

/// Sort direction (`Sort.Direction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Direction {
    #[default]
    Asc,
    Desc,
}

/// One `Sort.Order`. `property` is a `User` field name (`id`, `name`,
/// `email`, `password`, `role`, `about`), not a column name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortOrder {
    pub property: String,
    #[serde(default)]
    pub direction: Direction,
}

impl SortOrder {
    pub fn asc(property: impl Into<String>) -> Self {
        Self { property: property.into(), direction: Direction::Asc }
    }
    pub fn desc(property: impl Into<String>) -> Self {
        Self { property: property.into(), direction: Direction::Desc }
    }
}

/// `PageRequest`: zero-based page index, page size and optional sort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRequest {
    pub page: u32,
    pub size: u32,
    #[serde(default)]
    pub sort: Vec<SortOrder>,
}

impl PageRequest {
    pub fn of(page: u32, size: u32) -> Self {
        Self { page, size, sort: Vec::new() }
    }
    pub fn with_sort(mut self, sort: Vec<SortOrder>) -> Self {
        self.sort = sort;
        self
    }
}

/// `Page<T>`: one slice of results plus totals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    pub content: Vec<T>,
    pub number: u32,
    pub size: u32,
    pub total_elements: i64,
    pub total_pages: i64,
}

/// Maps a `User` property to its physical column. Only whitelisted names are
/// accepted, so user input never reaches the SQL text.
fn column_for(property: &str) -> Result<&'static str, UserRepoError> {
    match property {
        "id" => Ok("user_id"),
        "name" => Ok("user_name"),
        "email" => Ok("user_email"),
        "password" => Ok("user_password"),
        "role" => Ok("user_role"),
        "about" => Ok("user_about"),
        other => Err(UserRepoError::InvalidSortProperty(other.to_string())),
    }
}

/// Builds an `ORDER BY` clause. `user_id` is appended as a tiebreaker so
/// paging is deterministic. An empty sort keeps the default `ORDER BY user_id`.
fn order_by_clause(sort: &[SortOrder]) -> Result<String, UserRepoError> {
    let mut parts = Vec::with_capacity(sort.len() + 1);
    for order in sort {
        let col = column_for(&order.property)?;
        let dir = match order.direction {
            Direction::Asc => "ASC",
            Direction::Desc => "DESC",
        };
        parts.push(format!("{col} {dir}"));
    }
    if !sort.iter().any(|o| o.property == "id") {
        parts.push("user_id ASC".to_string());
    }
    Ok(format!("ORDER BY {}", parts.join(", ")))
}

impl UserRepository {
    /// `User findByName(String name)`: exact match on the `name` column.
    ///
    /// Returns `Ok(None)` when no row matches (Java `null`). If several rows
    /// match it returns [`UserRepoError::NonUniqueResult`], as Spring Data
    /// did. At most two rows are fetched to detect that case.
    pub async fn find_by_name(&self, name: &str) -> Result<Option<User>, UserRepoError> {
        self.ensure_schema().await?;
        let sql = format!(
            "SELECT {USER_SELECT_COLUMNS} FROM \"user\" WHERE user_name = $1 ORDER BY user_id LIMIT 2"
        );
        let mut rows = sqlx::query_as::<_, User>(&sql)
            .bind(name)
            .fetch_all(self.pool())
            .await?;
        match rows.len() {
            0 => Ok(None),
            1 => Ok(rows.pop()),
            _ => Err(UserRepoError::NonUniqueResult { name: name.to_string() }),
        }
    }

    /// `save(S entity)`: JPA persist-or-merge semantics.
    ///
    /// * `id == 0` (the unset primitive id) -> INSERT; the DB assigns the id.
    /// * `id != 0` and the row exists -> full-row overwrite. `None` fields
    ///   overwrite stored values with NULL, as `merge` does.
    /// * `id != 0` and no such row -> INSERT with a newly generated id. Hibernate's
    ///   `merge` of a detached entity with `@GeneratedValue` behaves the same way.
    ///
    /// Returns the persisted state, including the generated id.
    pub async fn save(&self, user: &User) -> Result<User, UserRepoError> {
        if user.id == 0 {
            return Ok(self.insert(user).await?);
        }
        match self.update(user).await? {
            Some(updated) => Ok(updated),
            None => Ok(self.insert(user).await?),
        }
    }

    /// `existsById(Integer id)`.
    pub async fn exists_by_id(&self, id: i32) -> Result<bool, UserRepoError> {
        self.ensure_schema().await?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM \"user\" WHERE user_id = $1)")
                .bind(id)
                .fetch_one(self.pool())
                .await?;
        Ok(exists)
    }

    /// `long count()`.
    pub async fn count(&self) -> Result<i64, UserRepoError> {
        self.ensure_schema().await?;
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM \"user\"")
            .fetch_one(self.pool())
            .await?;
        Ok(n)
    }

    /// `deleteById(Integer id)`. Returns `true` if a row was removed.
    ///
    // MIGRATION_NOTE: Spring Data 2.x threw `EmptyResultDataAccessException`
    // for a missing id; 3.x ignores it silently. This returns `false` and
    // lets the caller decide (e.g. map it to `UserNotFoundError`).
    pub async fn delete_by_id(&self, id: i32) -> Result<bool, UserRepoError> {
        Ok(self.delete(id).await?)
    }

    /// `findAll(Sort sort)`.
    pub async fn find_all_sorted(&self, sort: &[SortOrder]) -> Result<Vec<User>, UserRepoError> {
        let order_by = order_by_clause(sort)?;
        self.ensure_schema().await?;
        let sql = format!("SELECT {USER_SELECT_COLUMNS} FROM \"user\" {order_by}");
        Ok(sqlx::query_as::<_, User>(&sql).fetch_all(self.pool()).await?)
    }

    /// `findAll(Pageable pageable)`.
    pub async fn find_all_paged(&self, request: &PageRequest) -> Result<Page<User>, UserRepoError> {
        if request.size == 0 {
            return Err(UserRepoError::InvalidPageRequest(
                "page size must not be less than one".to_string(),
            ));
        }
        let order_by = order_by_clause(&request.sort)?;
        let total = self.count().await?;
        let limit = i64::from(request.size);
        let offset = i64::from(request.page) * limit;
        let sql =
            format!("SELECT {USER_SELECT_COLUMNS} FROM \"user\" {order_by} LIMIT $1 OFFSET $2");
        let content = sqlx::query_as::<_, User>(&sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(self.pool())
            .await?;
        let total_pages = if total == 0 { 0 } else { (total + limit - 1) / limit };
        Ok(Page {
            content,
            number: request.page,
            size: request.size,
            total_elements: total,
            total_pages,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_by_defaults_to_id() {
        assert_eq!(order_by_clause(&[]).unwrap(), "ORDER BY user_id ASC");
    }

    #[test]
    fn order_by_maps_properties_and_rejects_unknown() {
        let clause = order_by_clause(&[SortOrder::desc("name")]).unwrap();
        assert_eq!(clause, "ORDER BY user_name DESC, user_id ASC");
        assert!(matches!(
            order_by_clause(&[SortOrder::asc("name; DROP TABLE")]),
            Err(UserRepoError::InvalidSortProperty(_))
        ));
    }

    #[test]
    fn non_unique_maps_to_internal() {
        let e: AppError = UserRepoError::NonUniqueResult { name: "a".into() }.into();
        assert!(matches!(e, AppError::Internal(_)));
    }
}
