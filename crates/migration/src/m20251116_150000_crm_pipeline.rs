use sea_orm_migration::prelude::*;
use sea_query::{OnConflict, Query};

#[derive(DeriveIden)]
enum StageMeta {
    Table,
    Key,
    DisplayName,
    SortOrder,
    Probability,
    IsWon,
    IsLost,
}

const DEFAULT_STAGE_META: [(&str, &str, i16, i16, bool, bool); 6] = [
    ("NEW", "New", 10, 10, false, false),
    ("QUALIFY", "Qualify", 20, 25, false, false),
    ("PROPOSAL", "Proposal", 30, 50, false, false),
    ("NEGOTIATE", "Negotiate", 40, 70, false, false),
    ("WON", "Won", 90, 100, true, false),
    ("LOST", "Lost", 95, 0, false, true),
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(StageMeta::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(StageMeta::Key)
                            .string_len(32)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(StageMeta::DisplayName)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(StageMeta::SortOrder)
                            .small_integer()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(StageMeta::Probability)
                            .small_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(StageMeta::IsWon)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(StageMeta::IsLost)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_stage_meta_order")
                    .table(StageMeta::Table)
                    .col(StageMeta::SortOrder)
                    .to_owned(),
            )
            .await?;

        for (key, display_name, sort_order, probability, is_won, is_lost) in DEFAULT_STAGE_META {
            let stmt = Query::insert()
                .into_table(StageMeta::Table)
                .columns([
                    StageMeta::Key,
                    StageMeta::DisplayName,
                    StageMeta::SortOrder,
                    StageMeta::Probability,
                    StageMeta::IsWon,
                    StageMeta::IsLost,
                ])
                .values_panic([
                    key.into(),
                    display_name.into(),
                    sort_order.into(),
                    probability.into(),
                    is_won.into(),
                    is_lost.into(),
                ])
                .on_conflict(OnConflict::column(StageMeta::Key).do_nothing().to_owned())
                .to_owned();
            manager.exec_stmt(stmt).await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(StageMeta::Table).to_owned())
            .await
    }
}
