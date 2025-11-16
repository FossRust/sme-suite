mod graphql;
mod http;

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand};
use platform_obs::{ObsConfig, init_tracing};
use tracing::info;

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
    /// Run pending migrations (placeholder).
    Migrate,
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

#[derive(Args, Debug)]
struct ServeCommand {
    #[arg(long, default_value = "0.0.0.0")]
    host: std::net::IpAddr,
    #[arg(long, default_value_t = 8080)]
    port: u16,
}

impl From<ServeCommand> for http::ServeConfig {
    fn from(value: ServeCommand) -> Self {
        http::ServeConfig::new(value.host, value.port)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing(ObsConfig::default())?;
    let cli = Cli::parse();
    match cli.command {
        Command::Serve(cmd) => http::serve(cmd.into()).await,
        Command::Migrate => run_migrations().await,
        Command::Seed => run_seed().await,
        Command::SchemaPrint { output } => schema_print(output),
        Command::ApqGen => generate_apq_manifest(),
    }
}

async fn run_migrations() -> Result<()> {
    info!("migrate command invoked; implementation pending");
    Ok(())
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
