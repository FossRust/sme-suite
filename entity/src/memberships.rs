use crate::{orgs, users};
use sea_orm::prelude::{DateTimeWithTimeZone, *};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "memberships")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub org_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: Uuid,
    pub roles: Vec<String>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "orgs::Entity",
        from = "Column::OrgId",
        to = "orgs::Column::Id"
    )]
    Org,
    #[sea_orm(
        belongs_to = "users::Entity",
        from = "Column::UserId",
        to = "users::Column::Id"
    )]
    User,
}

impl Related<orgs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Org.def()
    }
}

impl Related<users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
