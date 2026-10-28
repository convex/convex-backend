//! The usage meter: evaluation of configured usage limits against the
//! in-memory metric stores.

use std::{
    collections::HashMap,
    time::SystemTime,
};

use common::types::UsageLimitStopState;
use model::usage_limits::types::{
    UsageLimitConfig,
    UsageLimitMetric,
    UsageLimitType,
    UsageLimitWindow,
};
use parking_lot::Mutex;
use strum::IntoEnumIterator;
use value::ResolvedDocumentId;

use super::stores::{
    window_range,
    UsageMetricResolution,
    UsageMetricStores,
};

/// How much of a deployment's historical-usage backfill has landed. Owned by
/// the seeder — it knows its own pass protocol — and surfaced by the usage API
/// so a consumer of live usage knows whether the numbers reflect the full
/// window. Delivered alongside each seed pass to
/// `Application::apply_app_metric_seed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedStatus {
    /// No usable seed has landed yet: usage covers only traffic since load.
    Pending,
    /// Some history has been hydrated, but the backfill isn't finished.
    Partial,
    /// The backfill finished and hydrated history.
    Complete,
    /// The backfill finished without hydrating any history (e.g. every seed
    /// query failed).
    Failed,
}

/// A limit whose window total reached its configured limit.
#[derive(Debug, Clone)]
pub struct ExceededUsageLimit {
    pub id: ResolvedDocumentId,
    pub config: UsageLimitConfig,
    /// Start of the window the limit is exceeded in.
    pub window_start: SystemTime,
}

/// Outcome of one enforcement evaluation.
#[derive(Debug)]
pub struct UsageLimitEvaluation {
    /// Every enabled limit currently at or over its configured limit.
    pub exceeded: Vec<ExceededUsageLimit>,
    /// The stop state the deployment should currently be in: `Disabled`
    /// while any enabled `Disable` limit is exceeded, `None` otherwise.
    pub desired_stop_state: UsageLimitStopState,
}

/// One usage rollup row to seed, in its bucket's raw unit.
#[derive(Debug, Clone)]
pub struct SeedRow {
    pub metric: UsageLimitMetric,
    pub resolution: UsageMetricResolution,
    pub time: SystemTime,
    pub value: f64,
}

/// How a bucket's seed value compares against the meter's in-memory value,
/// read before the seed max-merges over it.
///
/// A bucket is *new* when it starts at or after the meter's creation — live
/// recording covered its whole span — and *historical* when it ends at or
/// before, so the meter's value came from earlier seed passes. Buckets
/// straddling the meter's creation are never compared: live recording missed
/// their pre-creation fraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum SeedComparisonKind {
    /// The two sides agree within [`SEED_COMPARISON_TOLERANCE`].
    Match,
    /// New bucket where the meter leads the seed. Expected: the seed source
    /// lags live recording.
    NewMeterAhead,
    /// New bucket where the seed exceeds live recording: the pipeline
    /// counted usage the recording mapping missed. The alertable quadrant.
    NewSeedAhead,
    /// Historical bucket where the meter exceeds the seed. Both sides came
    /// from the same rollups, so this is a contract bug or rounding drift.
    HistoricalMeterAhead,
    /// Historical bucket where the seed exceeds the meter; same contract
    /// implications as [`Self::HistoricalMeterAhead`].
    HistoricalSeedAhead,
}

impl SeedComparisonKind {
    /// Whether this kind indicates a recording/seeding contract bug.
    pub fn is_bug(self) -> bool {
        matches!(
            self,
            Self::NewSeedAhead | Self::HistoricalMeterAhead | Self::HistoricalSeedAhead
        )
    }
}

/// One bucket's pre-merge comparison between the meter's in-memory value and
/// a seed delivery's value for the same metric bucket.
#[derive(Debug, Clone)]
pub struct SeedComparison {
    pub metric: UsageLimitMetric,
    pub resolution: UsageMetricResolution,
    pub bucket_start: SystemTime,
    pub meter_value: f64,
    pub seed_value: f64,
    pub kind: SeedComparisonKind,
}

/// Result of applying one seed delivery.
#[derive(Debug)]
pub struct SeedComparisonResult {
    /// Distinct buckets the delivery seeded.
    pub num_buckets: usize,
    /// Pre-merge comparisons for the buckets that were comparable.
    pub comparisons: Vec<SeedComparison>,
}

/// A gap within this fraction of the larger side counts as a match. Relative
/// rather than absolute so one threshold fits every unit (calls, bytes, GB,
/// GB·s); mapping bugs are order-of-magnitude, so 1% still catches them while
/// absorbing f64 rounding.
const SEED_COMPARISON_TOLERANCE: f64 = 0.01;

/// Current-window usage for a single metric, in the store's raw units (calls,
/// bytes, or GB·s). Convert with `UsageLimitMetric::usage_in_display_units`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricWindowUsage {
    pub current_day: f64,
    pub current_month: f64,
}

/// In-memory usage meter: owns the metric stores and the active limit
/// configs. Usage is recorded into it from the usage-event stream by
/// [`super::UsageLimitRecorder`] and evaluated against the limits by
/// [`super::UsageLimitWorker`].
pub struct UsageMeter {
    inner: Mutex<Inner>,
    /// When live recording began, classifying seed buckets as historical
    /// (fully before) or new (fully after) for seed comparisons.
    created_at: SystemTime,
}

struct Inner {
    stores: UsageMetricStores,
    configs: Vec<(ResolvedDocumentId, UsageLimitConfig)>,
    /// Latest backfill status reported by the seeder; see [`SeedStatus`].
    seed_status: SeedStatus,
}

impl UsageMeter {
    pub fn new(now: SystemTime) -> anyhow::Result<Self> {
        Ok(Self {
            inner: Mutex::new(Inner {
                stores: UsageMetricStores::new(now)?,
                configs: Vec::new(),
                seed_status: SeedStatus::Pending,
            }),
            created_at: now,
        })
    }

    /// Replace the active configs.
    pub fn refresh_configs(&self, configs: Vec<(ResolvedDocumentId, UsageLimitConfig)>) {
        self.inner.lock().configs = configs;
    }

    /// Record live usage deltas (raw units: calls, bytes, GB·s) that occurred
    /// at `ts` (the current time for live recording).
    ///
    /// Recording is unconditional: every metric is tracked whether or not a
    /// limit currently targets it. So enabling a limit mid-window enforces
    /// against the usage already accrued this window rather than only usage
    /// from the moment it was enabled. Enforcement stays gated on enabled
    /// configs (see [`Self::evaluate`]).
    pub fn record(&self, ts: SystemTime, deltas: &[(UsageLimitMetric, f64)]) {
        let mut inner = self.inner.lock();
        for (metric, delta) in deltas {
            if *delta <= 0.0 {
                continue;
            }
            inner.stores.add(metric.metric_name(), ts, *delta, ts);
        }
    }

    /// Seed the stores from one complete delivery of usage rollup rows.
    /// Sums the rows into per-bucket totals first — several source metrics
    /// feed one bucket, and the stores' max-merge expects each bucket's
    /// complete total in a single write. The seed query returns at most one
    /// row per (metric_name, resolution, rollup_time), so the sum only ever
    /// combines different source metrics.
    ///
    /// Each bucket's seed value is also compared against the meter's current
    /// value before the merge; see [`SeedComparisonKind`].
    pub fn seed_rows(&self, rows: Vec<SeedRow>, now: SystemTime) -> SeedComparisonResult {
        let mut combined: HashMap<(UsageLimitMetric, UsageMetricResolution, SystemTime), f64> =
            HashMap::new();
        for row in rows {
            *combined
                .entry((row.metric, row.resolution, row.time))
                .or_insert(0.0) += row.value;
        }
        let num_buckets = combined.len();
        let mut comparisons = Vec::new();
        let mut inner = self.inner.lock();
        for ((metric, resolution, time), seed_value) in combined {
            if let Some(comparison) =
                self.compare_bucket(&inner.stores, metric, resolution, time, seed_value, now)
            {
                comparisons.push(comparison);
            }
            inner
                .stores
                .seed(resolution, metric.metric_name(), time, seed_value, now);
        }
        SeedComparisonResult {
            num_buckets,
            comparisons,
        }
    }

    /// Compare one bucket's seed value against the meter's current value,
    /// classifying per [`SeedComparisonKind`]. Returns `None` for buckets
    /// with nothing meaningful to compare: buckets straddling the meter's
    /// creation, future buckets, and historical buckets the meter has no
    /// value for (first-pass hydration).
    fn compare_bucket(
        &self,
        stores: &UsageMetricStores,
        metric: UsageLimitMetric,
        resolution: UsageMetricResolution,
        time: SystemTime,
        seed_value: f64,
        now: SystemTime,
    ) -> Option<SeedComparison> {
        let bucket = resolution.bucket_range(time).ok()?;
        if bucket.start > now {
            return None;
        }
        let historical = bucket.end <= self.created_at;
        if !historical && bucket.start < self.created_at {
            return None;
        }
        let meter_value = stores
            .bucket_total(resolution, metric.metric_name(), time)
            .ok()?;
        if historical && meter_value == 0.0 {
            return None;
        }
        let tolerance = SEED_COMPARISON_TOLERANCE * seed_value.abs().max(meter_value.abs());
        let kind = if (seed_value - meter_value).abs() <= tolerance {
            SeedComparisonKind::Match
        } else {
            match (historical, seed_value > meter_value) {
                (false, false) => SeedComparisonKind::NewMeterAhead,
                (false, true) => SeedComparisonKind::NewSeedAhead,
                (true, false) => SeedComparisonKind::HistoricalMeterAhead,
                (true, true) => SeedComparisonKind::HistoricalSeedAhead,
            }
        };
        Some(SeedComparison {
            metric,
            resolution,
            bucket_start: bucket.start,
            meter_value,
            seed_value,
            kind,
        })
    }

    pub fn set_seed_status(&self, status: SeedStatus) {
        self.inner.lock().seed_status = status;
    }

    pub fn seed_status(&self) -> SeedStatus {
        self.inner.lock().seed_status
    }

    /// Current-window usage totals for every metric, in raw units. A metric
    /// with no recorded usage reads 0 across every window.
    pub fn usage_snapshot(
        &self,
        ts: SystemTime,
    ) -> anyhow::Result<Vec<(UsageLimitMetric, MetricWindowUsage)>> {
        let inner = self.inner.lock();
        UsageLimitMetric::iter()
            .map(|metric| {
                let name = metric.metric_name();
                let usage = MetricWindowUsage {
                    current_day: inner.stores.window_total(UsageLimitWindow::Day, name, ts)?,
                    current_month: inner
                        .stores
                        .window_total(UsageLimitWindow::Month, name, ts)?,
                };
                Ok((metric, usage))
            })
            .collect()
    }

    /// Evaluate every enabled limit against its current window. A limit is
    /// exceeded once its window total reaches the configured limit
    /// (`total >= limit`).
    pub fn evaluate(&self, now: SystemTime) -> anyhow::Result<UsageLimitEvaluation> {
        let inner = self.inner.lock();
        let mut exceeded = Vec::new();
        let mut any_disable_exceeded = false;
        for (id, config) in &inner.configs {
            if !config.enabled {
                continue;
            }
            let total =
                inner
                    .stores
                    .window_total(config.window, config.metric.metric_name(), now)?;
            if total < config.metric.limit_in_raw_units(config.limit) {
                continue;
            }
            if config.limit_type == UsageLimitType::Disable {
                any_disable_exceeded = true;
            }
            exceeded.push(ExceededUsageLimit {
                id: *id,
                config: config.clone(),
                window_start: window_range(config.window, now)?.start,
            });
        }
        Ok(UsageLimitEvaluation {
            exceeded,
            desired_stop_state: if any_disable_exceeded {
                UsageLimitStopState::Disabled
            } else {
                UsageLimitStopState::None
            },
        })
    }
}
