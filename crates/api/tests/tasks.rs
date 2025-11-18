mod common;

use api::auth::{CurrentUser, UserRole};
use async_graphql::{Request, Variables};
use chrono::{Duration, Utc};
use common::PgTestContext;
use serde_json::json;

fn owner_user(ctx: &PgTestContext) -> CurrentUser {
    let owner = ctx
        .seeded
        .user_email("owner@sme.test")
        .expect("seeded owner");
    CurrentUser {
        user_id: owner.id,
        roles: vec![UserRole::Owner, UserRole::Admin],
    }
}

fn response_errors(resp: &async_graphql::Response) -> Vec<String> {
    resp.errors.iter().map(|err| err.message.clone()).collect()
}

#[tokio::test]
async fn task_list_limit_enforced() {
    let Some(ctx) = PgTestContext::new_seeded().await else {
        eprintln!("skipping tasks tests: TEST_DATABASE_URL not set");
        return;
    };
    let query = r#"
        query Tasks($first: Int!) {
            crm {
                tasks(first: $first) {
                    id
                }
            }
        }
    "#;
    let resp = ctx
        .schema
        .execute(
            Request::new(query)
                .variables(Variables::from_json(json!({ "first": 500 })))
                .data(owner_user(&ctx)),
        )
        .await;
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
        response_errors(&resp)
    );
    ctx.cleanup().await;
}

#[tokio::test]
async fn task_crud_flow() {
    let Some(ctx) = PgTestContext::new_seeded().await else {
        eprintln!("skipping tasks tests: TEST_DATABASE_URL not set");
        return;
    };
    let company = ctx
        .seeded
        .company_named("ACME, Inc.")
        .expect("seeded company");
    let query = r#"
        mutation Create($input: NewTaskInput!) {
            crm {
                createTask(input: $input) {
                    id
                    title
                    status
                }
            }
        }
    "#;
    let create_vars = Variables::from_json(json!({
        "input": {
            "title": "Call customer",
            "notesMd": "review next steps",
            "priority": "HIGH",
            "companyId": company.id,
            "dueAt": Utc::now().to_rfc3339(),
        }
    }));
    let current_user = owner_user(&ctx);
    let resp = ctx
        .schema
        .execute(
            Request::new(query)
                .variables(create_vars)
                .data(current_user.clone()),
        )
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    let task_id = resp.data.into_json().unwrap()["crm"]["createTask"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let update = r#"
        mutation Update($input: UpdateTaskInput!) {
            crm {
                updateTask(input: $input) {
                    id
                    title
                }
            }
        }
    "#;
    let resp = ctx
        .schema
        .execute(
            Request::new(update)
                .variables(Variables::from_json(json!({
                    "input": {
                        "id": task_id,
                        "title": "Call customer tomorrow"
                    }
                })))
                .data(current_user.clone()),
        )
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);

    let complete = r#"
        mutation Complete($id: ID!) {
            crm {
                completeTask(id: $id) {
                    id
                    status
                }
            }
        }
    "#;
    let resp = ctx
        .schema
        .execute(
            Request::new(complete)
                .variables(Variables::from_json(json!({ "id": task_id })))
                .data(current_user.clone()),
        )
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    assert_eq!(
        resp.data.into_json().unwrap()["crm"]["completeTask"]["status"],
        "DONE"
    );

    let delete = r#"
        mutation Delete($id: ID!) {
            crm {
                deleteTask(id: $id)
            }
        }
    "#;
    let resp = ctx
        .schema
        .execute(
            Request::new(delete)
                .variables(Variables::from_json(json!({ "id": task_id })))
                .data(current_user),
        )
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    assert!(resp.data.into_json().unwrap()["crm"]["deleteTask"]
        .as_bool()
        .unwrap());
    ctx.cleanup().await;
}

#[tokio::test]
async fn task_filters_ordering_and_pagination() {
    let Some(ctx) = PgTestContext::new_seeded().await else {
        eprintln!("skipping tasks tests: TEST_DATABASE_URL not set");
        return;
    };
    let current_user = owner_user(&ctx);
    let contact = ctx
        .seeded
        .contact_email("ada@acme.test")
        .expect("seeded contact");
    let deal = ctx.seeded.deal_titled("ACME Pilot").expect("seeded deal");
    let now = Utc::now();
    for days in 0..3 {
        let due = now + Duration::days(days);
        let create = r#"
            mutation Create($input: NewTaskInput!) {
                crm {
                    createTask(input: $input) { id }
                }
            }
        "#;
        ctx.schema
            .execute(
                Request::new(create)
                    .variables(Variables::from_json(json!({
                        "input": {
                            "title": format!("Follow up {}", days),
                            "notesMd": "auto",
                            "priority": "MEDIUM",
                            "contactId": contact.id,
                            "dealId": deal.id,
                            "dueAt": due.to_rfc3339()
                        }
                    })))
                    .data(current_user.clone()),
            )
            .await;
    }

    let query = r#"
        query Filter($term: String!) {
            crm {
                tasks(first: 2, filter: { q: $term }, orderBy: { field: DUE_AT, direction: ASC }) {
                    title
                }
            }
        }
    "#;
    let resp = ctx
        .schema
        .execute(
            Request::new(query)
                .variables(Variables::from_json(json!({ "term": "follow" })))
                .data(current_user),
        )
        .await;
    assert!(resp.errors.is_empty(), "errors: {:?}", resp.errors);
    let tasks = resp.data.into_json().unwrap()["crm"]["tasks"]
        .as_array()
        .cloned()
        .unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0]["title"], "Follow up 0");
    assert_eq!(tasks[1]["title"], "Follow up 1");
    ctx.cleanup().await;
}
