use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "user_role")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub user_id: Uuid,
    #[sea_orm(primary_key)]
    pub role: Role,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::app_user::Entity",
        from = "Column::UserId",
        to = "super::app_user::Column::Id",
        on_delete = "Cascade"
    )]
    AppUser,
}

impl Related<super::app_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AppUser.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Eq, PartialEq)]
#[sea_orm(rs_type = "String", db_type = "String(Some(16))")]
pub enum Role {
    #[sea_orm(string_value = "OWNER")]
    Owner,
    #[sea_orm(string_value = "ADMIN")]
    Admin,
    #[sea_orm(string_value = "SALES")]
    Sales,
    #[sea_orm(string_value = "VIEWER")]
    Viewer,
}

impl ActiveModelBehavior for ActiveModel {}
