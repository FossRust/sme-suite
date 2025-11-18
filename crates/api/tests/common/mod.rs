use std::sync::Arc;

use api::{
    auth::{AuthConfig, AuthMode},
    schema::{build_schema, seed_crm_demo, AppSchema, SeededCrmRecords},
};
use async_graphql::Schema;
use migration::Migrator;
use migration::MigratorTrait;
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};
use url::Url;
use uuid::Uuid;

pub struct PgTestContext {
    pub db: Arc<DatabaseConnection>,
    #[allow(dead_code)]
    pub schema:
        Schema<api::schema::QueryRoot, api::schema::MutationRoot, async_graphql::EmptySubscription>,
    #[allow(dead_code)]
    pub seeded: SeededCrmRecords,
    #[allow(dead_code)]
    pub auth: Arc<AuthConfig>,
    admin_url: String,
    db_name: String,
}

impl PgTestContext {
    #[allow(dead_code)]
    pub async fn new_seeded() -> Option<Self> {
        Self::new_with_mode(AuthMode::Disabled).await
    }

    #[allow(dead_code)]
    pub async fn new_seeded_with_mode(mode: AuthMode) -> Option<Self> {
        Self::new_with_mode(mode).await
    }

    async fn new_with_mode(mode: AuthMode) -> Option<Self> {
        let base = std::env::var("TEST_DATABASE_URL").ok()?;
        let (admin_url, db_name, test_url) = build_urls(&base)?;
        let admin = Database::connect(&admin_url).await.ok()?;
        let drop_sql = format!("DROP DATABASE IF EXISTS \"{}\" WITH (FORCE);", db_name);
        let create_sql = format!("CREATE DATABASE \"{}\";", db_name);
        let _ = admin
            .execute(Statement::from_string(DatabaseBackend::Postgres, drop_sql))
            .await;
        admin
            .execute(Statement::from_string(
                DatabaseBackend::Postgres,
                create_sql,
            ))
            .await
            .ok()?;
        let conn = Database::connect(&test_url).await.ok()?;
        Migrator::up(&conn, None).await.ok()?;
        let seeded = seed_crm_demo(&conn).await.ok()?;
        let db = Arc::new(conn);
        let secret = match mode {
            AuthMode::Local => Some("test-secret".to_string()),
            AuthMode::Disabled => None,
        };
        let auth = Arc::new(AuthConfig::new(mode, secret, 15));
        let AppSchema(schema) = build_schema(db.clone(), auth.clone());
        Some(Self {
            db,
            schema,
            seeded,
            auth,
            admin_url,
            db_name,
        })
    }

    pub async fn cleanup(self) {
        let Self {
            db,
            admin_url,
            db_name,
            ..
        } = self;
        drop(db);
        if let Ok(admin) = Database::connect(&admin_url).await {
            let drop_sql = format!("DROP DATABASE IF EXISTS \"{}\" WITH (FORCE);", db_name);
            let _ = admin
                .execute(Statement::from_string(DatabaseBackend::Postgres, drop_sql))
                .await;
        }
    }
}

fn build_urls(base: &str) -> Option<(String, String, String)> {
    let url = Url::parse(base).ok()?;
    let db_path = url.path().trim_start_matches('/').to_string();
    let base_name = if db_path.is_empty() {
        "sme_suite_test".to_string()
    } else {
        db_path
    };
    let db_name = format!("{}_{}", base_name, Uuid::new_v4().simple());
    let mut admin_url = url.clone();
    admin_url.set_path("/postgres");
    let mut test_url = url.clone();
    test_url.set_path(&format!("/{}", db_name));
    Some((admin_url.to_string(), db_name, test_url.to_string()))
}
