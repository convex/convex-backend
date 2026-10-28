use std::{
    cmp,
    collections::{
        BTreeMap,
        BTreeSet,
    },
    ops::Bound,
    sync::{
        atomic::{
            AtomicUsize,
            Ordering,
        },
        Arc,
        LazyLock,
        OnceLock,
    },
    time::Duration,
};

use anyhow::{
    Context,
    Error,
};
use arc_swap::ArcSwapOption;
use async_lru::async_lru::AsyncLru;
use cmd_util::env::env_config;
use common::{
    bootstrap_model::{
        components::ComponentMetadata,
        index::{
            database_index::IndexedFields,
            IndexMetadata,
            TabletIndexMetadata,
            INDEX_TABLE,
        },
        tables::{
            TableMetadata,
            TableState,
            TABLES_TABLE,
        },
    },
    components::{
        ComponentId,
        ComponentPath,
    },
    document::{
        CreationTime,
        DocumentUpdate,
        PackedDocument,
        ParseDocument,
        ParsedDocument,
        ResolvedDocument,
    },
    index::IndexKeyBytes,
    interval::Interval,
    knobs::{
        DATA_SYNC_BY_ID_FRESHNESS,
        DATA_SYNC_MAX_ROWS_READ,
        DATA_SYNC_PAGE_BYTES_LIMIT,
        DATA_SYNC_PAGE_SIZE_LIMIT,
        DEFAULT_DOCUMENTS_PAGE_SIZE,
        LIST_SNAPSHOT_MAX_AGE_SECS,
        SNAPSHOT_LIST_TIME_LIMIT,
    },
    pause::Fault,
    persistence::{
        new_idle_repeatable_ts,
        ConflictStrategy,
        DocumentLogEntry,
        DocumentRevisionStream,
        DocumentStream,
        LatestDocument,
        LatestDocumentStream,
        Persistence,
        PersistenceGlobalKey,
        PersistenceIndexEntry,
        PersistenceReader,
        PersistenceSnapshot,
        RepeatablePersistence,
        RetentionValidator,
        TimestampRange,
    },
    query::{
        CursorPosition,
        Order,
    },
    runtime::{
        tokio_spawn,
        RateLimiter,
        Runtime,
        SpawnHandle,
    },
    shutdown::ShutdownSignal,
    sync::split_rw_lock::{
        new_split_rw_lock,
        Reader,
    },
    types::{
        GenericIndexName,
        IndexId,
        IndexName,
        IndexRef,
        IndexTableIdentifier,
        PersistenceIndexId,
        PersistenceVersion,
        RepeatableTimestamp,
        TableName,
        TabletIndexName,
        Timestamp,
    },
    value::{
        ConvexObject,
        ResolvedDocumentId,
        TableMapping,
        TabletId,
    },
    virtual_system_mapping::VirtualSystemMapping,
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
    OccInfo,
};
use futures::{
    future::{
        BoxFuture,
        Either,
    },
    pin_mut,
    stream::BoxStream,
    FutureExt,
    StreamExt,
    TryFutureExt as _,
    TryStreamExt,
};
use imbl::OrdMap;
use indexing::{
    database_index_snapshot::{
        DatabaseIndexSnapshot,
        TimestampedIndexCache,
    },
    in_memory_indexes::{
        BackendInMemoryIndexes,
        NoInMemoryIndexes,
    },
    index_cache::{
        IndexCacheHandle,
        IndexCacheHandleBuilder,
    },
    index_reader::{
        IndexEntry,
        IndexReader,
    },
    index_registry::IndexRegistry,
};
use itertools::Itertools;
use keybroker::Identity;
use search::{
    metrics::SearchType,
    Searcher,
    TextIndexManager,
    TextIndexManagerState,
    TextSearchResults,
};
use short_future::ShortBoxFuture;
use storage::Storage;
use sync_types::backoff::Backoff;
use tokio::{
    sync::mpsc,
    task,
    time::Instant,
};
use tokio_util::task::AbortOnDropHandle;
use usage_tracking::{
    FunctionUsageStats,
    FunctionUsageTracker,
};
use value::{
    id_v6::DeveloperDocumentId,
    InternalDocumentId,
    Size,
    TableNamespace,
    TableNumber,
};
use vector::{
    PublicVectorSearchQueryResult,
    VectorIndexManager,
    VectorSearch,
};

use crate::{
    bootstrap_model::{
        next_persistence_index_id::types::NextPersistenceIndexIdMetadata,
        table::{
            NUM_RESERVED_LEGACY_TABLE_NUMBERS,
            NUM_RESERVED_SYSTEM_TABLE_NUMBERS,
        },
    },
    committer::{
        Committer,
        CommitterClient,
    },
    defaults::{
        bootstrap_system_tables,
        DEFAULT_BOOTSTRAP_TABLE_NUMBERS,
    },
    metrics::{
        self,
        load_indexes_into_memory_timer,
        vector::vector_search_with_retries_timer,
        verify_invariants_timer,
    },
    retention::{
        LeaderRetentionManager,
        LeaderRetentionWorkers,
    },
    schema_registry::SchemaRegistry,
    search_flusher_wake::SearchFlusherWakeSubscriber,
    search_index_bootstrap::SearchIndexBootstrapWorker,
    snapshot_manager::{
        Snapshot,
        SnapshotManager,
        TableCounts,
    },
    stack_traces::StackTrace,
    streaming_export_selection::{
        StreamingExportDocument,
        StreamingExportSelection,
    },
    subscription::{
        InvalidationMetricCallback,
        Subscription,
        SubscriptionsClient,
        SubscriptionsWorker,
    },
    system_tables::{
        ErasedSystemIndex,
        SystemTable,
    },
    table_registry::TableRegistry,
    table_summary::{
        self,
        BootstrapKind,
        TableShapes,
        TableSummaries,
    },
    table_usage::TablesUsage,
    token::Token,
    transaction_id_generator::TransactionIdGenerator,
    transaction_index::{
        TextIndexManagerSnapshot,
        TransactionIndex,
    },
    write_log::{
        new_write_log,
        LogReader,
        WriteSource,
    },
    BootstrapComponentsModel,
    ComponentRegistry,
    ComponentsTable,
    DataSyncIterator,
    LatestTableIterator,
    MultiTableIterator,
    SchemasTable,
    TableIterator,
    Transaction,
    TransactionReadSet,
    TransactionTextSnapshot,
    COMPONENTS_TABLE,
    NEXT_PERSISTENCE_INDEX_ID_TABLE,
    SCHEMAS_TABLE,
};

/// Controls the number of read set backtraces to show when debugging
/// OCC errors. Collecting stack traces is expensive and should only
/// be used in development.
///
/// Must be used in tandem with `READ_SET_CAPTURE_BACKTRACES`.
static NUM_READ_SET_STACKS: LazyLock<usize> =
    LazyLock::new(|| env_config("NUM_READ_SET_STACKS", 1));
const INITIAL_OCC_BACKOFF: Duration = Duration::from_millis(10);
const MAX_OCC_BACKOFF: Duration = Duration::from_secs(2);
pub const MAX_OCC_FAILURES: u32 = 3;
pub const MAX_OVERLOADED_FAILURES: u32 = 20;
const INITIAL_OVERLOADED_BACKOFF: Duration = Duration::from_millis(10);
const MAX_OVERLOADED_BACKOFF: Duration = Duration::from_secs(30);

/// In memory vector changes are asynchronously backfilled on startup. Attempts
/// to query before backfill is finished will result in failure, so we need to
/// retry. Vector search is latency tolerant because it's only run in actions,
/// so we can retry for a while before we have to fail.
const INITIAL_VECTOR_BACKOFF: Duration = Duration::from_millis(150);
const MAX_VECTOR_BACKOFF: Duration = Duration::from_millis(2500);
// 150 * 2^5 ~= 5000 or 5 seconds total.
const MAX_VECTOR_ATTEMPTS: u32 = 5;

/// Public entry point for interacting with the database.
///
/// This structure is cheap to clone and can be shared throughout the
/// application. Internally, it only has read-only access to the database
/// metadata, document store, and index manager.
/// Beginning a transaction chooses a timestamp and procures a snapshot of the
/// DocumentStore and DatabaseIndex data structures, so operations on the
/// [Transaction] don't even need to acquire [Database]'s read-lock.
///
/// Then, the [Committer], accessed via the [CommitterClient], has exclusive
/// access to mutate the database state. Calling [Database::commit] sends a
/// message to the [Committer] task, which then applies each transaction
/// serially.
///
/// See the diagram in `database/README.md` for more details.
#[derive(Clone)]
pub struct Database<RT: Runtime> {
    committer: CommitterClient,
    subscriptions: SubscriptionsClient,
    log: LogReader,
    snapshot_manager: Reader<SnapshotManager>,
    /// Table shapes maintained asynchronously by the `TableSummaryWorker`,
    /// `None` until the worker has published for the first time.
    table_shapes: Arc<ArcSwapOption<TableShapes>>,
    pub(crate) runtime: RT,
    reader: Arc<dyn PersistenceReader>,
    write_commits_since_load: Arc<AtomicUsize>,
    retention_manager: Arc<LeaderRetentionManager<RT>>,
    retention_workers: LeaderRetentionWorkers,
    pub searcher: Arc<dyn Searcher>,
    pub search_storage: Arc<OnceLock<Arc<dyn Storage>>>,
    index_cache_handle: IndexCacheHandle,
    virtual_system_mapping: VirtualSystemMapping,
    pub bootstrap_metadata: BootstrapMetadata,
    invalidation_callback: InvalidationMetricCallback,
    // Caches of snapshot TableMapping and by_id index ids, which are used repeatedly by
    // /api/list_snapshot.
    table_mapping_snapshot_cache: AsyncLru<RT, Timestamp, TableMapping>,
    by_id_indexes_snapshot_cache: AsyncLru<RT, Timestamp, BTreeMap<TabletId, IndexRef>>,
    component_paths_snapshot_cache: AsyncLru<RT, Timestamp, BTreeMap<ComponentId, ComponentPath>>,
    list_snapshot_table_iterator_cache:
        Arc<tokio::sync::Mutex<Option<ListSnapshotTableIteratorCacheEntry<RT>>>>,
}

struct ListSnapshotTableIteratorCacheEntry<RT: Runtime> {
    cache_key: ListSnapshotTableIteratorCacheKey,
    iterator: BoxFuture<
        'static,
        anyhow::Result<
            BoxStream<'static, anyhow::Result<Either<LatestDocument, MultiTableIterator<RT>>>>,
        >,
    >,
}
#[derive(PartialEq, Eq)]
struct ListSnapshotTableIteratorCacheKey {
    snapshot: Timestamp,
    tablet_ids: Vec<TabletId>,
    by_id: IndexRef,
    cursor: Option<ResolvedDocumentId>,
}

#[derive(Clone)]
pub struct DatabaseSnapshot<RT: Runtime> {
    runtime: RT,
    ts: RepeatableTimestamp,
    pub bootstrap_metadata: BootstrapMetadata,
    pub snapshot: Snapshot,
    /// Counts and shapes for offline tools (`db-info`, `db-verifier`),
    /// populated by `load_table_summaries`. The inner `snapshot` only holds
    /// counts.
    table_summaries: Option<TableSummaries>,
    pub persistence_snapshot: PersistenceSnapshot,
    index_cache_handle: Option<IndexCacheHandle>,

    // To read lots of data at the snapshot, sometimes you need
    // to look at current data and walk backwards.
    // Use the `table_iterator` method to do that -- don't access these
    // fields directly.
    pub persistence_reader: Arc<dyn PersistenceReader>,
    pub retention_validator: Arc<dyn RetentionValidator>,
}

#[derive(PartialEq, Eq, Debug)]
pub struct DocumentDeltas {
    /// Document deltas returned in increasing (ts, tablet_id, id) order.
    /// We use ResolvedDocument here rather than DeveloperDocument
    /// because streaming export always uses string IDs
    pub deltas: Vec<(
        Timestamp,
        DeveloperDocumentId,
        ComponentPath,
        TableName,
        Option<StreamingExportDocument>,
    )>,
    /// Exclusive cursor timestamp to pass in to the next call to
    /// document_deltas.
    pub cursor: Timestamp,
    /// Continue calling document_deltas while has_more is true.
    pub has_more: bool,
    pub usage: FunctionUsageStats,
}

#[derive(PartialEq, Eq, Debug)]
pub struct SnapshotPage {
    pub documents: Vec<(Timestamp, ComponentPath, TableName, StreamingExportDocument)>,
    pub snapshot: Timestamp,
    pub cursor: Option<ResolvedDocumentId>,
    pub has_more: bool,
    pub usage: FunctionUsageStats,
}

#[derive(Clone)]
pub struct BootstrapMetadata {
    pub tables_by_id: IndexRef,
    pub index_by_id: IndexRef,
    pub tables_tablet_id: TabletId,
    pub index_tablet_id: TabletId,
}

impl<RT: Runtime> DatabaseSnapshot<RT> {
    pub async fn max_ts(reader: &dyn PersistenceReader) -> anyhow::Result<Timestamp> {
        reader
            .max_ts()
            .await?
            .ok_or_else(|| anyhow::anyhow!("no documents -- cannot load uninitialized database"))
    }

    #[fastrace::trace]
    async fn load_raw_and_parsed_table_documents<
        D: TryFrom<ConvexObject, Error = anyhow::Error>,
    >(
        persistence_snapshot: &PersistenceSnapshot,
        index: IndexRef,
        tablet_id: TabletId,
    ) -> anyhow::Result<(Vec<(Timestamp, PackedDocument)>, Vec<ParsedDocument<D>>)> {
        persistence_snapshot
            .index_scan(index, tablet_id, &Interval::all(), Order::Asc, usize::MAX)
            .map(|row| {
                let rev = row?.1;
                let doc = PackedDocument::pack(&rev.value);
                let parsed = rev.value.parse()?;
                Ok(((rev.ts, doc), parsed))
            })
            .try_collect()
            .await
    }

    fn load_table_documents<T: SystemTable>(
        in_memory_indexes: &BackendInMemoryIndexes,
        table_mapping: &TableMapping,
        index_registry: &IndexRegistry,
        namespace: TableNamespace,
    ) -> anyhow::Result<Vec<ParsedDocument<T::Metadata>>>
    where
        T::Metadata: Send + Sync + Clone,
        for<'a> &'a PackedDocument: ParseDocument<T::Metadata>,
    {
        let tablet_id = table_mapping.namespace(namespace).name_to_tablet()(T::TABLE_NAME.clone())?;
        let by_id = index_registry.must_get_by_id(tablet_id)?.id();
        let docs = in_memory_indexes
            .range(by_id, &Interval::all(), Order::Asc)?
            .with_context(|| format!("table {} is not in-memory?", T::TABLE_NAME))?;
        docs.into_iter()
            .map(|doc| doc.2.force().map(Arc::unwrap_or_clone))
            .try_collect()
    }

    pub fn table_mapping_and_states(
        table_documents: Vec<ParsedDocument<TableMetadata>>,
    ) -> (TableMapping, OrdMap<TabletId, TableState>) {
        let mut table_mapping = TableMapping::new();
        let mut table_states = OrdMap::new();
        for table_doc in table_documents {
            let tablet_id = TabletId(table_doc.id().internal_id());
            table_states.insert(tablet_id, table_doc.state);
            let table_number = table_doc.number;
            let table_metadata = table_doc.into_value();
            match table_metadata.state {
                TableState::Active => table_mapping.insert(
                    tablet_id,
                    table_metadata.namespace,
                    table_number,
                    table_metadata.name,
                ),
                TableState::Hidden => table_mapping.insert_tablet(
                    tablet_id,
                    table_metadata.namespace,
                    table_number,
                    table_metadata.name,
                ),
                TableState::Deleting => {},
            }
        }
        (table_mapping, table_states)
    }

    #[fastrace::trace]
    pub async fn load_table_and_index_metadata(
        persistence_snapshot: &PersistenceSnapshot,
    ) -> anyhow::Result<(
        TableMapping,
        OrdMap<TabletId, TableState>,
        IndexRegistry,
        Vec<(Timestamp, PackedDocument)>,
        Vec<(Timestamp, PackedDocument)>,
        BootstrapMetadata,
    )> {
        let _timer = metrics::load_table_and_index_metadata_timer();
        let bootstrap_metadata = Self::get_meta_ids(persistence_snapshot).await?;
        let BootstrapMetadata {
            tables_by_id,
            index_by_id,
            tables_tablet_id,
            index_tablet_id,
        }: BootstrapMetadata = bootstrap_metadata;

        let (index_documents, parsed_index_documents) = Self::load_raw_and_parsed_table_documents(
            persistence_snapshot,
            index_by_id,
            index_tablet_id,
        )
        .await?;
        let (table_documents, parsed_table_documents) = Self::load_raw_and_parsed_table_documents(
            persistence_snapshot,
            tables_by_id,
            tables_tablet_id,
        )
        .await?;

        let (table_mapping, table_states) = Self::table_mapping_and_states(parsed_table_documents);
        let index_registry =
            IndexRegistry::bootstrap(&table_mapping, parsed_index_documents.into_iter())?;
        Ok((
            table_mapping,
            table_states,
            index_registry,
            table_documents,
            index_documents,
            bootstrap_metadata,
        ))
    }

    #[fastrace::trace]
    pub fn load_table_registry(
        table_mapping: TableMapping,
        table_states: OrdMap<TabletId, TableState>,
        index_registry: &IndexRegistry,
    ) -> anyhow::Result<TableRegistry> {
        let table_registry = TableRegistry::bootstrap(table_mapping, table_states)?;
        Self::verify_invariants(&table_registry, index_registry)?;
        Ok(table_registry)
    }

    pub fn table_iterator(&self) -> TableIterator<RT> {
        TableIterator::new(
            self.runtime.clone(),
            self.timestamp(),
            self.persistence_reader.clone(),
            self.retention_validator.clone(),
            1000,
        )
    }

    /// A streaming-export iterator that syncs many tables with bounded reads
    /// and a small, resumable cursor. Each page reads at a snapshot at least as
    /// recent as this one. See [`DataSyncIterator`].
    pub fn data_sync_iterator(&self) -> anyhow::Result<DataSyncIterator<RT>> {
        DataSyncIterator::new(
            self.runtime.clone(),
            self.persistence_reader.clone(),
            self.retention_validator.clone(),
            self.timestamp(),
            *DATA_SYNC_PAGE_SIZE_LIMIT,
            *DATA_SYNC_PAGE_BYTES_LIMIT,
            *DATA_SYNC_MAX_ROWS_READ,
            *DATA_SYNC_BY_ID_FRESHNESS,
        )
    }

    #[fastrace::trace]
    pub fn get_document_and_index_storage(
        &self,
        identity: &Identity,
    ) -> anyhow::Result<TablesUsage> {
        if !(identity.is_admin() || identity.is_system()) {
            anyhow::bail!(unauthorized_error("get_user_document_storage"));
        }

        self.snapshot.get_document_and_index_storage()
    }

    #[fastrace::trace]
    pub fn get_vector_index_storage(
        &self,
        identity: &Identity,
    ) -> anyhow::Result<BTreeMap<(ComponentPath, TableName), u64>> {
        if !(identity.is_admin() || identity.is_system()) {
            anyhow::bail!(unauthorized_error("get_vector_index_storage"));
        }
        let table_mapping = &self.snapshot.table_registry.table_mapping();
        let index_registry = &self.snapshot.index_registry;
        let mut vector_index_storage = BTreeMap::new();
        for index in index_registry.all_vector_indexes().into_iter() {
            let (_, value) = index.into_id_and_value();
            let tablet_id = *value.name.table();
            let table_namespace = table_mapping.tablet_namespace(tablet_id)?;
            let component_id = ComponentId::from(table_namespace);
            let table_name = table_mapping.tablet_name(tablet_id)?;
            let size = value.config.estimate_pricing_size_bytes()?;
            if let Some(component_path) = self
                .snapshot
                .component_registry
                .get_component_path(component_id, &mut TransactionReadSet::new())
            {
                vector_index_storage
                    .entry((component_path, table_name))
                    .and_modify(|sum| *sum += size)
                    .or_insert(size);
            } else {
                // If there is no component path for this table namespace, this must be an empty
                // user table left over from incomplete components push
                anyhow::ensure!(
                    size == 0,
                    "Table {table_name} is in an orphaned TableNamespace without a component, but \
                     has non-zero vector index size {size}",
                );
            }
        }
        Ok(vector_index_storage)
    }

    #[fastrace::trace]
    pub fn get_text_index_storage(
        &self,
        identity: &Identity,
    ) -> anyhow::Result<BTreeMap<(ComponentPath, TableName), u64>> {
        if !(identity.is_admin() || identity.is_system()) {
            anyhow::bail!(unauthorized_error("get_text_index_storage"));
        }
        let table_mapping = &self.snapshot.table_registry.table_mapping();
        let index_registry = &self.snapshot.index_registry;
        let mut text_index_storage = BTreeMap::new();
        for index in index_registry.all_text_indexes().into_iter() {
            let (_, value) = index.into_id_and_value();
            let tablet_id = *value.name.table();
            let table_namespace = table_mapping.tablet_namespace(tablet_id)?;
            let component_id = ComponentId::from(table_namespace);
            let table_name = table_mapping.tablet_name(tablet_id)?;
            let size = value.config.estimate_pricing_size_bytes()?;
            if let Some(component_path) = self
                .snapshot
                .component_registry
                .get_component_path(component_id, &mut TransactionReadSet::new())
            {
                text_index_storage
                    .entry((component_path, table_name))
                    .and_modify(|sum| *sum += size)
                    .or_insert(size);
            } else {
                // If there is no component path for this table namespace, this must be an empty
                // user table left over from incomplete components push
                anyhow::ensure!(
                    size == 0,
                    "Table {table_name} is in an orphaned TableNamespace without a component, but \
                     has non-zero text index size {size}",
                );
            }
        }
        Ok(text_index_storage)
    }

    /// Counts the number of documents in each table, including system tables.
    #[fastrace::trace]
    pub fn get_document_counts(
        &self,
        identity: &Identity,
    ) -> anyhow::Result<Vec<(ComponentPath, TableName, u64)>> {
        if !(identity.is_admin() || identity.is_system()) {
            anyhow::bail!(unauthorized_error("get_document_counts"));
        }
        let mut document_counts = vec![];
        for entry in self.snapshot.iter_table_counts()? {
            let (_namespace, component_path, table_name, table_count) = entry?;
            let count = table_count.num_values();
            // exclude orphaned tables (in incomplete component namespaces)
            if let Some(component_path) = component_path {
                document_counts.push((component_path, table_name, count));
            }
        }
        Ok(document_counts)
    }

    pub async fn full_table_scan(
        &self,
        tablet_id: TabletId,
    ) -> anyhow::Result<LatestDocumentStream<'_>> {
        let table_iterator = self.table_iterator();
        let by_creation_time = self
            .index_registry()
            .get_enabled(&TabletIndexName::by_creation_time(tablet_id));
        let stream = if let Some(by_creation_time) = by_creation_time {
            let stream = table_iterator.stream_documents_in_table_by_index(
                tablet_id,
                IndexRef::try_from(by_creation_time)?,
                IndexedFields::creation_time(),
                None,
            );
            stream.map_ok(|(_, rev)| rev).boxed()
        } else {
            let table_by_id = IndexRef::try_from(self.index_registry().must_get_by_id(tablet_id)?)?;
            table_iterator
                .stream_documents_in_table(tablet_id, table_by_id, None)
                .boxed()
        };
        Ok(stream)
    }

    /// Fetch _tables.by_id and _index.by_id for bootstrapping.
    pub async fn get_meta_ids(
        persistence_snapshot: &PersistenceSnapshot,
    ) -> anyhow::Result<BootstrapMetadata> {
        let persistence = persistence_snapshot.persistence();
        let tables_by_id = persistence
            .get_persistence_global(PersistenceGlobalKey::TablesByIdIndex)
            .await?
            .context("missing _tables.by_id global")?
            .as_str()
            .context("_tables.by_id is not string")?
            .parse()?;
        let index_by_id = persistence
            .get_persistence_global(PersistenceGlobalKey::IndexByIdIndex)
            .await?
            .context("missing _index.by_id global")?
            .as_str()
            .context("_index.by_id is not string")?
            .parse()?;
        let tables_tablet_id: TabletId = persistence
            .get_persistence_global(PersistenceGlobalKey::TablesTabletId)
            .await?
            .context("missing _tables table ID global")?
            .as_str()
            .context("_tables table ID is not string")?
            .parse()?;
        let index_tablet_id = persistence
            .get_persistence_global(PersistenceGlobalKey::IndexTabletId)
            .await?
            .context("missing _index table ID global")?
            .as_str()
            .context("_index table ID is not string")?
            .parse()?;
        let tables_by_id =
            Self::bootstrap_index_ref(persistence_snapshot, index_tablet_id, tables_by_id).await?;
        let index_by_id =
            Self::bootstrap_index_ref(persistence_snapshot, index_tablet_id, index_by_id).await?;
        Ok(BootstrapMetadata {
            tables_by_id,
            index_by_id,
            tables_tablet_id,
            index_tablet_id,
        })
    }

    /// Resolves a bootstrap index from its `_index` document at the snapshot.
    /// Every other index resolves through the index registry, but these two
    /// are what the registry is loaded with, so their metadata is read by
    /// document ID instead.
    async fn bootstrap_index_ref(
        persistence_snapshot: &PersistenceSnapshot,
        index_tablet_id: TabletId,
        index_id: IndexId,
    ) -> anyhow::Result<IndexRef> {
        let document_id = InternalDocumentId::new(index_tablet_id, index_id.0);
        // `previous_revisions` returns the newest revision strictly before the
        // timestamp, so ask one past the snapshot.
        let after_snapshot = persistence_snapshot.timestamp().succ()?;
        let mut revisions = persistence_snapshot
            .persistence()
            .previous_revisions(
                BTreeSet::from([(document_id, after_snapshot)]),
                persistence_snapshot.retention_validator(),
            )
            .await?;
        let document = revisions
            .remove(&(document_id, after_snapshot))
            .and_then(|revision| revision.value)
            .with_context(|| format!("bootstrap index {index_id} has no `_index` document"))?;
        IndexRef::try_from(&TabletIndexMetadata::from_document(document)?)
    }

    #[fastrace::trace]
    pub async fn load(
        runtime: RT,
        persistence: Arc<dyn PersistenceReader>,
        snapshot: RepeatableTimestamp,
        retention_validator: Arc<dyn RetentionValidator>,
        virtual_system_mapping: VirtualSystemMapping,
    ) -> anyhow::Result<Self> {
        let repeatable_persistence: RepeatablePersistence =
            RepeatablePersistence::new(persistence.clone(), snapshot, retention_validator.clone());
        let persistence_snapshot =
            repeatable_persistence.read_snapshot(repeatable_persistence.upper_bound())?;

        // Step 1: Fetch tables and indexes from persistence.
        tracing::info!("Bootstrapping indexes...");
        let (
            table_mapping,
            table_states,
            index_registry,
            table_documents,
            index_documents,
            bootstrap_metadata,
        ) = Self::load_table_and_index_metadata(&persistence_snapshot).await?;

        // Step 2: Load bootstrap tables indexes into memory.
        let load_indexes_into_memory_timer = load_indexes_into_memory_timer();
        let in_memory_indexes = {
            let mut index =
                BackendInMemoryIndexes::bootstrap(&index_registry, index_documents, *snapshot)?;
            // Since we already loaded the `TablesTable` from persistence, feed
            // the documents from memory instead of re-fetching them.
            index.load_table(
                &index_registry,
                bootstrap_metadata.tables_tablet_id,
                table_documents,
                *snapshot,
            );
            // Then fetch the remaining in-memory tables.
            index
                .load_enabled_for_tables(
                    &index_registry,
                    &table_mapping,
                    &persistence_snapshot,
                    &bootstrap_system_tables()
                        .iter()
                        .map(|t| t.table_name().clone())
                        .collect(),
                )
                .await?;
            index
        };
        drop(load_indexes_into_memory_timer);

        let search = TextIndexManager::new(TextIndexManagerState::Bootstrapping);
        let vector = VectorIndexManager::bootstrap_index_metadata(&index_registry)?;

        // Step 3: Bootstrap our database metadata from the `_tables` documents
        tracing::info!("Bootstrapping table metadata...");
        let table_registry =
            Self::load_table_registry(table_mapping.clone(), table_states, &index_registry)?;

        let mut schema_docs = BTreeMap::new();
        for namespace in table_mapping.namespaces_for_name(&SCHEMAS_TABLE) {
            let schema_documents = Self::load_table_documents::<SchemasTable>(
                &in_memory_indexes,
                &table_mapping,
                &index_registry,
                namespace,
            )?;
            schema_docs.insert(namespace, schema_documents);
        }

        let schema_registry = SchemaRegistry::bootstrap(schema_docs);

        let component_docs = Self::load_table_documents::<ComponentsTable>(
            &in_memory_indexes,
            &table_mapping,
            &index_registry,
            TableNamespace::Global,
        )?;
        let component_registry = ComponentRegistry::bootstrap(&table_mapping, component_docs)?;
        Ok(Self {
            runtime,
            ts: persistence_snapshot.timestamp(),
            bootstrap_metadata,
            snapshot: Snapshot {
                table_registry,
                schema_registry,
                component_registry,
                table_counts: None,
                index_registry,
                in_memory_indexes,
                virtual_system_mapping,
                text_indexes: search,
                vector_indexes: vector,
            },
            table_summaries: None,
            persistence_snapshot,
            index_cache_handle: None,

            persistence_reader: persistence,
            retention_validator,
        })
    }

    /// Block on loading the table summaries at the current snapshot timestamp.
    /// We intentionally do not block on loading table summaries on database
    /// initialization since it can be expensive, and instead do it in the
    /// background and later update it via the committer.
    ///
    /// But for tools like `db-info` or `db-verifier`, we want the table
    /// summaries to be loaded (and can't rely on TableSummaryWorker +
    /// committer in these services).
    #[fastrace::trace]
    pub async fn load_table_summaries(&mut self) -> anyhow::Result<()> {
        if self.table_summaries.is_some() {
            return Ok(());
        }

        tracing::info!("Bootstrapping table summaries...");
        let (table_summary_snapshot, summaries_num_rows) = table_summary::bootstrap(
            self.runtime.clone(),
            self.persistence_reader.clone(),
            self.retention_validator.clone(),
            self.ts,
            BootstrapKind::FromCheckpoint,
        )
        .await?;
        let table_counts = TableCounts::new(
            table_summary_snapshot.clone(),
            self.table_registry().table_mapping(),
            &self.snapshot.virtual_system_mapping,
        )?;
        self.snapshot.table_counts = Some(table_counts);
        self.table_summaries = Some(TableSummaries::new(
            table_summary_snapshot,
            self.table_registry().table_mapping(),
        ));
        tracing::info!("Bootstrapped table summaries (read {summaries_num_rows} rows)");
        Ok(())
    }

    pub fn timestamp(&self) -> RepeatableTimestamp {
        self.ts
    }

    pub fn runtime(&self) -> &RT {
        &self.runtime
    }

    pub fn verify_invariants(
        table_registry: &TableRegistry,
        index_registry: &IndexRegistry,
    ) -> anyhow::Result<()> {
        let _timer = verify_invariants_timer();
        // Verify that all tables have table scan index.
        for (tablet_id, _, _, table_name) in table_registry.table_mapping().iter() {
            anyhow::ensure!(
                index_registry
                    .get_enabled(&TabletIndexName::by_id(tablet_id))
                    .is_some(),
                "Missing `by_id` index for {}",
                table_name,
            );
        }

        // Verify that all indexes are defined on tables that exist.
        for tablet_id in index_registry.all_tables_with_indexes() {
            anyhow::ensure!(
                table_registry.table_mapping().tablet_id_exists(tablet_id),
                "Table {:?} is missing but has one or more indexes",
                tablet_id,
            );
        }

        Ok(())
    }

    pub fn table_registry(&self) -> &TableRegistry {
        &self.snapshot.table_registry
    }

    pub fn index_registry(&self) -> &IndexRegistry {
        &self.snapshot.index_registry
    }

    pub fn table_summaries(&self) -> Option<&TableSummaries> {
        self.table_summaries.as_ref()
    }

    /// Create a [`Transaction`] at the snapshot's timestamp. This allows using
    /// read-only APIs that require a Transaction without needing a `Database`.
    ///
    /// The transaction will not use any in-memory index cache, so this should
    /// not be used to serve any frequently called APIs.
    pub fn begin_tx(
        &self,
        identity: Identity,
        text_index_snapshot: Arc<dyn TransactionTextSnapshot>,
        usage_tracker: FunctionUsageTracker,
        virtual_system_mapping: VirtualSystemMapping,
    ) -> anyhow::Result<Transaction<RT>> {
        let database_index_snapshot = DatabaseIndexSnapshot::new(
            self.snapshot.index_registry.clone(),
            Arc::new(NoInMemoryIndexes),
            self.snapshot.table_registry.table_mapping().clone(),
            Arc::new(self.persistence_snapshot.clone()),
            self.index_cache_handle.clone(),
            None,
        );

        let id_generator = TransactionIdGenerator::new(&self.runtime.clone())?;
        let creation_time =
            CreationTime::for_transaction(*self.ts, self.runtime.generate_timestamp()?)?;
        let transaction_index = TransactionIndex::new(
            self.snapshot.index_registry.clone(),
            database_index_snapshot,
            text_index_snapshot,
        );
        Ok(Transaction::new(
            identity,
            id_generator,
            creation_time,
            transaction_index,
            self.snapshot.table_registry.clone(),
            self.snapshot.schema_registry.clone(),
            self.snapshot.component_registry.clone(),
            Arc::new(self.snapshot.table_counts.clone()),
            self.runtime.clone(),
            usage_tracker,
            virtual_system_mapping,
        ))
    }
}

impl<RT: Runtime> Database<RT> {
    #[fastrace::trace]
    pub async fn load(
        mut persistence: Arc<dyn Persistence>,
        runtime: RT,
        searcher: Arc<dyn Searcher>,
        shutdown: ShutdownSignal,
        virtual_system_mapping: VirtualSystemMapping,
        index_cache_handle: IndexCacheHandleBuilder,
        retention_rate_limiter: Arc<RateLimiter<RT>>,
        deleted_tablet_sender: mpsc::Sender<TabletId>,
        deployment_name: String,
    ) -> anyhow::Result<Self> {
        let _load_database_timer = metrics::load_database_timer();

        // Initialize the database if it's a new database.
        if persistence.is_fresh() {
            tracing::info!("Initializing database with system tables...");
            Self::initialize(&runtime, &mut persistence).await?;
        }

        // Load data into a DatabaseSnapshot, including indexes.
        let reader = persistence.reader();

        // Since we hold the lease, update the max repeatable timestamp and get
        // the latest timestamp to perform the load at.
        let snapshot_ts = new_idle_repeatable_ts(persistence.as_ref(), &runtime).await?;

        let (retention_manager, retention_worker_seed) =
            LeaderRetentionManager::new(runtime.clone(), persistence.clone(), snapshot_ts).await?;

        let db_snapshot = DatabaseSnapshot::load(
            runtime.clone(),
            reader.clone(),
            snapshot_ts,
            retention_manager.clone(),
            virtual_system_mapping.clone(),
        )
        .await?;
        let max_ts = DatabaseSnapshot::<RT>::max_ts(&*reader).await?;
        anyhow::ensure!(
            *snapshot_ts == max_ts,
            "race while loading DatabaseSnapshot: max ts {snapshot_ts} at start, {max_ts} at end",
        );
        let DatabaseSnapshot {
            bootstrap_metadata,
            ts,
            snapshot,
            ..
        } = db_snapshot;
        let snapshot_manager = SnapshotManager::new(*ts, snapshot);
        let (snapshot_reader, snapshot_writer) = new_split_rw_lock(snapshot_manager);

        let retention_workers = retention_worker_seed
            .start_retention(
                bootstrap_metadata.clone(),
                snapshot_reader.clone(),
                shutdown.clone(),
                retention_rate_limiter,
                deleted_tablet_sender,
            )
            .await?;

        let persistence_reader = persistence.reader();
        let (log_owner, log_reader, log_writer) = new_write_log(*ts);
        let index_cache_handle = index_cache_handle.build(
            bootstrap_metadata.index_tablet_id,
            Arc::new(log_reader.clone()),
        );
        let invalidation_callback = InvalidationMetricCallback::new();
        let subscriptions =
            SubscriptionsWorker::start(log_owner, runtime.clone(), invalidation_callback.clone());
        let committer = Committer::start(
            log_writer,
            snapshot_writer,
            persistence,
            runtime.clone(),
            retention_manager.clone(),
            shutdown,
            virtual_system_mapping.clone(),
            index_cache_handle.clone(),
            deployment_name,
        );
        let table_mapping_snapshot_cache =
            AsyncLru::new(runtime.clone(), 20, 2, 200, "table_mapping_snapshot");
        let by_id_indexes_snapshot_cache =
            AsyncLru::new(runtime.clone(), 20, 2, 200, "by_id_indexes_snapshot");
        let component_paths_snapshot_cache =
            AsyncLru::new(runtime.clone(), 20, 2, 200, "component_paths_snapshot");
        let list_snapshot_table_iterator_cache = Arc::new(tokio::sync::Mutex::new(None));
        let database = Self {
            committer,
            subscriptions,
            runtime,
            log: log_reader,
            retention_manager,
            retention_workers,
            snapshot_manager: snapshot_reader,
            table_shapes: Arc::new(ArcSwapOption::empty()),
            reader: persistence_reader.clone(),
            write_commits_since_load: Arc::new(AtomicUsize::new(0)),
            searcher,
            search_storage: Arc::new(OnceLock::new()),
            index_cache_handle,
            virtual_system_mapping,
            bootstrap_metadata,
            invalidation_callback,
            table_mapping_snapshot_cache,
            by_id_indexes_snapshot_cache,
            component_paths_snapshot_cache,
            list_snapshot_table_iterator_cache,
        };

        Ok(database)
    }

    /// Sets the invalidation callback for tracking which mutations invalidate
    /// subscriptions. Must be called after `load` and before any subscriptions
    /// are invalidated.
    pub fn set_invalidation_callback(
        &self,
        callback: Arc<dyn Fn(Vec<crate::InvalidationEvent>) + Send + Sync>,
    ) -> anyhow::Result<()> {
        self.invalidation_callback.set(callback)
    }

    pub fn set_search_storage(&self, search_storage: Arc<dyn Storage>) {
        self.search_storage
            .set(search_storage.clone())
            .expect("Tried to set search storage more than once");
        tracing::info!("Set search storage to {search_storage:?}");
    }

    /// The latest table shapes published by the `TableSummaryWorker`, or `None`
    /// if the worker has not published yet. These are maintained asynchronously
    /// and may be stale and inconsistent with the latest snapshot.
    pub fn table_shapes(&self) -> Option<Arc<TableShapes>> {
        self.table_shapes.load_full()
    }

    /// Publish table shapes to the in-memory store. Deliberately
    /// `pub(crate)`: the only caller should be
    /// `TableSummaryWriter::checkpoint`, which persists a checkpoint first.
    /// [`Database::table_shapes_at`] relies on every published [`TableShapes`]
    /// having a persisted checkpoint to replay the documents log from.
    pub(crate) fn publish_table_shapes(&self, shapes: TableShapes) {
        self.table_shapes.store(Some(Arc::new(shapes)));
    }

    /// Table shapes exact at `ts`, for consumers (like schema validation) that
    /// cannot tolerate the staleness of the published [`TableShapes`].
    ///
    /// Returns `None` if the `TableSummaryWorker` has not published yet. When
    /// the published shapes lag (or lead) `ts`, this catches them up by
    /// replaying the documents log between the last persisted checkpoint and
    /// `ts`
    pub async fn table_shapes_at(
        &self,
        ts: RepeatableTimestamp,
    ) -> anyhow::Result<Option<Arc<TableShapes>>> {
        let Some(published) = self.table_shapes() else {
            return Ok(None);
        };
        if published.ts == *ts {
            return Ok(Some(published));
        }
        tracing::info!(
            "Catching up table shapes from published ts {} to {}",
            published.ts,
            *ts
        );
        let (snapshot, _) = table_summary::bootstrap(
            self.runtime.clone(),
            self.reader.clone(),
            self.retention_validator(),
            ts,
            BootstrapKind::FromCheckpoint,
        )
        .await?;
        Ok(Some(Arc::new(snapshot.into())))
    }

    pub fn start_search_and_vector_bootstrap(&self) -> Box<dyn SpawnHandle> {
        let worker = self.new_search_and_vector_bootstrap_worker();
        self.runtime
            .spawn("search_and_vector_bootstrap", async move {
                worker.start().await
            })
    }

    pub async fn finish_table_summary_bootstrap(&self) -> anyhow::Result<()> {
        self.committer.finish_table_summary_bootstrap().await
    }

    fn new_search_and_vector_bootstrap_worker(&self) -> SearchIndexBootstrapWorker<RT> {
        let (ts, snapshot) = self.snapshot_manager.lock().latest();
        let vector_persistence =
            RepeatablePersistence::new(self.reader.clone(), ts, self.retention_validator());
        let table_mapping = snapshot.table_mapping().namespace(TableNamespace::Global);
        SearchIndexBootstrapWorker::new(
            self.runtime.clone(),
            snapshot.index_registry,
            vector_persistence,
            table_mapping,
            self.committer.clone(),
        )
    }

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        self.committer.shutdown();
        self.subscriptions.shutdown();
        self.index_cache_handle.remove_deployment();
        self.retention_workers.shutdown().await?;
        tracing::info!("Database shutdown");
        Ok(())
    }

    pub fn retention_validator(&self) -> Arc<dyn RetentionValidator> {
        self.retention_manager.clone()
    }

    /// Load the set of documents and tombstones in the given table between
    /// within the given timestamp.
    ///
    /// See PersistenceReader.load_documents_from_table for performance caveats!
    ///
    /// rate_limiter must be based on rows per second.
    pub fn load_documents_in_table<'a>(
        &'a self,
        tablet_id: TabletId,
        timestamp_range: TimestampRange,
        order: Order,
        rate_limiter: &'a RateLimiter<RT>,
    ) -> DocumentStream<'a> {
        self.reader
            .load_documents_from_table(
                tablet_id,
                timestamp_range,
                order,
                *DEFAULT_DOCUMENTS_PAGE_SIZE,
                self.retention_validator(),
            )
            .then(|val| async {
                while let Err(not_until) = rate_limiter.check() {
                    let delay = not_until.wait_time_from(self.runtime.monotonic_now().into());
                    self.runtime.wait(delay).await;
                }
                val
            })
            .boxed()
    }

    /// Like `load_documents_in_table`, but also includes previous revisions.
    pub fn load_revision_pairs_in_table<'a>(
        &'a self,
        tablet_id: TabletId,
        timestamp_range: TimestampRange,
        order: Order,
        rate_limiter: &'a RateLimiter<RT>,
    ) -> DocumentRevisionStream<'a> {
        self.reader
            .load_revision_pairs(
                Some(tablet_id),
                timestamp_range,
                order,
                *DEFAULT_DOCUMENTS_PAGE_SIZE,
                self.retention_validator(),
            )
            .then(|val| async {
                while let Err(not_until) = rate_limiter.check() {
                    let delay = not_until.wait_time_from(self.runtime.monotonic_now().into());
                    self.runtime.wait(delay).await;
                }
                val
            })
            .boxed()
    }

    /// Allows iterating over tables at any repeatable timestamp,
    /// even if it's outside of retention.
    /// TableIterator will have to walk all documents between snapshot_ts
    /// and now, so it is inefficient for very old snapshots.
    pub fn table_iterator(
        &self,
        snapshot_ts: RepeatableTimestamp,
        page_size: usize,
    ) -> TableIterator<RT> {
        let retention_validator = self.retention_validator();
        let persistence = self.reader.clone();
        TableIterator::new(
            self.runtime.clone(),
            snapshot_ts,
            persistence,
            retention_validator,
            page_size,
        )
    }

    /// Walk tables at the latest repeatable timestamp instead of a snapshot,
    /// so the walk never replays the document log. Every page is read at or
    /// after `min_ts`; see [`LatestTableIterator`] for the consistency it
    /// offers.
    pub fn latest_table_iterator(
        &self,
        min_ts: RepeatableTimestamp,
        page_size: usize,
    ) -> LatestTableIterator<RT> {
        let database = self.clone();
        LatestTableIterator::new(
            self.runtime.clone(),
            min_ts,
            self.reader.clone(),
            self.retention_validator(),
            page_size,
            Arc::new(move || database.now_ts_for_reads()),
        )
    }

    #[fastrace::trace]
    async fn snapshot_table_mapping(
        &self,
        ts: RepeatableTimestamp,
    ) -> anyhow::Result<Arc<TableMapping>> {
        self.table_mapping_snapshot_cache
            .get(&*ts, || self.clone().compute_snapshot_table_mapping(ts))
            .await
    }

    /// The `by_id` ref of a system table, from the latest registry, for reading
    /// the table at any snapshot. These indexes are created at initialization
    /// and never replaced, and a persistence index ID never changes once
    /// assigned, so the latest metadata names the physical index every
    /// snapshot saw; metadata from a snapshot before the ID was assigned would
    /// name none, and a layout that keys rows by it could not read at all.
    fn system_by_id_ref(snapshot: &Snapshot, tablet_id: TabletId) -> anyhow::Result<IndexRef> {
        IndexRef::try_from(snapshot.index_registry.must_get_by_id(tablet_id)?)
    }

    #[fastrace::trace]
    async fn compute_snapshot_table_mapping(
        self,
        ts: RepeatableTimestamp,
    ) -> anyhow::Result<TableMapping> {
        let table_iterator = self.table_iterator(ts, 100);
        let (_, snapshot) = self.snapshot_manager.lock().latest();
        let tables_tablet_id = snapshot
            .table_registry
            .table_mapping()
            .namespace(TableNamespace::Global)
            .id(&TABLES_TABLE)?
            .tablet_id;
        let tables_by_id = Self::system_by_id_ref(&snapshot, tables_tablet_id)?;
        let stream = table_iterator.stream_documents_in_table(tables_tablet_id, tables_by_id, None);
        pin_mut!(stream);
        let mut table_mapping = TableMapping::new();
        while let Some(table_doc) = stream.try_next().await? {
            let table_doc: ParsedDocument<TableMetadata> = table_doc.value.parse()?;
            if table_doc.is_active() {
                table_mapping.insert(
                    TabletId(table_doc.id().internal_id()),
                    table_doc.namespace,
                    table_doc.number,
                    table_doc.into_value().name,
                );
            }
        }
        Ok(table_mapping)
    }

    #[fastrace::trace]
    async fn snapshot_by_id_indexes(
        &self,
        ts: RepeatableTimestamp,
    ) -> anyhow::Result<Arc<BTreeMap<TabletId, IndexRef>>> {
        self.by_id_indexes_snapshot_cache
            .get(&*ts, || self.clone().compute_snapshot_by_id_indexes(ts))
            .await
    }

    #[fastrace::trace]
    async fn compute_snapshot_by_id_indexes(
        self,
        ts: RepeatableTimestamp,
    ) -> anyhow::Result<BTreeMap<TabletId, IndexRef>> {
        let table_iterator = self.table_iterator(ts, 100);
        let (_, snapshot) = self.snapshot_manager.lock().latest();
        let index_tablet_id = snapshot.index_registry.index_table();
        let index_by_id = Self::system_by_id_ref(&snapshot, index_tablet_id)?;
        let stream = table_iterator.stream_documents_in_table(index_tablet_id, index_by_id, None);
        pin_mut!(stream);
        let mut by_id_indexes = BTreeMap::new();
        while let Some(index_doc) = stream.try_next().await? {
            let index_doc = TabletIndexMetadata::from_document(index_doc.value)?;
            if index_doc.name.is_by_id() {
                by_id_indexes.insert(*index_doc.name.table(), IndexRef::try_from(&index_doc)?);
            }
        }
        Ok(by_id_indexes)
    }

    async fn snapshot_component_paths(
        &self,
        ts: RepeatableTimestamp,
    ) -> anyhow::Result<Arc<BTreeMap<ComponentId, ComponentPath>>> {
        self.component_paths_snapshot_cache
            .get(&*ts, || self.clone().compute_snapshot_component_paths(ts))
            .await
    }

    async fn compute_snapshot_component_paths(
        self,
        ts: RepeatableTimestamp,
    ) -> anyhow::Result<BTreeMap<ComponentId, ComponentPath>> {
        let table_iterator = self.table_iterator(ts, 100);
        let (_, snapshot) = self.snapshot_manager.lock().latest();
        let component_tablet_id = snapshot
            .table_registry
            .table_mapping()
            .namespace(TableNamespace::Global)
            .id(&COMPONENTS_TABLE)?
            .tablet_id;
        let component_by_id = Self::system_by_id_ref(&snapshot, component_tablet_id)?;
        let stream =
            table_iterator.stream_documents_in_table(component_tablet_id, component_by_id, None);
        pin_mut!(stream);
        let mut component_docs = Vec::new();
        while let Some(component_doc) = stream.try_next().await? {
            let component_doc: ParsedDocument<ComponentMetadata> = component_doc.value.parse()?;
            component_docs.push(component_doc);
        }
        let component_registry =
            ComponentRegistry::bootstrap(snapshot.table_registry.table_mapping(), component_docs)?;
        let component_paths =
            component_registry.all_component_paths(&mut TransactionReadSet::new());
        Ok(component_paths)
    }

    async fn initialize(rt: &RT, persistence: &mut Arc<dyn Persistence>) -> anyhow::Result<()> {
        let system_tables = bootstrap_system_tables();
        let mut id_generator = TransactionIdGenerator::new(rt)?;
        let ts = rt.generate_timestamp()?;
        let mut creation_time = CreationTime::try_from(ts)?;
        let mut document_writes = vec![];

        let mut system_by_id = BTreeMap::new();
        let mut table_mapping = TableMapping::new();
        let mut next_persistence_index_id = PersistenceIndexId::FIRST.value();

        // Step 0: Generate document ids for bootstrapping database system tables.
        for table in &system_tables {
            let table_name = table.table_name();
            let table_number = *DEFAULT_BOOTSTRAP_TABLE_NUMBERS
                .get(&table_name)
                .context(format!("Table name {table_name} not found"))?;
            let tablet_id = TabletId(id_generator.generate_internal());
            let global_table_mapping = table_mapping.namespace(TableNamespace::Global);
            let existing_tn = global_table_mapping.name_by_number_if_exists(table_number);
            anyhow::ensure!(
                existing_tn.is_none(),
                "{table_number} is used by both {table_name} and {existing_tn:?}"
            );
            anyhow::ensure!(
                table_number < TableNumber::try_from(NUM_RESERVED_SYSTEM_TABLE_NUMBERS)?,
                "{table_number} picked for system table {table_name} is reserved for user tables"
            );
            anyhow::ensure!(
                table_number >= TableNumber::try_from(NUM_RESERVED_LEGACY_TABLE_NUMBERS)?,
                "{table_number} picked for system table {table_name} is reserved for legacy tables"
            );
            table_mapping.insert(
                tablet_id,
                TableNamespace::Global,
                table_number,
                table_name.clone(),
            );
        }

        // Get table ids for tables we will be populating.
        let tables_table_id =
            table_mapping.namespace(TableNamespace::Global).name_to_id()(TABLES_TABLE.clone())?;
        let index_table_id =
            table_mapping.namespace(TableNamespace::Global).name_to_id()(INDEX_TABLE.clone())?;

        persistence
            .write_persistence_global(
                PersistenceGlobalKey::TablesTabletId,
                tables_table_id.tablet_id.to_string().into(),
            )
            .await?;
        persistence
            .write_persistence_global(
                PersistenceGlobalKey::IndexTabletId,
                index_table_id.tablet_id.to_string().into(),
            )
            .await?;

        // Step 1: Generate documents.
        // Create bootstrap system table values.
        for table in &system_tables {
            let table_name = table.table_name();
            let table_id = table_mapping
                .namespace(TableNamespace::Global)
                .id(&table_name)?;
            let document_id = ResolvedDocumentId::new(
                tables_table_id.tablet_id,
                DeveloperDocumentId::new(tables_table_id.table_number, table_id.tablet_id.0),
            );
            let metadata = TableMetadata::new(
                TableNamespace::Global,
                table_name.clone(),
                table_id.table_number,
            );
            let document = ResolvedDocument::new(
                document_id,
                creation_time.increment()?,
                metadata.try_into()?,
            )?;
            document_writes.push((document_id, document));

            // Create the default `by_id` index. Since the table is created just now there
            // is no need to backfill.
            let index_id = id_generator.generate_resolved(index_table_id);
            system_by_id.insert(table_name.clone(), index_id.internal_id());
            let metadata = new_bootstrap_enabled(
                GenericIndexName::by_id(table_id.tablet_id),
                IndexedFields::by_id(),
                &mut next_persistence_index_id,
            );
            let document =
                ResolvedDocument::new(index_id, creation_time.increment()?, metadata.try_into()?)?;
            document_writes.push((index_id, document));

            // Create the `by_creation_time` index for all tables except "_index", which can
            // only have the "by_id" index.
            if table_name != INDEX_TABLE {
                let index_id = id_generator.generate_resolved(index_table_id);
                let metadata = new_bootstrap_enabled(
                    GenericIndexName::by_creation_time(table_id.tablet_id),
                    IndexedFields::creation_time(),
                    &mut next_persistence_index_id,
                );
                let document = ResolvedDocument::new(
                    index_id,
                    creation_time.increment()?,
                    metadata.try_into()?,
                )?;
                document_writes.push((index_id, document));
            }
        }

        // Create system indexes.
        for ErasedSystemIndex { name, fields } in
            system_tables.iter().flat_map(|table| table.indexes())
        {
            let name = name.map_table(
                &table_mapping
                    .namespace(TableNamespace::Global)
                    .name_to_tablet(),
            )?;
            let document_id = id_generator.generate_resolved(index_table_id);
            let index_metadata =
                new_bootstrap_enabled(name, fields, &mut next_persistence_index_id);
            let document = ResolvedDocument::new(
                document_id,
                creation_time.increment()?,
                index_metadata.try_into()?,
            )?;
            document_writes.push((document_id, document));
        }

        let next_id_table_id = table_mapping
            .namespace(TableNamespace::Global)
            .id(&NEXT_PERSISTENCE_INDEX_ID_TABLE)?;
        let next_id_document_id = id_generator.generate_resolved(next_id_table_id);
        let metadata = NextPersistenceIndexIdMetadata {
            next_id: PersistenceIndexId::new(next_persistence_index_id)
                .expect("bootstrap next persistence index ID is nonzero"),
        };
        let document = ResolvedDocument::new(
            next_id_document_id,
            creation_time.increment()?,
            metadata.try_into()?,
        )?;
        document_writes.push((next_id_document_id, document));

        // Step 2: Generate indexes updates.
        // Build the index metadata from the index documents.
        let index_documents = document_writes
            .iter()
            .filter(|(id, _)| id.tablet_id == index_table_id.tablet_id)
            .map(|(_, doc)| (ts, PackedDocument::pack(doc)))
            .collect::<Vec<_>>();
        let mut index_registry = IndexRegistry::bootstrap(
            &table_mapping,
            index_documents.iter().map(|(_, d)| d.clone()),
        )?;
        let mut in_memory_indexes =
            BackendInMemoryIndexes::bootstrap(&index_registry, index_documents, ts)?;

        // Compute the necessary index updates by feeding the remaining documents.
        let mut index_writes = Vec::new();
        for (_id, doc) in &document_writes {
            index_registry.update(None, Some(doc))?;
            let updates = in_memory_indexes.update(&index_registry, ts, None, Some(doc.clone()));
            index_writes.extend(updates);
        }

        // Step 3: Add timestamp and write everything to persistence.
        let ts = Timestamp::MIN;
        let document_writes = document_writes
            .into_iter()
            .map(|(id, doc)| DocumentLogEntry {
                ts,
                id: id.into(),
                value: Some(doc),
                prev_ts: None, // these are all freshly created documents
            })
            .collect_vec();
        let index_writes = index_writes
            .iter()
            .map(|update| PersistenceIndexEntry::from_index_update(ts, update))
            .collect_vec();

        // Write _tables.by_id and _index.by_id to persistence globals for
        // bootstrapping.
        let tables_by_id = *system_by_id
            .get(&TABLES_TABLE)
            .expect("_tables.by_id should exist");
        let index_by_id = *system_by_id
            .get(&INDEX_TABLE)
            .expect("_index.by_id should exist");
        persistence
            .write_persistence_global(
                PersistenceGlobalKey::TablesByIdIndex,
                tables_by_id.to_string().into(),
            )
            .await?;
        persistence
            .write_persistence_global(
                PersistenceGlobalKey::IndexByIdIndex,
                index_by_id.to_string().into(),
            )
            .await?;

        // Write directly to persistence.
        // This is a little unsafe because we generated random IDs for this documents
        // with `TransactionIdGenerator`, but aren't using a real `Transaction` so we
        // don't have our usual protections against ID collisions.
        // Our `ConflictStrategy::Error` should notice the problem but consider
        // improving in the future (CX-2265).
        persistence
            .write(
                document_writes.as_slice(),
                index_writes.as_slice(),
                ConflictStrategy::Error,
            )
            .await?;
        Ok(())
    }

    pub fn persistence_version(&self) -> PersistenceVersion {
        self.reader.version()
    }

    /// The ref for an index a funrun named by `_index` document ID alone,
    /// which only a funrun predating the persistence ID on the wire does. The
    /// registry at the snapshot supplies the ID, or the latest registry once
    /// that snapshot left memory; the two agree because an assigned ID never
    /// changes. An unknown index gets none; a text or vector index is an error.
    fn index_ref_at(
        &self,
        snapshot: Option<&Snapshot>,
        index_id: IndexId,
    ) -> anyhow::Result<IndexRef> {
        let latest;
        let registry = match snapshot {
            Some(snapshot) => &snapshot.index_registry,
            None => {
                latest = self.snapshot_manager.lock().latest().1;
                &latest.index_registry
            },
        };
        registry
            .enabled_index_by_index_id(&index_id)
            .or_else(|| registry.pending_index_by_index_id(&index_id))
            .map(IndexRef::try_from)
            .unwrap_or(Ok(IndexRef::unresolved(index_id)))
    }

    /// Scan a page of the index, checking in-memory indexes and the index cache
    /// first and falling back to the persistence reader.
    ///
    /// The read is not attached to any transaction. Reads inside of a
    /// transaction should go through `DatabaseIndexSnapshot` instead.
    pub async fn index_page(
        &self,
        ts: RepeatableTimestamp,
        index: IndexRef,
        tablet_id: TabletId,
        interval: &Interval,
        order: Order,
        max_size: usize,
    ) -> anyhow::Result<(
        Vec<(IndexKeyBytes, Timestamp, PackedDocument)>,
        CursorPosition,
    )> {
        let snapshot = {
            let snapshot_manager = self.snapshot_manager.lock();
            if *ts < snapshot_manager.earliest_ts() {
                None
            } else {
                Some(snapshot_manager.snapshot(*ts)?)
            }
        };
        // Try to serve from in-memory indexes.
        if let Some(snapshot) = &snapshot
            && let Some(range) = snapshot
                .in_memory_indexes
                .range(index.id(), interval, order)?
        {
            let results = range
                .into_iter()
                .take(max_size)
                .map(|(key, ts, doc)| (key, ts, doc.packed().clone()))
                .collect::<Vec<_>>();
            let cursor = if results.len() >= max_size {
                CursorPosition::After(results.last().unwrap().0.clone())
            } else {
                CursorPosition::End
            };
            Ok((results, cursor))
        } else {
            let persistence_snapshot =
                RepeatablePersistence::new(self.reader.clone(), ts, self.retention_validator())
                    .read_snapshot(ts)?;
            let index = if index.persistence_index_id().is_some() {
                index
            } else {
                self.index_ref_at(snapshot.as_ref(), index.id())?
            };
            let mut reader = Arc::new(persistence_snapshot) as Arc<dyn IndexReader>;
            if let Some(snapshot) = snapshot {
                // TODO: it is probably OK to still use the index cache even if
                // we don't have an index_registry at the same timestamp.
                reader = Arc::new(
                    self.index_cache_handle
                        .clone()
                        .caching_index_reader(reader, snapshot.index_registry),
                );
            }
            let index_page = reader
                .index_page(index, tablet_id, interval, order, max_size)
                .await?;
            let results = index_page
                .entries
                .into_iter()
                .map(|entry| {
                    let IndexEntry { key, ts, value } = Arc::unwrap_or_clone(entry);
                    (key, ts, value)
                })
                .collect();
            Ok((results, index_page.cursor))
        }
    }

    pub fn now_ts_for_reads(&self) -> RepeatableTimestamp {
        let snapshot_manager = self.snapshot_manager.lock();
        snapshot_manager.latest_ts()
    }

    /// Waits until the given write timestamp is observable for new
    /// transactions. Used by retry loops to ensure they observe the write.
    pub async fn wait_for_write_ts(&self, write_ts: Timestamp) {
        // Wait for the snapshot manager to advance past write_ts, so that writes
        // at the given timestamp are observable via now_ts_for_reads().
        if let Ok(pred) = write_ts.pred() {
            let fut = self.snapshot_manager.lock().wait_for_higher_ts(pred);
            fut.await;
        }
    }

    pub async fn begin_system(&self) -> anyhow::Result<Transaction<RT>> {
        self.begin(Identity::system()).await
    }

    pub async fn execute_with_retries<'a, T, R, F>(
        &'a self,
        identity: Identity,
        max_failures: u32,
        mut backoff: Backoff,
        usage: FunctionUsageTracker,
        is_retriable: R,
        write_source: impl Into<WriteSource>,
        f: F,
    ) -> anyhow::Result<(Timestamp, T, OccRetryStats)>
    where
        T: Send,
        R: Fn(&Error) -> bool,
        F: for<'b> Fn(&'b mut Transaction<RT>) -> ShortBoxFuture<'b, 'a, anyhow::Result<T>>,
    {
        let write_source = write_source.into();
        let mut error = None;
        while backoff.failures() < max_failures {
            let mut tx = self
                .begin_with_usage(identity.clone(), usage.clone())
                .await?;
            let pause_client = self.runtime.pause_client();
            pause_client.wait("retry_tx_loop_start").await;
            let start = Instant::now();
            let result = async {
                let t = f(&mut tx).0.await?;
                let func_end_time = Instant::now();
                let ts = self
                    .commit_with_write_source(tx, write_source.clone())
                    .await?;
                let commit_end_time = Instant::now();
                Ok((ts, t, func_end_time, commit_end_time))
            }
            .await;
            let total_duration = Instant::now() - start;
            match result {
                Err(e) => {
                    if is_retriable(&e) {
                        let delay = backoff.fail(&mut self.runtime.rng());
                        tracing::warn!(
                            "Retrying transaction `{write_source:?}` after error: {e:#}"
                        );
                        self.runtime.wait(delay).await;
                        if let Some(write_ts_raw) = e.occ_write_ts()
                            && let Ok(write_ts) = Timestamp::try_from(write_ts_raw)
                        {
                            self.wait_for_write_ts(write_ts).await;
                        }
                        error = Some(e);
                        continue;
                    } else {
                        return Err(e);
                    }
                },
                Ok((ts, t, func_end_time, commit_end_time)) => {
                    return Ok((
                        ts,
                        t,
                        OccRetryStats {
                            retries: backoff.failures(),
                            total_duration,
                            duration: func_end_time - start,
                            commit_duration: commit_end_time - func_end_time,
                        },
                    ))
                },
            }
        }
        let error = error.unwrap_or_else(|| anyhow::anyhow!("Error was not returned from commit"));
        tracing::warn!(
            "Giving up on retrying transaction `{write_source:?}` after {max_failures} failures"
        );
        Err(error)
    }

    pub async fn execute_with_occ_retries<'a, T, F>(
        &'a self,
        identity: Identity,
        usage: FunctionUsageTracker,
        max_failures: u32,
        write_source: impl Into<WriteSource>,
        f: F,
    ) -> anyhow::Result<(Timestamp, T, OccRetryStats)>
    where
        T: Send,
        F: for<'b> Fn(&'b mut Transaction<RT>) -> ShortBoxFuture<'b, 'a, anyhow::Result<T>>,
    {
        let backoff = Backoff::new(INITIAL_OCC_BACKOFF, MAX_OCC_BACKOFF);
        let is_retriable = |e: &Error| e.is_occ();
        self.execute_with_retries(
            identity,
            max_failures,
            backoff,
            usage,
            is_retriable,
            write_source,
            f,
        )
        .await
    }

    /// When the database is overloaded,
    /// sometimes it takes a while to clear up. As a rule of thumb, use this
    /// method if it's okay to wait for a search index to backfill.
    /// Also retries if it hits OCCs.
    pub async fn execute_with_overloaded_retries<'a, T, F>(
        &'a self,
        identity: Identity,
        usage: FunctionUsageTracker,
        write_source: impl Into<WriteSource>,
        f: F,
    ) -> anyhow::Result<(Timestamp, T, OccRetryStats)>
    where
        T: Send,
        F: for<'b> Fn(&'b mut Transaction<RT>) -> ShortBoxFuture<'b, 'a, anyhow::Result<T>>,
    {
        let backoff = Backoff::new(INITIAL_OVERLOADED_BACKOFF, MAX_OVERLOADED_BACKOFF);
        let is_retriable = |e: &Error| e.is_occ() || e.is_overloaded();
        self.execute_with_retries(
            identity,
            MAX_OVERLOADED_FAILURES,
            backoff,
            usage,
            is_retriable,
            write_source,
            f,
        )
        .await
    }

    /// Like `execute_with_overloaded_retries`, but also checks the write
    /// throughput limit before each attempt. If the limit is exceeded, retries
    /// indefinitely with exponential backoff.
    pub async fn execute_with_overloaded_and_ratelimited_retries<'a, T, F>(
        &'a self,
        identity: Identity,
        usage: FunctionUsageTracker,
        write_source: impl Into<WriteSource>,
        f: F,
    ) -> anyhow::Result<(Timestamp, T, OccRetryStats)>
    where
        T: Send,
        F: for<'b> Fn(&'b mut Transaction<RT>) -> ShortBoxFuture<'b, 'a, anyhow::Result<T>>,
    {
        let mut rate_limit_backoff =
            Backoff::new(INITIAL_OVERLOADED_BACKOFF, MAX_OVERLOADED_BACKOFF);
        loop {
            match self.check_write_throughput_limit() {
                Ok(()) => break,
                Err(e) if e.is_rate_limited() => {
                    let delay = rate_limit_backoff.fail(&mut self.runtime.rng());
                    tracing::warn!("Write throughput limit exceeded, retrying after {delay:?}",);
                    self.runtime.wait(delay).await;
                },
                Err(e) => return Err(e),
            }
        }
        self.execute_with_overloaded_retries(identity, usage, write_source, f)
            .await
    }

    pub async fn begin(&self, identity: Identity) -> anyhow::Result<Transaction<RT>> {
        self.begin_with_usage(identity, FunctionUsageTracker::new())
            .await
    }

    pub async fn begin_with_usage(
        &self,
        identity: Identity,
        usage: FunctionUsageTracker,
    ) -> anyhow::Result<Transaction<RT>> {
        let ts = self.now_ts_for_reads();
        self.begin_with_repeatable_ts(identity, ts, usage, None)
            .await
    }

    pub async fn begin_with_ts(
        &self,
        identity: Identity,
        ts: Timestamp,
        usage_tracker: FunctionUsageTracker,
    ) -> anyhow::Result<Transaction<RT>> {
        let ts = {
            let snapshot_manager = self.snapshot_manager.lock();
            snapshot_manager.latest_ts().prior_ts(ts)?
        };
        self.begin_with_repeatable_ts(identity, ts, usage_tracker, None)
            .await
    }

    async fn begin_with_repeatable_ts(
        &self,
        identity: Identity,
        repeatable_ts: RepeatableTimestamp,
        usage_tracker: FunctionUsageTracker,
        index_cache: Option<TimestampedIndexCache>,
    ) -> anyhow::Result<Transaction<RT>> {
        task::consume_budget().await;

        let latest_ts = self.now_ts_for_reads();
        if repeatable_ts > latest_ts {
            anyhow::bail!(
                "Timestamp {} beyond now_ts_for_reads {}",
                repeatable_ts,
                latest_ts
            );
        }
        let snapshot = self.snapshot_manager.lock().snapshot(*repeatable_ts)?;

        // TODO: Use `begin_ts` outside of just the "_creationTime".
        let begin_ts = cmp::max(latest_ts.succ()?, self.runtime.generate_timestamp()?);
        let creation_time = CreationTime::for_transaction(*repeatable_ts, begin_ts)?;
        let id_generator = TransactionIdGenerator::new(&self.runtime)?;
        let transaction_index = TransactionIndex::new(
            snapshot.index_registry.clone(),
            DatabaseIndexSnapshot::new(
                snapshot.index_registry.clone(),
                Arc::new(snapshot.in_memory_indexes),
                snapshot.table_registry.table_mapping().clone(),
                Arc::new(
                    RepeatablePersistence::new(
                        self.reader.clone(),
                        repeatable_ts,
                        self.retention_manager.clone(),
                    )
                    .read_snapshot(repeatable_ts)?,
                ),
                Some(self.index_cache_handle.clone()),
                index_cache,
            ),
            Arc::new(TextIndexManagerSnapshot::new(
                snapshot.index_registry,
                snapshot.text_indexes,
                self.searcher.clone(),
                self.search_storage.clone(),
            )),
        );
        let count_snapshot = Arc::new(snapshot.table_counts);
        let tx = Transaction::new(
            identity,
            id_generator,
            creation_time,
            transaction_index,
            snapshot.table_registry,
            snapshot.schema_registry,
            snapshot.component_registry,
            count_snapshot,
            self.runtime.clone(),
            usage_tracker,
            self.virtual_system_mapping.clone(),
        );
        Ok(tx)
    }

    pub async fn begin_with_index_cache(
        &self,
        identity: Identity,
        index_cache: TimestampedIndexCache,
    ) -> anyhow::Result<Transaction<RT>> {
        self.begin_with_repeatable_ts(
            identity,
            index_cache.ts,
            FunctionUsageTracker::new(),
            Some(index_cache),
        )
        .await
    }

    pub fn snapshot(&self, ts: RepeatableTimestamp) -> anyhow::Result<Snapshot> {
        self.snapshot_manager.lock().snapshot(*ts)
    }

    pub fn latest_snapshot(&self) -> anyhow::Result<Snapshot> {
        let snapshot = self.snapshot_manager.lock().latest_snapshot();
        Ok(snapshot)
    }

    pub fn latest_ts_and_snapshot(&self) -> anyhow::Result<(RepeatableTimestamp, Snapshot)> {
        Ok(self.snapshot_manager.lock().latest())
    }

    pub fn latest_database_snapshot(&self) -> anyhow::Result<DatabaseSnapshot<RT>> {
        let (ts, snapshot) = self.snapshot_manager.lock().latest();
        let repeatable_persistence =
            RepeatablePersistence::new(self.reader.clone(), ts, self.retention_validator());
        Ok(DatabaseSnapshot {
            runtime: self.runtime.clone(),
            ts,
            bootstrap_metadata: self.bootstrap_metadata.clone(),
            snapshot,
            table_summaries: None,
            persistence_snapshot: repeatable_persistence.read_snapshot(ts)?,
            index_cache_handle: Some(self.index_cache_handle.clone()),
            persistence_reader: self.reader.clone(),
            retention_validator: self.retention_validator(),
        })
    }

    pub fn check_write_throughput_limit(&self) -> anyhow::Result<()> {
        let ts = self.runtime.generate_timestamp()?;
        self.snapshot_manager
            .lock()
            .check_write_throughput_limit(ts)
    }

    #[fastrace::trace]
    pub async fn commit_with_write_source(
        &self,
        transaction: Transaction<RT>,
        write_source: impl Into<WriteSource>,
    ) -> anyhow::Result<Timestamp> {
        task::consume_budget().await;
        let readonly = transaction.is_readonly();
        let result = self
            .committer
            .commit(transaction, write_source.into())
            .await?;
        if !readonly {
            self.write_commits_since_load.fetch_add(1, Ordering::SeqCst);
        }
        Ok(result)
    }

    #[fastrace::trace]
    pub async fn load_indexes_into_memory(
        &self,
        tables: BTreeSet<TableName>,
    ) -> anyhow::Result<()> {
        self.committer.load_indexes_into_memory(tables).await
    }

    pub fn write_commits_since_load(&self) -> usize {
        self.write_commits_since_load.load(Ordering::SeqCst)
    }

    // TODO: consider making this function non-async
    /// N.B. Only use this function for user subscriptions. System subscriptions
    /// should use `subscribe_and_wait_for_invalidation`.
    pub async fn subscribe(&self, token: Token) -> anyhow::Result<Subscription> {
        self.subscriptions.subscribe(token, false)
    }

    pub async fn subscribe_and_wait_for_invalidation(
        &self,
        token: Token,
    ) -> anyhow::Result<Option<Timestamp>> {
        let subscription = self.subscriptions.subscribe(token, true)?;
        let invalid_ts = subscription.wait_for_invalidation().await;
        let current_ts = self.now_ts_for_reads();
        metrics::log_subscription_invalidation_lag(invalid_ts, *current_ts);
        Ok(invalid_ts)
    }

    /// Whether the legacy `document_deltas` / `list_snapshot` APIs export
    /// `tablet_id`.
    ///
    /// We allow snapshot imports to be streamed by default.
    /// Note this behavior is kind of odd for `--require-empty` imports
    /// because the rows are streamed before they are committed to Convex,
    /// and it's very strange for `--replace` imports because the imported
    /// rows are merged with existing rows.
    fn legacy_streaming_export_table_filter(
        selection: &StreamingExportSelection,
        tablet_id: TabletId,
        table_mapping: &TableMapping,
        component_paths: &BTreeMap<ComponentId, ComponentPath>,
    ) -> anyhow::Result<bool> {
        if !table_mapping.id_exists(tablet_id) {
            // Always exclude deleted tablets.
            return Ok(false);
        }
        if table_mapping.is_system_tablet(tablet_id) {
            return Ok(false);
        }

        let (table_namespace, _, table_name) = table_mapping
            .get_table_metadata(tablet_id)
            .with_context(|| format!("Can’t find the table entry for the tablet id {tablet_id}"))?;

        let Some(component_path) = component_paths.get(&ComponentId::from(*table_namespace)) else {
            tracing::warn!("Ignoring orphaned table in streaming export: {table_namespace:?}");
            return Ok(false);
        };

        Ok(selection.is_table_included(component_path, table_name))
    }

    #[fastrace::trace]
    pub async fn document_deltas(
        &self,
        identity: Identity,
        cursor: Option<Timestamp>,
        selection: StreamingExportSelection,
        rows_read_limit: usize,
        rows_returned_limit: usize,
    ) -> anyhow::Result<DocumentDeltas> {
        let usage = FunctionUsageTracker::new();

        anyhow::ensure!(
            identity.is_system() || identity.is_admin(),
            unauthorized_error("document_deltas")
        );
        anyhow::ensure!(rows_read_limit >= rows_returned_limit);
        let (upper_bound, table_mapping, component_paths) = {
            let mut tx = self.begin(identity).await?;
            (
                tx.begin_timestamp(),
                tx.table_mapping().clone(),
                BootstrapComponentsModel::new(&mut tx).all_component_paths(),
            )
        };
        let repeatable_persistence = RepeatablePersistence::new(
            self.reader.clone(),
            upper_bound,
            self.retention_validator(),
        );
        let range = match cursor {
            Some(ts) => TimestampRange::new((Bound::Excluded(ts), Bound::Unbounded)),
            None => TimestampRange::all(),
        };
        let mut document_stream = repeatable_persistence.load_documents(range, Order::Asc);
        // deltas accumulated in (ts, id) order to return.
        let mut deltas = vec![];
        // new_cursor is set once, when we know the final timestamp.
        let mut new_cursor = None;
        // has_more indicates there are more documents in the stream so the caller
        // should request another page.
        let mut has_more = false;
        let mut rows_read = 0;
        while let Some(DocumentLogEntry {
            ts,
            id,
            value: maybe_doc,
            ..
        }) = match document_stream.try_next().await {
            Ok::<_, Error>(doc) => doc,
            Err(e) if e.is_out_of_retention() => {
                // Throws a user error if the documents window is out of retention
                anyhow::bail!(ErrorMetadata::bad_request(
                    "InvalidWindowToReadDocuments",
                    format!(
                        "Trying to synchronize from timestamp {}, which is older than the \
                         database’s retention window. This may happen if you paused your \
                         Fivetran or Airbyte connector for a long period of time. Please perform \
                         a full sync of the connector. See \
                         https://fivetran.com/docs/connectors/troubleshooting/trigger-historical-re-syncs or \
                         https://docs.airbyte.com/platform/operator-guides/refreshes",
                        range.min_timestamp_inclusive()
                    )
                ))
            },
            Err(e) => anyhow::bail!(e),
        } {
            rows_read += 1;
            if let Some(new_cursor) = new_cursor
                && new_cursor < ts
            {
                // If we determined new_cursor already, we know the maximum ts we want to
                // return. So if we read a document with a higher ts, we are
                // done.
                has_more = true;
                break;
            }
            if new_cursor.is_none() && rows_read >= rows_read_limit {
                // We want to finish, but we have to process all documents at this timestamp.
                new_cursor = Some(ts);
            }
            // Skip deltas for system and non-selected tables.
            if Self::legacy_streaming_export_table_filter(
                &selection,
                id.table(),
                &table_mapping,
                &component_paths,
            )? {
                let table_number = table_mapping.tablet_number(id.table())?;
                let table_name = table_mapping.tablet_name(id.table())?;
                let component_id = ComponentId::from(table_mapping.tablet_namespace(id.table())?);
                let component_path = component_paths
                    .get(&component_id)
                    .cloned()
                    .unwrap_or_else(ComponentPath::root);
                let id = DeveloperDocumentId::new(table_number, id.internal_id());
                let column_filter = selection.column_filter(&component_path, &table_name)?;
                let filtered_doc = maybe_doc
                    .map(|doc| column_filter.filter_document(doc.to_developer()))
                    .transpose()?;

                // Track database egress for the document
                if let Some(ref doc) = filtered_doc {
                    let doc_size = doc.size();
                    usage.track_database_egress_v2(
                        component_path.clone(),
                        &table_name,
                        doc_size as u64,
                        false,
                    );
                    usage.track_database_egress_rows(component_path.clone(), &table_name, 1, false);
                }

                deltas.push((ts, id, component_path, table_name, filtered_doc));
                if new_cursor.is_none() && deltas.len() >= rows_returned_limit {
                    // We want to finish, but we have to process all documents at this timestamp.
                    new_cursor = Some(ts);
                }
            }
        }
        metrics::log_document_deltas_read_documents(rows_read);
        metrics::log_document_deltas_returned_documents(deltas.len());
        Ok(DocumentDeltas {
            deltas,
            // If new_cursor is still None, we exhausted the stream.
            cursor: new_cursor.unwrap_or(*upper_bound),
            has_more,
            usage: usage.gather_user_stats(),
        })
    }

    #[fastrace::trace]
    pub async fn list_snapshot(
        &self,
        identity: Identity,
        snapshot: Option<Timestamp>,
        cursor: Option<ResolvedDocumentId>,
        selection: StreamingExportSelection,
        rows_read_limit: usize,
        rows_returned_limit: usize,
    ) -> anyhow::Result<SnapshotPage> {
        let usage = FunctionUsageTracker::new();

        anyhow::ensure!(
            identity.is_system() || identity.is_admin(),
            unauthorized_error("list_snapshot")
        );
        anyhow::ensure!(rows_read_limit >= rows_returned_limit);
        let now = self.now_ts_for_reads();
        let snapshot = match snapshot {
            Some(ts) => {
                let ts = now.prior_ts(ts).with_context(|| {
                    ErrorMetadata::bad_request(
                        "SnapshotTooNew",
                        format!("Snapshot value {ts} is in the future."),
                    )
                })?;
                anyhow::ensure!(
                    *now - *ts <= *LIST_SNAPSHOT_MAX_AGE_SECS,
                    ErrorMetadata::bad_request(
                        "SnapshotTooOld",
                        format!("Snapshot value {ts} is too far in the past."),
                    )
                );
                ts
            },
            None => self.now_ts_for_reads(),
        };
        let (table_mapping, by_id_indexes, component_paths) = tokio::try_join!(
            self.snapshot_table_mapping(snapshot),
            self.snapshot_by_id_indexes(snapshot),
            self.snapshot_component_paths(snapshot)
        )?;
        let mut tablet_ids: Vec<_> = table_mapping
            .iter()
            .map(|(tablet_id, ..)| tablet_id)
            .filter_map(|tablet_id| {
                let has_table_already_been_treated = cursor
                    .as_ref()
                    .map(|c| tablet_id < c.tablet_id)
                    .unwrap_or(false);
                if has_table_already_been_treated {
                    return None;
                }

                match Self::legacy_streaming_export_table_filter(
                    &selection,
                    tablet_id,
                    &table_mapping,
                    &component_paths,
                ) {
                    Ok(true) => Some(Ok(tablet_id)),
                    Ok(false) => None,
                    Err(e) => Some(Err(e)),
                }
            })
            .try_collect()?;
        tablet_ids.sort_unstable();
        let tablet_id = match tablet_ids.first() {
            Some(first_table) => *first_table,
            None => {
                return Ok(SnapshotPage {
                    documents: vec![],
                    snapshot: *snapshot,
                    cursor: None,
                    has_more: false,
                    usage: usage.gather_user_stats(),
                });
            },
        };
        let by_id = *by_id_indexes
            .get(&tablet_id)
            .ok_or_else(|| anyhow::anyhow!("by_id index for {tablet_id:?} missing"))?;
        let mut document_stream = {
            let expected_cache_key = ListSnapshotTableIteratorCacheKey {
                snapshot: *snapshot,
                tablet_ids: tablet_ids.clone(),
                by_id,
                cursor,
            };
            // NOTE: this implies that only one list_snapshot call can be initializing at a
            // time, which is de-facto true anyway
            let mut cache = self.list_snapshot_table_iterator_cache.lock().await;
            let iterator_future = match *cache {
                Some(ref mut entry) if entry.cache_key == expected_cache_key => {
                    tracing::info!("Resuming cached list_snapshot iterator");
                    &mut entry.iterator
                },
                _ => {
                    if let Fault::Error(e) = self
                        .runtime
                        .pause_client()
                        .wait("list_snapshot_new_iterator")
                        .await
                    {
                        return Err(e);
                    }
                    tracing::info!(
                        ?snapshot,
                        ?tablet_ids,
                        ?cursor,
                        "Initializing a new list_snapshot iterator"
                    );
                    let table_iterator =
                        self.table_iterator(snapshot, 100).multi(tablet_ids.clone());
                    &mut cache
                        .insert(ListSnapshotTableIteratorCacheEntry {
                            cache_key: expected_cache_key,
                            iterator: AbortOnDropHandle::new(tokio_spawn(
                                "init_list_snapshot_iterator",
                                async move {
                                    let mut stream = Box::pin(
                                        table_iterator
                                            .into_stream_documents_in_table(
                                                tablet_id, by_id, cursor,
                                            )
                                            .peekable(),
                                    );
                                    // Wait for the iterator to initialize by peeking the first
                                    // element
                                    _ = stream.as_mut().peek().await;
                                    tracing::info!("Finished initializing list_snapshot iterator");
                                    stream as _
                                },
                            ))
                            .map_err(|e| anyhow::anyhow!(e))
                            .boxed(),
                        })
                        .iterator
                },
            };
            let iterator = iterator_future.await;
            // Always clear the cache if `iterator_future` yielded to avoid panicking on
            // polling after completion
            *cache = None;
            iterator?
        };

        enum PageResult<RT: Runtime> {
            /// Continue iterating the same table. `document_stream` can return
            /// more values.
            ContinueTable {
                new_cursor: ResolvedDocumentId,
                document_stream: BoxStream<
                    'static,
                    anyhow::Result<Either<LatestDocument, MultiTableIterator<RT>>>,
                >,
            },
            /// `tablet_id` is done, and `document_stream` has returned the
            /// MultiTableIterator. There may or may not be more
            /// tables.
            TableDone(MultiTableIterator<RT>),
        }
        // documents accumulated in (ts, id) order to return.
        let mut documents = vec![];
        let mut rows_read = 0;
        let start_time = Instant::now();
        let page_result = loop {
            let LatestDocument { ts, value: doc, .. } =
                match document_stream.try_next().await?.context(
                    "into_stream_documents_in_table stream ended without returning the iterator",
                )? {
                    Either::Left(rev) => rev,
                    Either::Right(mut iterator) => {
                        drop(document_stream);
                        iterator.unregister_table(tablet_id)?;
                        break PageResult::TableDone(iterator);
                    },
                };
            rows_read += 1;
            let id = doc.id();
            anyhow::ensure!(id.tablet_id == tablet_id);
            let table_name = table_mapping.tablet_name(id.tablet_id)?;
            let namespace = table_mapping.tablet_namespace(id.tablet_id)?;
            let component_id = ComponentId::from(namespace);
            let component_path = component_paths
                .get(&component_id)
                .cloned()
                .unwrap_or_else(ComponentPath::root);
            let column_filter = selection.column_filter(&component_path, &table_name)?;

            let filtered_doc = column_filter.filter_document(doc.to_developer())?;

            // Track database egress for the document
            let doc_size = filtered_doc.size();
            usage.track_database_egress_v2(
                component_path.clone(),
                &table_name,
                doc_size as u64,
                false,
            );
            usage.track_database_egress_rows(component_path.clone(), &table_name, 1, false);

            documents.push((ts, component_path, table_name, filtered_doc));
            if rows_read >= rows_read_limit
                || documents.len() >= rows_returned_limit
                || start_time.elapsed() > *SNAPSHOT_LIST_TIME_LIMIT
            {
                break PageResult::ContinueTable {
                    new_cursor: id,
                    document_stream,
                };
            }
        };
        let (new_cursor, next_cache_kv) = match page_result {
            PageResult::ContinueTable {
                new_cursor,
                document_stream,
            } => (
                Some(new_cursor),
                Some(ListSnapshotTableIteratorCacheEntry {
                    cache_key: ListSnapshotTableIteratorCacheKey {
                        snapshot: *snapshot,
                        tablet_ids,
                        by_id,
                        cursor: Some(new_cursor),
                    },
                    iterator: Box::pin(async move { Ok(document_stream) }),
                }),
            ),
            PageResult::TableDone(table_iterator) => match tablet_ids.get(1) {
                Some(&next_tablet_id) => {
                    let next_by_id = *by_id_indexes.get(&next_tablet_id).ok_or_else(|| {
                        anyhow::anyhow!("by_id index for {next_tablet_id:?} missing")
                    })?;
                    let next_cursor =
                        ResolvedDocumentId::new(next_tablet_id, DeveloperDocumentId::MIN);
                    let next_document_stream = table_iterator
                        .into_stream_documents_in_table(
                            next_tablet_id,
                            next_by_id,
                            Some(next_cursor),
                        )
                        .boxed();
                    (
                        Some(next_cursor),
                        Some(ListSnapshotTableIteratorCacheEntry {
                            cache_key: ListSnapshotTableIteratorCacheKey {
                                snapshot: *snapshot,
                                tablet_ids: tablet_ids[1..].to_vec(),
                                by_id: next_by_id,
                                cursor: Some(next_cursor),
                            },
                            iterator: Box::pin(async move { Ok(next_document_stream) }),
                        }),
                    )
                },
                None => (None, None),
            },
        };
        if let Some(kv) = next_cache_kv {
            *self.list_snapshot_table_iterator_cache.lock().await = Some(kv);
        }
        let has_more = new_cursor.is_some();
        metrics::log_list_snapshot_page_documents(documents.len());
        Ok(SnapshotPage {
            documents,
            snapshot: *snapshot,
            cursor: new_cursor,
            has_more,
            usage: usage.gather_user_stats(),
        })
    }

    /// Attempt to pull a token forward to a given timestamp, returning `Err`
    /// if there have been overlapping writes between the token's original
    /// timestamp and `ts`.
    pub async fn refresh_token(
        &self,
        token: Token,
        ts: Timestamp,
    ) -> anyhow::Result<Result<Token, Option<Timestamp>>> {
        let _timer = metrics::refresh_token_timer();
        self.log.refresh_token(token, ts)
    }

    pub fn log(&self) -> &LogReader {
        &self.log
    }

    pub fn memory_consistency_check(&self) -> anyhow::Result<()> {
        let snapshot = self.snapshot_manager.lock().latest_snapshot();
        snapshot.text_indexes.consistency_check()?;
        Ok(())
    }

    pub fn has_table_counts_bootstrapped(&self) -> bool {
        self.snapshot_manager
            .lock()
            .latest_snapshot()
            .table_counts
            .is_some()
    }

    pub fn search_storage(&self) -> Arc<dyn Storage> {
        self.search_storage
            .get()
            .expect("search_storage not initialized")
            .clone()
    }

    pub async fn vector_search(
        &self,
        _identity: Identity,
        query: VectorSearch,
    ) -> anyhow::Result<(Vec<PublicVectorSearchQueryResult>, FunctionUsageStats)> {
        let mut last_error = None;
        let mut backoff = Backoff::new(INITIAL_VECTOR_BACKOFF, MAX_VECTOR_BACKOFF);
        let timer = vector_search_with_retries_timer();
        while backoff.failures() < MAX_VECTOR_ATTEMPTS {
            let ts = self.now_ts_for_reads();
            match self.vector_search_at_ts(query.clone(), ts).await {
                Err(e) => {
                    // If backend hasn't loaded the in-memory index yet, it returns
                    // overloaded. We want to retry those.
                    if e.is_overloaded() {
                        let delay = backoff.fail(&mut self.runtime.rng());
                        last_error = Some(e);
                        if backoff.failures() >= MAX_VECTOR_ATTEMPTS {
                            break;
                        }
                        tracing::warn!(
                            "Retrying vector search error: {}",
                            last_error.as_ref().unwrap()
                        );
                        self.runtime.wait(delay).await;
                        continue;
                    } else {
                        timer.finish(false);
                        return Err(e);
                    }
                },
                Ok(result) => {
                    timer.finish(true);
                    return Ok(result);
                },
            }
        }
        let last_error = last_error.expect("Exited vector_search() loop without any failure");
        timer.finish(false);
        Err(last_error)
    }

    pub async fn vector_search_at_ts(
        &self,
        query: VectorSearch,
        ts: RepeatableTimestamp,
    ) -> anyhow::Result<(Vec<PublicVectorSearchQueryResult>, FunctionUsageStats)> {
        let timer = metrics::vector::vector_search_timer();
        let usage = FunctionUsageTracker::new();
        let snapshot = self.snapshot(ts)?;
        let component_id = query.component_id;
        let table_mapping = snapshot
            .table_mapping()
            .namespace(TableNamespace::from(component_id));
        if !table_mapping.name_exists(query.index_name.table()) {
            return Ok((vec![], usage.gather_user_stats()));
        }
        let table_number = table_mapping.id(query.index_name.table())?.table_number;
        let index_name = query
            .index_name
            .clone()
            .to_resolved(table_mapping.name_to_tablet())?;
        let index = snapshot
            .index_registry
            .require_enabled(&index_name, &query.index_name)?;
        let index_name = query.index_name.clone();
        let index_size = index.metadata().config.estimate_pricing_size_bytes()?;
        let resolved: vector::InternalVectorSearch = query.resolve(&table_mapping)?;
        let search_storage = self.search_storage();
        let results: Vec<_> = snapshot
            .vector_indexes
            .vector_search(
                &index,
                resolved,
                self.searcher.clone(),
                search_storage.clone(),
            )
            .await?
            .into_iter()
            .map(|r| r.to_public(table_number))
            .collect();
        let size: u64 = results.iter().map(|row| row.size() as u64).sum();
        let component_path = snapshot
            .component_registry
            .must_component_path(component_id, &mut TransactionReadSet::new())?;
        usage.track_vector_egress(component_path.clone(), index_name.table(), size);
        usage.track_vector_query(component_path, index_name, index_size);
        timer.finish();
        Ok((results, usage.gather_user_stats()))
    }

    pub async fn text_search_at_ts(
        &self,
        index_id: IndexId,
        printable_index_name: IndexName,
        query: pb::searchlight::TextQuery,
        pending_updates: Vec<DocumentUpdate>,
        ts: RepeatableTimestamp,
    ) -> anyhow::Result<TextSearchResults> {
        let snapshot = self.snapshot(ts)?;
        let index = snapshot
            .index_registry
            .enabled_index_by_index_id(&index_id)
            .with_context(|| format!("Missing index_id {index_id:?}"))?
            .clone();

        let search_snapshot = TextIndexManagerSnapshot::new(
            snapshot.index_registry,
            snapshot.text_indexes,
            self.searcher.clone(),
            self.search_storage.clone(),
        );

        search_snapshot
            .text_search(&index, &printable_index_name, query, &pending_updates)
            .await
    }

    pub fn runtime(&self) -> &RT {
        &self.runtime
    }

    /// Subscribe to be woken when an in-memory index of `search_type` grows
    /// past its soft size limit and needs flushing.
    pub fn subscribe_search_flusher_wake(
        &self,
        search_type: SearchType,
    ) -> SearchFlusherWakeSubscriber {
        self.committer.search_flusher_wake().subscribe(search_type)
    }
}

fn new_bootstrap_enabled<T: IndexTableIdentifier>(
    name: GenericIndexName<T>,
    fields: IndexedFields,
    next_persistence_index_id: &mut u32,
) -> IndexMetadata<T> {
    let mut metadata = IndexMetadata::new_enabled(name, fields);
    let persistence_index_id = PersistenceIndexId::new(*next_persistence_index_id)
        .expect("bootstrap persistence index IDs are nonzero");
    *next_persistence_index_id = next_persistence_index_id
        .checked_add(1)
        .expect("bootstrap persistence index IDs cannot exhaust u32");
    metadata.assign_persistence_index_id(persistence_index_id);
    metadata
}

/// Transaction statistics reported for a retried transaction
#[derive(Debug, PartialEq, Eq)]
pub struct OccRetryStats {
    /// Number of times the transaction was retried. 0 for a transaction that
    /// succeeded the first time.
    pub retries: u32,
    /// The duration of the successful transaction, not including commit
    pub duration: Duration,
    pub commit_duration: Duration,
    pub total_duration: Duration,
}

/// The read that conflicted as part of an OCC
#[derive(Debug, PartialEq, Eq)]
pub struct ConflictingRead {
    pub(crate) index: TabletIndexName,
    pub(crate) id: ResolvedDocumentId,
    pub(crate) stack_traces: Option<Vec<StackTrace>>,
}

fn occ_write_source_string(
    source: &WriteSource,
    document_id: String,
    is_same_write_source: bool,
) -> Option<String> {
    let preamble = if is_same_write_source {
        "Another call to this mutation".to_string()
    } else {
        match source {
            WriteSource::Udf(_) => format!("A call to \"{}\"", source.display_name()?),
            WriteSource::SystemUdf(_) | WriteSource::System(_) => {
                convex_writer_description(source).to_string()
            },
        }
    };
    Some(format!(
        "{preamble} changed the document with ID \"{document_id}\""
    ))
}

/// Convex's own writers have internal names, so describe them by what the
/// developer did.
pub(crate) fn convex_writer_description(source: &WriteSource) -> &'static str {
    match source {
        WriteSource::SystemUdf(identifier) => {
            let (_, udf_path) = (**identifier).clone().into_component_and_udf_path();
            if udf_path.starts_with("_system/frontend/") {
                "An edit in the Convex dashboard"
            } else {
                "A Convex system operation"
            }
        },
        WriteSource::System(label) if label.contains("fivetran") => "A Fivetran sync",
        WriteSource::System(label) if label.contains("airbyte") => "An Airbyte sync",
        WriteSource::System(label) if label.starts_with("snapshot_import") => "A data import",
        WriteSource::Udf(_) | WriteSource::System(_) => "A Convex system operation",
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConflictingReadWithWriteSource {
    pub(crate) read: ConflictingRead,
    pub(crate) write_source: WriteSource,
    /// The timestamp of the conflicting write.
    ///
    /// N.B.: this may be a non-repeatable timestamp, if this conflict occurred
    /// against a pending write!
    pub(crate) write_ts: Timestamp,
}

impl ConflictingReadWithWriteSource {
    pub fn into_error(
        self,
        mapping: &TableMapping,
        component_registry: &ComponentRegistry,
        current_writer: &WriteSource,
    ) -> anyhow::Error {
        let write_ts_val = Some(u64::from(self.write_ts));
        let table_name = mapping.tablet_name(*self.read.index.table());

        let Ok(table_name) = table_name else {
            let metadata = ErrorMetadata::user_occ(None, None, write_ts_val);
            return anyhow::anyhow!(metadata);
        };

        // Resolve the component path for the table's namespace.
        let component_path_str = mapping
            .tablet_namespace(*self.read.index.table())
            .ok()
            .and_then(|ns| {
                let component_id = ComponentId::from(ns);
                let path = component_registry
                    .get_component_path(component_id, &mut TransactionReadSet::new());
                path.and_then(|p| p.serialize())
            });

        // We want to show the document's ID only if we know which mutation changed it,
        // so use it only if we have a write source.
        let occ_msg = occ_write_source_string(
            &self.write_source,
            self.read.id.to_string(),
            *current_writer == self.write_source,
        );

        let write_source = self.write_source.display_name();

        if !table_name.is_system() {
            let metadata = ErrorMetadata::user_occ(
                Some(OccInfo {
                    table_name: Some(table_name.into()),
                    document_id: Some(self.read.id.developer_id.encode()),
                    write_source,
                    component_path: component_path_str,
                    ..Default::default()
                }),
                occ_msg,
                write_ts_val,
            );
            return anyhow::anyhow!(metadata);
        }

        let msg = occ_msg
            .map(|write_source| format!("{write_source}.\n"))
            .unwrap_or_default();
        let index = format!("{table_name}.{}", self.read.index.descriptor());
        let msg = format!(
            "{msg}(conflicts with read of system table {index} in this writer \"{}\")",
            current_writer
                .display_name()
                .as_deref()
                .unwrap_or("unknownwriter")
        );

        let formatted = format!(
            "{msg}. Use RUST_BACKTRACE=1 READ_SET_CAPTURE_BACKTRACES=true to find trace of \
             relevant reads"
        );

        if let Some(stack_traces) = self.read.stack_traces {
            tracing::error!(
                "Displaying {}/{} stack traces of relevant reads. Increase NUM_READ_SET_STACKS \
                 for more:",
                cmp::min(*NUM_READ_SET_STACKS, stack_traces.len()),
                stack_traces.len(),
            );

            for stack_trace in stack_traces.iter().take(*NUM_READ_SET_STACKS) {
                tracing::error!("Read of {index} occurred at {stack_trace}");
            }
        };
        let metadata = ErrorMetadata::system_occ(write_ts_val, write_source);
        anyhow::anyhow!(formatted).context(metadata)
    }
}

/// `op` is an internal name, so it stays out of the message.
pub fn unauthorized_error(_op: &'static str) -> ErrorMetadata {
    ErrorMetadata::forbidden(
        "SystemIdentityRequired",
        "You don't have permission to perform this operation.",
    )
}
