use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "stage_meta")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub key: String,
    pub display_name: String,
    pub sort_order: i16,
    pub probability: i16,
    pub is_won: bool,
    pub is_lost: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
