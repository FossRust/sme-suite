mod common;

use api::auth::{CurrentUser, UserRole};
use async_graphql::{Request, Variables};
use common::PgTestContext;
use serde_json::json;

fn owner_user(ctx: &PgTestContext) -> CurrentUser {
    let owner = ctx
        .seeded
        .user_email("owner@sme.test")
        .expect("seeded owner user");
    CurrentUser {
        user_id: owner.id,
        roles: vec![UserRole::Owner, UserRole::Admin],
    }
}

#[tokio::test]
async fn search_prefers_companies_and_contacts() {
    let Some(ctx) = PgTestContext::new_seeded().await else {
        eprintln!("skipping Postgres search tests: TEST_DATABASE_URL not set");
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
        .execute(Request::new(query).variables(vars).data(owner_user(&ctx)))
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    let hits = resp.data.into_json().unwrap()["crm"]["search"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!hits.is_empty(), "expected search hits");
    assert_eq!(hits[0]["kind"], "COMPANY");
    assert_eq!(hits[0]["title"], "ACME, Inc.");
    ctx.cleanup().await;
}

#[tokio::test]
async fn search_handles_trgm_typos() {
    let Some(ctx) = PgTestContext::new_seeded().await else {
        eprintln!("skipping Postgres search tests: TEST_DATABASE_URL not set");
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
        .execute(Request::new(query).variables(vars).data(owner_user(&ctx)))
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    let hits = resp.data.into_json().unwrap()["crm"]["search"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(hits.iter().any(|h| h["title"] == "ACME, Inc."));
    ctx.cleanup().await;
}

#[tokio::test]
async fn search_enforces_limit() {
    let Some(ctx) = PgTestContext::new_seeded().await else {
        eprintln!("skipping Postgres search tests: TEST_DATABASE_URL not set");
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
        .execute(
            Request::new(query)
                .variables(Variables::from_json(json!({ "term": "acme" })))
                .data(owner_user(&ctx)),
        )
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
    ctx.cleanup().await;
}
