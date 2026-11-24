use std::{
    future::Future,
    time::Duration,
};

use common::{
    backoff::Backoff,
    errors::{
        database_operational_error,
        database_timeout_error,
        is_transient_db_error,
    },
    knobs::{
        MYSQL_CONNECT_INITIAL_BACKOFF,
        MYSQL_CONNECT_MAX_BACKOFF,
        MYSQL_MAX_CONCURRENT_CONNECTS,
    },
};
use parking_lot::Mutex;
use tokio::{
    sync::Semaphore,
    time::{
        sleep_until,
        timeout_at,
        Instant,
    },
};

use crate::metrics::log_connect_backoff_rejected;

/// Rate-limits new connections across a pool: after a failed connect, no
/// attempt starts until a jittered exponential backoff has passed, and at most
/// `in_flight` attempts run at once.
pub(crate) struct ConnectionRateLimiter {
    in_flight: Semaphore,
    state: Mutex<RateLimiterState>,
}

struct RateLimiterState {
    backoff: Backoff,
    next_attempt: Instant,
    last_error: Option<String>,
}

impl ConnectionRateLimiter {
    pub(crate) fn new() -> Self {
        Self::with_limits(
            *MYSQL_MAX_CONCURRENT_CONNECTS,
            *MYSQL_CONNECT_INITIAL_BACKOFF,
            *MYSQL_CONNECT_MAX_BACKOFF,
        )
    }

    fn with_limits(
        max_concurrent_connects: usize,
        initial_backoff: Duration,
        max_backoff: Duration,
    ) -> Self {
        Self {
            in_flight: Semaphore::new(max_concurrent_connects),
            state: Mutex::new(RateLimiterState {
                backoff: Backoff::new(initial_backoff, max_backoff),
                next_attempt: Instant::now(),
                last_error: None,
            }),
        }
    }

    /// Retries transient errors until `deadline`, failing without an attempt
    /// if the backoff ends after it. An attempt still running at `deadline` is
    /// cancelled.
    pub(crate) async fn run<T, Fut: Future<Output = anyhow::Result<T>>>(
        &self,
        cluster_name: &str,
        deadline: Instant,
        mut attempt: impl FnMut() -> Fut,
    ) -> anyhow::Result<T> {
        loop {
            let next_attempt = self.next_attempt(cluster_name, deadline)?;
            sleep_until(next_attempt).await;
            let Ok(permit) = timeout_at(deadline, self.in_flight.acquire()).await else {
                return Err(database_timeout_error("MySQL"));
            };
            let _permit = permit.expect("ConnectionRateLimiter semaphore is never closed");
            // Another attempt may have failed while this one waited.
            if self.state.lock().next_attempt > Instant::now() {
                continue;
            }
            let Ok(result) = timeout_at(deadline, attempt()).await else {
                // Not a failure: a caller that queued for a pool permit can
                // reach its deadline during a healthy connect. Stalled connects
                // fail inside `attempt` at `MYSQL_CONNECT_TIMEOUT`.
                return Err(database_timeout_error("MySQL"));
            };
            match result {
                Ok(result) => {
                    self.record_success(cluster_name);
                    return Ok(result);
                },
                Err(e) => {
                    self.record_failure(cluster_name, &e);
                    if !is_transient_db_error(&e) || Instant::now() >= deadline {
                        return Err(e);
                    }
                },
            }
        }
    }

    fn next_attempt(&self, cluster_name: &str, deadline: Instant) -> anyhow::Result<Instant> {
        let state = self.state.lock();
        let failures = state.backoff.failures();
        if failures > 0 && state.next_attempt >= deadline {
            log_connect_backoff_rejected(cluster_name);
            return Err(database_operational_error(anyhow::anyhow!(
                "Not connecting to MySQL for another {:?} after {failures} consecutive connect \
                 failures; last error: {}",
                state.next_attempt.saturating_duration_since(Instant::now()),
                state.last_error.as_deref().unwrap_or("unknown"),
            )));
        }
        Ok(state.next_attempt)
    }

    fn record_success(&self, cluster_name: &str) {
        let mut state = self.state.lock();
        let failures = state.backoff.failures();
        if failures > 0 {
            tracing::info!(
                "MySQL connects to {cluster_name} recovered after {failures} consecutive failures"
            );
        }
        state.backoff.reset();
        state.next_attempt = Instant::now();
        state.last_error = None;
    }

    fn record_failure(&self, cluster_name: &str, error: &anyhow::Error) {
        let mut state = self.state.lock();
        if state.backoff.failures() == 0 {
            tracing::warn!("Backing off MySQL connects to {cluster_name}: {error:#}");
        }
        let delay = state.backoff.fail(&mut rand::rng());
        state.next_attempt = Instant::now() + delay;
        state.last_error = Some(format!("{error:#}"));
    }
}
