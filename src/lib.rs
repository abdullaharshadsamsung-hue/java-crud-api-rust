//! The application as a library: every module, the shared state and
//! the router. main.rs only starts the server, so the integration tests in
//! tests/ can build the same Router (`use migrated_service::{app, AppState}`)
//! and drive it without a network listener.

pub mod config;
pub mod error;

use std::sync::Arc;

use axum::{routing::get, Json, Router};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

use crate::config::Config;

/// Shared application state.
///
/// Handed to handlers via axum's `State` extractor as an `Arc`, which is
/// the ownership pattern to reach for by default — it sidesteps the
/// lifetime problems that a borrowed state struct would create.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Every route the service exposes.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
