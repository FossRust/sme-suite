use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "sme_session";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AuthMode {
    Disabled,
    Local,
}

#[derive(Clone, Debug)]
pub struct AuthConfig {
    pub mode: AuthMode,
    secret: Option<String>,
    pub session_ttl_minutes: i64,
}

impl AuthConfig {
    pub fn new(mode: AuthMode, secret: Option<String>, ttl: i64) -> Self {
        Self {
            mode,
            secret,
            session_ttl_minutes: ttl,
        }
    }

    fn encoding_key(&self) -> Option<EncodingKey> {
        self.secret
            .as_ref()
            .map(|secret| EncodingKey::from_secret(secret.as_bytes()))
    }

    fn decoding_key(&self) -> Option<DecodingKey> {
        self.secret
            .as_ref()
            .map(|secret| DecodingKey::from_secret(secret.as_bytes()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionClaims {
    pub sub: Uuid,
    pub exp: usize,
    pub iat: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum UserRole {
    Owner,
    Admin,
    Sales,
    Viewer,
}

impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            UserRole::Owner => "OWNER",
            UserRole::Admin => "ADMIN",
            UserRole::Sales => "SALES",
            UserRole::Viewer => "VIEWER",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "OWNER" => Some(UserRole::Owner),
            "ADMIN" => Some(UserRole::Admin),
            "SALES" => Some(UserRole::Sales),
            "VIEWER" => Some(UserRole::Viewer),
            _ => None,
        }
    }

    pub fn level(self) -> u8 {
        match self {
            UserRole::Owner => 4,
            UserRole::Admin => 3,
            UserRole::Sales => 2,
            UserRole::Viewer => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub user_id: Uuid,
    pub roles: Vec<UserRole>,
}

impl CurrentUser {
    pub fn has_role(&self, role: UserRole) -> bool {
        self.roles.iter().any(|r| r.level() >= role.level())
    }

    pub fn highest_role(&self) -> Option<UserRole> {
        self.roles.iter().copied().max_by_key(|role| role.level())
    }
}

pub fn issue_session_token(user_id: Uuid, config: &AuthConfig) -> anyhow::Result<String> {
    let encoding = config
        .encoding_key()
        .ok_or_else(|| anyhow::anyhow!("missing session secret"))?;
    let now = Utc::now();
    let exp = now
        .checked_add_signed(Duration::minutes(config.session_ttl_minutes))
        .unwrap_or(now)
        .timestamp() as usize;
    let claims = SessionClaims {
        sub: user_id,
        exp,
        iat: now.timestamp() as usize,
    };
    jsonwebtoken::encode(&Header::default(), &claims, &encoding).map_err(|err| anyhow::anyhow!(err))
}

pub fn decode_session_token(token: &str, config: &AuthConfig) -> anyhow::Result<SessionClaims> {
    let decoding = config
        .decoding_key()
        .ok_or_else(|| anyhow::anyhow!("missing session secret"))?;
    jsonwebtoken::decode::<SessionClaims>(token, &decoding, &Validation::default())
        .map(|data| data.claims)
        .map_err(|err| anyhow::anyhow!(err))
}

pub fn build_session_cookie(token: &str, ttl_minutes: i64) -> String {
    let max_age = (ttl_minutes.max(0) * 60).to_string();
    format!(
        "{}={}; Max-Age={}; Path=/; HttpOnly; SameSite=Lax",
        SESSION_COOKIE, token, max_age
    )
}

pub fn clear_session_cookie() -> String {
    format!(
        "{}=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax",
        SESSION_COOKIE
    )
}
