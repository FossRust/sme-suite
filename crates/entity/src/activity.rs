use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "activity")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub kind: Kind,
    pub direction: Option<Direction>,
    pub subject: Option<String>,
    pub body: Option<String>,
    pub at: DateTimeWithTimeZone,
    #[sea_orm(indexed)]
    pub company_id: Option<Uuid>,
    #[sea_orm(indexed)]
    pub contact_id: Option<Uuid>,
    #[sea_orm(indexed)]
    pub deal_id: Option<Uuid>,
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
}

impl Related<super::company::Entity> for Entity {
    fn to() -> RelationDef { Relation::Company.def() }
}

impl Related<super::contact::Entity> for Entity {
    fn to() -> RelationDef { Relation::Contact.def() }
}

impl Related<super::deal::Entity> for Entity {
    fn to() -> RelationDef { Relation::Deal.def() }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Eq, PartialEq)]
#[sea_orm(rs_type = "String", db_type = "String")]
pub enum Kind {
    #[sea_orm(string_value = "NOTE")]
    Note,
    #[sea_orm(string_value = "EMAIL")]
    Email,
    #[sea_orm(string_value = "CALL")]
    Call,
    #[sea_orm(string_value = "MEETING")]
    Meeting,
    #[sea_orm(string_value = "STAGE_CHANGE")]
    StageChange,
    #[sea_orm(string_value = "OTHER")]
    Other,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Eq, PartialEq)]
#[sea_orm(rs_type = "String", db_type = "String")]
pub enum Direction {
    #[sea_orm(string_value = "INBOUND")]
    Inbound,
    #[sea_orm(string_value = "OUTBOUND")]
    Outbound,
}

impl ActiveModelBehavior for ActiveModel {}
