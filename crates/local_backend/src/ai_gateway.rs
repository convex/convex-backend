use std::{
    sync::Arc,
    time::{
        Duration,
        SystemTime,
    },
};

use ai_gateway_jwt::unverified_expiration;
use anyhow::{
    ensure,
    Context,
};
use application::ai_gateway_jwt::AiGatewayJwtMinter;
use async_trait::async_trait;
use big_brain_client::BigBrainClient;
use common::{
    execution_context::RequestId,
    knobs::{
        GET_SERVICE_TOKEN_GUARANTEED_LIFETIME,
        SERVICE_TOKEN_CACHE_CAPACITY,
    },
    types::{
        AttributionClaims,
        DeploymentMetadata,
    },
};
use errors::ErrorMetadata;
use moka::sync::Cache;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TokenCacheKey {
    deployment_name: String,
    attribution: AttributionClaims,
}

#[derive(Clone)]
struct CachedToken {
    token: String,
    expires_at: SystemTime,
}

impl CachedToken {
    fn is_reusable(&self, now: SystemTime, guaranteed_lifetime: Duration) -> bool {
        self.expires_at
            .duration_since(now)
            .is_ok_and(|remaining| remaining >= guaranteed_lifetime)
    }
}

pub struct LocalAiGatewayTokenMinter {
    client: Option<BigBrainClient>,
    token_cache: Cache<TokenCacheKey, Arc<Mutex<Option<CachedToken>>>>,
    guaranteed_lifetime: Duration,
}

impl LocalAiGatewayTokenMinter {
    pub fn new(control_plane_url: String, access_token: Option<String>) -> Self {
        Self::new_with_guaranteed_lifetime(
            control_plane_url,
            access_token,
            *GET_SERVICE_TOKEN_GUARANTEED_LIFETIME,
        )
    }

    fn new_with_guaranteed_lifetime(
        control_plane_url: String,
        access_token: Option<String>,
        guaranteed_lifetime: Duration,
    ) -> Self {
        Self {
            client: access_token
                .map(|access_token| BigBrainClient::new(control_plane_url, access_token)),
            token_cache: Cache::builder()
                .max_capacity(*SERVICE_TOKEN_CACHE_CAPACITY)
                .build(),
            guaranteed_lifetime,
        }
    }
}

#[async_trait]
impl AiGatewayJwtMinter for LocalAiGatewayTokenMinter {
    async fn mint(
        &self,
        deployment: &DeploymentMetadata,
        attribution: AttributionClaims,
        _request_id: &RequestId,
    ) -> anyhow::Result<String> {
        let client = self.client.as_ref().ok_or_else(|| {
            anyhow::anyhow!(ErrorMetadata::bad_request(
                "AiGatewayUnavailable",
                "`getServiceToken(\"ai-gateway\")` requires an authenticated local deployment. \
                 Run `npx convex login`, then restart `npx convex dev`.",
            ))
        })?;
        let cache_key = TokenCacheKey {
            deployment_name: deployment.name.clone(),
            attribution: attribution.clone(),
        };
        let cache_entry = self
            .token_cache
            .get_with(cache_key, || Arc::new(Mutex::new(None)));
        let mut cached_token = cache_entry.lock().await;
        if let Some(token) = cached_token.as_ref()
            && token.is_reusable(SystemTime::now(), self.guaranteed_lifetime)
        {
            return Ok(token.token.clone());
        }

        let response = client
            .mint_local_ai_gateway_jwt(deployment.name.clone(), attribution)
            .await?;
        let expires_at = unverified_expiration(&response.token)
            .context("Newly minted service token is not a valid AI Gateway JWT")?;
        let remaining_lifetime = expires_at
            .duration_since(SystemTime::now())
            .context("Newly minted JWT is already expired")?;
        ensure!(
            remaining_lifetime >= self.guaranteed_lifetime,
            "Newly minted JWT does not satisfy the guaranteed lifetime"
        );
        let token = response.token;
        *cached_token = Some(CachedToken {
            token: token.clone(),
            expires_at,
        });
        Ok(token)
    }
}
