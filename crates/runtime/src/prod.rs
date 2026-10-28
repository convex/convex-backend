//! Production implementation of the Runtime trait.

use std::{
    future::Future,
    pin::Pin,
    sync::LazyLock,
    task::{
        ready,
        Poll,
    },
    thread,
    time::{
        Instant,
        SystemTime,
    },
};

use async_trait::async_trait;
use common::{
    knobs::{
        RUNTIME_DISABLE_LIFO_SLOT,
        RUNTIME_STACK_SIZE,
        RUNTIME_WORKER_THREADS,
    },
    pause::PauseClient,
    runtime::{
        propagate_tracing,
        propagate_tracing_blocking,
        JoinError,
        Runtime,
        SpawnHandle,
        TokioSpawnHandle,
        GLOBAL_TASK_MANAGER,
    },
};
use futures::{
    future::FusedFuture,
    FutureExt,
};
use rand::RngCore;
use tokio::{
    runtime::{
        Builder,
        Handle as TokioRuntimeHandle,
        Runtime as TokioRuntime,
    },
    sync::oneshot,
    time::{
        sleep,
        Duration,
    },
};
use tokio_metrics_collector::TaskMonitor;

static INSTANT_EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

pub struct ThreadHandle {
    cancel: Option<oneshot::Sender<()>>,
    done: Option<oneshot::Receiver<bool>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl SpawnHandle for ThreadHandle {
    fn shutdown(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }

    fn poll_join(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), JoinError>> {
        if let Some(done) = &mut self.done {
            let result = ready!(Pin::new(done).poll(cx));
            self.done = None;
            match result {
                // the future was not canceled
                Ok(false) => Poll::Ready(Ok(())),
                // the future was canceled by `.shutdown()`
                Ok(true) => Poll::Ready(Err(JoinError::Canceled)),
                Err(_) => {
                    let join_r = self.handle.take().expect("Future completed twice?").join();
                    // Otherwise look at the result from `std::thread` to see if it panicked.
                    let join_err =
                        join_r.expect_err("Future didn't exit cleanly but didn't panic?");
                    Poll::Ready(Err(JoinError::Panicked(anyhow::anyhow!("{:?}", join_err))))
                },
            }
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn detach(mut self: Box<Self>) {
        self.cancel.take();
    }
}

impl Drop for ThreadHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl ThreadHandle {
    fn spawn<Fut, F>(name: String, tokio_handle: TokioRuntimeHandle, f: F) -> Self
    where
        Fut: Future<Output = ()>,
        F: FnOnce() -> Fut + Send + 'static,
    {
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (done_tx, done_rx) = oneshot::channel();
        let thread_handle = thread::Builder::new()
            .stack_size(*RUNTIME_STACK_SIZE)
            .name(name)
            .spawn(move || {
                let _guard = tokio_handle.enter();
                let thread_body = async move {
                    let future = f();
                    tokio::pin!(future);
                    let was_canceled = tokio::select! {
                        r = cancel_rx => {
                            if r.is_ok() {
                                true
                            } else {
                                future.await;
                                false
                            }
                        },
                        _ = &mut future => false,
                    };
                    let _ = done_tx.send(was_canceled);
                };
                tokio_handle.block_on(thread_body);
            })
            .expect("Failed to start thread");
        ThreadHandle {
            handle: Some(thread_handle),
            cancel: Some(cancel_tx),
            done: Some(done_rx),
        }
    }
}

/// Runtime for running in production that sleeps for wallclock time, doesn't
/// mock out any functionality, etc.
#[derive(Clone)]
pub struct ProdRuntime {
    rt: TokioRuntimeHandle,
}

impl ProdRuntime {
    pub fn init_tokio() -> anyhow::Result<TokioRuntime> {
        assert!(
            TokioRuntimeHandle::try_current().is_err(),
            "Tried to create a `ProdRuntime` from within a Tokio context. Are you using \
             `#[tokio::main]` or `#[tokio::test]`?"
        );
        let mut tokio_builder = Builder::new_multi_thread();
        tokio_builder.thread_stack_size(*RUNTIME_STACK_SIZE);
        if *RUNTIME_WORKER_THREADS > 0 {
            tokio_builder.worker_threads(*RUNTIME_WORKER_THREADS);
        }
        if *RUNTIME_DISABLE_LIFO_SLOT {
            tokio_builder.disable_lifo_slot();
        }
        let tokio_rt = tokio_builder.enable_all().build()?;

        // Register a no-op SIGUSR1 handler so services don't crash when
        // receiving an unintended signal (the default disposition is to
        // terminate the process). The OS-level handler persists for the
        // lifetime of the process once registered.
        #[cfg(unix)]
        {
            let _guard = tokio_rt.enter();
            let _sig =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined1())
                    .expect("Failed to register SIGUSR1 handler");
        }

        Ok(tokio_rt)
    }

    pub fn task_monitor(name: &'static str) -> TaskMonitor {
        GLOBAL_TASK_MANAGER.lock().get(name)
    }

    /// Create a new tokio-based runtime.
    /// Expected usage:
    /// ```rust
    /// use runtime::prod::ProdRuntime;
    /// fn main() -> anyhow::Result<()> {
    ///     let tokio = ProdRuntime::init_tokio()?;
    ///     let rt = ProdRuntime::new(&tokio);
    ///     rt.block_on(async {});
    ///     Ok(())
    /// }
    /// ```
    /// The `tokio_rt` should live for the duration of `main`.
    /// At the end of `main` its `Drop` will run and join all spawned futures,
    /// which should include all references to the handle `ProdRuntime`.
    /// If `ProdRuntime` is used after the associated `TokioRuntime` has been
    /// dropped, it will panic.
    pub fn new(tokio_rt: &TokioRuntime) -> Self {
        let handle = tokio_rt.handle().clone();

        Self { rt: handle }
    }

    pub fn block_on<F: Future>(&self, name: &'static str, f: F) -> F::Output {
        let monitor = GLOBAL_TASK_MANAGER.lock().get(name);
        self.rt.block_on(monitor.instrument(f))
    }
}

#[async_trait]
impl Runtime for ProdRuntime {
    fn wait(&self, duration: Duration) -> Pin<Box<dyn FusedFuture<Output = ()> + Send + 'static>> {
        Box::pin(sleep(duration).fuse())
    }

    fn spawn(
        &self,
        name: &'static str,
        f: impl Future<Output = ()> + Send + 'static,
    ) -> Box<dyn SpawnHandle> {
        let monitor = GLOBAL_TASK_MANAGER.lock().get(name);
        let handle = self.rt.spawn(propagate_tracing(monitor.instrument(f)));
        Box::new(TokioSpawnHandle::from(handle))
    }

    fn spawn_thread<Fut: Future<Output = ()>, F: FnOnce() -> Fut + Send + 'static>(
        &self,
        name: &str,
        f: F,
    ) -> Box<dyn SpawnHandle> {
        Box::new(ThreadHandle::spawn(
            name.to_owned(),
            self.rt.clone(),
            propagate_tracing_blocking(move || propagate_tracing(f())),
        ))
    }

    fn system_time(&self) -> SystemTime {
        SystemTime::now()
    }

    fn monotonic_now(&self) -> tokio::time::Instant {
        // Guarantee that all `ProdInstant`s handed out are after `SYNC_EPOCH`.
        LazyLock::force(&INSTANT_EPOCH);
        tokio::time::Instant::now()
    }

    fn rng(&self) -> Box<dyn RngCore> {
        // `rand`'s default RNG is designed to be cryptographically secure:
        // > The PRNG algorithm in StdRng is chosen to be efficient on the current
        // platform, to be > statistically strong and unpredictable (meaning a
        // cryptographically secure PRNG). (Source: https://docs.rs/rand/latest/rand/rngs/struct.StdRng.html)
        Box::new(rand::rng())
    }

    fn pause_client(&self) -> PauseClient {
        PauseClient::new()
    }
}
