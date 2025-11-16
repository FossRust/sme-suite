pub use sea_orm_migration::prelude::*;

mod m20251116_000001_init;
mod m20251116_120000_crm_core;

pub struct Migrator;
#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20251116_000001_init::Migration),
            Box::new(m20251116_120000_crm_core::Migration),
        ]
    }
}
