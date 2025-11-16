use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use openidconnect::core::{
    CoreClient, CoreIdTokenClaims, CoreIdTokenVerifier, CoreProviderMetadata, CoreResponseType,
};
use openidconnect::reqwest::async_http_client;
use openidconnect::{
    AuthorizationCode, AuthenticationFlow, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug)]
pub struct ProviderConfig {
    pub id: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
}

#[derive(Clone, Debug)]
pub struct AuthRegistry {
    providers: HashMap<String, OidcProvider>,
}

impl AuthRegistry {
    pub async fn from_config(configs: &HashMap<String, ProviderConfig>) -> Result<Self> {
        let mut providers = HashMap::new();
        for (id, cfg) in configs {
            let provider = OidcProvider::discover(cfg.clone()).await?;
            providers.insert(id.clone(), provider);
        }
        Ok(Self { providers })
    }

    pub fn get(&self, id: &str) -> Option<&OidcProvider> {
        self.providers.get(id)
    }
}

#[derive(Clone, Debug)]
pub struct OidcProvider {
    pub id: String,
    client: CoreClient,
}

impl OidcProvider {
    async fn discover(cfg: ProviderConfig) -> Result<Self> {
        let issuer = IssuerUrl::new(cfg.issuer.clone())?;
        let metadata = CoreProviderMetadata::discover_async(issuer, async_http_client)
            .await
            .context("discover provider metadata")?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(cfg.client_id.clone()),
            Some(ClientSecret::new(cfg.client_secret.clone())),
        )
        .set_redirect_uri(RedirectUrl::new(cfg.redirect_url.clone())?);
        Ok(Self { id: cfg.id, client })
    }

    pub fn authorize(&self) -> Result<AuthUrl> {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let (auth_url, csrf_token, nonce) = self
            .client
            .authorize_url(
                AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("openid".to_string()))
            .add_scope(Scope::new("email".to_string()))
            .add_scope(Scope::new("profile".to_string()))
            .set_pkce_challenge(pkce_challenge)
            .url();
        Ok(AuthUrl {
            url: auth_url,
            pkce_verifier,
            nonce,
            csrf: csrf_token,
        })
    }

    pub async fn exchange(
        &self,
        code: AuthorizationCode,
        verifier: PkceCodeVerifier,
        nonce: Nonce,
    ) -> Result<OidcUserInfo> {
        let token_response = self
            .client
            .exchange_code(code)
            .set_pkce_verifier(verifier)
            .request_async(async_http_client)
            .await
            .context("token exchange failed")?;
        let id_token = token_response
            .id_token()
            .ok_or_else(|| anyhow!("missing id_token"))?;
        let claims = id_token
            .claims(&self.id_token_verifier(), &nonce)
            .context("invalid id token claims")?;
        Ok(claims_to_user(claims))
    }

    fn id_token_verifier(&self) -> CoreIdTokenVerifier<'_> {
        self.client.id_token_verifier()
    }
}

#[derive(Debug)]
pub struct AuthUrl {
    pub url: Url,
    pub pkce_verifier: PkceCodeVerifier,
    pub nonce: Nonce,
    pub csrf: CsrfToken,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TempLoginState {
    pub provider: String,
    pub csrf: String,
    pub nonce: String,
    pub pkce_verifier: String,
}

impl TempLoginState {
    pub fn random(provider: &str, auth: &AuthUrl) -> Self {
        Self {
            provider: provider.to_string(),
            csrf: auth.csrf.secret().to_string(),
            nonce: auth.nonce.secret().to_string(),
            pkce_verifier: auth.pkce_verifier.secret().to_string(),
        }
    }

    pub fn verifier(&self) -> PkceCodeVerifier {
        PkceCodeVerifier::new(self.pkce_verifier.clone())
    }

    pub fn nonce(&self) -> Nonce {
        Nonce::new(self.nonce.clone())
    }

    pub fn csrf(&self) -> CsrfToken {
        CsrfToken::new(self.csrf.clone())
    }
}

#[derive(Clone, Debug)]
pub struct OidcUserInfo {
    pub subject: String,
    pub email: String,
    pub name: Option<String>,
}

fn claims_to_user(claims: &CoreIdTokenClaims) -> OidcUserInfo {
    let email = claims
        .email()
        .map(|addr| addr.as_str().to_string())
        .unwrap_or_else(|| claims.subject().as_str().to_string());
    let name = claims
        .name()
        .and_then(|n| n.get(None).map(|inner| inner.as_str().to_string()));
    OidcUserInfo {
        subject: claims.subject().as_str().to_string(),
        email,
        name,
    }
}

/// Generate a random 32-byte base64 string, handy for cookie secrets/tests.
pub fn random_secret_base64() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    STANDARD.encode(bytes)
}
