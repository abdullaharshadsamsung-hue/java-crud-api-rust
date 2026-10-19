//! `UserServiceImp`: the `@Service` implementation of `UserService`.
//!
//! The free functions in `crate::services::user_service` already carry the
//! interface contract. This module adds the concrete service object: it owns
//! the injected repository (`@Autowired UserDao userDao`) and exposes the
//! `UserServiceImp` methods, delegating to those functions.
//!
//! The one behavioural difference from the free functions is `delete_user`.
//! Here it follows Spring Data 2.x: `deleteById` on an id that doesn't exist
//! throws `EmptyResultDataAccessException`, which surfaces as a 500.

use crate::error::{anyhow_like, AppError};
use crate::models::user::{User, UserRepository};
use crate::services::user_service;

/// Concrete user service holding the injected repository.
pub struct UserServiceImp {
    user_dao: UserRepository,
}

impl UserServiceImp {
    /// Constructor injection replaces `@Autowired` field injection.
    pub fn new(user_dao: UserRepository) -> Self {
        Self { user_dao }
    }

    /// Access to the underlying repository (`UserDao`).
    pub fn user_dao(&self) -> &UserRepository {
        &self.user_dao
    }

    /// `User saveUser(User user)`: persists the user (JPA save semantics) and
    /// returns the saved entity.
    pub async fn save_user(&self, user: User) -> Result<User, AppError> {
        user_service::save_user(&self.user_dao, user).await
    }

    /// `List<User> fetchUserList()`: returns all users.
    pub async fn fetch_user_list(&self) -> Result<Vec<User>, AppError> {
        user_service::fetch_user_list(&self.user_dao).await
    }

    /// `User fetchUserById(int id) throws UserNotFoundException`: returns a
    /// 404 `UserNotFoundError("User are not available")` if the user is absent.
    pub async fn fetch_user_by_id(&self, id: i32) -> Result<User, AppError> {
        user_service::fetch_user_by_id(&self.user_dao, id).await
    }

    /// `void deleteUser(int id)` -> `userDao.deleteById(id)`.
    ///
    /// Spring Data 2.x semantics: if no row was deleted, return a 500.
    pub async fn delete_user(&self, id: i32) -> Result<(), AppError> {
        let removed = self.user_dao.delete_by_id(id).await?;
        if !removed {
            return Err(AppError::Internal(anyhow_like::Error(format!(
                "No class com.smartContact.model.User entity with id {id} exists!"
            ))));
        }
        Ok(())
    }

    /// `void updateUser(int id, User user)`: sets the id on the user and
    /// saves it (upsert / merge).
    pub async fn update_user(&self, id: i32, user: User) -> Result<(), AppError> {
        user_service::update_user(&self.user_dao, id, user).await
    }

    /// `User getUserNameByName(String name)`: exact-name lookup. Returns
    /// `None` if no user matches (Java `null`) and a 500 if the name matches
    /// more than one user.
    pub async fn get_user_name_by_name(&self, name: &str) -> Result<Option<User>, AppError> {
        user_service::get_user_name_by_name(&self.user_dao, name).await
    }
}
