mod config;
mod graphql;
mod http;

use std::path::PathBuf;

use std::sync::Arc;

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand};
use migration::{Migrator, MigratorTrait};
use platform_authn::AuthRegistry;
use platform_db::{self, DatabaseSettings, DbPool, connect};
use platform_obs::{ObsConfig, init_tracing};
use tracing::info;

use crate::{
    config::AppConfig,
    graphql::GraphqlData,
    http::{AppState, ServeConfig},
};

#[derive(Parser, Debug)]
#[command(name = "suite-server", version, about = "FossRust SME Suite")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Start the HTTP + GraphQL server.
    Serve(ServeCommand),
    /// Run database migrations.
    #[command(subcommand)]
    Migrate(MigrateCommand),
    /// Seed fixture data (placeholder).
    Seed,
    /// Print the GraphQL schema snapshot.
    #[command(name = "schema:print")]
    SchemaPrint {
        #[arg(long, value_name = "FILE", help = "Destination file path")]
        output: Option<PathBuf>,
    },
    /// Generate persisted queries manifest (placeholder).
    #[command(name = "apq:gen")]
    ApqGen,
}

#[derive(Subcommand, Debug)]
enum MigrateCommand {
    /// Apply pending migrations.
    Up,
    /// Rollback the most recent migration.
    Down,
}

#[derive(Args, Debug)]
struct ServeCommand {
    #[arg(long, default_value = "0.0.0.0")]
    host: std::net::IpAddr,
    #[arg(long, default_value_t = 8080)]
    port: u16,
    #[arg(long, help = "Allow starting even when migrations are pending")]
    allow_dirty: bool,
}

impl From<ServeCommand> for ServeConfig {
    fn from(value: ServeCommand) -> Self {
        ServeConfig::new(value.host, value.port)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing(ObsConfig::default())?;
    let cli = Cli::parse();
    let app_config = Arc::new(AppConfig::load()?);
    match cli.command {
        Command::Serve(cmd) => run_server(cmd, app_config).await,
        Command::Migrate(action) => match action {
            MigrateCommand::Up => migrate_up().await,
            MigrateCommand::Down => migrate_down().await,
        },
        Command::Seed => run_seed().await,
        Command::SchemaPrint { output } => schema_print(output),
        Command::ApqGen => generate_apq_manifest(),
    }
}

async fn run_seed() -> Result<()> {
    info!("seed command invoked; implementation pending");
    Ok(())
}

fn schema_print(path: Option<PathBuf>) -> Result<()> {
    let target = path.unwrap_or_else(|| PathBuf::from("schema.graphql"));
    Err(anyhow!(
        "schema snapshot {} not generated yet",
        target.display()
    ))
}

fn generate_apq_manifest() -> Result<()> {
    Err(anyhow!("APQ manifest generation not implemented yet"))
}

async fn setup_pool() -> Result<DbPool> {
    let settings = DatabaseSettings::from_env();
    connect(&settings).await.map_err(Into::into)
}

async fn run_server(cmd: ServeCommand, config: Arc<AppConfig>) -> Result<()> {
    let pool = setup_pool().await?;
    ensure_migrations(&pool, cmd.allow_dirty).await?;
    let default_org_id =
        platform_db::ensure_default_org(&pool, &config.default_org_slug, &config.default_org_name)
            .await?;
    let graphql_data = GraphqlData {
        pool: pool.clone(),
        default_org_id,
        default_org_slug: config.default_org_slug.clone(),
        default_org_name: config.default_org_name.clone(),
    };
    let schema = graphql::build_schema(graphql_data);
    let auth = Arc::new(AuthRegistry::from_config(&config.providers).await?);
    let org_slug = config.default_org_slug.clone();
    let org_name = config.default_org_name.clone();
    let cookie_key = config.cookie_key.clone();
    let state = AppState {
        pool,
        schema,
        config: config.clone(),
        auth,
        cookie_key,
        default_org_id,
        default_org_slug: org_slug,
        default_org_name: org_name,
    };
    http::serve(cmd.into(), state).await
}

async fn ensure_migrations(pool: &DbPool, allow_dirty: bool) -> Result<()> {
    let pending = Migrator::get_pending_migrations(pool).await?;
    if !pending.is_empty() && !allow_dirty {
        anyhow::bail!(
            "pending migrations detected; run `cargo run -p server -- migrate up` or pass --allow-dirty"
        );
    }
    Ok(())
}

async fn migrate_up() -> Result<()> {
    let pool = setup_pool().await?;
    Migrator::up(&pool, None).await?;
    info!("database migrations applied");
    Ok(())
}

async fn migrate_down() -> Result<()> {
    let pool = setup_pool().await?;
    Migrator::down(&pool, Some(1)).await?;
    info!("most recent migration rolled back");
    Ok(())
}
