use std::net::{IpAddr, SocketAddr};

use anyhow::Context;
use async_graphql_axum::GraphQL;
use axum::{
    Json, Router,
    http::HeaderName,
    response::IntoResponse,
    routing::{get, post_service},
};
use serde::Serialize;
use tokio::{net::TcpListener, signal};
use tower::ServiceBuilder;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::{info, instrument};

use crate::graphql::{self, SchemaType};

#[derive(Clone, Debug)]
pub struct ServeConfig {
    addr: SocketAddr,
}

impl ServeConfig {
    pub fn new(host: IpAddr, port: u16) -> Self {
        Self {
            addr: SocketAddr::from((host, port)),
        }
    }
}

pub async fn serve(config: ServeConfig) -> anyhow::Result<()> {
    let schema = graphql::build_schema();
    let router = build_router(schema);
    let listener = TcpListener::bind(config.addr)
        .await
        .with_context(|| format!("failed to bind {}", config.addr))?;

    info!(%config.addr, "suite server listening");
    axum::serve(listener, router.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server error")?;

    Ok(())
}

pub fn build_router(schema: SchemaType) -> Router {
    let request_id = MakeRequestUuid;
    let header_name = HeaderName::from_static("x-request-id");
    Router::new()
        .route("/health", get(health_handler))
        .route("/graphql", post_service(GraphQL::new(schema)))
        .layer(
            ServiceBuilder::new()
                .layer(SetRequestIdLayer::new(header_name.clone(), request_id))
                .layer(PropagateRequestIdLayer::new(header_name))
                .layer(TraceLayer::new_for_http()),
        )
}

#[instrument(name = "http.health", skip_all)]
async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Serialize)]
struct HealthResponse {
    ok: bool,
    version: &'static str,
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install CTRL+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};

        signal(SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    ctrl_c.await;

    #[cfg(unix)]
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_route_returns_ok() {
        let schema = crate::graphql::build_schema();
        let app = build_router(schema);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["ok"], true);
    }
}
