use std::{
    cmp::Ordering,
    collections::BTreeMap,
    sync::{
        Arc,
        OnceLock,
    },
};

use async_trait::async_trait;
use common::{
    bootstrap_model::index::{
        database_index::{
            DatabaseIndexSpec,
            IndexedFields,
        },
        IndexConfig,
    },
    document::{
        DocumentUpdate,
        PackedDocument,
        ResolvedDocument,
    },
    index::{
        IndexKey,
        IndexKeyBytes,
    },
    interval::Interval,
    knobs::TRANSACTION_MAX_READ_SIZE_BYTES,
    query::{
        CursorPosition,
        InternalSearch,
        Order,
        SearchVersion,
    },
    runtime,
    try_anyhow,
    types::{
        DatabaseIndexUpdate,
        DatabaseIndexValue,
        IndexId,
        IndexName,
        TabletIndexName,
        WriteTimestamp,
    },
};
use imbl::OrdMap;
use indexing::{
    database_index_snapshot::{
        DatabaseIndexSnapshot,
        DatabaseIndexSnapshotCache,
    },
    in_memory_indexes::LazyDocument,
    index_reader::RangeRequest,
    index_registry::{
        Index,
        IndexRegistry,
    },
};
use search::{
    QueryResults,
    Searcher,
    TextIndexManager,
    TextSearchResults,
};
use storage::Storage;
use tokio::task;
use value::{
    DeveloperDocumentId,
    FieldPath,
    TabletId,
};

use crate::{
    preloaded::PreloadedIndexRange,
    query::IndexRangeResponse,
    reads::TransactionReadSet,
    writes::PendingWrites,
    DEFAULT_PAGE_SIZE,
};

/// [`TransactionIndex`] is an index used by transactions.
/// It gets constructed from [`DatabaseIndexSnapshot`] and [`IndexRegistry`] at
/// a timestamp snapshot. It buffers the transaction pending index updates and
/// merges and overlays them on top of the snapshot to allow the transaction to
/// read its own writes.
#[derive(Clone)]
pub struct TransactionIndex {
    // Metadata about existing indexes with any changes to the index tables applied. Note that
    // those changes are stored separately in `database_index_updates` and `search_index_updates`
    // in their underlying database writes too.
    index_registry: IndexRegistry,
    // Weather the index registry has been updates since the beginning of the transaction.
    index_registry_updated: bool,

    // Database indexes combine a base index snapshot in persistence with pending updates applied
    // in-memory.
    database_index_snapshot: DatabaseIndexSnapshot,
    database_index_updates: OrdMap<IndexId, TransactionIndexMap>,

    // Similar to database indexes, text search indexes are implemented by applying pending updates
    // on top of the transaction base snapshot.
    text_index_snapshot: Arc<dyn TransactionTextSnapshot>,
    text_index_updates: OrdMap<IndexId, Vec<DocumentUpdate>>,
}

impl PendingWrites for TransactionIndex {}

impl TransactionIndex {
    pub fn new(
        index_registry: IndexRegistry,
        database_index_snapshot: DatabaseIndexSnapshot,
        text_index_snapshot: Arc<dyn TransactionTextSnapshot>,
    ) -> Self {
        Self {
            index_registry,
            index_registry_updated: false,
            database_index_snapshot,
            database_index_updates: OrdMap::new(),
            text_index_snapshot,
            text_index_updates: OrdMap::new(),
        }
    }

    pub fn index_registry(&self) -> &IndexRegistry {
        &self.index_registry
    }

    pub(crate) fn pending_iter_for_interval<'a>(
        &'a self,
        index_name: &'a TabletIndexName,
        printable_index_name: &'a IndexName,
        interval: &'a Interval,
    ) -> Result<
        impl DoubleEndedIterator<Item = (IndexKeyBytes, Option<PackedDocument>)> + 'a,
        anyhow::Error,
    > {
        let iter = match self
            .index_registry
            .require_enabled(index_name, printable_index_name)
        {
            Ok(index) => self.database_index_updates.get(&index.id()),
            // Range queries on missing tables are allowed for system provided indexes.
            Err(_) if index_name.is_by_id_or_creation_time() => None,
            Err(e) => return Err(e),
        }
        .map(|pending| pending.range(interval))
        .into_iter()
        .flatten();
        Ok(iter)
    }

    pub(crate) fn pending_writes_for_by_id_index(
        &self,
        tablet_id: TabletId,
    ) -> Option<&TransactionIndexMap> {
        let index = self
            .index_registry
            .get_enabled(&TabletIndexName::by_id(tablet_id))?;
        self.database_index_updates.get(&index.id())
    }

    /// Range over a index including pending updates.
    /// `max_size` provides an estimate of the number of rows to be
    /// streamed from the database.
    /// The returned vecs may be larger or smaller than `max_size` depending on
    /// pending writes.
    pub(crate) async fn range_no_deps(
        &mut self,
        ranges: &[&RangeRequest],
    ) -> Vec<
        anyhow::Result<(
            Vec<(IndexKeyBytes, LazyDocument, WriteTimestamp)>,
            CursorPosition,
        )>,
    > {
        let batch_size = ranges.len();

        // Resolve singleton ranges that have pending writes without
        // hitting persistence.
        let mut pre_resolved = Vec::with_capacity(batch_size);
        let mut persistence_ranges: Vec<&RangeRequest> = Vec::new();

        for &range_request in ranges {
            if range_request.interval.is_singleton().is_some() {
                let pending_result = self.pending_iter_for_interval(
                    &range_request.index_name,
                    &range_request.printable_index_name,
                    &range_request.interval,
                );
                match pending_result {
                    Ok(mut pending_it) => {
                        if let Some((key, maybe_doc)) = pending_it.next() {
                            if pending_it.next().is_some() {
                                pre_resolved.push(Some(Err(anyhow::anyhow!(
                                    "Expected singleton range to have at most one result"
                                ))));
                                continue;
                            }
                            let mut range_results = Vec::new();
                            if let Some(doc) = maybe_doc {
                                range_results.push((key, doc.into(), WriteTimestamp::Pending));
                            }
                            pre_resolved.push(Some(Ok((range_results, CursorPosition::End))));
                            continue;
                        }
                    },
                    Err(e) => {
                        pre_resolved.push(Some(Err(e)));
                        continue;
                    },
                }
            }

            pre_resolved.push(None);
            persistence_ranges.push(range_request);
        }

        // Fetch only the ranges that weren't resolved from pending writes.
        let snapshot_results = self
            .database_index_snapshot
            .range_batch(&persistence_ranges)
            .await;

        let mut persistence_iter = snapshot_results.into_iter();

        let mut results = Vec::with_capacity(batch_size);
        for (&range_request, resolved) in ranges.iter().zip(pre_resolved) {
            // We need to preserve the order of the ranges.
            if let Some(resolved) = resolved {
                results.push(resolved);
                continue;
            }

            let snapshot_result = persistence_iter
                .next()
                .unwrap_or_else(|| Err(anyhow::anyhow!("fewer persistence results than expected")));

            let result = try_anyhow!({
                let (snapshot_result_vec, cursor) = snapshot_result?;
                let mut snapshot_it = snapshot_result_vec.into_iter();
                // Pending writes past `cursor` belong to later pages.
                let (snapshot_interval, _) = range_request
                    .interval
                    .split(cursor.clone(), range_request.order);
                let pending_it = self.pending_iter_for_interval(
                    &range_request.index_name,
                    &range_request.printable_index_name,
                    &snapshot_interval,
                )?;
                let mut pending_it = range_request.order.apply(pending_it);

                let mut snapshot_next = snapshot_it.next();
                let mut pending_next = pending_it.next();
                let mut range_results = vec![];
                loop {
                    task::consume_budget().await;
                    match (snapshot_next, pending_next) {
                        (
                            Some((snapshot_key, snapshot_ts, snapshot_doc)),
                            Some((pending_key, maybe_pending_doc)),
                        ) => {
                            let cmp = match range_request.order {
                                Order::Asc => snapshot_key.cmp(&pending_key),
                                Order::Desc => pending_key.cmp(&snapshot_key),
                            };
                            match cmp {
                                Ordering::Less => {
                                    range_results.push((
                                        snapshot_key,
                                        snapshot_doc,
                                        WriteTimestamp::Committed(snapshot_ts),
                                    ));
                                    snapshot_next = snapshot_it.next();
                                    pending_next = Some((pending_key, maybe_pending_doc));
                                },
                                Ordering::Equal => {
                                    // The pending entry overwrites the snapshot one.
                                    if let Some(pending_doc) = maybe_pending_doc {
                                        range_results.push((
                                            pending_key,
                                            pending_doc.into(),
                                            WriteTimestamp::Pending,
                                        ));
                                    };
                                    snapshot_next = snapshot_it.next();
                                    pending_next = pending_it.next();
                                },
                                Ordering::Greater => {
                                    if let Some(pending_doc) = maybe_pending_doc {
                                        range_results.push((
                                            pending_key,
                                            pending_doc.into(),
                                            WriteTimestamp::Pending,
                                        ));
                                    };
                                    snapshot_next = Some((snapshot_key, snapshot_ts, snapshot_doc));
                                    pending_next = pending_it.next();
                                },
                            }
                        },
                        (Some((snapshot_key, snapshot_ts, snapshot_doc)), None) => {
                            range_results.push((
                                snapshot_key,
                                snapshot_doc,
                                WriteTimestamp::Committed(snapshot_ts),
                            ));
                            snapshot_next = snapshot_it.next();
                            pending_next = None;
                        },
                        (None, Some((pending_key, maybe_pending_doc))) => {
                            if let Some(pending_doc) = maybe_pending_doc {
                                range_results.push((
                                    pending_key,
                                    pending_doc.into(),
                                    WriteTimestamp::Pending,
                                ));
                            };
                            snapshot_next = None;
                            pending_next = pending_it.next();
                        },
                        (None, None) => break,
                    }
                }
                if !range_request.interval.contains_cursor(&cursor) {
                    Err(anyhow::anyhow!(
                        "query for {:?} not making progress",
                        range_request.interval
                    ))?;
                }
                (range_results, cursor)
            });
            results.push(result);
        }
        assert_eq!(results.len(), batch_size);
        results
    }

    #[fastrace::trace]
    pub async fn search(
        &mut self,
        reads: &mut TransactionReadSet,
        query: &InternalSearch,
        index_name: TabletIndexName,
        version: SearchVersion,
    ) -> anyhow::Result<TextSearchResults> {
        // We do not allow modifying the index registry and performing a text search
        // in the same transaction. We could implement this by sending the index
        // updates in the search request, but there is no need to bother since we
        // don't yet have a use case of modifying an index metadata and performing
        // a text search in the same transaction.
        anyhow::ensure!(
            !self.index_registry_updated,
            "Text search and index registry update not allowed in the same transaction"
        );
        // HACK: instead of using `self.require_enabled` we access the
        // `IndexRegistry` directly to fetch index info, which skips recording the
        // read of `index.id()` into our `TransactionReadSet`.
        // This avoids invalidating the transaction based on the precise value
        // of the `TextIndexState`, as the transaction does not logically depend
        // on it, and therefore avoids invalidation after flushing or compacting
        // search indexes.
        // TODO(ENG-9324): this has the side effect of failing to invalidate
        // transactions if the search index is removed. In practice, that should
        // only happen as part of a push that would separately invalidate user
        // transactions anyway.
        let index = self
            .index_registry
            .require_enabled(&index_name, &query.printable_index_name()?)?;
        let empty = vec![];
        let pending_updates = self.text_index_updates.get(&index.id()).unwrap_or(&empty);
        let results = self
            .text_index_snapshot
            .search(&index, query, version, pending_updates)
            .await?;

        // Record the query results in the read set.
        reads.record_search(index_name.clone(), results.reads);

        Ok(TextSearchResults {
            revisions_with_keys: results.revisions_with_keys,
            filtered_bytes_searched: results.filtered_bytes_searched,
        })
    }

    /// Fetch a batch of index ranges. This method does not update the read set,
    /// since we might be fetching more documents than the caller actually needs
    /// due to filtering.
    ///
    /// Callers must call `record_indexed_directly` when consuming the results.
    pub async fn range_batch(
        &mut self,
        ranges: &[&RangeRequest],
    ) -> Vec<anyhow::Result<IndexRangeResponse>> {
        let batch_size = ranges.len();
        let mut results = Vec::with_capacity(batch_size);

        let fetch_results = self.range_no_deps(ranges).await;

        for (
            RangeRequest {
                index_name: _,
                printable_index_name: _,
                interval,
                order: _,
                max_size,
            },
            fetch_result,
        ) in ranges.iter().zip(fetch_results)
        {
            let result: anyhow::Result<_> = try_anyhow!({
                let (documents, fetch_cursor) = fetch_result?;
                let mut total_bytes = 0;
                let mut within_bytes_limit = true;
                let out: Vec<_> = documents
                    .into_iter()
                    .map(|(key, doc, ts)| (key, doc.pack(), ts))
                    .take(*max_size)
                    .take_while(|(_, document, _)| {
                        within_bytes_limit = total_bytes < *TRANSACTION_MAX_READ_SIZE_BYTES;
                        // Allow the query to exceed the limit by one document so the query
                        // is guaranteed to make progress and probably fail.
                        // Note system document limits are different, so a single document
                        // can be larger than `TRANSACTION_MAX_READ_SIZE_BYTES`.
                        total_bytes += document.size();
                        within_bytes_limit
                    })
                    .collect();

                let cursor = if let Some((last_key, ..)) = out.last()
                    && (out.len() >= *max_size || !within_bytes_limit)
                {
                    // We hit an early termination condition within this page.
                    CursorPosition::After(last_key.clone())
                } else {
                    // Everything fetched will be returned, so the cursor
                    // of the page is the fetch cursor
                    fetch_cursor
                };
                if !interval.contains_cursor(&cursor) {
                    Err(anyhow::anyhow!(
                        "query for {interval:?} not making progress"
                    ))?;
                }
                IndexRangeResponse { page: out, cursor }
            });
            results.push(result);
        }
        assert_eq!(results.len(), batch_size);
        results
    }

    #[fastrace::trace]
    pub async fn preload_index_range(
        &mut self,
        reads: &mut TransactionReadSet,
        tablet_index_name: &TabletIndexName,
        printable_index_name: &IndexName,
        interval: &Interval,
    ) -> anyhow::Result<PreloadedIndexRange> {
        let index = self.require_enabled(reads, tablet_index_name, printable_index_name)?;
        let IndexConfig::Database {
            spec: DatabaseIndexSpec { ref fields, .. },
            ..
        } = index.metadata().config
        else {
            anyhow::bail!("{printable_index_name} isn't a database index");
        };
        let indexed_fields: Vec<FieldPath> = fields.clone().into();
        let indexed_field = indexed_fields[0].clone();
        anyhow::ensure!(indexed_fields.len() == 1);
        let mut remaining_interval = interval.clone();
        let mut preloaded = BTreeMap::new();
        while !remaining_interval.is_empty() {
            let [result] = self
                .range_no_deps(&[&RangeRequest {
                    index_name: tablet_index_name.clone(),
                    printable_index_name: printable_index_name.clone(),
                    interval: remaining_interval,
                    order: Order::Asc,
                    max_size: DEFAULT_PAGE_SIZE,
                }])
                .await
                .try_into()
                .map_err(|_| anyhow::anyhow!("wrong number of results"))?;
            let (documents, cursor) = result?;
            (_, remaining_interval) = interval.split(cursor, Order::Asc);
            for (_, document, _) in documents {
                let document = document.pack();
                let key = document.value().get_path(&indexed_field);
                anyhow::ensure!(
                    preloaded.insert(key, document).is_none(),
                    "Index {printable_index_name:?} isn't unique",
                );
            }
        }
        // Since PreloadedIndexRange only permits looking up documents by the index
        // key, we don't need to record `interval` as a read dependency. Put another
        // way, even though we're reading all of the rows in `interval`, the layer
        // above is only allowed to do point queries against `index_name`.
        Ok(PreloadedIndexRange::new(
            printable_index_name.table().clone(),
            tablet_index_name.clone(),
            indexed_field,
            preloaded,
        ))
    }

    // TODO: Add precise error types to facilitate detecting which indexing errors
    // are the developer's fault or not.
    /// `old_document` comes with the write timestamp of its revision.
    pub fn begin_update(
        &mut self,
        old_document: Option<(ResolvedDocument, WriteTimestamp)>,
        new_document: Option<ResolvedDocument>,
    ) -> anyhow::Result<Update<'_>> {
        let mut registry = self.index_registry.clone();
        registry.update(
            old_document.as_ref().map(|(document, _)| document),
            new_document.as_ref(),
        )?;

        Ok(Update {
            index: self,
            deletion: old_document,
            insertion: new_document,
            registry,
        })
    }

    fn finish_update(
        &mut self,
        old_document: Option<(ResolvedDocument, WriteTimestamp)>,
        new_document: Option<ResolvedDocument>,
    ) -> Vec<DatabaseIndexUpdate> {
        let (old_document, old_ts) = match old_document {
            Some((document, ts)) => (Some(document), Some(ts)),
            None => (None, None),
        };
        // Update the index registry first.
        let index_registry_updated = self
            .index_registry
            .apply_verified_update(old_document.as_ref(), new_document.as_ref());
        self.index_registry_updated |= index_registry_updated;

        // Then compute the index updates.
        let updates = self
            .index_registry
            .index_updates(old_document.as_ref().zip(old_ts), new_document.as_ref());

        // Add the index updates to self.database_index_updates.
        for update in &updates {
            let new_value = match &update.value {
                DatabaseIndexValue::Deleted => None,
                DatabaseIndexValue::NonClustered(doc_id) => {
                    // The pending updates are clustered. Get the document
                    // from the update itself.
                    match new_document {
                        Some(ref doc) => {
                            assert_eq!(doc.id(), *doc_id);
                            Some(doc)
                        },
                        None => panic!("Unexpected index update: {:?}", update.value),
                    }
                },
            };
            self.database_index_updates
                .entry(update.index.id())
                .or_insert_with(TransactionIndexMap::new)
                .insert(update.key.to_bytes(), new_value);
        }

        // If we are updating a document, the old and new ids must be the same.
        let document_id = new_document
            .as_ref()
            .map(|d| d.id())
            .or(old_document.as_ref().map(|d| d.id()));
        if let Some(id) = document_id {
            // Add the update to all affected text search indexes.
            for index in self.index_registry.text_indexes_by_table(id.tablet_id) {
                self.text_index_updates
                    .entry(index.id())
                    .or_default()
                    .push(DocumentUpdate {
                        id,
                        old_document: old_document.clone(),
                        new_document: new_document.clone(),
                    });
            }
        }

        // Note that we do not update the vector index and we always read at the
        // base snapshot.

        updates
    }

    pub fn get_pending(
        &self,
        reads: &mut TransactionReadSet,
        index_name: &TabletIndexName,
    ) -> Option<&Index> {
        self._get(reads, || self.index_registry.get_pending(index_name))
    }

    pub fn get_enabled(
        &self,
        reads: &mut TransactionReadSet,
        index_name: &TabletIndexName,
    ) -> Option<&Index> {
        self._get(reads, || self.index_registry.get_enabled(index_name))
    }

    fn _get<'a>(
        &'a self,
        reads: &mut TransactionReadSet,
        getter: impl FnOnce() -> Option<&'a Index>,
    ) -> Option<&'a Index> {
        let result = getter();
        self.record_interval(reads, result);
        result
    }

    pub fn require_enabled(
        &self,
        reads: &mut TransactionReadSet,
        index_name: &TabletIndexName,
        printable_index_name: &IndexName,
    ) -> anyhow::Result<Index> {
        let result = self
            .index_registry
            .require_enabled(index_name, printable_index_name)?;
        self.record_interval(reads, Some(&result));
        Ok(result)
    }

    fn record_interval(&self, reads: &mut TransactionReadSet, index: Option<&Index>) {
        let index_table = self.index_registry.index_table();
        let index_table_number = self.index_registry.index_table_number();
        let interval = match index {
            // Note there is no _index.by_name index. In order for the
            // name->index mapping to depend only on index id, we rely
            // on index name being immutable.
            Some(index) => {
                let full_index_id = DeveloperDocumentId::new(index_table_number, index.id().0);
                let index_key = IndexKey::new(vec![], full_index_id);
                Interval::prefix(index_key.to_bytes().into())
            },
            // On a name lookup miss, depend on all indexes.
            None => Interval::all(),
        };
        reads.record_indexed_derived(
            TabletIndexName::by_id(index_table),
            IndexedFields::by_id(),
            interval,
        );
    }

    /// Returns the snapshot the transaction is based on ignoring any pending
    /// updates.
    pub fn base_snapshot(&self) -> &DatabaseIndexSnapshot {
        &self.database_index_snapshot
    }

    pub fn base_snapshot_mut(&mut self) -> &mut DatabaseIndexSnapshot {
        &mut self.database_index_snapshot
    }

    pub fn clone_for_snapshot_query(&self) -> Self {
        Self {
            index_registry: self.index_registry.clone(),
            index_registry_updated: false,
            database_index_snapshot: self.database_index_snapshot.clone(),
            database_index_updates: OrdMap::new(),
            text_index_snapshot: self.text_index_snapshot.clone(),
            text_index_updates: OrdMap::new(),
        }
    }

    pub fn into_cache(self) -> DatabaseIndexSnapshotCache {
        self.database_index_snapshot.into_cache()
    }
}

#[derive(Debug, Clone)]
pub struct TransactionIndexMap {
    /// Unlike IndexMap we can simply use BTreeMap since the TransactionIndexMap
    /// does not get clones. The value needs to be Option<Document> since we
    /// need to distinguish between objects deleted within the transaction
    /// from objects that never existed.
    inner: BTreeMap<Vec<u8>, Option<PackedDocument>>,
}

impl TransactionIndexMap {
    pub fn new() -> Self {
        Self {
            inner: BTreeMap::new(),
        }
    }

    pub fn range(
        &self,
        interval: &Interval,
    ) -> impl DoubleEndedIterator<Item = (IndexKeyBytes, Option<PackedDocument>)> + use<'_> {
        self.inner
            .range(interval)
            .map(|(k, v)| (IndexKeyBytes(k.clone()), v.clone()))
    }

    pub fn insert(&mut self, k: IndexKeyBytes, v: Option<&ResolvedDocument>) {
        self.inner.insert(k.0, v.map(PackedDocument::pack));
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

pub struct Update<'a> {
    index: &'a mut TransactionIndex,

    deletion: Option<(ResolvedDocument, WriteTimestamp)>,
    insertion: Option<ResolvedDocument>,
    registry: IndexRegistry,
}

impl Update<'_> {
    pub fn apply(self) -> Vec<DatabaseIndexUpdate> {
        self.index.finish_update(self.deletion, self.insertion)
    }

    pub fn registry(&self) -> &IndexRegistry {
        &self.registry
    }
}

#[async_trait]
pub trait TransactionTextSnapshot: Send + Sync + 'static {
    // Search at the given snapshot after applying the given writes.
    async fn search(
        &self,
        index: &Index,
        search: &InternalSearch,
        version: SearchVersion,
        // Note that we have to send the writes since we maintain an extremely high
        // bar of determinism - we expect the exact same result regardless if you
        // perform a query from a mutation with some pending writes, or a query after the
        // writes have been committed to the database. The easiest way to achieve
        // this is to send all pending writes back to the backend. This should be fine
        // in practice since mutations with a lot of writes *and* a lot searches
        // should be rare.
        // As a potential future optimization, we could try to make the caller much
        // more coupled with the search algorithm and require it to send bm25 statistics
        // diff, shortlisted query terms and other search specific properties derived
        // from the writes. Alternatively, we could only do subset of that and relax the
        // determinism requirement since we don't really need to have deterministic between
        // search calls in mutations and search calls in queries, and if anyone relies on
        // this they will get random differences due to parallel writes that alter the
        // statistics anyway.
        pending_updates: &Vec<DocumentUpdate>,
    ) -> anyhow::Result<QueryResults>;
}

#[derive(Clone)]
pub struct TextIndexManagerSnapshot {
    index_registry: IndexRegistry,
    text_indexes: TextIndexManager,

    searcher: Arc<dyn Searcher>,
    search_storage: Arc<OnceLock<Arc<dyn Storage>>>,
}

impl TextIndexManagerSnapshot {
    pub fn new(
        index_registry: IndexRegistry,
        text_indexes: TextIndexManager,
        searcher: Arc<dyn Searcher>,
        search_storage: Arc<OnceLock<Arc<dyn Storage>>>,
    ) -> Self {
        Self {
            index_registry,
            text_indexes,
            searcher,
            search_storage,
        }
    }

    // Applies the writes to the base snapshot and returns the new snapshot.
    fn snapshot_with_updates(
        &self,
        pending_updates: &Vec<DocumentUpdate>,
    ) -> anyhow::Result<TextIndexManager> {
        let mut text_indexes = self.text_indexes.clone();
        for DocumentUpdate {
            id: _,
            old_document,
            new_document,
        } in pending_updates
        {
            text_indexes.update(
                &self.index_registry,
                old_document.as_ref(),
                new_document.as_ref(),
                WriteTimestamp::Pending,
            )?;
        }
        Ok(text_indexes)
    }

    fn search_storage(&self) -> Arc<dyn Storage> {
        self.search_storage
            .get()
            .expect("search_storage not initialized")
            .clone()
    }

    #[fastrace::trace]
    pub async fn text_search(
        &self,
        index: &Index,
        printable_index_name: &IndexName,
        query: pb::searchlight::TextQuery,
        pending_updates: &Vec<DocumentUpdate>,
    ) -> anyhow::Result<TextSearchResults> {
        let text_indexes_snapshot =
            runtime::block_in_place(|| self.snapshot_with_updates(pending_updates))?;
        text_indexes_snapshot
            .text_search(
                index,
                printable_index_name,
                query,
                self.searcher.clone(),
                self.search_storage(),
            )
            .await
    }
}

#[async_trait]
impl TransactionTextSnapshot for TextIndexManagerSnapshot {
    async fn search(
        &self,
        index: &Index,
        search: &InternalSearch,
        version: SearchVersion,
        pending_updates: &Vec<DocumentUpdate>,
    ) -> anyhow::Result<QueryResults> {
        let text_indexes_snapshot = self.snapshot_with_updates(pending_updates)?;
        text_indexes_snapshot
            .search(
                index,
                search,
                self.searcher.clone(),
                self.search_storage(),
                version,
            )
            .await
    }
}

pub struct SearchNotEnabled;

#[async_trait]
impl TransactionTextSnapshot for SearchNotEnabled {
    async fn search(
        &self,
        _index: &Index,
        _search: &InternalSearch,
        _version: SearchVersion,
        _pending_updates: &Vec<DocumentUpdate>,
    ) -> anyhow::Result<QueryResults> {
        anyhow::bail!("search not implemented in db-info")
    }
}
