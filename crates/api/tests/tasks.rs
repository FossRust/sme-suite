use std::sync::Arc;

use api::schema::{build_schema, AppSchema};
use async_graphql::{Request, ServerError, Value as GqlValue, Variables};
use chrono::{DateTime, Duration, Utc};
use sea_orm::{
    ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement, Value as DbValue,
};
use serde_json::{json, Value};
use uuid::Uuid;

struct TaskTestEnv {
    schema: async_graphql::Schema<
        api::schema::QueryRoot,
        api::schema::MutationRoot,
        async_graphql::EmptySubscription,
    >,
    company_id: Uuid,
    contact_id: Uuid,
    deal_id: Uuid,
}

async fn setup_task_env() -> TaskTestEnv {
    let conn = Database::connect("sqlite::memory:").await.unwrap();
    let db = Arc::new(conn);
    bootstrap_sqlite(db.as_ref()).await;

    let now = Utc::now().to_rfc3339();
    let company_id = Uuid::new_v4();
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO company (id, name, website, phone, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        vec![
            company_id.into(),
            "ACME".into(),
            DbValue::from(None::<String>),
            DbValue::from(None::<String>),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await
    .unwrap();

    let contact_id = Uuid::new_v4();
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO contact (id, email, first_name, last_name, phone, company_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        vec![
            contact_id.into(),
            "ada@example.test".into(),
            "Ada".into(),
            "Lovelace".into(),
            DbValue::from(None::<String>),
            company_id.into(),
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
            "Sample Deal".into(),
            42_000.into(),
            "USD".into(),
            "NEW".into(),
            DbValue::from(None::<String>),
            company_id.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .unwrap();

    let AppSchema(schema) = build_schema(db.clone());

    TaskTestEnv {
        schema,
        company_id,
        contact_id,
        deal_id,
    }
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
        CREATE TABLE contact (
            id TEXT PRIMARY KEY,
            email TEXT NOT NULL,
            first_name TEXT,
            last_name TEXT,
            phone TEXT,
            company_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(company_id) REFERENCES company(id) ON DELETE SET NULL
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
            updated_at TEXT NOT NULL,
            FOREIGN KEY(company_id) REFERENCES company(id) ON DELETE CASCADE
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

    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        r#"
        CREATE TABLE task (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            notes_md TEXT,
            status TEXT NOT NULL DEFAULT 'OPEN',
            priority TEXT NOT NULL DEFAULT 'MEDIUM',
            assignee TEXT,
            due_at TEXT,
            completed_at TEXT,
            company_id TEXT,
            contact_id TEXT,
            deal_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            CHECK (
                ((company_id IS NOT NULL) + (contact_id IS NOT NULL) + (deal_id IS NOT NULL)) = 1
            ),
            FOREIGN KEY(company_id) REFERENCES company(id) ON DELETE CASCADE,
            FOREIGN KEY(contact_id) REFERENCES contact(id) ON DELETE CASCADE,
            FOREIGN KEY(deal_id) REFERENCES deal(id) ON DELETE CASCADE
        );
        "#,
    ))
    .await
    .unwrap();
}

#[tokio::test]
async fn task_crud_flow() {
    let env = setup_task_env().await;
    let create = r#"
        mutation Create($input: NewTaskInput!) {
            crm { createTask(input: $input) { id title status priority companyId dueAt } }
        }
    "#;
    let vars = json!({
        "input": {
            "title": "Prepare kickoff",
            "notesMd": "Outline agenda",
            "priority": "HIGH",
            "assignee": "pm@example.test",
            "dueAt": (Utc::now() + Duration::days(2)).to_rfc3339(),
            "companyId": env.company_id
        }
    });
    let resp = exec(&env.schema, create, vars).await;
    assert!(
        resp.errors.is_empty(),
        "unexpected errors: {:?}",
        resp.errors
    );
    let data = resp.data.into_json().unwrap();
    let task = &data["crm"]["createTask"];
    let task_id = task["id"].as_str().unwrap();
    assert_eq!(task["status"], "OPEN");
    assert_eq!(task["priority"], "HIGH");

    let fetch = r#"
        query Task($id: ID!) { crm { task(id: $id) { id title notesMd assignee companyId } } }
    "#;
    let resp = exec(&env.schema, fetch, json!({ "id": task_id })).await;
    assert!(resp.errors.is_empty());
    let fetched = resp.data.into_json().unwrap()["crm"]["task"]
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(fetched["title"], "Prepare kickoff");
    assert_eq!(fetched["companyId"], env.company_id.to_string());

    let update = r#"
        mutation Update($input: UpdateTaskInput!) {
            crm { updateTask(input: $input) { id title priority dueAt assignee } }
        }
    "#;
    let resp = exec(
        &env.schema,
        update,
        json!({
            "input": {
                "id": task_id,
                "title": "Prepare kickoff + slides",
                "priority": "MEDIUM",
                "dueAt": (Utc::now() + Duration::days(3)).to_rfc3339(),
                "assignee": "ops@example.test"
            }
        }),
    )
    .await;
    assert!(resp.errors.is_empty());
    let updated = resp.data.into_json().unwrap()["crm"]["updateTask"]
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(updated["priority"], "MEDIUM");
    assert_eq!(updated["assignee"], "ops@example.test");

    let delete = r#"
        mutation Delete($id: ID!) { crm { deleteTask(id: $id) } }
    "#;
    let resp = exec(&env.schema, delete, json!({ "id": task_id })).await;
    assert!(resp.errors.is_empty());
    assert!(resp.data.into_json().unwrap()["crm"]["deleteTask"]
        .as_bool()
        .unwrap());

    let resp = exec(&env.schema, fetch, json!({ "id": task_id })).await;
    assert!(resp.errors.is_empty());
    assert!(resp.data.into_json().unwrap()["crm"]["task"].is_null());
}

#[tokio::test]
async fn task_transitions_are_idempotent() {
    let env = setup_task_env().await;
    let create = r#"
        mutation Create($input: NewTaskInput!) {
            crm { createTask(input: $input) { id } }
        }
    "#;
    let resp = exec(
        &env.schema,
        create,
        json!({
            "input": {
                "title": "Transition me",
                "companyId": env.company_id
            }
        }),
    )
    .await;
    let task_id = resp.data.into_json().unwrap()["crm"]["createTask"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let complete = r#"
        mutation Complete($id: ID!) { crm { completeTask(id: $id) { status completedAt } } }
    "#;
    let resp = exec(&env.schema, complete, json!({ "id": task_id })).await;
    assert!(resp.errors.is_empty());
    let first = resp.data.into_json().unwrap()["crm"]["completeTask"]
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(first["status"], "DONE");
    assert!(first["completedAt"].is_string());

    let resp = exec(&env.schema, complete, json!({ "id": task_id })).await;
    assert!(resp.errors.is_empty());
    assert_eq!(
        resp.data.into_json().unwrap()["crm"]["completeTask"]["status"],
        "DONE"
    );

    let reopen = r#"
        mutation Reopen($id: ID!) { crm { reopenTask(id: $id) { status completedAt } } }
    "#;
    let resp = exec(&env.schema, reopen, json!({ "id": task_id })).await;
    assert!(resp.errors.is_empty());
    let reopened = resp.data.into_json().unwrap()["crm"]["reopenTask"]
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(reopened["status"], "OPEN");
    assert!(reopened["completedAt"].is_null());

    let cancel = r#"
        mutation Cancel($id: ID!) { crm { cancelTask(id: $id) { status completedAt } } }
    "#;
    let resp = exec(&env.schema, cancel, json!({ "id": task_id })).await;
    assert!(resp.errors.is_empty());
    let cancelled = resp.data.into_json().unwrap()["crm"]["cancelTask"]
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(cancelled["status"], "CANCELLED");
    assert!(cancelled["completedAt"].is_null());

    let resp = exec(&env.schema, cancel, json!({ "id": task_id })).await;
    assert!(resp.errors.is_empty());
    assert_eq!(
        resp.data.into_json().unwrap()["crm"]["cancelTask"]["status"],
        "CANCELLED"
    );
}

#[tokio::test]
async fn task_validation_errors() {
    let env = setup_task_env().await;
    let create = r#"
        mutation Create($input: NewTaskInput!) { crm { createTask(input: $input) { id } } }
    "#;

    let cases = vec![
        json!({
            "title": "Bad target",
            "companyId": env.company_id,
            "contactId": env.contact_id
        }),
        json!({ "title": "Missing target" }),
        json!({
            "title": "Missing FK",
            "companyId": Uuid::new_v4()
        }),
        json!({
            "title": "x".repeat(300),
            "companyId": env.company_id
        }),
        json!({
            "title": "Notes too long",
            "companyId": env.company_id,
            "notesMd": "y".repeat(70_000)
        }),
    ];

    for case in cases {
        let resp = exec(&env.schema, create, json!({ "input": case })).await;
        assert!(
            has_error_code(&resp.errors, "VALIDATION"),
            "expected validation error"
        );
    }

    // Create valid task for update validation
    let resp = exec(
        &env.schema,
        create,
        json!({ "input": { "title": "Valid", "companyId": env.company_id } }),
    )
    .await;
    let task_id = resp.data.into_json().unwrap()["crm"]["createTask"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let update = r#"
        mutation Update($input: UpdateTaskInput!) { crm { updateTask(input: $input) { id } } }
    "#;
    let resp = exec(
        &env.schema,
        update,
        json!({
            "input": {
                "id": task_id,
                "title": "z".repeat(400)
            }
        }),
    )
    .await;
    assert!(has_error_code(&resp.errors, "VALIDATION"));
}

#[tokio::test]
async fn task_filters_ordering_and_pagination() {
    let env = setup_task_env().await;
    let create = r#"
        mutation Create($input: NewTaskInput!) { crm { createTask(input: $input) { id } } }
    "#;
    let base = Utc::now();
    let make_vars = |title: &str,
                     priority: &str,
                     target_key: &str,
                     target_val: Value,
                     due: Option<DateTime<Utc>>| {
        json!({
            "input": {
                "title": title,
                "priority": priority,
                "notesMd": format!("notes for {}", title),
                "dueAt": due.map(|d: DateTime<Utc>| d.to_rfc3339()),
                target_key: target_val
            }
        })
    };

    let high_id = exec(
        &env.schema,
        create,
        make_vars(
            "High Priority",
            "HIGH",
            "companyId",
            json!(env.company_id),
            Some(base + Duration::days(1)),
        ),
    )
    .await
    .data
    .into_json()
    .unwrap()["crm"]["createTask"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let medium_id = exec(
        &env.schema,
        create,
        make_vars(
            "Medium Priority",
            "MEDIUM",
            "contactId",
            json!(env.contact_id),
            Some(base + Duration::days(4)),
        ),
    )
    .await
    .data
    .into_json()
    .unwrap()["crm"]["createTask"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let low_id = exec(
        &env.schema,
        create,
        make_vars("Low Priority", "LOW", "dealId", json!(env.deal_id), None),
    )
    .await
    .data
    .into_json()
    .unwrap()["crm"]["createTask"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let search_id = exec(
        &env.schema,
        create,
        json!({
            "input": {
                "title": "Keyword Task",
                "notesMd": "findme keyword body",
                "priority": "LOW",
                "companyId": env.company_id
            }
        }),
    )
    .await
    .data
    .into_json()
    .unwrap()["crm"]["createTask"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Move tasks through transitions
    let _ = exec(
        &env.schema,
        r#"mutation Complete($id: ID!) { crm { completeTask(id: $id) { id } } }"#,
        json!({ "id": medium_id }),
    )
    .await;
    let _ = exec(
        &env.schema,
        r#"mutation Cancel($id: ID!) { crm { cancelTask(id: $id) { id } } }"#,
        json!({ "id": low_id }),
    )
    .await;

    // Status filter
    let list = list_tasks(
        &env,
        json!({ "filter": { "status": "DONE" }, "orderBy": "UPDATED_DESC" }),
    )
    .await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"].as_str().unwrap(), medium_id);

    // Priority filter
    let list = list_tasks(
        &env,
        json!({ "filter": { "priority": "HIGH" }, "orderBy": "UPDATED_DESC" }),
    )
    .await;
    assert_eq!(list[0]["id"].as_str().unwrap(), high_id);

    // Target filters
    let company_tasks = list_tasks(
        &env,
        json!({ "filter": { "companyId": env.company_id }, "orderBy": "UPDATED_DESC" }),
    )
    .await;
    assert_eq!(company_tasks.len(), 2);
    let contact_tasks = list_tasks(
        &env,
        json!({ "filter": { "contactId": env.contact_id }, "orderBy": "UPDATED_DESC" }),
    )
    .await;
    assert_eq!(contact_tasks[0]["id"].as_str().unwrap(), medium_id);
    let deal_tasks = list_tasks(
        &env,
        json!({ "filter": { "dealId": env.deal_id }, "orderBy": "UPDATED_DESC" }),
    )
    .await;
    assert_eq!(deal_tasks[0]["id"].as_str().unwrap(), low_id);

    // Due filters
    let before = base + Duration::days(2);
    let due_before = list_tasks(
        &env,
        json!({ "filter": { "dueBefore": before.to_rfc3339() }, "orderBy": "DUE_ASC" }),
    )
    .await;
    assert_eq!(due_before[0]["id"].as_str().unwrap(), high_id);

    let after = base + Duration::days(3);
    let due_after = list_tasks(
        &env,
        json!({ "filter": { "dueAfter": after.to_rfc3339() }, "orderBy": "DUE_ASC" }),
    )
    .await;
    assert_eq!(due_after[0]["id"].as_str().unwrap(), medium_id);

    // Search filter
    let search_results = list_tasks(
        &env,
        json!({ "filter": { "q": "keyword" }, "orderBy": "UPDATED_DESC" }),
    )
    .await;
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0]["id"].as_str().unwrap(), search_id);

    // Ordering
    let ordered = list_tasks(&env, json!({ "first": 4, "orderBy": "PRIORITY_DESC" })).await;
    assert_eq!(ordered[0]["id"].as_str().unwrap(), high_id);
    assert_eq!(ordered[1]["id"].as_str().unwrap(), medium_id);

    let due_desc = list_tasks(&env, json!({ "first": 4, "orderBy": "DUE_DESC" })).await;
    assert!(due_desc.last().unwrap()["dueAt"].is_null());

    // Pagination stable order
    let page = list_tasks(
        &env,
        json!({ "first": 2, "offset": 1, "orderBy": "DUE_ASC" }),
    )
    .await;
    assert_eq!(page.len(), 2);

    // Updated desc after manual update
    let _ = exec(
        &env.schema,
        r#"mutation Update($input: UpdateTaskInput!) { crm { updateTask(input: $input) { id } } }"#,
        json!({ "input": { "id": high_id, "title": "High Priority Updated" } }),
    )
    .await;
    let updated = list_tasks(&env, json!({ "first": 1, "orderBy": "UPDATED_DESC" })).await;
    assert_eq!(updated[0]["id"].as_str().unwrap(), high_id);
}

#[tokio::test]
async fn task_list_limit_enforced() {
    let env = setup_task_env().await;
    let query = r#"
        query Tasks($first: Int!) {
            crm { tasks(first: $first) { id } }
        }
    "#;
    let resp = exec(&env.schema, query, json!({ "first": 101 })).await;
    assert!(has_error_code(&resp.errors, "LIMIT_EXCEEDED"));
}

fn has_error_code(errors: &[ServerError], code: &str) -> bool {
    errors
        .iter()
        .any(|e| matches_code(e.extensions.as_ref(), code))
}

fn matches_code(values: Option<&async_graphql::ErrorExtensionValues>, code: &str) -> bool {
    match values.and_then(|ext| ext.get("code")) {
        Some(GqlValue::String(s)) => s == code,
        Some(GqlValue::Enum(name)) => name.as_str() == code,
        _ => false,
    }
}

async fn list_tasks(env: &TaskTestEnv, params: Value) -> Vec<Value> {
    let query = r#"
        query Tasks($first: Int, $offset: Int, $filter: TaskFilter, $orderBy: TaskOrder) {
            crm { tasks(first: $first, offset: $offset, filter: $filter, orderBy: $orderBy) {
                id title status priority dueAt companyId contactId dealId
            } }
        }
    "#;
    let resp = exec(&env.schema, query, params).await;
    assert!(
        resp.errors.is_empty(),
        "unexpected errors: {:?}",
        resp.errors
    );
    resp.data.into_json().unwrap()["crm"]["tasks"]
        .as_array()
        .unwrap()
        .to_vec()
}

async fn exec(
    schema: &async_graphql::Schema<
        api::schema::QueryRoot,
        api::schema::MutationRoot,
        async_graphql::EmptySubscription,
    >,
    query: &str,
    vars: Value,
) -> async_graphql::Response {
    schema
        .execute(Request::new(query).variables(Variables::from_json(vars)))
        .await
}
