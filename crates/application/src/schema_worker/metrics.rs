use std::time::Duration;

use common::runtime::Runtime;
use metrics::{
    log_counter,
    log_distribution,
    register_convex_counter,
    register_convex_histogram,
    StatusTimer,
    STATUS_LABEL,
};
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

register_convex_histogram!(
    SCHEMA_VALIDATION_WALK_TS_LAG_SECONDS,
    "How far a schema walk's last page timestamp is ahead of its starting timestamp"
);
pub fn log_walk_ts_lag(lag: Duration) {
    log_distribution(&SCHEMA_VALIDATION_WALK_TS_LAG_SECONDS, lag.as_secs_f64());
}

register_convex_histogram!(
    SCHEMA_VALIDATION_TABLE_WALK_SECONDS,
    "Wall time of one table walk, from the first page fetch to its exit, whether it finished, \
     found a violation, was cancelled or failed. Divide the `_documents` histogram's sum by this \
     one's sum for documents per second"
);
register_convex_histogram!(
    SCHEMA_VALIDATION_TABLE_WALK_DOCUMENTS,
    "Documents validated by one table walk, including walks that exited early"
);

/// Records one sample in each table walk histogram when dropped, so every exit
/// path of a walk is counted.
pub struct TableWalkMetrics<RT: Runtime> {
    runtime: RT,
    start: Instant,
    pub docs: u64,
}

impl<RT: Runtime> TableWalkMetrics<RT> {
    pub fn start(runtime: RT) -> Self {
        let start = runtime.monotonic_now();
        Self {
            runtime,
            start,
            docs: 0,
        }
    }
}

impl<RT: Runtime> Drop for TableWalkMetrics<RT> {
    fn drop(&mut self) {
        let elapsed = self.runtime.monotonic_now() - self.start;
        log_distribution(&SCHEMA_VALIDATION_TABLE_WALK_SECONDS, elapsed.as_secs_f64());
        log_distribution(&SCHEMA_VALIDATION_TABLE_WALK_DOCUMENTS, self.docs as f64);
    }
}

register_convex_counter!(
    SCHEMA_VALIDATION_WALKS_STARTED_TOTAL,
    "Number of table walks started by the schema worker"
);
pub fn log_walk_started() {
    log_counter(&SCHEMA_VALIDATION_WALKS_STARTED_TOTAL, 1);
}
