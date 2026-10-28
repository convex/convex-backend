use std::sync::Arc;

use futures::{
    future::BoxFuture,
    pin_mut,
    Future,
    FutureExt,
    StreamExt,
};
use parking_lot::Mutex;
use sentry::SentryFutureExt as _;
use tokio::sync::{
    mpsc,
    oneshot,
};
use tracing::Instrument as _;

use crate::{
    codel_queue::{
        new_codel_queue_async,
        CoDelQueueReceiver,
        CoDelQueueSender,
        ExpiredInQueue,
    },
    runtime::{
        Runtime,
        SpawnHandle,
    },
};

struct Config {
    name: &'static str,
    max_exec_threads: usize,
}

/// A bounded thread pool based on Runtime's spawn_thread API that can accept
/// and run arbitrary futures.
///
/// Heavily based on IsolateScheduler. The major differences are that we do not
/// store any state per worker and that we can accept arbitrary functions to run
/// rather than a fixed enum of possible actions.
pub struct BoundedThreadPool<RT: Runtime> {
    sender: CoDelQueueSender<RT, Request>,
    handle: Arc<Mutex<Box<dyn SpawnHandle>>>,
}

impl<RT: Runtime> Clone for BoundedThreadPool<RT> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            handle: self.handle.clone(),
        }
    }
}

impl<RT: Runtime> BoundedThreadPool<RT> {
    pub fn new(rt: RT, queue_size: usize, num_threads: usize, name: &'static str) -> Self {
        let (sender, receiver) = new_codel_queue_async::<_, Request>(rt.clone(), queue_size);

        let config = Config {
            max_exec_threads: num_threads,
            name,
        };

        let handles = Arc::new(Mutex::new(Vec::new()));
        let rt_clone = rt.clone();
        let handle = rt.spawn(name, async move {
            let scheduler = Scheduler {
                rt: rt_clone,
                worker_senders: Vec::new(),
                available_workers: Vec::new(),
                handles,
                config,
            };
            scheduler.dispatch(receiver).await
        });
        Self {
            sender,
            handle: Arc::new(Mutex::new(handle)),
        }
    }

    pub async fn execute<T, R>(&self, f: T) -> anyhow::Result<R>
    where
        R: Send + 'static,
        T: FnOnce() -> R + Send + 'static,
    {
        self.execute_async(|| async { f() }).await
    }

    pub async fn execute_async<T, R, Fut>(&self, f: T) -> anyhow::Result<R>
    where
        R: Send + 'static,
        Fut: Future<Output = R> + Send + 'static,
        T: FnOnce() -> Fut + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let sentry_hub = sentry::Hub::current();
        let tracing_span = tracing::Span::current();
        let function = move |maybe_expired: Option<ExpiredInQueue>| {
            async {
                if let Some(expired) = maybe_expired {
                    let _ = tx.send(Err(anyhow::Error::new(expired)));
                } else {
                    let future = f();
                    let result = future.await;
                    let _ = tx.send(Ok(result));
                }
            }
            .bind_hub(sentry_hub)
            .instrument(tracing_span)
            .boxed()
        };
        let request = Request {
            job: Box::new(function),
        };
        self.sender.try_send(request)?;
        let receive_fut = rx.fuse();
        pin_mut!(receive_fut);
        receive_fut.await?
    }
}

struct Scheduler<RT: Runtime> {
    rt: RT,
    // Vec of channels for sending work to individual workers.
    worker_senders: Vec<mpsc::Sender<(Request, mpsc::Sender<usize>, usize)>>,

    // Stack of indexes into worker_senders, including exactly the workers
    // that are not running any request.
    // If we ever add worker specific state, it's important that it's a LIFO
    // stack so that we can prioritize recently used workers that have already
    // loaded the (potentially expensive) state.
    available_workers: Vec<usize>,

    handles: Arc<Mutex<Vec<Box<dyn SpawnHandle>>>>,

    config: Config,
}

impl<RT: Runtime> Scheduler<RT> {
    fn create_worker(&mut self) -> usize {
        let worker_index = self.worker_senders.len();
        let (work_sender, work_receiver) = mpsc::channel(1);
        self.worker_senders.push(work_sender);

        let handle = self.rt.spawn_thread("bounded_thread_pool", move || {
            Self::service_requests(work_receiver)
        });
        self.handles.lock().push(handle);
        worker_index
    }

    async fn service_requests(
        mut work_receiver: mpsc::Receiver<(Request, mpsc::Sender<usize>, usize)>,
    ) {
        // Wait for the next job from our sender.
        while let Some((request, done_sender, worker_index)) = work_receiver.recv().await {
            // Run one job
            request.execute().await;
            // Then tell our sender that we're ready for another job
            let _ = done_sender.try_send(worker_index);
        }
    }

    async fn dispatch(mut self, mut receiver: CoDelQueueReceiver<RT, Request>) {
        let mut expired_receiver = receiver.expired_receiver();
        let (in_progress_tx, mut in_progress_rx) = mpsc::channel(self.config.max_exec_threads);
        loop {
            let can_accept = !self.available_workers.is_empty()
                || self.worker_senders.len() < self.config.max_exec_threads;

            tokio::select! {
                biased;
                // Prioritize draining requests from the in_progress channel.
                Some(w) = in_progress_rx.recv() => {
                    self.available_workers.push(w);
                }
                r = expired_receiver.next(), if !can_accept => {
                    let Some((req, expired)) = r else {
                        return;
                    };
                    req.expire(expired).await;
                }
                r = receiver.next(), if can_accept => {
                    let Some((req, expired)) = r else {
                        // Request queue closed, shutting down.
                        return;
                    };
                    if let Some(expired) = expired {
                        req.expire(expired).await;
                    } else {
                        // `can_accept` implies that if `self.available_workers`
                        // is empty, we can still create more workers
                        let next_worker = self.available_workers
                            .pop()
                            .unwrap_or_else(|| self.create_worker());
                        if self.worker_senders[next_worker]
                            .try_send((req, in_progress_tx.clone(), next_worker))
                            .is_err()
                        {
                            // Available worker should have an empty channel, so if we fail
                            // here it must be shut down. We should shut down too.
                            tracing::warn!(
                                "Worker sender dropped. Shutting down {} scheduler.",
                                self.config.name
                            );
                            return;
                        }
                    }
                }
            };
        }
    }
}

struct Request {
    job: Box<dyn FnOnce(Option<ExpiredInQueue>) -> BoxFuture<'static, ()> + Send + 'static>,
}

impl Request {
    async fn execute(self) {
        (self.job)(None).await
    }

    async fn expire(self, error: ExpiredInQueue) {
        (self.job)(Some(error)).await;
    }
}
