use std::time::Duration;

use common::runtime::Runtime;
use metrics::{
    log_counter,
    log_counter_with_labels,
    log_distribution_with_labels,
    register_convex_counter,
    register_convex_histogram,
    StaticMetricLabel,
    StatusTimer,
    STATUS_LABEL,
};
use strum::IntoStaticStr;
use tokio::time::Instant;

register_convex_histogram!(
    SCHEMA_VALIDATION_TIMER_SECONDS,
    "Time taken to validate a schema",
    &STATUS_LABEL
);
pub fn schema_validation_timer() -> StatusTimer {
    StatusTimer::new(&SCHEMA_VALIDATION_TIMER_SECONDS)
}

register_convex_counter!(
    SCHEMA_VALIDATION_DOCUMENTS_VALIDATED_TOTAL,
    "Number of documents validated against a schem"
);
pub fn log_document_validated() {
    log_counter(&SCHEMA_VALIDATION_DOCUMENTS_VALIDATED_TOTAL, 1);
}

register_convex_counter!(
    SCHEMA_VALIDATION_DOCUMENT_BYTES,
    "Total bytes of documents validated against a schema"
);
pub fn log_document_bytes(bytes: usize) {
    log_counter(&SCHEMA_VALIDATION_DOCUMENT_BYTES, bytes as u64);
}

/// Which kind of validation a table walk serves. `Pending` walks gate a
/// schema push; `Staged` walks run in the background for staged validators of
/// the active schema.
#[derive(Clone, Copy, Debug, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum WalkKind {
    Pending,
    Staged,
}

impl WalkKind {
    fn label(self) -> StaticMetricLabel {
        StaticMetricLabel::new("kind", <&'static str>::from(self))
    }
}

register_convex_histogram!(
    SCHEMA_VALIDATION_WALK_TS_LAG_SECONDS,
    "How far a schema walk's last page timestamp is ahead of its starting timestamp",
    &["kind"]
);
pub fn log_walk_ts_lag(kind: WalkKind, lag: Duration) {
    log_distribution_with_labels(
        &SCHEMA_VALIDATION_WALK_TS_LAG_SECONDS,
        lag.as_secs_f64(),
        vec![kind.label()],
    );
}

register_convex_histogram!(
    SCHEMA_VALIDATION_TABLE_WALK_SECONDS,
    "Wall time of one table walk, from the first page fetch to its exit, whether it finished, \
     found a violation, was cancelled or failed. Divide the `_documents` histogram's sum by this \
     one's sum for documents per second",
    &["kind"]
);
register_convex_histogram!(
    SCHEMA_VALIDATION_TABLE_WALK_DOCUMENTS,
    "Documents validated by one table walk, including walks that exited early",
    &["kind"]
);

/// Records one sample in each table walk histogram when dropped, so every exit
/// path of a walk is counted.
pub struct TableWalkMetrics<RT: Runtime> {
    runtime: RT,
    kind: WalkKind,
    start: Instant,
    pub docs: u64,
}

impl<RT: Runtime> TableWalkMetrics<RT> {
    pub fn start(runtime: RT, kind: WalkKind) -> Self {
        let start = runtime.monotonic_now();
        Self {
            runtime,
            kind,
            start,
            docs: 0,
        }
    }
}

impl<RT: Runtime> Drop for TableWalkMetrics<RT> {
    fn drop(&mut self) {
        let elapsed = self.runtime.monotonic_now() - self.start;
        log_distribution_with_labels(
            &SCHEMA_VALIDATION_TABLE_WALK_SECONDS,
            elapsed.as_secs_f64(),
            vec![self.kind.label()],
        );
        log_distribution_with_labels(
            &SCHEMA_VALIDATION_TABLE_WALK_DOCUMENTS,
            self.docs as f64,
            vec![self.kind.label()],
        );
    }
}

register_convex_counter!(
    SCHEMA_VALIDATION_WALKS_STARTED_TOTAL,
    "Number of table walks started by the schema worker",
    &["kind"]
);
pub fn log_walk_started(kind: WalkKind) {
    log_counter_with_labels(
        &SCHEMA_VALIDATION_WALKS_STARTED_TOTAL,
        1,
        vec![kind.label()],
    );
}

/// How one staged validator's validation attempt resolved. Pending
/// (push-gating) validations are covered by the status label on
/// `schema_validation_timer_seconds` instead.
#[derive(Clone, Copy, Debug, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum StagedOutcome {
    /// The table has never been written, so the validator holds vacuously.
    ValidUnwrittenTable,
    /// The table's shape proved every document conforms without a walk.
    ValidByShape,
    /// A walk checked every document and found no violation.
    ValidByWalk,
    /// A walk found a document that violates the validator.
    Invalid,
    /// The attempt was superseded or invalidated before it resolved.
    Canceled,
}

register_convex_counter!(
    SCHEMA_VALIDATION_STAGED_OUTCOMES_TOTAL,
    "Number of staged validator attempts resolved by the schema worker, by outcome",
    &["outcome"]
);
pub fn log_staged_outcome(outcome: StagedOutcome) {
    log_counter_with_labels(
        &SCHEMA_VALIDATION_STAGED_OUTCOMES_TOTAL,
        1,
        vec![StaticMetricLabel::new(
            "outcome",
            <&'static str>::from(outcome),
        )],
    );
}

/// Logs `outcome` when the progress write that resolves the attempt applied,
/// and `Canceled` when the attempt was already superseded or resolved.
pub fn log_staged_resolution(applied: bool, outcome: StagedOutcome) {
    log_staged_outcome(if applied {
        outcome
    } else {
        StagedOutcome::Canceled
    });
}
