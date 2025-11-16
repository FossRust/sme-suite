use std::net::SocketAddr;
use axum::{routing::get, Router};
use tower_http::{cors::{Any, CorsLayer}, trace::TraceLayer, compression::CompressionLayer};
use tracing::{info, Level};
use clap::{Parser, Subcommand};
use dotenvy::dotenv;
use sea_orm::{Database, DatabaseConnection};
use async_graphql::{http::GraphiQLSource, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use api::schema::{build_schema, AppSchema};
use migration::{Migrator, MigratorTrait};
use tokio::net::TcpListener;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "fossrust-crm-suite", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}
#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run HTTP server
    Serve {
        #[arg(long, env = "BIND", default_value = "127.0.0.1:8080")]
        bind: String
    },
    /// Run migrations (up|down|reset)
    Migrate {
        #[arg(long, default_value = "up")]
        action: String
    },
    /// Seed sample data
    Seed,
    /// Print GraphQL SDL
    PrintSchema,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(Level::INFO.into()))
        .init();

    let cli = Cli::parse();

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db = Database::connect(&db_url).await?;

    match cli.cmd {
        Cmd::Migrate { action } => {
            match action.as_str() {
                "up" => Migrator::up(&db, None).await?,
                "down" => Migrator::down(&db, None).await?,
                "reset" => Migrator::reset(&db).await?,
                _ => eprintln!("Unknown action: {} (use up|down|reset)", action),
            }
            Ok(())
        }
        Cmd::Seed => {
            seed(&db).await?;
            Ok(())
        }
        Cmd::PrintSchema => {
            let AppSchema(schema) = build_schema(db.clone());
            println!("{}", schema.sdl());
            Ok(())
        }
        Cmd::Serve { bind } => {
            Migrator::up(&db, None).await?;
            let AppSchema(schema) = build_schema(db.clone());
            let app = app_router(schema);

            let addr: SocketAddr = bind.parse()?;
            let listener = TcpListener::bind(addr).await?;
            info!("listening on http://{}", addr);
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
                .with_graceful_shutdown(shutdown_signal())
                .await?;
            Ok(())
        }
    }
}

fn app_router(schema: Schema<api::schema::QueryRoot, api::schema::MutationRoot, async_graphql::EmptySubscription>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/graphiql", get(graphiql))
        .route("/graphql", get(graphql_get).post(graphql_post))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
        )
        .with_state(schema)
}

async fn graphql_get(
    axum::extract::State(schema): axum::extract::State<Schema<api::schema::QueryRoot, api::schema::MutationRoot, async_graphql::EmptySubscription>>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

async fn graphql_post(
    axum::extract::State(schema): axum::extract::State<Schema<api::schema::QueryRoot, api::schema::MutationRoot, async_graphql::EmptySubscription>>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

async fn graphiql() -> (axum::http::HeaderMap, String) {
    let html = GraphiQLSource::build().endpoint("/graphql").finish();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8".parse().unwrap());
    (headers, html)
}

async fn shutdown_signal() {
    use tokio::signal;
    let ctrl_c = async { signal::ctrl_c().await.expect("failed to install Ctrl+C handler") };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv().await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {}, }
}

async fn seed(db: &DatabaseConnection) -> anyhow::Result<()> {
    use entity::{company, contact};
    use sea_orm::{Set, ActiveModelTrait};
    let acme = company::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set("ACME, Inc.".into()),
        website: Set(Some("https://acme.test".into())),
        phone: Set(Some("+1-555-0100".into())),
        ..Default::default()
    }.insert(db).await?;
    let _ = contact::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set("ceo@acme.test".into()),
        first_name: Set(Some("Ada".into())),
        last_name: Set(Some("Lovelace".into())),
        company_id: Set(Some(acme.id)),
        ..Default::default()
    }.insert(db).await?;
    Ok(())
}
