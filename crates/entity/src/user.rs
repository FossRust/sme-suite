use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "user")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub email: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub is_active: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Identity,
    Role,
    Secret,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Relation::Identity => Entity::has_many(super::user_identity::Entity).into(),
            Relation::Role => Entity::has_many(super::user_role::Entity).into(),
            Relation::Secret => Entity::has_one(super::user_secret::Entity).into(),
        }
    }
}

impl ActiveModelBehavior for ActiveModel {}
