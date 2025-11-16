pub use sea_orm_migration::prelude::*;

mod m20240101_000001_init;
mod m20240102_000002_sessions_and_default_org;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240101_000001_init::Migration),
            Box::new(m20240102_000002_sessions_and_default_org::Migration),
        ]
    }
}
