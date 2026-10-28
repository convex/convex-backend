use std::hash::Hash;

use moka::ops::compute::{
    CompResult,
    Op,
};

/// Wraps [`moka::sync::Cache`] to enforce that all mutations go through
/// [`and_compute_with`], which holds moka's per-key WAITER lock for the
/// entire operation. Note that insert/invalidate/remove do not use the same
/// lock. Only exposing `compute` and `remove` prevents `invalidate` or
/// `insert` from bypassing that lock and racing with an in-progress
/// `and_compute_with` closure.
#[derive(Clone)]
pub struct AtomicCache<K, V>(moka::sync::Cache<K, V>);

impl<K, V> AtomicCache<K, V>
where
    K: Clone + Hash + Eq + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    pub fn new(cache: moka::sync::Cache<K, V>) -> Self {
        Self(cache)
    }

    pub fn get(&self, key: &K) -> Option<V> {
        self.0.get(key)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }

    pub fn weighted_size(&self) -> u64 {
        self.0.weighted_size()
    }

    pub fn compute(
        &self,
        key: K,
        f: impl FnOnce(Option<moka::Entry<K, V>>) -> Op<V>,
    ) -> CompResult<K, V> {
        self.0.entry(key).and_compute_with(f)
    }

    /// N.B. This must call `compute` instead of moka's native `remove` or
    /// `invalidate` because those fns do not use the same lock as `compute` and
    /// we need these operations to be atomic.
    pub fn remove(&self, key: K) -> CompResult<K, V> {
        self.compute(key, |_| Op::Remove)
    }
}
