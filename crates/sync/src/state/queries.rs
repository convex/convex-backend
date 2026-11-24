use std::{
    collections::BTreeMap,
    mem,
    sync::Arc,
};

use application::api::SubscriptionTrait;
use futures::future::AbortHandle;
use keybroker::Identity;
use sync_types::{
    Query,
    QueryId,
};

use super::{
    ErrorDigest,
    ValueDigest,
};

pub struct SyncedQuery {
    pub(super) query: Query,

    /// What is the active subscription for the given query?
    ///
    /// - Starts `None`: Query is newly inserted.
    /// - `None -> Some(subscription)`: `SyncState::complete_fetch`.
    /// - `Some(..) -> None`: `SyncedQueries::take_subscriptions`.
    subscription: Option<Arc<dyn SubscriptionTrait>>,

    /// What was the hash of the last successful return value? This allows us to
    /// deduplicate transitions for queries whose results haven't actually
    /// changed.
    ///
    /// - Starts `None`: Query is newly inserted.
    /// - `None -> Some(r)`: `SyncState::complete_fetch`, the first time the
    ///   query is executed.
    /// - `Some(..) -> Some(..)`: `SyncState::complete_fetch`, after the first
    ///   time.
    pub(super) result_hash: Option<Result<ValueDigest, ErrorDigest>>,

    /// Handle to the query's current invalidation future. This future completes
    /// when `self.subscription` is no longer valid and the query should be
    /// rerun.
    pub(super) invalidation_future: Option<AbortHandle>,
}

impl SyncedQuery {
    pub fn subscription(&self) -> Option<&Arc<dyn SubscriptionTrait>> {
        self.subscription.as_ref()
    }

    pub fn set_subscription(&mut self, subscription: Arc<dyn SubscriptionTrait>) {
        self.subscription = Some(subscription);
    }
}

#[derive(Default)]
pub struct SyncedQueries {
    by_id: BTreeMap<QueryId, SyncedQuery>,
    /// The identity passed to the latest `take_subscriptions`. Every
    /// subscription came from the transition that took them under it.
    identity: Option<Identity>,
}

pub struct SubscriptionsForIdentity {
    pub subscriptions: BTreeMap<QueryId, Arc<dyn SubscriptionTrait>>,
    /// Whether the subscriptions' results came from an identity that is
    /// `equivalent_identities` to the one they were taken under.
    pub equivalent_identity: bool,
}

impl SyncedQueries {
    pub fn insert(&mut self, query: Query) -> anyhow::Result<()> {
        let query_id = query.query_id;
        let synced_query = SyncedQuery {
            query,
            subscription: None,
            result_hash: None,
            invalidation_future: None,
        };
        if self.by_id.insert(query_id, synced_query).is_some() {
            anyhow::bail!("Duplicate query ID: {}", query_id);
        }
        Ok(())
    }

    pub fn get_mut(&mut self, query_id: &QueryId) -> Option<&mut SyncedQuery> {
        self.by_id.get_mut(query_id)
    }

    pub fn remove(&mut self, query_id: &QueryId) -> Option<SyncedQuery> {
        self.by_id.remove(query_id)
    }

    pub fn values(&self) -> impl Iterator<Item = &SyncedQuery> {
        self.by_id.values()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (QueryId, &mut SyncedQuery)> {
        self.by_id
            .iter_mut()
            .map(|(query_id, query)| (*query_id, query))
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn take_subscriptions(&mut self, identity: &Identity) -> SubscriptionsForIdentity {
        let previous = self.identity.replace(identity.clone());
        let equivalent_identity =
            previous.is_some_and(|previous| equivalent_identities(&previous, identity));
        let mut subscriptions = BTreeMap::new();
        for (query_id, query) in self.by_id.iter_mut() {
            if let Some(subscription) = query.subscription.take() {
                subscriptions.insert(*query_id, subscription);
            }
            if let Some(handle) = query.invalidation_future.take() {
                handle.abort();
            }
        }
        SubscriptionsForIdentity {
            subscriptions,
            equivalent_identity,
        }
    }
}

/// Whether identity `a` is equivalent to identity `b` for the purpose of
/// running a query.
///
/// It judges that by the cache_key (which captures everything identity related
/// that's important for queries) and compares the `Identity` discriminant since
/// the cache_key for a user and an admin acting as a user are equivalent but
/// the identities are truly different.
fn equivalent_identities(a: &Identity, b: &Identity) -> bool {
    mem::discriminant(a) == mem::discriminant(b) && a.cache_key() == b.cache_key()
}
