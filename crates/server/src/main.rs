use api::{
    auth::{
        build_session_cookie, decode_session_token, issue_session_token, AuthConfig, AuthMode,
        CurrentUser, UserRole, SESSION_COOKIE,
    },
    schema::{build_schema, AppSchema},
};
use async_graphql::{http::GraphiQLSource, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    body::Body,
    extract::{Extension, State},
    http::{HeaderMap, Request as AxumRequest},
    middleware::{from_fn_with_state, Next},
    response::Response,
    routing::get,
    Router,
};
use chrono::{Duration, Utc};
use clap::{Parser, Subcommand};
use dotenvy::dotenv;
use entity::{app_user, user_role};
use migration::{Migrator, MigratorTrait};
use sea_orm::{ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{info, Level};
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "fossrust-sme-suite", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}
#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run HTTP server
    Serve {
        #[arg(long, env = "BIND", default_value = "127.0.0.1:8080")]
        bind: String,
    },
    /// Run migrations (up|down|reset)
    Migrate {
        #[arg(long, default_value = "up")]
        action: String,
    },
    /// Seed sample data
    Seed,
    /// Print GraphQL SDL
    PrintSchema,
}

#[derive(Clone)]
struct AppState {
    schema:
        Schema<api::schema::QueryRoot, api::schema::MutationRoot, async_graphql::EmptySubscription>,
    db: Arc<DatabaseConnection>,
    auth: Arc<AuthConfig>,
    dev_user: Option<CurrentUser>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    let db_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => "postgres://sme_suite:sme_suite@localhost:5432/sme_suite".to_string(),
    };
    let db = Arc::new(Database::connect(&db_url).await?);
    let auth_config = Arc::new(load_auth_config_from_env()?);

    match cli.cmd {
        Cmd::Migrate { action } => {
            match action.as_str() {
                "up" => Migrator::up(db.as_ref(), None).await?,
                "down" => Migrator::down(db.as_ref(), None).await?,
                "reset" => Migrator::reset(db.as_ref()).await?,
                _ => eprintln!("Unknown action: {} (use up|down|reset)", action),
            }
            Ok(())
        }
        Cmd::Seed => {
            seed(db.as_ref()).await?;
            Ok(())
        }
        Cmd::PrintSchema => {
            let AppSchema(schema) = build_schema(db.clone(), auth_config.clone());
            println!("{}", schema.sdl());
            Ok(())
        }
        Cmd::Serve { bind } => {
            Migrator::up(db.as_ref(), None).await?;
            let AppSchema(schema) = build_schema(db.clone(), auth_config.clone());
            let dev_user = if auth_config.mode == AuthMode::Disabled {
                load_default_user(db.as_ref()).await?
            } else {
                None
            };
            let state = AppState {
                schema,
                db: db.clone(),
                auth: auth_config.clone(),
                dev_user,
            };
            let app = app_router(state);

            let addr: SocketAddr = bind.parse()?;
            let listener = TcpListener::bind(addr).await?;
            info!("listening on http://{}", addr);
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(shutdown_signal())
            .await?;
            Ok(())
        }
    }
}

fn app_router(state: AppState) -> Router {
    let middleware_state = state.clone();
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/graphiql", get(graphiql))
        .route("/graphql", get(graphql_get).post(graphql_post))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(from_fn_with_state(middleware_state, auth_middleware))
        .with_state(state)
}

async fn graphql_get(
    State(state): State<AppState>,
    current_user: Option<Extension<CurrentUser>>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    execute_graphql(state, current_user, req).await
}

async fn graphql_post(
    State(state): State<AppState>,
    current_user: Option<Extension<CurrentUser>>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    execute_graphql(state, current_user, req).await
}

async fn execute_graphql(
    state: AppState,
    current_user: Option<Extension<CurrentUser>>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    let mut request = req.into_inner();
    if let Some(Extension(user)) = current_user {
        request = request.data(user);
    }
    state.schema.execute(request).await.into()
}

async fn load_default_user(db: &DatabaseConnection) -> anyhow::Result<Option<CurrentUser>> {
    let user = match app_user::Entity::find()
        .order_by_asc(app_user::Column::CreatedAt)
        .one(db)
        .await?
    {
        Some(user) => user,
        None => return Ok(None),
    };
    Ok(load_current_user(db, user.id).await)
}

async fn graphiql() -> (axum::http::HeaderMap, String) {
    let html = GraphiQLSource::build().endpoint("/graphql").finish();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        "text/html; charset=utf-8".parse().unwrap(),
    );
    (headers, html)
}

async fn shutdown_signal() {
    use tokio::signal;
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler")
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {}, }
}

async fn seed(db: &DatabaseConnection) -> anyhow::Result<()> {
    use entity::task;
    use sea_orm::{ActiveModelTrait, Set};

    let seeded = api::schema::seed_crm_demo(db)
        .await
        .map_err(|err| anyhow::anyhow!("seed data failed: {}", err))?;
    let acme = seeded
        .company_named("ACME, Inc.")
        .ok_or_else(|| anyhow::anyhow!("missing seeded ACME company"))?;
    let ada = seeded
        .contact_email("ada@acme.test")
        .ok_or_else(|| anyhow::anyhow!("missing seeded Ada contact"))?;
    let acme_pilot = seeded
        .deal_titled("ACME Pilot")
        .ok_or_else(|| anyhow::anyhow!("missing seeded ACME Pilot deal"))?;

    let owner_user = seeded
        .user_email("owner@sme.test")
        .ok_or_else(|| anyhow::anyhow!("missing seeded owner user"))?;
    let sales_user = seeded
        .user_email("sales@sme.test")
        .ok_or_else(|| anyhow::anyhow!("missing seeded sales user"))?;

    let now = Utc::now();
    let open_due = now + Duration::days(7);
    task::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("Schedule kickoff call".into()),
        notes_md: Set(Some("Prepare deck with milestones.".into())),
        status: Set(task::Status::Open),
        priority: Set(task::Priority::High),
        assignee: Set(Some("pm@acme.test".into())),
        due_at: Set(Some(open_due.into())),
        completed_at: Set(None),
        company_id: Set(Some(acme.id)),
        contact_id: Set(None),
        deal_id: Set(None),
        assigned_user_id: Set(Some(sales_user.id)),
        created_by: Set(Some(owner_user.id)),
        updated_by: Set(Some(owner_user.id)),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    }
    .insert(db)
    .await?;

    let done_due = now - Duration::days(3);
    task::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("Send proposal".into()),
        notes_md: Set(Some("Proposal approved internally.".into())),
        status: Set(task::Status::Done),
        priority: Set(task::Priority::Medium),
        assignee: Set(Some("sales@acme.test".into())),
        due_at: Set(Some(done_due.into())),
        completed_at: Set(Some((now - Duration::days(1)).into())),
        company_id: Set(None),
        contact_id: Set(None),
        deal_id: Set(Some(acme_pilot.id)),
        assigned_user_id: Set(Some(sales_user.id)),
        created_by: Set(Some(owner_user.id)),
        updated_by: Set(Some(owner_user.id)),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    }
    .insert(db)
    .await?;

    task::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("Reschedule intro".into()),
        notes_md: Set(Some("Waiting on contact availability.".into())),
        status: Set(task::Status::Cancelled),
        priority: Set(task::Priority::Low),
        assignee: Set(None),
        due_at: Set(None),
        completed_at: Set(None),
        company_id: Set(None),
        contact_id: Set(Some(ada.id)),
        deal_id: Set(None),
        assigned_user_id: Set(Some(sales_user.id)),
        created_by: Set(Some(owner_user.id)),
        updated_by: Set(Some(owner_user.id)),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
    }
    .insert(db)
    .await?;

    Ok(())
}

async fn auth_middleware(
    State(state): State<AppState>,
    mut req: AxumRequest<Body>,
    next: Next,
) -> Response {
    let mut refresh_cookie: Option<String> = None;
    match state.auth.mode {
        AuthMode::Disabled => {
            if let Some(dev) = &state.dev_user {
                req.extensions_mut().insert(dev.clone());
            }
        }
        AuthMode::Local => {
            if let Some(token) = extract_session_token(req.headers()) {
                if let Ok(claims) = decode_session_token(&token, &state.auth) {
                    if let Some(user) = load_current_user(state.db.as_ref(), claims.sub).await {
                        req.extensions_mut().insert(user.clone());
                        if let Ok(new_token) = issue_session_token(user.user_id, &state.auth) {
                            refresh_cookie = Some(build_session_cookie(
                                &new_token,
                                state.auth.session_ttl_minutes,
                            ));
                        }
                    }
                }
            }
        }
    }
    let mut response = next.run(req).await;
    if let Some(cookie) = refresh_cookie {
        if let Ok(value) = cookie.parse() {
            response
                .headers_mut()
                .append(axum::http::header::SET_COOKIE, value);
        }
    }
    response
}

fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?;
    let value = raw.to_str().ok()?;
    for part in value.split(';') {
        let trimmed = part.trim();
        if let Some(rest) = trimmed.strip_prefix(SESSION_COOKIE) {
            let token = rest.trim_start_matches('=').trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

async fn load_current_user(db: &DatabaseConnection, user_id: Uuid) -> Option<CurrentUser> {
    let user = app_user::Entity::find_by_id(user_id).one(db).await.ok()??;
    if !user.is_active {
        return None;
    }
    let roles = user_role::Entity::find()
        .filter(user_role::Column::UserId.eq(user_id))
        .all(db)
        .await
        .ok()?
        .into_iter()
        .filter_map(|row| match row.role {
            user_role::Role::Owner => Some(UserRole::Owner),
            user_role::Role::Admin => Some(UserRole::Admin),
            user_role::Role::Sales => Some(UserRole::Sales),
            user_role::Role::Viewer => Some(UserRole::Viewer),
        })
        .collect();
    Some(CurrentUser { user_id, roles })
}

fn load_auth_config_from_env() -> anyhow::Result<AuthConfig> {
    let mode = match std::env::var("AUTH_MODE")
        .unwrap_or_else(|_| "disabled".to_string())
        .to_lowercase()
        .as_str()
    {
        "local" => AuthMode::Local,
        _ => AuthMode::Disabled,
    };
    let ttl = std::env::var("AUTH_SESSION_TTL_MINUTES")
        .ok()
        .and_then(|raw| raw.parse::<i64>().ok())
        .unwrap_or(15);
    let secret = std::env::var("AUTH_SESSION_SECRET").ok();
    if mode == AuthMode::Local && secret.is_none() {
        anyhow::bail!("AUTH_SESSION_SECRET must be set when AUTH_MODE=local");
    }
    Ok(AuthConfig::new(mode, secret, ttl))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    mod common {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../api/tests/common/mod.rs"
        ));
    }
    use common::PgTestContext;

    #[tokio::test]
    async fn login_logout_and_me_flow() {
        let Some(ctx) = PgTestContext::new_seeded_with_mode(AuthMode::Local).await else {
            eprintln!("skipping auth server test: TEST_DATABASE_URL not set");
            return;
        };
        let AppSchema(schema) = build_schema(ctx.db.clone(), ctx.auth.clone());
        let state = AppState {
            schema,
            db: ctx.db.clone(),
            auth: ctx.auth.clone(),
            dev_user: None,
        };
        let app = app_router(state);

        let login_body = json_request(
            r#"
            mutation Login($email: String!, $password: String!) {
                crm {
                    login(email: $email, password: $password) {
                        ok
                    }
                }
            }
            "#,
            json!({ "email": "owner@sme.test", "password": "ownerpass" }),
        );
        let response = app
            .clone()
            .oneshot(login_body)
            .await
            .expect("login response");
        let cookie = response
            .headers()
            .get(axum::http::header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string())
            .expect("session cookie");

        let response = app
            .clone()
            .oneshot(json_request(
                r#"
                query {
                    crm { me { email } }
                }
                "#,
                json!({}),
            ))
            .await
            .expect("me response");
        let body = response_json(response).await;
        assert!(body["data"]["crm"]["me"].is_null());

        let authed_me = app
            .clone()
            .oneshot(add_cookie(
                json_request(
                    r#"
                    query {
                        crm { me { email } }
                    }
                    "#,
                    json!({}),
                ),
                &cookie,
            ))
            .await
            .expect("me authed");
        let body = response_json(authed_me).await;
        assert_eq!(
            body["data"]["crm"]["me"]["email"].as_str(),
            Some("owner@sme.test")
        );

        let response = app
            .oneshot(add_cookie(
                json_request(
                    r#"
                    mutation {
                        crm { logout }
                    }
                    "#,
                    json!({}),
                ),
                &cookie,
            ))
            .await
            .expect("logout response");
        let logout_cookie = response
            .headers()
            .get(axum::http::header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            logout_cookie.contains("Max-Age=0"),
            "expected logout cookie, got {}",
            logout_cookie
        );

        ctx.cleanup().await;
    }

    fn json_request(query: &str, variables: serde_json::Value) -> Request<Body> {
        let payload = json!({ "query": query, "variables": variables }).to_string();
        Request::builder()
            .method(axum::http::Method::POST)
            .uri("/graphql")
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(payload))
            .unwrap()
    }

    fn add_cookie(mut request: Request<Body>, cookie: &str) -> Request<Body> {
        request
            .headers_mut()
            .insert(axum::http::header::COOKIE, cookie.parse().unwrap());
        request
    }

    async fn response_json(response: Response) -> Value {
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body collect")
            .to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }
}
