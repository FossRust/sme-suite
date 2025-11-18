use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "activity")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub kind: Kind,
    pub subject: Option<String>,
    pub body_md: Option<String>,
    pub meta_json: Json,
    pub created_at: DateTimeWithTimeZone,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::CreatedBy",
        to = "super::user::Column::Id",
        on_delete = "SetNull"
    )]
    CreatedByUser,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UpdatedBy",
        to = "super::user::Column::Id",
        on_delete = "SetNull"
    )]
    UpdatedByUser,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Eq, PartialEq)]
#[sea_orm(rs_type = "String", db_type = "String(Some(32))")]
pub enum Kind {
    #[sea_orm(string_value = "stage_change")]
    StageChange,
}

impl ActiveModelBehavior for ActiveModel {}
