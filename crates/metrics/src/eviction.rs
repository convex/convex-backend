//! Inactivity-based eviction for high-cardinality metric vectors.
//!
//! Labelled counters and histograms registered via the `register_convex_*`
//! macros are tracked here; gauges opt in via
//! `register_convex_gauge_evictable!`. [`spawn_sweep_task`] runs the
//! periodic sweep.

use std::{
    env,
    sync::{
        Arc,
        LazyLock,
    },
    time::Duration,
};

use parking_lot::Mutex;
use prometheus::{
    core::Evictable,
    timer,
};

use crate::{
    register_convex_counter,
    register_convex_gauge,
};

pub const DEFAULT_TTL: Duration = Duration::from_secs(12 * 60 * 60);
pub const DEFAULT_SWEEP_INTERVAL: Duration = Duration::from_secs(5 * 60);

struct Registered {
    vec: Arc<dyn Evictable>,
    ttl: Duration,
    name: String,
}

static EVICTABLE_METRICS: LazyLock<Mutex<Vec<Registered>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// Called by the `register_convex_*` macros. `name` is used only as a
/// label on the eviction visibility metrics.
pub fn register_evictable(name: &str, vec: Arc<dyn Evictable>, ttl: Duration) {
    // Duration::MAX means "never evict", so skip the list. Also keeps the
    // visibility metrics out of it, which is what lets `sweep_now` touch them
    // under the lock without re-entering here and deadlocking.
    if ttl == Duration::MAX {
        return;
    }
    EVICTABLE_METRICS.lock().push(Registered {
        vec,
        ttl,
        name: name.to_owned(),
    });
}

/// `CONVEX_METRICS_EVICTION_TTL_SECONDS=0` disables eviction.
pub fn ttl_from_env() -> Duration {
    match env::var("CONVEX_METRICS_EVICTION_TTL_SECONDS") {
        Ok(s) => match s.parse::<u64>() {
            Ok(0) => Duration::MAX,
            Ok(secs) => Duration::from_secs(secs),
            Err(e) => {
                tracing::warn!(
                    "CONVEX_METRICS_EVICTION_TTL_SECONDS={s:?} failed to parse ({e}); falling \
                     back to default TTL of {DEFAULT_TTL:?}"
                );
                DEFAULT_TTL
            },
        },
        Err(_) => DEFAULT_TTL,
    }
}

// We need to explicitly set the TTL to Duration::MAX to prevent the eviction
// counter itself being evicted.
register_convex_counter!(
    METRICS_SERIES_EVICTED_TOTAL,
    "Count of metric label-sets dropped by the inactivity sweeper",
    &["metric"],
    Duration::MAX,
);

register_convex_gauge!(
    METRICS_EVICTABLE_CARDINALITY_INFO,
    "Current cardinality of each evictable metric vector",
    &["metric"]
);

/// Returns the number of series removed across all registered vectors.
pub fn sweep_now() -> usize {
    let now_ms = timer::now_millis();
    let mut total = 0;
    // Held across the sweep. Don't increment a finite-TTL labelled metric in
    // here: its first-touch registration re-enters the lock and deadlocks.
    for r in EVICTABLE_METRICS.lock().iter() {
        // Duration::MAX (and any TTL past ~584M years) overflows u64 ms; clamp
        // so an out-of-range TTL yields threshold 0, i.e. never evict.
        let ttl_ms = u64::try_from(r.ttl.as_millis()).unwrap_or(u64::MAX);
        let threshold_ms = now_ms.saturating_sub(ttl_ms);
        let removed = r.vec.evict_stale_before(threshold_ms);
        if removed > 0 {
            METRICS_SERIES_EVICTED_TOTAL
                .with_label_values(&[r.name.as_str()])
                .inc_by(removed as u64);
        }
        METRICS_EVICTABLE_CARDINALITY_INFO
            .with_label_values(&[r.name.as_str()])
            .set(r.vec.cardinality() as f64);
        total += removed;
    }
    total
}

/// Idempotent; only the first call spawns. `None` uses
/// [`DEFAULT_SWEEP_INTERVAL`].
pub fn spawn_sweep_task(interval: Option<Duration>) {
    static STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let interval = interval.unwrap_or(DEFAULT_SWEEP_INTERVAL);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let removed = sweep_now();
            if removed > 0 {
                tracing::debug!(removed, "metrics inactivity sweep complete");
            }
        }
    });
}
