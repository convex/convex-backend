//! The usage-metric store model: per-resolution counter stores, the
//! bucket-width/retention constants they're sized by, and the
//! calendar-window math that maps limit windows onto them.

use std::{
    ops::Range,
    time::{
        Duration,
        SystemTime,
    },
};

use anyhow::Context;
use chrono::{
    DateTime,
    Datelike,
    DurationRound,
    Months,
    NaiveTime,
    TimeDelta,
    Utc,
};
use model::usage_limits::types::UsageLimitWindow;
use udf_metrics::SeedableCounterStore;

pub(crate) const MINUTELY_MAX_BUCKETS: u64 = 90;
pub(crate) const HOURLY_MAX_BUCKETS: u64 = 25;
pub(crate) const DAILY_MAX_BUCKETS: u64 = 32;

const MINUTELY_BUCKET_WIDTH: Duration = Duration::from_secs(60);
const HOURLY_BUCKET_WIDTH: Duration = Duration::from_secs(60 * 60);
const DAILY_BUCKET_WIDTH: Duration = Duration::from_secs(24 * 60 * 60);

/// The resolutions usage is stored at; a seed row targets exactly one of
/// them. `IntoStaticStr` supplies the metric-label name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum UsageMetricResolution {
    Minutely,
    Hourly,
    Daily,
}

impl UsageMetricResolution {
    fn bucket_width(&self) -> Duration {
        match self {
            UsageMetricResolution::Minutely => MINUTELY_BUCKET_WIDTH,
            UsageMetricResolution::Hourly => HOURLY_BUCKET_WIDTH,
            UsageMetricResolution::Daily => DAILY_BUCKET_WIDTH,
        }
    }

    /// The calendar-aligned bucket containing `ts` at this resolution,
    /// start-inclusive/end-exclusive.
    pub(super) fn bucket_range(&self, ts: SystemTime) -> anyhow::Result<Range<SystemTime>> {
        let width = self.bucket_width();
        let start = floor_to_utc_width(ts, width)?;
        Ok(start..start + width)
    }

    fn max_buckets(&self) -> u64 {
        match self {
            UsageMetricResolution::Minutely => MINUTELY_MAX_BUCKETS,
            UsageMetricResolution::Hourly => HOURLY_MAX_BUCKETS,
            UsageMetricResolution::Daily => DAILY_MAX_BUCKETS,
        }
    }
}

/// The three usage metric stores, one per resolution, sharing a `base_ts`
/// that is (a) UTC-day-aligned, so every bucket starts on its natural clock
/// boundary and window sums line up, and (b) back-dated by the daily
/// retention, so month-old seed rows land after it.
pub struct UsageMetricStores {
    minutely: SeedableCounterStore,
    hourly: SeedableCounterStore,
    daily: SeedableCounterStore,
}

impl UsageMetricStores {
    pub fn new(now: SystemTime) -> anyhow::Result<Self> {
        let base_ts = floor_to_utc_width(
            now - DAILY_BUCKET_WIDTH * DAILY_MAX_BUCKETS as u32,
            DAILY_BUCKET_WIDTH,
        )?;
        let store = |resolution: UsageMetricResolution| {
            SeedableCounterStore::new(base_ts, resolution.bucket_width(), resolution.max_buckets())
        };
        Ok(Self {
            minutely: store(UsageMetricResolution::Minutely),
            hourly: store(UsageMetricResolution::Hourly),
            daily: store(UsageMetricResolution::Daily),
        })
    }

    /// Record a live usage delta in all three resolutions. It lands in the
    /// bucket containing `ts`; a sample older than one resolution's retention
    /// is skipped there while coarser resolutions still count it.
    pub fn add(&mut self, metric_name: &str, ts: SystemTime, delta: f64, now: SystemTime) {
        self.minutely.add(metric_name, ts, delta, now);
        self.hourly.add(metric_name, ts, delta, now);
        self.daily.add(metric_name, ts, delta, now);
    }

    /// Hydrate one resolution's bucket from a seed row.
    ///
    /// `now` must be the current wall clock — it drives pruning and drops
    /// future seed rows. A row-derived `now` could prune live data.
    pub fn seed(
        &mut self,
        resolution: UsageMetricResolution,
        metric_name: &str,
        ts: SystemTime,
        value: f64,
        now: SystemTime,
    ) {
        self.store_mut(resolution)
            .seed_counter(metric_name, ts, value, now);
    }

    /// Usage within the calendar-aligned limit window containing `now`.
    ///
    /// A metric with no samples sums to 0 — which means a misspelled name
    /// reads 0 forever, so derive names from the `UsageLimitMetric` mapping,
    /// never free strings.
    pub fn window_total(
        &self,
        window: UsageLimitWindow,
        metric_name: &str,
        now: SystemTime,
    ) -> anyhow::Result<f64> {
        let range = window_range(window, now)?;
        Ok(self.sum_range(window_resolution(window), metric_name, &range))
    }

    /// The current total of the single bucket containing `ts` at
    /// `resolution`, in the metric's raw unit.
    pub fn bucket_total(
        &self,
        resolution: UsageMetricResolution,
        metric_name: &str,
        ts: SystemTime,
    ) -> anyhow::Result<f64> {
        let range = resolution.bucket_range(ts)?;
        Ok(self.sum_range(resolution, metric_name, &range))
    }

    /// Sum a metric over a time range within one resolution's store.
    fn sum_range(
        &self,
        resolution: UsageMetricResolution,
        metric_name: &str,
        range: &Range<SystemTime>,
    ) -> f64 {
        self.store(resolution).sum_counter(metric_name, range)
    }

    fn store(&self, resolution: UsageMetricResolution) -> &SeedableCounterStore {
        match resolution {
            UsageMetricResolution::Minutely => &self.minutely,
            UsageMetricResolution::Hourly => &self.hourly,
            UsageMetricResolution::Daily => &self.daily,
        }
    }

    fn store_mut(&mut self, resolution: UsageMetricResolution) -> &mut SeedableCounterStore {
        match resolution {
            UsageMetricResolution::Minutely => &mut self.minutely,
            UsageMetricResolution::Hourly => &mut self.hourly,
            UsageMetricResolution::Daily => &mut self.daily,
        }
    }
}

/// A window is summed from the resolution one step finer. Seed rollups only
/// exist for completed periods, so the in-progress day or month is only fully
/// covered one resolution down.
fn window_resolution(window: UsageLimitWindow) -> UsageMetricResolution {
    match window {
        UsageLimitWindow::Day => UsageMetricResolution::Hourly,
        UsageLimitWindow::Month => UsageMetricResolution::Daily,
    }
}

/// The calendar-aligned UTC window containing `now` (day or calendar month),
/// start-inclusive/end-exclusive to match `sum_counter`.
pub(super) fn window_range(
    window: UsageLimitWindow,
    now: SystemTime,
) -> anyhow::Result<Range<SystemTime>> {
    match window {
        UsageLimitWindow::Day => {
            let start = floor_to_utc_width(now, DAILY_BUCKET_WIDTH)?;
            Ok(start..start + DAILY_BUCKET_WIDTH)
        },
        UsageLimitWindow::Month => {
            let now_utc: DateTime<Utc> = now.into();
            let start = now_utc
                .date_naive()
                .with_day(1)
                .context("invalid month window start")?;
            let end = start
                .checked_add_months(Months::new(1))
                .context("invalid month window end")?;
            Ok(SystemTime::from(start.and_time(NaiveTime::MIN).and_utc())
                ..SystemTime::from(end.and_time(NaiveTime::MIN).and_utc()))
        },
    }
}

/// Floor `ts` to a multiple of `width` since the unix epoch — which is a UTC
/// midnight, so any width dividing a day lands on its natural UTC boundary.
fn floor_to_utc_width(ts: SystemTime, width: Duration) -> anyhow::Result<SystemTime> {
    let ts_utc: DateTime<Utc> = ts.into();
    let floored = ts_utc.duration_trunc(TimeDelta::from_std(width)?)?;
    Ok(floored.into())
}
