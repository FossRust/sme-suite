use api::{
    auth::{CurrentUser, UserRole},
    schema::{build_schema, AppSchema},
};
use async_graphql::{http::GraphiQLSource, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{extract::State, routing::get, Router};
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
    #[allow(dead_code)]
    db: Arc<DatabaseConnection>,
    current_user: CurrentUser,
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
            let AppSchema(schema) = build_schema(db.clone());
            println!("{}", schema.sdl());
            Ok(())
        }
        Cmd::Serve { bind } => {
            Migrator::up(db.as_ref(), None).await?;
            let AppSchema(schema) = build_schema(db.clone());
            let current_user = load_default_user(db.as_ref()).await?;
            let state = AppState {
                schema,
                db: db.clone(),
                current_user,
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
        .with_state(state)
}

async fn graphql_get(State(state): State<AppState>, req: GraphQLRequest) -> GraphQLResponse {
    execute_graphql(state, req).await
}

async fn graphql_post(State(state): State<AppState>, req: GraphQLRequest) -> GraphQLResponse {
    execute_graphql(state, req).await
}

async fn execute_graphql(state: AppState, req: GraphQLRequest) -> GraphQLResponse {
    let mut request = req.into_inner();
    request = request.data(state.current_user.clone());
    state.schema.execute(request).await.into()
}

async fn load_default_user(db: &DatabaseConnection) -> anyhow::Result<CurrentUser> {
    let user = app_user::Entity::find()
        .order_by_asc(app_user::Column::CreatedAt)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no users found; run seeds first"))?;
    let roles = user_role::Entity::find()
        .filter(user_role::Column::UserId.eq(user.id))
        .all(db)
        .await?
        .into_iter()
        .filter_map(|row| match row.role {
            user_role::Role::Owner => Some(UserRole::Owner),
            user_role::Role::Admin => Some(UserRole::Admin),
            user_role::Role::Sales => Some(UserRole::Sales),
            user_role::Role::Viewer => Some(UserRole::Viewer),
        })
        .collect();
    Ok(CurrentUser {
        user_id: user.id,
        roles,
    })
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
