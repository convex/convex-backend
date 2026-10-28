use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

use errors::ErrorMetadataAnyhowExt as _;
use metrics::{
    log_counter_with_labels,
    log_distribution_with_labels,
    log_gauge_with_labels,
    register_convex_counter,
    register_convex_gauge,
    register_convex_histogram,
    StaticMetricLabel,
    StatusTimer,
    STATUS_LABEL,
};
use model::source_packages::types::PackageSize;

register_convex_counter!(
    EXTERNAL_DEPS_PACKAGES_TOTAL,
    "Total pushes with external dependency packages",
    &["cache_status"],
);
pub fn log_external_deps_package(is_cache_hit: bool) {
    let cache_label = if is_cache_hit { "hit" } else { "miss" };

    log_counter_with_labels(
        &EXTERNAL_DEPS_PACKAGES_TOTAL,
        1,
        vec![StaticMetricLabel::new("cache_status", cache_label)],
    );
}

register_convex_histogram!(
    SOURCE_PACKAGE_SIZE_BYTES_TOTAL,
    "Size of source package in bytes",
    &["compressed"],
);
pub fn log_source_package_size_bytes_total(pkg_size: PackageSize) {
    let zipped_label = StaticMetricLabel::new("compressed", "true");
    let unzipped_label = StaticMetricLabel::new("compressed", "false");

    log_distribution_with_labels(
        &SOURCE_PACKAGE_SIZE_BYTES_TOTAL,
        pkg_size.zipped_size_bytes as f64,
        vec![zipped_label],
    );
    log_distribution_with_labels(
        &SOURCE_PACKAGE_SIZE_BYTES_TOTAL,
        pkg_size.unzipped_size_bytes as f64,
        vec![unzipped_label],
    );
}

pub struct AppWorkerStatus {
    name: &'static str,
}

impl Drop for AppWorkerStatus {
    fn drop(&mut self) {
        tracing::debug!("Worker {} finished", self.name);
    }
}

pub fn log_worker_starting(name: &'static str) -> AppWorkerStatus {
    tracing::debug!("Worker {} started", name);
    AppWorkerStatus { name }
}

register_convex_counter!(
    TABLE_SUMMARY_CHECKPOINT_TOTAL,
    "Number of table summary checkpoint writes",
    &["is_bootstrapping"],
);
pub fn log_table_summary_checkpoint(is_bootstrapping: bool) {
    log_counter_with_labels(
        &TABLE_SUMMARY_CHECKPOINT_TOTAL,
        1,
        vec![StaticMetricLabel::new(
            "is_bootstrapping",
            is_bootstrapping.to_string(),
        )],
    );
}

register_convex_histogram!(
    TABLE_SUMMARY_BOOTSTRAP_SECONDS,
    "Time to bootstrap table summary",
    &STATUS_LABEL
);
pub fn table_summary_bootstrap_timer() -> StatusTimer {
    StatusTimer::new(&TABLE_SUMMARY_BOOTSTRAP_SECONDS)
}

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

register_convex_gauge!(
    S3_EXPORT_NEXT_SYNC_DUE_TIMESTAMP_SECONDS,
    "Unix time by which a deployment's S3 export is due to sync again",
    &["instance_name"]
);

register_convex_gauge!(
    S3_EXPORT_SNAPSHOT_SYNCED_TABLES,
    "Tables finished so far by a deployment's initial S3 export snapshot",
    &["instance_name"]
);

/// One deployment's S3 export gauges. Dropping this removes the series, so a
/// deployment that leaves this process does not look stalled.
pub struct S3ExportGauges {
    instance_name: String,
}

impl S3ExportGauges {
    pub fn new(instance_name: String) -> Self {
        Self { instance_name }
    }

    fn labels(&self) -> Vec<StaticMetricLabel> {
        vec![StaticMetricLabel::new(
            "instance_name",
            self.instance_name.clone(),
        )]
    }

    /// Sets the due time only while the series is unset. A retry after a
    /// failed page keeps the earlier due time, so an export that keeps failing
    /// becomes overdue.
    pub fn start(&self, now: SystemTime) {
        let gauge =
            S3_EXPORT_NEXT_SYNC_DUE_TIMESTAMP_SECONDS.with_label_values(&[&self.instance_name]);
        if gauge.get() == 0.0 {
            gauge.set(unix_seconds(now));
        }
    }

    pub fn set_next_sync_due(&self, due: SystemTime) {
        log_gauge_with_labels(
            &S3_EXPORT_NEXT_SYNC_DUE_TIMESTAMP_SECONDS,
            unix_seconds(due),
            self.labels(),
        );
    }

    /// `None` once the initial snapshot is done.
    pub fn set_snapshot_tables_synced(&self, tables: Option<u64>) {
        match tables {
            Some(tables) => log_gauge_with_labels(
                &S3_EXPORT_SNAPSHOT_SYNCED_TABLES,
                tables as f64,
                self.labels(),
            ),
            None => {
                let _ =
                    S3_EXPORT_SNAPSHOT_SYNCED_TABLES.remove_label_values(&[&self.instance_name]);
            },
        }
    }

    pub fn clear(&self) {
        let _ =
            S3_EXPORT_NEXT_SYNC_DUE_TIMESTAMP_SECONDS.remove_label_values(&[&self.instance_name]);
        self.set_snapshot_tables_synced(None);
    }
}

impl Drop for S3ExportGauges {
    fn drop(&mut self) {
        self.clear();
    }
}

fn unix_seconds(time: SystemTime) -> f64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

// Joins the `app_metrics_seed_*` family (conductor emits the rest) so it
// lands on the existing "Usage Limit Seeds" Grafana dashboard.
register_convex_counter!(
    APP_METRICS_SEED_COMPARISON_TOTAL,
    "Seed buckets compared against the in-memory usage meter, by comparison kind",
    &["metric", "resolution", "kind"],
);
pub fn log_app_metrics_seed_comparison(
    metric: &'static str,
    resolution: &'static str,
    kind: &'static str,
) {
    log_counter_with_labels(
        &APP_METRICS_SEED_COMPARISON_TOTAL,
        1,
        vec![
            StaticMetricLabel::new("metric", metric),
            StaticMetricLabel::new("resolution", resolution),
            StaticMetricLabel::new("kind", kind),
        ],
    );
}
