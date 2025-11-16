use std::{collections::HashMap, sync::Arc};

use async_graphql::{
    dataloader::{DataLoader, Loader},
    ComplexObject, Context, EmptySubscription, Error, ErrorExtensions, InputValueError,
    InputValueResult, Object, ScalarType, Schema, SimpleObject, Value, ID,
};
use chrono::{DateTime, NaiveDate, Utc};
use entity::{company, contact, deal};
use sea_orm::sea_query::{extension::postgres::PgExpr, Expr};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};
use uuid::Uuid;

type Db = Arc<DatabaseConnection>;

#[derive(Clone)]
pub struct AppSchema(pub Schema<QueryRoot, MutationRoot, EmptySubscription>);

pub fn build_schema(db: Db) -> AppSchema {
    let loader = CompanyByIdLoader::new(db.clone());
    let schema = Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(db)
        .data(DataLoader::new(loader, tokio::spawn))
        .finish();
    AppSchema(schema)
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn health(&self) -> &str {
        "ok"
    }

    async fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    async fn crm(&self) -> CrmQuery {
        CrmQuery
    }
}

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    async fn crm(&self) -> CrmMutation {
        CrmMutation
    }
}

pub struct CrmQuery;

#[Object]
impl CrmQuery {
    async fn companies(
        &self,
        ctx: &Context<'_>,
        first: Option<i32>,
        offset: Option<u64>,
        q: Option<String>,
    ) -> async_graphql::Result<Vec<Company>> {
        let db = ctx.data::<Db>()?;
        let limit = clamp_limit(first);
        let off = offset.unwrap_or(0);
        let mut query = company::Entity::find()
            .order_by_asc(company::Column::CreatedAt)
            .limit(limit)
            .offset(off);

        if let Some(filter) = normalized_filter(q) {
            query = query.filter(Expr::col(company::Column::Name).ilike(format!("%{}%", filter)));
        }

        let rows = query.all(db.as_ref()).await?;
        Ok(rows.into_iter().map(Company::from_model).collect())
    }

    async fn contacts(
        &self,
        ctx: &Context<'_>,
        first: Option<i32>,
        offset: Option<u64>,
        company_id: Option<ID>,
        q: Option<String>,
    ) -> async_graphql::Result<Vec<Contact>> {
        let db = ctx.data::<Db>()?;
        let limit = clamp_limit(first);
        let off = offset.unwrap_or(0);
        let mut query = contact::Entity::find()
            .order_by_asc(contact::Column::CreatedAt)
            .limit(limit)
            .offset(off);

        let mut condition = Condition::all();
        if let Some(cid) = parse_optional_id(company_id.as_ref())? {
            condition = condition.add(contact::Column::CompanyId.eq(cid));
        }
        if let Some(filter) = normalized_filter(q) {
            condition =
                condition.add(Expr::col(contact::Column::Email).ilike(format!("%{}%", filter)));
        }
        if !condition.is_empty() {
            query = query.filter(condition);
        }

        let rows = query.all(db.as_ref()).await?;
        Ok(rows.into_iter().map(Contact::from_model).collect())
    }

    async fn deals(
        &self,
        ctx: &Context<'_>,
        first: Option<i32>,
        offset: Option<u64>,
        company_id: Option<ID>,
        stage: Option<DealStage>,
        q: Option<String>,
    ) -> async_graphql::Result<Vec<Deal>> {
        let db = ctx.data::<Db>()?;
        let limit = clamp_limit(first);
        let off = offset.unwrap_or(0);
        let mut query = deal::Entity::find()
            .order_by_asc(deal::Column::CreatedAt)
            .limit(limit)
            .offset(off);

        let mut condition = Condition::all();
        if let Some(cid) = parse_optional_id(company_id.as_ref())? {
            condition = condition.add(deal::Column::CompanyId.eq(cid));
        }
        if let Some(stage_filter) = stage {
            let value: deal::Stage = stage_filter.into();
            condition = condition.add(deal::Column::Stage.eq(value));
        }
        if let Some(filter) = normalized_filter(q) {
            condition =
                condition.add(Expr::col(deal::Column::Title).ilike(format!("%{}%", filter)));
        }
        if !condition.is_empty() {
            query = query.filter(condition);
        }

        let rows = query.all(db.as_ref()).await?;
        Ok(rows.into_iter().map(Deal::from_model).collect())
    }
}

pub struct CrmMutation;

#[Object]
impl CrmMutation {
    async fn create_company(
        &self,
        ctx: &Context<'_>,
        input: CompanyInput,
    ) -> async_graphql::Result<Company> {
        let db = ctx.data::<Db>()?;
        let CompanyInput {
            name,
            website,
            phone,
        } = input;
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(gql_error("VALIDATION", "name is required"));
        }
        let am = company::ActiveModel {
            id: Set(Uuid::new_v4()),
            name: Set(name),
            website: Set(website
                .map(|v| v.trim().to_string())
                .filter(|s| !s.is_empty())),
            phone: Set(phone
                .map(|v| v.trim().to_string())
                .filter(|s| !s.is_empty())),
            ..Default::default()
        };
        let res = am.insert(db.as_ref()).await?;
        Ok(Company::from_model(res))
    }

    async fn create_contact(
        &self,
        ctx: &Context<'_>,
        input: ContactInput,
    ) -> async_graphql::Result<Contact> {
        let db = ctx.data::<Db>()?;
        let email = input.email.trim().to_lowercase();
        if !is_valid_email(&email) {
            return Err(gql_error("VALIDATION", "invalid email address"));
        }

        if contact::Entity::find()
            .filter(contact::Column::Email.eq(email.clone()))
            .one(db.as_ref())
            .await?
            .is_some()
        {
            return Err(gql_error("CONFLICT", "email already exists"));
        }

        let company_id = parse_optional_id(input.company_id.as_ref())?;
        if let Some(cid) = company_id {
            ensure_company_exists(db.as_ref(), cid).await?;
        }

        let am = contact::ActiveModel {
            id: Set(Uuid::new_v4()),
            email: Set(email),
            first_name: Set(input
                .first_name
                .map(|v| v.trim().to_string())
                .filter(|s| !s.is_empty())),
            last_name: Set(input
                .last_name
                .map(|v| v.trim().to_string())
                .filter(|s| !s.is_empty())),
            phone: Set(input
                .phone
                .map(|v| v.trim().to_string())
                .filter(|s| !s.is_empty())),
            company_id: Set(company_id),
            ..Default::default()
        };
        let res = am.insert(db.as_ref()).await?;
        Ok(Contact::from_model(res))
    }

    async fn create_deal(
        &self,
        ctx: &Context<'_>,
        input: DealInput,
    ) -> async_graphql::Result<Deal> {
        let db = ctx.data::<Db>()?;
        let DealInput {
            title,
            amount_cents,
            currency,
            stage,
            close_date,
            company_id,
        } = input;
        let company_id = parse_id(&company_id)?;
        ensure_company_exists(db.as_ref(), company_id).await?;

        let amount = amount_cents.map(i64::from);
        let stage = stage.unwrap_or(DealStage::New);
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err(gql_error("VALIDATION", "title is required"));
        }

        let am = deal::ActiveModel {
            id: Set(Uuid::new_v4()),
            title: Set(title),
            amount_cents: Set(amount),
            currency: Set(currency
                .map(|v| v.trim().to_string())
                .filter(|s| !s.is_empty())),
            stage: Set(stage.into()),
            close_date: Set(close_date),
            company_id: Set(company_id),
            ..Default::default()
        };

        let res = am.insert(db.as_ref()).await?;
        Ok(Deal::from_model(res))
    }

    async fn move_deal_stage(
        &self,
        ctx: &Context<'_>,
        id: ID,
        stage: DealStage,
    ) -> async_graphql::Result<Deal> {
        let db = ctx.data::<Db>()?;
        let uid = parse_id(&id)?;
        let model = deal::Entity::find_by_id(uid).one(db.as_ref()).await?;
        let current = match model {
            Some(m) => m,
            None => return Err(gql_error("NOT_FOUND", "deal not found")),
        };
        let mut am: deal::ActiveModel = current.into();
        am.stage = Set(stage.into());
        am.updated_at = Set(Utc::now().into());
        let updated = am.update(db.as_ref()).await?;
        Ok(Deal::from_model(updated))
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
    fn from_model(m: company::Model) -> Self {
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

#[derive(SimpleObject)]
#[graphql(complex)]
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

#[ComplexObject]
impl Contact {
    async fn company(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<Company>> {
        let loader = ctx.data::<DataLoader<CompanyByIdLoader>>()?;
        if let Some(id) = &self.company_id {
            let uuid = parse_id(id)?;
            let result = loader.load_one(uuid).await?;
            Ok(result.map(Company::from_model))
        } else {
            Ok(None)
        }
    }
}

impl Contact {
    fn from_model(m: contact::Model) -> Self {
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
pub struct Deal {
    pub id: ID,
    pub title: String,
    pub amount_cents: Option<BigInt>,
    pub currency: Option<String>,
    pub stage: DealStage,
    pub close_date: Option<NaiveDate>,
    pub company_id: ID,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Deal {
    fn from_model(m: deal::Model) -> Self {
        Self {
            id: ID(m.id.to_string()),
            title: m.title,
            amount_cents: m.amount_cents.map(BigInt::from),
            currency: m.currency,
            stage: m.stage.into(),
            close_date: m.close_date,
            company_id: ID(m.company_id.to_string()),
            created_at: m.created_at.into(),
            updated_at: m.updated_at.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BigInt(pub i64);

#[async_graphql::Scalar(name = "BigInt")]
impl ScalarType for BigInt {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::Number(num) => num
                .as_i64()
                .map(BigInt)
                .ok_or_else(|| InputValueError::custom("invalid BigInt value")),
            Value::String(s) => s
                .parse::<i64>()
                .map(BigInt)
                .map_err(|_| InputValueError::custom("invalid BigInt value")),
            other => Err(InputValueError::expected_type(other)),
        }
    }

    fn to_value(&self) -> Value {
        Value::from(self.0)
    }
}

impl From<i64> for BigInt {
    fn from(value: i64) -> Self {
        BigInt(value)
    }
}

impl From<BigInt> for i64 {
    fn from(value: BigInt) -> Self {
        value.0
    }
}

#[derive(async_graphql::InputObject)]
pub struct CompanyInput {
    pub name: String,
    pub website: Option<String>,
    pub phone: Option<String>,
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
pub struct DealInput {
    pub title: String,
    pub amount_cents: Option<BigInt>,
    pub currency: Option<String>,
    pub stage: Option<DealStage>,
    pub close_date: Option<NaiveDate>,
    pub company_id: ID,
}

#[derive(Clone, Copy, Eq, PartialEq, async_graphql::Enum)]
pub enum DealStage {
    New,
    Qualify,
    Proposal,
    Negotiate,
    Won,
    Lost,
}

impl From<deal::Stage> for DealStage {
    fn from(stage: deal::Stage) -> Self {
        match stage {
            deal::Stage::New => DealStage::New,
            deal::Stage::Qualify => DealStage::Qualify,
            deal::Stage::Proposal => DealStage::Proposal,
            deal::Stage::Negotiate => DealStage::Negotiate,
            deal::Stage::Won => DealStage::Won,
            deal::Stage::Lost => DealStage::Lost,
        }
    }
}

impl From<DealStage> for deal::Stage {
    fn from(stage: DealStage) -> Self {
        match stage {
            DealStage::New => deal::Stage::New,
            DealStage::Qualify => deal::Stage::Qualify,
            DealStage::Proposal => deal::Stage::Proposal,
            DealStage::Negotiate => deal::Stage::Negotiate,
            DealStage::Won => deal::Stage::Won,
            DealStage::Lost => deal::Stage::Lost,
        }
    }
}

#[derive(Clone)]
pub struct CompanyByIdLoader {
    db: Db,
}

impl CompanyByIdLoader {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl Loader<Uuid> for CompanyByIdLoader {
    type Value = company::Model;
    type Error = Arc<sea_orm::DbErr>;

    fn load(
        &self,
        keys: &[Uuid],
    ) -> impl std::future::Future<Output = Result<HashMap<Uuid, Self::Value>, Self::Error>> + Send
    {
        let db = Arc::clone(&self.db);
        let lookup_keys: Vec<Uuid> = keys.to_vec();
        async move {
            if lookup_keys.is_empty() {
                return Ok(HashMap::new());
            }
            let rows = company::Entity::find()
                .filter(company::Column::Id.is_in(lookup_keys.clone()))
                .all(db.as_ref())
                .await
                .map_err(Arc::new)?;
            Ok(rows.into_iter().map(|row| (row.id, row)).collect())
        }
    }
}

fn clamp_limit(first: Option<i32>) -> u64 {
    let value = first.unwrap_or(25);
    value.clamp(1, 100) as u64
}

fn normalized_filter(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_id(id: &ID) -> async_graphql::Result<Uuid> {
    Uuid::parse_str(id.as_str()).map_err(|_| gql_error("VALIDATION", "invalid id"))
}

fn parse_optional_id(id: Option<&ID>) -> async_graphql::Result<Option<Uuid>> {
    match id {
        Some(value) => Ok(Some(parse_id(value)?)),
        None => Ok(None),
    }
}

async fn ensure_company_exists(
    db: &DatabaseConnection,
    id: Uuid,
) -> async_graphql::Result<company::Model> {
    match company::Entity::find_by_id(id).one(db).await? {
        Some(model) => Ok(model),
        None => Err(gql_error("VALIDATION", "company not found")),
    }
}

fn is_valid_email(email: &str) -> bool {
    let trimmed = email.trim();
    trimmed.contains('@') && trimmed.contains('.')
}

fn gql_error(code: &str, message: impl Into<String>) -> Error {
    Error::new(message).extend_with(|_, e| e.set("code", code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_graphql::{Request, Value as GqlValue};
    use chrono::Utc;
    use sea_orm::{DatabaseBackend, MockDatabase};
    use serde_json::json;
    use std::sync::Arc;

    fn test_schema(db: DatabaseConnection) -> Schema<QueryRoot, MutationRoot, EmptySubscription> {
        build_schema(Arc::new(db)).0
    }

    fn sample_company(name: &str) -> company::Model {
        let now = Utc::now().into();
        company::Model {
            id: Uuid::new_v4(),
            name: name.to_string(),
            website: Some("https://example.com".into()),
            phone: Some("+1-555-0100".into()),
            created_at: now,
            updated_at: now,
        }
    }

    fn sample_contact(company_id: Option<Uuid>, email: &str) -> contact::Model {
        let now = Utc::now().into();
        contact::Model {
            id: Uuid::new_v4(),
            email: email.into(),
            first_name: Some("Ada".into()),
            last_name: Some("Lovelace".into()),
            phone: Some("+1-555-0110".into()),
            company_id,
            created_at: now,
            updated_at: now,
        }
    }

    fn sample_deal(company_id: Uuid, stage: deal::Stage) -> deal::Model {
        let now = Utc::now().into();
        deal::Model {
            id: Uuid::new_v4(),
            title: "ACME Pilot".into(),
            amount_cents: Some(120_000),
            currency: Some("USD".into()),
            stage,
            close_date: None,
            company_id,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn clamp_limit_bounds() {
        assert_eq!(clamp_limit(None), 25);
        assert_eq!(clamp_limit(Some(-5)), 1);
        assert_eq!(clamp_limit(Some(150)), 100);
        assert_eq!(clamp_limit(Some(10)), 10);
    }

    #[test]
    fn normalized_filter_trims() {
        assert_eq!(
            normalized_filter(Some("  hello  ".into())),
            Some("hello".into())
        );
        assert_eq!(normalized_filter(Some("   ".into())), None);
        assert_eq!(normalized_filter(None), None);
    }

    #[test]
    fn parse_id_success_and_failure() {
        let id = Uuid::new_v4();
        let parsed = parse_id(&ID::from(id.to_string())).unwrap();
        assert_eq!(parsed, id);

        let err = parse_id(&ID::from("not-a-uuid"));
        assert!(err.is_err());
    }

    #[test]
    fn email_validation() {
        assert!(is_valid_email("user@example.com"));
        assert!(!is_valid_email("invalid-email"));
    }

    #[test]
    fn big_int_scalar_roundtrip() {
        let scalar = BigInt(99);
        let value = scalar.to_value();
        assert_eq!(value, GqlValue::from(99i64));
        let parsed = BigInt::parse(value).unwrap();
        assert_eq!(parsed.0, 99);
    }

    #[tokio::test]
    async fn companies_query_returns_rows() {
        let company = sample_company("ACME");
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results(vec![vec![company.clone()]])
            .into_connection();
        let schema = test_schema(db);
        let resp = schema
            .execute(Request::new(
                "{ crm { companies(first: 5) { id name website } } }",
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let data = resp.data.into_json().unwrap();
        assert_eq!(
            data,
            json!({
                "crm": {
                    "companies": [{
                        "id": company.id.to_string(),
                        "name": company.name,
                        "website": company.website
                    }]
                }
            })
        );
    }

    #[tokio::test]
    async fn contacts_query_includes_company_via_loader() {
        let company = sample_company("ACME");
        let contact = sample_contact(Some(company.id), "ada@acme.test");
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results(vec![vec![contact.clone()]])
            .append_query_results(vec![vec![company.clone()]])
            .into_connection();
        let schema = test_schema(db);
        let resp = schema
            .execute(Request::new(
                "{ crm { contacts(first: 5) { id email company { id name } } } }",
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let data = resp.data.into_json().unwrap();
        assert_eq!(data["crm"]["contacts"][0]["company"]["name"], company.name);
    }

    #[tokio::test]
    async fn create_company_mutation_inserts_row() {
        let created = sample_company("NewCo");
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results(vec![vec![created.clone()]])
            .into_connection();
        let schema = test_schema(db);
        let resp = schema
            .execute(Request::new(
                "mutation { crm { createCompany(input: { name: \"NewCo\" }) { id name } } }",
            ))
            .await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let data = resp.data.into_json().unwrap();
        assert_eq!(data["crm"]["createCompany"]["name"], created.name);
    }

    #[tokio::test]
    async fn create_contact_mutation_validates_and_inserts() {
        let company = sample_company("ACME");
        let contact = sample_contact(Some(company.id), "ada@acme.test");
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results(vec![Vec::<contact::Model>::new()]) // uniqueness check
            .append_query_results(vec![vec![company.clone()]]) // ensure company exists
            .append_query_results(vec![vec![contact.clone()]]) // insert returning
            .into_connection();
        let schema = test_schema(db);
        let mutation = format!(
            "mutation {{
                crm {{
                    createContact(input: {{
                        email: \"{}\",
                        companyId: \"{}\"
                    }}) {{
                        id
                        email
                        companyId
                    }}
                }}
            }}",
            contact.email, company.id
        );
        let resp = schema.execute(Request::new(mutation)).await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let data = resp.data.into_json().unwrap();
        assert_eq!(
            data["crm"]["createContact"]["companyId"],
            company.id.to_string()
        );
    }

    #[tokio::test]
    async fn move_deal_stage_updates_stage() {
        let company = sample_company("ACME");
        let deal_model = sample_deal(company.id, deal::Stage::New);
        let updated = deal::Model {
            stage: deal::Stage::Won,
            updated_at: Utc::now().into(),
            ..deal_model.clone()
        };
        let db = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results(vec![vec![deal_model.clone()]])
            .append_query_results(vec![vec![updated.clone()]])
            .into_connection();
        let schema = test_schema(db);
        let mutation = format!(
            "mutation {{
                crm {{
                    moveDealStage(id: \"{}\", stage: WON) {{
                        id
                        stage
                    }}
                }}
            }}",
            deal_model.id
        );
        let resp = schema.execute(Request::new(mutation)).await;
        assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
        let data = resp.data.into_json().unwrap();
        assert_eq!(data["crm"]["moveDealStage"]["stage"], "WON");
    }
}
