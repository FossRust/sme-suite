use std::sync::Arc;

use api::schema::{build_schema, seed_crm_demo, AppSchema};
use async_graphql::{Request, Variables};
use migration::MigratorTrait;
use sea_orm::Database;
use serde_json::json;

struct PgTestContext {
    schema: async_graphql::Schema<
        api::schema::QueryRoot,
        api::schema::MutationRoot,
        async_graphql::EmptySubscription,
    >,
}

async fn setup_pg() -> Option<PgTestContext> {
    let url = match std::env::var("TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("skipping Postgres search tests: TEST_DATABASE_URL not set");
            return None;
        }
    };

    let conn = Database::connect(&url).await.ok()?;
    let db = Arc::new(conn);
    migration::Migrator::reset(db.as_ref()).await.ok()?;
    seed_crm_demo(db.as_ref()).await.ok()?;
    let AppSchema(schema) = build_schema(db.clone());

    Some(PgTestContext { schema })
}

#[tokio::test]
async fn search_prefers_companies_and_contacts() {
    let Some(ctx) = setup_pg().await else {
        return;
    };
    let query = r#"
        query Search($term: String!) {
            crm {
                search(q: $term, first: 5) {
                    kind
                    title
                    subtitle
                }
            }
        }
    "#;
    let vars = Variables::from_json(json!({ "term": "ACME" }));
    let resp = ctx
        .schema
        .execute(Request::new(query).variables(vars))
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    let hits = resp.data.into_json().unwrap()["crm"]["search"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!hits.is_empty(), "expected search hits");
    assert_eq!(hits[0]["kind"], "COMPANY");
    assert_eq!(hits[0]["title"], "ACME, Inc.");
}

#[tokio::test]
async fn search_handles_trgm_typos() {
    let Some(ctx) = setup_pg().await else {
        return;
    };
    let query = r#"
        query Search($term: String!) {
            crm {
                search(q: $term, first: 5) {
                    title
                }
            }
        }
    "#;
    let vars = Variables::from_json(json!({ "term": "Ackme" }));
    let resp = ctx
        .schema
        .execute(Request::new(query).variables(vars))
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    let hits = resp.data.into_json().unwrap()["crm"]["search"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(hits.iter().any(|h| h["title"] == "ACME, Inc."));
}

#[tokio::test]
async fn search_enforces_limit() {
    let Some(ctx) = setup_pg().await else {
        return;
    };
    let query = r#"
        query Search($term: String!) {
            crm {
                search(q: $term, first: 200) {
                    title
                }
            }
        }
    "#;
    let resp = ctx
        .schema
        .execute(Request::new(query).variables(Variables::from_json(json!({
            "term": "acme"
        }))))
        .await;
    assert!(resp.errors.iter().any(|e| {
        e.extensions
            .as_ref()
            .and_then(|ext| ext.get("code"))
            .and_then(|code| match code {
                async_graphql::Value::String(inner) => Some(inner == "LIMIT_EXCEEDED"),
                _ => None,
            })
            .unwrap_or(false)
    }));
}
