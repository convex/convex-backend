//! Runtime trait for abstracting away OS-esque features and allow different
//! implementations for test, dev, prod, etc.

use std::{
    collections::HashMap,
    future::{
        self,
        Future,
    },
    hash::Hash,
    num::TryFromIntError,
    ops::{
        Add,
        Sub,
    },
    pin::Pin,
    sync::LazyLock,
    task::Poll,
    time::{
        Duration,
        SystemTime,
        UNIX_EPOCH,
    },
};

use anyhow::Context;
use async_trait::async_trait;
use fastrace::{
    future::FutureExt as _,
    Span,
};
use futures::{
    future::FusedFuture,
    select_biased,
    stream,
    FutureExt,
    Stream,
    StreamExt,
    TryStreamExt,
};
pub use governor::nanos::Nanos;
use governor::{
    middleware::NoOpMiddleware,
    state::{
        keyed::DefaultKeyedStateStore,
        InMemoryState,
        NotKeyed,
    },
    Quota,
};
use metrics::CONVEX_METRICS_REGISTRY;
use parking_lot::Mutex;
use pin_project::pin_project;
use rand::RngCore;
use sentry::SentryFutureExt;
use serde::Serialize;
use thiserror::Error;
use tokio::runtime::{
    Handle,
    RuntimeFlavor,
};
use tokio_metrics::Instrumented;
use tokio_metrics_collector::TaskMonitor;
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;
use value::heap_size::HeapSize;

pub use self::{
    join_map::JoinMap,
    join_set::JoinSet,
};
use crate::{
    errors::recapture_stacktrace,
    is_canceled::IsCanceled,
    pause::PauseClient,
    types::Timestamp,
};

mod join_map;
mod join_set;

#[derive(Error, Debug)]
pub enum JoinError {
    #[error("Future canceled")]
    Canceled,
    #[error("Future panicked: {0:?}")]
    Panicked(anyhow::Error),
}

impl From<tokio::task::JoinError> for JoinError {
    fn from(e: tokio::task::JoinError) -> Self {
        if e.is_canceled() {
            JoinError::Canceled
        } else {
            JoinError::Panicked(anyhow::anyhow!(e
                .into_panic()
                .downcast::<&str>()
                .expect("panic message must be a string")))
        }
    }
}

#[must_use = "Tasks are canceled when their `SpawnHandle` is dropped."]
pub trait SpawnHandle: Send + Sync {
    /// Stops the spawned task "soon". This happens asynchronously.
    fn shutdown(&mut self);
    /// Wait for the spawned task to finish. Don't use this function directly,
    /// call `.join()` instead.
    fn poll_join(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), JoinError>>;
    /// Allows the spawned task to keep running indefinitely. By default, a task
    /// is shut down on drop.
    fn detach(self: Box<Self>);
    /// Wait for the spawned task to finish. Returns an error if the task was
    /// canceled (using `.shutdown()`) or panicked.
    fn join<'a>(mut self) -> impl Future<Output = Result<(), JoinError>> + 'a
    where
        Self: Sized + 'a,
    {
        future::poll_fn(move |cx| self.poll_join(cx))
    }
}

impl<T: SpawnHandle + ?Sized> SpawnHandle for Box<T> {
    fn shutdown(&mut self) {
        (**self).shutdown()
    }

    fn poll_join(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), JoinError>> {
        (**self).poll_join(cx)
    }

    fn detach(self: Box<Self>) {
        (*self).detach()
    }
}

impl dyn SpawnHandle {
    // This inherent impl just forwards to the default trait impl, but means
    // that callers don't need to import the trait to call it
    pub fn join(self: Box<Self>) -> impl Future<Output = Result<(), JoinError>> {
        SpawnHandle::join(self)
    }

    /// Wait for the spawn task to finish, but if the returned future is
    /// canceled, the spawned task continues running as though it were
    /// `detach()`ed.
    pub fn join_or_detach(self: Box<Self>) -> impl Future<Output = Result<(), JoinError>> {
        struct DetachOnDrop(Option<Box<dyn SpawnHandle>>);
        impl Drop for DetachOnDrop {
            fn drop(&mut self) {
                self.0.take().expect("lost spawn handle?").detach();
            }
        }
        let mut handle = DetachOnDrop(Some(self));
        future::poll_fn(move |cx| handle.0.as_mut().expect("lost spawn handle?").poll_join(cx))
    }

    pub fn shutdown_and_join(self: Box<Self>) -> impl Future<Output = anyhow::Result<()>> {
        shutdown_and_join(self)
    }
}

pub struct TokioSpawnHandle {
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl From<tokio::task::JoinHandle<()>> for TokioSpawnHandle {
    fn from(handle: tokio::task::JoinHandle<()>) -> Self {
        Self {
            handle: Some(handle),
        }
    }
}

impl SpawnHandle for TokioSpawnHandle {
    fn shutdown(&mut self) {
        self.handle.as_ref().expect("shutdown after detach").abort();
    }

    fn poll_join(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), JoinError>> {
        std::task::ready!(
            Pin::new(&mut self.handle.as_mut().expect("poll after detach")).poll(cx)
        )?;
        Poll::Ready(Ok(()))
    }

    fn detach(mut self: Box<Self>) {
        self.handle.take();
    }
}

impl Drop for TokioSpawnHandle {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

/// Shutdown the associated future, preempting it at its next yield point, and
/// join on its result.
pub async fn shutdown_and_join(mut handle: Box<dyn SpawnHandle>) -> anyhow::Result<()> {
    handle.shutdown();
    if let Err(e) = handle.join().await
        && !matches!(e, JoinError::Canceled)
    {
        return Err(e.into());
    }
    Ok(())
}

// Why 20? ¯\_(ツ)_/¯. We use this value a lot elsewhere and it doesn't seem
// unreasonable as a starting point for lightweight things.
const JOIN_BUFFER_SIZE: usize = 20;

pub async fn try_join_buffered<
    RT: Runtime,
    T: Send + 'static,
    C: Default + Send + 'static + Extend<T>,
>(
    name: &'static str,
    tasks: impl Iterator<Item = impl Future<Output = anyhow::Result<T>> + Send + 'static>
        + Send
        + 'static,
) -> anyhow::Result<C> {
    assert_send(
        stream::iter(tasks.map(|task| assert_send(try_join(name, assert_send(task)))))
            .buffered(JOIN_BUFFER_SIZE)
            .try_collect(),
    )
    .await
}

// Work around "higher-ranked lifetime errors" due to the borrow checker's
// inability (bug) to determine that some futures are in fact send.  See
// https://github.com/rust-lang/rust/issues/102211#issuecomment-1367900125
pub fn assert_send<'a, T>(
    fut: impl 'a + Send + Future<Output = T>,
) -> impl 'a + Send + Future<Output = T> {
    fut
}

pub async fn try_join_buffer_unordered<
    T: Send + 'static,
    C: Default + Send + 'static + Extend<T>,
>(
    name: &'static str,
    tasks: impl Iterator<Item = impl Future<Output = anyhow::Result<T>> + Send + 'static> + Send,
) -> anyhow::Result<C> {
    assert_send(
        stream::iter(tasks.map(|task| try_join(name, task)))
            .buffer_unordered(JOIN_BUFFER_SIZE)
            .try_collect(),
    )
    .await
}

pub async fn try_join<T: Send + 'static>(
    name: &'static str,
    fut: impl Future<Output = anyhow::Result<T>> + Send + 'static,
) -> anyhow::Result<T> {
    let handle = AbortOnDropHandle::new(tokio_spawn(
        name,
        fut.in_span(Span::enter_with_local_parent(name)),
    ));
    match handle.await? {
        Ok(result) => Ok(result),
        Err(e) => Err(recapture_stacktrace(e).await),
    }
}

/// A Runtime can be considered somewhat like an operating system abstraction
/// for our codebase. Functionality like time, randomness, network access, etc
/// should operate quite differently between test, dev and prod, e.g., we don't
/// want `wait` to actually call `thread::sleep_ms()` in test but instead just
/// to advance local time. This trait should include all functionality that we
/// want to abstract out for different runtime environments so application
/// code can be parameterized by a given runtime implementation.
#[async_trait]
pub trait Runtime: Clone + Sync + Send + 'static {
    /// Sleep for the given duration.
    #[must_use]
    fn wait(&self, duration: Duration) -> Pin<Box<dyn FusedFuture<Output = ()> + Send + 'static>>;

    /// Spawn a future on the runtime's executor.
    ///
    /// The spawned task will be canceled if the returned `SpawnHandle` is
    /// dropped, unless `detach()` is called on it.
    fn spawn(
        &self,
        name: &'static str,
        f: impl Future<Output = ()> + Send + 'static,
    ) -> Box<dyn SpawnHandle>;

    /// Shorthand for `spawn().detach()`
    ///
    /// This should only be used for tasks that are best-effort (e.g. cleaning
    /// up partial progress) or that are truly process-global.
    fn spawn_background(&self, name: &'static str, f: impl Future<Output = ()> + Send + 'static) {
        self.spawn(name, f).detach()
    }

    /// Spawn a future on a reserved OS thread. This is only really necessary
    /// for libraries like `V8` that care about being called from a
    /// particular thread.
    ///
    /// The spawned task will be canceled if the returned `SpawnHandle` is
    /// dropped, unless `detach()` is called on it.
    fn spawn_thread<Fut: Future<Output = ()>, F: FnOnce() -> Fut + Send + 'static>(
        &self,
        name: &str,
        f: F,
    ) -> Box<dyn SpawnHandle>;

    /// Return (a potentially-virtualized) system time. Compare with
    /// `std::time::UNIX_EPOCH` to obtain a Unix timestamp.
    fn system_time(&self) -> SystemTime;

    fn unix_timestamp(&self) -> UnixTimestamp {
        UnixTimestamp(
            self.system_time()
                .duration_since(UNIX_EPOCH)
                .expect("Failed to compute unix timestamp"),
        )
    }

    /// Return (a potentially-virtualized) reading from a monotonic clock.
    fn monotonic_now(&self) -> tokio::time::Instant;

    /// Use the runtime's source of randomness.
    fn rng(&self) -> Box<dyn RngCore>;

    fn new_uuid_v4(&self) -> Uuid {
        let mut rng = self.rng();
        let mut bytes = [0u8; 16];
        rng.fill_bytes(&mut bytes);
        uuid::Builder::from_random_bytes(bytes).into_uuid()
    }

    fn generate_timestamp(&self) -> anyhow::Result<Timestamp> {
        Timestamp::try_from(self.system_time())
    }

    fn pause_client(&self) -> PauseClient;
}

/// Abstraction over a unix timestamp. Internally it stores a Duration since the
/// unix epoch.
///
/// NOTE: Only works for timestamps past the UNIX_EPOCH. Not suitable for user
/// defined input from javascript (where f64 can support timestamps prior to the
/// epoch).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct UnixTimestamp(Duration);

impl UnixTimestamp {
    pub fn from_secs_f64(secs: f64) -> anyhow::Result<Self> {
        Ok(UnixTimestamp(Duration::try_from_secs_f64(secs)?))
    }

    pub fn from_nanos(nanos: u64) -> Self {
        UnixTimestamp(Duration::from_nanos(nanos))
    }

    pub fn from_millis(ms: u64) -> Self {
        UnixTimestamp(Duration::from_millis(ms))
    }

    pub fn as_nanos(&self) -> u128 {
        self.0.as_nanos()
    }

    pub fn as_secs_f64(&self) -> f64 {
        self.0.as_secs_f64()
    }

    pub fn as_secs(&self) -> u64 {
        self.0.as_secs()
    }

    pub fn as_system_time(&self) -> SystemTime {
        UNIX_EPOCH + self.0
    }

    /// Returns `None` if `time` predates the Unix epoch
    pub fn from_system_time(time: SystemTime) -> Option<Self> {
        time.duration_since(SystemTime::UNIX_EPOCH).ok().map(Self)
    }

    pub fn checked_sub(&self, rhs: UnixTimestamp) -> Option<Duration> {
        self.0.checked_sub(rhs.0)
    }

    pub fn as_ms_since_epoch(&self) -> Result<u64, anyhow::Error> {
        self.0
            .as_millis()
            .try_into()
            .map_err(|e: TryFromIntError| anyhow::anyhow!(e))
    }
}

impl HeapSize for UnixTimestamp {
    fn heap_size(&self) -> usize {
        0
    }
}

impl Sub<UnixTimestamp> for UnixTimestamp {
    type Output = Duration;

    fn sub(self, rhs: UnixTimestamp) -> Duration {
        self.0 - rhs.0
    }
}

impl Add<Duration> for UnixTimestamp {
    type Output = UnixTimestamp;

    fn add(self, rhs: Duration) -> UnixTimestamp {
        UnixTimestamp(self.0 + rhs)
    }
}

impl Sub<Duration> for UnixTimestamp {
    type Output = UnixTimestamp;

    fn sub(self, rhs: Duration) -> UnixTimestamp {
        UnixTimestamp(self.0 - rhs)
    }
}

impl From<UnixTimestamp> for prost_types::Timestamp {
    fn from(ts: UnixTimestamp) -> Self {
        Self {
            seconds: ts.as_secs() as i64,
            nanos: ts.0.subsec_nanos() as i32,
        }
    }
}

impl TryFrom<prost_types::Timestamp> for UnixTimestamp {
    type Error = anyhow::Error;

    fn try_from(ts: prost_types::Timestamp) -> anyhow::Result<Self> {
        let system_time = SystemTime::try_from(ts)?;
        Ok(Self(
            system_time
                .duration_since(UNIX_EPOCH)
                .context("Failed to compute duration from epoch")?,
        ))
    }
}

#[derive(Clone)]
pub struct RuntimeClock<RT: Runtime> {
    runtime: RT,
}

pub type RateLimiter<RT> = governor::RateLimiter<
    NotKeyed,
    InMemoryState,
    RuntimeClock<RT>,
    NoOpMiddleware<<RuntimeClock<RT> as governor::clock::Clock>::Instant>,
>;

pub type KeyedRateLimiter<K, RT> = governor::RateLimiter<
    K,
    DefaultKeyedStateStore<K>,
    RuntimeClock<RT>,
    NoOpMiddleware<<RuntimeClock<RT> as governor::clock::Clock>::Instant>,
>;

pub fn new_rate_limiter<RT: Runtime>(runtime: RT, quota: Quota) -> RateLimiter<RT> {
    RateLimiter::direct_with_clock(quota, RuntimeClock { runtime })
}

/// Creates a rate limiter that is *nearly* unlimited, useful for testing.
pub fn new_unlimited_rate_limiter<RT: Runtime>(runtime: RT) -> RateLimiter<RT> {
    new_rate_limiter(
        runtime,
        Quota::with_period(Duration::from_nanos(1)).unwrap(),
    )
}

pub fn new_keyed_rate_limiter<RT: Runtime, K: Hash + Eq + Clone>(
    runtime: RT,
    quota: Quota,
) -> KeyedRateLimiter<K, RT> {
    KeyedRateLimiter::dashmap_with_clock(quota, RuntimeClock { runtime })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct GovernorInstant(tokio::time::Instant);

impl From<tokio::time::Instant> for GovernorInstant {
    fn from(instant: tokio::time::Instant) -> Self {
        Self(instant)
    }
}

impl<RT: Runtime> governor::clock::Clock for RuntimeClock<RT> {
    type Instant = GovernorInstant;

    fn now(&self) -> Self::Instant {
        GovernorInstant(self.runtime.monotonic_now())
    }
}

impl governor::clock::Reference for GovernorInstant {
    fn duration_since(&self, earlier: Self) -> Nanos {
        if earlier.0 < self.0 {
            (self.0 - earlier.0).into()
        } else {
            Nanos::from(Duration::ZERO)
        }
    }

    fn saturating_sub(&self, duration: Nanos) -> Self {
        self.0
            .checked_sub(duration.into())
            .map(GovernorInstant)
            .unwrap_or(*self)
    }
}

impl Add<Nanos> for GovernorInstant {
    type Output = GovernorInstant;

    fn add(self, rhs: Nanos) -> Self::Output {
        GovernorInstant(self.0 + rhs.into())
    }
}

impl<RT: Runtime> governor::clock::ReasonablyRealtime for RuntimeClock<RT> {}

#[async_trait]
pub trait WithTimeout {
    async fn with_timeout<T>(
        &self,
        description: &'static str,
        duration: Duration,
        fut: impl Future<Output = anyhow::Result<T>> + Send,
    ) -> anyhow::Result<T>;
}

#[async_trait]
impl<RT: Runtime> WithTimeout for RT {
    async fn with_timeout<T>(
        &self,
        description: &'static str,
        duration: Duration,
        fut: impl Future<Output = anyhow::Result<T>> + Send,
    ) -> anyhow::Result<T> {
        select_biased! {
            result = fut.fuse() => result,
            _q = self.wait(duration) => {
                anyhow::bail!(TimeoutError{description, duration});
            },
        }
    }
}

#[derive(thiserror::Error, Debug)]
#[error("'{description}' timeout after {duration:?}")]
pub struct TimeoutError {
    description: &'static str,
    duration: Duration,
}

/// Binds the current tracing & sentry contexts to the provided future.
pub fn propagate_tracing<F: Future>(
    f: F,
) -> tracing::instrument::Instrumented<sentry::SentryFuture<F>> {
    let sentry_hub = sentry::Hub::current();
    let tracing_span = tracing::Span::current();
    tracing::Instrument::instrument(SentryFutureExt::bind_hub(f, sentry_hub), tracing_span)
}

/// Binds the current tracing & sentry contexts to the provided synchronous
/// function.
pub fn propagate_tracing_blocking<'a, R, F: FnOnce() -> R + 'a>(f: F) -> impl FnOnce() -> R + 'a {
    let sentry_hub = sentry::Hub::current();
    let tracing_span = tracing::Span::current();
    move || {
        let _entered = tracing_span.entered();
        sentry::Hub::run(sentry_hub, f)
    }
}

/// Wraps `tokio::spawn` with our tokio metrics integration and propagates
/// tracing context.
pub fn tokio_spawn<F>(name: &'static str, f: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let monitor = GLOBAL_TASK_MANAGER.lock().get(name);
    tokio::spawn(propagate_tracing(monitor.instrument(f)))
}

/// Wraps `tokio::task::spawn_blocking` and propagates tracing context.
pub fn tokio_spawn_blocking<F, R>(_name: &'static str, f: F) -> tokio::task::JoinHandle<F::Output>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    // TODO: do something with `_name`
    tokio::task::spawn_blocking(propagate_tracing_blocking(f))
}

pub static GLOBAL_TASK_MANAGER: LazyLock<Mutex<TaskManager>> = LazyLock::new(|| {
    let task_collector = tokio_metrics_collector::default_task_collector();
    CONVEX_METRICS_REGISTRY
        .register(Box::new(task_collector))
        .unwrap();

    let manager = TaskManager {
        monitors: HashMap::new(),
    };
    Mutex::new(manager)
});

pub struct TaskManager {
    monitors: HashMap<&'static str, TaskMonitor>,
}

impl TaskManager {
    pub fn get(&mut self, name: &'static str) -> TaskMonitor {
        if let Some(monitor) = self.monitors.get(name) {
            return monitor.clone();
        }
        let monitor = TaskMonitor::new();
        self.monitors.insert(name, monitor.clone());
        tokio_metrics_collector::default_task_collector()
            .add(name, monitor.clone())
            .expect("Duplicate task label?");
        monitor
    }

    pub fn instrument<F: Future>(name: &'static str, f: F) -> Instrumented<F> {
        let monitor = {
            let mut manager = GLOBAL_TASK_MANAGER.lock();
            manager.get(name)
        };
        monitor.instrument(f)
    }
}

// Helper function to only call into `tokio::task::block_in_place` if we're not
// using the single threaded runtime.
pub fn block_in_place<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let handle = Handle::current();
    if handle.runtime_flavor() == RuntimeFlavor::CurrentThread {
        f()
    } else {
        tokio::task::block_in_place(f)
    }
}

#[pin_project]
pub struct CoopStream<S> {
    #[pin]
    inner: S,
    /// If false, we need to consume budget before proceeding
    proceed: bool,
}

pub trait CoopStreamExt: Sized {
    /// Wraps `self` in a stream that participates in Tokio's cooperative
    /// scheduling system.
    /// If the current task is out of budget, the stream may yield
    /// `Poll::Pending` even if the underlying stream still has elements to
    /// yield.
    fn cooperative(self) -> CoopStream<Self>;
}

impl<S: Stream> CoopStreamExt for S {
    fn cooperative(self) -> CoopStream<Self> {
        CoopStream {
            inner: self,
            proceed: false,
        }
    }
}

impl<S: Stream> Stream for CoopStream<S> {
    type Item = S::Item;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.project();
        if !*this.proceed {
            std::task::ready!(tokio::task::coop::poll_proceed(cx)).made_progress();
            *this.proceed = true;
        }
        let r = this.inner.poll_next(cx);
        if r.is_ready() {
            *this.proceed = false;
        }
        r
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
