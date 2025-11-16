use sea_orm_migration::prelude::*;
use sea_query::SimpleExpr;

#[derive(DeriveIden)]
enum Activity {
    Table,
    Id,
    Kind,
    Direction,
    Subject,
    Body,
    At,
    CompanyId,
    ContactId,
    DealId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
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

#[derive(DeriveIden)]
enum DealStageHistory {
    Table,
    Id,
    DealId,
    FromStage,
    ToStage,
    ChangedAt,
    Note,
}

#[derive(DeriveIden)]
enum Company {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Contact {
    Table,
    Id,
}

#[derive(DeriveIden)]
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
                    .col(uuid_pk(Activity::Id))
                    .col(ColumnDef::new(Activity::Kind).string_len(32).not_null())
                    .col(ColumnDef::new(Activity::Direction).string_len(32))
                    .col(ColumnDef::new(Activity::Subject).string_len(512))
                    .col(ColumnDef::new(Activity::Body).text())
                    .col(
                        ColumnDef::new(Activity::At)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::cust("now()")),
                    )
                    .col(ColumnDef::new(Activity::CompanyId).uuid())
                    .col(ColumnDef::new(Activity::ContactId).uuid())
                    .col(ColumnDef::new(Activity::DealId).uuid())
                    .col(timestamp_with_default(Activity::CreatedAt))
                    .col(timestamp_with_default(Activity::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_activity_company")
                            .from(Activity::Table, Activity::CompanyId)
                            .to(Company::Table, Company::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_activity_contact")
                            .from(Activity::Table, Activity::ContactId)
                            .to(Contact::Table, Contact::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_activity_deal")
                            .from(Activity::Table, Activity::DealId)
                            .to(Deal::Table, Deal::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .check(target_check("activity"))
                    .to_owned(),
            )
            .await?;

        create_index(manager, "idx_activity_kind", Activity::Table, &[Activity::Kind]).await?;
        create_index(manager, "idx_activity_at", Activity::Table, &[Activity::At]).await?;
        create_index(manager, "idx_activity_company", Activity::Table, &[Activity::CompanyId]).await?;
        create_index(manager, "idx_activity_contact", Activity::Table, &[Activity::ContactId]).await?;
        create_index(manager, "idx_activity_deal", Activity::Table, &[Activity::DealId]).await?;

        manager
            .create_table(
                Table::create()
                    .table(Task::Table)
                    .if_not_exists()
                    .col(uuid_pk(Task::Id))
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
                    .col(timestamp_with_default(Task::CreatedAt))
                    .col(timestamp_with_default(Task::UpdatedAt))
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

        create_index(manager, "idx_task_status", Task::Table, &[Task::Status]).await?;
        create_index(manager, "idx_task_priority", Task::Table, &[Task::Priority]).await?;
        create_index(manager, "idx_task_due_at", Task::Table, &[Task::DueAt]).await?;
        create_index(manager, "idx_task_company", Task::Table, &[Task::CompanyId]).await?;
        create_index(manager, "idx_task_contact", Task::Table, &[Task::ContactId]).await?;
        create_index(manager, "idx_task_deal", Task::Table, &[Task::DealId]).await?;

        manager
            .create_table(
                Table::create()
                    .table(DealStageHistory::Table)
                    .if_not_exists()
                    .col(uuid_pk(DealStageHistory::Id))
                    .col(ColumnDef::new(DealStageHistory::DealId).uuid().not_null())
                    .col(ColumnDef::new(DealStageHistory::FromStage).string_len(32))
                    .col(ColumnDef::new(DealStageHistory::ToStage).string_len(32).not_null())
                    .col(timestamp_with_default(DealStageHistory::ChangedAt))
                    .col(ColumnDef::new(DealStageHistory::Note).text())
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

        create_index(
            manager,
            "idx_deal_stage_history_deal",
            DealStageHistory::Table,
            &[DealStageHistory::DealId, DealStageHistory::ChangedAt],
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

fn uuid_pk<C: Iden>(col: C) -> ColumnDef {
    ColumnDef::new(col)
        .uuid()
        .not_null()
        .primary_key()
        .default(Expr::cust("gen_random_uuid()"))
}

fn timestamp_with_default<C: Iden>(col: C) -> ColumnDef {
    ColumnDef::new(col)
        .timestamp_with_time_zone()
        .not_null()
        .default(Expr::cust("now()"))
}

fn target_check(table: &str) -> SimpleExpr {
    Expr::cust(&format!(
        "(((CASE WHEN {tbl}.company_id IS NOT NULL THEN 1 ELSE 0 END) + \
         (CASE WHEN {tbl}.contact_id IS NOT NULL THEN 1 ELSE 0 END) + \
         (CASE WHEN {tbl}.deal_id IS NOT NULL THEN 1 ELSE 0 END)) = 1)",
        tbl = table
    ))
}

async fn create_index(
    manager: &SchemaManager,
    name: &str,
    table: impl Iden + Copy,
    cols: &[impl Iden + Copy],
) -> Result<(), DbErr> {
    let mut stmt = Index::create().name(name).table(table);
    for col in cols {
        stmt = stmt.col(*col);
    }
    manager.create_index(stmt.to_owned()).await
}
