use std::{
    cmp::max,
    collections::VecDeque,
    future::poll_fn,
    pin::Pin,
    sync::Arc,
    task::{
        ready,
        Context,
        Poll,
    },
    time::Duration,
};

use event_listener::Event;
use futures::{
    future::BoxFuture,
    Future,
    Stream,
};
use parking_lot::Mutex;
use tokio::time::Instant;

use crate::{
    knobs::{
        CODEL_QUEUE_CONGESTED_EXPIRATION_MILLIS,
        CODEL_QUEUE_IDLE_EXPIRATION_MILLIS,
    },
    metrics::{
        log_codel_queue_overloaded,
        log_codel_queue_size,
        log_codel_queue_time_since_empty,
    },
    runtime::Runtime,
};

#[derive(thiserror::Error, Debug)]
#[error("Queue full")]
pub struct QueueFull;

/// Instead of simply dropping items from the queue,
/// we return expired items so the caller can dispose of them.
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
#[error("Item expired in queue")]
pub struct ExpiredInQueue;

/// Queue for buffering requests while avoiding consistently large latency.
/// Following the algorithm described at https://queue.acm.org/detail.cfm?id=2839461
///
/// There's an alternate C++
/// implementation at https://github.com/facebook/folly/blob/main/folly/executors/Codel.cpp
/// which was not used in the making of this implementation.
pub struct CoDelQueue<RT: Runtime, T> {
    rt: RT,
    /// (item, expiration)
    buffer: VecDeque<(T, Instant)>,
    capacity: usize,
    last_time_empty: Instant,
    idle_expiration: Duration,
    congested_expiration: Duration,
}

impl<RT: Runtime, T> CoDelQueue<RT, T> {
    pub fn new_with_defaults(rt: RT, capacity: usize) -> Self {
        Self::new(
            rt,
            capacity,
            *CODEL_QUEUE_IDLE_EXPIRATION_MILLIS,
            *CODEL_QUEUE_CONGESTED_EXPIRATION_MILLIS,
        )
    }

    pub fn new(
        rt: RT,
        capacity: usize,
        idle_expiration: Duration,
        congested_expiration: Duration,
    ) -> Self {
        let last_time_empty = rt.monotonic_now();
        Self {
            rt,
            buffer: VecDeque::new(),
            capacity,
            last_time_empty,
            idle_expiration,
            congested_expiration,
        }
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    fn _is_idle(&self, now: Instant) -> bool {
        (self.last_time_empty + self.idle_expiration) > now
    }

    fn update_last_time_empty(&mut self, now: Instant) {
        if self.is_empty() {
            self.last_time_empty = now;
        }
        self.log_metrics(now);
    }

    fn log_metrics(&self, now: Instant) {
        log_codel_queue_size(self.len());
        log_codel_queue_overloaded(!self._is_idle(now));
        log_codel_queue_time_since_empty(now - self.last_time_empty)
    }

    pub fn push(&mut self, item: T) -> Result<(), (QueueFull, T)> {
        if self.len() >= self.capacity {
            return Err((QueueFull, item));
        }
        let now = self.rt.monotonic_now();
        self.update_last_time_empty(now);
        // the time at which we would transition from idle to congested;
        // this may be in the past
        // `self.last_time_empty` can't change during the lifetime of an item,
        // so this is fine to calculate now.
        // N.B.: `now + self.idle_expiration >= congested_time` always holds, so
        // we will always be in the congested regime by the time any item
        // expires
        let congested_time = self.last_time_empty + self.idle_expiration;
        let expiration = max(congested_time, now + self.congested_expiration);
        self.buffer.push_back((item, expiration));
        Ok(())
    }

    fn pop_front(&mut self, now: Instant) -> Option<(T, Instant)> {
        let result = self.buffer.pop_front();
        // If the queue is newly empty, update last_empty_time=now().
        // This is redundant since it will remain empty and that will only
        // matter if we check is_idle, which will also update last_empty_time.
        // But it doesn't hurt to keep it updated.
        self.update_last_time_empty(now);
        result
    }

    fn pop_back(&mut self, now: Instant) -> Option<(T, Instant)> {
        let result = self.buffer.pop_back();
        self.update_last_time_empty(now);
        result
    }

    fn pop_expired(&mut self) -> Option<(T, ExpiredInQueue)> {
        let now = self.rt.monotonic_now();
        if let Some((item, _)) = self
            .buffer
            .pop_front_if(|(_, expiration)| *expiration <= now)
        {
            self.update_last_time_empty(now);
            Some((item, ExpiredInQueue))
        } else {
            None
        }
    }

    pub fn pop_with_expiration(&mut self) -> Option<(T, Result<Instant, ExpiredInQueue>)> {
        let now = self.rt.monotonic_now();
        self.update_last_time_empty(now);
        if let Some((_, oldest_expiry_time)) = self.buffer.front()
            && *oldest_expiry_time <= now
        {
            // Drain expired item.
            self.pop_front(now)
                .map(|(item, _)| (item, Err(ExpiredInQueue)))
        } else {
            if self._is_idle(now) {
                // FIFO
                self.pop_front(now)
            } else {
                // LIFO
                self.pop_back(now)
            }
            .map(|(item, expiration)| (item, Ok(expiration)))
        }
    }

    pub fn pop(&mut self) -> Option<(T, Option<ExpiredInQueue>)> {
        self.pop_with_expiration()
            .map(|(item, expiration)| (item, expiration.err()))
    }

    pub fn into_sender_and_receiver(self) -> (CoDelQueueSender<RT, T>, CoDelQueueReceiver<RT, T>) {
        let inner = Arc::new(Mutex::new(Inner {
            queue: self,
            event: Event::new(),
            expired_event: Event::new(),
            senders: 1,
        }));
        (
            CoDelQueueSender {
                inner: inner.clone(),
            },
            CoDelQueueReceiver {
                inner,
                listener: None,
            },
        )
    }
}

/// Wrapper around CoDelQueue that makes it async.
pub fn new_codel_queue_async<RT: Runtime, T>(
    rt: RT,
    capacity: usize,
) -> (CoDelQueueSender<RT, T>, CoDelQueueReceiver<RT, T>) {
    CoDelQueue::new_with_defaults(rt, capacity).into_sender_and_receiver()
}

struct Inner<RT: Runtime, T> {
    queue: CoDelQueue<RT, T>,
    event: Event,
    expired_event: Event,
    senders: usize,
}

pub struct CoDelQueueReceiver<RT: Runtime, T> {
    inner: Arc<Mutex<Inner<RT, T>>>,
    listener: Option<event_listener::EventListener>,
}

impl<RT: Runtime, T> Clone for CoDelQueueReceiver<RT, T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            listener: None,
        }
    }
}

pub struct CoDelQueueExpiredReceiver<RT: Runtime, T> {
    inner: Arc<Mutex<Inner<RT, T>>>,
    listener: Option<event_listener::EventListener>,
    next_expiry_timer: Option<BoxFuture<'static, ()>>,
}

pub struct CoDelQueueSender<RT: Runtime, T> {
    inner: Arc<Mutex<Inner<RT, T>>>,
}

impl<RT: Runtime, T> Clone for CoDelQueueSender<RT, T> {
    fn clone(&self) -> Self {
        self.inner.lock().senders += 1;
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<RT: Runtime, T> Drop for CoDelQueueSender<RT, T> {
    fn drop(&mut self) {
        let mut inner = self.inner.lock();
        inner.senders -= 1;
        if inner.senders == 0 {
            // Queue is closed. Wake up all receivers so they return None.
            inner.event.notify(usize::MAX);
            inner.expired_event.notify(usize::MAX);
        }
    }
}

impl<RT: Runtime, T> CoDelQueueSender<RT, T> {
    pub fn try_send(&self, item: T) -> Result<(), QueueFull> {
        // N.B.: on error, only drops `item` after releasing the lock
        self.try_send_or_recover(item)
            .map_err(|(QueueFull, _)| QueueFull)
    }

    pub fn try_send_or_recover(&self, item: T) -> Result<(), (QueueFull, T)> {
        let mut inner = self.inner.lock();
        inner.queue.push(item)?;
        inner.event.notify_additional(1);
        // All `CoDelQueueExpiredReceiver`s need to be woken since they don't consume
        // the queue item
        inner.expired_event.notify(usize::MAX);
        Ok(())
    }
}

impl<RT: Runtime, T> CoDelQueueReceiver<RT, T> {
    pub fn poll_next_with_expiration(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<(T, Result<Instant, ExpiredInQueue>)>> {
        let mut inner = self.inner.lock();
        // If there is an item in the queue, pop it.
        // If the queue is closed, return None.
        if let Some(result) = inner.queue.pop_with_expiration() {
            return Poll::Ready(Some(result));
        } else if inner.senders == 0 {
            return Poll::Ready(None);
        }

        loop {
            match Pin::new(self.listener.get_or_insert_with(|| inner.event.listen())).poll(cx) {
                // The queue is still empty. The listener is stored for the next
                // poll, and it has registered with cx.waker to be woken when
                // it is notified of the queue becoming nonempty.
                Poll::Pending => return Poll::Pending,
                Poll::Ready(()) => {
                    // This should not happen, because the listener is only notified
                    // when the queue state changes, which is impossible while we are
                    // holding self.inner.lock(). But we can be defensive in case of
                    // spurious wakeups, by dropping the listener and looping.
                    self.listener.take();
                    continue;
                },
            }
        }
    }

    /// Like `.next()`, but additionally returns the expiration time for
    /// non-expired requests.
    pub async fn recv_with_expiration(&mut self) -> Option<(T, Result<Instant, ExpiredInQueue>)> {
        poll_fn(|cx| self.poll_next_with_expiration(cx)).await
    }

    /// Returns a stream that yields only expired entries from the queue. If
    /// nothing has expired, it blocks until the next expiry.
    pub fn expired_receiver(&self) -> CoDelQueueExpiredReceiver<RT, T> {
        CoDelQueueExpiredReceiver {
            inner: self.inner.clone(),
            listener: None,
            next_expiry_timer: None,
        }
    }
}

impl<RT: Runtime, T> Stream for CoDelQueueReceiver<RT, T> {
    type Item = (T, Option<ExpiredInQueue>);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.poll_next_with_expiration(cx) {
            Poll::Ready(Some((item, expiration))) => Poll::Ready(Some((item, expiration.err()))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<RT: Runtime, T> Stream for CoDelQueueExpiredReceiver<RT, T> {
    type Item = (T, ExpiredInQueue);

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<(T, ExpiredInQueue)>> {
        let this = &mut *self;
        let mut inner = this.inner.lock();
        loop {
            if let Some(result) = inner.queue.pop_expired() {
                return Poll::Ready(Some(result));
            } else if inner.senders == 0 {
                return Poll::Ready(None);
            } else if let Some(&(_, next_expiry_time)) = inner.queue.buffer.front() {
                ready!(this
                    .next_expiry_timer
                    .insert(inner.queue.rt.wait(
                        next_expiry_time.saturating_duration_since(inner.queue.rt.monotonic_now()),
                    ))
                    .as_mut()
                    .poll(cx));
                // Timer completed immediately, try again
                continue;
            }
            break;
        }

        loop {
            // See comments on `poll_next_with_expiration`
            match Pin::new(
                this.listener
                    .get_or_insert_with(|| inner.expired_event.listen()),
            )
            .poll(cx)
            {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(()) => {
                    this.listener.take();
                    continue;
                },
            }
        }
    }
}
