use std::{
    collections::BTreeMap,
    fmt::Debug,
    ops::RangeBounds,
    sync::{
        Arc,
        LazyLock,
    },
};

use async_trait::async_trait;
use common::{
    bootstrap_model::index::{
        database_index::{
            DatabaseIndexSpec,
            DatabaseIndexState,
        },
        IndexConfig,
    },
    document::{
        IndexKeyBuffer,
        PackedDocument,
    },
    document_index_keys::DatabaseIndexWrite,
    index::IndexKeyBytes,
    interval::{
        EndRef,
        Interval,
        IntervalSet,
        StartIncluded,
    },
    knobs::{
        INDEX_CACHE_VERIFY_PERCENT,
        MAX_TRANSACTION_CACHE_SIZE_BYTES,
    },
    query::{
        CursorPosition,
        Order,
    },
    runtime::{
        assert_send,
        try_join,
    },
    static_span,
    types::{
        IndexId,
        IndexName,
        IndexRef,
        RepeatableTimestamp,
        TabletIndexName,
        Timestamp,
    },
    utils::ReadOnly,
};
use errors::ErrorMetadata;
use fastrace::local::LocalSpan;
use futures::{
    stream,
    StreamExt as _,
};
use imbl::OrdMap;
use itertools::Itertools;
use value::{
    DeveloperDocumentId,
    TableMapping,
    TabletId,
};

use crate::{
    in_memory_indexes::{
        InMemoryIndexes,
        LazyDocument,
        MemoryDocument,
    },
    index_cache::IndexCacheHandle,
    index_reader::{
        IndexEntry,
        IndexPage,
        IndexReader,
        RangeRequest,
    },
    index_registry::IndexRegistry,
    metrics::{
        log_index_cache_cleared,
        log_transaction_cache_query,
        log_transaction_index_cache_retained_size,
        log_transaction_index_cache_size,
    },
};

struct IndexCacheReader {
    reader: Arc<dyn IndexReader>,
    handle: IndexCacheHandle,
    index_registry: ReadOnly<IndexRegistry>,
}

impl IndexCacheHandle {
    /// Constructs an `IndexReader` that reads through from `reader` and caches
    /// the result.
    pub fn caching_index_reader(
        self,
        reader: Arc<dyn IndexReader>,
        index_registry: IndexRegistry,
    ) -> impl IndexReader {
        IndexCacheReader {
            reader,
            handle: self,
            index_registry: ReadOnly::new(index_registry),
        }
    }
}

/// Deliberately logs only document ids, timestamps, and sizes — never index
/// keys or document values, which are user data — plus a category for the first
/// divergence. The `diff_kind` lets us distinguish a stale *value* (a write
/// that wasn't invalidated) from a same-value/different-*ts* rewrite, and a
/// shifted key (insert/delete) from an in-place change, which point at
/// different bugs.
#[allow(clippy::too_many_arguments)]
fn log_index_page_mismatch(
    index_id: IndexId,
    index_name: &Option<TabletIndexName>,
    tablet_id: TabletId,
    order: Order,
    max_results: usize,
    cache_ts: RepeatableTimestamp,
    snapshot_ts: RepeatableTimestamp,
    cached: &IndexPage,
    persistence: &IndexPage,
) {
    let mut diff_kind = "none";
    let mut diff_index: Option<usize> = None;
    let mut cached_id: Option<DeveloperDocumentId> = None;
    let mut cached_entry_ts: Option<Timestamp> = None;
    let mut cached_size: Option<usize> = None;
    let mut persistence_id: Option<DeveloperDocumentId> = None;
    let mut persistence_entry_ts: Option<Timestamp> = None;
    let mut persistence_size: Option<usize> = None;
    let n = cached.entries.len().max(persistence.entries.len());
    for i in 0..n {
        match (cached.entries.get(i), persistence.entries.get(i)) {
            (Some(c), Some(p)) => {
                if c.key != p.key {
                    diff_kind = "key_mismatch";
                } else if c.value != p.value {
                    diff_kind = if c.ts == p.ts {
                        "value_mismatch_same_ts"
                    } else {
                        "value_mismatch"
                    };
                } else if c.ts != p.ts {
                    diff_kind = "ts_only_mismatch";
                } else {
                    continue;
                }
                diff_index = Some(i);
                cached_id = Some(c.value.developer_id());
                cached_entry_ts = Some(c.ts);
                cached_size = Some(c.value.size());
                persistence_id = Some(p.value.developer_id());
                persistence_entry_ts = Some(p.ts);
                persistence_size = Some(p.value.size());
                break;
            },
            (Some(c), None) => {
                diff_kind = "cache_has_extra";
                diff_index = Some(i);
                cached_id = Some(c.value.developer_id());
                cached_entry_ts = Some(c.ts);
                cached_size = Some(c.value.size());
                break;
            },
            (None, Some(p)) => {
                diff_kind = "persistence_has_extra";
                diff_index = Some(i);
                persistence_id = Some(p.value.developer_id());
                persistence_entry_ts = Some(p.ts);
                persistence_size = Some(p.value.size());
                break;
            },
            (None, None) => break,
        }
    }
    tracing::warn!(
        index_id = ?index_id,
        index_name = ?index_name,
        tablet_id = ?tablet_id,
        order = ?order,
        max_results,
        cache_ts = %cache_ts,
        snapshot_ts = %snapshot_ts,
        cached_len = cached.entries.len(),
        persistence_len = persistence.entries.len(),
        cursors_match = cached.cursor == persistence.cursor,
        diff_kind,
        diff_index = ?diff_index,
        cached_id = ?cached_id,
        cached_entry_ts = ?cached_entry_ts,
        cached_size = ?cached_size,
        persistence_id = ?persistence_id,
        persistence_entry_ts = ?persistence_entry_ts,
        persistence_size = ?persistence_size,
        "IndexCache result does not match Persistence",
    );
}

#[async_trait]
impl IndexReader for IndexCacheReader {
    async fn index_page(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        interval: &Interval,
        order: Order,
        max_results: usize,
    ) -> anyhow::Result<IndexPage> {
        let index_id = index.id();
        let interval = Arc::new(interval.clone());
        let maybe_page = self.handle.get(
            index_id,
            interval.clone(),
            self.reader.timestamp(),
            order,
            max_results,
        );
        let index_page = if let Some((cached_page, cache_ts)) = maybe_page {
            let verify_cache_results = cfg!(any(test, feature = "testing"))
                || rand::random_range(0..100) < *INDEX_CACHE_VERIFY_PERCENT;
            if verify_cache_results {
                let index_page = self
                    .reader
                    .index_page(index, tablet_id, &interval, order, max_results)
                    .await?;
                if index_page != cached_page {
                    let index_name = self
                        .index_registry
                        .enabled_index_by_index_id(&index_id)
                        .map(|index| index.name());
                    log_index_page_mismatch(
                        index_id,
                        &index_name,
                        tablet_id,
                        order,
                        max_results,
                        cache_ts,
                        self.reader.timestamp(),
                        &cached_page,
                        &index_page,
                    );
                    // Panic if there is an inconsistency between the cache and the persistence
                    // layer. This means there is likely data corruption.
                    panic!(
                        "IndexCache result does not match Persistence index_page for index_id \
                         {:?} tablet_id {:?} interval {:?} order {:?} max_results {} begin_ts {:?}",
                        index_id, tablet_id, interval, order, max_results, cache_ts,
                    );
                }
            }
            cached_page
        } else {
            let index_page = self
                .reader
                .index_page(index, tablet_id, &interval, order, max_results)
                .await?;
            self.handle.populate(
                index_id,
                interval,
                self.reader.timestamp(),
                order,
                max_results,
                index_page.clone(),
                &self.index_registry,
            );
            index_page
        };
        Ok(index_page)
    }

    fn timestamp(&self) -> RepeatableTimestamp {
        self.reader.timestamp()
    }
}

/// Represents the state of the index at a certain snapshot of persistence.
#[derive(Clone)]
pub struct DatabaseIndexSnapshot {
    index_registry: ReadOnly<IndexRegistry>,
    in_memory_indexes: Arc<dyn InMemoryIndexes>,
    table_mapping: ReadOnly<TableMapping>,

    reader: Arc<dyn IndexReader>,

    // Cache results reads from the snapshot. The snapshot is immutable and thus
    // we don't have to do any invalidation.
    cache: DatabaseIndexSnapshotCache,
}

enum RangeFetchResult {
    /// The range was served from an in-memory table.
    /// This happens for tables that are statically configured to be kept in
    /// memory (e.g. `APP_TABLES_TO_LOAD_IN_MEMORY`).
    MemoryCached {
        documents: Vec<(IndexKeyBytes, Timestamp, MemoryDocument)>,
        next_cursor: CursorPosition,
    },
    /// The range was against a non-memory table.
    /// Some documents may still have been served from the
    /// `DatabaseIndexSnapshotCache`.
    NonCached {
        index: IndexRef,
        cache_results: Vec<DatabaseIndexSnapshotCacheResult>,
    },
}

impl DatabaseIndexSnapshot {
    pub fn new(
        index_registry: IndexRegistry,
        in_memory_indexes: Arc<dyn InMemoryIndexes>,
        table_mapping: TableMapping,
        reader: Arc<dyn IndexReader>,
        index_cache_handle: Option<IndexCacheHandle>,
        cache: Option<TimestampedIndexCache>,
    ) -> Self {
        let cache = cache
            .map(|c| c.cache)
            .unwrap_or(DatabaseIndexSnapshotCache::new());
        let reader = if let Some(handle) = index_cache_handle {
            Arc::new(handle.caching_index_reader(reader, index_registry.clone()))
        } else {
            reader
        };
        Self {
            index_registry: ReadOnly::new(index_registry),
            in_memory_indexes,
            table_mapping: ReadOnly::new(table_mapping),
            reader,
            cache,
        }
    }

    pub fn into_cache(self) -> DatabaseIndexSnapshotCache {
        self.cache
    }

    async fn start_range_fetch(
        &self,
        range_request: &RangeRequest,
    ) -> anyhow::Result<RangeFetchResult> {
        let index = match self.index_registry.require_enabled(
            &range_request.index_name,
            &range_request.printable_index_name,
        ) {
            Ok(index) => index,
            Err(e) => {
                // We verify that indexes are enabled at the transaction index layer,
                // so if an index is missing in our `index_registry` (which is from the
                // beginning of the transaction), then it must have been
                // inserted in this transaction. Return an empty result in this
                // condition for all indexes on all tables except the `_index` table, which must
                // always exist.
                if range_request.index_name.table() != &self.index_registry.index_table() {
                    return Ok(RangeFetchResult::MemoryCached {
                        documents: vec![],
                        next_cursor: CursorPosition::End,
                    });
                }
                anyhow::bail!(e);
            },
        };

        // Check that the index is indeed a database index.
        let IndexConfig::Database { on_disk_state, .. } = &index.metadata.config else {
            let err = index_not_a_database_index_error(
                &range_request
                    .index_name
                    .clone()
                    .map_table(&self.table_mapping.tablet_to_name())?,
            );
            anyhow::bail!(err);
        };
        anyhow::ensure!(
            *on_disk_state == DatabaseIndexState::Enabled,
            "Index returned from `require_enabled` but not enabled?"
        );

        // Now that we know it's a database index, serve it from the pinned
        // in-memory index if it's there.
        if let Some(range) = self
            .in_memory_indexes
            .range(
                IndexRef::try_from(&index)?,
                &range_request.interval,
                range_request.order,
                *range_request.index_name.table(),
                range_request.printable_index_name.table().clone(),
            )
            .await?
        {
            return Ok(RangeFetchResult::MemoryCached {
                documents: range,
                next_cursor: CursorPosition::End,
            });
        }

        // Next, try the transaction cache.
        let cache_results =
            self.cache
                .get(index.id(), &range_request.interval, range_request.order);
        Ok(RangeFetchResult::NonCached {
            index: IndexRef::try_from(&index)?,
            cache_results,
        })
    }

    /// Query the given indexes at the snapshot.
    ///
    /// Returns a separate result for each range request in the batch, in the
    /// same order as the input.
    pub async fn range_batch(
        &mut self,
        range_requests: &[&RangeRequest],
    ) -> Vec<
        anyhow::Result<(
            Vec<(IndexKeyBytes, Timestamp, LazyDocument)>,
            CursorPosition,
        )>,
    > {
        // Preallocate the result slots for each input request. This makes it
        // easier to concurrently populate the result vector.
        let mut results: Vec<_> = std::iter::repeat_with(|| {
            // dummy value
            Ok((vec![], CursorPosition::End))
        })
        .take(range_requests.len())
        .collect();

        // Concurrently run each range request, filling in its corresponding
        // slot in `results`.
        let stream = stream::iter(range_requests.iter().zip(&mut results[..]))
            .map(|(range_request, out)| async {
                let populate_cache;
                (*out, populate_cache) = match self.range_fetch(range_request).await {
                    Ok((range_result, populate_cache)) => (Ok(range_result), populate_cache),
                    Err(e) => (Err(e), None),
                };
                stream::iter(populate_cache)
            })
            .buffer_unordered(20)
            .flatten()
            .collect();
        let populate_cache_results: Vec<(IndexId, Vec<(Timestamp, PackedDocument)>, Interval)> =
            assert_send(stream).await; // works around https://github.com/rust-lang/rust/issues/102211

        for (index_id, cache_miss_results, interval_read) in populate_cache_results {
            self.populate_cache_misses(index_id, cache_miss_results, interval_read);
        }

        results
    }

    /// The concurrent part of `range_batch`; the result should be fed into
    /// `populate_cache_misses` afterward
    async fn range_fetch(
        &self,
        range_request: &RangeRequest,
    ) -> anyhow::Result<(
        (
            Vec<(IndexKeyBytes, Timestamp, LazyDocument)>,
            CursorPosition,
        ),
        Option<(IndexId, Vec<(Timestamp, PackedDocument)>, Interval)>,
    )> {
        match self.start_range_fetch(range_request).await? {
            RangeFetchResult::MemoryCached {
                documents,
                next_cursor,
            } => Ok((
                (
                    documents
                        .into_iter()
                        .map(|(key, ts, doc)| (key, ts, LazyDocument::Memory(doc)))
                        .collect(),
                    next_cursor,
                ),
                // There's no need to populate the transaction cache for memory indexes
                None,
            )),
            RangeFetchResult::NonCached {
                index,
                cache_results,
            } => {
                let any_misses = cache_results
                    .iter()
                    .any(|result| matches!(result, DatabaseIndexSnapshotCacheResult::CacheMiss(_)));
                let fut = Self::fetch_cache_misses(
                    self.reader.clone(),
                    index,
                    range_request.clone(),
                    cache_results,
                );
                let (fetch_result_vec, cache_miss_results, cursor) = if any_misses {
                    // Only spawn onto a new task if any database reads are required
                    try_join("fetch_cache_misses", fut).await?
                } else {
                    fut.await?
                };
                // If we actually fetched anything, feed those results into
                // `Self::populate_cache_misses` so we can update the
                // DatabaseIndexSnapshotCache.
                // We can't do that here because we can't mutate `self`
                // during the concurrent phase of the fetch.
                let (interval_read, _) = range_request
                    .interval
                    .split(cursor.clone(), range_request.order);
                Ok((
                    (fetch_result_vec, cursor),
                    Some((index.id(), cache_miss_results, interval_read)),
                ))
            },
        }
    }

    /// `cache_miss_results` contains all the documents in the range
    /// `interval_read` in `index_id` that are *not* already cached
    fn populate_cache_misses(
        &mut self,
        index_id: IndexId,
        cache_miss_results: Vec<(Timestamp, PackedDocument)>,
        interval_read: Interval,
    ) {
        let mut buffer = IndexKeyBuffer::new();
        for (ts, doc) in cache_miss_results {
            // Populate all index point lookups that can result in the given
            // document.
            for index in self
                .index_registry
                .enabled_indexes_for_table(doc.id().tablet_id)
            {
                let IndexConfig::Database {
                    spec: DatabaseIndexSpec { fields },
                    ..
                } = &index.metadata.config
                else {
                    continue;
                };
                let index_key = doc.index_key(&fields[..], &mut buffer);
                self.cache.populate(
                    index.id(),
                    index.metadata.name.is_by_id(),
                    index_key,
                    ts,
                    doc.clone(),
                );
            }
        }
        // After all documents in an index interval have been
        // added to the cache with `populate_cache`, record the entire interval as
        // being populated.
        self.cache
            .record_interval_populated(index_id, interval_read);
    }

    async fn fetch_cache_misses(
        reader: Arc<dyn IndexReader>,
        index: IndexRef,
        range_request: RangeRequest,
        cache_results: Vec<DatabaseIndexSnapshotCacheResult>,
    ) -> anyhow::Result<(
        Vec<(IndexKeyBytes, Timestamp, LazyDocument)>,
        Vec<(Timestamp, PackedDocument)>,
        CursorPosition,
    )> {
        let mut results = vec![];
        let mut cache_miss_results = vec![];
        let mut traced = false;
        for cache_result in cache_results {
            match cache_result {
                DatabaseIndexSnapshotCacheResult::Document(index_key, ts, document) => {
                    // Serve from cache.
                    log_transaction_cache_query(true);
                    results.push((index_key, ts, LazyDocument::Packed(document)));
                },
                DatabaseIndexSnapshotCacheResult::CacheMiss(interval) => {
                    log_transaction_cache_query(false);
                    if !traced {
                        LocalSpan::add_property(|| {
                            ("index", range_request.printable_index_name.to_string())
                        });
                        traced = true;
                    }
                    // Query persistence.
                    let index_page = reader
                        .index_page(
                            index,
                            *range_request.index_name.table(),
                            &interval,
                            range_request.order,
                            range_request.max_size,
                        )
                        .await?;
                    for entry in index_page.entries {
                        let IndexEntry { key, ts, value } = Arc::unwrap_or_clone(entry);
                        cache_miss_results.push((ts, value.clone()));
                        results.push((key, ts, LazyDocument::Packed(value)));
                    }
                },
            }
            if results.len() >= range_request.max_size {
                let last_key = results
                    .last()
                    .expect("should be at least one result")
                    .0
                    .clone();
                return Ok((results, cache_miss_results, CursorPosition::After(last_key)));
            }
        }
        Ok((results, cache_miss_results, CursorPosition::End))
    }

    pub fn timestamp(&self) -> RepeatableTimestamp {
        self.reader.timestamp()
    }
}

static MAX_TRANSACTION_CACHE_SIZE: LazyLock<usize> =
    LazyLock::new(|| *MAX_TRANSACTION_CACHE_SIZE_BYTES);

#[derive(Clone)]
pub struct DatabaseIndexSnapshotCache {
    /// Cache structure:
    /// Each document is stored, keyed by its index key for each index.
    /// Then for each index we have a set of intervals that are fully populated.
    /// The documents are populated first, then the intervals that contain them.
    ///
    /// For example, suppose a query does
    /// db.query('users').withIndex('by_age', q=>q.gt('age', 18)).collect()
    ///
    /// This will first populate `documents` with
    /// by_age -> <age:30, id:alice> -> (ts:100, { <alice document> })
    /// by_id -> <id:alice> -> (ts:100, { <alice document> })
    /// And it will populate the intervals:
    /// by_age -> <age:30, id:alice>
    /// by_id -> <id:alice>
    /// And it will do this for each document found.
    /// After the query is complete, we insert the final interval, which merges
    /// with the existing intervals:
    /// by_age -> (<age:18>, Unbounded)
    ///
    /// After the cache has been fully populated, `db.get`s which do point
    /// queries against by_id will be cached, and any indexed query against
    /// by_age that is a subset of (<age:18>, Unbounded) will be cached.
    documents: OrdMap<IndexId, IndexDocuments>,
    cache_size: usize,
}

#[derive(Clone, Default)]
struct IndexDocuments {
    docs: OrdMap<IndexKeyBytes, (Timestamp, PackedDocument)>,
    interval_set: IntervalSet,
    /// Only tracked for by_id indexes.
    total_size: Option<usize>,
}

impl IndexDocuments {
    fn insert(&mut self, key: IndexKeyBytes, ts: Timestamp, doc: PackedDocument) {
        if let Some(ref mut size) = self.total_size {
            *size += doc.value().size();
        }
        self.docs.insert(key, (ts, doc));
    }

    fn remove(&mut self, key: &IndexKeyBytes) -> Option<(Timestamp, PackedDocument)> {
        let removed = self.docs.remove(key);
        if let Some((_, ref doc)) = removed
            && let Some(ref mut size) = self.total_size
        {
            *size = size.saturating_sub(doc.value().size());
        }
        removed
    }

    fn total_size(&self) -> Option<usize> {
        self.total_size
    }

    fn range(
        &self,
        range: impl RangeBounds<IndexKeyBytes>,
    ) -> impl DoubleEndedIterator<Item = (&IndexKeyBytes, &(Timestamp, PackedDocument))> {
        self.docs.range(range)
    }
}

#[derive(Clone, Debug)]
enum DatabaseIndexSnapshotCacheResult {
    Document(IndexKeyBytes, Timestamp, PackedDocument),
    CacheMiss(Interval),
}

/// How many persistence index scans to do for a single interval.
/// If some results are cached, we can do multiple index scans to avoid
/// re-fetching the cached results. But we don't want to perform too many index
/// scans because there is fixed overhead for each one.
const MAX_CACHED_RANGES_PER_INTERVAL: usize = 3;

impl DatabaseIndexSnapshotCache {
    pub fn new() -> Self {
        Self {
            documents: OrdMap::new(),
            cache_size: 0,
        }
    }

    pub fn is_index_tracked(&self, index_id: &IndexId) -> bool {
        self.documents.contains_key(index_id)
    }

    pub fn tracked_index_ids(&self) -> impl Iterator<Item = IndexId> + '_ {
        self.documents.keys().copied()
    }

    /// Returns false if the cache is over the max size so the cache didn't
    /// populate.
    fn populate(
        &mut self,
        index_id: IndexId,
        is_by_id: bool,
        index_key_bytes: &IndexKeyBytes,
        ts: Timestamp,
        doc: PackedDocument,
    ) -> bool {
        if self.cache_size > *MAX_TRANSACTION_CACHE_SIZE {
            return false;
        }
        if is_by_id {
            self.cache_size += doc.value().size();
        }
        let _s = static_span!();
        // Allow cache to exceed max size by one document, so we can detect that
        // the cache has maxed out.
        let interval = Interval::prefix(index_key_bytes.clone().into());
        let index_docs = self
            .documents
            .entry(index_id)
            .or_insert_with(|| IndexDocuments {
                total_size: if is_by_id { Some(0) } else { None },
                ..Default::default()
            });
        index_docs.insert(index_key_bytes.clone(), ts, doc);
        index_docs.interval_set.add(interval);
        true
    }

    fn record_interval_populated(&mut self, index_id: IndexId, interval: Interval) {
        if self.cache_size <= *MAX_TRANSACTION_CACHE_SIZE {
            self.documents
                .entry(index_id)
                .or_default()
                .interval_set
                .add(interval);
        }
    }

    fn get(
        &self,
        index_id: IndexId,
        interval: &Interval,
        order: Order,
    ) -> Vec<DatabaseIndexSnapshotCacheResult> {
        let index_docs = match self.documents.get(&index_id) {
            None => {
                return vec![DatabaseIndexSnapshotCacheResult::CacheMiss(
                    interval.clone(),
                )]
            },
            Some(index_docs) => index_docs,
        };
        let components = index_docs
            .interval_set
            .split_interval_components(interval.as_ref());
        let mut results = vec![];
        let mut cache_hit_count = 0;
        for (in_set, component_interval) in components {
            // There are better ways to pick which cached intervals to use
            // (use the biggest ones, allow an extra if it's at the end),
            // but those are more complicated to implement so we can improve when a
            // use-case requires it. For now we pick the first cached ranges
            // until we hit `MAX_CACHED_RANGES_PER_INTERVAL`.
            if cache_hit_count >= MAX_CACHED_RANGES_PER_INTERVAL {
                results.push(DatabaseIndexSnapshotCacheResult::CacheMiss(Interval {
                    start: StartIncluded(component_interval.start.to_vec().into()),
                    end: interval.end.clone(),
                }));
                break;
            }
            if in_set {
                cache_hit_count += 1;
                let range = index_docs
                    .range(
                        // TODO: `to_vec()` is not necessary
                        IndexKeyBytes(component_interval.start.to_vec())..,
                    )
                    .take_while(|&(key, _)| match &component_interval.end {
                        EndRef::Excluded(end) => key[..] < end[..],
                        EndRef::Unbounded => true,
                    });
                results.extend(range.map(|(index_key, (ts, doc))| {
                    DatabaseIndexSnapshotCacheResult::Document(index_key.clone(), *ts, doc.clone())
                }));
            } else {
                results.push(DatabaseIndexSnapshotCacheResult::CacheMiss(
                    component_interval.to_owned(),
                ));
            }
        }
        order.apply(results.into_iter()).collect_vec()
    }

    /// Remove all documents for a single index from the cache.
    pub fn remove_index(&mut self, index_id: IndexId) {
        if let Some(index_docs) = self.documents.remove(&index_id)
            && let Some(size) = index_docs.total_size()
        {
            self.cache_size = self.cache_size.saturating_sub(size);
        }
    }

    /// Apply a single write to the cache. Returns `false` if the cache was
    /// cleared due to exceeding the size limit (caller should stop).
    pub fn apply_write(
        &mut self,
        ts: Timestamp,
        index_id: IndexId,
        is_by_id: bool,
        write: &DatabaseIndexWrite,
    ) -> bool {
        // Remove old entry from cache.
        if let Some(old_key) = &write.update.old
            && let Some(index_docs) = self.documents.get_mut(&index_id)
            && let Some((_, old_doc)) = index_docs.remove(old_key)
            && is_by_id
        {
            self.cache_size = self.cache_size.saturating_sub(old_doc.value().size());
        }
        // Insert new entry if not a delete, but only for indexes where the
        // key falls within the range the cache is already tracking.
        if let Some(doc) = &write.new_document
            && let Some(new_key) = &write.update.new
            && let Some(index_docs) = self.documents.get(&index_id)
            && index_docs.interval_set.contains(new_key)
        {
            // If the cache is too big, empty the cache
            if !self.populate(index_id, is_by_id, new_key, ts, doc.clone()) {
                log_index_cache_cleared();
                *self = Self::new();
                return false;
            }
        }
        true
    }

    /// Shrink the cache to only the intervals in `keep` (the finished
    /// transaction's read set, keyed by index id).
    ///
    /// This is only sound for a *reused* cache: one extracted from a finished
    /// transaction and reused for the next, near-identical one, as the
    /// scheduled job executor does. There the read set is the best predictor of
    /// what the next transaction will read, so anything outside it is dead
    /// weight. It would be the wrong policy for general within-transaction
    /// caching, where speculative cross-index population is the whole point
    /// (read `by_age`, then `db.get(id)`).
    pub fn retain_read_intervals(&mut self, keep: &BTreeMap<IndexId, IntervalSet>) {
        log_transaction_index_cache_size(self.cache_size);
        let index_ids: Vec<IndexId> = self.documents.keys().copied().collect();
        for index_id in index_ids {
            let Some(keep_intervals) = keep.get(&index_id) else {
                self.remove_index(index_id);
                continue;
            };
            let removed_size = {
                let Some(index_docs) = self.documents.get_mut(&index_id) else {
                    continue;
                };
                // Only retain the intersection of populated and keep_intervals
                let mut retained = IntervalSet::new();
                for populated in index_docs.interval_set.iter() {
                    for (in_set, component) in
                        keep_intervals.split_interval_components(populated.as_ref())
                    {
                        if in_set {
                            retained.add(component.to_owned());
                        }
                    }
                }

                // Rebuild `index_docs` by moving the entries out of the old
                // map and reinserting after we've checked they are in the retained intervals.
                // `index_docs` and `retained` are sorted, so we can do a linear merge to check
                // which entries should be added back.
                let track_size = index_docs.total_size().is_some();
                let old_total = index_docs.total_size().unwrap_or(0);
                let old_docs = std::mem::take(&mut index_docs.docs);
                let new_total = {
                    let mut membership_cursor = retained.membership_cursor();
                    let mut new_total = 0;
                    for (key, value) in old_docs {
                        if membership_cursor.contains(&key[..]) {
                            if track_size {
                                new_total += value.1.value().size();
                            }
                            index_docs.docs.insert(key, value);
                        }
                    }
                    new_total
                };
                index_docs.interval_set = retained;
                if let Some(total) = index_docs.total_size.as_mut() {
                    *total = new_total;
                }
                old_total.saturating_sub(new_total)
            };
            self.cache_size = self.cache_size.saturating_sub(removed_size);
        }
        log_transaction_index_cache_retained_size(self.cache_size);
    }
}

/// [`DatabaseIndexSnapshotCache`] paired with the [`RepeatableTimestamp`] it
/// is valid at.
pub struct TimestampedIndexCache {
    pub cache: DatabaseIndexSnapshotCache,
    pub ts: RepeatableTimestamp,
}

pub fn index_not_a_database_index_error(name: &IndexName) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "IndexNotADatabaseIndex",
        format!("Index {name} is not a database index"),
    )
}
