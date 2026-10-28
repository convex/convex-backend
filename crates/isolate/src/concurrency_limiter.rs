use std::{
    mem,
    pin::pin,
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use common::runtime::Runtime;
use fastrace::{
    func_path,
    Span,
};
use futures::Future;
use parking_lot::Mutex;
use scopeguard::ScopeGuard;
use slab::Slab;
use tokio::sync::Notify;

use crate::metrics::{
    concurrency_permit_acquire_timer,
    log_concurrency_permit_used,
};

/// ConcurrencyLimiter is used to limit how many isolate threads can be
/// actively running at the same time. We use it to avoid oversubscribing the
/// CPU, which would result in unexpected user timeouts and arbitrary slow
/// downs throughout the system.
///
/// When the max number of permits has been handed out, `acquire` calls queue up
/// in a "high priority" and a "low priority" queue. The high priority queue is
/// used for functions that are re-acquiring a permit after execution has
/// already started, while low priority is given to new function invocations.
/// When a permit becomes available, it is always given to a high priority
/// waiter (if present) before any low priority waiter.
#[derive(Clone, Debug)]
pub struct ConcurrencyLimiter {
    inner: Arc<ConcurrencyLimiterInner>,
}

#[derive(Debug)]
struct ConcurrencyLimiterInner {
    low_priority: Notify,
    high_priority: Notify,
    tracker: Mutex<ActivePermitsTracker>,
    max_permits: usize,
}

#[derive(Debug)]
struct ActivePermitsTracker {
    // invariant: `active_permits.len() <= max_permits`
    active_permits: Slab<(Arc<String>, Instant)>,
    // invariant: `(active_permits.len() + waiting_high_priority).saturating_sub(max_permits)`
    // equals the number of unwoken high-priority `Notified` futures
    waiting_high_priority: usize,
}

impl ConcurrencyLimiter {
    pub fn new(max_concurrency: usize) -> Self {
        assert!(
            max_concurrency > 0,
            "max_concurrency must be greater than zero"
        );
        Self {
            inner: Arc::new(ConcurrencyLimiterInner {
                low_priority: Notify::new(),
                high_priority: Notify::new(),
                tracker: Mutex::new(ActivePermitsTracker {
                    active_permits: Slab::new(),
                    waiting_high_priority: 0,
                }),
                max_permits: max_concurrency,
            }),
        }
    }

    pub fn unlimited() -> Self {
        Self::new(usize::MAX)
    }

    pub fn active_permits(&self) -> usize {
        self.inner.tracker.lock().active_permits.len()
    }

    pub fn max_permits(&self) -> Option<usize> {
        if self.inner.max_permits == usize::MAX {
            None
        } else {
            Some(self.inner.max_permits)
        }
    }

    // If a client uses a thread for too long. We still want to log periodically.
    pub fn go_log<RT: Runtime>(
        &self,
        rt: RT,
        frequency: Duration,
    ) -> impl Future<Output = ()> + use<RT> {
        let inner = self.inner.clone();
        async move {
            loop {
                rt.wait(frequency).await;
                let current_permits = inner.tracker.lock().reset_start_time();
                for (client_id, start_time) in current_permits {
                    if start_time.elapsed() >= frequency {
                        tracing::warn!(
                            "{client_id} held concurrency semaphore for more than {frequency:?}"
                        );
                    }
                    log_concurrency_permit_used(client_id, start_time.elapsed());
                }
            }
        }
    }

    pub async fn acquire(&self, client_id: Arc<String>, high_priority: bool) -> ConcurrencyPermit {
        let timer = concurrency_permit_acquire_timer();
        let mut span = None;
        let mut tracker = loop {
            let mut notify_future = pin!(None);
            {
                let mut tracker = self.inner.tracker.lock();
                if tracker.admits_low_priority(self.inner.max_permits) {
                    break tracker;
                }
                span.get_or_insert_with(|| Span::enter_with_local_parent(func_path!()));
                let notify = if high_priority {
                    tracker.waiting_high_priority += 1;
                    &self.inner.high_priority
                } else {
                    &self.inner.low_priority
                };
                notify_future.as_mut().set(Some(notify.notified()));
                // N.B.: enable while still holding the lock.
                let had_permit = notify_future.as_mut().as_pin_mut().unwrap().enable();
                // There should never be a permit saved in the `high_priority`
                // Notify since we do our own accounting to transfer the permits to
                // low_priority.
                assert!(!(had_permit && high_priority));
                drop(tracker);
            }
            // In case of cancellation, decrement `waiting_high_priority` and notify
            // low-priority waiters if appropriate
            let mut guard = scopeguard::guard(notify_future, |mut notify_future| {
                if high_priority {
                    let mut tracker = self.inner.tracker.lock();
                    tracker.waiting_high_priority -= 1;
                    // If we've been notified, consume it and transfer the
                    // notification to the next waiter. Do this under lock so
                    // that it goes to the right queue.
                    if notify_future.as_mut().as_pin_mut().unwrap().enable() {
                        if tracker.admits_low_priority(self.inner.max_permits) {
                            self.inner.low_priority.notify_one();
                        } else {
                            self.inner.high_priority.notify_one();
                        }
                    }
                    // Otherwise drop the future right away so that we
                    // definitely don't get a notification
                    notify_future.as_mut().set(None);
                }
                // For low priority waiters, the `Notified` future will drop and
                // eventually transfer its notification automatically
            });
            guard.as_mut().as_pin_mut().unwrap().await;
            ScopeGuard::into_inner(guard);
            if high_priority {
                let mut tracker = self.inner.tracker.lock();
                // high-priority is always FIFO, so we can continue as soon as
                // we are notified
                tracker.waiting_high_priority -= 1;
                break tracker;
            }
            // ... but low-priority callers can barge so we
            // need to re-check that there are available permits
        };
        assert!(tracker.active_permits.len() < self.inner.max_permits);
        let permit_id = tracker.register(client_id.clone());
        timer.finish(true);
        drop(tracker);
        ConcurrencyPermit {
            permit_id,
            limiter: self.clone(),
            client_id,
        }
    }
}

#[derive(Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq)]
struct PermitId(usize);

impl ActivePermitsTracker {
    fn register(&mut self, client_id: Arc<String>) -> PermitId {
        PermitId(self.active_permits.insert((client_id, Instant::now())))
    }

    fn deregister(&mut self, id: PermitId) -> Duration {
        let (_, start) = self.active_permits.remove(id.0);
        start.elapsed()
    }

    fn admits_low_priority(&self, max_permits: usize) -> bool {
        self.active_permits.len() + self.waiting_high_priority < max_permits
    }

    fn reset_start_time(&mut self) -> Vec<(Arc<String>, Instant)> {
        let now = Instant::now();
        self.active_permits
            .iter_mut()
            .map(|(_, (client, start))| (client.clone(), mem::replace(start, now)))
            .collect()
    }
}

#[derive(Debug)]
pub struct ConcurrencyPermit {
    permit_id: PermitId,

    limiter: ConcurrencyLimiter,
    client_id: Arc<String>,
}

impl ConcurrencyPermit {
    pub async fn with_suspend<'a, T>(
        self,
        f: impl Future<Output = T> + 'a,
    ) -> (T, ConcurrencyPermit) {
        let regain = self.suspend();
        let result = f.await;
        let permit = regain.acquire().await;
        (result, permit)
    }

    pub fn suspend(self) -> SuspendedPermit {
        let client_id = self.client_id.clone();
        let limiter = self.limiter.clone();
        SuspendedPermit { client_id, limiter }
    }

    pub fn limiter(&self) -> &ConcurrencyLimiter {
        &self.limiter
    }
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        let mut tracker = self.limiter.inner.tracker.lock();
        let duration = tracker.deregister(self.permit_id);
        if tracker.admits_low_priority(self.limiter.inner.max_permits) {
            self.limiter.inner.low_priority.notify_one();
        } else {
            self.limiter.inner.high_priority.notify_one();
        }
        drop(tracker);
        log_concurrency_permit_used(self.client_id.clone(), duration);
    }
}

pub struct SuspendedPermit {
    limiter: ConcurrencyLimiter,
    client_id: Arc<String>,
}

impl SuspendedPermit {
    pub async fn acquire(self) -> ConcurrencyPermit {
        self.limiter
            .acquire(self.client_id, true /* high_priority */)
            .await
    }
}
