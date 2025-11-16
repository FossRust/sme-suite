use anyhow::{Result, anyhow};
use once_cell::sync::OnceCell;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{self as sdk, Resource};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

static INIT: OnceCell<()> = OnceCell::new();

/// Configuration for tracing initialization.
#[derive(Clone, Debug)]
pub struct ObsConfig {
    pub service_name: &'static str,
    pub env_filter: Option<String>,
    pub otlp_endpoint: Option<String>,
}

impl Default for ObsConfig {
    fn default() -> Self {
        Self {
            service_name: "suite-server",
            env_filter: None,
            otlp_endpoint: None,
        }
    }
}

/// Install tracing subscribers with optional OTLP exporter.
pub fn init_tracing(config: ObsConfig) -> Result<()> {
    if INIT.get().is_some() {
        return Ok(());
    }

    let filter = config
        .env_filter
        .or_else(|| std::env::var("RUST_LOG").ok())
        .unwrap_or_else(|| "info,tower_http=warn".to_string());

    let env_filter = EnvFilter::try_new(filter)?;
    let fmt_layer = tracing_subscriber::fmt::layer().with_target(false);
    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer);

    let otlp_endpoint = config
        .otlp_endpoint
        .or_else(|| std::env::var("OTLP_ENDPOINT").ok());

    if let Some(endpoint) = otlp_endpoint {
        let exporter = SpanExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .with_endpoint(endpoint)
            .build()?;

        let resource = Resource::builder()
            .with_service_name(config.service_name)
            .build();

        let provider = sdk::trace::SdkTracerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(exporter)
            .build();
        let tracer = provider.tracer(config.service_name);

        registry
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .try_init()?;
    } else {
        registry.try_init()?;
    }

    INIT.set(())
        .map_err(|_| anyhow!("tracing already initialized"))?;
    Ok(())
}
