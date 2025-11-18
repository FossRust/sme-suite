use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "sme_session";

#[derive(Clone, Debug)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub local_auth_enabled: bool,
    pub oidc_enabled: bool,
    pub session_ttl_minutes: i64,
}

impl AuthConfig {
    pub fn encoding_key(&self) -> EncodingKey {
        EncodingKey::from_secret(self.jwt_secret.as_bytes())
    }

    pub fn decoding_key(&self) -> DecodingKey {
        DecodingKey::from_secret(self.jwt_secret.as_bytes())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionClaims {
    pub sub: Uuid,
    pub roles: Vec<String>,
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
        self.roles.iter().copied().max_by_key(UserRole::level)
    }
}

pub fn issue_token(
    user_id: Uuid,
    roles: &[UserRole],
    config: &AuthConfig,
) -> jsonwebtoken::errors::Result<String> {
    let now = Utc::now();
    let exp = now
        .checked_add_signed(Duration::minutes(config.session_ttl_minutes))
        .unwrap_or(now)
        .timestamp() as usize;
    let claims = SessionClaims {
        sub: user_id,
        roles: roles.iter().map(|r| r.as_str().to_string()).collect(),
        exp,
        iat: now.timestamp() as usize,
    };
    jsonwebtoken::encode(&Header::default(), &claims, &config.encoding_key())
}

pub fn decode_token(
    token: &str,
    config: &AuthConfig,
) -> jsonwebtoken::errors::Result<SessionClaims> {
    jsonwebtoken::decode::<SessionClaims>(
        token,
        &config.decoding_key(),
        &Validation::default(),
    )
    .map(|data| data.claims)
}
