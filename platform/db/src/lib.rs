//! Lightweight database primitives. Actual pool wiring lands in Task 02.

use serde::Deserialize;
use sqlx::{Pool, Postgres};
use thiserror::Error;

/// Shared Postgres pool alias.
pub type DbPool = Pool<Postgres>;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database url missing")]
    MissingUrl,
    #[error("database pool not initialized yet")]
    PoolNotInitialized,
}

pub type DbResult<T> = Result<T, DbError>;

/// Basic environment-driven settings for future DB wiring.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DatabaseSettings {
    #[serde(default = "default_url_key")]
    env_key: String,
}

fn default_url_key() -> String {
    "DATABASE_URL".to_string()
}

impl DatabaseSettings {
    pub fn new(env_key: impl Into<String>) -> Self {
        Self {
            env_key: env_key.into(),
        }
    }

    #[allow(dead_code)]
    pub fn database_url(&self) -> Result<String, DbError> {
        std::env::var(&self.env_key).map_err(|_| DbError::MissingUrl)
    }
}
