use std::{
    cmp,
    collections::{
        BTreeMap,
        BTreeSet,
    },
    ops::{
        Bound,
        RangeBounds,
    },
    str::FromStr,
    sync::Arc,
};

use anyhow::Context;
use async_trait::async_trait;
use enum_iterator::Sequence;
use futures::{
    stream::BoxStream,
    try_join,
    StreamExt,
    TryStreamExt,
};
use serde_json::Value as JsonValue;
use value::{
    InternalDocumentId,
    TabletId,
};

use crate::{
    bootstrap_model::index::database_index::IndexedFields,
    document::ResolvedDocument,
    index::{
        IndexKey,
        IndexKeyBytes,
    },
    interval::Interval,
    knobs::DEFAULT_DOCUMENTS_PAGE_SIZE,
    persistence_helpers::RevisionPair,
    query::Order,
    runtime::Runtime,
    types::{
        DatabaseIndexUpdate,
        DatabaseIndexValue,
        GenericIndexName,
        IndexId,
        IndexRef,
        IndexWriteMode,
        PersistenceIndexId,
        PersistenceVersion,
        PrevIndexEntry,
        RepeatableReason,
        RepeatableTimestamp,
        Timestamp,
    },
};

pub mod row_index_retention;

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentLogEntry {
    pub ts: Timestamp,
    pub id: InternalDocumentId,
    pub value: Option<ResolvedDocument>,
    pub prev_ts: Option<Timestamp>,
}

impl DocumentLogEntry {
    pub fn size(&self) -> u64 {
        let mut size = self.ts.size() + self.id.size();
        if let Some(ref value) = self.value {
            size += value.size();
        }
        if let Some(prev_ts) = self.prev_ts {
            size += prev_ts.size();
        }
        size as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PersistenceIndexEntry {
    /// Whether index backfill may still be inserting entries for existing
    /// documents.
    pub mode: IndexWriteMode,
    pub ts: Timestamp,
    pub index: IndexRef,
    pub key: IndexKeyBytes,
    pub value: Option<InternalDocumentId>,
    /// `Some` when the key had an entry before this one: always for a
    /// tombstone, and for a live entry whose key the document's previous
    /// revision also produced.
    pub prev: Option<PrevIndexEntry>,
}

impl PersistenceIndexEntry {
    pub fn from_index_update(ts: Timestamp, update: &DatabaseIndexUpdate) -> Self {
        Self {
            ts,
            index: update.index,
            mode: update.mode,
            key: update.key.to_bytes(),
            value: match update.value {
                DatabaseIndexValue::Deleted => None,
                DatabaseIndexValue::NonClustered(id) => {
                    Some(InternalDocumentId::new(id.tablet_id, id.internal_id()))
                },
            },
            prev: update.prev,
        }
    }

    /// The bytes the entry adds, as the write throughput limiter and the write
    /// log account for them. The superseded entry is the persistence layer's
    /// bookkeeping, not data the commit adds, so it is not counted.
    pub fn size(&self) -> u64 {
        let mut size = self.ts.size() + self.index.id().size() + self.key.0.len();
        if let Some(value) = self.value {
            size += value.size();
        }
        size as u64
    }
}

/// An index entry written while building or replaying an index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexBackfillEntry {
    pub mode: IndexWriteMode,
    pub ts: Timestamp,
    pub index: IndexRef,
    pub key: IndexKeyBytes,
    pub value: Option<InternalDocumentId>,
}

impl TryFrom<PersistenceIndexEntry> for IndexBackfillEntry {
    type Error = anyhow::Error;

    fn try_from(entry: PersistenceIndexEntry) -> anyhow::Result<Self> {
        let value = entry.value.context("index backfill wrote a tombstone")?;
        anyhow::ensure!(
            entry.prev.is_none(),
            "index backfill wrote an entry superseding another"
        );
        Ok(Self {
            mode: entry.mode,
            ts: entry.ts,
            index: entry.index,
            key: entry.key,
            value: Some(value),
        })
    }
}

impl IndexBackfillEntry {
    pub fn from_replay(entry: &PersistenceIndexEntry) -> Self {
        Self {
            mode: entry.mode,
            ts: entry.ts,
            index: entry.index,
            key: entry.key.clone(),
            value: entry.value,
        }
    }
}

impl From<&IndexBackfillEntry> for PersistenceIndexEntry {
    fn from(entry: &IndexBackfillEntry) -> Self {
        Self {
            mode: entry.mode,
            ts: entry.ts,
            index: entry.index,
            key: entry.key.clone(),
            value: entry.value,
            prev: None,
        }
    }
}

pub type DocumentStream<'a> = BoxStream<'a, anyhow::Result<DocumentLogEntry>>;

pub type DocumentRevisionStream<'a> = BoxStream<'a, anyhow::Result<RevisionPair>>;

/// No tombstones included
pub type LatestDocumentStream<'a> = BoxStream<'a, anyhow::Result<LatestDocument>>;

pub type IndexStream<'a> = BoxStream<'a, anyhow::Result<(IndexKeyBytes, LatestDocument)>>;

/// A `DocumentLogEntry` that is not a tombstone.
#[derive(Debug, Clone, PartialEq)]
pub struct LatestDocument {
    pub ts: Timestamp,
    pub value: ResolvedDocument,
    pub prev_ts: Option<Timestamp>,
}

/// Indicates how write conflicts should be handled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictStrategy {
    /// If the record being written already exists with the same key, return an
    /// error and abort the write.
    Error,
    /// If the record being written already exists with the same key, overwrite
    /// the record.
    Overwrite,
}

// When adding a new persistence global, make sure it's copied
// or computed in migrate_db_cluster/text_index_worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Sequence)]
pub enum PersistenceGlobalKey {
    /// Minimum snapshot that is retained for indexes. Data in earlier snapshots
    /// may have been deleted.
    IndexRetentionMinSnapshotTimestamp,

    /// Timestamp for a snapshot that has been deleted by index retention.
    /// This is used as a cursor by retention, bumped after retention deletes
    /// entries at the snapshot.
    IndexRetentionConfirmedDeletedTimestamp,

    /// Minimum timestamp for valid write-ahead log
    DocumentRetentionMinSnapshotTimestamp,

    /// Timestamp for a document that has been deleted by retention.
    /// This is used as a cursor by document retention, bumped after retention
    /// deletes entries at a timestamp.
    DocumentRetentionConfirmedDeletedTimestamp,

    /// Maximum snapshot that is repeatable. All future commits will have
    /// timestamp > this timestamp.
    MaxRepeatableTimestamp,

    /// Latest snapshot of all tables' summaries, cached to speed up startup.
    TableSummary,

    /// Internal id of _tables.by_id index, for bootstrapping.
    TablesByIdIndex,
    /// Internal id of _tables table, for bootstrapping.
    TablesTabletId,
    /// Internal id of _index.by_id index, for bootstrapping.
    IndexByIdIndex,
    /// Internal id of _index table, for bootstrapping.
    IndexTabletId,
}

impl From<PersistenceGlobalKey> for String {
    fn from(key: PersistenceGlobalKey) -> Self {
        match key {
            PersistenceGlobalKey::IndexRetentionMinSnapshotTimestamp => {
                "min_snapshot_ts".to_string()
            },
            PersistenceGlobalKey::IndexRetentionConfirmedDeletedTimestamp => {
                "confirmed_deleted_ts".to_string()
            },
            PersistenceGlobalKey::DocumentRetentionMinSnapshotTimestamp => {
                "document_min_snapshot_ts".to_string()
            },
            PersistenceGlobalKey::DocumentRetentionConfirmedDeletedTimestamp => {
                "document_confirmed_deleted_ts".to_string()
            },
            PersistenceGlobalKey::MaxRepeatableTimestamp => "max_repeatable_ts".to_string(),
            PersistenceGlobalKey::TableSummary => "table_summary_v2".to_string(),
            PersistenceGlobalKey::TablesByIdIndex => "tables_by_id".to_string(),
            PersistenceGlobalKey::IndexByIdIndex => "index_by_id".to_string(),
            // NB: For compatibility, these are referred to as "table_id"s, not "tablet_id"s.
            PersistenceGlobalKey::TablesTabletId => "tables_table_id".to_string(),
            PersistenceGlobalKey::IndexTabletId => "index_table_id".to_string(),
        }
    }
}
impl FromStr for PersistenceGlobalKey {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "min_snapshot_ts" => Ok(Self::IndexRetentionMinSnapshotTimestamp),
            "confirmed_deleted_ts" => Ok(Self::IndexRetentionConfirmedDeletedTimestamp),
            "document_min_snapshot_ts" => Ok(Self::DocumentRetentionMinSnapshotTimestamp),
            "document_confirmed_deleted_ts" => Ok(Self::DocumentRetentionConfirmedDeletedTimestamp),
            "max_repeatable_ts" => Ok(Self::MaxRepeatableTimestamp),
            "table_summary_v2" => Ok(Self::TableSummary),
            "tables_by_id" => Ok(Self::TablesByIdIndex),
            "tables_table_id" => Ok(Self::TablesTabletId),
            "index_by_id" => Ok(Self::IndexByIdIndex),
            "index_table_id" => Ok(Self::IndexTabletId),
            _ => anyhow::bail!("unrecognized persistence global key"),
        }
    }
}

impl PersistenceGlobalKey {
    pub fn all_keys() -> Vec<Self> {
        enum_iterator::all().collect()
    }
}

/// What [`Persistence::reclaim_index_history`] is being asked to reclaim:
/// everything that stopped being visible between `cursor` and
/// `min_snapshot_ts`.
///
/// `all_indexes` and `retention_validator` are what it takes to re-derive the
/// expired entries from the document log, so a layout that reclaims whole
/// storage units ignores both.
pub struct IndexRetentionRequest<'a> {
    pub min_snapshot_ts: RepeatableTimestamp,
    pub cursor: RepeatableTimestamp,
    pub all_indexes: &'a BTreeMap<IndexId, (GenericIndexName<TabletId>, IndexedFields)>,
    pub retention_validator: Arc<dyn RetentionValidator>,
}

/// What a pass of [`Persistence::reclaim_index_history`] accomplished.
pub struct IndexRetentionProgress {
    /// How far the retention cursor advanced.
    pub cursor: RepeatableTimestamp,
    /// Index entries the pass found had expired. Not the same as the rows it
    /// removed: retention re-derives expiry from the document log, so a range
    /// it has already swept expires the same entries again.
    pub expired_entries: usize,
    /// Index rows the persistence reported removing. A layout that reclaims
    /// whole storage units rather than rows removes none and reports zero.
    pub deleted_rows: usize,
    /// Distinct indexes for which the pass re-derived expired entries -- a
    /// measure of how spread the retention work was across the index
    /// population, as opposed to concentrated in a few. A layout that
    /// reclaims whole storage units rather than rows reports zero.
    pub unique_indexes: usize,
}

#[async_trait]
pub trait Persistence: Sync + Send + 'static {
    /// Whether the persistence layer is freshely created or not.
    fn is_fresh(&self) -> bool;

    fn reader(&self) -> Arc<dyn PersistenceReader>;

    /// Writes documents and the respective derived indexes.
    async fn write<'a>(
        &self,
        documents: &'a [DocumentLogEntry],
        indexes: &'a [PersistenceIndexEntry],
        conflict_strategy: ConflictStrategy,
    ) -> anyhow::Result<()>;

    /// Durably invalidates shadow state before administrative writes that may
    /// bypass normal document/index write pairing, such as restores and
    /// migration copies.
    async fn invalidate_shadow(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Persists entries read from an index backfill snapshot at their document
    /// timestamps. Replaying a chunk must be idempotent.
    async fn write_index_backfill(&self, entries: &[IndexBackfillEntry]) -> anyhow::Result<()> {
        let entries: Vec<_> = entries.iter().map(PersistenceIndexEntry::from).collect();
        self.write(&[], &entries, ConflictStrategy::Overwrite).await
    }

    /// Reconciles backfill entries with live writes. The caller runs this
    /// after the last backfill chunk is durable and before writes switch to
    /// [`IndexWriteMode::ScanComplete`]. Implementations must be idempotent.
    async fn reconcile_index_backfill(&self, _index: PersistenceIndexId) -> anyhow::Result<()> {
        Ok(())
    }

    /// The indexes for which this persistence records keys that live writes
    /// deleted. Empty for a layout that keeps no delete markers.
    async fn index_backfill_marker_indexes(&self) -> anyhow::Result<Vec<PersistenceIndexId>> {
        Ok(Vec::new())
    }

    /// Returns the number of deletion markers removed. Each call deletes a
    /// bounded chunk; the caller must ensure no new markers can be written
    /// for the index.
    async fn delete_index_backfill_markers_chunk(
        &self,
        _index: PersistenceIndexId,
    ) -> anyhow::Result<u64> {
        Ok(0)
    }

    /// Writes global key-value data for the whole persistence.
    /// This is expected to be small data that does not make sense in a
    /// versioned or transaction context. See `PersistenceGlobalKey`.
    async fn write_persistence_global(
        &self,
        key: PersistenceGlobalKey,
        value: JsonValue,
    ) -> anyhow::Result<()>;

    /// Whether any index entries have been written yet.
    async fn has_index_entries(&self) -> anyhow::Result<bool>;

    /// Reclaims index history no longer visible at `request.min_snapshot_ts`,
    /// returning how far the cursor advanced and how many entries expired.
    /// How the space comes back is the persistence's business -- see
    /// [`row_index_retention::delete_expired_entries`] for the row-per-revision
    /// case.
    async fn reclaim_index_history(
        &self,
        request: IndexRetentionRequest<'_>,
    ) -> anyhow::Result<IndexRetentionProgress>;

    // Deletes documents
    async fn delete(
        &self,
        documents: Vec<(Timestamp, InternalDocumentId)>,
    ) -> anyhow::Result<usize>;

    async fn delete_tablet_documents(
        &self,
        tablet_id: TabletId,
        chunk_size: usize,
    ) -> anyhow::Result<usize>;

    // No-op by default. Persistence implementation can override.
    async fn shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn import_documents_batch(
        &self,
        mut documents: BoxStream<'_, Vec<DocumentLogEntry>>,
    ) -> anyhow::Result<()> {
        while let Some(chunk) = documents.next().await {
            self.write(&chunk, &[], ConflictStrategy::Error).await?;
        }
        Ok(())
    }

    async fn import_indexes_batch(
        &self,
        mut indexes: BoxStream<'_, Vec<PersistenceIndexEntry>>,
    ) -> anyhow::Result<()> {
        while let Some(chunk) = indexes.next().await {
            self.write(&[], &chunk, ConflictStrategy::Error).await?;
        }
        Ok(())
    }

    async fn finish_loading(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TimestampRange {
    start_inclusive: Timestamp,
    end_inclusive: Timestamp,
}

impl TimestampRange {
    #[inline]
    pub fn new<T: RangeBounds<Timestamp>>(range: T) -> Self {
        let start_inclusive = match range.start_bound() {
            Bound::Included(t) => *t,
            Bound::Excluded(t) => {
                if let Some(succ) = t.succ_opt() {
                    succ
                } else {
                    return Self::empty();
                }
            },
            Bound::Unbounded => Timestamp::MIN,
        };
        let end_inclusive = match range.end_bound() {
            Bound::Included(t) => *t,
            Bound::Excluded(t) => {
                if let Some(pred) = t.pred_opt() {
                    pred
                } else {
                    return Self::empty();
                }
            },
            Bound::Unbounded => Timestamp::MAX,
        };
        Self {
            start_inclusive,
            end_inclusive,
        }
    }

    #[inline]
    pub fn empty() -> Self {
        Self {
            start_inclusive: Timestamp::MAX,
            end_inclusive: Timestamp::MIN,
        }
    }

    #[inline]
    pub fn snapshot(ts: Timestamp) -> Self {
        Self::new(..=ts)
    }

    #[inline]
    pub fn all() -> Self {
        Self::new(..)
    }

    #[inline]
    pub fn at(ts: Timestamp) -> Self {
        Self::new(ts..=ts)
    }

    #[inline]
    pub fn greater_than(t: Timestamp) -> Self {
        Self::new((Bound::Excluded(t), Bound::Unbounded))
    }

    #[inline]
    pub fn min_timestamp_inclusive(&self) -> Timestamp {
        self.start_inclusive
    }

    #[inline]
    pub fn max_timestamp_exclusive(&self) -> Timestamp {
        // assumes that Timestamp::MAX never actually exists
        self.end_inclusive.succ_opt().unwrap_or(Timestamp::MAX)
    }

    #[inline]
    pub fn contains(&self, ts: Timestamp) -> bool {
        self.start_inclusive <= ts && ts <= self.end_inclusive
    }

    #[inline]
    pub fn intersect(&self, other: Self) -> Self {
        Self {
            start_inclusive: self.start_inclusive.max(other.start_inclusive),
            end_inclusive: self.end_inclusive.min(other.end_inclusive),
        }
    }
}

#[async_trait]
pub trait RetentionValidator: Sync + Send {
    /// Call optimistic_validate_snapshot *before* reading at the snapshot,
    /// to confirm all data in the snapshot may be within retention, so it's
    /// worth continuing.
    fn optimistic_validate_snapshot(&self, ts: Timestamp) -> anyhow::Result<()>;
    /// Call validate_snapshot *after* reading at the snapshot, to confirm all
    /// data in the snapshot is within retention.
    async fn validate_snapshot(&self, ts: Timestamp) -> anyhow::Result<()>;
    /// Call validate_document_snapshot *after* reading at the snapshot, to
    /// confirm the documents log is valid at this snapshot.
    async fn validate_document_snapshot(&self, ts: Timestamp) -> anyhow::Result<()>;
    async fn min_snapshot_ts(&self) -> anyhow::Result<RepeatableTimestamp>;
    async fn min_document_snapshot_ts(&self) -> anyhow::Result<RepeatableTimestamp>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentPrevTsQuery {
    pub id: InternalDocumentId,
    pub ts: Timestamp,
    pub prev_ts: Timestamp,
}

#[async_trait]
pub trait PersistenceReader: Send + Sync + 'static {
    /// The persistence is required to load documents within the given timestamp
    /// range.
    /// page_size is how many documents to fetch with a single query. It doesn't
    /// affect load_documents results, just efficiency of the internal queries.
    fn load_documents(
        &self,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentStream<'_>;

    /// Loads documents within the given table and the given timestamp range.
    ///
    /// page_size is how many documents to fetch with a single query. It doesn't
    /// affect load_documents results, just efficiency of the internal queries.
    fn load_documents_from_table(
        &self,
        tablet_id: TabletId,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentStream<'_>;

    /// Loads revision pairs from the document log in the given timestamp range.
    ///
    /// If a tablet id is provided, the results are filtered to a single table.
    fn load_revision_pairs(
        &self,
        tablet_id: Option<TabletId>,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentRevisionStream<'_> {
        let stream = if let Some(tablet_id) = tablet_id {
            self.load_documents_from_table(
                tablet_id,
                range,
                order,
                page_size,
                retention_validator.clone(),
            )
        } else {
            self.load_documents(range, order, page_size, retention_validator.clone())
        };
        crate::persistence_helpers::persistence_reader_stream_revision_pairs(
            stream,
            self,
            retention_validator,
        )
        .boxed()
    }

    /// Look up the previous revision of `(id, ts)`, returning a map where for
    /// each `(id, ts)` we have...
    ///
    /// 1. no value: there are no revisions of `id` before ts.
    /// 2. (prev_ts, None): the previous revision is a delete @ prev_ts.
    /// 3. (prev_ts, Some(document)): the previous revision @ prev_ts.
    async fn previous_revisions(
        &self,
        ids: BTreeSet<(InternalDocumentId, Timestamp)>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<(InternalDocumentId, Timestamp), DocumentLogEntry>>;

    /// Look up documents at exactly the specified prev_ts timestamps, returning
    /// a map where for each `DocumentPrevTsQuery` we have an entry only if
    /// a document exists at `(id, prev_ts)`.
    async fn previous_revisions_of_documents(
        &self,
        ids: BTreeSet<DocumentPrevTsQuery>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<DocumentPrevTsQuery, DocumentLogEntry>>;

    /// Loads documentIds with respective timestamps that match the
    /// index query criteria.
    /// `size_hint` is a best-effort estimate of the number of
    /// rows to be consumed from returned stream. This argument should only be
    /// used to tune batching in order to balance round trips and redundant
    /// queries. The returned stream should always yield the same results if
    /// fully consumed regardless of the estimate.
    fn index_scan(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        read_timestamp: Timestamp,
        range: &Interval,
        order: Order,
        size_hint: usize,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> IndexStream<'_>;

    async fn get_persistence_global(
        &self,
        key: PersistenceGlobalKey,
    ) -> anyhow::Result<Option<JsonValue>>;

    /// Performs a single point get using an index.
    async fn index_get(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        read_timestamp: Timestamp,
        key: IndexKey,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<Option<LatestDocument>> {
        let mut stream = self.index_scan(
            index,
            tablet_id,
            read_timestamp,
            &Interval::prefix(key.to_bytes().into()),
            Order::Asc,
            2,
            retention_validator,
        );
        match stream.try_next().await? {
            Some((key, rev)) => {
                anyhow::ensure!(
                    stream.try_next().await?.is_none(),
                    "Got multiple values for key {:?}",
                    key
                );
                Ok(Some(rev))
            },
            None => Ok(None),
        }
    }

    /// max_ts is the largest timestamp written to persistence.
    /// It's not necessarily safe to read snapshots at this timestamp.
    /// Use a RepeatableTimestamp constructor to find a safe timestamp for
    /// reads. It is safe to read at max_ts iff there are no ongoing
    /// commits, e.g. when a database is loading and has acquired the lease
    /// but not begun commits.
    async fn max_ts(&self) -> anyhow::Result<Option<Timestamp>> {
        // Fetch the document with the maximum timestamp and also MaxRepeatableTimestamp
        // in parallel.
        let mut stream = self.load_documents(
            TimestampRange::all(),
            Order::Desc,
            1,
            // We don't know the ID of the most recent document, so we
            // need to scan the entire timestamp range to find it
            // (this may include looking at the `documents` log outside of the retention window)
            Arc::new(NoopRetentionValidator) as Arc<_>,
        );
        let max_repeatable =
            self.get_persistence_global(PersistenceGlobalKey::MaxRepeatableTimestamp);
        let (max_committed, max_repeatable) = try_join!(stream.try_next(), max_repeatable)?;
        let max_committed_ts = max_committed.map(|entry| entry.ts);
        let max_repeatable_ts = max_repeatable.map(Timestamp::try_from).transpose()?;
        let max_ts = cmp::max(max_committed_ts, max_repeatable_ts); // note None < Some
        Ok(max_ts)
    }

    fn version(&self) -> PersistenceVersion;

    /// Whether the database must run the index retention worker. Backends with
    /// independent index history maintenance return `false`, and their index
    /// retention persistence globals are not maintained.
    fn needs_index_retention_deletes(&self) -> bool {
        true
    }

    async fn table_size_stats(&self) -> anyhow::Result<Vec<PersistenceTableSize>> {
        Ok(vec![])
    }

}

/// Timestamp that is repeatable because the caller is holding the lease and
/// no one is writing to persistence. In particular the Committer is not
/// running. So all future commits will be after the returned
/// IdleRepeatableTimestamp (even when commits write in parallel). e.g. this can
/// be used on database load.
pub async fn new_idle_repeatable_ts<RT: Runtime>(
    persistence: &dyn Persistence,
    rt: &RT,
) -> anyhow::Result<RepeatableTimestamp> {
    let reader = persistence.reader();
    let max_ts = reader.max_ts().await?.unwrap_or(Timestamp::MIN);
    let now = cmp::max(max_ts, rt.generate_timestamp()?);
    // Enforce that all subsequent commits are > now by writing to MaxRepeatableTs.
    persistence
        .write_persistence_global(PersistenceGlobalKey::MaxRepeatableTimestamp, now.into())
        .await?;
    Ok(RepeatableTimestamp::new_validated(
        now,
        RepeatableReason::IdleMaxTs,
    ))
}

/// RepeatablePersistence can read from Persistence at a range of snapshots
/// <= the given snapshot. Therefore reads from RepeatablePersistence
/// will not see new writes, i.e. all reads will see the same data.
#[derive(Clone)]
pub struct RepeatablePersistence {
    reader: Arc<dyn PersistenceReader>,
    upper_bound: RepeatableTimestamp,
    retention_validator: Arc<dyn RetentionValidator>,
}

impl RepeatablePersistence {
    pub fn new(
        reader: Arc<dyn PersistenceReader>,
        upper_bound: RepeatableTimestamp,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> Self {
        Self {
            reader,
            upper_bound,
            retention_validator,
        }
    }

    pub fn upper_bound(&self) -> RepeatableTimestamp {
        self.upper_bound
    }

    /// Same as [`PersistenceReader::load_documents`] but only including
    /// documents in the snapshot range.
    pub fn load_documents(&self, range: TimestampRange, order: Order) -> DocumentStream<'_> {
        self.reader.load_documents(
            range.intersect(TimestampRange::snapshot(*self.upper_bound)),
            order,
            *DEFAULT_DOCUMENTS_PAGE_SIZE,
            self.retention_validator.clone(),
        )
    }

    /// Same as [`PersistenceReader::load_documents_from_table`] but only
    /// including documents in the snapshot range.
    pub fn load_documents_from_table(
        &self,
        tablet_id: TabletId,
        range: TimestampRange,
        order: Order,
    ) -> DocumentStream<'_> {
        self.reader.load_documents_from_table(
            tablet_id,
            range.intersect(TimestampRange::snapshot(*self.upper_bound)),
            order,
            *DEFAULT_DOCUMENTS_PAGE_SIZE,
            self.retention_validator.clone(),
        )
    }

    /// Same as [`PersistenceReader::load_revision_pairs`] but only including
    /// revisions in the snapshot range.
    pub fn load_revision_pairs(
        &self,
        tablet_id: Option<TabletId>,
        range: TimestampRange,
        order: Order,
    ) -> DocumentRevisionStream<'_> {
        self.reader.load_revision_pairs(
            tablet_id,
            range.intersect(TimestampRange::snapshot(*self.upper_bound)),
            order,
            *DEFAULT_DOCUMENTS_PAGE_SIZE,
            self.retention_validator.clone(),
        )
    }

    pub async fn previous_revisions(
        &self,
        ids: BTreeSet<(InternalDocumentId, Timestamp)>,
    ) -> anyhow::Result<BTreeMap<(InternalDocumentId, Timestamp), DocumentLogEntry>> {
        for (_, ts) in &ids {
            // Reading documents <ts, so ts-1 needs to be repeatable.
            anyhow::ensure!(*ts <= self.upper_bound.succ()?);
        }
        self.reader
            .previous_revisions(ids, self.retention_validator.clone())
            .await
    }

    pub async fn previous_revisions_of_documents(
        &self,
        ids: BTreeSet<DocumentPrevTsQuery>,
    ) -> anyhow::Result<BTreeMap<DocumentPrevTsQuery, DocumentLogEntry>> {
        for DocumentPrevTsQuery { prev_ts, .. } in &ids {
            // Reading documents with timestamp prev_ts, so it needs to be repeatable.
            anyhow::ensure!(*prev_ts <= self.upper_bound);
        }
        self.reader
            .previous_revisions_of_documents(ids, self.retention_validator.clone())
            .await
    }

    pub fn read_snapshot(&self, at: RepeatableTimestamp) -> anyhow::Result<PersistenceSnapshot> {
        anyhow::ensure!(at <= self.upper_bound);
        self.retention_validator.optimistic_validate_snapshot(*at)?;
        Ok(PersistenceSnapshot {
            reader: self.reader.clone(),
            at,
            retention_validator: self.retention_validator.clone(),
        })
    }

    pub fn version(&self) -> PersistenceVersion {
        self.reader.version()
    }
}

/// This timestamp is determined to be repeatable by reading max_repeatable_ts
/// from persistence. It may be lagging a few minutes behind live writes.
/// It is expected only to be called from background tasks that don't need to
/// read recent writes.
pub async fn new_static_repeatable_recent(
    reader: &dyn PersistenceReader,
) -> anyhow::Result<RepeatableTimestamp> {
    match reader
        .get_persistence_global(PersistenceGlobalKey::MaxRepeatableTimestamp)
        .await?
    {
        None => Ok(RepeatableTimestamp::MIN),
        Some(value) => Ok(RepeatableTimestamp::new_validated(
            Timestamp::try_from(value)?,
            RepeatableReason::MaxRepeatableTsPersistence,
        )),
    }
}

/// PersistenceSnapshot can perform reads from Persistence at a given
/// snapshot.
#[derive(Clone)]
pub struct PersistenceSnapshot {
    reader: Arc<dyn PersistenceReader>,
    at: RepeatableTimestamp,
    retention_validator: Arc<dyn RetentionValidator>,
}

impl PersistenceSnapshot {
    /// Same as [`Persistence::index_scan`] but with fixed timestamp.
    pub fn index_scan(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        interval: &Interval,
        order: Order,
        size_hint: usize,
    ) -> IndexStream<'_> {
        self.reader.index_scan(
            index,
            tablet_id,
            *self.at,
            interval,
            order,
            size_hint,
            self.retention_validator.clone(),
        )
    }

    /// Same as [`Persistence::index_get`] but with fixed timestamp.
    pub async fn index_get(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        key: IndexKey,
    ) -> anyhow::Result<Option<LatestDocument>> {
        let result = self
            .reader
            .index_get(
                index,
                tablet_id,
                *self.at,
                key,
                self.retention_validator.clone(),
            )
            .await?;
        Ok(result)
    }

    pub fn timestamp(&self) -> RepeatableTimestamp {
        self.at
    }

    pub fn persistence(&self) -> &dyn PersistenceReader {
        self.reader.as_ref()
    }

    pub fn retention_validator(&self) -> Arc<dyn RetentionValidator> {
        self.retention_validator.clone()
    }
}

/// Test-only snapshot validator that doesn't validate anything.
/// Prod and most tests should use (Follower|Leader)RetentionManager,
#[derive(Clone, Copy)]
pub struct NoopRetentionValidator;

#[async_trait]
impl RetentionValidator for NoopRetentionValidator {
    fn optimistic_validate_snapshot(&self, _ts: Timestamp) -> anyhow::Result<()> {
        Ok(())
    }

    async fn validate_snapshot(&self, _ts: Timestamp) -> anyhow::Result<()> {
        Ok(())
    }

    async fn validate_document_snapshot(&self, _ts: Timestamp) -> anyhow::Result<()> {
        Ok(())
    }

    async fn min_snapshot_ts(&self) -> anyhow::Result<RepeatableTimestamp> {
        Ok(RepeatableTimestamp::MIN)
    }

    async fn min_document_snapshot_ts(&self) -> anyhow::Result<RepeatableTimestamp> {
        Ok(RepeatableTimestamp::MIN)
    }
}

#[derive(Debug, Clone)]
pub struct PersistenceTableSize {
    /// The name of the underlying persistence table
    pub table_name: String,
    pub data_bytes: u64,
    pub index_bytes: u64,
    pub row_count: Option<u64>,
}
