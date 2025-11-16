use anyhow::{Context, Result, anyhow};
use migration::{Migrator, MigratorTrait};
use platform_db::{DbPool, with_tenant};
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};
use testcontainers::{GenericImage, clients::Cli, core::WaitFor};
use uuid::Uuid;

#[tokio::test]
async fn rls_keeps_orgs_isolated() -> Result<()> {
    let docker = Cli::default();
    let image = GenericImage::new("postgres", "16-alpine")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_DB", "postgres")
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections",
        ));
    let container = docker.run(image);
    let port = container.get_host_port_ipv4(5432);
    let admin_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");

    let admin_pool = Database::connect(&admin_url).await?;

    Migrator::up(&admin_pool, None).await?;
    provision_app_role(&admin_pool).await?;

    let app_url = format!("postgres://app_user:app_pass@127.0.0.1:{port}/postgres");
    let pool = Database::connect(&app_url).await?;

    let org_a = Uuid::new_v4();
    let org_b = Uuid::new_v4();
    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    seed_org(&admin_pool, org_a, "org-a", "Org A").await?;
    seed_org(&admin_pool, org_b, "org-b", "Org B").await?;
    seed_user(&admin_pool, user_a, "a@example.com").await?;
    seed_user(&admin_pool, user_b, "b@example.com").await?;
    add_membership(&admin_pool, org_a, user_a).await?;
    add_membership(&admin_pool, org_b, user_b).await?;

    insert_activity(&pool, org_a, "crm", "note", user_a).await?;

    let count_a = tenant_count(&pool, org_a).await?;
    let count_b = tenant_count(&pool, org_b).await?;
    assert_eq!(count_a, 1);
    assert_eq!(count_b, 0);

    let conn = with_tenant(&pool, org_a).await?;
    let insert_err = conn
        .execute(Statement::from_string(
            DatabaseBackend::Postgres,
            format!(
                "INSERT INTO activity_feed (org_id, app, type, entity_id, title, href) \
                 VALUES ('{org_b}', 'crm', 'note', '{user_a}', 'Cross', '/href')"
            ),
        ))
        .await
        .expect_err("RLS should reject cross-tenant insert");

    assert!(
        insert_err
            .to_string()
            .contains("violates row-level security policy")
    );
    Ok(())
}

async fn seed_org(pool: &DbPool, id: Uuid, slug: &str, name: &str) -> Result<()> {
    exec(
        pool,
        format!("INSERT INTO orgs (id, slug, name) VALUES ('{id}', '{slug}', '{name}')"),
    )
    .await
}

async fn seed_user(pool: &DbPool, id: Uuid, email: &str) -> Result<()> {
    exec(
        pool,
        format!("INSERT INTO users (id, email) VALUES ('{id}', '{email}')"),
    )
    .await
}

async fn add_membership(pool: &DbPool, org: Uuid, user: Uuid) -> Result<()> {
    exec(
        pool,
        format!("INSERT INTO memberships (org_id, user_id) VALUES ('{org}', '{user}')"),
    )
    .await
}

async fn insert_activity(
    pool: &DbPool,
    tenant: Uuid,
    app: &str,
    kind: &str,
    entity: Uuid,
) -> Result<()> {
    let conn = with_tenant(pool, tenant).await?;
    conn.execute(Statement::from_string(
        DatabaseBackend::Postgres,
        format!(
            "INSERT INTO activity_feed (org_id, app, type, entity_id, title, href) \
                 VALUES ('{tenant}', '{app}', '{kind}', '{entity}', 'Created', '/entity')"
        ),
    ))
    .await?;
    conn.commit().await?;
    Ok(())
}

async fn tenant_count(pool: &DbPool, tenant: Uuid) -> Result<i64> {
    let conn = with_tenant(pool, tenant).await?;
    let row = conn
        .query_one(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT count(*) as count FROM activity_feed".to_string(),
        ))
        .await?
        .context("missing count row")?;
    let count: i64 = sea_orm::TryGetable::try_get(&row, "", "count")
        .map_err(|err| anyhow!(format!("failed to extract count from row: {err:?}")))?;
    conn.commit().await?;
    Ok(count)
}

async fn provision_app_role(pool: &DbPool) -> Result<()> {
    exec(pool, "DROP ROLE IF EXISTS app_user").await?;
    exec(pool, "CREATE ROLE app_user LOGIN PASSWORD 'app_pass'").await?;
    exec(
        pool,
        "GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO app_user",
    )
    .await?;
    exec(
        pool,
        "GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public TO app_user",
    )
    .await?;
    exec(
        pool,
        "ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON TABLES TO app_user",
    )
    .await?;
    exec(
        pool,
        "ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON SEQUENCES TO app_user",
    )
    .await?;
    Ok(())
}

async fn exec(pool: &DbPool, sql: impl Into<String>) -> Result<()> {
    pool.execute(Statement::from_string(
        DatabaseBackend::Postgres,
        sql.into(),
    ))
    .await?;
    Ok(())
}
