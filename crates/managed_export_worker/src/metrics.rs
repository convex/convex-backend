use common::types::Timestamp;
use errors::ErrorMetadataAnyhowExt as _;
use metrics::{
    log_counter_with_labels,
    register_convex_counter,
    register_convex_histogram,
    StaticMetricLabel,
    StatusTimer,
    STATUS_LABEL,
};
use streaming_export::SyncStatus;

register_convex_histogram!(
    S3_EXPORT_SYNC_DURATION_SECONDS,
    "Duration of S3 export provider sync calls, including service discovery and connection setup",
    &STATUS_LABEL
);
pub fn s3_export_sync_timer() -> StatusTimer {
    StatusTimer::new(&S3_EXPORT_SYNC_DURATION_SECONDS)
}

register_convex_counter!(
    S3_EXPORT_WORKER_FAILED_TOTAL,
    "Number of failed S3 export worker iterations, including sync and cursor persistence failures",
    &STATUS_LABEL
);
pub fn log_s3_export_worker_failed(e: &anyhow::Error) {
    log_counter_with_labels(
        &S3_EXPORT_WORKER_FAILED_TOTAL,
        1,
        vec![StaticMetricLabel::new(
            "status",
            e.metric_status_label_value(),
        )],
    );
}

register_convex_histogram!(
    S3_EXPORT_LAG_SECONDS,
    "Lag between the latest database timestamp and the consistent snapshot exported by an S3 sync \
     page",
);

register_convex_histogram!(
    S3_EXPORT_SNAPSHOT_SYNCED_TABLES,
    "Tables synced in the initial snapshot, observed after each S3 export snapshot page",
);

pub fn log_s3_export_progress(status: &SyncStatus, latest_ts: Timestamp) {
    match status {
        SyncStatus::Snapshotting { progress } => metrics::log_distribution(
            &S3_EXPORT_SNAPSHOT_SYNCED_TABLES,
            progress.num_tables_synced as f64,
        ),
        SyncStatus::Stale { ts } | SyncStatus::UpToDate { ts } => metrics::log_distribution(
            &S3_EXPORT_LAG_SECONDS,
            latest_ts.secs_since_f64(*ts).max(0.0),
        ),
    }
}
