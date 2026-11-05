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
    /// Shared by comparisons, backfill waits, and shutdown's wait for
    /// in-flight source calls.
    pub(super) apply_wait: Duration,
    /// Shutdown's budget to drain the queue and write the clean-shutdown
    /// certificate once source calls have finished. With `apply_wait` for
    /// those calls, the total stays under Nomad's 15 s kill timeout.
    pub(super) shutdown_wait: Duration,
    /// Queue overflow retires the replica to bound memory use. A pending
    /// writer applies nothing until its predecessor certifies, so the queue
    /// must hold the busiest deployment's commits for that window.
    pub(super) max_queued_jobs: usize,
    /// Consecutive write jobs one V6 transaction may replay, bounding the work
    /// a transient failure retries and the locks a transaction holds.
    pub(super) max_merged_writes: usize,
    /// Retry budget per operation; exhaustion retires the replica.
    pub(super) job_retry: Duration,
    /// Deployment loading waits for replica initialization.
    pub(super) init_timeout: Duration,
    /// How long a new writer queues its writes while waiting for the previous
    /// writer's clean-shutdown certificate before comparing the replica with
    /// the source instead. A rolling update stops the old
    /// conductor within seconds of the new one taking the lease; a deployment
    /// move waits for the source conductor's next reconcile, up to a minute.
    pub(super) handoff_grace: Duration,
    /// Fraction of V5 reads selected before eligibility checks.
    pub(super) comparison_sample_rate: f64,
    /// Entries captured per sampled read; a longer source result compares as a
    /// prefix.
    pub(super) max_comparison_entries: usize,
    /// Sampled reads capturing entries or awaiting a verdict; bounds capture
    /// memory. Reads sampled beyond it are dropped.
    pub(super) max_concurrent_comparisons: usize,
    /// V6 reads in flight for comparisons. A comparison waits for a slot until
    /// its `apply_wait` deadline, which the replay wait shares, then is
    /// dropped.
    pub(super) max_concurrent_replica_reads: usize,
}

/// Fraction of V5 reads (index scans, previous revisions, revision pairs)
/// compared against the V6 replica, settable per conductor. Values outside
/// `0..=1` compare every read.
pub(super) static MYSQL_V6_REPLICA_COMPARE_SAMPLE_RATE: LazyLock<f64> = LazyLock::new(|| {
    let rate: f64 = env_config("MYSQL_V6_REPLICA_COMPARE_SAMPLE_RATE", 1.0);
    if rate.is_finite() && (0.0..=1.0).contains(&rate) {
        rate
    } else {
        tracing::warn!("Invalid MYSQL_V6_REPLICA_COMPARE_SAMPLE_RATE {rate}; comparing every scan");
        1.0
    }
});

/// V6 reads in flight for comparisons per deployment, settable per conductor.
/// Zero makes every comparison wait out its deadline and drop; use the sample
/// rate to turn comparisons off.
pub(super) static MYSQL_V6_REPLICA_MAX_CONCURRENT_REPLICA_READS: LazyLock<usize> =
    LazyLock::new(|| env_config("MYSQL_V6_REPLICA_MAX_CONCURRENT_REPLICA_READS", 8));

/// The sample rate and the replica read slots are knobs; the other limits are
/// constants until operating the replica shows which of them need tuning.
pub(super) static SETTINGS: LazyLock<Settings> = LazyLock::new(|| Settings {
    apply_wait: Duration::from_secs(2),
    shutdown_wait: Duration::from_secs(10),
    max_queued_jobs: 4096,
    max_merged_writes: 64,
    job_retry: Duration::from_secs(30),
    init_timeout: Duration::from_secs(10),
    handoff_grace: Duration::from_secs(120),
    comparison_sample_rate: *MYSQL_V6_REPLICA_COMPARE_SAMPLE_RATE,
    max_comparison_entries: 128,
    max_concurrent_comparisons: 64,
    max_concurrent_replica_reads: *MYSQL_V6_REPLICA_MAX_CONCURRENT_REPLICA_READS,
});
