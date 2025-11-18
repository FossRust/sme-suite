use sea_orm_migration::prelude::*;

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
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(StageMeta::DisplayName).string().not_null())
                    .col(
                        ColumnDef::new(StageMeta::SortOrder)
                            .small_integer()
                            .not_null(),
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
                            .default(Expr::value(false)),
                    )
                    .col(
                        ColumnDef::new(StageMeta::IsLost)
                            .boolean()
                            .not_null()
                            .default(Expr::value(false)),
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
                    .unique()
                    .to_owned(),
            )
            .await?;

        let default_rows = [
            ("NEW", "New", 10_i16, 10_i16, false, false),
            ("QUALIFY", "Qualify", 20_i16, 25_i16, false, false),
            ("PROPOSAL", "Proposal", 30_i16, 50_i16, false, false),
            ("NEGOTIATE", "Negotiate", 40_i16, 70_i16, false, false),
            ("WON", "Won", 90_i16, 100_i16, true, false),
            ("LOST", "Lost", 95_i16, 0_i16, false, true),
        ];
        for (key, display_name, sort_order, probability, is_won, is_lost) in default_rows {
            let insert = Query::insert()
                .into_table(StageMeta::Table)
                .columns(vec![
                    StageMeta::Key,
                    StageMeta::DisplayName,
                    StageMeta::SortOrder,
                    StageMeta::Probability,
                    StageMeta::IsWon,
                    StageMeta::IsLost,
                ])
                .values_panic(vec![
                    key.into(),
                    display_name.into(),
                    sort_order.into(),
                    probability.into(),
                    is_won.into(),
                    is_lost.into(),
                ])
                .on_conflict(OnConflict::column(StageMeta::Key).do_nothing().to_owned())
                .to_owned();
            manager.exec_stmt(insert).await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(StageMeta::Table).to_owned())
            .await
    }
}

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
