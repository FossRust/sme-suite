use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "deal_stage_history")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    #[sea_orm(indexed)]
    pub deal_id: Uuid,
    pub from_stage: Option<super::deal::Stage>,
    pub to_stage: super::deal::Stage,
    pub changed_at: DateTimeWithTimeZone,
    pub note: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::deal::Entity",
        from = "Column::DealId",
        to = "super::deal::Column::Id",
        on_delete = "Cascade"
    )]
    Deal,
}

impl Related<super::deal::Entity> for Entity {
    fn to() -> RelationDef { Relation::Deal.def() }
}

impl ActiveModelBehavior for ActiveModel {}
