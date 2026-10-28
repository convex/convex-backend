//! In-memory store for UDF metrics. This data structure is optimized for
//! storing many sparse metrics with a low retention time (default: 1 hour) and
//! relatively coarse aggregation buckets (default: 1 minute).
//!
//! We support two types of metrics:
//!
//! - Counter: A simple counter that records some summable quantity associated
//!   with an event.
//! - Histogram: A histogram that records the distribution of a duration between
//!   1ms and 15m at millisecond resolution.
//!
//! # Implementation notes
//! The data structure keeps a `base_ts` for its creation time and partitions
//! time into one minute "buckets" of the form
//! `[base_ts + i * 1m, base_ts + (i + 1) * 1m)`. For memory efficiency, buckets
//! only store their index, not the full timestamp.
//!
//! Non empty buckets are stored in two slabs: one for counters and one for
//! histograms. Metrics must be strictly either counters or histograms, and it's
//! an error to log a histogram sample to a counter bucket.
//!
//! We maintain two indexes on the bucket slabs for efficient querying: one on
//! `(bucket_index, metric_key)` for efficiently finding buckets ordered by
//! time, and one on `(metric_key, bucket_index)` for efficiently finding the
//! buckets for a given metric.
use std::{
    cmp::{
        self,
    },
    collections::{
        BTreeMap,
        BTreeSet,
    },
    ops::{
        Range,
        RangeInclusive,
    },
    time::{
        Duration,
        SystemTime,
    },
};

use anyhow::Context;
use hdrhistogram::Histogram;
use imbl::{
    hashmap,
    ordmap,
    HashMap,
    OrdMap,
};
use imbl_slab::{
    Slab,
    SlabKey,
};
use serde::Deserialize;

mod seedable;
pub use seedable::SeedableCounterStore;

type BucketKey = SlabKey;
type MetricKey = SlabKey;

// To keep memory usage down, we store timestamps as multiples of `bucket_width`
// since `base_ts`. Assuming a bucket width of 1 minute, a u32 gives us ~8000
// years of data.
pub type BucketIndex = u32;

pub type MetricName = String;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
}

#[derive(Clone)]
struct Metric {
    name: MetricName,
    metric_type: MetricType,
}

#[derive(Clone)]
pub struct CounterBucket {
    pub index: BucketIndex,
    pub value: f32,
}

impl CounterBucket {
    pub fn new(index: BucketIndex, value: f32) -> Self {
        Self { index, value }
    }
}

#[derive(Clone)]
pub struct HistogramBucket {
    pub index: BucketIndex,
    pub histogram: Histogram<u8>,
}

impl HistogramBucket {
    fn new(config: &MetricStoreConfig, index: BucketIndex) -> Result<Self, UdfMetricsError> {
        let histogram = Histogram::new_with_bounds(
            config.histogram_min_duration.as_millis() as u64,
            config.histogram_max_duration.as_millis() as u64,
            config.histogram_significant_figures,
        )
        .map_err(UdfMetricsError::InvalidHistogram)?;
        Ok(Self { index, histogram })
    }

    fn record(
        &mut self,
        config: &MetricStoreConfig,
        duration: Duration,
    ) -> Result<(), UdfMetricsError> {
        let millis = (duration.as_millis() as u64)
            .clamp(1, config.histogram_max_duration.as_millis() as u64);
        self.histogram.record(millis)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct GaugeBucket {
    pub index: BucketIndex,
    pub value: f32,
}

impl GaugeBucket {
    pub fn new(index: BucketIndex, value: f32) -> Self {
        Self { index, value }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct MetricStoreConfig {
    pub bucket_width: Duration,
    pub max_buckets: usize,

    pub histogram_min_duration: Duration,
    pub histogram_max_duration: Duration,
    pub histogram_significant_figures: u8,
}

#[derive(Clone)]
pub struct MetricStore {
    base_ts: SystemTime,
    config: MetricStoreConfig,

    metrics: Slab<Metric>,
    metrics_by_name: HashMap<MetricName, MetricKey>,

    counter_buckets: Slab<CounterBucket>,
    histogram_buckets: Slab<HistogramBucket>,
    gauge_buckets: Slab<GaugeBucket>,

    // NB: Both of these indexes have bucket keys that point either to `counter_buckets`
    // or `histogram_buckets`: Since a single metric has to either be a counter or a
    // histogram, we can look at `metrics` to know which bucket slab to look at.
    bucket_by_start: OrdMap<(BucketIndex, MetricKey), BucketKey>,
    bucket_by_metric: OrdMap<(MetricKey, BucketIndex), BucketKey>,
}

impl MetricStore {
    pub fn new(base_ts: SystemTime, config: MetricStoreConfig) -> Self {
        Self {
            base_ts,
            config,
            metrics: Slab::new(),
            metrics_by_name: HashMap::new(),
            counter_buckets: Slab::new(),
            histogram_buckets: Slab::new(),
            gauge_buckets: Slab::new(),
            bucket_by_start: OrdMap::new(),
            bucket_by_metric: OrdMap::new(),
        }
    }

    /// Add a sample to a counter metric, allocating the metric if it doesn't
    /// exist already and pruning sufficiently old buckets.
    ///
    /// This method will return a `UdfMetricsError` if the sample precedes the
    /// most recent bucket across *all* metrics. This requires that samples
    /// be monotonically increasing over time modulo the bucket width.
    pub fn add_counter(
        &mut self,
        metric_name: &str,
        ts: SystemTime,
        value: f32,
    ) -> Result<(), UdfMetricsError> {
        self.add(MetricType::Counter, metric_name, ts, value)
    }

    /// Add a sample to a histogram metric. Similar to `add_counter`, this
    /// method allocates the metric if it doesn't exist and returns an
    /// error if isn't within the metric store's latest bucket.
    pub fn add_histogram(
        &mut self,
        metric_name: &str,
        ts: SystemTime,
        value: Duration,
    ) -> Result<(), UdfMetricsError> {
        self.add(MetricType::Histogram, metric_name, ts, value.as_secs_f32())
    }

    /// Add a sample to a gauge metric, allocating the metric if it doesn't
    /// exist already and pruning sufficiently old buckets.
    pub fn add_gauge(
        &mut self,
        metric_name: &str,
        ts: SystemTime,
        value: f32,
    ) -> Result<(), UdfMetricsError> {
        self.add(MetricType::Gauge, metric_name, ts, value)
    }

    /// Add a sample to a gauge metric, but only update if the new value is
    /// greater than the existing value in the bucket (tracking maximum).
    pub fn add_gauge_max(
        &mut self,
        metric_name: &str,
        ts: SystemTime,
        value: f32,
    ) -> Result<(), UdfMetricsError> {
        self.add_gauge_with_op(metric_name, ts, value, |existing| existing.max(value))
    }

    /// Validate timestamp and compute the bucket index.
    fn validate_and_get_bucket_index(
        &self,
        ts: SystemTime,
    ) -> Result<BucketIndex, UdfMetricsError> {
        let Ok(since_base) = ts.duration_since(self.base_ts) else {
            return Err(UdfMetricsError::SamplePrecedesBaseTimestamp {
                ts,
                base_ts: self.base_ts,
            });
        };
        let bucket_index = (since_base.as_nanos() / self.config.bucket_width.as_nanos()) as u32;
        if let Some(((max_bucket_index, _), _)) = self.bucket_by_start.get_max()
            && bucket_index < *max_bucket_index
        {
            return Err(UdfMetricsError::SamplePrecedesCutoff {
                ts,
                cutoff: self.bucket_start(*max_bucket_index),
            });
        }
        Ok(bucket_index)
    }

    /// Get or create a metric key, validating that the metric type matches
    /// if the metric already exists.
    fn get_or_create_metric(
        &mut self,
        metric_name: &str,
        metric_type: MetricType,
    ) -> Result<MetricKey, UdfMetricsError> {
        match self.metrics_by_name.entry(metric_name.to_string()) {
            hashmap::Entry::Occupied(entry) => {
                let metric = self
                    .metrics
                    .get(*entry.get())
                    .context("Invalid metric key")?;
                if metric.metric_type != metric_type {
                    return Err(UdfMetricsError::MetricTypeMismatch {
                        metric_type,
                        expected_type: metric.metric_type,
                    });
                }
                Ok(*entry.get())
            },
            hashmap::Entry::Vacant(entry) => {
                let metric = Metric {
                    name: metric_name.to_string(),
                    metric_type,
                };
                let metric_key = self.metrics.alloc(metric);
                entry.insert(metric_key);
                Ok(metric_key)
            },
        }
    }

    fn add_gauge_with_op(
        &mut self,
        metric_name: &str,
        ts: SystemTime,
        value: f32,
        op: impl FnOnce(f32) -> f32,
    ) -> Result<(), UdfMetricsError> {
        let bucket_index = self.validate_and_get_bucket_index(ts)?;
        let metric_key = self.get_or_create_metric(metric_name, MetricType::Gauge)?;

        let inserted = match self.bucket_by_metric.entry((metric_key, bucket_index)) {
            ordmap::Entry::Occupied(bucket_key) => {
                let bucket = self
                    .gauge_buckets
                    .get_mut(*bucket_key.get())
                    .context("Invalid bucket key")?;
                bucket.value = op(bucket.value);
                false
            },
            ordmap::Entry::Vacant(entry) => {
                let new_bucket = GaugeBucket::new(bucket_index, value);
                let new_bucket_key = self.gauge_buckets.alloc(new_bucket);
                entry.insert(new_bucket_key);
                self.bucket_by_start
                    .insert((bucket_index, metric_key), new_bucket_key);
                true
            },
        };

        if inserted {
            self.prune_buckets()?;
        }

        Ok(())
    }

    fn add(
        &mut self,
        metric_type: MetricType,
        metric_name: &str,
        ts: SystemTime,
        value: f32,
    ) -> Result<(), UdfMetricsError> {
        let bucket_index = self.validate_and_get_bucket_index(ts)?;
        let metric_key = self.get_or_create_metric(metric_name, metric_type)?;

        let inserted = match self.bucket_by_metric.entry((metric_key, bucket_index)) {
            // Try to log into the desired bucket if it exists.
            ordmap::Entry::Occupied(bucket_key) => {
                match metric_type {
                    MetricType::Counter => {
                        let bucket = self
                            .counter_buckets
                            .get_mut(*bucket_key.get())
                            .context("Invalid bucket key")?;
                        bucket.value += value;
                    },
                    MetricType::Gauge => {
                        let bucket = self
                            .gauge_buckets
                            .get_mut(*bucket_key.get())
                            .context("Invalid bucket key")?;
                        bucket.value = value;
                    },
                    MetricType::Histogram => {
                        let bucket = self
                            .histogram_buckets
                            .get_mut(*bucket_key.get())
                            .context("Invalid bucket key")?;
                        bucket.record(&self.config, Duration::from_secs_f32(value))?;
                    },
                }
                false
            },
            // Otherwise, create a new bucket.
            ordmap::Entry::Vacant(entry) => {
                let new_bucket_key = match metric_type {
                    MetricType::Counter => {
                        let new_bucket = CounterBucket::new(bucket_index, value);
                        self.counter_buckets.alloc(new_bucket)
                    },
                    MetricType::Gauge => {
                        let new_bucket = GaugeBucket::new(bucket_index, value);
                        self.gauge_buckets.alloc(new_bucket)
                    },
                    MetricType::Histogram => {
                        let mut new_bucket = HistogramBucket::new(&self.config, bucket_index)?;
                        new_bucket.record(&self.config, Duration::from_secs_f32(value))?;
                        self.histogram_buckets.alloc(new_bucket)
                    },
                };
                entry.insert(new_bucket_key);
                self.bucket_by_start
                    .insert((bucket_index, metric_key), new_bucket_key);
                true
            },
        };

        // We only need to prune buckets if we've created a new one.
        if inserted {
            self.prune_buckets()?;
        }

        Ok(())
    }

    /// Query all of the metrics that match a given metric type and name within
    /// a desired time range. The time range is inclusive of its start
    /// endpoint and exclusive of its end endpoint.
    pub fn metric_names_for_type(&self, metric_type: MetricType) -> Vec<MetricName> {
        self.metrics
            .iter()
            .filter_map(|(_, metric)| {
                if metric.metric_type == metric_type {
                    Some(metric.name.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Query all of the counter buckets that cover a desired time range. The
    /// time range is inclusive of its start endpoint and exclusive of its
    /// end endpoint.
    pub fn query_counter(
        &self,
        metric_name: &str,
        range: Range<SystemTime>,
    ) -> Result<Vec<&CounterBucket>, UdfMetricsError> {
        if range.end <= range.start {
            return Err(UdfMetricsError::InvalidTimeRange {
                start: range.start,
                end: range.end,
            });
        }
        let Some(metric_key) = self.metrics_by_name.get(metric_name) else {
            return Ok(vec![]);
        };
        let metric = self
            .metrics
            .get(*metric_key)
            .context("Invalid metric key")?;
        if metric.metric_type != MetricType::Counter {
            return Err(UdfMetricsError::MetricTypeMismatch {
                metric_type: metric.metric_type,
                expected_type: MetricType::Counter,
            });
        }
        // Compute the bucket indexes of the (inclusive) start of the range and the
        // predecessor of the (exclusive) end of the range. Then, we'll return all
        // buckets in the inclusive range `start_bucket_index..=end_bucket_index`.
        let start = (*metric_key, self.saturating_bucket_index(range.start));
        let end = (
            *metric_key,
            self.saturating_bucket_index(range.end - Duration::from_nanos(1)),
        );

        let mut result = Vec::new();
        for (_, bucket_key) in self.bucket_by_metric.range(start..=end) {
            let bucket = self
                .counter_buckets
                .get(*bucket_key)
                .context("Invalid bucket key")?;
            result.push(bucket);
        }
        Ok(result)
    }

    /// Query all of the gauge buckets that cover a desired time range. The
    /// time range is inclusive of its start endpoint and exclusive of its
    /// end endpoint.
    pub fn query_gauge(
        &self,
        metric_name: &str,
        range: Range<SystemTime>,
    ) -> Result<Vec<&GaugeBucket>, UdfMetricsError> {
        if range.end <= range.start {
            return Err(UdfMetricsError::InvalidTimeRange {
                start: range.start,
                end: range.end,
            });
        }
        let Some(metric_key) = self.metrics_by_name.get(metric_name) else {
            return Ok(vec![]);
        };
        let metric = self
            .metrics
            .get(*metric_key)
            .context("Invalid metric key")?;
        if metric.metric_type != MetricType::Gauge {
            return Err(UdfMetricsError::MetricTypeMismatch {
                metric_type: metric.metric_type,
                expected_type: MetricType::Gauge,
            });
        }

        // As with counters, map the input half-open interval into a closed interval
        // of covering bucket indexes.
        let start = (*metric_key, self.saturating_bucket_index(range.start));
        let end = (
            *metric_key,
            self.saturating_bucket_index(range.end - Duration::from_nanos(1)),
        );

        let mut result = Vec::new();
        for (_, bucket_key) in self.bucket_by_metric.range(start..=end) {
            let bucket = self
                .gauge_buckets
                .get(*bucket_key)
                .context("Invalid bucket key")?;
            result.push(bucket);
        }
        Ok(result)
    }

    /// Query all of the histogram buckets that cover a desired half-open time
    /// range.
    pub fn query_histogram(
        &self,
        metric_name: &str,
        range: Range<SystemTime>,
    ) -> Result<Vec<&HistogramBucket>, UdfMetricsError> {
        if range.end <= range.start {
            return Err(UdfMetricsError::InvalidTimeRange {
                start: range.start,
                end: range.end,
            });
        }
        let Some(metric_key) = self.metrics_by_name.get(metric_name) else {
            return Ok(vec![]);
        };
        let metric = self
            .metrics
            .get(*metric_key)
            .context("Invalid metric key")?;
        if metric.metric_type != MetricType::Histogram {
            return Err(UdfMetricsError::MetricTypeMismatch {
                metric_type: metric.metric_type,
                expected_type: MetricType::Histogram,
            });
        }

        // As with counters, map the input half-open interval into a closed interval
        // of covering bucket indexes.
        let start = (*metric_key, self.saturating_bucket_index(range.start));
        let end = (
            *metric_key,
            self.saturating_bucket_index(range.end - Duration::from_nanos(1)),
        );

        let mut result = Vec::new();
        for (_, bucket_key) in self.bucket_by_metric.range(start..=end) {
            let bucket = self
                .histogram_buckets
                .get(*bucket_key)
                .context("Invalid bucket key")?;
            result.push(bucket);
        }
        Ok(result)
    }

    pub fn bucket_index_range(&self) -> Option<RangeInclusive<BucketIndex>> {
        let ((max_bucket_index, _), _) = self.bucket_by_start.get_max()?;
        let Some(min_bucket_index) = max_bucket_index.checked_sub(self.config.max_buckets as u32)
        else {
            return Some(0..=*max_bucket_index);
        };
        Some((min_bucket_index + 1)..=*max_bucket_index)
    }

    // Compute the index of a the bucket that contains a given timestamp, saturating
    // to zero if the timestamp precedes the base timestamp.
    fn saturating_bucket_index(&self, ts: SystemTime) -> BucketIndex {
        let since_base = ts.duration_since(self.base_ts).unwrap_or(Duration::ZERO);
        (since_base.as_nanos() / self.config.bucket_width.as_nanos()) as u32
    }

    fn bucket_start(&self, index: BucketIndex) -> SystemTime {
        self.base_ts + (index * self.config.bucket_width)
    }

    fn prune_buckets(&mut self) -> anyhow::Result<()> {
        let Some(((max_bucket_index, _), _)) = self.bucket_by_start.get_max() else {
            return Ok(());
        };
        let Some(max_index_to_prune) = max_bucket_index.checked_sub(self.config.max_buckets as u32)
        else {
            return Ok(());
        };
        let mut touched_metrics = BTreeSet::new();
        while let Some(&((bucket_index, metric_key), bucket_key)) = self.bucket_by_start.get_min() {
            if max_index_to_prune < bucket_index {
                break;
            }
            let metric = self.metrics.get(metric_key).context("Invalid metric key")?;
            match metric.metric_type {
                MetricType::Counter => {
                    self.counter_buckets.free(bucket_key);
                },
                MetricType::Gauge => {
                    self.gauge_buckets.free(bucket_key);
                },
                MetricType::Histogram => {
                    self.histogram_buckets.free(bucket_key);
                },
            }
            self.bucket_by_metric
                .remove(&(metric_key, bucket_index))
                .context("Invalid bucket")?;
            self.bucket_by_start
                .remove(&(bucket_index, metric_key))
                .context("Invalid bucket")?;
            touched_metrics.insert(metric_key);
        }
        for metric_key in touched_metrics {
            let is_empty = self
                .bucket_by_metric
                .range((metric_key, 0)..(metric_key + 1, 0))
                .next()
                .is_none();
            if is_empty {
                let metric = self.metrics.free(metric_key);
                self.metrics_by_name.remove(&metric.name);
            }
        }
        Ok(())
    }

    pub fn base_ts(&self) -> SystemTime {
        self.base_ts
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UdfMetricsError {
    #[error("Invalid histogram parameters: {0}")]
    InvalidHistogram(hdrhistogram::CreationError),

    #[error("Sample precedes base timestamp: {ts:?} < {base_ts:?}")]
    SamplePrecedesBaseTimestamp { ts: SystemTime, base_ts: SystemTime },

    #[error("Sample precedes cutoff for metric: {ts:?} < {cutoff:?}")]
    SamplePrecedesCutoff { ts: SystemTime, cutoff: SystemTime },

    #[error("Metric type mismatch: {metric_type:?} != {expected_type:?}")]
    MetricTypeMismatch {
        metric_type: MetricType,
        expected_type: MetricType,
    },

    #[error("Failed to record value in histogram: {0}")]
    HistogramRecordError(#[from] hdrhistogram::RecordError),

    #[error("Invalid time range: {end:?} < {start:?}")]
    InvalidTimeRange { start: SystemTime, end: SystemTime },

    #[error(transparent)]
    InternalError(#[from] anyhow::Error),
}

/// A user-defined window for querying metrics. Note that the time window and
/// its bucket boundaries may not align with the `MetricStore`'s underlying
/// bucket boundaries.
#[derive(Debug)]
pub struct MetricsWindow {
    pub start: SystemTime,
    pub end: SystemTime,
    pub num_buckets: usize,
}

impl TryFrom<serde_json::Value> for MetricsWindow {
    type Error = anyhow::Error;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        #[derive(Debug, Deserialize)]
        struct MetricsWindowInner {
            start: SystemTime,
            end: SystemTime,
            num_buckets: usize,
        }
        let parsed: MetricsWindowInner = serde_json::from_value(value)?;
        if parsed.end < parsed.start {
            anyhow::bail!(
                "Invalid query window: {:?} < {:?}",
                parsed.end,
                parsed.start
            );
        }
        if parsed.num_buckets == 0 || parsed.num_buckets > 10000 {
            anyhow::bail!("Invalid query num_buckets: {}", parsed.num_buckets);
        }
        Ok(Self {
            start: parsed.start,
            end: parsed.end,
            num_buckets: parsed.num_buckets,
        })
    }
}

impl MetricsWindow {
    pub fn bucket_width(&self) -> anyhow::Result<Duration> {
        let interval_width = self
            .end
            .duration_since(self.start)
            .unwrap_or_else(|_| panic!("Invalid query window: {:?} < {:?}", self.end, self.start));
        Ok(interval_width / (self.num_buckets as u32))
    }

    pub fn bucket_index(&self, ts: SystemTime) -> anyhow::Result<usize> {
        if !(self.start..self.end).contains(&ts) {
            anyhow::bail!("{:?} not in [{:?}, {:?})", ts, self.start, self.end);
        }
        let since_start = ts.duration_since(self.start).unwrap();
        Ok((since_start.as_secs_f64() / self.bucket_width()?.as_secs_f64()) as usize)
    }

    pub fn bucket_start(&self, i: usize) -> anyhow::Result<SystemTime> {
        let bucket_start = self.start + self.bucket_width()? * (i as u32);
        if self.end < bucket_start {
            anyhow::bail!(
                "Invalid bucket index {} for {} buckets in [{:?}, {:?})",
                i,
                self.num_buckets,
                self.start,
                self.end
            );
        }
        Ok(bucket_start)
    }

    /// Resample a (potentially sparse) counter timeseries into the desired
    /// `MetricsWindow`.
    pub fn resample_counters(
        &self,
        metrics: &MetricStore,
        buckets: Vec<&CounterBucket>,
        is_rate: bool,
    ) -> anyhow::Result<Timeseries> {
        // Start by filling out the output buckets with unknown values.
        let mut result = Vec::with_capacity(self.num_buckets);
        for i in 0..self.num_buckets {
            let bucket_start = self.bucket_start(i)?;
            result.push((bucket_start, None));
        }

        // Fill in zeros for the range where we have data.
        let Some(bucket_index_range) = metrics.bucket_index_range() else {
            return Ok(result);
        };
        for bucket_index in bucket_index_range {
            let bucket_start = metrics.bucket_start(bucket_index);
            if (self.start..self.end).contains(&bucket_start) {
                let (_, value) = &mut result[self.bucket_index(bucket_start)?];
                *value = Some(0.0);
            }
        }

        // Map input buckets to output buckets by the input bucket's start time: We
        // simply find which output bucket the input bucket's start time falls into.
        // This may create some aliasing, especially if the output bucket size is small
        // relative to the input bucket size, but is good enough for now.
        for &CounterBucket { index, value } in buckets {
            let bucket_start = metrics.bucket_start(index);
            if (self.start..self.end).contains(&bucket_start) {
                let (_, existing) = &mut result[self.bucket_index(bucket_start)?];
                *existing.as_mut().context("Missing counter")? += value as f64;
            }
        }

        // Convert the counters to rates if needed by dividing by the bucket width.
        if is_rate {
            let width = self.bucket_width()?.as_secs_f64();
            for (_, value) in &mut result {
                if let Some(value) = value {
                    *value /= width;
                }
            }
        }

        Ok(result)
    }

    pub fn resample_gauges(
        &self,
        metrics: &MetricStore,
        buckets: Vec<&GaugeBucket>,
    ) -> anyhow::Result<Timeseries> {
        // Start by filling out the output buckets with unknown values.
        let mut result = Vec::with_capacity(self.num_buckets);
        for i in 0..self.num_buckets {
            let bucket_start = self.bucket_start(i)?;
            result.push((bucket_start, None));
        }

        // If we don't overlap with with any input buckets, return early.
        if metrics.bucket_index_range().is_none() {
            return Ok(result);
        }

        // Fill in values in increasing time order, taking the last value in case
        // multiple input buckets map to the same output bucket.
        let mut output_range: Option<RangeInclusive<usize>> = None;
        for bucket in buckets {
            let bucket_start = metrics.bucket_start(bucket.index);
            if (self.start..self.end).contains(&bucket_start) {
                let output_index = self.bucket_index(bucket_start)?;
                let new_range = match output_range {
                    None => RangeInclusive::new(output_index, output_index),
                    Some(range) => RangeInclusive::new(
                        cmp::min(*range.start(), output_index),
                        cmp::max(*range.end(), output_index),
                    ),
                };
                output_range = Some(new_range);
                let (_, existing) = &mut result[output_index];
                *existing = Some(bucket.value as f64);
            }
        }

        // Fill in missing output buckets within our known output range with the last
        // known value.
        if let Some(range) = output_range {
            let mut last_value = None;
            for (_, value) in &mut result[range] {
                match value {
                    Some(..) => {
                        last_value = *value;
                    },
                    None => {
                        *value = last_value;
                    },
                }
            }
        }

        Ok(result)
    }

    pub fn resample_histograms(
        &self,
        metrics: &MetricStore,
        buckets: Vec<&HistogramBucket>,
        percentiles: &[Percentile],
    ) -> anyhow::Result<BTreeMap<Percentile, Timeseries>> {
        if percentiles.len() > 5 {
            anyhow::bail!("Invalid query percentiles: {}", percentiles.len());
        }

        let mut histograms = Vec::with_capacity(self.num_buckets);
        for i in 0..self.num_buckets {
            let bucket_start = self.bucket_start(i)?;
            histograms.push((bucket_start, None));
        }

        // Default to an empty timeseries if we don't have any data.
        let Some(bucket_index_range) = metrics.bucket_index_range() else {
            let mut result = BTreeMap::new();
            for percentile in percentiles {
                let mut timeseries = Vec::with_capacity(self.num_buckets);
                for i in 0..self.num_buckets {
                    let bucket_start = self.bucket_start(i)?;
                    timeseries.push((bucket_start, None));
                }
                result.insert(*percentile, timeseries);
            }
            return Ok(result);
        };
        // Fill in an empty histogram for the range where we have data.
        for bucket_index in bucket_index_range {
            let bucket_start = metrics.bucket_start(bucket_index);
            if (self.start..self.end).contains(&bucket_start) {
                let (_, value) = &mut histograms[self.bucket_index(bucket_start)?];
                let histogram = Histogram::new_with_bounds(
                    metrics.config.histogram_min_duration.as_millis() as u64,
                    metrics.config.histogram_max_duration.as_millis() as u64,
                    metrics.config.histogram_significant_figures,
                )?;
                *value = Some(histogram);
            }
        }

        // Merge in the input histograms to the output buckets' histograms.
        for bucket in buckets {
            let bucket_start = metrics.bucket_start(bucket.index);
            if (self.start..self.end).contains(&bucket_start) {
                let (_, existing) = &mut histograms[self.bucket_index(bucket_start)?];
                let histogram = existing.as_mut().context("Missing histogram")?;
                histogram.add(&bucket.histogram)?;
            }
        }

        // Compute all desired percentiles for each output bucket.
        let mut result = BTreeMap::new();
        for percentile in percentiles {
            let mut timeseries = Vec::with_capacity(self.num_buckets);
            for (bucket_start, histogram) in &histograms {
                let value = match histogram {
                    Some(histogram) => {
                        let mut millis = histogram.value_at_percentile(*percentile as f64);
                        if !histogram.is_empty() {
                            millis = cmp::max(1, millis);
                        }
                        Some((millis as f64) / 1000.)
                    },
                    None => None,
                };
                timeseries.push((*bucket_start, value));
            }
            result.insert(*percentile, timeseries);
        }

        Ok(result)
    }
}

/// Timeseries with potentially missing values.
pub type Timeseries = Vec<(SystemTime, Option<f64>)>;

/// Integer in [0, 100].
pub type Percentile = usize;
