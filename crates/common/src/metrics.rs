use std::time::Duration;

use metrics::{
    log_counter,
    log_counter_with_labels,
    log_distribution,
    log_gauge,
    register_convex_counter,
    register_convex_gauge,
    register_convex_histogram,
    register_convex_int_gauge,
    StaticMetricLabel,
    Timer,
};
use prometheus::VMHistogram;

register_convex_counter!(
    COMMON_UNDEFINED_FILTER_TOTAL,
    "Count of undefined JsonValues filtered"
);
pub fn log_undefined_filter() {
    log_counter(&COMMON_UNDEFINED_FILTER_TOTAL, 1);
}

// Note: These Codel queue gauges are incorrect if the process contains multiple
// queues.
register_convex_gauge!(COMMON_CODEL_QUEUE_LENGTH_TOTAL, "Length of the CoDel queue");
pub fn log_codel_queue_size(size: usize) {
    log_gauge(&COMMON_CODEL_QUEUE_LENGTH_TOTAL, size as f64)
}

register_convex_gauge!(
    COMMON_CODEL_QUEUE_OVERLOADED_TOTAL,
    "1 if the CoDel queue is overloaded, 0 otherwise"
);
pub fn log_codel_queue_overloaded(overloaded: bool) {
    log_gauge(
        &COMMON_CODEL_QUEUE_OVERLOADED_TOTAL,
        if overloaded { 1.0 } else { 0.0 },
    )
}

// static $metric: LazyLock<IntCounter> = LazyLock::new(|| {
// register_int_counter_with_registry!(&*$metricname, $help,
// CONVEX_METRICS_REGISTRY).unwrap()}); ==>> register_convex_counter!($metric,
// $help);
register_convex_histogram!(
    COMMON_CODEL_QUEUE_TIME_SINCE_EMPTY_SECONDS,
    "Time since the CoDel queue was empty"
);

pub fn log_codel_queue_time_since_empty(duration: Duration) {
    log_distribution(
        &COMMON_CODEL_QUEUE_TIME_SINCE_EMPTY_SECONDS,
        duration.as_secs_f64(),
    )
}

register_convex_counter!(
    CLIENT_VERSION_UNSUPPORTED_TOTAL,
    "Count of requests with an unsupported client version",
    &["version"]
);
pub fn log_client_version_unsupported(version: String) {
    log_counter_with_labels(
        &CLIENT_VERSION_UNSUPPORTED_TOTAL,
        1,
        vec![StaticMetricLabel::new("version", version)],
    );
}

register_convex_counter!(ERRORS_REPORTED_TOTAL, "Count of errors reported", &["type"]);
pub fn log_errors_reported_total(tag: StaticMetricLabel) {
    log_counter_with_labels(&ERRORS_REPORTED_TOTAL, 1, vec![tag]);
}

register_convex_histogram!(LOAD_ID_TRACKER_SECONDS, "Time to load IdTracker in seconds");
pub fn load_id_tracker_timer() -> Timer<VMHistogram> {
    Timer::new(&LOAD_ID_TRACKER_SECONDS)
}

register_convex_histogram!(ID_TRACKER_SIZE_BYTES, "IdTracker file size");
pub fn log_id_tracker_size(size: usize) {
    log_distribution(&ID_TRACKER_SIZE_BYTES, size as f64);
}

register_convex_int_gauge!(
    HTTP_SERVICE_MAX_CONCURRENT_REQUESTS,
    "Maximum number of concurrent requests for an HTTP service",
    &["service_name"]
);

pub fn log_http_service_max_concurrent_requests(
    service_name: &'static str,
    max_concurrent_requests: usize,
) {
    HTTP_SERVICE_MAX_CONCURRENT_REQUESTS
        .with_label_values(&[service_name])
        .set(max_concurrent_requests as i64);
}

register_convex_histogram!(
    INDEX_RETENTION_DELETE_CHUNK_SECONDS,
    "Time for index retention to delete one chunk"
);
pub fn index_retention_delete_chunk_timer() -> Timer<VMHistogram> {
    Timer::new(&INDEX_RETENTION_DELETE_CHUNK_SECONDS)
}

register_convex_counter!(
    INDEX_RETENTION_SCANNED_DOCUMENT_TOTAL,
    "Count of documents scanned by index retention",
    &["tombstone", "prev_rev"]
);
pub fn log_index_retention_scanned_document(is_tombstone: bool, has_prev_rev: bool) {
    log_counter_with_labels(
        &INDEX_RETENTION_SCANNED_DOCUMENT_TOTAL,
        1,
        vec![
            StaticMetricLabel::new(
                "tombstone",
                if is_tombstone {
                    "is_tombstone"
                } else {
                    "is_document"
                },
            ),
            StaticMetricLabel::new(
                "prev_rev",
                if has_prev_rev {
                    "has_prev_rev"
                } else {
                    "no_prev_rev"
                },
            ),
        ],
    )
}

register_convex_counter!(
    RETENTION_EXPIRED_INDEX_ENTRY_TOTAL,
    "Number of index entries expired by retention",
    &["reason"]
);
pub fn log_retention_expired_index_entry(is_tombstone: bool, is_key_change_tombstone: bool) {
    log_counter_with_labels(
        &RETENTION_EXPIRED_INDEX_ENTRY_TOTAL,
        1,
        vec![StaticMetricLabel::new(
            "reason",
            if is_tombstone {
                if is_key_change_tombstone {
                    "key_change_tombstone"
                } else {
                    "tombstone"
                }
            } else {
                "overwritten"
            },
        )],
    )
}
