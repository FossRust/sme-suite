use sea_orm_migration::prelude::*;
use sea_query::SimpleExpr;

#[derive(DeriveIden, Copy, Clone)]
enum Activity {
    Table,
    Id,
    EntityType,
    EntityId,
    Kind,
    Subject,
    BodyMd,
    MetaJson,
    CreatedAt,
    CreatedBy,
}

#[derive(DeriveIden, Copy, Clone)]
enum Task {
    Table,
    Id,
    Title,
    Notes,
    Status,
    Priority,
    DueAt,
    CompletedAt,
    CompanyId,
    ContactId,
    DealId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden, Copy, Clone)]
enum DealStageHistory {
    Table,
    Id,
    DealId,
    FromStage,
    ToStage,
    ChangedAt,
    Note,
    ChangedBy,
}

#[derive(DeriveIden, Copy, Clone)]
enum Company {
    Table,
    Id,
}

#[derive(DeriveIden, Copy, Clone)]
enum Contact {
    Table,
    Id,
}

#[derive(DeriveIden, Copy, Clone)]
enum Deal {
    Table,
    Id,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Activity::Table)
                    .if_not_exists()
                    .col(&mut uuid_pk(Activity::Id))
                    .col(
                        ColumnDef::new(Activity::EntityType)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Activity::EntityId).uuid().not_null())
                    .col(ColumnDef::new(Activity::Kind).string_len(32).not_null())
                    .col(ColumnDef::new(Activity::Subject).string_len(512))
                    .col(ColumnDef::new(Activity::BodyMd).text())
                    .col(
                        ColumnDef::new(Activity::MetaJson)
                            .json_binary()
                            .not_null()
                            .default(Expr::cust("'{}'::jsonb")),
                    )
                    .col(&mut timestamp_with_default(Activity::CreatedAt))
                    .col(ColumnDef::new(Activity::CreatedBy).string_len(128))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_activity_kind")
                    .table(Activity::Table)
                    .col(Activity::Kind)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_activity_entity")
                    .table(Activity::Table)
                    .col(Activity::EntityType)
                    .col(Activity::EntityId)
                    .col(Activity::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Task::Table)
                    .if_not_exists()
                    .col(&mut uuid_pk(Task::Id))
                    .col(ColumnDef::new(Task::Title).string_len(512).not_null())
                    .col(ColumnDef::new(Task::Notes).text())
                    .col(
                        ColumnDef::new(Task::Status)
                            .string_len(32)
                            .not_null()
                            .default(Expr::cust("'OPEN'")),
                    )
                    .col(
                        ColumnDef::new(Task::Priority)
                            .string_len(32)
                            .not_null()
                            .default(Expr::cust("'MEDIUM'")),
                    )
                    .col(ColumnDef::new(Task::DueAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Task::CompletedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Task::CompanyId).uuid())
                    .col(ColumnDef::new(Task::ContactId).uuid())
                    .col(ColumnDef::new(Task::DealId).uuid())
                    .col(&mut timestamp_with_default(Task::CreatedAt))
                    .col(&mut timestamp_with_default(Task::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_task_company")
                            .from(Task::Table, Task::CompanyId)
                            .to(Company::Table, Company::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_task_contact")
                            .from(Task::Table, Task::ContactId)
                            .to(Contact::Table, Contact::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_task_deal")
                            .from(Task::Table, Task::DealId)
                            .to(Deal::Table, Deal::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .check(target_check("task"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_task_status")
                    .table(Task::Table)
                    .col(Task::Status)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_task_priority")
                    .table(Task::Table)
                    .col(Task::Priority)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_task_due_at")
                    .table(Task::Table)
                    .col(Task::DueAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_task_company")
                    .table(Task::Table)
                    .col(Task::CompanyId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_task_contact")
                    .table(Task::Table)
                    .col(Task::ContactId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_task_deal")
                    .table(Task::Table)
                    .col(Task::DealId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(DealStageHistory::Table)
                    .if_not_exists()
                    .col(&mut uuid_pk(DealStageHistory::Id))
                    .col(ColumnDef::new(DealStageHistory::DealId).uuid().not_null())
                    .col(
                        ColumnDef::new(DealStageHistory::FromStage)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DealStageHistory::ToStage)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(&mut timestamp_with_default(DealStageHistory::ChangedAt))
                    .col(ColumnDef::new(DealStageHistory::Note).text())
                    .col(ColumnDef::new(DealStageHistory::ChangedBy).string_len(128))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_deal_stage_history_deal")
                            .from(DealStageHistory::Table, DealStageHistory::DealId)
                            .to(Deal::Table, Deal::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_deal_stage_history_deal")
                    .table(DealStageHistory::Table)
                    .col(DealStageHistory::DealId)
                    .col(DealStageHistory::ChangedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DealStageHistory::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Task::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Activity::Table).to_owned())
            .await?;
        Ok(())
    }
}

fn uuid_pk<C: Iden + 'static>(col: C) -> ColumnDef {
    let mut column = ColumnDef::new(col);
    column
        .uuid()
        .not_null()
        .primary_key()
        .default(Expr::cust("gen_random_uuid()"));
    column
}

fn timestamp_with_default<C: Iden + 'static>(col: C) -> ColumnDef {
    let mut column = ColumnDef::new(col);
    column
        .timestamp_with_time_zone()
        .not_null()
        .default(Expr::cust("now()"));
    column
}

fn target_check(table: &str) -> SimpleExpr {
    Expr::cust(&format!(
        "(((CASE WHEN {tbl}.company_id IS NOT NULL THEN 1 ELSE 0 END) + \
         (CASE WHEN {tbl}.contact_id IS NOT NULL THEN 1 ELSE 0 END) + \
         (CASE WHEN {tbl}.deal_id IS NOT NULL THEN 1 ELSE 0 END)) = 1)",
        tbl = table
    ))
}
