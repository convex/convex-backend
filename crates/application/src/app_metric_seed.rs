use std::{
    collections::BTreeMap,
    time::SystemTime,
};

use common::{
    errors::report_error_sync,
    runtime::Runtime,
};
use model::usage_limits::types::UsageLimitMetric;
use usage_limits::{
    SeedComparison,
    SeedComparisonResult,
    SeedRow,
    SeedStatus,
    UsageMetricResolution,
};

use crate::{
    metrics::log_app_metrics_seed_comparison,
    Application,
};

/// Time granularity of a seeded app-metric data point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Granularity {
    Minute,
    Hour,
    Day,
}

impl Granularity {
    fn resolution(self) -> UsageMetricResolution {
        match self {
            Granularity::Minute => UsageMetricResolution::Minutely,
            Granularity::Hour => UsageMetricResolution::Hourly,
            Granularity::Day => UsageMetricResolution::Daily,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppMetricSeedRow {
    pub metric_name: String,
    pub granularity: Granularity,
    pub time: SystemTime,
    /// In the source metric's raw unit — calls, bytes, GB, or GB·s (see
    /// `UsageLimitMetric::from_seed_metric`). `f64` because the GB search and
    /// GB·s compute rollups have fractional values; call counts and byte
    /// totals are whole numbers.
    pub value: f64,
}

/// Signals that the seed data source failed. Delivered to the deployment
/// instead of an empty row set so it can distinguish a query error from a
/// deployment that genuinely has no historical usage, and skip hydration
/// rather than zero-fill its metric stores.
#[derive(Clone, Debug)]
pub struct AppMetricSeedError {
    pub message: String,
}

pub type AppMetricSeedResult = Result<Vec<AppMetricSeedRow>, AppMetricSeedError>;

impl<RT: Runtime> Application<RT> {
    /// Apply one seed pass: record the seeder's `status` and, when the pass
    /// carried data, hydrate the metric stores from it. The status is stored
    /// even on failure, so a terminal `Failed`/`Complete` is surfaced.
    pub fn apply_app_metric_seed(&self, result: AppMetricSeedResult, status: SeedStatus) {
        let deployment_name = self.deployment_name();
        self.usage_meter().set_seed_status(status);
        let rows = match result {
            Ok(rows) => rows,
            Err(err) => {
                // The seed query failed. Skip hydration so we don't mistake
                // the empty result for "this deployment has no usage".
                tracing::warn!(
                    "App-metrics seed query failed for {deployment_name}: {}; skipping hydration",
                    err.message,
                );
                return;
            },
        };
        let num_rows = rows.len();
        let mut unknown = 0usize;
        let seed_rows: Vec<SeedRow> = rows
            .into_iter()
            .filter_map(|row| {
                let Some(metric) = UsageLimitMetric::from_seed_metric(&row.metric_name) else {
                    unknown += 1;
                    return None;
                };
                Some(SeedRow {
                    metric,
                    resolution: row.granularity.resolution(),
                    time: row.time,
                    value: row.value,
                })
            })
            .collect();
        let SeedComparisonResult {
            num_buckets,
            comparisons,
        } = self
            .usage_meter()
            .seed_rows(seed_rows, self.runtime().system_time());
        self.report_seed_comparisons(&comparisons);
        if unknown > 0 {
            tracing::warn!(
                "App-metrics seed for {deployment_name}: skipped {unknown} row(s) with \
                 unrecognized metric names",
            );
        }
        tracing::info!(
            "Seeded app metrics for {deployment_name}: {num_buckets} bucket(s) from {num_rows} \
             row(s)",
        );
    }

    /// Report one seed pass's meter-vs-seed comparisons: a counter per
    /// compared bucket, and a Sentry error per (metric, kind) for the bug
    /// classes, carrying the pass's worst-offending bucket.
    fn report_seed_comparisons(&self, comparisons: &[SeedComparison]) {
        // Counting every class, `match` included, gives alerts a denominator.
        let mut bugs: BTreeMap<(&'static str, &'static str), (usize, &SeedComparison)> =
            BTreeMap::new();
        for comparison in comparisons {
            log_app_metrics_seed_comparison(
                comparison.metric.metric_name(),
                comparison.resolution.into(),
                comparison.kind.into(),
            );
            if !comparison.kind.is_bug() {
                continue;
            }
            let gap = (comparison.seed_value - comparison.meter_value).abs();
            bugs.entry((comparison.metric.metric_name(), comparison.kind.into()))
                .and_modify(|(count, worst)| {
                    *count += 1;
                    if gap > (worst.seed_value - worst.meter_value).abs() {
                        *worst = comparison;
                    }
                })
                .or_insert((1, comparison));
        }
        // Leading with the stable `{metric} ({kind})` groups a broken metric
        // into one Sentry issue; the per-pass numbers trail it, and the
        // deployment rides the ambient Sentry scope.
        for ((metric, kind), (count, worst)) in bugs {
            report_error_sync(&mut anyhow::anyhow!(
                "Usage-limit seed discrepancy on {metric} ({kind}): {count} bucket(s), worst \
                 {resolution} bucket meter={meter} seed={seed}",
                resolution = <&str>::from(worst.resolution),
                meter = worst.meter_value,
                seed = worst.seed_value,
            ));
        }
    }
}
