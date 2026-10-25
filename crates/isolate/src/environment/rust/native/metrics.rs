//! Prometheus metrics and per-call phase timers for rust-native mode
//! (DESIGN.md §7). Registered with the backend's `metrics` macros, in the
//! style of `isolate/src/metrics.rs`.

use std::time::{
    Duration,
    Instant,
};

use metrics::{
    log_counter,
    log_counter_with_labels,
    log_distribution,
    log_distribution_with_labels,
    log_gauge_with_labels,
    register_convex_counter,
    register_convex_gauge,
    register_convex_histogram,
    StaticMetricLabel,
};

register_convex_histogram!(
    RUST_NATIVE_PHASE_SECONDS,
    "Per-call phase timers of a rust-native action",
    &["phase"]
);
register_convex_histogram!(
    RUST_NATIVE_LOOKUP_SECONDS,
    "rust-native: metadata lookup and artifact cache"
);
register_convex_histogram!(
    RUST_NATIVE_ACQUIRE_SECONDS,
    "rust-native: time to get a worker",
    &["outcome"]
);
register_convex_histogram!(
    RUST_NATIVE_SPAWN_SECONDS,
    "rust-native: worker spawn until Ready",
    &["status"]
);
register_convex_histogram!(
    RUST_NATIVE_CALL_SECONDS,
    "rust-native: Call sent until Result received"
);
register_convex_histogram!(
    RUST_NATIVE_TEARDOWN_SECONDS,
    "rust-native: call teardown (release or kill)"
);
register_convex_histogram!(
    RUST_NATIVE_WORKER_RSS_BYTES,
    "rust-native: worker RSS sampled at release"
);
register_convex_counter!(
    RUST_NATIVE_CALLBACKS_TOTAL,
    "rust-native: guest callbacks serviced by the pump",
    &["kind"]
);
register_convex_counter!(
    RUST_NATIVE_IPC_BYTES_TOTAL,
    "rust-native: bytes exchanged with workers",
    &["dir"]
);
register_convex_counter!(
    RUST_NATIVE_WORKER_EXITS_TOTAL,
    "rust-native: why workers ended",
    &["reason"]
);
register_convex_counter!(
    RUST_NATIVE_SPAWN_FAILURES_TOTAL,
    "rust-native: worker spawn failures"
);
register_convex_gauge!(
    RUST_NATIVE_WORKERS_TOTAL,
    "rust-native: workers by state",
    &["state"]
);
register_convex_gauge!(
    RUST_NATIVE_BREAKER_OPEN_TOTAL,
    "rust-native: number of pools whose spawn breaker is open"
);

/// Why a worker process ended (label of `rust_native_worker_exits_total`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum ExitReason {
    Recycle,
    Idle,
    Timeout,
    Oom,
    Stack,
    Signal,
    Exit,
    Sigsys,
    Protocol,
    Shutdown,
    Dead,
    Cancelled,
}

pub fn log_worker_exit(reason: ExitReason) {
    log_counter_with_labels(
        &RUST_NATIVE_WORKER_EXITS_TOTAL,
        1,
        vec![StaticMetricLabel::new("reason", <&'static str>::from(reason))],
    );
}

pub fn log_spawn_failure() {
    log_counter(&RUST_NATIVE_SPAWN_FAILURES_TOTAL, 1);
}

pub fn log_spawn(elapsed: Duration, ok: bool) {
    log_distribution_with_labels(
        &RUST_NATIVE_SPAWN_SECONDS,
        elapsed.as_secs_f64(),
        vec![StaticMetricLabel::new("status", if ok { "ok" } else { "error" })],
    );
}

pub fn log_acquire(elapsed: Duration, spawned: bool) {
    log_distribution_with_labels(
        &RUST_NATIVE_ACQUIRE_SECONDS,
        elapsed.as_secs_f64(),
        vec![StaticMetricLabel::new(
            "outcome",
            if spawned { "spawn" } else { "idle" },
        )],
    );
}

pub fn log_lookup(elapsed: Duration) {
    log_distribution(&RUST_NATIVE_LOOKUP_SECONDS, elapsed.as_secs_f64());
}

pub fn log_teardown(elapsed: Duration) {
    log_distribution(&RUST_NATIVE_TEARDOWN_SECONDS, elapsed.as_secs_f64());
}

pub fn log_worker_rss(bytes: u64) {
    log_distribution(&RUST_NATIVE_WORKER_RSS_BYTES, bytes as f64);
}

pub fn log_workers(idle: usize, busy: usize) {
    log_gauge_with_labels(
        &RUST_NATIVE_WORKERS_TOTAL,
        idle as f64,
        vec![StaticMetricLabel::new("state", "idle")],
    );
    log_gauge_with_labels(
        &RUST_NATIVE_WORKERS_TOTAL,
        busy as f64,
        vec![StaticMetricLabel::new("state", "busy")],
    );
}

pub fn log_breaker_open(open_pools: usize) {
    metrics::log_gauge(&RUST_NATIVE_BREAKER_OPEN_TOTAL, open_pools as f64);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum CallbackKind {
    Submit,
    Sync,
    PollBlock,
    Log,
}

pub fn log_callback(kind: CallbackKind) {
    log_counter_with_labels(
        &RUST_NATIVE_CALLBACKS_TOTAL,
        1,
        vec![StaticMetricLabel::new("kind", <&'static str>::from(kind))],
    );
}

pub fn log_ipc_bytes(dir_in: bool, bytes: usize) {
    log_counter_with_labels(
        &RUST_NATIVE_IPC_BYTES_TOTAL,
        bytes as u64,
        vec![StaticMetricLabel::new("dir", if dir_in { "in" } else { "out" })],
    );
}

/// Phases of one call. `queue` and `checkout` are filled by the caller
/// (runtime), `send`, `guest` and `pump` by [`CallPump`](super::pump::CallPump).
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Phase {
    /// Waiting for the concurrency permit.
    Queue,
    /// Getting a worker (idle pop or spawn).
    Checkout,
    /// Writing the `Call` frame until the socket accepted it.
    Send,
    /// `Call` sent until `Result` received (includes callbacks).
    Guest,
    /// Host time spent servicing guest callbacks (backend work inside the pump).
    Pump,
    /// Whole call from `queue` start to the end of the pump.
    Total,
}

/// Collected per-call timings. Cheap: a few `Duration`s. Emit with
/// [`record`](Self::record) once, at the end of the call.
#[derive(Clone, Debug, Default)]
pub struct CallPhases {
    pub queue: Duration,
    pub checkout: Duration,
    pub send: Duration,
    pub guest: Duration,
    pub pump: Duration,
    pub total: Duration,
    pub guest_reported: Duration,
    /// Native-runtime-only host overhead: building the plan / call spec and
    /// the post-pump work (envelope parse, lease release). Excludes `queue`,
    /// `checkout` and `total`, and the shared code around the runtime
    /// (`load_environment`, executor setup), so it isolates what native adds.
    pub runtime: Duration,
}

impl CallPhases {
    pub fn get(&self, phase: Phase) -> Duration {
        match phase {
            Phase::Queue => self.queue,
            Phase::Checkout => self.checkout,
            Phase::Send => self.send,
            Phase::Guest => self.guest,
            Phase::Pump => self.pump,
            Phase::Total => self.total,
        }
    }

    pub fn set(&mut self, phase: Phase, d: Duration) {
        match phase {
            Phase::Queue => self.queue = d,
            Phase::Checkout => self.checkout = d,
            Phase::Send => self.send = d,
            Phase::Guest => self.guest = d,
            Phase::Pump => self.pump = d,
            Phase::Total => self.total = d,
        }
    }

    /// Observes every phase on the histograms; also feeds
    /// `rust_native_call_seconds` with the guest phase.
    pub fn record(&self) {
        for phase in [
            Phase::Queue,
            Phase::Checkout,
            Phase::Send,
            Phase::Guest,
            Phase::Pump,
            Phase::Total,
        ] {
            log_distribution_with_labels(
                &RUST_NATIVE_PHASE_SECONDS,
                self.get(phase).as_secs_f64(),
                vec![StaticMetricLabel::new("phase", <&'static str>::from(phase))],
            );
        }
        log_distribution(&RUST_NATIVE_CALL_SECONDS, self.guest.as_secs_f64());
    }

    /// One line for `RUST_NATIVE_TRACE=1`.
    pub fn trace_line(&self) -> String {
        format!(
            "queue={:?} checkout={:?} send={:?} guest={:?} (guest_reported={:?}) pump={:?} total={:?} runtime={:?}",
            self.queue, self.checkout, self.send, self.guest, self.guest_reported, self.pump, self.total, self.runtime
        )
    }
}

/// Stopwatch that adds elapsed time to a `Duration` accumulator on `stop`.
pub struct PhaseTimer(Instant);

impl PhaseTimer {
    pub fn start() -> Self {
        Self(Instant::now())
    }

    pub fn stop(self) -> Duration {
        self.0.elapsed()
    }
}
