//! A time-bucketed counter store for usage-limit enforcement.
//!
//! Usage is grouped into fixed-width time buckets and kept for a bounded
//! window.
//!
//! - **f64**: integer counts stay exact up to 2^53, so adding a small delta to
//!   a large count retains its precision (f32 would absorb it), and fractional
//!   units like GB·s are representable
//! - **Writes can arrive out of order**: seed rows can lag live usage by up to
//!   ~90 minutes, so a write can land in an older bucket.
//! - **Seeding keeps the larger value**: a seed row and live recording can
//!   count the same usage, so taking the max avoids double-counting and is safe
//!   to apply more than once.

use std::{
    collections::{
        BTreeMap,
        HashMap,
    },
    ops::Range,
    time::{
        Duration,
        SystemTime,
    },
};

pub struct SeedableCounterStore {
    base_ts: SystemTime,
    bucket_width: Duration,
    max_buckets: u64,
    counters: HashMap<String, BTreeMap<u64, f64>>,
}

impl SeedableCounterStore {
    /// Bucket 0 starts at `base_ts` and each bucket is `bucket_width` wide.
    /// Timestamps before `base_ts` can't be stored, so set `base_ts` earlier
    /// than the oldest data you'll seed.
    pub fn new(base_ts: SystemTime, bucket_width: Duration, max_buckets: u64) -> Self {
        assert!(bucket_width.as_secs() >= 60, "bucket_width must be >= 60s");
        assert!(max_buckets > 0, "max_buckets must be nonzero");
        Self {
            base_ts,
            bucket_width,
            max_buckets,
            counters: HashMap::new(),
        }
    }

    fn bucket_index(&self, ts: SystemTime) -> Option<u64> {
        let since_base = ts.duration_since(self.base_ts).ok()?;
        Some(since_base.as_secs() / self.bucket_width.as_secs())
    }

    /// Which bucket `ts` belongs to, or `None` if `ts` is before `base_ts`
    /// or so far behind `now` that it has already fallen out of retention.
    fn retained_bucket_index(&self, ts: SystemTime, now: SystemTime) -> Option<u64> {
        let index = self.bucket_index(ts)?;
        if let Some(now_index) = self.bucket_index(now)
            && index.saturating_add(self.max_buckets) <= now_index
        {
            return None;
        }
        Some(index)
    }

    fn bucket_entry(&mut self, metric_name: &str, index: u64) -> &mut f64 {
        if !self.counters.contains_key(metric_name) {
            // First write for this metric; allocate its bucket map.
            self.counters
                .insert(metric_name.to_string(), BTreeMap::new());
        }
        let buckets = self
            .counters
            .get_mut(metric_name)
            .expect("metric inserted above");
        buckets.entry(index).or_insert(0.0)
    }

    /// Drop buckets that are now too old to fall in any window.
    fn prune(&mut self, now: SystemTime) {
        let Some(now_index) = self.bucket_index(now) else {
            return;
        };
        let floor = now_index.saturating_sub(self.max_buckets - 1);
        if floor == 0 {
            return;
        }
        for buckets in self.counters.values_mut() {
            // Keep buckets at or after the floor; drop older ones.
            *buckets = buckets.split_off(&floor);
        }
    }

    /// The one write path: update the bucket, then drop expired buckets.
    fn write_bucket(
        &mut self,
        metric_name: &str,
        index: u64,
        now: SystemTime,
        merge: impl FnOnce(&mut f64),
    ) {
        merge(self.bucket_entry(metric_name, index));
        self.prune(now);
    }

    /// Add live usage to the bucket for `ts`. A `ts` too old to matter is
    /// dropped; a `ts` past `now` (clock skew) is treated as `now`.
    pub fn add(&mut self, metric_name: &str, ts: SystemTime, delta: f64, now: SystemTime) {
        let ts = ts.min(now);
        let Some(index) = self.retained_bucket_index(ts, now) else {
            return;
        };
        self.write_bucket(metric_name, index, now, |count| {
            *count += delta;
        });
    }

    /// Set a bucket from a seed row, keeping the larger of the stored and
    /// seeded value. Safe to replay and to mix with live recording. A value
    /// above the true total would stick, so seeds are assumed not to exceed
    /// it. A `ts` more than one bucket past `now` is dropped as clock skew.
    pub fn seed_counter(&mut self, metric_name: &str, ts: SystemTime, value: f64, now: SystemTime) {
        if ts > now + self.bucket_width {
            return;
        }
        let Some(index) = self.retained_bucket_index(ts, now) else {
            return;
        };
        self.write_bucket(metric_name, index, now, |count| {
            *count = count.max(value);
        });
    }

    /// Total across the buckets in `range` (start included, end excluded;
    /// both must land on bucket boundaries). An unknown metric or empty
    /// range totals 0.
    pub fn sum_counter(&self, metric_name: &str, range: &Range<SystemTime>) -> f64 {
        let Some(buckets) = self.counters.get(metric_name) else {
            return 0.0;
        };
        if range.end <= range.start {
            return 0.0;
        }
        // A start before `base_ts` counts from the first bucket.
        let start_index = self.bucket_index(range.start).unwrap_or(0);
        let Some(end_index) = self.bucket_index(range.end - Duration::from_nanos(1)) else {
            // The whole range is before `base_ts`.
            return 0.0;
        };
        buckets.range(start_index..=end_index).map(|(_, c)| c).sum()
    }
}
