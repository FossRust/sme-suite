use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use axum_extra::extract::cookie::Key;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use platform_authn::ProviderConfig;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub single_tenant: bool,
    pub default_org_slug: String,
    pub default_org_name: String,
    pub cookie_key: Key,
    pub cors_allowed_origins: Vec<String>,
    pub providers: HashMap<String, ProviderConfig>,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let single_tenant = std::env::var("SINGLE_TENANT")
            .ok()
            .map(|val| matches!(val.to_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(true);

        let default_org_slug =
            std::env::var("DEFAULT_ORG_SLUG").unwrap_or_else(|_| "default".into());
        let default_org_name =
            std::env::var("DEFAULT_ORG_NAME").unwrap_or_else(|_| "Default".into());

        let cookie_secret =
            std::env::var("COOKIE_SECRET_BASE64").context("COOKIE_SECRET_BASE64 missing")?;
        let secret_bytes = STANDARD
            .decode(cookie_secret.trim())
            .context("invalid COOKIE_SECRET_BASE64")?;
        if secret_bytes.len() < 32 {
            return Err(anyhow!(
                "COOKIE_SECRET_BASE64 must decode to at least 32 bytes"
            ));
        }
        let cookie_key = Key::from(&secret_bytes[..32]);

        let cors_allowed_origins = std::env::var("CORS_ALLOWED_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:5173".into())
            .split(',')
            .filter_map(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .collect::<Vec<_>>();

        let providers_list = std::env::var("AUTH_PROVIDERS").unwrap_or_default();
        let mut providers = HashMap::new();
        for raw in providers_list.split(',') {
            let id = raw.trim();
            if id.is_empty() {
                continue;
            }
            let upper = id.to_ascii_uppercase();
            let issuer = env_required(&format!("{}_ISSUER", upper))?;
            let client_id = env_required(&format!("{}_CLIENT_ID", upper))?;
            let client_secret = env_required(&format!("{}_CLIENT_SECRET", upper))?;
            let redirect_url = env_required(&format!("{}_REDIRECT_URL", upper))?;
            providers.insert(
                id.to_string(),
                ProviderConfig {
                    id: id.to_string(),
                    issuer,
                    client_id,
                    client_secret,
                    redirect_url,
                },
            );
        }

        Ok(Self {
            single_tenant,
            default_org_slug,
            default_org_name,
            cookie_key,
            cors_allowed_origins,
            providers,
        })
    }
}

fn env_required(key: &str) -> Result<String> {
    std::env::var(key).map_err(|_| anyhow!("missing env {}", key))
}
