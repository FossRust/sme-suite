//! Database configuration + connection primitives.

use sea_orm::{Database, DatabaseConnection};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database url missing")]
    MissingUrl,
    #[error(transparent)]
    SeaOrm(#[from] sea_orm::DbErr),
}

pub type DbResult<T> = Result<T, DbError>;

#[derive(Clone, Debug, Deserialize)]
pub struct DatabaseSettings {
    database_url: Option<String>,
}

impl DatabaseSettings {
    pub fn from_env() -> Self {
        let database_url = std::env::var("DATABASE_URL").ok();
        Self { database_url }
    }

    pub fn database_url(&self) -> Option<&str> {
        self.database_url.as_deref()
    }

    pub async fn connect(&self) -> DbResult<DatabaseConnection> {
        let url = self.database_url().ok_or(DbError::MissingUrl)?;
        Database::connect(url).await.map_err(DbError::from)
    }
}

impl Default for DatabaseSettings {
    fn default() -> Self {
        Self { database_url: None }
    }
}
