use core::hash::Hash;
use std::{
    borrow::Borrow,
    collections::{
        hash_map::Entry,
        BTreeMap,
        HashMap,
    },
    fmt::Debug,
    sync::Arc,
};

use anyhow::Context as _;
use async_broadcast::Receiver as BroadcastReceiver;
use common::{
    codel_queue::{
        CoDelQueue,
        CoDelQueueReceiver,
        CoDelQueueSender,
    },
    components::{
        ComponentId,
        ComponentPath,
    },
    errors::recapture_stacktrace_noreport,
    runtime::{
        Runtime,
        SpawnHandle,
    },
    types::IndexRef,
};
use fastrace::{
    collector::SpanContext,
    future::{
        FutureExt as _,
        InSpan,
    },
    Span,
};
use futures::{
    future::BoxFuture,
    FutureExt,
    StreamExt,
};
use lru::LruCache;
use parking_lot::Mutex;
use value::{
    TableMapping,
    TabletId,
};

use crate::metrics::{
    async_lru_compute_timer,
    async_lru_get_timer,
    async_lru_log_eviction,
    log_async_lru_cache_hit,
    log_async_lru_cache_miss,
    log_async_lru_cache_waiting,
    log_async_lru_size,
};

const PAUSE_DURING_GENERATE_VALUE_LABEL: &str = "generate_value";

/// A write through cache with support for cancelation.
///
/// Use this class over LruCache when you're in an asynchronous context, you
/// may have multiple concurrent requests for the same key and value generation
/// is relatively expensive. Unlike LruCache, this struct will and ensure that
/// any expensive values are calculated exactly once while also notifying all
/// requestors when that single value calculation finishes.
///
/// Cancelation is handled by using internal worker threads (determined by
/// the value passed to `concurrency` in `AsyncLru::new`) to calculate values.
/// Callers wait asynchronously for the calculation to complete, then are
/// notified via channels. This allows any individual caller to be canceled
/// without risking accidentally canceling the value calculation and triggering
/// errors for other requests to the same key that happen to be waiting. The
/// cost is that we have to spawn more value calculating threads and that the
/// desired concurrency of the cache may not match that of the caller.
pub struct AsyncLru<RT: Runtime, Key, Value: ?Sized, FetchKey = Key> {
    runtime: RT,
    inner: Arc<Mutex<Inner<RT, Key, Value, FetchKey>>>,
    label: &'static str,
    handle: Arc<Box<dyn SpawnHandle>>,
}

impl<RT: Runtime, Key, Value: ?Sized, FetchKey> Clone for AsyncLru<RT, Key, Value, FetchKey> {
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
            inner: self.inner.clone(),
            label: self.label,
            handle: self.handle.clone(),
        }
    }
}
struct CacheEntry<Value: ?Sized> {
    value: Arc<Value>,
    // Memoize the size to guard against implementations of `SizedValue`
    // that (unexpectedly) change while the value is in the cache.
    size: u64,
    added: tokio::time::Instant,
}

impl<Value: SizedValue + ?Sized> SizedValue for CacheEntry<Value> {
    fn size(&self) -> u64 {
        self.size
    }
}

struct Inner<RT: Runtime, Key, Value: ?Sized, FetchKey> {
    cache: LruCache<Key, CacheEntry<Value>>,
    current_size: u64,
    max_size: u64,
    label: &'static str,
    tx: CoDelQueueSender<RT, BuildValueRequest<Key, Value, FetchKey>>,
    in_progress: HashMap<FetchKey, BroadcastReceiver<BuildValueResult<Key, Value>>>,
}

impl<RT: Runtime, Key, Value: ?Sized, FetchKey> Inner<RT, Key, Value, FetchKey> {
    fn new(
        cache: LruCache<Key, CacheEntry<Value>>,
        max_size: u64,
        label: &'static str,
        tx: CoDelQueueSender<RT, BuildValueRequest<Key, Value, FetchKey>>,
    ) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            cache,
            current_size: 0,
            max_size,
            label,
            tx,
            in_progress: HashMap::new(),
        }))
    }
}

pub trait SizedValue {
    fn size(&self) -> u64;
}

/// Wrapper struct when you're inserting values into the LRU that should always
/// be considered to be 1 unit in size.
pub struct UnitSizedValue<T>(pub T);

impl<T> SizedValue for UnitSizedValue<T> {
    fn size(&self) -> u64 {
        1
    }
}

impl<Value: SizedValue> SizedValue for Arc<Value> {
    fn size(&self) -> u64 {
        Value::size(self)
    }
}

// TableMapping and BTreeMap<TabletId, IndexRef> don't vary much in size within
// a deployment, so it's easier to think about the caches as having a number of
// items, instead of considering the number of bytes cached.
impl SizedValue for TableMapping {
    fn size(&self) -> u64 {
        1
    }
}
impl SizedValue for BTreeMap<TabletId, IndexRef> {
    fn size(&self) -> u64 {
        1
    }
}
impl SizedValue for BTreeMap<ComponentId, ComponentPath> {
    fn size(&self) -> u64 {
        1
    }
}

type BuildValueResult<Key, Value> = Result<Arc<HashMap<Key, Arc<Value>>>, Arc<anyhow::Error>>;

type BuildValueRequest<Key, Value, FetchKey> = (
    FetchKey,
    InSpan<BoxFuture<'static, anyhow::Result<HashMap<Key, Arc<Value>>>>>,
    async_broadcast::Sender<BuildValueResult<Key, Value>>,
);

enum Status<Key, Value: ?Sized> {
    Ready(Arc<Value>),
    Waiting(async_broadcast::Receiver<BuildValueResult<Key, Value>>),
    Kickoff(anyhow::Result<async_broadcast::Receiver<BuildValueResult<Key, Value>>>),
}

impl<Key, Value: ?Sized> std::fmt::Display for Status<Key, Value> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Ready(_) => write!(f, "Ready"),
            Status::Waiting(_) => write!(f, "Waiting"),
            Status::Kickoff(..) => write!(f, "Kickoff"),
        }
    }
}

impl<
        RT: Runtime,
        Key: Hash + Eq + Debug + Clone + Send + Sync + 'static,
        Value: Send + Sync + 'static + SizedValue + ?Sized,
        FetchKey: Hash + Eq + Debug + Clone + Send + Sync + 'static,
    > AsyncLru<RT, Key, Value, FetchKey>
{
    /// Create a new fixed size LRU where the maximum size is determined by
    /// `max_size` and the size of each entry is determined by the
    /// implementation of `SizedValue` for the corresponding value.
    ///
    /// label - a string for logging to differentiate between LRUs.
    /// max_size - the maximum number of Values that will be kept in memory by
    /// the LRU. Must be > 0, or we will panic.
    /// generate_value - a function that generates a new value for a given Key
    /// if no value is present.
    /// concurrency - The number of values that can be concurrently generated.
    /// This should be set based on system values.
    /// queue_size - The size of the CoDel queue used to buffer pending value
    /// generation requests.
    pub fn new(
        rt: RT,
        max_size: u64,
        concurrency: usize,
        queue_size: usize,
        label: &'static str,
    ) -> Self {
        Self::_new(
            CoDelQueue::new_with_defaults(rt.clone(), queue_size),
            rt,
            LruCache::unbounded(),
            max_size,
            concurrency,
            label,
        )
    }

    fn _new(
        queue: CoDelQueue<RT, BuildValueRequest<Key, Value, FetchKey>>,
        rt: RT,
        cache: LruCache<Key, CacheEntry<Value>>,
        max_size: u64,
        concurrency: usize,
        label: &'static str,
    ) -> Self {
        let (tx, rx) = queue.into_sender_and_receiver();
        let inner = Inner::new(cache, max_size, label, tx);
        let handle = rt.spawn(
            label,
            Self::value_generating_worker_thread(rt.clone(), rx, inner.clone(), concurrency, label),
        );
        Self {
            runtime: rt.clone(),
            inner,
            label,
            handle: Arc::new(handle),
        }
    }

    fn drop_waiting(inner: &mut Inner<RT, Key, Value, FetchKey>, key: &FetchKey) {
        inner.in_progress.remove(key);
    }

    fn update_value(
        rt: &RT,
        inner: &Arc<Mutex<Inner<RT, Key, Value, FetchKey>>>,
        key: Key,
        result: &Arc<Value>,
    ) {
        let mut inner = inner.lock();
        let new_value = CacheEntry {
            size: result.size(),
            value: result.clone(),
            added: rt.monotonic_now(),
        };
        inner.current_size += new_value.size();
        if let Some(old_value) = inner.cache.put(key, new_value) {
            // Allow overwriting entries which may have been populated
            // by racing requests with prefetches.
            inner.current_size -= old_value.size();
        }
        Self::trim_to_size(&mut inner);
    }

    fn trim_to_size(inner: &mut Inner<RT, Key, Value, FetchKey>) {
        while inner.current_size > inner.max_size {
            let (_, evicted) = inner
                .cache
                .pop_lru()
                .expect("Over max size, but no more entries");
            // This isn't catastrophic necessarily, but it may lead to
            // under / over counting of the cache's size.
            let CacheEntry { value, size, added } = evicted;
            if size != value.size() {
                tracing::warn!(
                    "Value changed size from {} to {} while in the {} cache!",
                    size,
                    value.size(),
                    inner.label
                )
            }
            async_lru_log_eviction(inner.label, added.elapsed());
            inner.current_size -= size;
        }
    }

    pub fn size(&self) -> u64 {
        let inner = self.inner.lock();
        inner.current_size
    }

    /// Get `key`. If it is not present, run `value_generator` and cache every
    /// key/value pair it returns.
    ///
    /// Concurrent fetches are deduplicated by `fetch_key`: if a fetch with the
    /// same `fetch_key` is already in flight, this call waits for its result
    /// instead of running `value_generator`. Callers must therefore ensure
    /// that any generator passed with a given `fetch_key` produces a map
    /// containing every `key` that may be requested alongside that
    /// `fetch_key`; otherwise the deduplicated calls will fail.
    pub async fn get_and_prepopulate<F, Q>(
        &self,
        key: &Q,
        value_generator: impl FnOnce() -> (FetchKey, F),
    ) -> anyhow::Result<Arc<Value>>
    where
        F: Future<Output = anyhow::Result<HashMap<Key, Arc<Value>>>> + Send + 'static,
        Q: Hash + Eq + Debug + ?Sized,
        Key: Borrow<Q>,
    {
        let timer = async_lru_get_timer(self.label);
        let result = self
            ._get(key, || {
                let (fetch_key, f) = value_generator();
                (fetch_key, f.boxed())
            })
            .await;
        timer.finish(result.is_ok());
        result
    }

    pub async fn get<V: 'static, F>(
        &self,
        key: &Key,
        value_generator: impl FnOnce() -> F,
    ) -> anyhow::Result<Arc<Value>>
    where
        Key: Clone,
        Arc<Value>: From<V>,
        FetchKey: From<Key>,
        F: Future<Output = anyhow::Result<V>> + Send + 'static,
    {
        let timer = async_lru_get_timer(self.label);
        let result = self
            ._get(key, || {
                let key = key.clone();
                (
                    key.clone().into(),
                    value_generator()
                        .map(|value| {
                            let mut hashmap = HashMap::new();
                            hashmap.insert(key, <Arc<Value>>::from(value?));
                            Ok(hashmap)
                        })
                        .boxed(),
                )
            })
            .await;
        timer.finish(result.is_ok());
        result
    }

    async fn _get<Q>(
        &self,
        key: &Q,
        value_generator: impl FnOnce() -> (
            FetchKey,
            BoxFuture<'static, anyhow::Result<HashMap<Key, Arc<Value>>>>,
        ),
    ) -> anyhow::Result<Arc<Value>>
    where
        Q: Hash + Eq + Debug + ?Sized,
        Key: Borrow<Q>,
    {
        let pause_client = self.runtime.pause_client();
        let status = self.get_sync(key, value_generator);
        tracing::debug!("Getting key {key:?} with status {status}");
        match status {
            Status::Ready(value) => {
                log_async_lru_cache_hit(self.label);
                Ok(value)
            },
            Status::Waiting(rx) => {
                log_async_lru_cache_waiting(self.label);
                Ok(Self::wait_for_value(key, rx).await?)
            },
            Status::Kickoff(rx) => {
                log_async_lru_cache_miss(self.label);
                let rx = rx?;
                let timer = async_lru_compute_timer(self.label);
                pause_client.wait(PAUSE_DURING_GENERATE_VALUE_LABEL).await;
                let result = Self::wait_for_value(key, rx).await?;
                timer.finish();
                Ok(result)
            },
        }
    }

    fn get_sync<Q>(
        &self,
        key: &Q,
        value_generator: impl FnOnce() -> (
            FetchKey,
            BoxFuture<'static, anyhow::Result<HashMap<Key, Arc<Value>>>>,
        ),
    ) -> Status<Key, Value>
    where
        Q: Hash + Eq + Debug + ?Sized,
        Key: Borrow<Q>,
    {
        if let Some(CacheEntry { value, .. }) = self.inner.lock().cache.get(key) {
            return Status::Ready(value.clone());
        }
        // Create the value generator outside of the lock
        let (fetch_key, future) = value_generator();
        let mut inner = self.inner.lock();
        let inner = &mut *inner;
        if let Some(CacheEntry { value, .. }) = inner.cache.get(key) {
            return Status::Ready(value.clone());
        }
        match inner.in_progress.entry(fetch_key) {
            Entry::Occupied(waiting) => Status::Waiting(waiting.get().clone()),
            Entry::Vacant(v) => {
                // Run the value_generator in the span context of the original client that
                // fired off the job. If multiple callers instantiate the same job, only the
                // first one will execute the future and get the sub-spans.
                let span = SpanContext::current_local_parent()
                    .map(|ctx| Span::root("async_lru_compute_value", ctx))
                    .unwrap_or(Span::noop());

                let (tx, rx) = async_broadcast::broadcast(1);
                Status::Kickoff(
                    match inner
                        .tx
                        .clone()
                        .try_send((v.key().clone(), future.in_span(span), tx))
                    {
                        Ok(()) => {
                            v.insert(rx.clone());
                            Ok(rx)
                        },
                        // If the queue is too full, just bail here. The cache state is unmodified
                        // and there can't be any other waiters for this key
                        // right now, so it should be safe to abort.
                        Err(e) => Err(e.into()),
                    },
                )
            },
        }
    }

    #[fastrace::trace]
    async fn wait_for_value<Q>(
        key: &Q,
        mut receiver: async_broadcast::Receiver<BuildValueResult<Key, Value>>,
    ) -> anyhow::Result<Arc<Value>>
    where
        Q: Hash + Eq + Debug + ?Sized,
        Key: Borrow<Q>,
    {
        // No work should be canceled while anyone is waiting on it, so it's a
        // developer error if recv ever returns a failure due to the channel
        // being closed.
        let recv_result = receiver.recv().await?;
        match recv_result {
            Ok(value) => {
                tracing::debug!("Finished waiting on another task to fetch key {key:?}");
                value
                    .get(key)
                    .context("Value generator did not produce requested key")
                    .cloned()
            },
            // We recapture the error in the string so that we don't lose the stacktrace since the
            // original stacktrace is stuck inside an Arc<anyhow::Error>
            Err(e) => Err(recapture_stacktrace_noreport(&e)),
        }
    }

    async fn value_generating_worker_thread(
        rt: RT,
        rx: CoDelQueueReceiver<RT, BuildValueRequest<Key, Value, FetchKey>>,
        inner: Arc<Mutex<Inner<RT, Key, Value, FetchKey>>>,
        concurrency: usize,
        label: &'static str,
    ) {
        rx.for_each_concurrent(concurrency, |((fetch_key, generator, tx), expired)| {
            let inner = inner.clone();
            let rt = rt.clone();
            async move {
                if let Some(expired) = expired {
                    Self::drop_waiting(&mut *inner.lock(), &fetch_key);
                    _ = tx.try_broadcast(Err(Arc::new(anyhow::anyhow!(expired))));
                    return;
                }

                match generator.await {
                    Ok(values) => {
                        for (k, value) in &values {
                            Self::update_value(&rt, &inner, k.clone(), value);
                        }
                        _ = tx.try_broadcast(Ok(values.into()));
                    },
                    Err(e) => {
                        _ = tx.try_broadcast(Err(Arc::new(e)));
                    },
                }
                let mut inner = inner.lock();
                Self::drop_waiting(&mut *inner, &fetch_key);
                log_async_lru_size(inner.cache.len(), inner.current_size, label);
            }
        })
        .await;
        tracing::warn!("Worker shut down, shutting down scheduler");
    }
}
