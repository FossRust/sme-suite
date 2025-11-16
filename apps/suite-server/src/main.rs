use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use platform_authn::AuthnService;
use platform_authz::PolicyEngine;
use platform_db::DatabaseSettings;
use products_crm::CrmModule;
use products_hr::HrModule;
use tokio::signal;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "suite-server", version, about = "Unified backend for the SME suite")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the HTTP + GraphQL server
    Serve(ServeCommand),
    /// Run database migrations
    Migrate {
        #[arg(long, default_value_t = MigrationDirection::Up)]
        direction: MigrationDirection,
    },
    /// Seed developer data
    Seed,
}

#[derive(clap::Args, Debug)]
struct ServeCommand {
    #[arg(long, default_value_t = String::from("0.0.0.0"))]
    host: String,
    #[arg(long, default_value_t = 8080)]
    port: u16,
}

#[derive(ValueEnum, Clone, Debug)]
enum MigrationDirection {
    Up,
    Down,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve(opts) => serve(opts).await?,
        Commands::Migrate { direction } => run_migrations(direction).await?,
        Commands::Seed => seed_data().await?,
    }

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).with_target(false).init();
}

async fn serve(opts: ServeCommand) -> anyhow::Result<()> {
    tracing::info!(host = %opts.host, port = opts.port, "launching suite-server placeholder");

    let database_settings = DatabaseSettings::from_env();
    if database_settings.database_url().is_some() {
        tracing::debug!(url = ?database_settings.database_url(), "database configuration detected");
    } else {
        tracing::warn!("DATABASE_URL not set; running in dev stub mode");
    }

    let _authn = AuthnService::default();
    let _policy_engine = PolicyEngine::default();
    let _crm = CrmModule::default();
    let _hr = HrModule::default();

    tracing::info!("waiting for ctrl+c to simulate server lifecycle");
    signal::ctrl_c()
        .await
        .context("failed to install Ctrl+C handler")?;

    tracing::info!("shutdown signal received");
    Ok(())
}

async fn run_migrations(direction: MigrationDirection) -> anyhow::Result<()> {
    match direction {
        MigrationDirection::Up => tracing::info!("migrations would run upward in future task"),
        MigrationDirection::Down => tracing::info!("migrations would roll back"),
    }
    Ok(())
}

async fn seed_data() -> anyhow::Result<()> {
    tracing::info!("seeding placeholder data for CRM + HR apps");
    Ok(())
}
