//! User service layer. Port of the `com.smartContact.service.UserService`
//! interface and its only implementation, `UserServiceImp`.
//!
//! There was only one implementation, so there is no trait here. The service
//! is a set of free async functions that take the shared [`UserRepository`],
//! which plays the role of the `@Autowired UserDao`. Handlers call them with
//! the repository stored in `AppState`.
//!
//! Errors come back as [`AppError`]:
//! * a missing user in `fetch_user_by_id` becomes `UserNotFoundError("User are not available")`,
//!   which renders as 404 `{"status":"NOT_FOUND","message":"User are not available"}`.
//!   This is the checked `UserNotFoundException` from the source.
//! * a validation failure (`@NotBlank` on name) becomes 400.
//! * any other data-access failure becomes 500.

use crate::error::{AppError, UserNotFoundError};
use crate::models::user::{User, UserRepository};
use crate::repository::user_repo::UserRepoError;

/// Message used by `UserServiceImp.fetchUserById` when no row matches.
pub const USER_NOT_AVAILABLE_MESSAGE: &str = "User are not available";

/// `User saveUser(User user)` -> `userDao.save(user)`.
///
/// Uses JPA persist-or-merge semantics: `id == 0` inserts, otherwise the row
/// is overwritten (or inserted if it does not exist). Returns the saved
/// entity together with its generated id.
pub async fn save_user(repo: &UserRepository, user: User) -> Result<User, AppError> {
    Ok(repo.save(&user).await?)
}

/// `List<User> fetchUserList()` -> `userDao.findAll()`.
pub async fn fetch_user_list(repo: &UserRepository) -> Result<Vec<User>, AppError> {
    Ok(repo.find_all().await.map_err(UserRepoError::from)?)
}

/// `User fetchUserById(int id) throws UserNotFoundException`.
pub async fn fetch_user_by_id(repo: &UserRepository, id: i32) -> Result<User, AppError> {
    repo.find_by_id(id)
        .await
        .map_err(UserRepoError::from)?
        .ok_or_else(|| UserNotFoundError::with_message(USER_NOT_AVAILABLE_MESSAGE).into())
}

/// `void deleteUser(int id)` -> `userDao.deleteById(id)`.
///
// MIGRATION_NOTE: a missing id is ignored here, as Spring Data 3.x does.
// Spring Data 2.x threw `EmptyResultDataAccessException` instead, which gave
// a 500. Check which version the source used if that difference matters.
pub async fn delete_user(repo: &UserRepository, id: i32) -> Result<(), AppError> {
    let _removed = repo.delete_by_id(id).await?;
    Ok(())
}

/// `void updateUser(int id, User user)`: does `user.setId(id)` and then
/// `userDao.save(user)`.
///
/// This is a full-row merge, so `None` fields overwrite stored values with
/// NULL. If no row has that id, a new row is inserted (Hibernate merge
/// semantics). Nothing is returned, like the Java `void` method.
pub async fn update_user(repo: &UserRepository, id: i32, mut user: User) -> Result<(), AppError> {
    user.set_id(id);
    repo.save(&user).await?;
    Ok(())
}

/// `User getUserNameByName(String name)` -> `userDao.findByName(name)`.
///
/// Returns `None` when no user matches (Java `null`; the controller turns
/// this into an empty 200). If several users share the name, the result is a
/// 500, matching Spring's `IncorrectResultSizeDataAccessException`.
pub async fn get_user_name_by_name(
    repo: &UserRepository,
    name: &str,
) -> Result<Option<User>, AppError> {
    Ok(repo.find_by_name(name).await?)
}
