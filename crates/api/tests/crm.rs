mod common;

use api::auth::{CurrentUser, UserRole};
use async_graphql::{Request, Variables};
use common::PgTestContext;
use entity::{deal, deal_stage_history};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;
use uuid::Uuid;

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
async fn move_deal_stage_happy_path() {
    let Some(ctx) = PgTestContext::new_seeded().await else {
        return;
    };
    let schema = ctx.schema.clone();
    let current_user = owner_user(&ctx);
    let deal = ctx.seeded.deal_titled("ACME Pilot").expect("seeded deal");
    let mutation = r#"
        mutation Move($id: ID!, $stage: DealStage!, $note: String) {
            crm {
                moveDealStage(id: $id, stage: $stage, note: $note) {
                    id
                    stage
                }
            }
        }
    "#;
    let vars = Variables::from_json(json!({
        "id": deal.id,
        "stage": "PROPOSAL",
        "note": "kickoff"
    }));
    let resp = schema
        .execute(
            Request::new(mutation)
                .variables(vars)
                .data(current_user.clone()),
        )
        .await;
    assert!(
        resp.errors.is_empty(),
        "unexpected errors: {:?}",
        resp.errors
    );
    let updated = deal::Entity::find_by_id(deal.id)
        .one(ctx.db.as_ref())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.stage, deal::Stage::Proposal);
    let history = deal_stage_history::Entity::find()
        .filter(deal_stage_history::Column::DealId.eq(deal.id))
        .all(ctx.db.as_ref())
        .await
        .unwrap();
    assert!(
        history
            .iter()
            .any(|row| row.to_stage == deal::Stage::Proposal),
        "expected history entry in {:?}",
        history
    );
    ctx.cleanup().await;
}

#[tokio::test]
async fn move_deal_stage_noop_does_not_write_history() {
    let Some(ctx) = PgTestContext::new_seeded().await else {
        return;
    };
    let schema = ctx.schema.clone();
    let current_user = owner_user(&ctx);
    let deal = ctx
        .seeded
        .deal_titled("Rust Tooling Upgrade")
        .expect("seeded deal");
    let mutation = r#"
        mutation Move($id: ID!, $stage: DealStage!) {
            crm {
                moveDealStage(id: $id, stage: $stage) {
                    id
                    stage
                }
            }
        }
    "#;
    let vars = Variables::from_json(json!({
        "id": deal.id,
        "stage": "PROPOSAL"
    }));
    let resp = schema
        .execute(
            Request::new(mutation)
                .variables(vars)
                .data(current_user.clone()),
        )
        .await;
    assert!(resp.errors.is_empty());
    let history = deal_stage_history::Entity::find()
        .filter(deal_stage_history::Column::DealId.eq(deal.id))
        .all(ctx.db.as_ref())
        .await
        .unwrap();
    // tooling deal already in PROPOSAL stage -> no new entries
    assert!(history.is_empty());
    ctx.cleanup().await;
}

#[tokio::test]
async fn move_deal_stage_invalid_stage_rejected() {
    let Some(ctx) = PgTestContext::new_seeded().await else {
        return;
    };
    let schema = ctx.schema.clone();
    let current_user = owner_user(&ctx);
    let mutation = r#"
        mutation Move($id: ID!, $stage: DealStage!) {
            crm {
                moveDealStage(id: $id, stage: $stage) {
                    id
                }
            }
        }
    "#;
    let vars = Variables::from_json(json!({
        "id": Uuid::new_v4(),
        "stage": "WON"
    }));
    let resp = schema
        .execute(
            Request::new(mutation)
                .variables(vars)
                .data(current_user.clone()),
        )
        .await;
    assert!(
        resp.errors.iter().any(|err| {
            err.extensions
                .as_ref()
                .and_then(|ext| ext.get("code"))
                .and_then(|code| match code {
                    async_graphql::Value::String(inner) => Some(inner == "NOT_FOUND"),
                    _ => None,
                })
                .unwrap_or(false)
        }),
        "expected NOT_FOUND error, got {:?}",
        resp.errors
    );
    ctx.cleanup().await;
}

#[tokio::test]
async fn deal_stage_history_query_returns_latest_first() {
    let Some(ctx) = PgTestContext::new_seeded().await else {
        return;
    };
    let schema = ctx.schema.clone();
    let current_user = owner_user(&ctx);
    let mutation = r#"
        mutation Move($id: ID!, $stage: DealStage!) {
            crm {
                moveDealStage(id: $id, stage: $stage) {
                    id
                }
            }
        }
    "#;
    let deal = ctx.seeded.deal_titled("ACME Pilot").expect("seeded deal");
    let vars = Variables::from_json(json!({
        "id": deal.id,
        "stage": "NEGOTIATE"
    }));
    schema
        .execute(
            Request::new(mutation)
                .variables(vars)
                .data(current_user.clone()),
        )
        .await;
    let query = r#"
        query History($id: ID!) {
            crm {
                dealStageHistory(dealId: $id, first: 10) {
                    toStage
                }
            }
        }
    "#;
    let resp = schema
        .execute(
            Request::new(query)
                .variables(Variables::from_json(json!({ "id": deal.id })))
                .data(current_user),
        )
        .await;
    assert!(resp.errors.is_empty());
    let nodes = resp.data.into_json().unwrap()["crm"]["dealStageHistory"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!nodes.is_empty());
    assert_eq!(nodes[0]["toStage"], "NEGOTIATE");
    ctx.cleanup().await;
}
