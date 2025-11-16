use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "deal")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub title: String,
    pub amount_cents: Option<i64>,
    pub currency: Option<String>,
    pub stage: Stage,
    pub close_date: Option<Date>,
    #[sea_orm(indexed)]
    pub company_id: Uuid,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::company::Entity",
        from = "Column::CompanyId",
        to = "super::company::Column::Id"
    )]
    Company,
}

impl Related<super::company::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Company.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveActiveEnum, Eq, PartialEq)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "deal_stage")]
pub enum Stage {
    #[sea_orm(string_value = "NEW")]
    New,
    #[sea_orm(string_value = "QUALIFY")]
    Qualify,
    #[sea_orm(string_value = "PROPOSAL")]
    Proposal,
    #[sea_orm(string_value = "NEGOTIATE")]
    Negotiate,
    #[sea_orm(string_value = "WON")]
    Won,
    #[sea_orm(string_value = "LOST")]
    Lost,
}

impl ActiveModelBehavior for ActiveModel {}
