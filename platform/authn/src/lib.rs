//! Platform authentication helpers.
//!
//! This crate will host the configurable OIDC + BFF middleware stack. For now we
//! provide lightweight structs to unblock other crates.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthnError {
    #[error("missing provider configuration: {0}")]
    MissingProvider(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderConfig {
    pub issuer_url: String,
    pub client_id: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            issuer_url: "https://example.com".into(),
            client_id: "suite-dev".into(),
        }
    }
}

#[derive(Default, Debug)]
pub struct AuthnService {
    providers: Vec<ProviderConfig>,
}

impl AuthnService {
    pub fn with_provider(mut self, config: ProviderConfig) -> Self {
        self.providers.push(config);
        self
    }

    pub fn provider(&self, issuer_url: &str) -> Result<&ProviderConfig, AuthnError> {
        self.providers
            .iter()
            .find(|cfg| cfg.issuer_url == issuer_url)
            .ok_or_else(|| AuthnError::MissingProvider(issuer_url.into()))
    }
}
