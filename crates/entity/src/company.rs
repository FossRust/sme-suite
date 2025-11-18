use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "company")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    #[sea_orm(indexed)]
    pub name: String,
    pub website: Option<String>,
    pub phone: Option<String>,
    #[sea_orm(indexed)]
    pub assigned_user_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Contact,
    Deal,
    AssignedUser,
    CreatedByUser,
    UpdatedByUser,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Contact => Entity::has_many(super::contact::Entity).into(),
            Self::Deal => Entity::has_many(super::deal::Entity).into(),
            Self::AssignedUser => Entity::belongs_to(super::user::Entity)
                .from(Column::AssignedUserId)
                .to(super::user::Column::Id)
                .into(),
            Self::CreatedByUser => Entity::belongs_to(super::user::Entity)
                .from(Column::CreatedBy)
                .to(super::user::Column::Id)
                .into(),
            Self::UpdatedByUser => Entity::belongs_to(super::user::Entity)
                .from(Column::UpdatedBy)
                .to(super::user::Column::Id)
                .into(),
        }
    }
}

impl Related<super::contact::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Contact.def()
    }
}

impl Related<super::deal::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Deal.def()
    }
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AssignedUser.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
