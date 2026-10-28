use std::{
    collections::BTreeMap,
    sync::Arc,
};

use anyhow::Context;
use async_lru::{
    async_lru::SizedValue,
    multi_type_async_lru::{
        LruKey,
        MultiTypeAsyncLru,
    },
};
use async_trait::async_trait;
use common::{
    document::{
        CreationTime,
        PackedDocument,
        ParseDocument,
    },
    index::IndexKeyBytes,
    interval::Interval,
    knobs::{
        FUNRUN_INDEX_CACHE_CONCURRENCY,
        FUNRUN_INDEX_CACHE_QUEUE_SIZE,
        FUNRUN_INDEX_CACHE_SIZE,
    },
    query::Order,
    runtime::Runtime,
    types::{
        IndexId,
        IndexRef,
        RepeatableTimestamp,
    },
    virtual_system_mapping::VirtualSystemMapping,
};
use database::{
    BootstrapMetadata,
    ComponentRegistry,
    DatabaseSnapshot,
    SchemaRegistry,
    TableCountSnapshot,
    TableRegistry,
    Transaction,
    TransactionIdGenerator,
    TransactionIndex,
    TransactionReadSet,
    TransactionTextSnapshot,
    COMPONENTS_TABLE,
    SCHEMAS_TABLE,
};
use futures::TryStreamExt;
use indexing::{
    database_index_snapshot::DatabaseIndexSnapshot,
    in_memory_indexes::{
        InMemoryIndexes,
        MemoryDocument,
    },
    index_reader::IndexReader,
    index_registry::IndexRegistry,
};
use keybroker::Identity;
use model::virtual_system_mapping;
use sync_types::Timestamp;
use usage_tracking::FunctionUsageTracker;
use value::{
    heap_size::HeapSize,
    TableName,
    TableNamespace,
    TabletId,
};

use super::metrics::{
    load_index_timer,
    log_funrun_index_cache_get,
    log_funrun_index_load_rows,
};
use crate::{
    metrics::begin_tx_timer,
    FunctionWrites,
};

fn make_transaction<RT: Runtime>(
    ts: RepeatableTimestamp,
    identity: Identity,
    rt: RT,
    table_registry: TableRegistry,
    schema_registry: SchemaRegistry,
    component_registry: ComponentRegistry,
    index_registry: IndexRegistry,
    table_count_snapshot: Arc<dyn TableCountSnapshot>,
    database_index_snapshot: DatabaseIndexSnapshot,
    text_index_snapshot: Arc<dyn TransactionTextSnapshot>,
    virtual_system_mapping: VirtualSystemMapping,
    usage_tracker: FunctionUsageTracker,
) -> anyhow::Result<Transaction<RT>> {
    let id_generator = TransactionIdGenerator::new(&rt)?;
    // The transaction timestamp might be few minutes behind if the backend
    // has been idle. Make sure creation time is always recent.
    let creation_time = CreationTime::for_transaction(*ts, rt.generate_timestamp()?)?;
    let transaction_index =
        TransactionIndex::new(index_registry, database_index_snapshot, text_index_snapshot);
    Ok(Transaction::new(
        identity,
        id_generator,
        creation_time,
        transaction_index,
        table_registry,
        schema_registry,
        component_registry,
        table_count_snapshot,
        rt,
        usage_tracker,
        virtual_system_mapping,
    ))
}

#[derive(Hash, PartialEq, Eq, Clone, Debug)]
struct IndexCacheKey {
    deployment_name: String,
    index_id: IndexId,
    last_modified: Timestamp,
}

impl LruKey for IndexCacheKey {
    type Value = IndexCacheValue;
}

/// The cache value is the same as [DatabaseIndexMap] apart from keeping track
/// of last modified timestamps. The [BTreeMap] keys are the index keys.
struct IndexCacheValue {
    map: BTreeMap<Vec<u8>, (Timestamp, MemoryDocument)>,
    size: usize,
}

impl SizedValue for IndexCacheValue {
    fn size(&self) -> u64 {
        self.size as u64
    }
}

/// Caches in-memory indexes for many instances.
/// This object is cheaply cloneable and clones reference the same underlying
/// cache.
#[derive(Clone)]
pub(crate) struct InMemoryIndexCache<RT: Runtime> {
    cache: MultiTypeAsyncLru<RT>,
    rt: RT,
}

#[fastrace::trace]
async fn load_index(
    index: IndexRef,
    index_reader: Arc<dyn IndexReader>,
    tablet_id: TabletId,
    table_name: String,
) -> anyhow::Result<Arc<IndexCacheValue>> {
    let _timer = load_index_timer(&table_name);
    let mut size = 0;
    let index_map: BTreeMap<Vec<u8>, _> = index_reader
        .index_scan(index, tablet_id, Interval::all(), Order::Asc, usize::MAX)
        .map_ok(|entry| {
            let doc = entry.value;
            // This doesn't take into account the future size of the cached
            // SystemDocument, so the cache will use more memory than its
            // nominal size
            size += entry.key.heap_size() + doc.heap_size();
            (entry.key.0, (entry.ts, MemoryDocument::new(doc)))
        })
        .try_collect()
        .await?;
    log_funrun_index_load_rows(index_map.len() as u64, &table_name);
    Ok(Arc::new(IndexCacheValue {
        map: index_map,
        size,
    }))
}

#[fastrace::trace]
async fn load_unpacked_index(
    index: IndexRef,
    index_reader: &Arc<dyn IndexReader>,
    tablet_id: TabletId,
    table_name: &str,
) -> anyhow::Result<(Vec<PackedDocument>, u64)> {
    let _timer = load_index_timer(table_name);
    let documents: Vec<PackedDocument> = index_reader
        .index_scan(index, tablet_id, Interval::all(), Order::Asc, usize::MAX)
        .map_ok(|entry| entry.value)
        .try_collect()
        .await?;
    log_funrun_index_load_rows(documents.len() as u64, table_name);
    let size = documents.iter().map(|d| d.size() as u64).sum();
    Ok((documents, size))
}

struct WithSize<T>(T, u64);
impl<T> SizedValue for WithSize<T> {
    fn size(&self) -> u64 {
        self.1
    }
}

impl<RT: Runtime> InMemoryIndexCache<RT> {
    pub fn new(rt: RT) -> Self {
        Self {
            cache: MultiTypeAsyncLru::new(
                rt.clone(),
                *FUNRUN_INDEX_CACHE_SIZE,
                *FUNRUN_INDEX_CACHE_CONCURRENCY,
                *FUNRUN_INDEX_CACHE_QUEUE_SIZE,
                "funrun_index_cache",
            ),
            rt,
        }
    }
}

impl<RT: Runtime> FunctionRunnerInMemoryIndexes<RT> {
    /// Get the index from the cache or load it from persistence and put it in
    /// the cache. If the index is not in the last_modified map, it is not an
    /// in-memory index and should not be cached.
    #[fastrace::trace]
    async fn get_or_load(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        table_name: TableName,
    ) -> anyhow::Result<Option<Arc<IndexCacheValue>>> {
        let Some(key) = self
            .backend_last_modified
            .get(&index.id())
            .map(|ts| IndexCacheKey {
                deployment_name: self.deployment_name.clone(),
                index_id: index.id(),
                last_modified: *ts,
            })
        else {
            return Ok(None);
        };
        let cache_value_result = self
            .cache
            .get(&key, || {
                load_index(
                    index,
                    self.index_reader.clone(),
                    tablet_id,
                    table_name.to_string(),
                )
            })
            .await
            .map(Some);
        log_funrun_index_cache_get(&table_name);
        cache_value_result
    }

    /// Returns the TableRegistry together with its last-modified time
    #[fastrace::trace]
    async fn load_table_registry(
        &self,
        &BootstrapMetadata {
            tables_by_id,
            tables_tablet_id,
            ..
        }: &BootstrapMetadata,
    ) -> anyhow::Result<(Timestamp, TableRegistry)> {
        #[derive(Hash, PartialEq, Eq, Debug, Clone)]
        struct Key {
            deployment_name: String,
            tables_last_modified: Timestamp,
        }
        impl LruKey for Key {
            type Value = WithSize<TableRegistry>;
        }
        let tables_last_modified = *self
            .backend_last_modified
            .get(&tables_by_id.id())
            .context("_tables not configured to be in-memory")?;
        const NAME: &str = "_table_registry";
        log_funrun_index_cache_get(NAME);
        let index_reader = self.index_reader.clone();
        let table_registry = self
            .cache
            .get(
                &Key {
                    deployment_name: self.deployment_name.clone(),
                    tables_last_modified,
                },
                || async move {
                    let (documents, size) =
                        load_unpacked_index(tables_by_id, &index_reader, tables_tablet_id, NAME)
                            .await?;
                    let (table_mapping, table_states) =
                        DatabaseSnapshot::<RT>::table_mapping_and_states(
                            documents.into_iter().map(|doc| doc.parse()).try_collect()?,
                        );
                    let registry = TableRegistry::bootstrap(table_mapping, table_states)?;
                    // We don't have `HeapSize` implemented for `TableRegistry`
                    // so just approximate its size using the size of the
                    // documents it was made from.
                    Ok(WithSize(registry, size))
                },
            )
            .await?;
        Ok((tables_last_modified, table_registry.0.clone()))
    }

    #[fastrace::trace]
    async fn load_index_registry(
        &self,
        table_registry: (Timestamp, TableRegistry),
        &BootstrapMetadata {
            index_by_id,
            index_tablet_id,
            ..
        }: &BootstrapMetadata,
    ) -> anyhow::Result<(Timestamp, IndexRegistry)> {
        #[derive(Hash, PartialEq, Eq, Debug, Clone)]
        struct Key {
            deployment_name: String,
            last_modified: Timestamp,
        }
        impl LruKey for Key {
            type Value = WithSize<IndexRegistry>;
        }
        let indexes_last_modified = *self
            .backend_last_modified
            .get(&index_by_id.id())
            .context("_index not configured to be in-memory")?;
        const NAME: &str = "_index_registry";
        log_funrun_index_cache_get(NAME);
        let index_reader = self.index_reader.clone();

        let index_registry = self
            .cache
            .get(
                &Key {
                    deployment_name: self.deployment_name.clone(),
                    // We use the max of the two timestamps as our cache key
                    // because it's "as if" `load_unpacked_index` is reading at
                    // that timestamp.
                    last_modified: table_registry.0.max(indexes_last_modified),
                },
                || async move {
                    let (documents, size) =
                        load_unpacked_index(index_by_id, &index_reader, index_tablet_id, NAME)
                            .await?;
                    let index_registry = IndexRegistry::bootstrap(
                        table_registry.1.table_mapping(),
                        documents.into_iter(),
                    )?;
                    DatabaseSnapshot::<RT>::verify_invariants(&table_registry.1, &index_registry)?;
                    Ok(WithSize(index_registry, size))
                },
            )
            .await?;
        Ok((indexes_last_modified, index_registry.0.clone()))
    }

    #[fastrace::trace]
    async fn load_component_registry(
        &self,
        table_registry: (Timestamp, TableRegistry),
        index_registry: (Timestamp, IndexRegistry),
    ) -> anyhow::Result<(Timestamp, ComponentRegistry)> {
        #[derive(Hash, PartialEq, Eq, Debug, Clone)]
        struct Key {
            deployment_name: String,
            last_modified: Timestamp,
        }
        impl LruKey for Key {
            type Value = WithSize<ComponentRegistry>;
        }
        let component_tablet_id = table_registry
            .1
            .table_mapping()
            .namespace(TableNamespace::Global)
            .id(&COMPONENTS_TABLE)?
            .tablet_id;
        let components_by_id =
            IndexRef::try_from(index_registry.1.must_get_by_id(component_tablet_id)?)?;
        let components_last_modified = *self
            .backend_last_modified
            .get(&components_by_id.id())
            .context("_components not configured to be in-memory")?;
        const NAME: &str = "_component_registry";
        log_funrun_index_cache_get(NAME);
        let index_reader = self.index_reader.clone();
        let component_registry = self
            .cache
            .get(
                &Key {
                    deployment_name: self.deployment_name.clone(),
                    last_modified: table_registry
                        .0
                        .max(index_registry.0)
                        .max(components_last_modified),
                },
                || async move {
                    let (documents, size) = load_unpacked_index(
                        components_by_id,
                        &index_reader,
                        component_tablet_id,
                        NAME,
                    )
                    .await?;
                    let component_registry = ComponentRegistry::bootstrap(
                        table_registry.1.table_mapping(),
                        documents.into_iter().map(|d| d.parse()).try_collect()?,
                    )?;
                    Ok(WithSize(component_registry, size))
                },
            )
            .await?;
        Ok((components_last_modified, component_registry.0.clone()))
    }

    #[fastrace::trace]
    async fn load_schema_registry(
        &self,
        table_registry: (Timestamp, TableRegistry),
        index_registry: (Timestamp, IndexRegistry),
        component_registry: (Timestamp, ComponentRegistry),
    ) -> anyhow::Result<(Timestamp, SchemaRegistry)> {
        #[derive(Hash, PartialEq, Eq, Debug, Clone)]
        struct Key {
            deployment_name: String,
            last_modified: Timestamp,
        }
        impl LruKey for Key {
            type Value = WithSize<SchemaRegistry>;
        }
        let table_mapping = table_registry.1.table_mapping();
        // Each component's namespace has a _schemas table.
        // Note there may be _schemas table in other namespaces, but we don't care about
        // those (and also they're not necessarily loaded into memory yet).
        // This argument only applies because we're in the function runner, which
        // can only operate in components' namespaces -- internal database workers
        // like IndexWorker and SchemaWorker include schemas from all namespaces.
        let component_ids = component_registry
            .1
            .all_component_paths(&mut TransactionReadSet::new())
            .into_keys();
        let mut last_modified_ts = table_registry
            .0
            .max(index_registry.0)
            .max(component_registry.0);
        let mut schema_tables = vec![];
        const NAME: &str = "_schema_registry";
        for component_id in component_ids {
            let namespace = component_id.into();
            let schema_tablet =
                table_mapping.namespace(namespace).name_to_tablet()(SCHEMAS_TABLE.clone())?;
            let index = IndexRef::try_from(index_registry.1.must_get_by_id(schema_tablet)?)?;
            let schemas_last_modified = *self
                .backend_last_modified
                .get(&index.id())
                .context("_schemas not configured to be in-memory")?;
            last_modified_ts = last_modified_ts.max(schemas_last_modified);
            schema_tables.push((namespace, schema_tablet, index));
            log_funrun_index_cache_get(NAME);
        }
        let index_reader = self.index_reader.clone();
        let schema_registry = self
            .cache
            .get(
                &Key {
                    deployment_name: self.deployment_name.clone(),
                    last_modified: last_modified_ts,
                },
                || async move {
                    let mut size = 0;
                    let mut schema_docs = BTreeMap::new();
                    for (namespace, schema_tablet, index) in schema_tables {
                        let (component_documents, component_size) =
                            load_unpacked_index(index, &index_reader, schema_tablet, NAME).await?;
                        schema_docs.insert(
                            namespace,
                            component_documents
                                .into_iter()
                                .map(|d| d.parse())
                                .try_collect()?,
                        );
                        size += component_size;
                    }
                    let schema_registry = SchemaRegistry::bootstrap(schema_docs);
                    Ok(WithSize(schema_registry, size))
                },
            )
            .await?;
        Ok((last_modified_ts, schema_registry.0.clone()))
    }

    #[fastrace::trace]
    async fn load_registries(
        &self,
        bootstrap_metadata: BootstrapMetadata,
    ) -> anyhow::Result<(
        TableRegistry,
        SchemaRegistry,
        ComponentRegistry,
        IndexRegistry,
    )> {
        // This is unfortunate but we need this cascade of cached lookups
        // because we don't know what cache keys to use until the previous
        // registry has been fetched.
        let table_registry = self.load_table_registry(&bootstrap_metadata).await?;
        let index_registry = self
            .load_index_registry(table_registry.clone(), &bootstrap_metadata)
            .await?;
        let component_registry = self
            .load_component_registry(table_registry.clone(), index_registry.clone())
            .await?;
        let schema_registry = self
            .load_schema_registry(
                table_registry.clone(),
                index_registry.clone(),
                component_registry.clone(),
            )
            .await?;
        Ok((
            table_registry.1,
            schema_registry.1,
            component_registry.1,
            index_registry.1,
        ))
    }
}

impl<RT: Runtime> InMemoryIndexCache<RT> {
    /// Loads table and index registry from cache or persistence snapshot.
    #[fastrace::trace]
    pub(crate) async fn begin_tx(
        &self,
        identity: Identity,
        existing_writes: FunctionWrites,
        index_reader: Arc<dyn IndexReader>,
        deployment_name: String,
        in_memory_index_last_modified: BTreeMap<IndexId, Timestamp>,
        bootstrap_metadata: BootstrapMetadata,
        table_count_snapshot: Arc<dyn TableCountSnapshot>,
        text_index_snapshot: Arc<dyn TransactionTextSnapshot>,
        usage_tracker: FunctionUsageTracker,
    ) -> anyhow::Result<Transaction<RT>> {
        let _timer = begin_tx_timer();
        let ts = index_reader.timestamp();
        for (index_id, last_modified) in &in_memory_index_last_modified {
            anyhow::ensure!(
                *last_modified <= *ts,
                "Last modified timestamp {last_modified} for index {index_id} is ahead of \
                 transaction timestamp {}",
                *ts
            );
        }

        let in_memory_indexes = FunctionRunnerInMemoryIndexes {
            cache: self.cache.clone(),
            deployment_name,
            backend_last_modified: in_memory_index_last_modified,
            index_reader: index_reader.clone(),
        };
        let (table_registry, schema_registry, component_registry, index_registry) =
            in_memory_indexes
                .load_registries(bootstrap_metadata)
                .await?;
        let database_index_snapshot = DatabaseIndexSnapshot::new(
            index_registry.clone(),
            Arc::new(in_memory_indexes),
            table_registry.table_mapping().clone(),
            index_reader,
            None,
            None,
        );

        let mut tx = make_transaction(
            ts,
            identity,
            self.rt.clone(),
            table_registry,
            schema_registry,
            component_registry,
            index_registry,
            table_count_snapshot,
            database_index_snapshot,
            text_index_snapshot,
            virtual_system_mapping().clone(),
            usage_tracker,
        )?;
        tx.merge_writes(existing_writes.updates)?;
        Ok(tx)
    }
}

/// An [`InMemoryIndexCache`] that is bound to a single instance at a specific
/// timestamp. One instance is created at the beginning of each UDF's
/// transaction.
pub(crate) struct FunctionRunnerInMemoryIndexes<RT: Runtime> {
    pub(crate) cache: MultiTypeAsyncLru<RT>,
    pub(crate) deployment_name: String,
    /// The last modified timestamp for each index at the beginning of the
    /// Transaction (i.e. as of `index_reader.timestamp()`).
    pub(crate) backend_last_modified: BTreeMap<IndexId, Timestamp>,
    pub(crate) index_reader: Arc<dyn IndexReader>,
}

#[async_trait]
impl<RT: Runtime> InMemoryIndexes for FunctionRunnerInMemoryIndexes<RT> {
    async fn range(
        &self,
        index: IndexRef,
        interval: &Interval,
        order: Order,
        tablet_id: TabletId,
        table_name: TableName,
    ) -> anyhow::Result<Option<Vec<(IndexKeyBytes, Timestamp, MemoryDocument)>>> {
        let Some(index_map) = self.get_or_load(index, tablet_id, table_name).await? else {
            return Ok(None);
        };
        let range = order
            .apply(
                index_map
                    .map
                    .range(interval)
                    .map(|(k, (ts, v))| (IndexKeyBytes(k.clone()), *ts, v.clone())),
            )
            .collect::<Vec<(IndexKeyBytes, Timestamp, MemoryDocument)>>();
        Ok(Some(range))
    }
}
