use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema, SimpleObject};
use platform_api::ApiResult;
use serde::Serialize;
use tracing::instrument;

pub type SchemaType = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

pub fn build_schema() -> SchemaType {
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription).finish()
}

#[derive(Default)]
pub struct QueryRoot;

#[Object]
impl QueryRoot {
    #[instrument(name = "graphql.health", skip_all)]
    async fn health(&self) -> ApiResult<HealthPayload> {
        Ok(HealthPayload { ok: true })
    }

    #[instrument(name = "graphql.me", skip_all)]
    async fn me(&self) -> ApiResult<Option<String>> {
        Ok(None)
    }

    #[instrument(name = "graphql.version", skip_all)]
    async fn version(&self) -> ApiResult<String> {
        Ok(env!("CARGO_PKG_VERSION").to_string())
    }
}

#[derive(Clone, Debug, SimpleObject, Serialize)]
pub struct HealthPayload {
    pub ok: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_graphql::Request;
    use serde_json::json;

    #[tokio::test]
    async fn health_query_returns_ok() {
        let schema = build_schema();
        let response = schema.execute(Request::new("{ health { ok } }")).await;
        assert!(response.errors.is_empty());
        let body = response.data.into_json().unwrap();
        assert_eq!(body, json!({"health": {"ok": true}}));
    }
}
