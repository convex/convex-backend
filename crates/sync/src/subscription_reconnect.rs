use std::{
    collections::{
        HashMap,
        VecDeque,
    },
    fmt,
    sync::Arc,
    time::Duration,
};

use common::runtime::Runtime;
use parking_lot::Mutex;
use tokio::{
    sync::Notify,
    time::Instant,
};

/// Limits reconnect query replays independently per partition within one Usher
/// process. Reservations are FIFO and weighted by the number of queries they
/// will replay.
#[derive(Clone)]
pub struct SubscriptionReconnectRateLimiter {
    inner: Arc<Mutex<LimiterState>>,
    queries_per_second: f64,
}

impl fmt::Debug for SubscriptionReconnectRateLimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubscriptionReconnectRateLimiter")
            .field("queries_per_second", &self.queries_per_second)
            .finish_non_exhaustive()
    }
}

struct LimiterState {
    next_id: u64,
    partition_queues: HashMap<u64, PartitionQueue>,
}

struct PartitionQueue {
    /// Earliest time the next reservation may start. It represents
    /// already-admitted work.
    next_available: Option<Instant>,
    /// FIFO order may contain canceled IDs; `reservations` owns the live
    /// entries.
    queue: VecDeque<u64>,
    reservations: HashMap<u64, QueuedReservation>,
}

struct QueuedReservation {
    query_count: usize,
    notify: Arc<Notify>,
}

/// A replay reservation. Dropping it before admission cancels it.
pub struct SubscriptionReconnectReservation {
    limiter: SubscriptionReconnectRateLimiter,
    id: Option<u64>,
    partition_id: u64,
    notify: Arc<Notify>,
}

impl SubscriptionReconnectRateLimiter {
    pub fn new(queries_per_second: f64) -> anyhow::Result<Self> {
        anyhow::ensure!(
            queries_per_second.is_finite() && queries_per_second > 0.0,
            "subscription reconnect queries per second must be finite and positive"
        );
        Ok(Self {
            inner: Arc::new(Mutex::new(LimiterState {
                next_id: 0,
                partition_queues: HashMap::new(),
            })),
            queries_per_second,
        })
    }

    /// Reserves capacity for a query set.
    ///
    /// The first reservation starts immediately and spaces the next admission
    /// by its size. Empty query sets are admitted immediately without
    /// consuming capacity.
    pub fn reserve(
        &self,
        partition_id: u64,
        query_count: usize,
        now: Instant,
    ) -> SubscriptionReconnectReservation {
        if query_count == 0 {
            return SubscriptionReconnectReservation {
                limiter: self.clone(),
                id: None,
                partition_id,
                notify: Arc::new(Notify::new()),
            };
        }

        let notify = Arc::new(Notify::new());
        let mut state = self.inner.lock();
        self.prune_idle_partition_queues(&mut state, now);
        let state = &mut *state;
        let next_id = &mut state.next_id;
        let partition_queue =
            state
                .partition_queues
                .entry(partition_id)
                .or_insert(PartitionQueue {
                    next_available: None,
                    queue: VecDeque::new(),
                    reservations: HashMap::new(),
                });
        self.refresh_availability(partition_queue, now);
        let id = if partition_queue.queue.is_empty() && partition_queue.next_available.is_none() {
            partition_queue.next_available = Some(now + self.spacing(query_count));
            None
        } else {
            let id = *next_id;
            *next_id += 1;
            partition_queue.queue.push_back(id);
            partition_queue.reservations.insert(
                id,
                QueuedReservation {
                    query_count,
                    notify: notify.clone(),
                },
            );
            Some(id)
        };
        SubscriptionReconnectReservation {
            limiter: self.clone(),
            id,
            partition_id,
            notify,
        }
    }

    fn spacing(&self, query_count: usize) -> Duration {
        Duration::from_secs_f64(query_count as f64 / self.queries_per_second)
    }

    /// Expires elapsed spacing and wakes the front waiter.
    fn refresh_availability(&self, partition_queue: &mut PartitionQueue, now: Instant) {
        Self::front_reservation(partition_queue);
        if partition_queue
            .next_available
            .is_some_and(|next_available| next_available <= now)
        {
            partition_queue.next_available = None;
            if let Some((_, reservation)) = Self::front_reservation(partition_queue) {
                reservation.notify.notify_one();
            }
        }
    }

    /// Removes canceled IDs and returns the first live reservation.
    fn front_reservation(
        partition_queue: &mut PartitionQueue,
    ) -> Option<(u64, &QueuedReservation)> {
        while partition_queue
            .queue
            .front()
            .is_some_and(|id| !partition_queue.reservations.contains_key(id))
        {
            partition_queue.queue.pop_front();
        }
        let id = *partition_queue.queue.front()?;
        let reservation = partition_queue
            .reservations
            .get(&id)
            .expect("queue front must have a live reservation");
        Some((id, reservation))
    }

    fn prune_idle_partition_queues(&self, state: &mut LimiterState, now: Instant) {
        state.partition_queues.retain(|_, partition_queue| {
            self.refresh_availability(partition_queue, now);
            partition_queue.next_available.is_some() || !partition_queue.reservations.is_empty()
        });
    }

    fn try_admit(&self, partition_id: u64, id: u64, now: Instant) -> Admission {
        let mut state = self.inner.lock();
        self.prune_idle_partition_queues(&mut state, now);
        {
            let partition_queue = state
                .partition_queues
                .get_mut(&partition_id)
                .expect("queued reservation must retain its partition queue");
            let is_next =
                Self::front_reservation(partition_queue).is_some_and(|(next_id, _)| next_id == id);
            if is_next && partition_queue.next_available.is_none() {
                let admitted_id = partition_queue
                    .queue
                    .pop_front()
                    .expect("checked queue front");
                let reservation = partition_queue
                    .reservations
                    .remove(&admitted_id)
                    .expect("queue front must have a live reservation");
                partition_queue.next_available = Some(now + self.spacing(reservation.query_count));
                if let Some((_, next_reservation)) = Self::front_reservation(partition_queue) {
                    next_reservation.notify.notify_one();
                }
                Admission::Admitted
            } else if is_next {
                Admission::FrontWaiting(
                    partition_queue
                        .next_available
                        .expect("queued front must wait for the next admission")
                        .saturating_duration_since(now),
                )
            } else {
                Admission::WaitingForNotification
            }
        }
    }

    /// Cancels without scanning the FIFO, waking a successor when the front
    /// changes.
    fn cancel(&self, partition_id: u64, id: u64) {
        let mut state = self.inner.lock();
        {
            let Some(partition_queue) = state.partition_queues.get_mut(&partition_id) else {
                return;
            };
            let was_front =
                Self::front_reservation(partition_queue).is_some_and(|(next_id, _)| next_id == id);
            partition_queue.reservations.remove(&id);
            if partition_queue.reservations.is_empty() {
                partition_queue.queue.clear();
            } else if was_front
                && let Some((_, next_reservation)) = Self::front_reservation(partition_queue)
            {
                next_reservation.notify.notify_one();
            }
        }
        if state
            .partition_queues
            .get(&partition_id)
            .is_some_and(|partition_queue| {
                partition_queue.next_available.is_none() && partition_queue.reservations.is_empty()
            })
        {
            state.partition_queues.remove(&partition_id);
        }
    }
}

enum Admission {
    Admitted,
    /// The FIFO front waits for its partition's next admission time.
    FrontWaiting(Duration),
    /// Another reservation is ahead and will notify this waiter.
    WaitingForNotification,
}

impl SubscriptionReconnectReservation {
    /// Waits for admission. Only the FIFO front owns a timer; other waiters
    /// await notification.
    pub async fn wait<RT: Runtime>(&mut self, rt: &RT) {
        let Some(id) = self.id else {
            return;
        };

        loop {
            let now = rt.monotonic_now();
            match self.limiter.try_admit(self.partition_id, id, now) {
                Admission::Admitted => {
                    self.id = None;
                    return;
                },
                Admission::FrontWaiting(wait) => {
                    let notify = self.notify.clone();
                    tokio::select! {
                        _ = rt.wait(wait) => {},
                        _ = notify.notified() => {},
                    }
                },
                Admission::WaitingForNotification => {
                    self.notify.notified().await;
                },
            }
        }
    }
}

impl Drop for SubscriptionReconnectReservation {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            self.limiter.cancel(self.partition_id, id);
        }
    }
}
