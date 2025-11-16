//! Database primitives: pooled connections
use chrono::Utc;
use entity::{memberships, orgs, users};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectOptions, Database, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, Set,
};
use serde::Deserialize;
use thiserror::Error;
use uuid::Uuid;

/// Shared Postgres pool alias built on SeaORM.
pub type DbPool = DatabaseConnection;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database url missing")]
    MissingUrl,
    #[error(transparent)]
    SeaOrm(#[from] sea_orm::DbErr),
}

pub type DbResult<T> = Result<T, DbError>;

/// Basic environment-driven settings.
#[derive(Clone, Debug, Deserialize)]
pub struct DatabaseSettings {
    database_url: Option<String>,
    #[serde(default = "default_max_connections")]
    max_connections: u32,
}

const fn default_max_connections() -> u32 {
    10
}

impl Default for DatabaseSettings {
    fn default() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL").ok(),
            max_connections: default_max_connections(),
        }
    }
}

impl DatabaseSettings {
    /// Construct settings from environment variables (DATABASE_URL).
    pub fn from_env() -> Self {
        Self::default()
    }

    /// Override the connection string (useful in tests).
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.database_url = Some(url.into());
        self
    }

    pub fn database_url(&self) -> DbResult<&str> {
        self.database_url.as_deref().ok_or(DbError::MissingUrl)
    }

    pub fn max_connections(&self) -> u32 {
        self.max_connections
    }
}

/// Initialize a Postgres connection pool using SeaORM (rustls TLS).
pub async fn connect(settings: &DatabaseSettings) -> DbResult<DbPool> {
    let url = settings.database_url()?;
    let mut opts = ConnectOptions::new(url.to_owned());
    opts.max_connections(settings.max_connections());
    opts.sqlx_logging(false);
    Database::connect(opts).await.map_err(DbError::from)
}

/// Ensure an organization exists for the provided slug; returns its ID.
pub async fn ensure_default_org(pool: &DbPool, slug: &str, name: &str) -> DbResult<Uuid> {
    if let Some(existing) = orgs::Entity::find()
        .filter(orgs::Column::Slug.eq(slug))
        .one(pool)
        .await?
    {
        return Ok(existing.id);
    }

    let new_id = Uuid::new_v4();
    let model = orgs::ActiveModel {
        id: Set(new_id),
        slug: Set(slug.to_string()),
        name: Set(name.to_string()),
        created_at: Set(Utc::now().into()),
    };
    model.insert(pool).await?;
    Ok(new_id)
}

pub async fn upsert_user(
    pool: &DbPool,
    email: &str,
    name: Option<String>,
) -> DbResult<users::Model> {
    if let Some(existing) = users::Entity::find()
        .filter(users::Column::Email.eq(email))
        .one(pool)
        .await?
    {
        return Ok(existing);
    }

    let model = users::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set(email.to_string()),
        name: Set(name),
        created_at: Set(Utc::now().into()),
    };
    let inserted = model.insert(pool).await?;
    Ok(inserted)
}

pub async fn ensure_membership(
    pool: &DbPool,
    org_id: Uuid,
    user_id: Uuid,
    roles: Vec<String>,
) -> DbResult<memberships::Model> {
    if let Some(existing) = memberships::Entity::find_by_id((org_id, user_id))
        .one(pool)
        .await?
    {
        return Ok(existing);
    }

    let model = memberships::ActiveModel {
        org_id: Set(org_id),
        user_id: Set(user_id),
        roles: Set(roles),
        created_at: Set(Utc::now().into()),
    };
    Ok(model.insert(pool).await?)
}

pub async fn user_count(pool: &DbPool) -> DbResult<u64> {
    let count = users::Entity::find().count(pool).await?;
    Ok(count)
}
