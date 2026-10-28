//! Limits for V6 replay and read comparisons.
use std::{
    sync::LazyLock,
    time::Duration,
};

use anyhow::Context as _;
use cmd_util::env::env_config;
use url::Url;

pub(super) fn parse_url(url: &str) -> anyhow::Result<(Url, String)> {
    let parsed: Url = url.parse().context("invalid V6 replica URL")?;
    let db_name = parsed.path().trim_start_matches('/').to_owned();
    anyhow::ensure!(
        !db_name.is_empty(),
        "the V6 replica URL must name a database"
    );
    Ok((parsed, db_name))
}

#[derive(Clone)]
pub(super) struct Settings {
    /// Shared by comparisons, backfill waits, shutdown, and startup's wait
    /// for the previous writer to set clean_shutdown.
    pub(super) apply_wait: Duration,
    /// Queue overflow retires the replica to bound memory use.
    pub(super) max_queued_jobs: usize,
    /// Retry budget per operation; exhaustion retires the replica.
    pub(super) job_retry: Duration,
    /// Deployment loading waits for replica initialization.
    pub(super) init_timeout: Duration,
    /// Fraction of V5 index scans selected before eligibility checks.
    pub(super) comparison_sample_rate: f64,
    pub(super) max_comparison_entries: usize,
    /// Includes source scans still collecting entries for comparison.
    pub(super) max_concurrent_comparisons: usize,
}

/// Fraction of V5 index scans compared against the V6 replica, settable per
/// conductor. Values outside `0..=1` compare every scan.
pub(super) static MYSQL_V6_REPLICA_COMPARE_SAMPLE_RATE: LazyLock<f64> = LazyLock::new(|| {
    let rate: f64 = env_config("MYSQL_V6_REPLICA_COMPARE_SAMPLE_RATE", 1.0);
    if rate.is_finite() && (0.0..=1.0).contains(&rate) {
        rate
    } else {
        tracing::warn!("Invalid MYSQL_V6_REPLICA_COMPARE_SAMPLE_RATE {rate}; comparing every scan");
        1.0
    }
});

/// The sample rate is the one knob; the limits are constants until operating
/// the replica shows which of them need tuning.
pub(super) static SETTINGS: LazyLock<Settings> = LazyLock::new(|| Settings {
    apply_wait: Duration::from_secs(2),
    max_queued_jobs: 1024,
    job_retry: Duration::from_secs(30),
    init_timeout: Duration::from_secs(10),
    comparison_sample_rate: *MYSQL_V6_REPLICA_COMPARE_SAMPLE_RATE,
    max_comparison_entries: 128,
    max_concurrent_comparisons: 4,
});
