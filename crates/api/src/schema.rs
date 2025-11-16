use async_graphql::{Context, EmptySubscription, Object, Schema, Result as GqlResult, SimpleObject, ID};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, QueryOrder, QuerySelect, Set};
#[allow(unused_imports)]
use entity::{prelude::*, contact, company};
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct AppSchema(pub Schema<QueryRoot, MutationRoot, EmptySubscription>);

pub fn build_schema(db: DatabaseConnection) -> AppSchema {
    let schema = Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(db)
        .finish();
    AppSchema(schema)
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn health(&self) -> &str { "ok" }

    async fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }

    async fn contacts(&self, ctx: &Context<'_>, first: Option<i32>) -> GqlResult<Vec<Contact>> {
        let db = ctx.data::<DatabaseConnection>()?;
        let limit = first.unwrap_or(20) as u64;
        let rows = contact::Entity::find().order_by_asc(contact::Column::CreatedAt).limit(limit).all(db).await?;
        Ok(rows.into_iter().map(Contact::from_model).collect())
    }

    async fn contact(&self, ctx: &Context<'_>, id: ID) -> GqlResult<Option<Contact>> {
        let db = ctx.data::<DatabaseConnection>()?;
        let uid = Uuid::parse_str(id.as_str())?;
        let row = contact::Entity::find_by_id(uid).one(db).await?;
        Ok(row.map(Contact::from_model))
    }
}

pub struct MutationRoot;

#[derive(SimpleObject)]
pub struct Contact {
    pub id: ID,
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub company_id: Option<ID>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Contact {
    fn from_model(m: entity::contact::Model) -> Self {
        Self {
            id: ID(m.id.to_string()),
            email: m.email,
            first_name: m.first_name,
            last_name: m.last_name,
            phone: m.phone,
            company_id: m.company_id.map(|c| ID(c.to_string())),
            created_at: m.created_at.into(),
            updated_at: m.updated_at.into(),
        }
    }
}

#[derive(SimpleObject)]
pub struct Company {
    pub id: ID,
    pub name: String,
    pub website: Option<String>,
    pub phone: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Company {
    fn from_model(m: entity::company::Model) -> Self {
        Self {
            id: ID(m.id.to_string()),
            name: m.name,
            website: m.website,
            phone: m.phone,
            created_at: m.created_at.into(),
            updated_at: m.updated_at.into(),
        }
    }
}

#[derive(async_graphql::InputObject)]
pub struct ContactInput {
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub company_id: Option<ID>,
}

#[derive(async_graphql::InputObject)]
pub struct CompanyInput {
    pub name: String,
    pub website: Option<String>,
    pub phone: Option<String>,
}

#[Object]
impl MutationRoot {
    async fn create_company(&self, ctx: &Context<'_>, input: CompanyInput) -> GqlResult<Company> {
        let db = ctx.data::<DatabaseConnection>()?;
        let am = company::ActiveModel {
            id: Set(Uuid::new_v4()),
            name: Set(input.name),
            website: Set(input.website),
            phone: Set(input.phone),
            ..Default::default()
        };
        let res = am.insert(db).await?;
        Ok(Company::from_model(res))
    }

    async fn create_contact(&self, ctx: &Context<'_>, input: ContactInput) -> GqlResult<Contact> {
        let db = ctx.data::<DatabaseConnection>()?;
        let company_id = input.company_id.as_ref().map(|id| Uuid::parse_str(id.as_str())).transpose()?;
        let am = contact::ActiveModel {
            id: Set(Uuid::new_v4()),
            email: Set(input.email),
            first_name: Set(input.first_name),
            last_name: Set(input.last_name),
            phone: Set(input.phone),
            company_id: Set(company_id),
            ..Default::default()
        };
        let res = am.insert(db).await?;
        Ok(Contact::from_model(res))
    }
}
