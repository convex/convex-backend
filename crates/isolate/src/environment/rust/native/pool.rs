//! Warm worker pool keyed by `(artifact sha256, env hash)` (DESIGN.md §1.2).
//!
//! * `acquire` pops an idle worker or spawns one on the dedicated
//!   `rust-native-spawner` thread (`PR_SET_PDEATHSIG` fires when the forking
//!   *thread* exits, so spawning must not happen on tokio blocking threads).
//! * One call per worker. A [`Lease`] is released clean (worker returns to
//!   idle unless a recycle threshold is hit) or failed (worker is killed).
//!   Dropping a lease without releasing it kills the worker (cancellation).
//! * After a release the pool refills `min_idle` in the background.
//! * Spawn failures feed a per-key circuit breaker; call failures do not.
//! * An idle reaper (`reap`, driven by a tokio interval) and `shutdown`.

use std::{
    collections::{
        HashMap,
        VecDeque,
    },
    sync::{
        atomic::{
            AtomicUsize,
            Ordering,
        },
        mpsc,
        Arc,
        Weak,
    },
    time::{
        Duration,
        Instant,
    },
};

use parking_lot::Mutex;
use tokio::sync::oneshot;

use super::{
    metrics::{
        self,
        ExitReason,
    },
    Artifact,
    NativeFailure,
    SandboxPlan,
    Spawner,
};

/// Knob values (`knobs.rs` is WN-06's; they fill this in).
#[derive(Clone, Debug)]
pub struct PoolConfig {
    /// `RUST_NATIVE_MIN_IDLE`: idle workers kept per key.
    pub min_idle: usize,
    /// `RUST_NATIVE_MAX_IDLE`: idle workers over all keys.
    pub max_idle: usize,
    /// `RUST_NATIVE_IDLE_TIMEOUT_SECS`
    pub idle_timeout: Duration,
    /// `RUST_NATIVE_MAX_CALLS_PER_WORKER`
    pub max_calls_per_worker: u32,
    /// `RUST_NATIVE_RECYCLE_RSS_BYTES`
    pub recycle_rss_bytes: u64,
    /// How often the reaper runs (10 s).
    pub reaper_interval: Duration,
    /// Spawn failures within `breaker_window` that open the breaker (3).
    pub breaker_threshold: usize,
    pub breaker_window: Duration,
    /// First backoff (1 s), doubled per consecutive open, up to the cap.
    pub breaker_backoff_base: Duration,
    pub breaker_backoff_cap: Duration,
    /// Time idle workers get to exit after `Shutdown` before `SIGKILL` (500 ms).
    pub shutdown_grace: Duration,
    /// `RUST_NATIVE_SPAWN_THREADS`: max threads spawning workers in parallel.
    pub spawn_threads: usize,
    /// `RUST_NATIVE_PROBE_EVERY`: the RSS probe runs on every Nth call of a
    /// worker (and its first, and slow ones). 1 probes every call.
    ///
    /// Trade-off: skipping the RSS probe means growth past
    /// `recycle_rss_bytes` is noticed up to N - 1 calls late (the hard memory
    /// limit still applies meanwhile). The runaway-thread check is NOT
    /// sampled: a leaked guest thread must never reach another call, so it is
    /// checked at every release (one cheap thread-count query).
    pub probe_every: u32,
    /// Calls at least this long are always probed.
    pub probe_slow_call: Duration,
    /// `RUST_NATIVE_IDLE_RECYCLE_RSS_BYTES`: the reaper recycles an idle
    /// worker whose RSS is above this, but only while another idle worker of
    /// the same key exists (0 disables). Backstop for guests without
    /// `convex_native_trim` and for allocators that keep freed pages.
    pub idle_recycle_rss_bytes: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_idle: 1,
            max_idle: 4,
            idle_timeout: Duration::from_secs(300),
            max_calls_per_worker: 1000,
            recycle_rss_bytes: 256 * 1024 * 1024,
            reaper_interval: Duration::from_secs(10),
            breaker_threshold: 3,
            breaker_window: Duration::from_secs(60),
            breaker_backoff_base: Duration::from_secs(1),
            breaker_backoff_cap: Duration::from_secs(300),
            shutdown_grace: Duration::from_millis(500),
            spawn_threads: 4,
            probe_every: 1,
            probe_slow_call: Duration::from_millis(20),
            idle_recycle_rss_bytes: 0,
        }
    }
}

/// What the pool needs from a worker handle (besides what the pump needs).
pub trait PoolWorker: Send + 'static {
    /// The process has not exited. Cheap (`try_wait`).
    fn is_alive(&mut self) -> bool;
    /// Current resident set size, if it can be sampled.
    fn rss_bytes(&mut self) -> Option<u64>;
    /// A detached sampler for [`Self::rss_bytes`], so the reaper can read
    /// RSS (a `/proc` read or syscall) without holding the pool lock. `None`
    /// means the worker is not sampled by the reaper.
    fn rss_sampler(&self) -> Option<Box<dyn Fn() -> Option<u64> + Send>> {
        None
    }
    /// The guest left threads running after its call returned (more threads
    /// than right after `Ready`); such a worker must not be reused.
    fn has_extra_threads(&mut self) -> bool {
        false
    }
    /// Kills (and reaps) the process group. Idempotent.
    fn terminate(&mut self);
    /// Best-effort `Shutdown` frame; the pool kills the worker after the
    /// grace period anyway.
    fn request_shutdown(&mut self) {
        self.terminate();
    }
    fn pid(&self) -> Option<u32> {
        None
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct PoolKey {
    pub sha256: String,
    pub env_hash: u64,
}

/// For the info endpoint (`/api/rust_native/info`, NATIVE.md §5.1).
#[derive(Clone, Debug, Default)]
pub struct KeyStats {
    pub artifact: String,
    pub env_hash: u64,
    pub live: usize,
    pub idle: usize,
    pub busy: usize,
    pub spawned: u64,
    pub crashes: u64,
    pub last_crash: Option<String>,
    pub breaker_open: bool,
    pub idle_pids: Vec<u32>,
}

// ---------------------------------------------------------------------------
// Circuit breaker
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Breaker {
    failures: VecDeque<Instant>,
    open_until: Option<Instant>,
    next_backoff: Duration,
    /// Has opened since the last success: a single further failure reopens.
    tripped: bool,
    last_error: Option<String>,
}

impl Breaker {
    fn new(cfg: &PoolConfig) -> Self {
        Self {
            failures: VecDeque::new(),
            open_until: None,
            next_backoff: cfg.breaker_backoff_base,
            tripped: false,
            last_error: None,
        }
    }

    /// `Some(last error)` while open.
    fn check(&self, now: Instant) -> Option<&str> {
        match self.open_until {
            Some(until) if until > now => Some(self.last_error.as_deref().unwrap_or("spawn failed")),
            _ => None,
        }
    }

    fn failure(&mut self, now: Instant, err: String, cfg: &PoolConfig) {
        self.last_error = Some(err);
        self.failures.push_back(now);
        while let Some(front) = self.failures.front() {
            if now.duration_since(*front) > cfg.breaker_window {
                self.failures.pop_front();
            } else {
                break;
            }
        }
        if self.tripped || self.failures.len() >= cfg.breaker_threshold {
            self.open_until = Some(now + self.next_backoff);
            self.next_backoff = (self.next_backoff * 2).min(cfg.breaker_backoff_cap);
            self.tripped = true;
        }
    }

    fn success(&mut self, cfg: &PoolConfig) {
        *self = Self::new(cfg);
    }

    fn is_open(&self, now: Instant) -> bool {
        self.open_until.is_some_and(|u| u > now)
    }

    /// Worth keeping the key's state around while it holds failure history.
    fn has_history(&self) -> bool {
        self.tripped || !self.failures.is_empty()
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

static IDLE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct Idle<W> {
    /// Identifies this entry across lock releases (see `reap`).
    seq: u64,
    worker: W,
    calls: u32,
    since: Instant,
}

struct KeyState<W> {
    artifact: Arc<Artifact>,
    plan: SandboxPlan,
    /// LIFO: the most recently used worker is reused first.
    idle: Vec<Idle<W>>,
    busy: usize,
    pending_spawns: usize,
    breaker: Breaker,
    spawned: u64,
    crashes: u64,
    last_crash: Option<String>,
}

struct State<W> {
    keys: HashMap<PoolKey, KeyState<W>>,
    shutdown: bool,
}

impl<W> State<W> {
    fn total_idle(&self) -> usize {
        self.keys.values().map(|k| k.idle.len()).sum()
    }

    fn total_busy(&self) -> usize {
        self.keys.values().map(|k| k.busy).sum()
    }

    fn publish(&self, now: Instant) {
        metrics::log_workers(self.total_idle(), self.total_busy());
        metrics::log_breaker_open(
            self.keys
                .values()
                .filter(|k| k.breaker.is_open(now))
                .count(),
        );
    }
}

type Job = Box<dyn FnOnce() + Send>;

struct Inner<S: Spawner>
where
    S::Worker: PoolWorker,
{
    spawner: Arc<S>,
    cfg: PoolConfig,
    state: Mutex<State<S::Worker>>,
    jobs: Mutex<Option<mpsc::Sender<Job>>>,
    /// Spawner threads: started on demand up to `cfg.spawn_threads`.
    spawners: Spawners,
}

/// Bookkeeping for the on-demand spawner threads sharing one job queue.
struct Spawners {
    rx: Arc<Mutex<mpsc::Receiver<Job>>>,
    /// Jobs submitted and not yet picked up.
    queued: AtomicUsize,
    /// Threads waiting for a job.
    idle: AtomicUsize,
    total: AtomicUsize,
}

impl<S: Spawner> Drop for Inner<S>
where
    S::Worker: PoolWorker,
{
    fn drop(&mut self) {
        let state = self.state.get_mut();
        for ks in state.keys.values_mut() {
            for mut idle in ks.idle.drain(..) {
                idle.worker.terminate();
            }
        }
    }
}

pub struct Pool<S: Spawner>
where
    S::Worker: PoolWorker,
{
    inner: Arc<Inner<S>>,
}

impl<S: Spawner> Clone for Pool<S>
where
    S::Worker: PoolWorker,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<S: Spawner> Pool<S>
where
    S::Worker: PoolWorker,
{
    /// Starts the spawner thread, and the reaper when called inside a tokio
    /// runtime.
    pub fn new(spawner: S, cfg: PoolConfig) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let pool = Self {
            inner: Arc::new(Inner {
                spawner: Arc::new(spawner),
                cfg,
                state: Mutex::new(State {
                    keys: HashMap::new(),
                    shutdown: false,
                }),
                jobs: Mutex::new(Some(tx)),
                spawners: Spawners {
                    rx: Arc::new(Mutex::new(rx)),
                    queued: AtomicUsize::new(0),
                    idle: AtomicUsize::new(0),
                    total: AtomicUsize::new(0),
                },
            }),
        };
        pool.start_reaper();
        pool
    }

    fn start_reaper(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let weak: Weak<Inner<S>> = Arc::downgrade(&self.inner);
        let interval = self.inner.cfg.reaper_interval;
        handle.spawn(async move {
            let mut tick = tokio::time::interval(interval);
            tick.tick().await;
            loop {
                tick.tick().await;
                let Some(inner) = weak.upgrade() else { return };
                if inner.state.lock().shutdown {
                    return;
                }
                Pool { inner }.reap(Instant::now());
            }
        });
    }

    pub fn config(&self) -> &PoolConfig {
        &self.inner.cfg
    }

    /// Queues a spawn job. Worker processes are forked from dedicated
    /// threads (never tokio's: `PR_SET_PDEATHSIG` fires when the forking
    /// *thread* exits). Up to `spawn_threads` run in parallel; a new thread is
    /// started when more jobs are queued than threads are waiting.
    fn submit_job(&self, job: Job) -> bool {
        let sp = &self.inner.spawners;
        sp.queued.fetch_add(1, Ordering::SeqCst);
        let sent = match &*self.inner.jobs.lock() {
            Some(tx) => tx.send(job).is_ok(),
            None => false,
        };
        if !sent {
            sp.queued.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        let max = self.inner.cfg.spawn_threads.max(1);
        if sp.queued.load(Ordering::SeqCst) > sp.idle.load(Ordering::SeqCst)
            && sp.total.load(Ordering::SeqCst) < max
        {
            sp.total.fetch_add(1, Ordering::SeqCst);
            let rx = sp.rx.clone();
            let weak: Weak<Inner<S>> = Arc::downgrade(&self.inner);
            let started = std::thread::Builder::new()
                .name("rust-native-spawner".into())
                .spawn(move || loop {
                    // Counters live in `Inner`; stop if it is gone.
                    let Some(inner) = weak.upgrade() else { return };
                    inner.spawners.idle.fetch_add(1, Ordering::SeqCst);
                    drop(inner);
                    // Hold the receiver lock only while waiting for a job.
                    let job = rx.lock().recv();
                    let Some(inner) = weak.upgrade() else { return };
                    inner.spawners.idle.fetch_sub(1, Ordering::SeqCst);
                    let Ok(job) = job else { return };
                    inner.spawners.queued.fetch_sub(1, Ordering::SeqCst);
                    drop(inner);
                    job();
                });
            if started.is_err() {
                // Cannot start another thread; an existing one (if any) will
                // run the job.
                sp.total.fetch_sub(1, Ordering::SeqCst);
                if sp.total.load(Ordering::SeqCst) == 0 {
                    tracing::error!("failed to start a rust-native-spawner thread");
                }
            }
        }
        true
    }

    /// Gets a worker for one call: idle reuse or a fresh spawn. Cancel-safe:
    /// a worker spawned for a dropped `acquire` is parked as idle.
    pub async fn acquire(
        &self,
        artifact: &Arc<Artifact>,
        env_hash: u64,
        plan: &SandboxPlan,
    ) -> Result<Lease<S>, NativeFailure> {
        let started = Instant::now();
        let key = PoolKey {
            sha256: artifact.sha256.clone(),
            env_hash,
        };
        let rx = {
            let now = Instant::now();
            let mut dead = Vec::new();
            let mut st = self.inner.state.lock();
            if st.shutdown {
                return Err(NativeFailure::StartFailed("the worker pool is shut down".into()));
            }
            let cfg = &self.inner.cfg;
            let ks = st.keys.entry(key.clone()).or_insert_with(|| KeyState {
                artifact: artifact.clone(),
                plan: plan.clone(),
                idle: Vec::new(),
                busy: 0,
                pending_spawns: 0,
                breaker: Breaker::new(cfg),
                spawned: 0,
                crashes: 0,
                last_crash: None,
            });
            // `ks.plan` was set when the key was created: the plan only differs
            // by environment, which the key's `env_hash` covers.
            let mut found = None;
            while let Some(mut idle) = ks.idle.pop() {
                if idle.worker.is_alive() {
                    found = Some(idle);
                    break;
                }
                dead.push(idle.worker);
            }
            if let Some(idle) = found {
                ks.busy += 1;
                st.publish(now);
                drop(st);
                for mut w in dead {
                    w.terminate();
                    metrics::log_worker_exit(ExitReason::Dead);
                }
                metrics::log_acquire(started.elapsed(), false);
                return Ok(Lease {
                    pool: self.clone(),
                    key,
                    worker: Some(idle.worker),
                    calls: idle.calls,
                });
            }
            for mut w in dead {
                w.terminate();
                metrics::log_worker_exit(ExitReason::Dead);
            }
            if let Some(err) = ks.breaker.check(now) {
                return Err(NativeFailure::StartFailed(err.to_string()));
            }
            ks.pending_spawns += 1;
            let (tx, rx) = oneshot::channel();
            let me = self.clone();
            let k = key.clone();
            let art = artifact.clone();
            let plan = plan.clone();
            let queued = self.submit_job(Box::new(move || {
                let res = me.do_spawn(&k, &art, &plan);
                // The receiver is gone when `acquire` was cancelled: keep the
                // worker as idle. It is counted busy only once leased.
                if let Err(Ok(worker)) = tx.send(res) {
                    me.park(&k, worker, 0);
                }
            }));
            if !queued {
                ks_pending_dec(&mut st, &key);
                return Err(NativeFailure::StartFailed("the worker pool is shut down".into()));
            }
            rx
        };
        let spawned = match rx.await {
            Ok(Ok(worker)) => worker,
            Ok(Err(e)) => return Err(NativeFailure::StartFailed(e)),
            Err(_) => return Err(NativeFailure::StartFailed("spawner thread went away".into())),
        };
        {
            let mut st = self.inner.state.lock();
            if st.shutdown {
                drop(st);
                let mut w = spawned;
                w.terminate();
                metrics::log_worker_exit(ExitReason::Shutdown);
                return Err(NativeFailure::StartFailed("the worker pool is shut down".into()));
            }
            if let Some(ks) = st.keys.get_mut(&key) {
                ks.busy += 1;
            }
            st.publish(Instant::now());
        }
        metrics::log_acquire(started.elapsed(), true);
        Ok(Lease {
            pool: self.clone(),
            key,
            worker: Some(spawned),
            calls: 0,
        })
    }

    /// Runs on the spawner thread.
    fn do_spawn(
        &self,
        key: &PoolKey,
        artifact: &Arc<Artifact>,
        plan: &SandboxPlan,
    ) -> Result<S::Worker, String> {
        let t = Instant::now();
        let res = self.inner.spawner.spawn(artifact, plan);
        let ok = res.is_ok();
        metrics::log_spawn(t.elapsed(), ok);
        let now = Instant::now();
        let mut st = self.inner.state.lock();
        if let Some(ks) = st.keys.get_mut(key) {
            ks.pending_spawns = ks.pending_spawns.saturating_sub(1);
            match &res {
                Ok(_) => {
                    ks.breaker.success(&self.inner.cfg);
                    ks.spawned += 1;
                },
                Err(e) => ks.breaker.failure(now, format!("{e:#}"), &self.inner.cfg),
            }
        }
        st.publish(now);
        drop(st);
        match res {
            Ok(w) => Ok(w),
            Err(e) => {
                metrics::log_spawn_failure();
                tracing::warn!("rust-native worker spawn failed: {e:#}");
                Err(format!("{e:#}"))
            },
        }
    }

    /// Puts a worker into idle, enforcing `max_idle` and shutdown.
    fn park(&self, key: &PoolKey, worker: S::Worker, calls: u32) {
        let mut evicted: Vec<S::Worker> = Vec::new();
        let mut worker = Some(worker);
        {
            let now = Instant::now();
            let mut st = self.inner.state.lock();
            if !st.shutdown
                && let Some(ks) = st.keys.get_mut(key) {
                    ks.idle.push(Idle {
                        seq: IDLE_SEQ.fetch_add(1, Ordering::Relaxed),
                        worker: worker.take().unwrap(),
                        calls,
                        since: now,
                    });
                }
            // Cap the total number of idle workers: drop the oldest.
            while st.total_idle() > self.inner.cfg.max_idle {
                let oldest = st
                    .keys
                    .iter()
                    .filter_map(|(k, ks)| {
                        ks.idle
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, i)| i.since)
                            .map(|(idx, i)| (k.clone(), idx, i.since))
                    })
                    .min_by_key(|(_, _, since)| *since);
                let Some((k, idx, _)) = oldest else { break };
                let ks = st.keys.get_mut(&k).unwrap();
                evicted.push(ks.idle.remove(idx).worker);
            }
            st.publish(now);
        }
        if let Some(w) = worker {
            evicted.push(w);
        }
        for mut w in evicted {
            w.terminate();
            metrics::log_worker_exit(ExitReason::Recycle);
        }
    }

    /// Spawns in the background until the key has `min_idle` idle workers
    /// (counting spawns in flight).
    fn refill(&self, key: &PoolKey) {
        let target = self.inner.cfg.min_idle;
        self.refill_to(key, target);
    }

    /// Starts workers for an artifact in the background so the first call
    /// finds one idle (the first load of a fresh library can take hundreds of
    /// ms on macOS). Never blocks; a no-op when the key already has a worker
    /// or a spawn in flight, the breaker is open or the pool is shut down.
    pub fn prewarm(&self, artifact: &Arc<Artifact>, env_hash: u64, plan: &SandboxPlan) {
        let key = PoolKey {
            sha256: artifact.sha256.clone(),
            env_hash,
        };
        {
            let mut st = self.inner.state.lock();
            if st.shutdown {
                return;
            }
            let cfg = &self.inner.cfg;
            st.keys.entry(key.clone()).or_insert_with(|| KeyState {
                artifact: artifact.clone(),
                plan: plan.clone(),
                idle: Vec::new(),
                busy: 0,
                pending_spawns: 0,
                breaker: Breaker::new(cfg),
                spawned: 0,
                crashes: 0,
                last_crash: None,
            });
        }
        let target = self.inner.cfg.min_idle.max(1);
        self.refill_to(&key, target);
    }

    fn refill_to(&self, key: &PoolKey, target: usize) {
        let (artifact, plan, n) = {
            let mut st = self.inner.state.lock();
            if st.shutdown {
                return;
            }
            let now = Instant::now();
            let cfg = &self.inner.cfg;
            let Some(ks) = st.keys.get_mut(key) else { return };
            if ks.breaker.check(now).is_some() {
                return;
            }
            let have = ks.idle.len() + ks.pending_spawns;
            let n = target.min(cfg.max_idle).saturating_sub(have);
            if n == 0 {
                return;
            }
            ks.pending_spawns += n;
            (ks.artifact.clone(), ks.plan.clone(), n)
        };
        for _ in 0..n {
            let me = self.clone();
            let k = key.clone();
            let art = artifact.clone();
            let plan = plan.clone();
            let queued = self.submit_job(Box::new(move || {
                if let Ok(w) = me.do_spawn(&k, &art, &plan) {
                    me.park(&k, w, 0);
                }
            }));
            if !queued {
                let mut st = self.inner.state.lock();
                ks_pending_dec(&mut st, key);
            }
        }
    }

    /// Kills idle workers older than `idle_timeout` and forgets empty keys.
    pub fn reap(&self, now: Instant) {
        let mut victims: Vec<S::Worker> = Vec::new();
        let mut bloated: Vec<PoolKey> = Vec::new();
        let mut recycled = 0usize;
        let mut spare: Vec<PoolKey> = Vec::new();
        // RSS is sampled between two short critical sections, so a slow
        // `/proc` read never stalls `acquire`/`release`. Entries that appear
        // in between are simply not judged until the next tick.
        let limit = self.inner.cfg.idle_recycle_rss_bytes;
        let mut rss: HashMap<u64, u64> = HashMap::new();
        if limit > 0 {
            let samplers: Vec<(u64, Box<dyn Fn() -> Option<u64> + Send>)> = {
                let st = self.inner.state.lock();
                st.keys
                    .values()
                    .flat_map(|ks| ks.idle.iter())
                    .filter_map(|i| Some((i.seq, i.worker.rss_sampler()?)))
                    .collect()
            };
            for (seq, f) in samplers {
                if let Some(r) = f() {
                    rss.insert(seq, r);
                }
            }
        }
        {
            let mut st = self.inner.state.lock();
            let timeout = self.inner.cfg.idle_timeout;
            for ks in st.keys.values_mut() {
                let mut i = 0;
                while i < ks.idle.len() {
                    if now.saturating_duration_since(ks.idle[i].since) >= timeout {
                        victims.push(ks.idle.remove(i).worker);
                    } else {
                        i += 1;
                    }
                }
            }
            if limit > 0 {
                for (key, ks) in st.keys.iter_mut() {
                    // Recycle bloated idle workers, but always keep one idle
                    // worker (the most recently used if all are bloated), so
                    // capacity never drops.
                    let mut i = 0;
                    while i < ks.idle.len() && ks.idle.len() >= 2 {
                        if rss.get(&ks.idle[i].seq).is_some_and(|&r| r > limit) {
                            bloated.push(key.clone());
                            victims.push(ks.idle.remove(i).worker);
                            recycled += 1;
                        } else {
                            i += 1;
                        }
                    }
                    // The kept one is bloated too: spawn a spare now so the
                    // next tick can replace it.
                    if ks.idle.len() == 1
                        && ks.busy == 0
                        && ks.pending_spawns == 0
                        && rss.get(&ks.idle[0].seq).is_some_and(|&r| r > limit)
                    {
                        spare.push(key.clone());
                    }
                }
            }
            st.keys.retain(|_, ks| {
                !ks.idle.is_empty()
                    || ks.busy > 0
                    || ks.pending_spawns > 0
                    || ks.breaker.has_history()
            });
            st.publish(now);
        }
        let n_idle = victims.len() - recycled;
        for (i, mut w) in victims.into_iter().enumerate() {
            w.terminate();
            // Idle-timeout victims were pushed first, RSS victims after.
            metrics::log_worker_exit(if i < n_idle { ExitReason::Idle } else { ExitReason::Recycle });
        }
        for key in bloated {
            self.refill(&key);
        }
        for key in spare {
            self.refill_to(&key, 2);
        }
    }

    pub fn stats(&self) -> Vec<KeyStats> {
        let now = Instant::now();
        let mut st = self.inner.state.lock();
        st.keys
            .iter_mut()
            .map(|(k, ks)| KeyStats {
                artifact: k.sha256.clone(),
                env_hash: k.env_hash,
                live: ks.idle.len() + ks.busy,
                idle: ks.idle.len(),
                busy: ks.busy,
                spawned: ks.spawned,
                crashes: ks.crashes,
                last_crash: ks.last_crash.clone(),
                breaker_open: ks.breaker.is_open(now),
                idle_pids: ks.idle.iter().filter_map(|i| i.worker.pid()).collect(),
            })
            .collect()
    }

    /// Stops the pool: idle workers get `Shutdown`, then `SIGKILL` after the
    /// grace period. Leases in flight are killed when released.
    pub async fn shutdown(&self) {
        let mut workers: Vec<S::Worker> = Vec::new();
        {
            let mut st = self.inner.state.lock();
            st.shutdown = true;
            for ks in st.keys.values_mut() {
                workers.extend(ks.idle.drain(..).map(|i| i.worker));
            }
            st.publish(Instant::now());
        }
        // Closing the channel stops the spawner thread after queued jobs.
        *self.inner.jobs.lock() = None;
        for w in &mut workers {
            w.request_shutdown();
        }
        let deadline = Instant::now() + self.inner.cfg.shutdown_grace;
        while Instant::now() < deadline && workers.iter_mut().any(|w| w.is_alive()) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        for mut w in workers {
            w.terminate();
            metrics::log_worker_exit(ExitReason::Shutdown);
        }
    }
}

fn ks_pending_dec<W>(st: &mut State<W>, key: &PoolKey) {
    if let Some(ks) = st.keys.get_mut(key) {
        ks.pending_spawns = ks.pending_spawns.saturating_sub(1);
    }
}

// ---------------------------------------------------------------------------
// Lease
// ---------------------------------------------------------------------------

/// A worker checked out for exactly one call.
pub struct Lease<S: Spawner>
where
    S::Worker: PoolWorker,
{
    pool: Pool<S>,
    key: PoolKey,
    worker: Option<S::Worker>,
    calls: u32,
}

impl<S: Spawner> Lease<S>
where
    S::Worker: PoolWorker,
{
    pub fn worker_mut(&mut self) -> &mut S::Worker {
        self.worker.as_mut().expect("lease already released")
    }

    /// Calls this worker has completed before this lease.
    pub fn calls_before(&self) -> u32 {
        self.calls
    }

    /// The call ended cleanly (a Result came back). The worker is reused
    /// unless a recycle threshold is hit.
    pub fn release_clean(self) {
        // Unknown duration: always probe.
        self.release_clean_after(Duration::MAX);
    }

    /// Like [`release_clean`](Self::release_clean) but runs the RSS and
    /// thread probes only on sampled calls (see `PoolConfig::probe_every`):
    /// a worker's first call, every Nth call, and calls that took at least
    /// `probe_slow_call`.
    pub fn release_clean_after(mut self, call_elapsed: Duration) {
        let t = Instant::now();
        let mut worker = self.worker.take().unwrap();
        let cfg = &self.pool.inner.cfg;
        let calls = self.calls + 1;
        let every = cfg.probe_every.max(1);
        let probe = every == 1
            || calls == 1
            || calls % every == 0
            || call_elapsed >= cfg.probe_slow_call;
        let rss = if probe { worker.rss_bytes() } else { None };
        if let Some(rss) = rss {
            metrics::log_worker_rss(rss);
        }
        let reason = if calls >= cfg.max_calls_per_worker
            || rss.is_some_and(|r| r > cfg.recycle_rss_bytes)
        {
            Some(ExitReason::Recycle)
        } else if !worker.is_alive() {
            Some(ExitReason::Dead)
        } else if worker.has_extra_threads() {
            Some(ExitReason::Recycle)
        } else {
            None
        };
        self.finish(Some(worker), calls, reason, None);
        metrics::log_teardown(t.elapsed());
    }

    /// The call failed: the worker is killed and counted as a crash.
    pub fn release_failed(mut self, failure: &NativeFailure) {
        let t = Instant::now();
        let worker = self.worker.take().unwrap();
        let reason = match failure {
            NativeFailure::Timeout => ExitReason::Timeout,
            NativeFailure::OutOfMemory { .. } => ExitReason::Oom,
            NativeFailure::StackOverflow { .. } => ExitReason::Stack,
            NativeFailure::Protocol(_) | NativeFailure::ResultTooLarge { .. } => {
                ExitReason::Protocol
            },
            NativeFailure::Crashed(_) | NativeFailure::StartFailed(_) => ExitReason::Signal,
        };
        self.finish(Some(worker), self.calls + 1, Some(reason), Some(format!("{failure:?}")));
        metrics::log_teardown(t.elapsed());
    }

    /// Kills the worker for a reason that is not a failure (e.g. a caught
    /// panic that the runtime wants isolated).
    pub fn release_recycle(mut self) {
        let worker = self.worker.take().unwrap();
        self.finish(Some(worker), self.calls + 1, Some(ExitReason::Recycle), None);
    }

    fn finish(
        &mut self,
        worker: Option<S::Worker>,
        calls: u32,
        kill: Option<ExitReason>,
        crash: Option<String>,
    ) {
        let pool = self.pool.clone();
        let mut shutdown = false;
        {
            let mut st = pool.inner.state.lock();
            shutdown = shutdown || st.shutdown;
            if let Some(ks) = st.keys.get_mut(&self.key) {
                ks.busy = ks.busy.saturating_sub(1);
                if let Some(c) = crash {
                    ks.crashes += 1;
                    ks.last_crash = Some(c);
                }
            }
            st.publish(Instant::now());
        }
        match (worker, kill) {
            (Some(mut w), Some(reason)) => {
                w.terminate();
                metrics::log_worker_exit(reason);
            },
            (Some(mut w), None) if shutdown => {
                w.terminate();
                metrics::log_worker_exit(ExitReason::Shutdown);
            },
            (Some(w), None) => pool.park(&self.key, w, calls),
            (None, _) => {},
        }
        pool.refill(&self.key);
    }
}

impl<S: Spawner> Drop for Lease<S>
where
    S::Worker: PoolWorker,
{
    fn drop(&mut self) {
        // Dropped without a release: the call was cancelled, so the worker
        // may still be running user code.
        if let Some(w) = self.worker.take() {
            self.finish(Some(w), self.calls + 1, Some(ExitReason::Cancelled), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::atomic::{
            AtomicBool,
            AtomicU64,
            AtomicUsize,
            Ordering,
        },
    };

    use super::*;

    #[derive(Default)]
    struct Ctl {
        spawned: AtomicUsize,
        fail: AtomicBool,
        attempts: AtomicUsize,
        rss: AtomicU64,
        delay_ms: AtomicU64,
        killed: AtomicUsize,
        /// Workers handed out; lets tests mark one dead.
        workers: Mutex<Vec<Arc<WState>>>,
    }

    struct WState {
        alive: AtomicBool,
        terminated: AtomicBool,
        shutdown_requested: AtomicBool,
        /// A worker that ignores the Shutdown frame.
        stubborn: bool,
    }

    struct FakeWorker {
        ctl: Arc<Ctl>,
        st: Arc<WState>,
    }

    impl PoolWorker for FakeWorker {
        fn is_alive(&mut self) -> bool {
            self.st.alive.load(Ordering::SeqCst)
        }
        fn rss_bytes(&mut self) -> Option<u64> {
            Some(self.ctl.rss.load(Ordering::SeqCst))
        }
        fn rss_sampler(&self) -> Option<Box<dyn Fn() -> Option<u64> + Send>> {
            let ctl = self.ctl.clone();
            Some(Box::new(move || Some(ctl.rss.load(Ordering::SeqCst))))
        }
        fn terminate(&mut self) {
            if !self.st.terminated.swap(true, Ordering::SeqCst) {
                self.ctl.killed.fetch_add(1, Ordering::SeqCst);
            }
            self.st.alive.store(false, Ordering::SeqCst);
        }
        fn request_shutdown(&mut self) {
            self.st.shutdown_requested.store(true, Ordering::SeqCst);
            if !self.st.stubborn {
                self.st.alive.store(false, Ordering::SeqCst);
            }
        }
    }

    struct FakeSpawner {
        ctl: Arc<Ctl>,
        stubborn: bool,
    }

    impl Spawner for FakeSpawner {
        type Worker = FakeWorker;
        fn spawn(&self, _a: &Arc<Artifact>, _p: &SandboxPlan) -> anyhow::Result<FakeWorker> {
            self.ctl.attempts.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(self.ctl.delay_ms.load(Ordering::SeqCst)));
            if self.ctl.fail.load(Ordering::SeqCst) {
                anyhow::bail!("exec failed");
            }
            self.ctl.spawned.fetch_add(1, Ordering::SeqCst);
            let st = Arc::new(WState {
                alive: AtomicBool::new(true),
                terminated: AtomicBool::new(false),
                shutdown_requested: AtomicBool::new(false),
                stubborn: self.stubborn,
            });
            self.ctl.workers.lock().push(st.clone());
            Ok(FakeWorker {
                ctl: self.ctl.clone(),
                st,
            })
        }
    }

    fn art(sha: &str) -> Arc<Artifact> {
        Arc::new(Artifact {
            sha256: sha.into(),
            path: PathBuf::from("/nonexistent"),
            size: 1,
            functions: Default::default(),
        })
    }

    fn cfg() -> PoolConfig {
        PoolConfig {
            min_idle: 0,
            max_idle: 4,
            idle_timeout: Duration::from_secs(300),
            max_calls_per_worker: 1000,
            recycle_rss_bytes: 256 << 20,
            reaper_interval: Duration::from_secs(3600),
            breaker_threshold: 3,
            breaker_window: Duration::from_secs(60),
            breaker_backoff_base: Duration::from_millis(100),
            breaker_backoff_cap: Duration::from_millis(400),
            shutdown_grace: Duration::from_millis(100),
            spawn_threads: 4,
            probe_every: 1,
            probe_slow_call: Duration::from_millis(20),
            idle_recycle_rss_bytes: 0,
        }
    }

    fn pool(cfg: PoolConfig) -> (Pool<FakeSpawner>, Arc<Ctl>) {
        let ctl = Arc::new(Ctl::default());
        (
            Pool::new(
                FakeSpawner {
                    ctl: ctl.clone(),
                    stubborn: false,
                },
                cfg,
            ),
            ctl,
        )
    }

    async fn wait_for(mut f: impl FnMut() -> bool) {
        for _ in 0..500 {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("condition not reached");
    }

    fn plan() -> SandboxPlan {
        SandboxPlan::default()
    }

    #[tokio::test]
    async fn spawns_run_in_parallel_up_to_spawn_threads() {
        let (p, ctl) = pool(cfg());
        ctl.delay_ms.store(150, Ordering::SeqCst);
        let a = art("a");
        let t = Instant::now();
        let pl = plan();
        let acquires: Vec<_> = (0..4).map(|_| p.acquire(&a, 1, &pl)).collect();
        let leases = futures::future::join_all(acquires).await;
        assert!(leases.iter().all(|l| l.is_ok()));
        // Serial spawning would take 600 ms.
        assert!(t.elapsed() < Duration::from_millis(400), "{:?}", t.elapsed());
        assert_eq!(ctl.spawned.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn prewarm_parks_a_worker_without_blocking() {
        let (p, ctl) = pool(cfg());
        ctl.delay_ms.store(100, Ordering::SeqCst);
        let a = art("a");
        let t = Instant::now();
        p.prewarm(&a, 1, &plan());
        p.prewarm(&a, 1, &plan());
        assert!(t.elapsed() < Duration::from_millis(50));
        wait_for(|| p.stats().iter().map(|s| s.idle).sum::<usize>() == 1).await;
        // The second prewarm saw the first one in flight.
        assert_eq!(ctl.spawned.load(Ordering::SeqCst), 1);
        let l = p.acquire(&a, 1, &plan()).await.unwrap();
        assert_eq!(ctl.spawned.load(Ordering::SeqCst), 1);
        l.release_clean();
    }

    #[tokio::test]
    async fn probes_are_sampled_but_always_catch_slow_and_first_calls() {
        let mut c = cfg();
        c.probe_every = 4;
        c.recycle_rss_bytes = 100;
        let (p, ctl) = pool(c);
        let a = art("a");
        // First call: probed, RSS over the limit.
        ctl.rss.store(1000, Ordering::SeqCst);
        let l = p.acquire(&a, 1, &plan()).await.unwrap();
        l.release_clean_after(Duration::ZERO);
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 1);
        // Calls 1..3 of a fresh worker: call 1 probed (ok), 2 and 3 skipped.
        ctl.rss.store(0, Ordering::SeqCst);
        let l = p.acquire(&a, 1, &plan()).await.unwrap();
        l.release_clean_after(Duration::ZERO);
        ctl.rss.store(1000, Ordering::SeqCst);
        for _ in 0..2 {
            let l = p.acquire(&a, 1, &plan()).await.unwrap();
            l.release_clean_after(Duration::ZERO);
        }
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 1);
        // Call 4 is sampled and recycles.
        let l = p.acquire(&a, 1, &plan()).await.unwrap();
        l.release_clean_after(Duration::ZERO);
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 2);
        // A slow call is probed even off-sample.
        ctl.rss.store(0, Ordering::SeqCst);
        let l = p.acquire(&a, 1, &plan()).await.unwrap();
        l.release_clean_after(Duration::ZERO);
        ctl.rss.store(1000, Ordering::SeqCst);
        let l = p.acquire(&a, 1, &plan()).await.unwrap();
        l.release_clean_after(Duration::from_millis(50));
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn idle_workers_are_reused_one_call_each() {
        let (p, ctl) = pool(cfg());
        let a = art("a");
        let l1 = p.acquire(&a, 1, &plan()).await.unwrap();
        // One call per worker: a second concurrent acquire spawns another.
        let l2 = p.acquire(&a, 1, &plan()).await.unwrap();
        assert_eq!(ctl.spawned.load(Ordering::SeqCst), 2);
        l1.release_clean();
        l2.release_clean();
        let l3 = p.acquire(&a, 1, &plan()).await.unwrap();
        assert_eq!(ctl.spawned.load(Ordering::SeqCst), 2);
        assert_eq!(l3.calls_before(), 1);
        l3.release_clean();
        // A different env hash is a different pool.
        let l4 = p.acquire(&a, 2, &plan()).await.unwrap();
        assert_eq!(ctl.spawned.load(Ordering::SeqCst), 3);
        l4.release_clean();
        let stats = p.stats();
        assert_eq!(stats.iter().map(|s| s.idle).sum::<usize>(), 3);
    }

    #[tokio::test]
    async fn recycles_after_max_calls() {
        let mut c = cfg();
        c.max_calls_per_worker = 2;
        let (p, ctl) = pool(c);
        let a = art("a");
        for _ in 0..2 {
            p.acquire(&a, 1, &plan()).await.unwrap().release_clean();
        }
        // Second call hit the threshold: the worker was killed.
        assert_eq!(ctl.spawned.load(Ordering::SeqCst), 1);
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 1);
        let l = p.acquire(&a, 1, &plan()).await.unwrap();
        assert_eq!(ctl.spawned.load(Ordering::SeqCst), 2);
        assert_eq!(l.calls_before(), 0);
        l.release_clean();
    }

    #[tokio::test]
    async fn recycles_on_rss_and_on_failure() {
        let (p, ctl) = pool(cfg());
        let a = art("a");
        ctl.rss.store(300 << 20, Ordering::SeqCst);
        p.acquire(&a, 1, &plan()).await.unwrap().release_clean();
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 1);
        ctl.rss.store(1 << 20, Ordering::SeqCst);
        p.acquire(&a, 1, &plan())
            .await
            .unwrap()
            .release_failed(&NativeFailure::Timeout);
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 2);
        let s = &p.stats()[0];
        assert_eq!((s.idle, s.busy, s.crashes), (0, 0, 1));
        assert!(s.last_crash.as_deref().unwrap().contains("Timeout"));
        // A crash in one pool does not open the breaker.
        p.acquire(&a, 1, &plan()).await.unwrap().release_clean();
        assert_eq!(p.stats()[0].idle, 1);
    }

    #[tokio::test]
    async fn reaper_recycles_bloated_idle_worker_only_with_a_spare() {
        let mut c = cfg();
        c.idle_recycle_rss_bytes = 100;
        let (p, ctl) = pool(c);
        let a = art("a");
        // One idle worker, bloated: not killed, but a spare is spawned.
        let l1 = p.acquire(&a, 1, &plan()).await.unwrap();
        ctl.rss.store(1000, Ordering::SeqCst);
        l1.release_clean_after(Duration::ZERO);
        p.reap(Instant::now());
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 0);
        for _ in 0..50 {
            if p.stats()[0].idle == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(p.stats()[0].idle, 2);
        // Next tick: the spare exists, one is recycled, one stays.
        p.reap(Instant::now());
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 1);
        assert!(p.stats()[0].idle >= 1);
    }

    #[tokio::test]
    async fn dropped_lease_kills_the_worker() {
        let (p, ctl) = pool(cfg());
        let a = art("a");
        let l = p.acquire(&a, 1, &plan()).await.unwrap();
        drop(l);
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 1);
        assert_eq!(p.stats()[0].busy, 0);
    }

    #[tokio::test]
    async fn dead_idle_worker_is_discarded() {
        let (p, ctl) = pool(cfg());
        let a = art("a");
        p.acquire(&a, 1, &plan()).await.unwrap().release_clean();
        ctl.workers.lock()[0].alive.store(false, Ordering::SeqCst);
        let l = p.acquire(&a, 1, &plan()).await.unwrap();
        assert_eq!(ctl.spawned.load(Ordering::SeqCst), 2);
        assert_eq!(l.calls_before(), 0);
        l.release_clean();
    }

    #[tokio::test]
    async fn refills_min_idle_in_the_background() {
        let mut c = cfg();
        c.min_idle = 2;
        let (p, ctl) = pool(c);
        let a = art("a");
        let l = p.acquire(&a, 1, &plan()).await.unwrap();
        assert_eq!(ctl.spawned.load(Ordering::SeqCst), 1);
        // The release parks the used worker and tops up to two idle.
        l.release_clean();
        wait_for(|| ctl.spawned.load(Ordering::SeqCst) == 2).await;
        wait_for(|| p.stats()[0].idle == 2).await;
        // Taking one idle worker triggers the next refill on release.
        let l = p.acquire(&a, 1, &plan()).await.unwrap();
        assert_eq!(p.stats()[0].idle, 1);
        l.release_failed(&NativeFailure::Timeout);
        wait_for(|| p.stats()[0].idle == 2).await;
        assert_eq!(ctl.spawned.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn max_idle_caps_total_idle_workers() {
        let mut c = cfg();
        c.max_idle = 2;
        let (p, ctl) = pool(c);
        let a = art("a");
        let mut leases = Vec::new();
        for _ in 0..4 {
            leases.push(p.acquire(&a, 1, &plan()).await.unwrap());
        }
        for l in leases {
            l.release_clean();
        }
        assert_eq!(p.stats()[0].idle, 2);
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn reaper_kills_idle_workers_and_drops_empty_pools() {
        let mut c = cfg();
        c.idle_timeout = Duration::from_secs(10);
        let (p, ctl) = pool(c);
        let a = art("a");
        p.acquire(&a, 1, &plan()).await.unwrap().release_clean();
        p.reap(Instant::now() + Duration::from_secs(5));
        assert_eq!(p.stats()[0].idle, 1);
        p.reap(Instant::now() + Duration::from_secs(11));
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 1);
        assert!(p.stats().is_empty());
    }

    #[tokio::test]
    async fn breaker_opens_after_repeated_spawn_failures_and_backs_off() {
        let (p, ctl) = pool(cfg());
        let a = art("a");
        ctl.fail.store(true, Ordering::SeqCst);
        for _ in 0..3 {
            let e = p.acquire(&a, 1, &plan()).await.err().unwrap();
            assert!(matches!(e, NativeFailure::StartFailed(m) if m.contains("exec failed")));
        }
        assert_eq!(ctl.attempts.load(Ordering::SeqCst), 3);
        // Open: fails fast with the last spawn error, without spawning.
        let e = p.acquire(&a, 1, &plan()).await.err().unwrap();
        assert!(matches!(e, NativeFailure::StartFailed(m) if m.contains("exec failed")));
        assert_eq!(ctl.attempts.load(Ordering::SeqCst), 3);
        assert!(p.stats()[0].breaker_open);
        // Another artifact is unaffected.
        ctl.fail.store(false, Ordering::SeqCst);
        p.acquire(&art("b"), 1, &plan()).await.unwrap().release_clean();
        // After the first backoff (100 ms) one attempt is allowed; failing
        // again re-opens with a doubled backoff (200 ms).
        ctl.fail.store(true, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(p.acquire(&a, 1, &plan()).await.is_err());
        assert_eq!(ctl.attempts.load(Ordering::SeqCst), 5);
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(p.acquire(&a, 1, &plan()).await.is_err());
        assert_eq!(ctl.attempts.load(Ordering::SeqCst), 5, "still backing off");
        // One success resets the breaker.
        ctl.fail.store(false, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(120)).await;
        p.acquire(&a, 1, &plan()).await.unwrap().release_clean();
        assert!(!p.stats().iter().any(|s| s.breaker_open));
        ctl.fail.store(true, Ordering::SeqCst);
        // Not tripped any more: needs three failures again.
        let _ = p.acquire(&art("a"), 1, &plan()).await; // reuses idle
    }

    #[test]
    fn breaker_state_machine() {
        let c = cfg();
        let mut b = Breaker::new(&c);
        let t0 = Instant::now();
        b.failure(t0, "e1".into(), &c);
        b.failure(t0, "e2".into(), &c);
        assert!(b.check(t0).is_none());
        b.failure(t0, "e3".into(), &c);
        assert_eq!(b.check(t0), Some("e3"));
        assert!(b.check(t0 + Duration::from_millis(101)).is_none());
        // Half open: one more failure reopens with 2x, then capped at 400 ms.
        let t1 = t0 + Duration::from_millis(101);
        b.failure(t1, "e4".into(), &c);
        assert!(b.check(t1 + Duration::from_millis(199)).is_some());
        assert!(b.check(t1 + Duration::from_millis(201)).is_none());
        let t2 = t1 + Duration::from_millis(201);
        b.failure(t2, "e5".into(), &c);
        let t3 = t2 + Duration::from_millis(401);
        b.failure(t3, "e6".into(), &c);
        assert!(b.check(t3 + Duration::from_millis(399)).is_some());
        assert!(b.check(t3 + Duration::from_millis(401)).is_none());
        b.success(&c);
        assert!(!b.has_history());
        // Failures outside the window do not accumulate.
        let mut b = Breaker::new(&c);
        b.failure(t0, "a".into(), &c);
        b.failure(t0 + Duration::from_secs(30), "b".into(), &c);
        b.failure(t0 + Duration::from_secs(61), "c".into(), &c);
        assert!(b.check(t0 + Duration::from_secs(61)).is_none());
    }

    #[tokio::test]
    async fn shutdown_asks_idle_workers_then_kills() {
        let ctl = Arc::new(Ctl::default());
        let p = Pool::new(
            FakeSpawner {
                ctl: ctl.clone(),
                stubborn: true,
            },
            cfg(),
        );
        let a = art("a");
        let l1 = p.acquire(&a, 1, &plan()).await.unwrap();
        let l2 = p.acquire(&a, 1, &plan()).await.unwrap();
        l1.release_clean();
        let t = Instant::now();
        p.shutdown().await;
        // The stubborn idle worker ignored Shutdown and was killed after the grace.
        assert!(t.elapsed() >= Duration::from_millis(100));
        let ws = ctl.workers.lock().clone();
        assert!(ws[0].shutdown_requested.load(Ordering::SeqCst));
        assert!(ws[0].terminated.load(Ordering::SeqCst));
        // Busy lease is killed when released; new acquires fail.
        l2.release_clean();
        assert!(ws[1].terminated.load(Ordering::SeqCst));
        assert!(matches!(
            p.acquire(&a, 1, &plan()).await.err().unwrap(),
            NativeFailure::StartFailed(_)
        ));
    }

    #[tokio::test]
    async fn cooperative_shutdown_does_not_wait_for_the_grace_period() {
        let (p, ctl) = pool(cfg());
        let a = art("a");
        p.acquire(&a, 1, &plan()).await.unwrap().release_clean();
        let t = Instant::now();
        p.shutdown().await;
        assert!(t.elapsed() < Duration::from_millis(90));
        assert_eq!(ctl.killed.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancelled_acquire_parks_the_spawned_worker() {
        let (p, ctl) = pool(cfg());
        ctl.delay_ms.store(100, Ordering::SeqCst);
        let a = art("a");
        // Drop the acquire future before it can observe the spawn result.
        let sp = plan();
        let fut = p.acquire(&a, 1, &sp);
        let _ = tokio::time::timeout(Duration::from_millis(10), fut).await;
        wait_for(|| p.stats().first().is_some_and(|s| s.idle == 1)).await;
        assert_eq!(ctl.spawned.load(Ordering::SeqCst), 1);
        assert_eq!(p.stats()[0].busy, 0);
    }
}
