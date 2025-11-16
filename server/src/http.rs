use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use anyhow::Context;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::{FromRef, Path, Query, State},
    http::{self, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, Key, PrivateCookieJar, SameSite};
use chrono::{Duration, Utc};
use entity::{memberships, sessions, users};
use platform_authn::{AuthRegistry, TempLoginState};
use platform_db::{self, DbPool};
use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseBackend, EntityTrait, Set, Statement};
use serde::{Deserialize, Serialize};
use time::Duration as TimeDuration;
use tower::ServiceBuilder;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::info;
use uuid::Uuid;

use crate::{
    config::AppConfig,
    graphql::{RequestUser, SchemaType},
};

const SESSION_COOKIE: &str = "__Host-fs_session";
const OIDC_STATE_COOKIE: &str = "__Host-fs_oidc";

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub schema: SchemaType,
    pub config: Arc<AppConfig>,
    pub auth: Arc<AuthRegistry>,
    pub cookie_key: Key,
    pub default_org_id: Uuid,
    pub default_org_slug: String,
    pub default_org_name: String,
}

impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.cookie_key.clone()
    }
}

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

pub async fn serve(config: ServeConfig, state: AppState) -> anyhow::Result<()> {
    let router = build_router(state);
    let listener = tokio::net::TcpListener::bind(config.addr)
        .await
        .with_context(|| format!("failed to bind {}", config.addr))?;

    info!(%config.addr, "suite server listening");
    axum::serve(listener, router.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server error")?;
    Ok(())
}

fn cors_layer(origins: &[String]) -> CorsLayer {
    let allowed = origins
        .iter()
        .filter_map(|origin| origin.parse::<HeaderValue>().ok())
        .collect::<Vec<_>>();
    let allow_origin = if allowed.is_empty() {
        AllowOrigin::any()
    } else {
        AllowOrigin::list(allowed)
    };
    CorsLayer::new()
        .allow_credentials(true)
        .allow_headers([http::header::CONTENT_TYPE])
        .allow_methods([Method::POST, Method::GET])
        .allow_origin(allow_origin)
}

pub fn build_router(state: AppState) -> Router {
    let request_id = MakeRequestUuid;
    let header_name = HeaderName::from_static("x-request-id");
    Router::new()
        .route("/health", get(health_handler))
        .route("/login", get(login_handler))
        .route("/oidc/callback/:provider", get(oidc_callback_handler))
        .route("/logout", post(logout_handler))
        .route("/graphql", post(graphql_handler))
        .layer(
            ServiceBuilder::new()
                .layer(SetRequestIdLayer::new(header_name.clone(), request_id))
                .layer(PropagateRequestIdLayer::new(header_name))
                .layer(TraceLayer::new_for_http())
                .layer(cors_layer(&state.config.cors_allowed_origins))
        )
        .with_state(state)
}

#[derive(Deserialize)]
struct LoginQuery {
    provider: String,
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

async fn login_handler(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    Query(query): Query<LoginQuery>,
) -> HttpResult<(PrivateCookieJar, Redirect)> {
    let provider = state
        .auth
        .get(&query.provider)
        .ok_or_else(|| HttpError::new(StatusCode::NOT_FOUND, "unknown provider"))?;
    let auth_url = provider
        .authorize()
        .map_err(|err| HttpError::internal(err.into()))?;
    let temp_state = TempLoginState::random(&query.provider, &auth_url);
    let state_cookie = Cookie::build((
        OIDC_STATE_COOKIE,
        serde_json::to_string(&temp_state).unwrap(),
    ))
    .path("/")
    .secure(true)
    .http_only(true)
    .same_site(SameSite::Lax)
    .max_age(TimeDuration::minutes(10))
    .build();
    let jar = jar.add(state_cookie);
    Ok((jar, Redirect::to(auth_url.url.as_str())))
}

async fn oidc_callback_handler(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    Path(provider): Path<String>,
    Query(params): Query<CallbackQuery>,
) -> HttpResult<(PrivateCookieJar, Redirect)> {
    let provider = state
        .auth
        .get(&provider)
        .ok_or_else(|| HttpError::new(StatusCode::NOT_FOUND, "unknown provider"))?;
    let Some(cookie) = jar.get(OIDC_STATE_COOKIE) else {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "missing authentication state",
        ));
    };
    let jar = jar.remove(
        Cookie::build((OIDC_STATE_COOKIE, ""))
            .path("/")
            .build(),
    );
    let saved: TempLoginState = serde_json::from_str(cookie.value())
        .map_err(|_| HttpError::new(StatusCode::BAD_REQUEST, "invalid state cookie"))?;
    if saved.csrf != params.state {
        return Err(HttpError::new(StatusCode::BAD_REQUEST, "state mismatch"));
    }
    if saved.provider != provider.id {
        return Err(HttpError::new(StatusCode::BAD_REQUEST, "provider mismatch"));
    }
    let code = openidconnect::AuthorizationCode::new(params.code.clone());
    let user_info = provider
        .exchange(code, saved.verifier(), saved.nonce())
        .await
        .map_err(|err| HttpError::internal(err.into()))?;

    let user = platform_db::upsert_user(&state.pool, &user_info.email, user_info.name.clone())
        .await
        .map_err(|err| HttpError::internal(err.into()))?;
    let total_users = platform_db::user_count(&state.pool)
        .await
        .map_err(|err| HttpError::internal(err.into()))?;
    let roles = if total_users == 1 {
        vec!["owner".to_string()]
    } else {
        vec!["member".to_string()]
    };
    platform_db::ensure_membership(&state.pool, state.default_org_id, user.id, roles.clone())
        .await
        .map_err(|err| HttpError::internal(err.into()))?;

    let session_id = Uuid::new_v4();
    let now = Utc::now();
    let expires_at = now + Duration::days(30);
    let model = sessions::ActiveModel {
        id: Set(session_id),
        user_id: Set(user.id),
        created_at: Set(now.into()),
        expires_at: Set(expires_at.into()),
        ip: Set(None),
        user_agent: Set(None),
    };
    model
        .insert(&state.pool)
        .await
        .map_err(|err| HttpError::internal(err.into()))?;

    let cookie = Cookie::build((SESSION_COOKIE, session_id.to_string()))
        .path("/")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .max_age(TimeDuration::days(30))
        .build();
    let jar = jar.add(cookie);
    let redirect_target = state
        .config
        .cors_allowed_origins
        .first()
        .cloned()
        .unwrap_or_else(|| "/".into());
    Ok((jar, Redirect::to(&redirect_target)))
}

async fn logout_handler(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
) -> HttpResult<(PrivateCookieJar, StatusCode)> {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        if let Ok(session_id) = Uuid::parse_str(cookie.value()) {
            let _ = sessions::Entity::delete_by_id(session_id)
                .exec(&state.pool)
                .await;
        }
    }
    let jar = jar.remove(
        Cookie::build((SESSION_COOKIE, ""))
            .path("/")
            .build(),
    );
    Ok((jar, StatusCode::NO_CONTENT))
}

async fn graphql_handler(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    request: GraphQLRequest,
) -> HttpResult<GraphQLResponse> {
    let user = load_session(&state, &jar).await?;
    let mut req = request.into_inner();
    req = req.data(user);
    let response = state.schema.execute(req).await;
    Ok(GraphQLResponse::from(response))
}

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let db_ok = state
        .pool
        .execute(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT 1".to_string(),
        ))
        .await
        .is_ok();
    Json(HealthResponse {
        ok: db_ok,
        db_ok,
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Serialize)]
struct HealthResponse {
    ok: bool,
    db_ok: bool,
    version: &'static str,
}

type HttpResult<T> = Result<T, HttpError>;

async fn load_session(state: &AppState, jar: &PrivateCookieJar) -> HttpResult<RequestUser> {
    let cookie = jar
        .get(SESSION_COOKIE)
        .ok_or_else(|| HttpError::new(StatusCode::UNAUTHORIZED, "missing session"))?;
    let session_id = Uuid::parse_str(cookie.value())
        .map_err(|_| HttpError::new(StatusCode::UNAUTHORIZED, "invalid session"))?;
    let session = sessions::Entity::find_by_id(session_id)
        .one(&state.pool)
        .await
        .map_err(|err| HttpError::internal(err.into()))?
        .ok_or_else(|| HttpError::new(StatusCode::UNAUTHORIZED, "session not found"))?;
    if session.expires_at.with_timezone(&Utc) < Utc::now() {
        let _ = sessions::Entity::delete_by_id(session_id)
            .exec(&state.pool)
            .await;
        return Err(HttpError::new(StatusCode::UNAUTHORIZED, "session expired"));
    }
    let user = users::Entity::find_by_id(session.user_id)
        .one(&state.pool)
        .await
        .map_err(|err| HttpError::internal(err.into()))?
        .ok_or_else(|| HttpError::new(StatusCode::UNAUTHORIZED, "user not found"))?;
    let roles = memberships::Entity::find_by_id((state.default_org_id, user.id))
        .one(&state.pool)
        .await
        .map_err(|err| HttpError::internal(err.into()))?
        .map(|m| m.roles)
        .unwrap_or_default();
    Ok(RequestUser {
        id: user.id,
        email: user.email,
        name: user.name,
        roles,
    })
}

#[derive(Debug)]
struct HttpError {
    status: StatusCode,
    message: String,
}

impl HttpError {
    fn new(status: StatusCode, msg: &str) -> Self {
        Self {
            status,
            message: msg.to_string(),
        }
    }

    fn internal(err: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.to_string(),
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
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
