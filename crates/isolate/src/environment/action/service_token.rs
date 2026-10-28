use std::time::{
    Duration,
    SystemTime,
};

use ai_gateway_jwt::unverified_expiration;
use anyhow::{
    ensure,
    Context,
};
use common::knobs::GET_SERVICE_TOKEN_GUARANTEED_LIFETIME;

#[derive(Clone)]
struct CachedServiceToken {
    token: String,
    expires_at: SystemTime,
}

impl CachedServiceToken {
    fn is_reusable(&self, now: SystemTime, guaranteed_lifetime: Duration) -> bool {
        self.expires_at
            .duration_since(now)
            .is_ok_and(|remaining| remaining >= guaranteed_lifetime)
    }
}

pub(crate) struct ServiceTokenCache {
    cached: tokio::sync::Mutex<Option<CachedServiceToken>>,
    guaranteed_lifetime: Duration,
}

impl Default for ServiceTokenCache {
    fn default() -> Self {
        Self::new(*GET_SERVICE_TOKEN_GUARANTEED_LIFETIME)
    }
}

impl ServiceTokenCache {
    fn new(guaranteed_lifetime: Duration) -> Self {
        Self {
            cached: tokio::sync::Mutex::new(None),
            guaranteed_lifetime,
        }
    }

    /// The lock stays held while minting so concurrent refreshes share the
    /// result. A failed mint releases the lock without replacing the cache.
    pub(crate) async fn get_or_mint<N, F, Fut>(&self, now: N, mint: F) -> anyhow::Result<String>
    where
        N: Fn() -> SystemTime,
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<String>>,
    {
        let mut cached = self.cached.lock().await;
        if let Some(token) = cached.as_ref()
            && token.is_reusable(now(), self.guaranteed_lifetime)
        {
            return Ok(token.token.clone());
        }

        let token = mint().await?;
        let expires_at = unverified_expiration(&token)
            .context("Newly minted service token is not a valid JWT")?;
        let remaining_lifetime = expires_at
            .duration_since(now())
            .context("Newly minted service token is already expired")?;
        ensure!(
            remaining_lifetime >= self.guaranteed_lifetime,
            "Newly minted service token does not satisfy the guaranteed lifetime"
        );
        *cached = Some(CachedServiceToken {
            token: token.clone(),
            expires_at,
        });
        Ok(token)
    }
}
