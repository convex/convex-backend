use metrics::{
    log_counter_with_labels,
    log_distribution_with_labels,
    register_convex_counter,
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
