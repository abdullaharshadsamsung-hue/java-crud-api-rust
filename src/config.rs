//! Typed application configuration.
//!
//! Every environment variable this project reads is declared here, so
//! there are no scattered `std::env::var` calls through the codebase.

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub database_url: String,
    pub secret_key: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8080),
            database_url: std::env::var("DATABASE_URL").unwrap_or_default(),
            secret_key: std::env::var("SECRET_KEY")
                .unwrap_or_else(|_| "change-me".to_string()),
        }
    }
}
