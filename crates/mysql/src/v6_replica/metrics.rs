//! Replay, read comparison, and writer lifecycle metrics by source cluster.
use metrics::{
    register_convex_counter,
    register_convex_gauge,
    register_convex_histogram,
};

register_convex_counter!(
    MYSQL_V6_REPLICA_JOBS_TOTAL,
    "V6 replica jobs by outcome: applied, failed when the replica rejected or could not apply \
     one, skipped when the replica had already retired",
    &["cluster_name", "outcome"]
);
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::IntoStaticStr, strum::EnumIter)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum JobOutcome {
    Applied,
    Failed,
    Skipped,
}
pub(crate) fn log_jobs(cluster_name: &str, outcome: JobOutcome, count: u64) {
    MYSQL_V6_REPLICA_JOBS_TOTAL
        .with_label_values(&[cluster_name, outcome.into()])
        .inc_by(count);
}

register_convex_histogram!(
    MYSQL_V6_REPLICA_WRITE_BATCH_COMMITS,
    "Source commits replayed by one V6 transaction. Consecutive writes to disjoint index keys \
     merge, so the mean rises with queue depth",
    &["cluster_name"]
);
pub(crate) fn log_write_batch(cluster_name: &str, commits: u64) {
    MYSQL_V6_REPLICA_WRITE_BATCH_COMMITS
        .with_label_values(&[cluster_name])
        .observe(commits as f64);
}

register_convex_counter!(
    MYSQL_V6_REPLICA_RETIREMENTS_TOTAL,
    "V6 replicas retired, by reason",
    &["cluster_name", "reason"]
);
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::IntoStaticStr, strum::EnumIter)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum RetireReason {
    /// A replay failure leaves replica completeness uncertain.
    JobFailed,
    QueueFull,
    /// Restore, import, or migration wrote outside the mirrored path.
    BulkWrite,
    /// The replica may be missing source writes at a writer handoff.
    UncleanHandoff,
    /// The V6 side could not be opened after a resolved handoff.
    InitializationFailed,
}
pub(crate) fn log_retirement(cluster_name: &str, reason: RetireReason) {
    MYSQL_V6_REPLICA_RETIREMENTS_TOTAL
        .with_label_values(&[cluster_name, reason.into()])
        .inc();
}

register_convex_counter!(
    MYSQL_V6_REPLICA_HANDOFFS_TOTAL,
    "Pending writer handoffs by how they resolved",
    &["cluster_name", "outcome"]
);
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::IntoStaticStr, strum::EnumIter)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum HandoffOutcome {
    /// The previous writer certified a clean shutdown.
    Certified,
    /// No certificate arrived, but the replica matched the source as of the
    /// lease change.
    Verified,
    /// No certificate arrived and the replica could not be shown complete.
    Expired,
    /// A newer writer registered first.
    Superseded,
    /// Another process retired the replica.
    Retired,
}
pub(crate) fn log_handoff(cluster_name: &str, outcome: HandoffOutcome) {
    MYSQL_V6_REPLICA_HANDOFFS_TOTAL
        .with_label_values(&[cluster_name, outcome.into()])
        .inc();
}

register_convex_counter!(
    MYSQL_V6_REPLICA_MARKER_READ_FAILURES_TOTAL,
    "Failed reads of a V6 replica's index backfill markers. Retention retries the read, so a \
     failure does not retire the replica",
    &["cluster_name"]
);
pub(crate) fn log_marker_read_failure(cluster_name: &str) {
    MYSQL_V6_REPLICA_MARKER_READ_FAILURES_TOTAL
        .with_label_values(&[cluster_name])
        .inc();
}

register_convex_counter!(
    MYSQL_V6_REPLICA_COMPARISONS_TOTAL,
    "Sampled V5 reads by kind and comparison outcome against the V6 replica",
    &["cluster_name", "read", "outcome"]
);
/// Reads re-run on the replica for comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::IntoStaticStr, strum::EnumIter)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ComparedRead {
    IndexScan,
    PreviousRevisions,
    RevisionPairs,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::IntoStaticStr, strum::EnumIter)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ComparisonOutcome {
    Sampled,
    /// Comparison requires an active replica and, for index scans, a
    /// persistence index ID.
    Ineligible,
    /// No capture slot when sampled, or no replica read slot before the
    /// comparison's `apply_wait` deadline.
    Dropped,
    /// The V5 scan failed while collecting entries.
    ServingError,
    /// Source calls were in flight, replay timed out, or the replica stopped,
    /// changed owners, or retired.
    Invalidated,
    /// Retention invalidated the scan's snapshot.
    Expired,
    /// Reading V6 entries or replica ownership state failed.
    Error,
    Mismatch,
    Match,
}
pub(crate) fn log_comparison(cluster_name: &str, read: ComparedRead, outcome: ComparisonOutcome) {
    MYSQL_V6_REPLICA_COMPARISONS_TOTAL
        .with_label_values(&[cluster_name, read.into(), outcome.into()])
        .inc();
}

register_convex_gauge!(
    MYSQL_V6_REPLICA_WRITERS_TOTAL,
    "V5 writers by replica state",
    &["cluster_name", "state"]
);
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::IntoStaticStr, strum::EnumIter)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum WriterState {
    Ready,
    /// Queueing writes until the previous writer certifies its shutdown.
    Pending,
    Displaced,
    Closed,
    /// The deployment has history the replica never received.
    Ineligible,
    Retired,
    InitializationFailed,
}

register_convex_gauge!(
    MYSQL_V6_REPLICA_QUEUED_JOBS_TOTAL,
    "V6 replica jobs queued in this process and not yet finished",
    &["cluster_name"]
);
pub(crate) fn add_queued(cluster_name: &str, delta: f64) {
    MYSQL_V6_REPLICA_QUEUED_JOBS_TOTAL
        .with_label_values(&[cluster_name])
        .add(delta);
}

/// Each live instance contributes one writer, even when shared through `Arc`.
pub(crate) struct WriterStateGauge {
    cluster_name: String,
    state: parking_lot::Mutex<WriterState>,
}
impl WriterStateGauge {
    pub(crate) fn new(cluster_name: &str, state: WriterState) -> Self {
        let gauge = Self {
            cluster_name: cluster_name.to_owned(),
            state: parking_lot::Mutex::new(state),
        };
        gauge.add(state, 1.0);
        gauge
    }

    fn add(&self, state: WriterState, delta: f64) {
        MYSQL_V6_REPLICA_WRITERS_TOTAL
            .with_label_values(&[self.cluster_name.as_str(), state.into()])
            .add(delta);
    }

    pub(crate) fn set(&self, next: WriterState) {
        let mut state = self.state.lock();
        if *state != next {
            self.add(*state, -1.0);
            self.add(next, 1.0);
            *state = next;
        }
    }
}
impl Drop for WriterStateGauge {
    fn drop(&mut self) {
        let state = *self.state.get_mut();
        self.add(state, -1.0);
    }
}

/// Publishes zeroes so the first event of each series has a baseline.
pub(crate) fn initialize(cluster_name: &str) {
    use strum::IntoEnumIterator as _;
    for outcome in JobOutcome::iter() {
        MYSQL_V6_REPLICA_JOBS_TOTAL
            .with_label_values(&[cluster_name, outcome.into()])
            .inc_by(0);
    }
    for reason in RetireReason::iter() {
        MYSQL_V6_REPLICA_RETIREMENTS_TOTAL
            .with_label_values(&[cluster_name, reason.into()])
            .inc_by(0);
    }
    for outcome in HandoffOutcome::iter() {
        MYSQL_V6_REPLICA_HANDOFFS_TOTAL
            .with_label_values(&[cluster_name, outcome.into()])
            .inc_by(0);
    }
    for read in ComparedRead::iter() {
        for outcome in ComparisonOutcome::iter() {
            MYSQL_V6_REPLICA_COMPARISONS_TOTAL
                .with_label_values(&[cluster_name, read.into(), outcome.into()])
                .inc_by(0);
        }
    }
    MYSQL_V6_REPLICA_MARKER_READ_FAILURES_TOTAL
        .with_label_values(&[cluster_name])
        .inc_by(0);
}
