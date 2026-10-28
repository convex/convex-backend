use std::{
    any::{
        type_name,
        Any,
    },
    borrow::Borrow,
    cmp::{
        self,
        Ordering,
    },
    collections::{
        BTreeMap,
        BTreeSet,
    },
    fmt::Debug,
    iter,
    mem,
    sync::{
        Arc,
        OnceLock,
    },
};

use anyhow::Context;
use async_trait::async_trait;
use common::{
    bootstrap_model::index::{
        database_index::DatabaseIndexState,
        IndexConfig,
        TabletIndexMetadata,
    },
    document::{
        PackedDocument,
        ParseDocument,
        ParsedDocument,
        ResolvedDocument,
    },
    index::{
        IndexKey,
        IndexKeyBytes,
    },
    interval::Interval,
    persistence::PersistenceSnapshot,
    query::Order,
    static_span,
    types::{
        DatabaseIndexUpdate,
        DatabaseIndexValue,
        IndexId,
        IndexRef,
        TabletIndexName,
        Timestamp,
        WriteTimestamp,
    },
    value::Size,
};
use futures::TryStreamExt;
use imbl::{
    OrdMap,
    OrdSet,
};
use value::{
    ResolvedDocumentId,
    TableMapping,
    TableName,
    TabletId,
};

use crate::index_registry::IndexRegistry;

#[async_trait]
pub trait InMemoryIndexes: Send + Sync {
    /// Returns the index range if it is found in the cache (backend) or loaded
    /// into the cache (function runner). If the index is not supposed to be in
    /// memory, returns None so it is safe to call on any index.
    async fn range(
        &self,
        index: IndexRef,
        interval: &Interval,
        order: Order,
        tablet_id: TabletId,
        table_name: TableName,
    ) -> anyhow::Result<Option<Vec<(IndexKeyBytes, Timestamp, MemoryDocument)>>>;
}

/// [`BackendInMemoryIndexes`] maintains in-memory database indexes. With the
/// exception of the table scan index, newly created indexes are not initially
/// loaded in memory. A post-commit, asynchronous backfill job is responsible
/// for filling the index.
#[derive(Clone)]
pub struct BackendInMemoryIndexes {
    /// Fully loaded in-memory indexes. If not present, the index is not loaded.
    in_memory_indexes: OrdMap<IndexId, DatabaseIndexMap>,
}

#[async_trait]
impl InMemoryIndexes for BackendInMemoryIndexes {
    async fn range(
        &self,
        index: IndexRef,
        interval: &Interval,
        order: Order,
        _tablet_id: TabletId,
        _table_name: TableName,
    ) -> anyhow::Result<Option<Vec<(IndexKeyBytes, Timestamp, MemoryDocument)>>> {
        self.range(index.id(), interval, order)
    }
}

impl BackendInMemoryIndexes {
    #[fastrace::trace]
    pub fn bootstrap(
        index_registry: &IndexRegistry,
        index_documents: Vec<(Timestamp, PackedDocument)>,
        ts: Timestamp,
    ) -> anyhow::Result<Self> {
        // Load the indexes by_id index
        let meta_index = index_registry
            .get_enabled(&TabletIndexName::by_id(index_registry.index_table()))
            .context("Missing meta index")?;
        let mut meta_index_map = DatabaseIndexMap::new_at(ts);
        for (ts, index_doc) in index_documents {
            let index_key = IndexKey::new(vec![], index_doc.developer_id());
            meta_index_map.insert(index_key.to_bytes(), ts, MemoryDocument::new(index_doc));
        }

        let mut in_memory_indexes = OrdMap::new();
        in_memory_indexes.insert(meta_index.id(), meta_index_map);

        Ok(Self { in_memory_indexes })
    }

    /// Fetch tables across all namespaces whose name is in `tables` and load
    /// their enabled indexes into memory.
    #[fastrace::trace]
    pub async fn load_enabled_for_tables(
        &mut self,
        index_registry: &IndexRegistry,
        table_mapping: &TableMapping,
        snapshot: &PersistenceSnapshot,
        tables: &BTreeSet<TableName>,
    ) -> anyhow::Result<()> {
        let enabled_indexes = index_registry.all_enabled_indexes();
        let mut indexes_by_table: BTreeMap<TabletId, Vec<_>> = BTreeMap::new();
        let mut indexes_to_load = 0;
        for index_metadata in enabled_indexes {
            let table_name = table_mapping.tablet_name(*index_metadata.name.table())?;
            if tables.contains(&table_name) {
                match &index_metadata.config {
                    IndexConfig::Database { on_disk_state, .. } => {
                        anyhow::ensure!(
                            *on_disk_state == DatabaseIndexState::Enabled,
                            "Index should have been enabled: {:?}, state: {on_disk_state:?}",
                            index_metadata.name
                        )
                    },
                    IndexConfig::Text { .. } | IndexConfig::Vector { .. } => {
                        // We do not load search or vector indexes into memory.
                        continue;
                    },
                }
                tracing::debug!(
                    "Loading {table_name}.{} ...",
                    index_metadata.name.descriptor()
                );
                indexes_by_table
                    .entry(*index_metadata.name.table())
                    .or_default()
                    .push(index_metadata);
                indexes_to_load += 1;
            }
        }
        tracing::info!(
            "Loading {} tables with {} indexes...",
            indexes_by_table.len(),
            indexes_to_load
        );
        for (tablet_id, index_metadatas) in indexes_by_table {
            let (num_keys, total_bytes) = self
                .load_enabled(tablet_id, index_metadatas, snapshot)
                .await?;
            tracing::debug!("Loaded {num_keys} keys, {total_bytes} bytes.");
        }
        Ok(())
    }

    #[fastrace::trace]
    pub async fn load_enabled(
        &mut self,
        tablet_id: TabletId,
        mut indexes: Vec<ParsedDocument<TabletIndexMetadata>>,
        snapshot: &PersistenceSnapshot,
    ) -> anyhow::Result<(usize, usize)> {
        indexes.retain(|index| {
            !self
                .in_memory_indexes
                .contains_key(&index.id().internal_id().into())
        });
        if indexes.is_empty() {
            // Already loaded in memory.
            return Ok((0, 0));
        }
        for index in &indexes {
            anyhow::ensure!(
                *index.name.table() == tablet_id,
                "Index is for wrong table {:?}",
                index.name.table()
            );
            if let IndexConfig::Database { on_disk_state, .. } = &index.config {
                anyhow::ensure!(
                    *on_disk_state == DatabaseIndexState::Enabled,
                    "Attempting to load index {} that is not backfilled yet {:?}",
                    index.name,
                    index,
                );
            } else {
                anyhow::bail!(
                    "Attempted to load index {} that isn't a database index {:?}",
                    index.name,
                    index,
                )
            }
        }

        // Read the table using an arbitrary index from the list
        let entries: Vec<_> = snapshot
            .index_scan(
                IndexRef::try_from(&indexes[0])?,
                tablet_id,
                &Interval::all(),
                Order::Asc,
                usize::MAX,
            )
            .try_collect()
            .await?;
        let mut num_keys: usize = 0;
        let mut total_size: usize = 0;
        let mut index_maps = vec![DatabaseIndexMap::new_at(*snapshot.timestamp()); indexes.len()];
        for (_, rev) in entries.into_iter() {
            num_keys += 1;
            total_size += rev.value.value().size();
            let doc = MemoryDocument::new(PackedDocument::pack(&rev.value));
            // Calculate all the index keys. For simplicity we throw away the
            // index key that we read from persistence and recalculate it.
            for ((index, index_map), doc) in indexes
                .iter()
                .zip(&mut index_maps)
                .zip(iter::repeat_n(doc, indexes.len()))
            {
                let IndexConfig::Database { spec, .. } = &index.config else {
                    unreachable!()
                };
                let key = doc.packed().index_key_owned(&spec.fields);
                index_map.insert(key, rev.ts, doc);
            }
        }

        for (index, index_map) in indexes.iter().zip(index_maps) {
            self.in_memory_indexes
                .insert(index.id().internal_id().into(), index_map);
        }
        Ok((num_keys, total_size))
    }

    /// Insert enabled indexes for the given `tablet_id` with the provided,
    /// already-fetched documents.
    #[fastrace::trace]
    pub fn load_table(
        &mut self,
        index_registry: &IndexRegistry,
        tablet_id: TabletId,
        documents: Vec<(Timestamp, PackedDocument)>,
        snapshot_timestamp: Timestamp,
    ) {
        // Build the shared document once so that every index on the table stores
        // (and parses) the same one.
        let documents: Vec<(Timestamp, MemoryDocument)> = documents
            .into_iter()
            .map(|(ts, doc)| (ts, MemoryDocument::new(doc)))
            .collect();
        for index_doc in index_registry.enabled_indexes_for_table(tablet_id) {
            let IndexConfig::Database {
                spec,
                on_disk_state,
                ..
            } = &index_doc.metadata().config
            else {
                continue;
            };
            assert_eq!(*on_disk_state, DatabaseIndexState::Enabled); // ensured by IndexRegistry
            let mut index_map = DatabaseIndexMap::new_at(snapshot_timestamp);
            for (ts, doc) in &documents {
                let key = doc.packed().index_key_owned(&spec.fields);
                index_map.insert(key, *ts, doc.clone());
            }
            self.in_memory_indexes.insert(index_doc.id(), index_map);
        }
    }

    pub fn update(
        &mut self,
        // NB: We assume that `index_registry` has already received this update.
        index_registry: &IndexRegistry,
        ts: Timestamp,
        deletion: Option<(ResolvedDocument, WriteTimestamp)>,
        insertion: Option<ResolvedDocument>,
    ) -> Vec<DatabaseIndexUpdate> {
        if let (Some((old_document, _)), None) = (&deletion, &insertion)
            && old_document.id().tablet_id == index_registry.index_table()
        {
            // Drop the index from memory.
            self.in_memory_indexes
                .remove(&old_document.id().internal_id().into());
        }

        // Build up the list of updates to apply to all database indexes.
        let updates = index_registry.index_updates(
            deletion.as_ref().map(|(document, ts)| (document, *ts)),
            insertion.as_ref(),
        );

        let mut memory_doc = None;

        // Apply the updates to the subset of database indexes in memory.
        for update in &updates {
            match self.in_memory_indexes.get_mut(&update.index.id()) {
                Some(key_set) => match &update.value {
                    DatabaseIndexValue::Deleted => {
                        key_set.remove(&update.key.to_bytes(), ts);
                    },
                    DatabaseIndexValue::NonClustered(doc_id) => {
                        // All in-memory indexes are clustered. Get the document
                        // from the update itself.
                        match insertion {
                            Some(ref doc) => {
                                assert_eq!(*doc_id, doc.id());
                                // reuse the MemoryDocument if inserting into more than one
                                // index
                                let memory_doc = memory_doc
                                    .get_or_insert_with(|| {
                                        MemoryDocument::new(PackedDocument::pack(doc))
                                    })
                                    .clone();
                                key_set.insert(update.key.to_bytes(), ts, memory_doc);
                            },
                            None => panic!("Unexpected index update: {:?}", update.value),
                        }
                    },
                },
                None => {},
            };
        }

        updates
    }

    pub fn in_memory_indexes_last_modified(&self) -> BTreeMap<IndexId, Timestamp> {
        self.in_memory_indexes
            .iter()
            .map(|(index_id, index_map)| (*index_id, index_map.last_modified))
            .collect()
    }

    pub fn range(
        &self,
        index_id: IndexId,
        interval: &Interval,
        order: Order,
    ) -> anyhow::Result<Option<Vec<(IndexKeyBytes, Timestamp, MemoryDocument)>>> {
        Ok(self
            .in_memory_indexes
            .get(&index_id)
            .map(|index_map| order.apply(index_map.range(interval)).collect()))
    }

}

/// Implementor of `InMemoryIndexes` if no indexes are available in-memory.
pub struct NoInMemoryIndexes;
#[async_trait]
impl InMemoryIndexes for NoInMemoryIndexes {
    async fn range(
        &self,
        _index: IndexRef,
        _interval: &Interval,
        _order: Order,
        _tablet_id: TabletId,
        _table_name: TableName,
    ) -> anyhow::Result<Option<Vec<(IndexKeyBytes, Timestamp, MemoryDocument)>>> {
        Ok(None)
    }
}

#[derive(Debug)]
struct IndexDocument {
    key: IndexKeyBytes,
    ts: Timestamp,
    document: MemoryDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, derive_more::Deref)]
struct ArcIndexDocument(Arc<IndexDocument>);

impl Borrow<[u8]> for ArcIndexDocument {
    fn borrow(&self) -> &[u8] {
        self.0.key.borrow()
    }
}

impl PartialEq for IndexDocument {
    fn eq(&self, other: &Self) -> bool {
        self.key.eq(&other.key)
    }
}
impl Eq for IndexDocument {}
impl PartialOrd for IndexDocument {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for IndexDocument {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

#[derive(Clone, Debug)]
pub struct DatabaseIndexMap {
    // We use OrdSet to provide efficient copy-on-write.
    // Note that all in-memory indexes are clustered.
    // N.B.: OrdMap/OrdSet are very sensitive to the size of keys and values (as
    // a map stores a minimum of 64 key-value pairs, even if empty) and likes to
    // clone them at will, so we store just a single Arc inside of it
    inner: OrdSet<ArcIndexDocument>,
    /// The timestamp of the last update to the index.
    last_modified: Timestamp,
}

impl DatabaseIndexMap {
    /// Construct an empty set.
    fn new_at(ts: Timestamp) -> Self {
        Self {
            inner: OrdSet::new(),
            last_modified: ts,
        }
    }

    /// Returns an iterator over the index that are within `range`, in order.
    fn range(
        &self,
        interval: &Interval,
    ) -> impl DoubleEndedIterator<Item = (IndexKeyBytes, Timestamp, MemoryDocument)> + use<'_> {
        let _s = static_span!();
        self.inner
            .range(interval)
            .map(|e| (e.key.clone(), e.ts, e.document.clone()))
    }

    fn insert(&mut self, key: IndexKeyBytes, ts: Timestamp, document: MemoryDocument) {
        self.inner.insert(ArcIndexDocument(Arc::new(IndexDocument {
            key,
            ts,
            document,
        })));
        self.last_modified = cmp::max(self.last_modified, ts);
    }

    fn remove(&mut self, k: &IndexKeyBytes, ts: Timestamp) {
        self.inner.remove::<[u8]>(k);
        self.last_modified = cmp::max(self.last_modified, ts);
    }
}

pub enum LazyDocument {
    Packed(PackedDocument),
    Memory(MemoryDocument),
}

/// A system document fetched from an in-memory index. This is internally
/// reference-counted and cheaply cloneable. All indexes on a table share the
/// same `MemoryDocument` for a given document, so the parse cache is populated
/// at most once per document.
#[derive(Clone, Debug)]
pub struct MemoryDocument(Arc<MemoryDocumentInner>);

#[derive(Debug)]
struct MemoryDocumentInner {
    packed_document: PackedDocument,
    cached_system_document: SystemDocument,
}

impl MemoryDocument {
    pub fn new(packed_document: PackedDocument) -> Self {
        Self(Arc::new(MemoryDocumentInner {
            packed_document: packed_document.shrink(),
            cached_system_document: SystemDocument::new(),
        }))
    }

    pub fn packed(&self) -> &PackedDocument {
        &self.0.packed_document
    }

    /// Parse and return the document. The same document must not be parsed
    /// twice with different types `T`.
    pub fn force<T: Send + Sync + 'static>(&self) -> anyhow::Result<Arc<ParsedDocument<T>>>
    where
        for<'a> &'a PackedDocument: ParseDocument<T>,
    {
        self.0.cached_system_document.force(&self.0.packed_document)
    }

    /// Parse without populating the shared cache. For one-shot scans whose
    /// result is used once and dropped.
    pub fn parse_uncached<T>(&self) -> anyhow::Result<ParsedDocument<T>>
    where
        for<'a> &'a PackedDocument: ParseDocument<T>,
    {
        ParseDocument::parse(&self.0.packed_document)
    }
}

const _: () = {
    // `MemoryDocument` is stored in every index on a table and cloned out of
    // every range scan, so keep it pointer-sized.
    assert!(mem::size_of::<MemoryDocument>() == mem::size_of::<usize>());
};

/// Stores a lazily-populated, cached `ParsedDocument` of the right type for
/// this system document.
#[derive(Default, Debug)]
pub struct SystemDocument(OnceLock<Arc<dyn Any + Send + Sync>>);

impl SystemDocument {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn force<T: Send + Sync + 'static>(
        &self,
        doc: &PackedDocument,
    ) -> anyhow::Result<Arc<ParsedDocument<T>>>
    where
        for<'a> &'a PackedDocument: ParseDocument<T>,
    {
        if let Ok(val) = self
            .0
            .get_or_try_init(|| doc.parse().map(|doc| Arc::new(doc) as Arc<_>))?
            .clone()
            .downcast()
        {
            return Ok(val);
        }
        // This is unexpected; it could happen if there is more than one
        // SystemTable type pointing at a table.
        let msg = format!(
            "doc {} already has a cached system document not of type {}",
            doc.id(),
            type_name::<T>()
        );
        if cfg!(debug_assertions) {
            panic!("{msg}");
        }
        tracing::warn!("{msg}");
        doc.parse().map(Arc::new)
    }
}

impl From<PackedDocument> for LazyDocument {
    fn from(value: PackedDocument) -> Self {
        Self::Packed(value)
    }
}

impl LazyDocument {
    pub fn unpack(self) -> ResolvedDocument {
        match self {
            LazyDocument::Packed(doc) => doc.unpack(),
            LazyDocument::Memory(doc) => doc.packed().unpack(),
        }
    }

    pub fn size(&self) -> usize {
        match self {
            LazyDocument::Packed(doc) => doc.size(),
            LazyDocument::Memory(doc) => doc.packed().size(),
        }
    }

    pub fn id(&self) -> ResolvedDocumentId {
        match self {
            LazyDocument::Packed(doc) => doc.id(),
            LazyDocument::Memory(doc) => doc.packed().id(),
        }
    }

    pub fn pack(self) -> PackedDocument {
        match self {
            LazyDocument::Packed(doc) => doc,
            LazyDocument::Memory(doc) => doc.packed().clone(),
        }
    }
}
