use std::sync::Arc;

use api::schema::{build_schema, seed_crm_demo, AppSchema};
use async_graphql::{Request, Variables};
use migration::MigratorTrait;
use sea_orm::Database;
use serde_json::json;

struct PipelineTestContext {
    schema: async_graphql::Schema<
        api::schema::QueryRoot,
        api::schema::MutationRoot,
        async_graphql::EmptySubscription,
    >,
}

async fn setup_pg() -> Option<PipelineTestContext> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let conn = Database::connect(&url).await.ok()?;
    let db = Arc::new(conn);
    migration::Migrator::reset(db.as_ref()).await.ok()?;
    seed_crm_demo(db.as_ref()).await.ok()?;
    let AppSchema(schema) = build_schema(db.clone());
    Some(PipelineTestContext { schema })
}

#[tokio::test]
async fn pipeline_stages_return_defaults() {
    let Some(ctx) = setup_pg().await else {
        eprintln!("skipping pipeline tests: TEST_DATABASE_URL not set");
        return;
    };
    let query = r#"
        query PipelineStages {
            crm {
                pipelineStages {
                    key
                    displayName
                    sortOrder
                    probability
                    isWon
                    isLost
                }
            }
        }
    "#;
    let resp = ctx.schema.execute(Request::new(query)).await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    let stages = resp.data.into_json().unwrap()["crm"]["pipelineStages"]
        .as_array()
        .cloned()
        .unwrap();
    assert_eq!(stages.len(), 6, "expected default stage count");
    assert_eq!(stages[0]["key"], "NEW");
    assert_eq!(stages[0]["probability"], 10);
    assert_eq!(stages.last().unwrap()["key"], "LOST");
    assert!(stages.iter().any(|stage| stage["isWon"] == true));
    assert!(stages.iter().any(|stage| stage["isLost"] == true));
}

#[tokio::test]
async fn pipeline_board_returns_columns_and_totals() {
    let Some(ctx) = setup_pg().await else {
        eprintln!("skipping pipeline tests: TEST_DATABASE_URL not set");
        return;
    };
    let query = r#"
        query PipelineBoard($first: Int!) {
            crm {
                pipelineBoard(firstPerStage: $first) {
                    totalCount
                    totalAmountCents
                    totalExpectedCents
                    columns {
                        stage { key }
                        totalCount
                        totalAmountCents
                        expectedValueCents
                        deals {
                            id
                            title
                            stageKey
                        }
                    }
                }
            }
        }
    "#;
    let vars = Variables::from_json(json!({ "first": 2 }));
    let resp = ctx
        .schema
        .execute(Request::new(query).variables(vars))
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    let board = resp.data.into_json().unwrap()["crm"]["pipelineBoard"].clone();
    assert_eq!(board["totalCount"], 8);
    assert_eq!(board["totalAmountCents"], 680000);
    assert_eq!(board["totalExpectedCents"], 302500);
    let columns = board["columns"].as_array().cloned().unwrap();
    assert_eq!(columns.len(), 6);
    let qualify = columns
        .iter()
        .find(|col| col["stage"]["key"] == "QUALIFY")
        .cloned()
        .expect("qualify column");
    assert_eq!(qualify["totalCount"], 2);
    assert_eq!(qualify["totalAmountCents"], 330000);
    assert_eq!(qualify["expectedValueCents"], 82500);
    assert!(
        qualify["deals"].as_array().unwrap().len() <= 2,
        "expected per-stage limit respected"
    );
}

#[tokio::test]
async fn pipeline_board_enforces_limit() {
    let Some(ctx) = setup_pg().await else {
        eprintln!("skipping pipeline tests: TEST_DATABASE_URL not set");
        return;
    };
    let query = r#"
        query PipelineBoard {
            crm {
                pipelineBoard(firstPerStage: 150) {
                    totalCount
                }
            }
        }
    "#;
    let resp = ctx.schema.execute(Request::new(query)).await;
    assert!(
        resp.errors.iter().any(|err| {
            err.extensions
                .as_ref()
                .and_then(|ext| ext.get("code"))
                .and_then(|code| match code {
                    async_graphql::Value::String(inner) => Some(inner == "LIMIT_EXCEEDED"),
                    _ => None,
                })
                .unwrap_or(false)
        }),
        "expected limit error, got {:?}",
        resp.errors
    );
}

#[tokio::test]
async fn pipeline_report_returns_metrics() {
    let Some(ctx) = setup_pg().await else {
        eprintln!("skipping pipeline tests: TEST_DATABASE_URL not set");
        return;
    };
    let query = r#"
        query PipelineReport($range: DateRange!) {
            crm {
                pipelineReport(range: $range, group: MONTH) {
                    stageTotals {
                        stage { key }
                        count
                        amountCents
                    }
                    forecast {
                        period
                        amountCents
                        expectedCents
                        deals
                    }
                    velocity {
                        dealsWon
                        avgDaysToWin
                        p50DaysToWin
                        p90DaysToWin
                    }
                }
            }
        }
    "#;
    let vars = Variables::from_json(json!({
        "range": { "from": "2025-01-01", "to": "2025-03-31" }
    }));
    let resp = ctx
        .schema
        .execute(Request::new(query).variables(vars))
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    let report = resp.data.into_json().unwrap()["crm"]["pipelineReport"].clone();
    let stage_totals = report["stageTotals"].as_array().cloned().unwrap();
    assert!(
        stage_totals.iter().any(|row| row["stage"]["key"] == "WON"
            && row["count"] == 2
            && row["amountCents"] == 135000),
        "expected won totals in {:?}",
        stage_totals
    );
    assert!(
        stage_totals.iter().all(|row| row["stage"]["key"] != "LOST"),
        "lost stage excluded by default"
    );
    let forecast = report["forecast"].as_array().cloned().unwrap();
    assert_eq!(forecast.len(), 3);
    assert_eq!(forecast[0]["period"], "2025-01");
    assert_eq!(forecast[0]["amountCents"], 215000);
    assert_eq!(forecast[2]["period"], "2025-03");
    assert_eq!(forecast[2]["deals"], 2);
    let velocity = report["velocity"].clone();
    assert_eq!(velocity["dealsWon"], 2);
    let avg = velocity["avgDaysToWin"].as_f64().unwrap();
    assert!((avg - 33.0).abs() < 0.1, "unexpected avg days: {}", avg);
    assert!(velocity["p50DaysToWin"].as_f64().unwrap() > 0.0);
    assert!(
        velocity["p90DaysToWin"].as_f64().unwrap() >= velocity["p50DaysToWin"].as_f64().unwrap()
    );
}
