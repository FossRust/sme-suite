use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "CREATE EXTENSION IF NOT EXISTS pg_trgm;",
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "CREATE EXTENSION IF NOT EXISTS unaccent;",
        ))
        .await?;

        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
            ALTER TABLE company
            ADD COLUMN IF NOT EXISTS tsv tsvector GENERATED ALWAYS AS (
                setweight(to_tsvector('simple', coalesce(name, '')), 'A') ||
                setweight(to_tsvector('simple', coalesce(website, '')), 'D')
            ) STORED;
            "#,
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "CREATE INDEX IF NOT EXISTS idx_company_tsv ON company USING GIN (tsv);",
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "CREATE INDEX IF NOT EXISTS idx_company_name_trgm ON company USING GIN (name gin_trgm_ops);",
        ))
        .await?;

        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
            ALTER TABLE contact
            ADD COLUMN IF NOT EXISTS tsv tsvector GENERATED ALWAYS AS (
                setweight(to_tsvector('simple', coalesce(email, '')), 'A') ||
                setweight(to_tsvector('simple', coalesce(first_name, '')), 'B') ||
                setweight(to_tsvector('simple', coalesce(last_name, '')), 'B') ||
                setweight(to_tsvector('simple', coalesce(phone, '')), 'D')
            ) STORED;
            "#,
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "CREATE INDEX IF NOT EXISTS idx_contact_tsv ON contact USING GIN (tsv);",
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "CREATE INDEX IF NOT EXISTS idx_contact_email_trgm ON contact USING GIN (email gin_trgm_ops);",
        ))
        .await?;

        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
            ALTER TABLE deal
            ADD COLUMN IF NOT EXISTS tsv tsvector GENERATED ALWAYS AS (
                setweight(to_tsvector('simple', coalesce(title, '')), 'A')
            ) STORED;
            "#,
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "CREATE INDEX IF NOT EXISTS idx_deal_tsv ON deal USING GIN (tsv);",
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "CREATE INDEX IF NOT EXISTS idx_deal_title_trgm ON deal USING GIN (title gin_trgm_ops);",
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "DROP INDEX IF EXISTS idx_deal_title_trgm;",
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "DROP INDEX IF EXISTS idx_deal_tsv;",
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "ALTER TABLE deal DROP COLUMN IF EXISTS tsv;",
        ))
        .await?;

        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "DROP INDEX IF EXISTS idx_contact_email_trgm;",
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "DROP INDEX IF EXISTS idx_contact_tsv;",
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "ALTER TABLE contact DROP COLUMN IF EXISTS tsv;",
        ))
        .await?;

        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "DROP INDEX IF EXISTS idx_company_name_trgm;",
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "DROP INDEX IF EXISTS idx_company_tsv;",
        ))
        .await?;
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "ALTER TABLE company DROP COLUMN IF EXISTS tsv;",
        ))
        .await?;

        Ok(())
    }
}
