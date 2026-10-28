use std::{
    cmp,
    collections::{
        BTreeMap,
        BTreeSet,
    },
    ops::Deref,
    sync::Arc,
};

use anyhow::Context;
use common::{
    bootstrap_model::index::database_index::IndexedFields,
    document::ResolvedDocument,
    errors::DatabaseTimeoutError,
    index::{
        IndexKey,
        IndexKeyBytes,
    },
    interval::Interval,
    knobs::{
        DOCUMENTS_IN_MEMORY,
        TABLE_ITERATOR_MAX_RETRIES,
    },
    persistence::{
        new_static_repeatable_recent,
        DocumentLogEntry,
        DocumentPrevTsQuery,
        LatestDocument,
        PersistenceReader,
        RepeatablePersistence,
        RetentionValidator,
        TimestampRange,
    },
    query::{
        CursorPosition,
        Order,
    },
    runtime::Runtime,
    try_chunks::TryChunksExt,
    types::{
        IndexRef,
        RepeatableTimestamp,
        Timestamp,
    },
    value::ResolvedDocumentId,
};
use errors::ErrorMetadataAnyhowExt;
use futures::{
    future::Either,
    pin_mut,
    stream,
    Stream,
    StreamExt,
    TryStreamExt,
};
use futures_async_stream::try_stream;
use value::{
    InternalDocumentId,
    InternalId,
    TabletId,
};

use crate::metrics::page_timer;

/// A cursor for use while scanning a table by ID.
///
/// The key is the last element processed thus far.
#[derive(Clone, Debug, Default)]
pub struct TableScanCursor {
    pub index_key: Option<CursorPosition>,
}

impl TableScanCursor {
    pub fn interval(&self) -> Interval {
        match &self.index_key {
            Some(index_key) => {
                let (_, remaining) = Interval::all().split(index_key.clone(), Order::Asc);
                remaining
            },
            None => Interval::all(),
        }
    }

    pub fn advance(&mut self, index_key: CursorPosition) -> anyhow::Result<()> {
        if let Some(existing_key) = &self.index_key {
            anyhow::ensure!(index_key > existing_key);
        }
        self.index_key = Some(index_key);
        Ok(())
    }
}

fn cursor_has_walked(cursor: Option<&CursorPosition>, key: &IndexKeyBytes) -> bool {
    match cursor {
        None => false,
        Some(CursorPosition::End) => true,
        Some(CursorPosition::After(cursor)) => key <= cursor,
    }
}

pub struct TableIterator<RT: Runtime> {
    inner: TableIteratorInner<RT>,
}

impl<RT: Runtime> TableIterator<RT> {
    pub fn new(
        runtime: RT,
        snapshot_ts: RepeatableTimestamp,
        persistence: Arc<dyn PersistenceReader>,
        retention_validator: Arc<dyn RetentionValidator>,
        page_size: usize,
    ) -> Self {
        Self {
            inner: TableIteratorInner {
                runtime,
                persistence,
                retention_validator,
                page_size,
                snapshot_ts,
            },
        }
    }

    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.inner.page_size = page_size;
        self
    }

    /// Create a `MultiTableIterator`, which can iterate multiple tables at the
    /// same snapshot timestamp. This is more efficient than creating a separate
    /// `TableIterator` for each table since each table can share the work of
    /// iterating the document log.
    ///
    /// The iterator will only be able to visit those tables passed to `multi`.
    /// Trying to visit a table that wasn't initially specified will error.
    ///
    /// The iterator keeps some state in memory for each of the `tables`
    /// provided. To reduce memory usage, you can call
    /// [`MultiTableIterator::unregister_table`] if you know that a given table
    /// will not be iterated again.
    ///
    /// Example:
    /// ```no_run
    /// # async fn iterate_example<RT: common::runtime::Runtime>(
    /// #     db: &database::Database<RT>,
    /// #     [table1, table2, table3]: [value::TabletId; 3],
    /// #     by_id: common::types::IndexRef,
    /// #     ts: common::types::RepeatableTimestamp,
    /// # ) -> anyhow::Result<()> {
    /// # use futures::stream::TryStreamExt;
    /// # use std::pin::pin;
    /// let tables = vec![table1, table2, table3];
    /// let page_size = 100;
    /// let mut iterator = db.table_iterator(ts, page_size).multi(tables.clone());
    /// for tablet_id in tables {
    ///     iterator.stream_documents_in_table(tablet_id, by_id, None).try_for_each(async |doc| {
    ///         // handle doc
    ///         Ok(())
    ///     }).await?;
    ///     iterator.unregister_table(tablet_id);
    ///     // afterward, `iterator` can no longer visit `tablet_id`
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn multi(self, tables: Vec<TabletId>) -> MultiTableIterator<RT> {
        MultiTableIterator {
            end_ts: self.inner.snapshot_ts,
            buffered_documents: tables
                .into_iter()
                .map(|tablet_id| (tablet_id, BTreeMap::new()))
                .collect(),
            inner: self.inner,
        }
    }

    pub async fn fetch_page(
        &self,
        index_id: IndexRef,
        tablet_id: TabletId,
        cursor: &mut TableScanCursor,
    ) -> anyhow::Result<(Vec<(IndexKeyBytes, LatestDocument)>, RepeatableTimestamp)> {
        self.inner.fetch_page(index_id, tablet_id, cursor).await
    }

    #[try_stream(ok = LatestDocument, error = anyhow::Error)]
    pub async fn stream_documents_in_table(
        self,
        tablet_id: TabletId,
        by_id: IndexRef,
        cursor: Option<ResolvedDocumentId>,
    ) {
        let mut iterator = self.multi(vec![]);
        let stream = iterator.stream_documents_in_table(tablet_id, by_id, cursor);
        pin_mut!(stream);
        while let Some(rev) = stream.try_next().await? {
            yield rev;
        }
    }

    #[try_stream(ok = (IndexKeyBytes, LatestDocument), error = anyhow::Error)]
    pub async fn stream_documents_in_table_by_index(
        self,
        tablet_id: TabletId,
        index_id: IndexRef,
        indexed_fields: IndexedFields,
        cursor: Option<CursorPosition>,
    ) {
        let mut iterator = self.multi(vec![]);
        let stream = iterator.stream_documents_in_table_by_index(
            tablet_id,
            index_id,
            indexed_fields,
            cursor,
        );
        pin_mut!(stream);
        while let Some(rev) = stream.try_next().await? {
            yield rev;
        }
    }
}

/// Walks a table by reading each page at the latest repeatable timestamp,
/// with no snapshot to reconstruct: unlike [`TableIterator`], it never replays
/// the document log, so its cost stays flat under concurrent writes.
///
/// The walk is NOT a consistent snapshot of the table. Page timestamps are
/// weakly monotonic and at least `min_ts`, each document is its latest
/// revision as of its page's timestamp, and a document modified concurrently
/// with the walk may be observed at either revision (or, for an insert or
/// delete, in either state). Every document that exists unchanged for the
/// whole walk is yielded exactly once, in `by_id` order.
pub struct LatestTableIterator<RT: Runtime> {
    runtime: RT,
    /// Lower bound on page timestamps, so the walk observes every write that
    /// committed before it. The latest repeatable timestamp in persistence
    /// advances only periodically and can lag behind recent commits.
    min_ts: RepeatableTimestamp,
    persistence: Arc<dyn PersistenceReader>,
    retention_validator: Arc<dyn RetentionValidator>,
    page_size: usize,
    latest_ts_source: LatestTsSource,
}

/// Supplies the "recent" half of a page's read timestamp, saving the
/// persistence round trip that reading the persisted max repeatable timestamp
/// would cost per page. It must return timestamps that are repeatable, within
/// retention, and weakly monotonic across calls;
/// `Database::now_ts_for_reads` qualifies because the committer publishes a
/// timestamp only after its writes are durable, and it is at least the
/// persisted repeatable timestamp.
pub type LatestTsSource = Arc<dyn Fn() -> RepeatableTimestamp + Send + Sync>;

impl<RT: Runtime> LatestTableIterator<RT> {
    pub fn new(
        runtime: RT,
        min_ts: RepeatableTimestamp,
        persistence: Arc<dyn PersistenceReader>,
        retention_validator: Arc<dyn RetentionValidator>,
        page_size: usize,
        latest_ts_source: LatestTsSource,
    ) -> Self {
        Self {
            runtime,
            min_ts,
            persistence,
            retention_validator,
            page_size,
            latest_ts_source,
        }
    }

    /// Stream every document in the table with the timestamp its page was
    /// read at.
    #[try_stream(ok = (LatestDocument, RepeatableTimestamp), error = anyhow::Error)]
    pub async fn stream_documents_in_table(self, tablet_id: TabletId, by_id: IndexRef) {
        let mut cursor = TableScanCursor::default();
        loop {
            let pause_client = self.runtime.pause_client();
            pause_client.wait("before_latest_page").await;
            let (page, ts) = fetch_page(
                &self.persistence,
                &self.retention_validator,
                self.page_size,
                self.min_ts,
                Some(&self.latest_ts_source),
                by_id,
                tablet_id,
                &mut cursor,
            )
            .await?;
            for (_, doc) in page {
                tokio::task::consume_budget().await;
                yield (doc, ts);
            }
            if matches!(cursor.index_key, Some(CursorPosition::End)) {
                break;
            }
        }
    }
}

struct TableIteratorInner<RT: Runtime> {
    runtime: RT,
    persistence: Arc<dyn PersistenceReader>,
    retention_validator: Arc<dyn RetentionValidator>,
    page_size: usize,
    snapshot_ts: RepeatableTimestamp,
}
pub struct MultiTableIterator<RT: Runtime> {
    inner: TableIteratorInner<RT>,
    end_ts: RepeatableTimestamp,
    // Buffered document metadata between `snapshot_ts` and `end_ts`.
    // This is useful because there is no way to read the document log for just
    // a single table.
    buffered_documents: BufferedDocumentMetadata,
}

// For each table that we are interested in, stores a map containing every
// observed id in that table and its first prev_ts (or None if the first row had
// no prev_ts)
type BufferedDocumentMetadata = BTreeMap<TabletId, BTreeMap<InternalId, Option<PrevTs>>>;

#[derive(Copy, Clone, Debug)]
struct PrevTs {
    prev_ts: Timestamp,
}

impl<RT: Runtime> MultiTableIterator<RT> {
    /// Signal that the given `tablet_id` will not be iterated in the future.
    /// The `tablet_id` must have been provided during this iterator's
    /// construction.
    ///
    /// Calling this is an optimization to reduce memory usage by dropping the
    /// list of changed documents.
    pub fn unregister_table(&mut self, tablet_id: TabletId) -> anyhow::Result<()> {
        self.buffered_documents
            .remove(&tablet_id)
            .context("unregister_table on an unknown table")?;
        Ok(())
    }

    #[try_stream(ok = LatestDocument, error = anyhow::Error)]
    pub async fn stream_documents_in_table(
        &mut self,
        tablet_id: TabletId,
        by_id: IndexRef,
        cursor: Option<ResolvedDocumentId>,
    ) {
        let stream = self.stream_documents_in_table_by_index(
            tablet_id,
            by_id,
            IndexedFields::by_id(),
            cursor.map(|id| CursorPosition::After(IndexKey::new(vec![], id.into()).to_bytes())),
        );
        pin_mut!(stream);
        while let Some((_, rev)) = stream.try_next().await? {
            yield rev;
        }
    }

    /// Wrapper for `stream_documents_in_table` that takes `self` by value, and
    /// yields it back when the stream is finished. This is helpful because the
    /// resulting stream is `'static`.
    #[try_stream(ok = Either<LatestDocument, Self>, error = anyhow::Error)]
    pub async fn into_stream_documents_in_table(
        mut self,
        tablet_id: TabletId,
        by_id: IndexRef,
        cursor: Option<ResolvedDocumentId>,
    ) {
        {
            let stream = self.stream_documents_in_table(tablet_id, by_id, cursor);
            pin_mut!(stream);
            while let Some(rev) = stream.try_next().await? {
                yield Either::Left(rev);
            }
        }
        yield Either::Right(self);
    }

    /// Algorithm overview:
    /// Walk a table ordered by an index at snapshot_ts which may be outside
    /// retention, so you can't walk the index directly.
    ///
    /// 1. Walk the index at recent, increasing snapshots, and
    /// collect documents that have not changed since snapshot_ts.
    /// 2. Also walk the document log between snapshot_ts and the increasing
    /// snapshots, and for each changed document look at its revision as of
    /// snapshot_ts.
    /// 3. Merge the results.
    ///
    /// Why it works:
    /// Consider a document that exists in the index at snapshot_ts.
    /// Either it has changed since snapshot_ts, in which case (2) will find
    /// it, or it has not, in which case (1) will find it.
    ///
    /// (2) is implemented by recording the *first* prev_ts (if any) for each
    /// document id encountered in the document log. That prev_ts points to the
    /// document that belongs to the snapshot (or, if null, indicates that the
    /// document is new and doesn't belong in the snapshot).
    ///
    /// The document log can't be filtered by table, so `MultiTableIterator`
    /// additionally remembers this metadata for every table that the caller is
    /// interested in. This allows each subsequent table iteration to continue
    /// where the previous walk left off.
    #[try_stream(ok = (IndexKeyBytes, LatestDocument), error = anyhow::Error)]
    pub async fn stream_documents_in_table_by_index(
        &mut self,
        tablet_id: TabletId,
        index_id: IndexRef,
        indexed_fields: IndexedFields,
        cursor: Option<CursorPosition>,
    ) {
        let mut cursor = TableScanCursor { index_key: cursor };

        // skipped_keys are keys of documents that were modified after
        // snapshot_ts but before the index key was walked over.
        // Such documents must be
        // (a) modified/deleted after snapshot_ts but before new_end_ts.
        // (b) have key > cursor.
        // We insert skipped documents into future pages of the index walk when we get
        // to them.
        let mut skipped_keys = IterationDocuments::default();
        // observed_ids is the set of document IDs in the table between
        // `(snapshot_ts, end_ts]`, and also corresponds to all the documents
        // that may have been inserted into `skipped_keys` (if present in the
        // snapshot & not already walked)
        let mut observed_ids: BTreeSet<InternalId> = BTreeSet::new();

        if let Some(buffered_documents) = self.buffered_documents.get(&tablet_id) {
            // We've already walked the document log and stored some document
            // timestamps for this table. Load those documents and prepopulate
            // `skipped_keys`.
            observed_ids.extend(buffered_documents.keys().copied());
            let mut revisions_at_snapshot = self
                .inner
                .load_revisions_at_snapshot_ts(stream::iter(buffered_documents.iter().filter_map(
                    |(&id, &timestamps)| {
                        timestamps.map(|ts| Ok((InternalDocumentId::new(tablet_id, id), ts)))
                    },
                )))
                .boxed(); // `boxed()` instead of `pin_mut!` works around https://github.com/rust-lang/rust/issues/96865
            while let Some(rev) = revisions_at_snapshot.try_next().await? {
                let index_key = rev.value.index_key(&indexed_fields).to_bytes();
                skipped_keys.insert(index_key, rev.ts, rev.value, rev.prev_ts);
            }
        } else {
            // As a special case, the very first table visited by a
            // `MultiTableIterator` is allowed to be any table, even if it
            // wasn't specified. This is just to support the single-table
            // iteration methods on `TableIterator`.
            anyhow::ensure!(
                self.inner.snapshot_ts == self.end_ts,
                "this MultiTableIterator has already advanced from {snapshot_ts} to {end_ts}, but \
                 it has no buffered documents for table {tablet_id}",
                snapshot_ts = self.inner.snapshot_ts,
                end_ts = self.end_ts
            );
        }

        loop {
            let pause_client = self.inner.runtime.pause_client();
            pause_client.wait("before_index_page").await;
            let page_start = cursor.index_key.clone();
            let (page, new_end_ts) = self
                .inner
                .fetch_page(index_id, tablet_id, &mut cursor)
                .await?;
            anyhow::ensure!(*new_end_ts >= self.end_ts);
            let page_end = cursor
                .index_key
                .as_ref()
                .context("cursor after page should not be empty")?;
            // Filter out rows from the index scan that were modified after
            // snapshot_ts. Such documents will be found when walking the
            // documents log to generate skipped_keys.
            let page: BTreeMap<_, _> = page
                .into_iter()
                .filter(|(_, rev)| rev.ts <= *self.inner.snapshot_ts)
                .map(|(index_key, LatestDocument { ts, value, prev_ts })| {
                    (index_key, (ts, IterationDocument::Full { value, prev_ts }))
                })
                .collect();

            // 2. Find any keys for documents that were skipped by this
            // page or will be skipped by future pages.
            // These documents are returned with index keys and revisions as
            // they existed at snapshot_ts.
            self.inner
                .fetch_skipped_keys(
                    tablet_id,
                    &indexed_fields,
                    page_start.as_ref(),
                    *self.end_ts,
                    new_end_ts,
                    &mut skipped_keys,
                    &mut observed_ids,
                    &mut self.buffered_documents,
                )
                .await?;
            if let Some((first_skipped_key, _)) = skipped_keys.iter().next() {
                // Check all skipped ids are after the old cursor,
                // which ensures the yielded output is in index key order.
                anyhow::ensure!(!cursor_has_walked(page_start.as_ref(), first_skipped_key));
            }
            self.end_ts = new_end_ts;
            // Extract the documents from skipped_keys that should be returned in
            // the current page.
            let page_skipped_keys = {
                let mut page_skipped_keys = BTreeMap::new();
                while let Some(first_skipped_key) = skipped_keys.keys().next()
                    && cursor_has_walked(Some(page_end), first_skipped_key)
                {
                    let (key, value) = skipped_keys
                        .remove(&first_skipped_key.clone())
                        .context("skipped_keys should be nonempty")?;
                    page_skipped_keys.insert(key, value);
                }
                page_skipped_keys
            };
            // Merge index walk and skipped keys into BTreeMap, which sorts by index key.
            let merged_page =
                IterationDocuments::new(page.into_iter().chain(page_skipped_keys).collect());

            // Sanity check output.
            let all_ids: BTreeSet<_> = merged_page
                .values()
                .map(|(_, doc)| doc.internal_id())
                .collect();
            anyhow::ensure!(
                all_ids.len() == merged_page.len(),
                "duplicate id in table iterator {merged_page:?}"
            );
            anyhow::ensure!(
                merged_page
                    .values()
                    .all(|(ts, _)| *ts <= *self.inner.snapshot_ts),
                "document after snapshot in table iterator {merged_page:?}"
            );
            anyhow::ensure!(
                merged_page.keys().all(|key| {
                    !cursor_has_walked(page_start.as_ref(), key)
                        && cursor_has_walked(Some(page_end), key)
                }),
                "document outside page in table iterator {merged_page:?}"
            );

            let mut merged_page_docs = self.inner.reload_revisions_at_snapshot_ts(merged_page);
            while let Some((key, rev)) = merged_page_docs.try_next().await? {
                // The caller will likely consume the documents in a CPU-intensive loop,
                // and `merged_page_docs.try_next().await` will often be Ready
                // immediately, so it won't yield.
                // Make sure we yield to not starve other tokio tasks.
                tokio::task::consume_budget().await;

                yield (key, rev);
            }
            if matches!(page_end, CursorPosition::End) {
                // If we are done, all skipped_keys would be put in this final page.
                anyhow::ensure!(skipped_keys.is_empty());
                break;
            }
        }
    }
}

impl<RT: Runtime> TableIteratorInner<RT> {
    /// A document may be skipped if:
    /// 1. it is in the correct table
    /// 2. at the snapshot, it had a key higher than what we've walked so far
    /// 3. it was modified after the snapshot but before we walked its key
    /// range.
    #[fastrace::trace(properties = {"start_ts": "{start_ts}", "end_ts": "{end_ts}"})]
    async fn fetch_skipped_keys(
        &self,
        tablet_id: TabletId,
        indexed_fields: &IndexedFields,
        lower_bound: Option<&CursorPosition>,
        start_ts: Timestamp,
        end_ts: RepeatableTimestamp,
        output: &mut IterationDocuments,
        observed_ids: &mut BTreeSet<InternalId>,
        buffered_documents: &mut BufferedDocumentMetadata,
    ) -> anyhow::Result<()> {
        let skipped_revs = self.walk_document_log(
            tablet_id,
            start_ts,
            end_ts,
            observed_ids,
            buffered_documents,
        );
        pin_mut!(skipped_revs);
        let revisions_at_snapshot = self.load_revisions_at_snapshot_ts(skipped_revs);
        pin_mut!(revisions_at_snapshot);
        while let Some(rev) = revisions_at_snapshot.try_next().await? {
            let index_key = rev.value.index_key(indexed_fields).to_bytes();
            if !cursor_has_walked(lower_bound, &index_key) {
                output.insert(index_key, rev.ts, rev.value, rev.prev_ts);
            }
        }
        Ok(())
    }

    #[try_stream(ok = (InternalDocumentId, PrevTs), error = anyhow::Error)]
    async fn walk_document_log<'a>(
        &'a self,
        tablet_id: TabletId,
        start_ts: Timestamp,
        end_ts: RepeatableTimestamp,
        observed_ids: &'a mut BTreeSet<InternalId>,
        buffered_documents: &'a mut BufferedDocumentMetadata,
    ) {
        let reader = self.persistence.clone();
        let repeatable_persistence =
            RepeatablePersistence::new(reader, end_ts, self.retention_validator.clone());
        // TODO: don't fetch document contents from the database
        let documents = repeatable_persistence
            .load_documents(TimestampRange::new(start_ts.succ()?..=*end_ts), Order::Asc);
        pin_mut!(documents);
        while let Some(entry) = documents.try_next().await? {
            if let Some(buffer) = buffered_documents.get_mut(&entry.id.table()) {
                // Don't overwrite any existing entry at `id`
                buffer
                    .entry(entry.id.internal_id())
                    .or_insert(entry.prev_ts.map(|prev_ts| PrevTs { prev_ts }));
            }

            if entry.id.table() == tablet_id {
                // only yield if this is the first time we have seen this ID
                if observed_ids.insert(entry.id.internal_id()) {
                    // If prev_ts is None, we still add to `observed_ids` to
                    // ignore future log entries with this `id`
                    if let Some(prev_ts) = entry.prev_ts {
                        yield (entry.id, PrevTs { prev_ts });
                    }
                }
            }
        }
    }

    async fn fetch_page(
        &self,
        index_id: IndexRef,
        tablet_id: TabletId,
        cursor: &mut TableScanCursor,
    ) -> anyhow::Result<(Vec<(IndexKeyBytes, LatestDocument)>, RepeatableTimestamp)> {
        fetch_page(
            &self.persistence,
            &self.retention_validator,
            self.page_size,
            self.snapshot_ts,
            None,
            index_id,
            tablet_id,
            cursor,
        )
        .await
    }

    /// Load the revisions of documents visible at `self.snapshot_ts`.
    /// Documents are yielded in the same order as input, skipping documents
    /// that didn't exist at the snapshot. Input ids must be unique.
    #[try_stream(ok = LatestDocument, error = anyhow::Error)]
    async fn load_revisions_at_snapshot_ts<'a>(
        &'a self,
        ids: impl Stream<Item = anyhow::Result<(InternalDocumentId, PrevTs)>> + 'a,
    ) {
        let repeatable_persistence = RepeatablePersistence::new(
            self.persistence.clone(),
            self.snapshot_ts,
            self.retention_validator.clone(),
        );

        // Note even though `previous_revisions` can paginate internally, we don't want
        // to hold the entire result set in memory, because documents can be large.
        let id_chunks = ids.try_chunks2(self.page_size);
        pin_mut!(id_chunks);

        let snapshot_ts_succ = self.snapshot_ts.succ()?;
        let make_query =
            |&(id, PrevTs { prev_ts }): &(InternalDocumentId, PrevTs)| DocumentPrevTsQuery {
                id,
                // HAX: we do not remember the `ts` of the original row that this
                // query came from. However, `snapshot_ts_succ` is correct for the
                // purposes of the retention validator since `prev_ts` was latest as of
                // `snapshot_ts`.
                ts: snapshot_ts_succ,
                prev_ts,
            };

        while let Some(chunk) = id_chunks.try_next().await? {
            for q in &chunk {
                anyhow::ensure!(
                    snapshot_ts_succ > q.1.prev_ts,
                    "Querying a prev_ts {prev_ts} that does not lie within the snapshot \
                     {snapshot_ts}",
                    prev_ts = q.1.prev_ts,
                    snapshot_ts = self.snapshot_ts
                );
            }
            let ids_to_load = chunk.iter().map(make_query).collect();
            let mut old_revisions = repeatable_persistence
                .previous_revisions_of_documents(ids_to_load)
                .await?;
            // Yield in the same order as the input
            for q in chunk {
                let DocumentLogEntry {
                    ts, value, prev_ts, ..
                } = old_revisions
                    .remove(&make_query(&q))
                    .with_context(|| format!("Missing revision at snapshot: {q:?}"))?;
                let Some(value) = value else { continue };
                yield LatestDocument { ts, value, prev_ts };
            }
            anyhow::ensure!(
                old_revisions.is_empty(),
                "logic error: unfetched results remain in old_revisions"
            );
        }
    }

    #[try_stream(boxed, ok = (IndexKeyBytes, LatestDocument), error = anyhow::Error)]
    async fn load_index_entries_at_snapshot_ts(
        &self,
        entries: Vec<(InternalDocumentId, Timestamp, IndexKeyBytes)>,
    ) {
        let ids: Vec<_> = entries
            .iter()
            .map(|&(id, ts, _)| (id, PrevTs { prev_ts: ts }))
            .collect();
        let mut key_by_id: BTreeMap<_, _> =
            entries.into_iter().map(|(id, _, key)| (id, key)).collect();
        let revisions = self.load_revisions_at_snapshot_ts(stream::iter(ids.into_iter().map(Ok)));
        pin_mut!(revisions);
        while let Some(rev) = revisions.try_next().await? {
            let key = key_by_id
                .remove(&rev.value.id_with_table_id())
                .context("key_by_id missing")?;
            yield (key, rev);
        }
    }

    /// Like `load_revisions_at_snapshot_ts` but doesn't need to fetch
    /// if the IterationDocument has the Full document.
    #[try_stream(boxed, ok = (IndexKeyBytes, LatestDocument), error = anyhow::Error)]
    async fn reload_revisions_at_snapshot_ts(&self, documents: IterationDocuments) {
        let mut current_batch = Vec::new();
        for (key, (ts, doc)) in documents.into_iter() {
            match doc {
                IterationDocument::Full { value, prev_ts } => {
                    let mut flush = self.load_index_entries_at_snapshot_ts(current_batch);
                    while let Some((key, rev)) = flush.try_next().await? {
                        yield (key, rev);
                    }
                    current_batch = Vec::new();
                    yield (key, LatestDocument { ts, value, prev_ts });
                },
                IterationDocument::Id(id) => {
                    current_batch.push((id, ts, key));
                },
            }
        }
        let mut flush = self.load_index_entries_at_snapshot_ts(current_batch);
        while let Some((key, rev)) = flush.try_next().await? {
            yield (key, rev);
        }
    }
}

#[derive(Debug)]
enum IterationDocument {
    Full {
        value: ResolvedDocument,
        prev_ts: Option<Timestamp>,
    },
    Id(InternalDocumentId),
}

impl IterationDocument {
    fn internal_id(&self) -> InternalId {
        match self {
            Self::Full { value, .. } => value.internal_id(),
            Self::Id(id) => id.internal_id(),
        }
    }
}

/// To avoid storing too many documents in memory, we evict the document values,
/// leaving only the IDs.
#[derive(Default, Debug)]
struct IterationDocuments {
    count_full: usize,
    docs: BTreeMap<IndexKeyBytes, (Timestamp, IterationDocument)>,
}

impl IterationDocuments {
    fn new(docs: BTreeMap<IndexKeyBytes, (Timestamp, IterationDocument)>) -> Self {
        Self {
            count_full: docs
                .values()
                .filter(|(_, doc)| matches!(doc, IterationDocument::Full { .. }))
                .count(),
            docs,
        }
    }

    fn insert(
        &mut self,
        index_key: IndexKeyBytes,
        ts: Timestamp,
        doc: ResolvedDocument,
        prev_ts: Option<Timestamp>,
    ) {
        if self.count_full < *DOCUMENTS_IN_MEMORY {
            self.docs.insert(
                index_key,
                (
                    ts,
                    IterationDocument::Full {
                        value: doc,
                        prev_ts,
                    },
                ),
            );
            self.count_full += 1;
        } else {
            self.docs.insert(
                index_key,
                (ts, IterationDocument::Id(doc.id_with_table_id())),
            );
        }
    }

    fn remove(
        &mut self,
        index_key: &IndexKeyBytes,
    ) -> Option<(IndexKeyBytes, (Timestamp, IterationDocument))> {
        let removed = self.docs.remove_entry(index_key);
        if let Some((_, (_, IterationDocument::Full { .. }))) = &removed {
            self.count_full -= 1;
        }
        removed
    }
}

impl IntoIterator for IterationDocuments {
    type IntoIter =
        <BTreeMap<IndexKeyBytes, (Timestamp, IterationDocument)> as IntoIterator>::IntoIter;
    type Item = (IndexKeyBytes, (Timestamp, IterationDocument));

    fn into_iter(self) -> Self::IntoIter {
        self.docs.into_iter()
    }
}

impl Deref for IterationDocuments {
    type Target = BTreeMap<IndexKeyBytes, (Timestamp, IterationDocument)>;

    fn deref(&self) -> &Self::Target {
        &self.docs
    }
}

/// Read one page of `index_id` in `tablet_id` after `cursor`, advancing the
/// cursor, and return the page with the repeatable timestamp it was read at.
///
/// Each page is read at `max(min_ts, new_static_repeatable_recent())`, which
/// satisfies every constraint on a walk's page timestamps:
///
/// 1. each page is >= `min_ts`;
/// 2. successive pages are weakly monotonic, since `min_ts` never changes and
///    `new_static_repeatable_recent` only moves forward;
/// 3. each page is repeatable, as the max of repeatable timestamps;
/// 4. each page is within retention, since `new_static_repeatable_recent` is.
///
/// A `latest_ts_source` replaces `new_static_repeatable_recent` and must
/// satisfy the same properties (see [`LatestTsSource`]).
///
/// Reads that fall out of retention or time out are retried at a fresh
/// timestamp up to `TABLE_ITERATOR_MAX_RETRIES` times.
#[fastrace::trace]
async fn fetch_page(
    persistence: &Arc<dyn PersistenceReader>,
    retention_validator: &Arc<dyn RetentionValidator>,
    page_size: usize,
    min_ts: RepeatableTimestamp,
    latest_ts_source: Option<&LatestTsSource>,
    index_id: IndexRef,
    tablet_id: TabletId,
    cursor: &mut TableScanCursor,
) -> anyhow::Result<(Vec<(IndexKeyBytes, LatestDocument)>, RepeatableTimestamp)> {
    // An empty page never advances the cursor, so a zero page size would loop
    // forever.
    anyhow::ensure!(page_size > 0, "table iterator page size must be positive");
    for attempt in 0.. {
        let recent_ts = {
            let _timer = page_timer("ts");
            match latest_ts_source {
                Some(source) => source(),
                None => new_static_repeatable_recent(persistence.as_ref()).await?,
            }
        };
        let ts = cmp::max(min_ts, recent_ts);
        let repeatable_persistence =
            RepeatablePersistence::new(persistence.clone(), ts, retention_validator.clone());
        let reader = repeatable_persistence.read_snapshot(ts)?;
        let stream = reader.index_scan(
            index_id,
            tablet_id,
            &cursor.interval(),
            Order::Asc,
            page_size,
        );
        let scan_timer = page_timer("scan");
        let documents_in_page: Vec<_> = match stream.take(page_size).try_collect().await {
            Ok(docs) => docs,
            Err(e)
                if attempt < *TABLE_ITERATOR_MAX_RETRIES
                    && (e.is_out_of_retention() || e.is::<DatabaseTimeoutError>()) =>
            {
                tracing::warn!("TableIterator hit retriable error {e}, retrying...");
                continue;
            },
            Err(e) => return Err(e),
        };
        drop(scan_timer);
        if documents_in_page.len() < page_size {
            cursor.advance(CursorPosition::End)?;
        } else if let Some((index_key, ..)) = documents_in_page.last() {
            cursor.advance(CursorPosition::After(index_key.clone()))?;
        }
        return Ok((documents_in_page, ts));
    }
    unreachable!()
}
