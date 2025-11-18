use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "task")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub title: String,
    pub notes_md: Option<String>,
    pub status: Status,
    pub priority: Priority,
    pub assignee: Option<String>,
    #[sea_orm(indexed)]
    pub assigned_user_id: Option<Uuid>,
    pub due_at: Option<DateTimeWithTimeZone>,
    pub completed_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(indexed)]
    pub company_id: Option<Uuid>,
    #[sea_orm(indexed)]
    pub contact_id: Option<Uuid>,
    #[sea_orm(indexed)]
    pub deal_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::company::Entity",
        from = "Column::CompanyId",
        to = "super::company::Column::Id",
        on_delete = "Cascade"
    )]
    Company,
    #[sea_orm(
        belongs_to = "super::contact::Entity",
        from = "Column::ContactId",
        to = "super::contact::Column::Id",
        on_delete = "Cascade"
    )]
    Contact,
    #[sea_orm(
        belongs_to = "super::deal::Entity",
        from = "Column::DealId",
        to = "super::deal::Column::Id",
        on_delete = "Cascade"
    )]
    Deal,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::AssignedUserId",
        to = "super::user::Column::Id",
        on_delete = "SetNull"
    )]
    AssignedUser,
}

impl Related<super::company::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Company.def()
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

#[derive(Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Eq, PartialEq)]
#[sea_orm(rs_type = "String", db_type = "String(Some(32))")]
pub enum Status {
    #[sea_orm(string_value = "OPEN")]
    Open,
    #[sea_orm(string_value = "DONE")]
    Done,
    #[sea_orm(string_value = "CANCELLED")]
    Cancelled,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Eq, PartialEq)]
#[sea_orm(rs_type = "String", db_type = "String(Some(32))")]
pub enum Priority {
    #[sea_orm(string_value = "LOW")]
    Low,
    #[sea_orm(string_value = "MEDIUM")]
    Medium,
    #[sea_orm(string_value = "HIGH")]
    High,
}

impl ActiveModelBehavior for ActiveModel {}
