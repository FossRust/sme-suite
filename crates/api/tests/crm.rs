use std::sync::Arc;

use api::schema::{build_schema, AppSchema};
use async_graphql::{Request, Variables};
use chrono::Utc;
use entity::{activity, deal, deal_stage_history};
use sea_orm::{
    ColumnTrait, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, Statement, Value,
};
use serde_json::json;
use uuid::Uuid;

async fn setup_graphql_with_deal(
    initial_stage: deal::Stage,
) -> (
    Arc<DatabaseConnection>,
    async_graphql::Schema<
        api::schema::QueryRoot,
        api::schema::MutationRoot,
        async_graphql::EmptySubscription,
    >,
    deal::Model,
) {
    let conn = Database::connect("sqlite::memory:").await.unwrap();
    let db = Arc::new(conn);
    bootstrap_sqlite(db.as_ref()).await;

    let company_id = Uuid::new_v4();
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO company (id, name, website, phone, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        vec![
            company_id.into(),
            "ACME".into(),
            Value::from(None::<String>),
            Value::from(None::<String>),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await
    .unwrap();

    let deal_id = Uuid::new_v4();
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO deal (id, title, amount_cents, currency, stage, close_date, company_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![
            deal_id.into(),
            "Seed Deal".into(),
            5_000.into(),
            "USD".into(),
            stage_value(initial_stage).into(),
            Value::from(None::<String>),
            company_id.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .unwrap();

    let deal = deal::Entity::find_by_id(deal_id)
        .one(db.as_ref())
        .await
        .unwrap()
        .unwrap();

    let AppSchema(schema) = build_schema(db.clone());

    (db, schema, deal)
}

async fn bootstrap_sqlite(db: &DatabaseConnection) {
    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "PRAGMA foreign_keys = ON;",
    ))
    .await
    .unwrap();

    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        r#"
        CREATE TABLE company (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            website TEXT,
            phone TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        "#,
    ))
    .await
    .unwrap();

    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        r#"
        CREATE TABLE deal (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            amount_cents INTEGER,
            currency TEXT,
            stage TEXT NOT NULL DEFAULT 'NEW',
            close_date TEXT,
            company_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        "#,
    ))
    .await
    .unwrap();

    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        r#"
        CREATE TABLE deal_stage_history (
            id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
            deal_id TEXT NOT NULL,
            from_stage TEXT NOT NULL,
            to_stage TEXT NOT NULL,
            changed_at TEXT NOT NULL,
            note TEXT,
            changed_by TEXT,
            FOREIGN KEY(deal_id) REFERENCES deal(id) ON DELETE CASCADE
        );
        "#,
    ))
    .await
    .unwrap();

    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        r#"
        CREATE TABLE activity (
            id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
            entity_type TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            subject TEXT,
            body_md TEXT,
            meta_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            created_by TEXT
        );
        "#,
    ))
    .await
    .unwrap();
}

fn stage_value(stage: deal::Stage) -> &'static str {
    match stage {
        deal::Stage::New => "NEW",
        deal::Stage::Qualify => "QUALIFY",
        deal::Stage::Proposal => "PROPOSAL",
        deal::Stage::Negotiate => "NEGOTIATE",
        deal::Stage::Won => "WON",
        deal::Stage::Lost => "LOST",
    }
}

#[tokio::test]
async fn move_deal_stage_happy_path() {
    let (db, schema, deal) = setup_graphql_with_deal(deal::Stage::New).await;
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
    let resp = schema.execute(Request::new(mutation).variables(vars)).await;
    assert!(
        resp.errors.is_empty(),
        "unexpected errors: {:?}",
        resp.errors
    );

    let saved = deal::Entity::find_by_id(deal.id)
        .one(db.as_ref())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.stage, deal::Stage::Proposal);

    let history = deal_stage_history::Entity::find()
        .filter(deal_stage_history::Column::DealId.eq(deal.id))
        .all(db.as_ref())
        .await
        .unwrap();
    assert_eq!(history.len(), 1);
    let entry = &history[0];
    assert_eq!(entry.from_stage, deal::Stage::New);
    assert_eq!(entry.to_stage, deal::Stage::Proposal);
    assert_eq!(entry.note.as_deref(), Some("kickoff"));

    let activities = activity::Entity::find()
        .filter(activity::Column::EntityId.eq(deal.id))
        .all(db.as_ref())
        .await
        .unwrap();
    assert_eq!(activities.len(), 1);
    let act = &activities[0];
    assert_eq!(act.subject.as_deref(), Some("Stage: NEW -> PROPOSAL"));
    assert_eq!(act.body_md.as_deref(), Some("kickoff"));
    assert_eq!(act.meta_json, json!({"from":"NEW","to":"PROPOSAL"}));
}

#[tokio::test]
async fn move_deal_stage_noop_does_not_write_history() {
    let (db, schema, deal) = setup_graphql_with_deal(deal::Stage::Proposal).await;
    let before = deal.updated_at;
    let mutation = r#"
        mutation Move($id: ID!, $stage: DealStage!) {
            crm {
                moveDealStage(id: $id, stage: $stage) {
                    id
                    updatedAt
                }
            }
        }
    "#;
    let vars = Variables::from_json(json!({
        "id": deal.id,
        "stage": "PROPOSAL"
    }));
    let resp = schema.execute(Request::new(mutation).variables(vars)).await;
    assert!(
        resp.errors.is_empty(),
        "unexpected errors: {:?}",
        resp.errors
    );

    let saved = deal::Entity::find_by_id(deal.id)
        .one(db.as_ref())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.stage, deal::Stage::Proposal);
    assert!(saved.updated_at > before);

    let history_count = deal_stage_history::Entity::find()
        .filter(deal_stage_history::Column::DealId.eq(deal.id))
        .count(db.as_ref())
        .await
        .unwrap();
    assert_eq!(history_count, 0);

    let activity_count = activity::Entity::find()
        .filter(activity::Column::EntityId.eq(deal.id))
        .count(db.as_ref())
        .await
        .unwrap();
    assert_eq!(activity_count, 0);
}

#[tokio::test]
async fn move_deal_stage_invalid_stage_rejected() {
    let (_db, schema, deal) = setup_graphql_with_deal(deal::Stage::New).await;
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
        "id": deal.id,
        "stage": "NOT_A_STAGE"
    }));
    let resp = schema.execute(Request::new(mutation).variables(vars)).await;
    assert!(!resp.errors.is_empty(), "expected validation error");
}

#[tokio::test]
async fn deal_stage_history_query_returns_latest_first() {
    let (_db, schema, deal) = setup_graphql_with_deal(deal::Stage::New).await;
    let mutation = r#"
        mutation Move($id: ID!, $stage: DealStage!) {
            crm {
                moveDealStage(id: $id, stage: $stage) { id }
            }
        }
    "#;
    let req = |stage_value: &str| {
        Request::new(mutation).variables(Variables::from_json(json!({
            "id": deal.id,
            "stage": stage_value
        })))
    };

    let first = schema.execute(req("QUALIFY")).await;
    assert!(
        first.errors.is_empty(),
        "unexpected errors: {:?}",
        first.errors
    );
    let second = schema.execute(req("PROPOSAL")).await;
    assert!(
        second.errors.is_empty(),
        "unexpected errors: {:?}",
        second.errors
    );

    let query = r#"
        query History($id: ID!) {
            crm {
                dealStageHistory(dealId: $id, first: 10, offset: 0) {
                    fromStage
                    toStage
                }
            }
        }
    "#;
    let resp = schema
        .execute(Request::new(query).variables(Variables::from_json(json!({ "id": deal.id }))))
        .await;
    assert!(
        resp.errors.is_empty(),
        "unexpected errors: {:?}",
        resp.errors
    );
    let data = resp.data.into_json().unwrap();
    let items = data["crm"]["dealStageHistory"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["fromStage"], "QUALIFY");
    assert_eq!(items[0]["toStage"], "PROPOSAL");
    assert_eq!(items[1]["fromStage"], "NEW");
    assert_eq!(items[1]["toStage"], "QUALIFY");
}
