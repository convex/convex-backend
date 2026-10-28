use std::{
    collections::HashMap,
    ops::{
        Deref,
        DerefMut,
    },
    sync::Arc,
    time::Duration,
};

use anyhow::Context as _;
use async_broadcast::broadcast;
use common::{
    runtime::{
        Runtime,
        SpawnHandle,
    },
    types::UdfType,
};
use errors::ErrorMetadata;
use futures::{
    future::{
        self,
        Either,
    },
    pin_mut,
    Future,
    FutureExt,
};
use parking_lot::Mutex;
use tokio::select;

use crate::{
    concurrency_limiter::SuspendedPermit,
    environment::JsEnvironment,
    metrics,
    termination::{
        ContextId,
        ExecutionHandle,
        IsolateTerminationReason,
        TerminationReason,
    },
    ConcurrencyPermit,
};

pub const SYSTEM_TIMEOUT_ERROR_MESSAGE: &str =
    "Your request timed out performing too many system operations.";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PauseReason {
    DatabaseSyscall { name: String },
    ConcurrencyPermitReacquire,
    UdfInitialize,
    LoadModule,
}

impl PauseReason {
    pub fn as_str(&self) -> String {
        match self {
            Self::DatabaseSyscall { name } => format!("database_syscall({name})"),
            Self::ConcurrencyPermitReacquire => "concurrency_permit_reacquire".to_string(),
            Self::UdfInitialize => "udf_initialize".to_string(),
            Self::LoadModule => "load_module".to_string(),
        }
    }
}

/// A `Timeout` is an asynchronous background job that terminates an
/// `ExecutionHandle` after some time has passed. The holder of a `Timeout` can
/// temporarily pause the termination countdown with [`Timeout::pause`] and then
/// resume time tracking by dropping the returned guard.
///
/// If the higher level operation succeeds, call `Timeout::finish` to cancel the
/// background job and prevent it from terminating the isolate.
pub struct Timeout<RT: Runtime> {
    handle: Box<dyn SpawnHandle>,
    inner: Arc<Mutex<TimeoutInner<RT>>>,
    done_rx: async_broadcast::Receiver<()>,
    pub permit: Option<ConcurrencyPermit>,
}

struct TimeoutInner<RT: Runtime> {
    rt: RT,

    start: tokio::time::Instant,
    timeout: Option<Duration>,

    // How long has the timeout been in the paused state?
    pause_elapsed: Duration,
    max_time_paused: Option<Duration>,

    state: TimeoutState,
    pause_breakdown: HashMap<PauseReason, (usize, Duration)>,
}

impl<RT: Runtime> TimeoutInner<RT> {
    fn termination_reason_or_wait(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<TerminationReason>, impl Future<Output = ()> + 'static + use<RT>> {
        let initial_deadline = self.start + timeout;
        match self.state {
            TimeoutState::Running => {
                // Extend the deadline by the time spent paused.
                let deadline = initial_deadline + self.pause_elapsed;
                let now = self.rt.monotonic_now();
                if now >= deadline {
                    metrics::log_user_timeout();
                    return Ok(Some(IsolateTerminationReason::UserTimeout(timeout).into()));
                }
                // Wait on our current deadline to pass.
                Err(Either::Left(self.rt.wait(deadline - now)))
            },
            TimeoutState::Paused {
                ref pause_done,
                ref pause_start,
                ref reason,
            } => {
                let expired = if let Some(max_time_paused) = self.max_time_paused {
                    let current_pause_duration = pause_start.elapsed();
                    let total_pause_elapsed = self.pause_elapsed + current_pause_duration;
                    if total_pause_elapsed >= max_time_paused {
                        metrics::log_system_timeout();

                        let mut reasons: Vec<_> = self.pause_breakdown.iter().collect();
                        reasons.sort_by_key(|(_, (_, duration))| std::cmp::Reverse(*duration));
                        let reasons_str = reasons
                            .iter()
                            .map(|(reason, (count, duration))| {
                                format!("{}={}({:?})", reason.as_str(), count, duration)
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        tracing::warn!(
                            "SystemTimeout: pause breakdown: {} ({:?}). Final pause {} ({:?})",
                            reasons_str,
                            total_pause_elapsed,
                            reason.as_str(),
                            current_pause_duration,
                        );

                        return Ok(Some(
                            IsolateTerminationReason::SystemTimeout(max_time_paused).into(),
                        ));
                    }
                    Either::Left(self.rt.wait(max_time_paused - total_pause_elapsed))
                } else {
                    Either::Right(future::pending())
                };
                let mut pause_done = pause_done.clone();
                Err(Either::Right(async move {
                    let _ = future::select(expired, pause_done.recv()).await;
                }))
            },
            TimeoutState::Finished => Ok(None),
        }
    }
}

#[derive(Debug)]
enum TimeoutState {
    Running,
    Paused {
        pause_start: tokio::time::Instant,
        pause_done: async_broadcast::Receiver<()>,
        reason: PauseReason,
    },
    Finished,
}

impl<RT: Runtime> Drop for Timeout<RT> {
    fn drop(&mut self) {
        self.finish();
    }
}

// We default to counting everything as user time but we exempt async syscalls
// from the user timeout and count them as system time instead.
impl<RT: Runtime> Timeout<RT> {
    pub fn new(
        rt: RT,
        handle: ExecutionHandle,
        timeout: Option<Duration>,
        max_time_paused: Option<Duration>,
        permit: ConcurrencyPermit,
    ) -> Self {
        let start = rt.monotonic_now();
        let inner = TimeoutInner {
            rt: rt.clone(),
            start,
            timeout,
            pause_elapsed: Duration::ZERO,
            max_time_paused,
            state: TimeoutState::Running,
            pause_breakdown: HashMap::new(),
        };
        let inner = Arc::new(Mutex::new(inner));
        let (done_tx, done_rx) = broadcast(1);
        let handle = rt.spawn("isolate_timeout", Self::go(handle, inner.clone(), done_tx));
        Self {
            handle,
            inner,
            done_rx,
            permit: Some(permit),
        }
    }

    // Returns a future that resolves when the background timeout thread has
    // completed. This can either happen if the isolate has been terminated
    // due to timeout or the Timeout has been dropped due to an error.
    pub fn wait_until_completed(&self) -> impl Future<Output = ()> + 'static + use<RT> {
        let mut done_rx = self.done_rx.clone();
        async move {
            let _ = done_rx.recv().await;
        }
    }

    /// Runs the future until completely or until the timeout has expired.
    /// Returns an error in the latter case.
    pub fn with_timeout<T, F: Future<Output = T>>(
        &self,
    ) -> impl FnOnce(F) -> (impl Future<Output = anyhow::Result<T>> + use<T, F, RT>) + use<T, F, RT>
    {
        let completed = self.wait_until_completed();
        move |f| async move {
            select! {
                biased;
                result = f => Ok(result),
                // NOTE: When the background thread returns, it either means we have
                // terminated due to timeout or have been dropped. Either way, it
                // is ok to stop executing. The exact error we throw here doesn't
                // matter since we know the isolate layer overrides the error if there
                // is a termination reason.
                _ = completed => anyhow::bail!("Timed out"),
            }
        }
    }

    fn pause_start(
        &mut self,
        reason: PauseReason,
    ) -> (tokio::time::Instant, async_broadcast::Sender<()>) {
        let (tx, rx) = broadcast(1);
        let mut inner = self.inner.lock();
        let TimeoutState::Running = inner.state else {
            panic!("Overlapping calls to timeout.pause()");
        };
        let pause_start = inner.rt.monotonic_now();
        inner.state = TimeoutState::Paused {
            pause_done: rx,
            pause_start,
            reason,
        };
        (pause_start, tx)
    }

    pub fn finish(&mut self) {
        {
            let mut inner = self.inner.lock();
            if matches!(inner.state, TimeoutState::Finished) {
                return;
            }
            inner.state = TimeoutState::Finished;
        }
        self.handle.shutdown();
    }

    pub fn finish_with_permit(mut self) -> anyhow::Result<ConcurrencyPermit> {
        self.permit.take().context("lost the permit")
    }

    // Similar to releasing the GIL in Python, it's advisable to drop the
    // ConcurrencyPermit when entering async code on the V8 thread. This helper also
    // integrates with our user time tracking to not count async code against the
    // user timeout.
    pub async fn with_release_permit_regainable<T>(
        &mut self,
        reason: PauseReason,
        f: impl AsyncFnOnce(&mut PauseGuard<'_, RT>) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let permit = self.permit.take().context("lost the permit")?;
        let timeout = self.with_timeout();
        let (pause_start, tx) = self.pause_start(reason.clone());
        let suspended_permit = permit.suspend();
        let mut pause_guard = PauseGuard {
            timeout: self,
            pause_start,
            pause_done: Some(tx),
            reason,
            suspended_permit: Some(suspended_permit),
        };
        let result = timeout(f(&mut pause_guard))
            .await
            .context(ErrorMetadata::bad_request(
                "SystemTimeoutError",
                SYSTEM_TIMEOUT_ERROR_MESSAGE,
            ))?;
        let suspended_permit = pause_guard
            .suspended_permit
            .take()
            .context("lost the suspended permit")?;
        drop(pause_guard);

        // Time the permit reacquire separately.
        let timeout = self.with_timeout();
        let (pause_start, tx) = self.pause_start(PauseReason::ConcurrencyPermitReacquire);
        let pause_guard = PauseGuard {
            timeout: self,
            pause_start,
            pause_done: Some(tx),
            reason: PauseReason::ConcurrencyPermitReacquire,
            suspended_permit: None,
        };
        let permit =
            timeout(suspended_permit.acquire())
                .await
                .context(ErrorMetadata::bad_request(
                    "SystemTimeoutError",
                    SYSTEM_TIMEOUT_ERROR_MESSAGE,
                ))?;
        drop(pause_guard);
        self.permit = Some(permit);
        result
    }

    pub async fn with_release_permit<T>(
        &mut self,
        reason: PauseReason,
        f: impl Future<Output = anyhow::Result<T>>,
    ) -> anyhow::Result<T> {
        pin_mut!(f);
        // If the future completes without blocking, don't pause.
        // This is useful if `f` is just reading from caches.
        if let Some(r) = f.as_mut().now_or_never() {
            return r;
        }
        self.with_release_permit_regainable(reason, async move |_| f.await)
            .await
    }

    async fn go(
        handle: ExecutionHandle,
        inner: Arc<Mutex<TimeoutInner<RT>>>,
        done_tx: async_broadcast::Sender<()>,
    ) {
        let timeout = match inner.lock().timeout {
            None => return,
            Some(timeout) => timeout,
        };
        let termination_reason = loop {
            let future = match inner.lock().termination_reason_or_wait(timeout) {
                Ok(termination_reason) => break termination_reason,
                Err(future) => future,
            };
            future.await;
        };
        if let Some(reason) = termination_reason {
            handle.terminate(reason);
        }
        let _ = done_tx.try_broadcast(());
    }

    pub fn into_function_execution_time(self, udf_type: UdfType) -> FunctionExecutionTime {
        let inner = self.inner.lock();
        let elapsed = inner.rt.monotonic_now() - inner.start - inner.pause_elapsed;
        let limit = inner.timeout.unwrap_or(Duration::ZERO);
        metrics::log_user_function_execution_time(udf_type, elapsed);
        FunctionExecutionTime { elapsed, limit }
    }
}

/// Starts a request on `handle`, arming a `Timeout` against the environment's
/// user and system timeouts. `max_user_timeout` is an upper bound set by tests.
///
/// The caller is responsible for popping the returned `ContextId`.
pub fn start_request_on_handle<RT: Runtime, E: JsEnvironment<RT>>(
    rt: RT,
    handle: &ExecutionHandle,
    permit: ConcurrencyPermit,
    environment: &E,
    max_user_timeout: Option<Duration>,
) -> (ContextId, Timeout<RT>) {
    let context_id = handle.push_context(false /* nested */);
    let mut user_timeout = environment.user_timeout();
    if let Some(max_user_timeout) = max_user_timeout {
        user_timeout = user_timeout.min(max_user_timeout);
    }
    let timeout = Timeout::new(
        rt,
        handle.clone(),
        Some(user_timeout),
        Some(environment.system_timeout()),
        permit,
    );
    (context_id, timeout)
}

/// Starts a request on a fresh handle for an engine with no V8 isolate behind
/// it. The timeout can only record its reason, so the engine must poll
/// [`ExecutionHandle::check_terminated`] between steps to notice it.
/// TODO(runtime): Replace this when we have an impl that uses epoch
/// interruption in wasm
pub fn start_cooperative_request<RT: Runtime, E: JsEnvironment<RT>>(
    rt: RT,
    permit: ConcurrencyPermit,
    environment: &E,
    max_user_timeout: Option<Duration>,
) -> (ExecutionHandle, ContextId, Timeout<RT>) {
    let handle = ExecutionHandle::cooperative();
    let (context_id, timeout) =
        start_request_on_handle(rt, &handle, permit, environment, max_user_timeout);
    (handle, context_id, timeout)
}

#[derive(Debug)]
pub struct FunctionExecutionTime {
    pub elapsed: Duration,
    pub limit: Duration,
}

pub struct PauseGuard<'a, RT: Runtime> {
    timeout: &'a mut Timeout<RT>,
    pause_start: tokio::time::Instant,
    pause_done: Option<async_broadcast::Sender<()>>,
    reason: PauseReason,
    suspended_permit: Option<SuspendedPermit>,
}

pub struct RegainedTimeout<'a, 'b, RT: Runtime> {
    paused: &'b mut PauseGuard<'a, RT>,
}

impl<'a, 'b, RT: Runtime> Deref for RegainedTimeout<'a, 'b, RT> {
    type Target = Timeout<RT>;

    fn deref(&self) -> &Self::Target {
        self.paused.timeout
    }
}
impl<'a, 'b, RT: Runtime> DerefMut for RegainedTimeout<'a, 'b, RT> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.paused.timeout
    }
}

impl<'a, 'b, RT: Runtime> Drop for RegainedTimeout<'a, 'b, RT> {
    fn drop(&mut self) {
        if let Some(permit) = self.paused.timeout.permit.take() {
            self.paused.suspended_permit = Some(permit.suspend());
        }
        let (pause_start, tx) = self.paused.timeout.pause_start(self.paused.reason.clone());
        self.paused.pause_start = pause_start;
        self.paused.pause_done = Some(tx);
    }
}

impl<'a, RT: Runtime> PauseGuard<'a, RT> {
    pub async fn regain<'b>(&'b mut self) -> anyhow::Result<RegainedTimeout<'a, 'b, RT>> {
        let permit = self
            .suspended_permit
            .take()
            .context("lost the suspended permit")?
            .acquire()
            .await;
        self.timeout.permit = Some(permit);
        self.unpause();
        Ok(RegainedTimeout { paused: self })
    }
}

impl<RT: Runtime> PauseGuard<'_, RT> {
    fn unpause(&mut self) {
        let Some(tx) = self.pause_done.take() else {
            return;
        };
        {
            let mut inner = self.timeout.inner.lock();
            assert!(matches!(inner.state, TimeoutState::Paused { .. }));

            let pause_duration = self.pause_start.elapsed();
            inner.pause_elapsed += pause_duration;

            let entry = inner
                .pause_breakdown
                .entry(self.reason.clone())
                .or_insert((0, Duration::ZERO));
            entry.0 += 1;
            entry.1 += pause_duration;

            inner.state = TimeoutState::Running;
        }
        let _ = tx.try_broadcast(());
    }
}

impl<RT: Runtime> Drop for PauseGuard<'_, RT> {
    fn drop(&mut self) {
        self.unpause();
    }
}
