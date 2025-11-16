use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum Company { Table, Id, Name, Website, Phone, CreatedAt, UpdatedAt }

#[derive(DeriveIden)]
enum Contact { Table, Id, Email, FirstName, LastName, Phone, CompanyId, CreatedAt, UpdatedAt }

#[derive(DeriveMigrationName)]
pub struct Migration;
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Extensions (safe if already present)
        manager.get_connection().execute_unprepared(r#"CREATE EXTENSION IF NOT EXISTS "pgcrypto";"#).await?;

        manager.create_table(
            Table::create()
                .table(Company::Table)
                .if_not_exists()
                .col(ColumnDef::new(Company::Id).uuid().not_null().primary_key().default(Expr::cust("gen_random_uuid()")))
                .col(ColumnDef::new(Company::Name).string_len(256).not_null())
                .col(ColumnDef::new(Company::Website).string_len(512))
                .col(ColumnDef::new(Company::Phone).string_len(64))
                .col(ColumnDef::new(Company::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::cust("now()")))
                .col(ColumnDef::new(Company::UpdatedAt).timestamp_with_time_zone().not_null().default(Expr::cust("now()")))
                .to_owned()
        ).await?;

        manager.create_index(
            Index::create().name("idx_company_name").table(Company::Table).col(Company::Name).to_owned()
        ).await?;

        manager.create_table(
            Table::create()
                .table(Contact::Table)
                .if_not_exists()
                .col(ColumnDef::new(Contact::Id).uuid().not_null().primary_key().default(Expr::cust("gen_random_uuid()")))
                .col(ColumnDef::new(Contact::Email).string_len(320).not_null())
                .col(ColumnDef::new(Contact::FirstName).string_len(128))
                .col(ColumnDef::new(Contact::LastName).string_len(128))
                .col(ColumnDef::new(Contact::Phone).string_len(64))
                .col(ColumnDef::new(Contact::CompanyId).uuid())
                .col(ColumnDef::new(Contact::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::cust("now()")))
                .col(ColumnDef::new(Contact::UpdatedAt).timestamp_with_time_zone().not_null().default(Expr::cust("now()")))
                .foreign_key(ForeignKey::create()
                    .name("fk_contact_company")
                    .from(Contact::Table, Contact::CompanyId)
                    .to(Company::Table, Company::Id)
                    .on_delete(ForeignKeyAction::SetNull)
                    .on_update(ForeignKeyAction::Cascade)
                )
                .to_owned()
        ).await?;

        manager.create_index(
            Index::create().name("idx_contact_email").table(Contact::Table).col(Contact::Email).unique().to_owned()
        ).await?;

        manager.create_index(
            Index::create().name("idx_contact_company").table(Contact::Table).col(Contact::CompanyId).to_owned()
        ).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Contact::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Company::Table).to_owned()).await?;
        Ok(())
    }
}
