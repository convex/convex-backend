use std::{
    future::Future,
    time::Duration,
};

use crate::backoff::Backoff;

#[derive(Clone, Copy, Debug)]
pub struct RetryConfig {
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub max_attempts: u32,
}

/// Retry `f` with exponential backoff while `should_retry` returns true for
/// the returned error. The caller is responsible for ensuring `f` is
/// idempotent.
pub async fn retry_with_backoff<T, F: Future<Output = anyhow::Result<T>>>(
    name: &'static str,
    config: RetryConfig,
    should_retry: impl Fn(&anyhow::Error) -> bool,
    f: impl Fn() -> F,
) -> anyhow::Result<T> {
    let mut backoff = Backoff::new(config.initial_backoff, config.max_backoff);
    let mut attempt = 1u32;
    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) if attempt < config.max_attempts && should_retry(&e) => {
                let delay = backoff.fail(&mut rand::rng());
                tracing::warn!(
                    "retriable error in {name} (attempt {attempt}/{}), retrying in {delay:?}: \
                     {e:#}",
                    config.max_attempts,
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            },
            Err(e) => return Err(e),
        }
    }
}
