use std::{
    borrow::Borrow,
    cmp::{
        Ordering,
        Reverse,
    },
    collections::{
        btree_map::Entry,
        BTreeMap,
        BinaryHeap,
        VecDeque,
    },
    iter,
    mem,
    ops::{
        Bound,
        Deref,
    },
    sync::Arc,
};

use common::{
    components::CanonicalizedComponentFunctionPath,
    document::{
        DocumentUpdateWithPrevTs,
        PackedDocument,
    },
    document_index_keys::{
        DatabaseIndexWrite,
        IndexKeyUpdate,
        TextIndexWrite,
        Update,
    },
    erased_slot::ErasedSlot,
    interval::IntervalSet,
    knobs::{
        WRITE_LOG_MAX_RETENTION_SECS,
        WRITE_LOG_MIN_RETENTION_SECS,
        WRITE_LOG_SOFT_MAX_SIZE_BYTES,
    },
    runtime::block_in_place,
    types::{
        RepeatableTimestamp,
        SubscriberId,
        TabletIndexName,
        Timestamp,
        UdfIdentifier,
    },
    value::ResolvedDocumentId,
};
use errors::ErrorMetadata;
use futures::Future;
use imbl::{
    OrdMap,
    OrdSet,
};
use indexing::{
    database_index_snapshot::TimestampedIndexCache,
    index_cache::WriteLogIndexReader,
    index_registry::IndexRegistry,
};
use parking_lot::Mutex;
use search::query::tokenize;
use tokio::sync::oneshot;
use value::{
    heap_size::{
        ElementsHeapSize,
        HeapSize,
    },
    TabletId,
};

use crate::{
    database::{
        ConflictingRead,
        ConflictingReadWithWriteSource,
    },
    metrics,
    reads::ReadSet,
    Snapshot,
    Token,
};

/// The packed writes of a single commit, in `table_dependency_sort_key` order.
/// Shared because both `PendingWrites` and the off-thread persistence write
/// need them, and neither mutates them.
pub type OrderedDocumentWrites = Arc<[PackedDocumentUpdate]>;

#[derive(Clone)]
pub struct PackedDocumentUpdate {
    pub id: ResolvedDocumentId,
    /// The old document with the timestamp of its revision.
    pub old_document: Option<(PackedDocument, Timestamp)>,
    pub new_document: Option<PackedDocument>,
}

impl HeapSize for PackedDocumentUpdate {
    fn heap_size(&self) -> usize {
        self.old_document
            .as_ref()
            .map_or(0, |(document, _)| document.heap_size())
            + self.new_document.heap_size()
    }
}

impl PackedDocumentUpdate {
    pub fn pack(update: &DocumentUpdateWithPrevTs) -> Self {
        Self {
            id: update.id,
            old_document: update
                .old_document
                .as_ref()
                .map(|(document, ts)| (PackedDocument::pack(document), *ts)),
            new_document: update.new_document.as_ref().map(PackedDocument::pack),
        }
    }

    pub fn unpack(&self) -> DocumentUpdateWithPrevTs {
        DocumentUpdateWithPrevTs {
            id: self.id,
            old_document: self
                .old_document
                .as_ref()
                .map(|(document, ts)| (document.unpack(), *ts)),
            new_document: self.new_document.as_ref().map(|doc| doc.unpack()),
        }
    }
}
/// Indicates whether an index entry in the write log belongs to the
/// `by_database_index` or `by_text_index` map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IndexKind {
    Database,
    Text,
}

/// The per-commit index-key writes, split by index kind so each Vec holds a
/// homogeneous update type.
#[derive(Default)]
pub struct IndexKeyWrites {
    pub database: Vec<(TabletIndexName, Arc<WriteInIndex<DatabaseIndexWrite>>)>,
    pub text: Vec<(TabletIndexName, Arc<WriteInIndex<TextIndexWrite>>)>,
}

impl IndexKeyWrites {
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Converts [PackedDocumentUpdate]s (the log used in `PendingWrites` that
/// contains full documents) to [IndexKeyWrites] (the log used in `WriteLog`
/// that contains index keys too).
pub fn index_keys_from_full_documents(
    ts: Timestamp,
    ordered_writes: &[PackedDocumentUpdate],
    write_source: &WriteSource,
    index_registry: &IndexRegistry,
) -> IndexKeyWrites {
    let _timer = metrics::pending_writes_to_write_log_timer();
    let mut database: BTreeMap<TabletIndexName, Vec<DatabaseIndexWrite>> = BTreeMap::new();
    let mut text: BTreeMap<TabletIndexName, Vec<TextIndexWrite>> = BTreeMap::new();
    for update in ordered_writes {
        for (index_name, index_update) in index_registry
            .document_index_keys(
                update.id,
                update.old_document.as_ref().map(|(document, _)| document),
                update.new_document.as_ref(),
                tokenize,
            )
            .0
            .into_iter()
        {
            match index_update.update {
                IndexKeyUpdate::Database(u) => {
                    database
                        .entry(index_name)
                        .or_default()
                        .push(DatabaseIndexWrite {
                            document_id: index_update.document_id,
                            update: u,
                            new_document: index_update.new_document,
                        });
                },
                IndexKeyUpdate::Text(u) => {
                    text.entry(index_name).or_default().push(TextIndexWrite {
                        document_id: index_update.document_id,
                        update: u,
                    });
                },
            }
        }
    }

    IndexKeyWrites {
        database: make_writes(ts, write_source, database),
        text: make_writes(ts, write_source, text),
    }
}

fn make_writes<K, T>(
    ts: Timestamp,
    write_source: &WriteSource,
    writes: BTreeMap<K, Vec<T>>,
) -> Vec<(K, Arc<WriteInIndex<T>>)> {
    writes
        .into_iter()
        .map(|(index, index_updates)| {
            (
                index,
                Arc::new(WriteInIndex {
                    ts,
                    index_updates,
                    write_source: write_source.clone(),
                }),
            )
        })
        .collect()
}

#[derive(Clone, PartialEq, Eq)]
pub enum WriteSource {
    /// A user-defined function (mutation) that performed the write.
    Udf(Arc<UdfIdentifier>),
    /// A system UDF (e.g. _system/ mutations) that performed the write.
    /// Separated from `Udf` so callers can choose whether to expose it.
    SystemUdf(Arc<UdfIdentifier>),
    /// An internal system operation (e.g. "system_table_cleanup").
    System(&'static str),
}

impl std::fmt::Debug for WriteSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Udf(id) => write!(f, "Udf({id})"),
            Self::SystemUdf(id) => write!(f, "SystemUdf({id})"),
            Self::System(s) => write!(f, "System({s:?})"),
        }
    }
}

impl WriteSource {
    /// Create a system write source from a static label.
    pub fn system(label: &'static str) -> Self {
        Self::System(label)
    }

    /// Create the write source for a mutation on `path`. `_system/` functions
    /// get `SystemUdf` so that consumers filtering on `is_udf` don't surface
    /// them as developer functions.
    pub fn mutation(path: CanonicalizedComponentFunctionPath) -> Self {
        let is_system = path.is_system();
        let identifier = Arc::new(UdfIdentifier::Function(path));
        if is_system {
            Self::SystemUdf(identifier)
        } else {
            Self::Udf(identifier)
        }
    }

    /// Returns a display string for this write source, including the
    /// component path for UDF sources.
    pub fn display_name(&self) -> Option<String> {
        match self {
            Self::Udf(identifier) | Self::SystemUdf(identifier) => {
                let (component, id) = (**identifier).clone().into_component_and_udf_path();
                Some(match component {
                    Some(component) => format!("{component}/{id}"),
                    None => id,
                })
            },
            Self::System(s) => Some(s.to_string()),
        }
    }

    /// Returns true if this is a user UDF write source.
    pub fn is_udf(&self) -> bool {
        matches!(self, Self::Udf(_))
    }

    /// Returns the UDF identifier if this is a user function write source.
    pub fn udf_identifier(&self) -> Option<&UdfIdentifier> {
        match self {
            Self::Udf(id) => Some(id),
            Self::SystemUdf(_) => None,
            Self::System(_) => None,
        }
    }
}

impl From<&'static str> for WriteSource {
    fn from(value: &'static str) -> Self {
        Self::System(value)
    }
}

impl HeapSize for WriteSource {
    fn heap_size(&self) -> usize {
        match self {
            Self::Udf(_) | Self::SystemUdf(_) => std::mem::size_of::<Arc<UdfIdentifier>>(),
            Self::System(_) => 0,
        }
    }
}

struct WriteLogManager {
    log: WriteLog,
    size: usize,
    /// Keeps track of the minimum timestamps in each index's log, used for fast
    /// purging. Each entry records which map (`IndexKind`) the index belongs to
    /// so we can remove from the right map.
    min_ts_to_index: BinaryHeap<Reverse<(Timestamp, TabletIndexName, IndexKind)>>,
    waiters: VecDeque<(Timestamp, oneshot::Sender<()>)>,
}

impl WriteLogManager {
    fn new(initial_timestamp: Timestamp) -> Self {
        let log = WriteLog::new(initial_timestamp);
        let waiters = VecDeque::new();
        Self {
            log,
            size: 0,
            min_ts_to_index: BinaryHeap::new(),
            waiters,
        }
    }

    fn notify_waiters(&mut self) {
        let ts = self.log.max_ts();
        // Notify waiters
        let mut i = 0;
        while i < self.waiters.len() {
            if ts > self.waiters[i].0 || self.waiters[i].1.is_closed() {
                // Remove from the waiters.
                let w = self.waiters.swap_remove_back(i).expect("checked above");
                // Notify. Ignore if receiver has dropped.
                let _ = w.1.send(());
                // Continue without increasing i, since we just swapped the
                // element and that position and need to check it too.
                continue;
            }
            i += 1;
        }
    }

    fn append(&mut self, ts: Timestamp, writes: IndexKeyWrites) {
        assert!(self.log.max_ts() < ts, "{:?} >= {}", self.log.max_ts(), ts);

        for (index, updates) in writes.database {
            assert_eq!(updates.ts, ts);
            self.log.by_database_index.append(
                index,
                ArcWriteInIndex(updates),
                IndexKind::Database,
                &mut self.size,
                &mut self.min_ts_to_index,
            );
        }
        for (index, updates) in writes.text {
            assert_eq!(updates.ts, ts);
            self.log.by_text_index.append(
                index,
                ArcWriteInIndex(updates),
                IndexKind::Text,
                &mut self.size,
                &mut self.min_ts_to_index,
            );
        }
        self.log.max_ts = ts;

        self.notify_waiters();
    }

    /// Returns a future that blocks until the log has advanced past the given
    /// timestamp.
    fn wait_for_higher_ts(&mut self, target_ts: Timestamp) -> impl Future<Output = ()> + use<> {
        // Clean up waiters that are canceled.
        self.notify_waiters();

        let receiver = if self.log.max_ts() <= target_ts {
            let (sender, receiver) = oneshot::channel();
            self.waiters.push_back((target_ts, sender));
            Some(receiver)
        } else {
            None
        };

        async move {
            if let Some(receiver) = receiver {
                _ = receiver.await;
            }
        }
    }

    fn enforce_retention_policy(&mut self, current_ts: Timestamp) {
        let hard_limit_ts = current_ts
            .sub(*WRITE_LOG_MIN_RETENTION_SECS)
            .unwrap_or(Timestamp::MIN);
        let soft_limit_ts = current_ts
            .sub(*WRITE_LOG_MAX_RETENTION_SECS)
            .unwrap_or(Timestamp::MIN);
        loop {
            let limit_ts = if self.size >= *WRITE_LOG_SOFT_MAX_SIZE_BYTES {
                hard_limit_ts
            } else {
                soft_limit_ts
            };
            let Some(Reverse((ts, index, kind))) = self
                .min_ts_to_index
                .pop_if(|&Reverse((ts, ..))| ts == self.log.purged_ts || ts < limit_ts)
            else {
                break;
            };

            self.log.purged_ts = ts;

            match kind {
                IndexKind::Database => {
                    self.log.by_database_index.remove_at_ts(
                        index,
                        ts,
                        IndexKind::Database,
                        &mut self.size,
                        &mut self.min_ts_to_index,
                    );
                },
                IndexKind::Text => {
                    self.log.by_text_index.remove_at_ts(
                        index,
                        ts,
                        IndexKind::Text,
                        &mut self.size,
                        &mut self.min_ts_to_index,
                    );
                },
            }
        }
    }
}

/// All of the updates at `ts` within a single index.
pub struct WriteInIndex<T> {
    pub ts: Timestamp,
    pub index_updates: Vec<T>,
    pub write_source: WriteSource,
}
/// A `WriteInIndex` that can be stored in an OrdSet.
/// Note that Eq/Ord compare timestamp only.
pub(crate) struct ArcWriteInIndex<T>(Arc<WriteInIndex<T>>);

impl<T> Clone for ArcWriteInIndex<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Deref for ArcWriteInIndex<T> {
    type Target = WriteInIndex<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T: HeapSize> ArcWriteInIndex<T> {
    fn heap_size(&self) -> usize {
        mem::size_of_val(&*self.0)
            + self.0.index_updates.capacity() * mem::size_of::<T>()
            + self.0.index_updates.elements_heap_size()
            + self.0.write_source.heap_size()
    }
}

impl<T> PartialEq for ArcWriteInIndex<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.ts == other.0.ts
    }
}
impl<T> Eq for ArcWriteInIndex<T> {}
impl<T> PartialOrd for ArcWriteInIndex<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for ArcWriteInIndex<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.ts.cmp(&other.0.ts)
    }
}
impl<T> Borrow<Timestamp> for ArcWriteInIndex<T> {
    fn borrow(&self) -> &Timestamp {
        &self.0.ts
    }
}

/// A typed map from index name to timestamped update vectors.
/// Shared structure for both database and search index maps in the write log.
struct WritesByIndex<T>(OrdMap<TabletIndexName, OrdSet<ArcWriteInIndex<T>>>);

impl<T> Clone for WritesByIndex<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: HeapSize> WritesByIndex<T> {
    fn new() -> Self {
        Self(OrdMap::new())
    }

    fn append(
        &mut self,
        index: TabletIndexName,
        update: ArcWriteInIndex<T>,
        kind: IndexKind,
        by_index_size: &mut usize,
        min_ts_to_index: &mut BinaryHeap<Reverse<(Timestamp, TabletIndexName, IndexKind)>>,
    ) {
        *by_index_size += update.heap_size();
        if let Some(e) = self.0.get_mut(&index) {
            e.insert(update);
        } else {
            let ts = update.ts;
            self.0.insert(index.clone(), OrdSet::unit(update));
            min_ts_to_index.push(Reverse((ts, index, kind)));
        }
    }

    /// Remove the entry at `ts` for `index`. If the index has remaining
    /// entries, re-register its new minimum timestamp.
    fn remove_at_ts(
        &mut self,
        index: TabletIndexName,
        ts: Timestamp,
        kind: IndexKind,
        by_index_size: &mut usize,
        min_ts_to_index: &mut BinaryHeap<Reverse<(Timestamp, TabletIndexName, IndexKind)>>,
    ) {
        let Some(inner) = self.0.get_mut(&index) else {
            return;
        };
        if let Some(update) = inner.remove(&ts) {
            *by_index_size = by_index_size.saturating_sub(update.heap_size());
        }
        if let Some(update) = inner.get_min() {
            let new_min_ts = update.0.ts;
            min_ts_to_index.push(Reverse((new_min_ts, index, kind)));
        } else {
            self.0.remove(&index);
        }
    }

    fn iter(&self) -> impl Iterator<Item = (&TabletIndexName, &OrdSet<ArcWriteInIndex<T>>)> {
        self.0.iter()
    }
}

/// WriteLog holds recent commits that have been written to persistence and
/// snapshot manager. These commits may cause OCC aborts for new commits, and
/// they may trigger subscriptions.
#[derive(Clone)]
struct WriteLog {
    by_database_index: WritesByIndex<DatabaseIndexWrite>,
    by_text_index: WritesByIndex<TextIndexWrite>,
    max_ts: Timestamp,
    purged_ts: Timestamp,
}

pub struct OutOfRetentionError {
    reads_ts: Timestamp,
    purged_ts: Timestamp,
}

impl From<OutOfRetentionError> for anyhow::Error {
    #[track_caller]
    fn from(e: OutOfRetentionError) -> Self {
        anyhow::anyhow!(
            "Timestamp {reads_ts} is outside of write log retention window (minimum timestamp \
             {purged_ts})",
            reads_ts = e.reads_ts,
            purged_ts = e.purged_ts
        )
        .context(ErrorMetadata::out_of_retention())
    }
}

impl WriteLog {
    fn new(initial_timestamp: Timestamp) -> Self {
        Self {
            by_database_index: WritesByIndex::new(),
            by_text_index: WritesByIndex::new(),
            max_ts: initial_timestamp,
            purged_ts: initial_timestamp,
        }
    }

    fn max_ts(&self) -> Timestamp {
        self.max_ts
    }

    fn is_stale(
        &self,
        reads: &ReadSet,
        reads_ts: Timestamp,
        ts: Timestamp,
    ) -> Result<Option<ConflictingReadWithWriteSource>, OutOfRetentionError> {
        if reads_ts < self.purged_ts {
            Err(OutOfRetentionError {
                reads_ts,
                purged_ts: self.purged_ts,
            })
        } else {
            Ok(reads.writes_overlap_by_index(
                &self.by_database_index.0,
                &self.by_text_index.0,
                reads_ts,
                ts,
            ))
        }
    }

    /// Returns Err(write_ts) if the token could not be refreshed, where
    /// write_ts is the timestamp of a conflicting write (if known)
    fn refresh_token(&self, mut token: Token, ts: Timestamp) -> Result<Token, Option<Timestamp>> {
        metrics::log_read_set_age(ts.secs_since_f64(token.ts()).max(0.0));
        match self.is_stale(token.reads(), token.ts(), ts) {
            Ok(Some(conflict)) => Err(Some(conflict.write_ts)),
            Err(OutOfRetentionError { .. }) => {
                metrics::log_reads_refresh_miss();
                Err(None)
            },
            Ok(None) => {
                if token.ts() < ts {
                    token.advance_ts(ts);
                }
                Ok(token)
            },
        }
    }
}

pub fn new_write_log(initial_timestamp: Timestamp) -> (LogOwner, LogReader, LogWriter) {
    let log_manager = Arc::new(Mutex::new(WriteLogManager::new(initial_timestamp)));
    (
        LogOwner {
            inner: log_manager.clone(),
        },
        LogReader {
            inner: log_manager.clone(),
        },
        LogWriter { inner: log_manager },
    )
}

/// LogOwner consumes the log and is responsible for trimming it.
pub struct LogOwner {
    inner: Arc<Mutex<WriteLogManager>>,
}

impl LogOwner {
    pub fn enforce_retention_policy(&mut self, current_ts: Timestamp) {
        self.inner.lock().enforce_retention_policy(current_ts)
    }

    pub fn reader(&self) -> LogReader {
        LogReader {
            inner: self.inner.clone(),
        }
    }
}

#[derive(Clone)]
pub struct LogReader {
    inner: Arc<Mutex<WriteLogManager>>,
}

impl LogReader {
    #[fastrace::trace]
    pub fn refresh_token(
        &self,
        token: Token,
        ts: Timestamp,
    ) -> anyhow::Result<Result<Token, Option<Timestamp>>> {
        if token.ts() == ts {
            // Nothing to do. We can return Ok even if `token.ts()` has fallen
            // out of the write log retention window.
            return Ok(Ok(token));
        }
        let snapshot = { self.inner.lock().log.clone() };
        let max_ts = snapshot.max_ts();
        anyhow::ensure!(
            ts <= max_ts,
            "Can't refresh token to newer timestamp {ts} than max ts {max_ts}"
        );
        Ok(snapshot.refresh_token(token, ts))
    }

    pub fn refresh_reads_until_max_ts(&self, token: Token) -> Result<Token, Option<Timestamp>> {
        let snapshot = { self.inner.lock().log.clone() };
        block_in_place(|| {
            let max_ts = snapshot.max_ts();
            snapshot.refresh_token(token, max_ts)
        })
    }

    pub fn max_ts(&self) -> Timestamp {
        self.inner.lock().log.max_ts()
    }

    /// Blocks until the log has advanced past the given timestamp.
    pub async fn wait_for_higher_ts(&self, target_ts: Timestamp) -> Timestamp {
        let fut = self.inner.lock().wait_for_higher_ts(target_ts);
        fut.await;
        let result = self.inner.lock().log.max_ts();
        assert!(result > target_ts);
        result
    }

    /// Iterates over all index write log entries in the range [from, to]
    /// (inclusive), calling `f` for each database (index_name, updates) pair
    /// and `g` for each text index (index_name, updates) pair.
    ///
    /// Entries are yielded per-index (not per-document or per-commit). The same
    /// commit may produce entries across multiple index vectors.
    pub fn for_each_index<F, G>(
        &self,
        from: Timestamp,
        to: Timestamp,
        to_notify: &mut BTreeMap<SubscriberId, (Timestamp, Option<WriteSource>, TabletId)>,
        num_index_updates: &mut usize,
        mut f: F,
        mut g: G,
    ) -> anyhow::Result<()>
    where
        F: for<'a> FnMut(
            &'a TabletIndexName,
            Box<dyn Iterator<Item = &'a WriteInIndex<DatabaseIndexWrite>> + 'a>,
            &'a mut BTreeMap<SubscriberId, (Timestamp, Option<WriteSource>, TabletId)>,
            &'a mut usize,
        ),
        G: for<'a> FnMut(
            &'a TabletIndexName,
            Box<dyn Iterator<Item = &'a WriteInIndex<TextIndexWrite>> + 'a>,
            &'a mut BTreeMap<SubscriberId, (Timestamp, Option<WriteSource>, TabletId)>,
            &'a mut usize,
        ),
    {
        let snapshot = { self.inner.lock().log.clone() };
        block_in_place(|| {
            anyhow::ensure!(
                from > snapshot.purged_ts,
                anyhow::anyhow!(
                    "Timestamp {from} is outside of write log retention window (minimum timestamp \
                     {})",
                    snapshot.purged_ts
                )
                .context(ErrorMetadata::out_of_retention())
            );
            for (index_name, updates) in snapshot.by_database_index.iter() {
                f(
                    index_name,
                    Box::new(updates.range(from..=to).map(|u| &*u.0)),
                    to_notify,
                    num_index_updates,
                );
            }
            for (index_name, updates) in snapshot.by_text_index.iter() {
                g(
                    index_name,
                    Box::new(updates.range(from..=to).map(|u| &*u.0)),
                    to_notify,
                    num_index_updates,
                );
            }
            Ok(())
        })
    }

    /// Walks the write log and updates the index cache with documents in
    /// RefreshableTablets with index updates the cache is already tracking.
    ///
    /// Returns None if the begin_ts is out of the retention window.
    pub async fn fast_forward_index_cache(
        &self,
        cache: TimestampedIndexCache,
        index_registry: &IndexRegistry, // Must be from the snapshot at end_ts
        end_ts: RepeatableTimestamp,
    ) -> anyhow::Result<Option<TimestampedIndexCache>> {
        let TimestampedIndexCache {
            mut cache,
            ts: begin_ts,
        } = cache;
        anyhow::ensure!(*begin_ts <= *end_ts);
        // Drop any cached indexes that are no longer in the registry (e.g.
        // deleted or no longer enabled).
        let unknown_indexes: Vec<_> = cache
            .tracked_index_ids()
            .filter(|id| index_registry.enabled_index_by_index_id(id).is_none())
            .collect();
        for index_id in unknown_indexes {
            cache.remove_index(index_id);
        }
        if *begin_ts != *end_ts {
            let from = (*begin_ts).succ()?;
            let snapshot = { self.inner.lock().log.clone() };

            if from <= snapshot.purged_ts {
                return Ok(None);
            }

            block_in_place(|| {
                'outer: for (index_name, writes) in snapshot.by_database_index.iter() {
                    let Some(index) = index_registry.get_enabled(index_name) else {
                        continue;
                    };
                    if !cache.is_index_tracked(&index.id()) {
                        continue;
                    }
                    let is_by_id = index.metadata.name.is_by_id();
                    let index_id = index.id();

                    for update in writes.range(from..=*end_ts) {
                        let ts = update.0.ts;
                        for write in &update.0.index_updates {
                            if !cache.apply_write(ts, index_id, is_by_id, write) {
                                break 'outer;
                            }
                        }
                    }
                }
            });
        }

        Ok(Some(TimestampedIndexCache { cache, ts: end_ts }))
    }
}

impl WriteLogIndexReader for LogReader {
    fn iter_writes_after<'a>(
        &self,
        index_name: &TabletIndexName,
        ts: Timestamp,
        storage: &'a mut ErasedSlot,
    ) -> anyhow::Result<Box<dyn Iterator<Item = &'a DatabaseIndexWrite> + 'a>> {
        let snapshot = self.inner.lock().log.clone();
        if ts < snapshot.purged_ts {
            anyhow::bail!("Timestamp is out of retention window");
        }
        let Some(index) = storage.insert(snapshot.by_database_index.0).get(index_name) else {
            return Ok(Box::new(iter::empty()));
        };
        Ok(Box::new(
            index
                .range((Bound::Excluded(ts), Bound::Unbounded))
                .flat_map(|write| write.index_updates.iter()),
        ))
    }
}

/// LogWriter can append to the log.
pub struct LogWriter {
    inner: Arc<Mutex<WriteLogManager>>,
}

impl LogWriter {
    // N.B.: `writes` is `OrderedWrites` because that's what the committer
    // already has, but the write log doesn't actually care about the ordering.
    //
    // N.B. log.append must be called before apply_writes to prevent a cache
    // interval from getting populated and checking the write log does not have any
    // overlapping writes in `IndexCache::populate` before the write is appended,
    // thereby missing a write.
    pub fn append(
        &mut self,
        ts: Timestamp,
        writes: IndexKeyWrites,
        apply_writes_callback: impl FnOnce(),
    ) {
        block_in_place(|| self.inner.lock().append(ts, writes));
        apply_writes_callback();
    }

    pub fn is_stale(
        &self,
        reads: &ReadSet,
        reads_ts: Timestamp,
        ts: Timestamp,
    ) -> anyhow::Result<Option<ConflictingReadWithWriteSource>> {
        let snapshot = { self.inner.lock().log.clone() };
        Ok(block_in_place(|| snapshot.is_stale(reads, reads_ts, ts))?)
    }

    pub fn snapshot(&self) -> WriteLogSnapshot {
        WriteLogSnapshot(self.inner.lock().log.clone())
    }
}

/// A point-in-time view of the write log that can be moved to another thread.
pub struct WriteLogSnapshot(WriteLog);

impl WriteLogSnapshot {
    pub fn max_ts(&self) -> Timestamp {
        self.0.max_ts()
    }

    pub fn is_stale(
        &self,
        reads: &ReadSet,
        reads_ts: Timestamp,
        ts: Timestamp,
    ) -> anyhow::Result<Option<ConflictingReadWithWriteSource>> {
        Ok(self.0.is_stale(reads, reads_ts, ts)?)
    }
}

/// The commit that wrote a database index key, and the document it wrote.
struct PendingKeyWriter {
    ts: Timestamp,
    document_id: ResolvedDocumentId,
}

/// The database index keys written by all pending commits for a single index,
/// sorted by key. Index keys end with the document id, and a document written
/// by one pending commit can't be written by another. The later commit read
/// the document at a timestamp before every pending commit, so it conflicts
/// with the earlier write. Each key therefore has exactly one pending writer.
#[derive(Default)]
struct PendingKeysInIndex(BTreeMap<Arc<[u8]>, PendingKeyWriter>);

impl PendingKeysInIndex {
    fn insert(&mut self, key: Arc<[u8]>, writer: PendingKeyWriter) {
        let ts = writer.ts;
        if let Some(existing) = self.0.insert(key, writer) {
            panic!(
                "index key written by pending commits at {} and {ts}",
                existing.ts
            );
        }
    }

    /// Removes the entry for `key`, which must have been written by the commit
    /// at `ts`.
    fn remove(&mut self, key: &[u8], ts: Timestamp) {
        let writer = self
            .0
            .remove(key)
            .unwrap_or_else(|| panic!("index key of pending write at {ts} is missing"));
        assert_eq!(
            writer.ts, ts,
            "index key of pending write at {ts} was written at {}",
            writer.ts
        );
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns an index key contained in `intervals` that has a pending write,
    /// along with the commit that wrote it.
    fn overlaps(&self, intervals: &IntervalSet) -> Option<(&[u8], &PendingKeyWriter)> {
        // Both sides are sorted by key, so probe whichever is smaller against
        // the other rather than scanning both.
        let (key, writer) = if self.0.len() <= intervals.len() {
            self.0.iter().find(|(key, _)| intervals.contains(key))?
        } else {
            intervals
                .iter_ref()
                .find_map(|interval| self.0.range::<[u8], _>(interval).next())?
        };
        Some((key, writer))
    }
}

struct PendingWrite {
    ts: Timestamp,
    writes: OrderedDocumentWrites,
    write_source: WriteSource,
    snapshot: Snapshot,
    /// The keys this commit added to [`PendingWrites::by_database_index`],
    /// retained so they can be removed once it is published.
    index_keys: BTreeMap<TabletIndexName, Vec<Arc<[u8]>>>,
}

/// Pending writes are used by the committer to detect conflicts between a new
/// commit and a commit that has started but has not finished writing to
/// persistence and snapshot_manager.
/// These pending writes do not conflict with each other so any subset of them
/// may be written to persistence, in any order.
pub struct PendingWrites {
    /// Pending commits in commit order, oldest first.
    by_ts: VecDeque<PendingWrite>,
    /// The database index keys of every pending commit, so that `is_stale` can
    /// intersect a read set with the pending writes by index and key instead of
    /// walking every pending document.
    ///
    /// Keying by name rather than index id relies on every pending key under
    /// one name having been computed with the same index definition.
    by_database_index: BTreeMap<TabletIndexName, PendingKeysInIndex>,
}

impl PendingWrites {
    pub fn new() -> Self {
        Self {
            by_ts: VecDeque::new(),
            by_database_index: BTreeMap::new(),
        }
    }

    pub fn push_back(
        &mut self,
        ts: Timestamp,
        writes: OrderedDocumentWrites,
        write_source: WriteSource,
        snapshot: Snapshot,
    ) -> PendingWriteHandle {
        if let Some(last) = self.by_ts.back() {
            assert!(last.ts < ts, "{:?} >= {}", last.ts, ts);
        }

        let index_keys = self.index_by_key(ts, &writes, &snapshot.index_registry);
        self.by_ts.push_back(PendingWrite {
            ts,
            writes,
            write_source,
            snapshot,
            index_keys,
        });
        PendingWriteHandle(ts)
    }

    /// Adds the database index keys the commit at `ts` writes to
    /// `by_database_index`, returning them so `pop_first` can take them out
    /// again.
    fn index_by_key(
        &mut self,
        ts: Timestamp,
        writes: &OrderedDocumentWrites,
        index_registry: &IndexRegistry,
    ) -> BTreeMap<TabletIndexName, Vec<Arc<[u8]>>> {
        let mut index_keys: BTreeMap<TabletIndexName, Vec<Arc<[u8]>>> = BTreeMap::new();
        for update in writes.iter() {
            let update_keys = index_registry.database_index_keys(
                update.id,
                update.old_document.as_ref().map(|(document, _)| document),
                update.new_document.as_ref(),
            );
            for (index_name, Update { old, new }) in update_keys {
                // An update that leaves the indexed fields alone writes the
                // same key twice.
                let new = new.filter(|new| old.as_ref() != Some(new));
                let index = self
                    .by_database_index
                    .entry(index_name.clone())
                    .or_default();
                let keys = index_keys.entry(index_name).or_default();
                for key in old.into_iter().chain(new) {
                    let key: Arc<[u8]> = Arc::from(key.0);
                    index.insert(
                        key.clone(),
                        PendingKeyWriter {
                            ts,
                            document_id: update.id,
                        },
                    );
                    keys.push(key);
                }
            }
        }
        index_keys
    }

    pub fn latest_snapshot(&self) -> Option<Snapshot> {
        self.by_ts
            .back()
            .map(|pending_write| pending_write.snapshot.clone())
    }

    /// Recomputes the snapshot associated with each pending write, rebasing the
    /// pending writes on the new base snapshot provided.
    pub fn recompute_pending_snapshots(&mut self, mut base_snapshot: Snapshot) {
        for pending_write in self.by_ts.iter_mut() {
            let ts = pending_write.ts;
            for document_update in pending_write.writes.iter() {
                base_snapshot
                    .update(&document_update.unpack(), ts)
                    .expect("Failed to update snapshot");
            }
            pending_write.snapshot = base_snapshot.clone();
        }
    }

    /// Returns a pending write that conflicts with `reads`, if there is one.
    #[fastrace::trace]
    pub fn is_stale(&self, reads: &ReadSet) -> Option<ConflictingReadWithWriteSource> {
        self.database_index_conflict(reads)
            .or_else(|| self.text_index_conflict(reads))
    }

    /// Intersects the read set's index intervals with the pending index keys,
    /// touching only the indexes that were read.
    fn database_index_conflict(&self, reads: &ReadSet) -> Option<ConflictingReadWithWriteSource> {
        reads.iter_indexed().find_map(|(index, index_reads)| {
            let pending = self.by_database_index.get(index)?;
            let (index_key, writer) = pending.overlaps(&index_reads.intervals)?;
            Some(ConflictingReadWithWriteSource {
                read: ConflictingRead {
                    index: index.clone(),
                    id: writer.document_id,
                    stack_traces: index_reads.stack_traces_covering(index_key),
                },
                write_source: self.write_source(writer.ts).clone(),
                write_ts: writer.ts,
            })
        })
    }

    /// Checks the read set against every pending document.
    ///
    /// Text index keys aren't indexed like database index keys are: tokenizing
    /// a document is too expensive to do on the committer thread, so it is
    /// deferred until the commit is written to persistence. Text index reads
    /// are rare enough that the linear scan is worth avoiding that cost.
    fn text_index_conflict(&self, reads: &ReadSet) -> Option<ConflictingReadWithWriteSource> {
        if !reads.has_search_reads() {
            return None;
        }
        self.by_ts.iter().find_map(|pending_write| {
            let read = pending_write
                .writes
                .iter()
                .flat_map(|update| {
                    [
                        update.old_document.as_ref().map(|(document, _)| document),
                        update.new_document.as_ref(),
                    ]
                })
                .flatten()
                .find_map(|document| reads.search_overlaps_document(document))?;
            Some(ConflictingReadWithWriteSource {
                read,
                write_source: pending_write.write_source.clone(),
                write_ts: pending_write.ts,
            })
        })
    }

    fn write_source(&self, ts: Timestamp) -> &WriteSource {
        let position = self
            .by_ts
            .binary_search_by(|pending_write| pending_write.ts.cmp(&ts))
            .expect("pending index key without a pending write");
        &self.by_ts[position].write_source
    }

    pub fn pop_first(&mut self, handle: PendingWriteHandle) -> (OrderedDocumentWrites, Snapshot) {
        let pending_write = self
            .by_ts
            .pop_front()
            .unwrap_or_else(|| panic!("commit at {} not pending", handle.0));
        let ts = pending_write.ts;
        assert_eq!(
            ts, handle.0,
            "pending write handle ts {} does not match first pending write {ts}",
            handle.0,
        );
        for (index_name, keys) in pending_write.index_keys {
            let Entry::Occupied(mut entry) = self.by_database_index.entry(index_name) else {
                panic!("index of pending write is missing");
            };
            let index = entry.get_mut();
            for key in keys {
                index.remove(&key, ts);
            }
            if index.is_empty() {
                entry.remove();
            }
        }
        (pending_write.writes, pending_write.snapshot)
    }

    pub fn min_ts(&self) -> Option<Timestamp> {
        self.by_ts.front().map(|pending_write| pending_write.ts)
    }
}

pub struct PendingWriteHandle(Timestamp);

impl PendingWriteHandle {
    pub fn must_commit_ts(&self) -> Timestamp {
        self.0
    }
}
