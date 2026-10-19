//! HTTP handlers for the `User` resource. Port of
//! `com.smartContact.Controller.UserController`.
//!
//! Every handler delegates to [`UserServiceImp`]; the controller holds no
//! business logic of its own. Errors are returned as [`AppError`], which
//! renders Spring-compatible responses (empty-body 400/415, 404 `ErrorMessage`
//! JSON, Boot-style 500 JSON).
//!
//! Routes (axum 0.7 path syntax):
//! * `POST   /save_user_data`              -> [`save_user`]
//! * `GET    /get_user_data`               -> [`fetch_user_list`]
//! * `GET    /get_user_data/:id`           -> [`fetch_user_by_id`]
//! * `DELETE /delete_user_data/:id`        -> [`delete_user`]
//! * `PUT    /update_user_data/:id`        -> [`update_user`]
//! * `GET    /get_user_name/name/:name`    -> [`get_user_name_by_name`]
//!
//! MIGRATION_NOTE: `AppState` in `src/lib.rs` currently only carries `config`.
//! These handlers therefore use their own state type, [`UserHandlerState`],
//! and expose [`user_routes`], which returns a ready-to-merge `Router`
//! (state already applied). Wire it up in `lib.rs::app()` with
//! `.merge(handlers::user_handlers::user_routes(service))`, and declare
//! `pub mod handlers;` (with `handlers/mod.rs` containing
//! `pub mod user_handlers;`). Trailing-slash matching is expected to be
//! provided by `NormalizePathLayer::trim_trailing_slash` in main.rs.

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{OriginalUri, Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};

use crate::error::AppError;
use crate::models::user::User;
use crate::services::user_service_imp::UserServiceImp;

/// Shared state for the user handlers (replaces `@Autowired UserService`).
#[derive(Clone)]
pub struct UserHandlerState {
    pub service: Arc<UserServiceImp>,
}

impl UserHandlerState {
    pub fn new(service: UserServiceImp) -> Self {
        Self {
            service: Arc::new(service),
        }
    }
}

/// Builds the router with every `UserController` route registered.
pub fn user_routes(service: UserServiceImp) -> Router {
    Router::new()
        .route("/save_user_data", post(save_user))
        .route("/get_user_data", get(fetch_user_list))
        .route("/get_user_data/:id", get(fetch_user_by_id))
        .route("/delete_user_data/:id", delete(delete_user))
        .route("/update_user_data/:id", put(update_user))
        .route("/get_user_name/name/:name", get(get_user_name_by_name))
        .with_state(UserHandlerState::new(service))
}

/// Maps an axum JSON body rejection to the Spring equivalent:
/// missing/unsupported content type -> 415, anything else -> 400 (empty body).
fn json_rejection(rej: JsonRejection) -> AppError {
    match rej {
        JsonRejection::MissingJsonContentType(_) => AppError::UnsupportedMediaType,
        _ => AppError::BadRequest,
    }
}

/// `@PathVariable` type mismatch -> 400 with an empty body.
fn path_rejection(_rej: PathRejection) -> AppError {
    AppError::BadRequest
}

fn text_ok(body: &'static str) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain;charset=UTF-8")],
        body,
    )
        .into_response()
}

/// `POST /save_user_data` — `@Valid @RequestBody User`.
pub async fn save_user(
    State(state): State<UserHandlerState>,
    OriginalUri(uri): OriginalUri,
    body: Result<Json<User>, JsonRejection>,
) -> Result<Response, AppError> {
    tracing::info!("inside the saveUser of UserController ");
    let Json(user) = body.map_err(json_rejection)?;
    // Bean validation (@Valid): MethodArgumentNotValidException -> 400, empty body.
    if user.validate().is_err() {
        return Err(AppError::BadRequest);
    }
    state
        .service
        .save_user(user)
        .await
        .map_err(|e| e.with_path(uri.path()))?;
    Ok(text_ok("User data saved successfully!"))
}

/// `GET /get_user_data` — all users as a JSON array (`[]` when empty).
pub async fn fetch_user_list(
    State(state): State<UserHandlerState>,
    OriginalUri(uri): OriginalUri,
) -> Result<Json<Vec<User>>, AppError> {
    tracing::info!("inside the fetchUserList of UserController ");
    let users = state
        .service
        .fetch_user_list()
        .await
        .map_err(|e| e.with_path(uri.path()))?;
    Ok(Json(users))
}

/// `GET /get_user_data/{id}` — 200 user, 404 `{status, message}` if absent.
pub async fn fetch_user_by_id(
    State(state): State<UserHandlerState>,
    OriginalUri(uri): OriginalUri,
    id: Result<Path<i32>, PathRejection>,
) -> Result<Json<User>, AppError> {
    let Path(id) = id.map_err(path_rejection)?;
    let user = state
        .service
        .fetch_user_by_id(id)
        .await
        .map_err(|e| e.with_path(uri.path()))?;
    Ok(Json(user))
}

/// `DELETE /delete_user_data/{id}` — 200 text, 500 if the id does not exist
/// (Spring Data 2.x `EmptyResultDataAccessException`, enforced in the service).
pub async fn delete_user(
    State(state): State<UserHandlerState>,
    OriginalUri(uri): OriginalUri,
    id: Result<Path<i32>, PathRejection>,
) -> Result<Response, AppError> {
    let Path(id) = id.map_err(path_rejection)?;
    state
        .service
        .delete_user(id)
        .await
        .map_err(|e| e.with_path(uri.path()))?;
    Ok(text_ok("user data deleted Successfully"))
}

/// `PUT /update_user_data/{id}` — no bean validation; echoes the request body.
///
/// The Java service mutates the same `User` instance (`user.setId(id)`) before
/// the controller returns it, so the echoed body carries the path id.
pub async fn update_user(
    State(state): State<UserHandlerState>,
    OriginalUri(uri): OriginalUri,
    id: Result<Path<i32>, PathRejection>,
    body: Result<Json<User>, JsonRejection>,
) -> Result<Json<User>, AppError> {
    let Path(id) = id.map_err(path_rejection)?;
    let Json(mut user) = body.map_err(json_rejection)?;
    user.set_id(id);
    state
        .service
        .update_user(id, user.clone())
        .await
        .map_err(|e| e.with_path(uri.path()))?;
    Ok(Json(user))
}

/// `GET /get_user_name/name/{name}` — 200 user, or 200 with an EMPTY body
/// when no user matches (Spring writes nothing for a `null` return value).
pub async fn get_user_name_by_name(
    State(state): State<UserHandlerState>,
    OriginalUri(uri): OriginalUri,
    name: Result<Path<String>, PathRejection>,
) -> Result<Response, AppError> {
    let Path(name) = name.map_err(path_rejection)?;
    let found = state
        .service
        .get_user_name_by_name(&name)
        .await
        .map_err(|e| e.with_path(uri.path()))?;
    Ok(match found {
        Some(user) => Json(user).into_response(),
        None => StatusCode::OK.into_response(),
    })
}
