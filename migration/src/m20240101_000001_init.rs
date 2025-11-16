use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

const DOWN_SQL: &str = r#"
DROP POLICY IF EXISTS org_isolation ON notifications;
DROP POLICY IF EXISTS org_isolation ON tasks;
DROP POLICY IF EXISTS org_isolation ON activity_feed;
DROP POLICY IF EXISTS org_isolation ON policies;
DROP POLICY IF EXISTS org_isolation ON entitlements;
DROP POLICY IF EXISTS org_isolation ON memberships;

ALTER TABLE notifications DISABLE ROW LEVEL SECURITY;
ALTER TABLE tasks DISABLE ROW LEVEL SECURITY;
ALTER TABLE activity_feed DISABLE ROW LEVEL SECURITY;
ALTER TABLE policies DISABLE ROW LEVEL SECURITY;
ALTER TABLE entitlements DISABLE ROW LEVEL SECURITY;
ALTER TABLE memberships DISABLE ROW LEVEL SECURITY;

DROP TABLE IF EXISTS notifications CASCADE;
DROP TABLE IF EXISTS tasks CASCADE;
DROP TABLE IF EXISTS activity_feed CASCADE;
DROP TABLE IF EXISTS policies CASCADE;
DROP TABLE IF EXISTS entitlements CASCADE;
DROP TABLE IF EXISTS memberships CASCADE;
DROP TABLE IF EXISTS users CASCADE;
DROP TABLE IF EXISTS orgs CASCADE;

DROP FUNCTION IF EXISTS current_tenant();
"#;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql = include_str!("../../migrations/0001_init.sql");
        manager
            .get_connection()
            .execute_unprepared(sql)
            .await
            .map(|_| ())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(DOWN_SQL)
            .await
            .map(|_| ())
    }
}
