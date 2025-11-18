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
            eprintln!("skipping Postgres pipeline tests: TEST_DATABASE_URL not set");
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
async fn pipeline_stages_show_defaults() {
    let Some(ctx) = setup_pg().await else {
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
        .unwrap_or_default();
    assert_eq!(stages.len(), 6);
    assert_eq!(stages[0]["key"], "NEW");
    assert_eq!(stages.last().unwrap()["key"], "LOST");
    assert_eq!(
        stages
            .iter()
            .filter(|s| s["isWon"].as_bool().unwrap())
            .count(),
        1
    );
}

#[tokio::test]
async fn pipeline_board_reports_totals() {
    let Some(ctx) = setup_pg().await else {
        return;
    };
    let query = r#"
        query PipelineBoard($first: Int!) {
            crm {
                pipelineBoard(firstPerStage: $first, orderByUpdated: true) {
                    totalCount
                    totalAmountCents
                    totalExpectedCents
                    columns {
                        stage { key }
                        totalCount
                        totalAmountCents
                        expectedValueCents
                        deals { title stageKey companyName }
                    }
                }
            }
        }
    "#;
    let resp = ctx
        .schema
        .execute(Request::new(query).variables(Variables::from_json(json!({
            "first": 5
        }))))
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    let board = &resp.data.into_json().unwrap()["crm"]["pipelineBoard"];
    assert_eq!(board["totalCount"].as_i64().unwrap(), 5);
    assert_eq!(board["totalAmountCents"].as_i64().unwrap(), 605_000);
    assert_eq!(board["totalExpectedCents"].as_i64().unwrap(), 324_000);
    let columns = board["columns"].as_array().cloned().unwrap_or_default();
    let lost_column = columns
        .iter()
        .find(|c| c["stage"]["key"] == "LOST")
        .cloned()
        .unwrap();
    assert_eq!(lost_column["totalCount"].as_i64().unwrap(), 1);
    assert!(lost_column["deals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["title"] == "FossRust Support"));
}

#[tokio::test]
async fn pipeline_report_filters_and_forecasts() {
    let Some(ctx) = setup_pg().await else {
        return;
    };
    let query = r#"
        query PipelineReport($from: Date!, $to: Date!) {
            crm {
                pipelineReport(range: { from: $from, to: $to }, includeLost: false) {
                    stageTotals { stage { key } count amountCents expectedCents }
                    forecast { period amountCents expectedCents deals }
                    velocity { dealsWon avgDaysToWin p50DaysToWin p90DaysToWin }
                }
            }
        }
    "#;
    let resp = ctx
        .schema
        .execute(Request::new(query).variables(Variables::from_json(json!({
            "from": "2025-01-01",
            "to": "2025-03-31"
        }))))
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    let report = &resp.data.into_json().unwrap()["crm"]["pipelineReport"];
    let totals = report["stageTotals"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(totals
        .iter()
        .any(|t| t["stage"]["key"] == "WON" && t["count"].as_i64() == Some(1)));
    assert!(totals.iter().all(|t| t["stage"]["key"] != "LOST"));

    let forecast = report["forecast"].as_array().cloned().unwrap_or_default();
    assert_eq!(forecast.len(), 3); // Jan, Feb, Mar
    let january = forecast.iter().find(|p| p["period"] == "2025-01").unwrap();
    assert_eq!(january["amountCents"].as_i64().unwrap(), 270_000);
    let march = forecast.iter().find(|p| p["period"] == "2025-03").unwrap();
    assert_eq!(march["expectedCents"].as_i64().unwrap(), 90_000);

    let velocity = &report["velocity"];
    assert!(velocity["dealsWon"].as_i64().unwrap() >= 1);
    assert!(velocity["avgDaysToWin"].as_f64().unwrap() > 0.0);
}
