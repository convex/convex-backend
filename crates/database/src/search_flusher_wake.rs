use std::{
    collections::BTreeSet,
    sync::Arc,
};

use common::types::IndexId;
use parking_lot::Mutex;
use search::metrics::SearchType;
use tokio::sync::watch;

/// Wakes the live search index flushers when an in-memory index grows past its
/// soft size limit. Without this the flushers only notice on their polling
/// interval, which leaves the index growing towards the hard limit (where
/// writes start failing) for up to `DATABASE_WORKERS_POLL_INTERVAL`.
#[derive(Clone)]
pub struct SearchFlusherWakeSignals {
    text: Signal,
    vector: Signal,
}

impl SearchFlusherWakeSignals {
    pub(crate) fn new() -> Self {
        Self {
            text: Signal::new(),
            vector: Signal::new(),
        }
    }

    fn signal(&self, search_type: SearchType) -> &Signal {
        match search_type {
            SearchType::Text => &self.text,
            SearchType::Vector => &self.vector,
        }
    }

    /// Wakes the flusher for `search_type` if any index has newly grown past
    /// `soft_limit`. Indexes that were already over it don't signal again, so a
    /// sustained overage doesn't wake the flusher on every commit.
    pub(crate) fn update_index_sizes(
        &self,
        search_type: SearchType,
        sizes: impl Iterator<Item = (IndexId, usize)>,
        soft_limit: usize,
    ) {
        let over_soft_limit: BTreeSet<IndexId> = sizes
            .filter(|(_, size)| *size > soft_limit)
            .map(|(index_id, _)| index_id)
            .collect();
        let signal = self.signal(search_type);
        let newly_over_soft_limit = {
            let mut previously_over_soft_limit = signal.over_soft_limit.lock();
            let newly_over = over_soft_limit
                .difference(&previously_over_soft_limit)
                .next()
                .is_some();
            *previously_over_soft_limit = over_soft_limit;
            newly_over
        };
        if newly_over_soft_limit {
            signal.sender.send_modify(|generation| *generation += 1);
        }
    }

    pub fn subscribe(&self, search_type: SearchType) -> SearchFlusherWakeSubscriber {
        SearchFlusherWakeSubscriber(self.signal(search_type).sender.subscribe())
    }
}

#[derive(Clone)]
struct Signal {
    /// Bumped each time an index newly exceeds the soft limit.
    sender: watch::Sender<u64>,
    /// Indexes that were over the soft limit as of the last commit.
    over_soft_limit: Arc<Mutex<BTreeSet<IndexId>>>,
}

impl Signal {
    fn new() -> Self {
        let (sender, _receiver) = watch::channel(0);
        Self {
            sender,
            over_soft_limit: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }
}

pub struct SearchFlusherWakeSubscriber(watch::Receiver<u64>);

impl SearchFlusherWakeSubscriber {
    /// Resolves once an index has exceeded the soft limit since the last call.
    pub async fn wait_for_wake(&mut self) {
        if self.0.changed().await.is_err() {
            // The sender lives as long as the `Database`, so this only happens
            // during shutdown, when the worker is about to be dropped.
            std::future::pending().await
        }
    }
}
