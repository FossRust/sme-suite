mod me;

use anyhow::anyhow;
use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema, SimpleObject};
use me::MePayload;
use platform_api::{ApiError, ApiResult};
use platform_db::DbPool;
use serde::Serialize;
use uuid::Uuid;

pub type SchemaType = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

#[derive(Clone)]
pub struct GraphqlData {
    pub pool: DbPool,
    pub default_org_id: Uuid,
    pub default_org_slug: String,
    pub default_org_name: String,
}

#[derive(Clone, Debug)]
pub struct RequestUser {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub roles: Vec<String>,
}

pub fn build_schema(data: GraphqlData) -> SchemaType {
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .data(data)
        .finish()
}

#[derive(Default)]
pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn health(&self) -> ApiResult<HealthPayload> {
        Ok(HealthPayload { ok: true })
    }

    async fn me(&self, ctx: &Context<'_>) -> ApiResult<MePayload> {
        let requester = ctx
            .data::<RequestUser>()
            .map_err(|_| ApiError::Unauthorized)?
            .clone();
        let data = ctx
            .data::<GraphqlData>()
            .map_err(|_| ApiError::Internal(anyhow!("missing graphql data").into()))?;
        Ok(MePayload::from_requester(data, requester))
    }

    async fn version(&self) -> ApiResult<String> {
        Ok(env!("CARGO_PKG_VERSION").to_string())
    }
}

#[derive(Clone, Debug, SimpleObject, Serialize)]
pub struct HealthPayload {
    pub ok: bool,
}
