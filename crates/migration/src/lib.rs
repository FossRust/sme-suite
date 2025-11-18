pub use sea_orm_migration::prelude::*;

mod m20251116_000001_init;
mod m20251116_120000_crm_core;
mod m20251116_130000_crm_v2;
mod m20251116_140000_crm_search;
mod m20251116_150000_crm_pipeline;

pub struct Migrator;
#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20251116_000001_init::Migration),
            Box::new(m20251116_120000_crm_core::Migration),
            Box::new(m20251116_130000_crm_v2::Migration),
            Box::new(m20251116_140000_crm_search::Migration),
            Box::new(m20251116_150000_crm_pipeline::Migration),
        ]
    }
}
