use api::schema::{build_schema, AppSchema};
use async_graphql::{http::GraphiQLSource, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{routing::get, Router};
use clap::{Parser, Subcommand};
use dotenvy::dotenv;
use migration::{Migrator, MigratorTrait};
use sea_orm::{Database, DatabaseConnection};
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
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
            let app = app_router(schema);

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

fn app_router(
    schema: Schema<
        api::schema::QueryRoot,
        api::schema::MutationRoot,
        async_graphql::EmptySubscription,
    >,
) -> Router {
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
        .with_state(schema)
}

async fn graphql_get(
    axum::extract::State(schema): axum::extract::State<
        Schema<api::schema::QueryRoot, api::schema::MutationRoot, async_graphql::EmptySubscription>,
    >,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

async fn graphql_post(
    axum::extract::State(schema): axum::extract::State<
        Schema<api::schema::QueryRoot, api::schema::MutationRoot, async_graphql::EmptySubscription>,
    >,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
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
    use entity::{company, contact, deal};
    use sea_orm::{ActiveModelTrait, Set};
    let acme = company::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set("ACME, Inc.".into()),
        website: Set(Some("https://acme.test".into())),
        phone: Set(Some("+1-555-0100".into())),
        ..Default::default()
    }
    .insert(db)
    .await?;

    let _ = contact::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set("ada@acme.test".into()),
        first_name: Set(Some("Ada".into())),
        last_name: Set(Some("Lovelace".into())),
        phone: Set(Some("+1-555-0110".into())),
        company_id: Set(Some(acme.id)),
        ..Default::default()
    }
    .insert(db)
    .await?;

    let _ = contact::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set("charles@acme.test".into()),
        first_name: Set(Some("Charles".into())),
        last_name: Set(Some("Babbage".into())),
        phone: Set(Some("+1-555-0111".into())),
        company_id: Set(Some(acme.id)),
        ..Default::default()
    }
    .insert(db)
    .await?;

    let inserted = deal::ActiveModel {
        id: Set(Uuid::new_v4()),
        title: Set("ACME Pilot".into()),
        amount_cents: Set(Some(120_000)),
        currency: Set(Some("USD".into())),
        stage: Set(deal::Stage::New),
        close_date: Set(None),
        company_id: Set(acme.id),
        ..Default::default()
    }
    .insert(db)
    .await?;

    api::schema::move_deal_stage_service(
        db,
        inserted.id,
        deal::Stage::Qualify,
        Some("Qualified via discovery".into()),
        Some("seed".into()),
    )
    .await
    .map_err(|err| anyhow::anyhow!("seed stage change failed: {:?}", err))?;
    Ok(())
}
