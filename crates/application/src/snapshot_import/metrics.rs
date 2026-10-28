use std::time::Duration;

use errors::ErrorMetadataAnyhowExt;
use metrics::{
    log_counter_with_labels,
    log_distribution,
    register_convex_counter,
    register_convex_histogram,
    StaticMetricLabel,
    StatusTimer,
    STATUS_LABEL,
};

register_convex_histogram!(
    SNAPSHOT_IMPORT_TIMER_SECONDS,
    "Time taken for a snapshot import",
    &STATUS_LABEL
);
pub fn snapshot_import_timer() -> StatusTimer {
    StatusTimer::new(&SNAPSHOT_IMPORT_TIMER_SECONDS)
}

register_convex_histogram!(
    SNAPSHOT_IMPORT_AGE_SECONDS,
    "Age of in-progress snapshot import",
);
pub fn log_snapshot_import_age(age: Duration) {
    log_distribution(&SNAPSHOT_IMPORT_AGE_SECONDS, age.as_secs_f64());
}

register_convex_counter!(
    SNAPSHOT_IMPORT_FAILED_TOTAL,
    "Number of times the snapshot import worker died",
    &["status"]
);
pub fn log_snapshot_import_failed(e: &anyhow::Error) {
    let status = e.metric_status_label_value();
    log_counter_with_labels(
        &SNAPSHOT_IMPORT_FAILED_TOTAL,
        1,
        vec![StaticMetricLabel::new("status", status)],
    );
}

register_convex_counter!(
    SNAPSHOT_IMPORT_LEGACY_GENERATED_SCHEMA_TOTAL,
    "Number of times snapshot import encountered a non-uniform generated_schema.jsonl"
);
pub fn log_snapshot_import_found_legacy_generated_schema() {
    SNAPSHOT_IMPORT_LEGACY_GENERATED_SCHEMA_TOTAL.inc();
}
