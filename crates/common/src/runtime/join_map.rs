#![allow(clippy::disallowed_types)]

use std::{
    fmt::{
        self,
        Debug,
    },
    future::Future,
    hash::{
        BuildHasher,
        Hash,
        RandomState,
    },
};

use fastrace::{
    future::FutureExt as _,
    Span,
};
use tokio::task::JoinError;

use super::{
    propagate_tracing,
    GLOBAL_TASK_MANAGER,
};

/// A wrapper around [`tokio_util::task::JoinMap`] that participates in runtime
/// usage instrumentation, much like [`crate::runtime::tokio_spawn`]
pub struct JoinMap<K, V, S = RandomState> {
    inner: tokio_util::task::JoinMap<K, V, S>,
}

impl<K, V> JoinMap<K, V> {
    pub fn new() -> Self {
        Self {
            inner: tokio_util::task::JoinMap::new(),
        }
    }
}

impl<K, V, S: Clone> JoinMap<K, V, S> {
    pub fn with_hasher(hash_builder: S) -> Self {
        Self {
            inner: tokio_util::task::JoinMap::with_hasher(hash_builder),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

impl<K, V, S> JoinMap<K, V, S>
where
    K: Hash + Eq,
    V: 'static,
    S: BuildHasher,
{
    #[track_caller]
    pub fn spawn<F>(&mut self, name: &'static str, key: K, task: F)
    where
        F: Future<Output = V>,
        F: Send + 'static,
        V: Send,
    {
        let monitor = GLOBAL_TASK_MANAGER.lock().get(name);
        self.inner.spawn(
            key,
            propagate_tracing(
                monitor.instrument(task.in_span(Span::enter_with_local_parent(name))),
            ),
        )
    }

    pub async fn join_next(&mut self) -> Option<(K, Result<V, JoinError>)> {
        self.inner.join_next().await
    }

    pub fn abort_matching(&mut self, predicate: impl FnMut(&K) -> bool) {
        self.inner.abort_matching(predicate)
    }
}

impl<K: Debug, V, S> Debug for JoinMap<K, V, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.inner, f)
    }
}
