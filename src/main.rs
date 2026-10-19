//! Starts the server; the application itself is in lib.rs.

use std::sync::Arc;

use migrated_service::{app, config::Config, AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let config = Config::from_env();
    let port = config.port;
    let state = AppState { config: Arc::new(config) };

    // 0.0.0.0, not localhost — the app runs in a container and must be
    // reachable from outside it.
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("listening on port {}", port);

    axum::serve(listener, app(state)).await?;
    Ok(())
}
