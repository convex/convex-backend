use std::{
    sync::LazyLock,
    time::{
        Duration,
        SystemTime,
    },
};

use aws_config::default_provider::credentials::DefaultCredentialsChain;
use aws_credential_types::{
    provider::{
        self,
        error::CredentialsError,
        future,
        ProvideCredentials,
        SharedCredentialsProvider,
    },
    Credentials,
};
use aws_smithy_runtime::expiring_cache::ExpiringCache;
use cmd_util::env::env_config;
use tokio::sync::OnceCell;

/// The minimum lifetime of a credential returned by the
/// CachingCredentialProvider. The default value is sufficient to cover the
/// 15-minute RDS token lifetime, as well as the 10 minutes required for
/// presigned upload URLs used by the Node executor.
///
/// If the underlying credential provider returns credentials with a shorter
/// lifetime than this margin, they're returned to the caller and refreshed
/// again on the next request. Keep the margin below the credentials' lifetime
/// to allow reuse between requests.
static REFRESH_MARGIN: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("AWS_CREDENTIALS_REFRESH_MARGIN_SECS", 20 * 60))
});

/// How long credentials without an expiry (e.g. static access keys) are cached
/// before being resolved again. The default matches the AWS SDK's identity
/// cache.
static NON_EXPIRING_TTL: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("AWS_CREDENTIALS_NON_EXPIRING_TTL_SECS", 15 * 60))
});

/// Bounds loading credentials. The default is close to the AWS SDK identity
/// cache's (about 22s with standard retries).
static LOAD_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("AWS_CREDENTIALS_LOAD_TIMEOUT_SECS", 22)));

static SHARED_CREDENTIALS: OnceCell<SharedCredentialsProvider> = OnceCell::const_new();

/// The default credentials chain, but with a process-wide cache.
pub(crate) async fn shared_credentials() -> SharedCredentialsProvider {
    SHARED_CREDENTIALS
        .get_or_init(|| async {
            SharedCredentialsProvider::new(CachingCredentialsProvider::new(
                DefaultCredentialsChain::builder().build().await,
                *REFRESH_MARGIN,
                *NON_EXPIRING_TTL,
                *LOAD_TIMEOUT,
            ))
        })
        .await
        .clone()
}

/// Caches the credentials from `inner` until `refresh_margin` before they
/// expire. Concurrent callers share a single refresh.
#[derive(Debug)]
struct CachingCredentialsProvider<P> {
    inner: P,
    cache: ExpiringCache<Credentials, CredentialsError>,
    refresh_margin: Duration,
    non_expiring_ttl: Duration,
    load_timeout: Duration,
}

impl<P: ProvideCredentials> CachingCredentialsProvider<P> {
    fn new(
        inner: P,
        refresh_margin: Duration,
        non_expiring_ttl: Duration,
        load_timeout: Duration,
    ) -> Self {
        Self {
            inner,
            cache: ExpiringCache::new(refresh_margin),
            refresh_margin,
            non_expiring_ttl,
            load_timeout,
        }
    }

    async fn credentials(&self, now: SystemTime) -> provider::Result {
        if let Some(credentials) = self.cache.yield_or_clear_if_expired(now).await {
            return Ok(credentials);
        }
        self.cache
            .get_or_load(|| async {
                let credentials =
                    match tokio::time::timeout(self.load_timeout, self.inner.provide_credentials())
                        .await
                    {
                        Ok(result) => result?,
                        Err(_) => self.inner.fallback_on_interrupt().ok_or_else(|| {
                            CredentialsError::provider_timed_out(self.load_timeout)
                        })?,
                    };
                // Compensate for the cache's early refresh so static credentials
                // are reused for the full TTL from the start of this request.
                let expiry = credentials
                    .expiry()
                    .unwrap_or(now + self.non_expiring_ttl + self.refresh_margin);
                Ok((credentials, expiry))
            })
            .await
    }
}

impl<P: ProvideCredentials> ProvideCredentials for CachingCredentialsProvider<P> {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(self.credentials(SystemTime::now()))
    }

    fn fallback_on_interrupt(&self) -> Option<Credentials> {
        self.inner.fallback_on_interrupt()
    }
}
