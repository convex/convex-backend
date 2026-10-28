//! An AsyncLru that can store multiple types of data at the same time, while
//! still offering type safety.

use std::{
    any::Any,
    collections::HashMap,
    fmt::Debug,
    hash::{
        DefaultHasher,
        Hash,
        Hasher,
    },
    sync::Arc,
};

use anyhow::Context as _;
use common::runtime::Runtime;
use futures::FutureExt as _;

use crate::async_lru::{
    AsyncLru,
    SizedValue,
};

pub trait LruKey: Any + Hash + Eq + Clone + Send + Sync + Debug {
    type Value: SizedValue + Send + Sync;
}

trait BaseLruKey: Any + Send + Sync + Debug {
    fn eq(&self, other: &dyn BaseLruKey) -> bool;
    fn key_hash(&self) -> u64;
    fn box_clone(&self) -> GenericKey;
}

impl<T: LruKey> BaseLruKey for T {
    fn eq(&self, other: &dyn BaseLruKey) -> bool {
        <dyn Any>::downcast_ref::<Self>(other).is_some_and(|other| self == other)
    }

    fn key_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.type_id().hash(&mut hasher);
        self.hash(&mut hasher);
        hasher.finish()
    }

    fn box_clone(&self) -> GenericKey {
        Box::new(self.clone())
    }
}

type GenericKey = Box<dyn BaseLruKey>;

impl Hash for dyn BaseLruKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key_hash().hash(state)
    }
}

impl PartialEq for dyn BaseLruKey {
    fn eq(&self, other: &Self) -> bool {
        BaseLruKey::eq(self, other)
    }
}

impl Eq for dyn BaseLruKey {}

impl Clone for GenericKey {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

trait BaseLruValue: Any + SizedValue + Send + Sync {}
impl<T: Any + SizedValue + Send + Sync> BaseLruValue for T {}

#[derive(Clone)]
pub struct MultiTypeAsyncLru<RT: Runtime> {
    inner: AsyncLru<RT, GenericKey, dyn BaseLruValue>,
}

impl<RT: Runtime> MultiTypeAsyncLru<RT> {
    pub fn new(
        rt: RT,
        max_size: u64,
        concurrency: usize,
        queue_size: usize,
        label: &'static str,
    ) -> Self {
        Self {
            inner: AsyncLru::new(rt, max_size, concurrency, queue_size, label),
        }
    }

    pub fn size(&self) -> u64 {
        self.inner.size()
    }

    pub async fn get<Key: LruKey + Clone, V: 'static, F>(
        &self,
        key: &Key,
        value_generator: impl FnOnce() -> F,
    ) -> anyhow::Result<Arc<Key::Value>>
    where
        Arc<Key::Value>: From<V>,
        F: Future<Output = anyhow::Result<V>> + Send + 'static,
    {
        let result = self
            .inner
            .get_and_prepopulate(key as &dyn BaseLruKey, || {
                let key = key.clone();
                (
                    Box::new(key.clone()),
                    value_generator().map(|value| {
                        let mut hashmap = HashMap::new();
                        hashmap.insert(
                            Box::new(key) as GenericKey,
                            <Arc<Key::Value>>::from(value?) as Arc<dyn BaseLruValue>,
                        );
                        Ok(hashmap)
                    }),
                )
            })
            .await?;
        (result as Arc<dyn Any + Send + Sync>)
            .downcast()
            .ok()
            .context("MultiTypeAsyncLru error: cached value was wrong type")
    }
}
