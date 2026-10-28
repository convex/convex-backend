use std::{
    sync::{
        atomic::{
            AtomicU64,
            Ordering,
        },
        Arc,
    },
    time::Duration,
};

/// Callback invoked with the read-, write-, and throttle-side durations
/// accumulated during the most recent reporting window. Lets a caller in a
/// crate that knows extra labels (e.g. a db-cluster migration) emit a metric
/// for the read/write/throttle split measured by generic pipeline code that
/// does not know those labels.
pub type ReadWriteReporter = Arc<dyn Fn(Duration, Duration, Duration) + Send + Sync>;

/// Accumulates how long a producer-consumer copy pipeline spends on the read
/// side (pulling from the source), the write side (pushing to the
/// destination), and waiting on a rate limiter (throttle), so callers can tell
/// whether a phase is read-, write-, or throttle-bound.
///
/// Cheap to clone and share between the producer and consumer tasks: all
/// `record_*` methods are lock-free atomic adds.
///
/// Note: when the consumer runs several writers concurrently, `throttle` is
/// summed across them, so it can exceed wall-clock time and (compared with a
/// single-task producer's read/write) somewhat over-weights throttle in the
/// verdict. It is still a reliable signal of *whether* the rate limiter is the
/// active constraint.
#[derive(Clone, Default)]
pub struct ReadWriteBalance {
    read_nanos: Arc<AtomicU64>,
    write_nanos: Arc<AtomicU64>,
    throttle_nanos: Arc<AtomicU64>,
}

impl ReadWriteBalance {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_read(&self, elapsed: Duration) {
        self.read_nanos
            .fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
    }

    pub fn record_write(&self, elapsed: Duration) {
        self.write_nanos
            .fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Time spent blocked on a rate limiter before a write. This is a
    /// deliberate (self-imposed) cap rather than the destination being
    /// slow, so it is tracked separately from `write`.
    pub fn record_throttle(&self, elapsed: Duration) {
        self.throttle_nanos
            .fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Total (read, write, throttle) durations accumulated so far.
    pub fn totals(&self) -> (Duration, Duration, Duration) {
        (
            Duration::from_nanos(self.read_nanos.load(Ordering::Relaxed)),
            Duration::from_nanos(self.write_nanos.load(Ordering::Relaxed)),
            Duration::from_nanos(self.throttle_nanos.load(Ordering::Relaxed)),
        )
    }
}
