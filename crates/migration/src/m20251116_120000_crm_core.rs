use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
enum Deal {
    Table,
    Id,
    Title,
    AmountCents,
    Currency,
    Stage,
    CloseDate,
    CompanyId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum DealStageEnum {
    #[sea_orm(iden = "deal_stage")]
    Table,
}

#[derive(DeriveIden)]
enum Company {
    Table,
    Id,
    Name,
    Website,
    Phone,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Contact {
    Table,
    Id,
    Email,
    FirstName,
    LastName,
    Phone,
    CompanyId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

const DEAL_STAGE_VALUES: &[&str] = &["NEW", "QUALIFY", "PROPOSAL", "NEGOTIATE", "WON", "LOST"];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let create_enum_sql = format!(
            "DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'deal_stage') THEN CREATE TYPE deal_stage AS ENUM ({}); END IF; END $$;",
            DEAL_STAGE_VALUES
                .iter()
                .map(|v| format!("'{}'", v))
                .collect::<Vec<_>>()
                .join(", ")
        );
        manager
            .get_connection()
            .execute_unprepared(&create_enum_sql)
            .await?;

        // Ensure base tables
        manager
            .create_table(
                Table::create()
                    .table(Company::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Company::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(Company::Name).string_len(256).not_null())
                    .col(ColumnDef::new(Company::Website).string_len(512))
                    .col(ColumnDef::new(Company::Phone).string_len(64))
                    .col(
                        ColumnDef::new(Company::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::cust("now()")),
                    )
                    .col(
                        ColumnDef::new(Company::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::cust("now()")),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_company_name")
                    .table(Company::Table)
                    .col(Company::Name)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Contact::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Contact::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(Contact::Email).string_len(320).not_null())
                    .col(ColumnDef::new(Contact::FirstName).string_len(128))
                    .col(ColumnDef::new(Contact::LastName).string_len(128))
                    .col(ColumnDef::new(Contact::Phone).string_len(64))
                    .col(ColumnDef::new(Contact::CompanyId).uuid())
                    .col(
                        ColumnDef::new(Contact::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::cust("now()")),
                    )
                    .col(
                        ColumnDef::new(Contact::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::cust("now()")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_contact_company")
                            .from(Contact::Table, Contact::CompanyId)
                            .to(Company::Table, Company::Id)
                            .on_delete(ForeignKeyAction::SetNull)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_contact_email")
                    .table(Contact::Table)
                    .col(Contact::Email)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_contact_company")
                    .table(Contact::Table)
                    .col(Contact::CompanyId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Deal::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Deal::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(Deal::Title).string_len(300).not_null())
                    .col(ColumnDef::new(Deal::AmountCents).big_integer())
                    .col(ColumnDef::new(Deal::Currency).string_len(3))
                    .col(
                        ColumnDef::new(Deal::Stage)
                            .custom(DealStageEnum::Table)
                            .not_null()
                            .default(Expr::cust("'NEW'::deal_stage")),
                    )
                    .col(ColumnDef::new(Deal::CloseDate).date())
                    .col(ColumnDef::new(Deal::CompanyId).uuid().not_null())
                    .col(
                        ColumnDef::new(Deal::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::cust("now()")),
                    )
                    .col(
                        ColumnDef::new(Deal::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::cust("now()")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_deal_company")
                            .from(Deal::Table, Deal::CompanyId)
                            .to(Company::Table, Company::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_deal_company")
                    .table(Deal::Table)
                    .col(Deal::CompanyId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_deal_stage")
                    .table(Deal::Table)
                    .col(Deal::Stage)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Deal::Table).to_owned())
            .await?;
        manager
            .get_connection()
            .execute_unprepared("DROP TYPE IF EXISTS deal_stage;")
            .await?;
        Ok(())
    }
}
