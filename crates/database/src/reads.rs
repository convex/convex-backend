//! Read set tracking for an active transaction
use std::{
    any::Any,
    collections::BTreeMap,
    mem,
    ops::Bound,
    sync::LazyLock,
};

use cmd_util::env::env_config;
use common::{
    bootstrap_model::index::database_index::IndexedFields,
    components::ComponentPath,
    document::PackedDocument,
    document_index_keys::{
        DatabaseIndexWrite,
        TextIndexWrite,
    },
    interval::{
        Interval,
        IntervalSet,
    },
    static_span,
    types::{
        TabletIndexName,
        Timestamp,
    },
    virtual_system_mapping::VirtualSystemMapping,
};
use errors::ErrorMetadata;
use imbl::{
    OrdMap,
    OrdSet,
};
use search::QueryReads as SearchQueryReads;
use usage_tracking::FunctionUsageTracker;
use value::{
    heap_size::{
        HeapSize,
        WithHeapSize,
    },
    TableName,
    TabletId,
};

#[cfg(doc)]
use crate::Transaction;
use crate::{
    database::{
        ConflictingRead,
        ConflictingReadWithWriteSource,
    },
    execution_size::TransactionLimits,
    stack_traces::StackTrace,
    write_log::ArcWriteInIndex,
};

pub const OVER_LIMIT_HELP: &str = "Consider using smaller limits in your queries, paginating your \
                                   queries, or using indexed queries with a selective index range \
                                   expressions.";

/// If set to 'true', then collect backtraces of every database read in order
/// to help debug OCC errors. Collecting stack traces is expensive and should
/// only be used in development.
static READ_SET_CAPTURE_BACKTRACES: LazyLock<bool> =
    LazyLock::new(|| env_config("READ_SET_CAPTURE_BACKTRACES", false));

#[derive(Debug, Clone)]
pub struct IndexReads {
    pub fields: IndexedFields,
    pub intervals: IntervalSet,
    pub stack_traces: Option<Vec<(Interval, StackTrace)>>,
}

impl IndexReads {
    /// The stack traces of the reads that cover `index_key`, or `None` if
    /// backtraces aren't being collected.
    pub(crate) fn stack_traces_covering(&self, index_key: &[u8]) -> Option<Vec<StackTrace>> {
        let stack_traces = self.stack_traces.as_ref()?;
        Some(
            stack_traces
                .iter()
                .filter(|(interval, _)| interval.contains(index_key))
                .map(|(_, trace)| trace.clone())
                .collect(),
        )
    }
}

impl HeapSize for IndexReads {
    fn heap_size(&self) -> usize {
        self.fields.heap_size() + self.intervals.heap_size()
    }
}

#[derive(Debug, Clone)]
pub struct ReadSet {
    indexed: WithHeapSize<BTreeMap<TabletIndexName, IndexReads>>,
    search: WithHeapSize<BTreeMap<TabletIndexName, SearchQueryReads>>,
}

impl HeapSize for ReadSet {
    fn heap_size(&self) -> usize {
        self.indexed.heap_size() + self.search.heap_size()
    }
}

impl ReadSet {
    pub fn empty() -> Self {
        Self {
            indexed: WithHeapSize::default(),
            search: WithHeapSize::default(),
        }
    }

    pub fn new(
        indexed: BTreeMap<TabletIndexName, IndexReads>,
        search: BTreeMap<TabletIndexName, SearchQueryReads>,
    ) -> Self {
        Self {
            indexed: indexed.into(),
            search: search.into(),
        }
    }

    /// Iterate over all range reads for the given index.
    pub fn iter_indexed(&self) -> impl Iterator<Item = (&TabletIndexName, &IndexReads)> {
        self.indexed.iter()
    }

    pub fn iter_search(&self) -> impl Iterator<Item = (&TabletIndexName, &SearchQueryReads)> {
        self.search.iter()
    }

    pub fn has_search_reads(&self) -> bool {
        !self.search.is_empty()
    }

    pub fn consume(
        self,
    ) -> (
        impl Iterator<Item = (TabletIndexName, IndexReads)>,
        impl Iterator<Item = (TabletIndexName, SearchQueryReads)>,
    ) {
        (self.indexed.into_iter(), self.search.into_iter())
    }

    /// Determine whether a mutation to a document overlaps with the text index
    /// reads in the read set.
    pub fn search_overlaps_document(&self, document: &PackedDocument) -> Option<ConflictingRead> {
        for (index, search_reads) in iter_indexes_for_table(&self.search, document.id().tablet_id) {
            if search_reads.overlaps_document(document) {
                return Some(ConflictingRead {
                    index: index.clone(),
                    id: document.id(),
                    stack_traces: None,
                });
            }
        }
        None
    }

    /// Check whether any writes in the given index maps in the timestamp
    /// range `(from, to]` conflict with this read set. Only looks up indexes
    /// that were read.
    #[fastrace::trace]
    pub(crate) fn writes_overlap_by_index(
        &self,
        by_database_index: &OrdMap<TabletIndexName, OrdSet<ArcWriteInIndex<DatabaseIndexWrite>>>,
        by_search_index: &OrdMap<TabletIndexName, OrdSet<ArcWriteInIndex<TextIndexWrite>>>,
        from: Timestamp,
        to: Timestamp,
    ) -> Option<ConflictingReadWithWriteSource> {
        // Check database index reads
        for (index, index_reads) in self.indexed.iter() {
            let Some(updates) = by_database_index.get(index) else {
                continue;
            };
            for write in updates.range((Bound::Excluded(from), Bound::Included(to))) {
                for update in &write.index_updates {
                    for index_key in update.update.iter() {
                        if index_reads.intervals.contains(index_key) {
                            return Some(ConflictingReadWithWriteSource {
                                read: ConflictingRead {
                                    index: index.clone(),
                                    id: update.document_id,
                                    stack_traces: index_reads.stack_traces_covering(index_key),
                                },
                                write_source: write.write_source.clone(),
                                write_ts: write.ts,
                            });
                        }
                    }
                }
            }
        }
        // Check search index reads
        for (index, search_reads) in self.search.iter() {
            let Some(updates) = by_search_index.get(index) else {
                continue;
            };
            for write in updates.range((Bound::Excluded(from), Bound::Included(to))) {
                for update in &write.index_updates {
                    for value in update.update.iter() {
                        if search_reads.overlaps_search_index_key_value(value) {
                            return Some(ConflictingReadWithWriteSource {
                                read: ConflictingRead {
                                    index: index.clone(),
                                    id: update.document_id,
                                    stack_traces: None,
                                },
                                write_source: write.write_source.clone(),
                                write_ts: write.ts,
                            });
                        }
                    }
                }
            }
        }
        None
    }
}

/// Iterates just those pairs in `map` whose table matches `tablet_id`
fn iter_indexes_for_table<T>(
    map: &BTreeMap<TabletIndexName, T>,
    tablet_id: TabletId,
) -> impl Iterator<Item = (&TabletIndexName, &T)> {
    // uses the fact that TabletIndexName is ordered by TabletId first,
    // then descriptor
    map.range(TabletIndexName::min_for_table(tablet_id)..)
        .take_while(move |(index, _)| *index.table() == tablet_id)
}

/// Tracks the read set for the current transaction. Records successful reads as
/// well as missing documents so we can ensure future reads in this transaction
/// are consistent against the current snapshot.
///
/// [`Transaction`] keeps this read set up to date when accessing documents
/// or the index. We want to minimize the amount of code that updates this state
/// so we avoid missing an update.
#[derive(Debug, Clone)]
pub struct TransactionReadSet {
    read_set: ReadSet,

    // Pre-computed sum of all of the `IntervalSet`'s sizes.
    num_intervals: usize,

    user_tx_size: TransactionReadSize,
    system_tx_size: TransactionReadSize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, derive_more::Add, derive_more::AddAssign)]
pub struct TransactionReadSize {
    // Sum of doc.size() for all documents read.
    pub total_document_size: usize,
    // Count of all documents read.
    pub total_document_count: usize,
}

impl TransactionReadSet {
    /// Create a read-set at the given timestamp.
    pub fn new() -> Self {
        Self {
            read_set: ReadSet::empty(),
            num_intervals: 0,
            user_tx_size: TransactionReadSize::default(),
            system_tx_size: TransactionReadSize::default(),
        }
    }

    pub fn into_read_set(self) -> ReadSet {
        self.read_set
    }

    pub fn read_set(&self) -> &ReadSet {
        &self.read_set
    }

    fn _record_indexed(
        &mut self,
        index_name: TabletIndexName,
        fields: IndexedFields,
        mut intervals: impl IntoIterator<Item = Interval> + 'static,
    ) -> (usize, usize) {
        self.read_set.indexed.mutate_entry_or_insert_with(
            index_name.clone(),
            || IndexReads {
                fields: fields.clone(),
                intervals: IntervalSet::new(),
                stack_traces: (*READ_SET_CAPTURE_BACKTRACES).then_some(vec![]),
            },
            |reads| {
                let IndexReads {
                    intervals: range_set,
                    stack_traces,
                    fields: existing_fields,
                } = reads;

                assert_eq!(
                    *existing_fields, fields,
                    "trying to change index fields for index {index_name:?}!"
                );

                let range_num_intervals_before = range_set.len();
                if range_set.is_empty()
                    && let Some(intervals) =
                        (&mut intervals as &mut dyn Any).downcast_mut::<IntervalSet>()
                {
                    // optimization: reuse the existing IntervalSet
                    *range_set = mem::take(intervals);
                    if let Some(stack_traces) = stack_traces.as_mut() {
                        for interval in range_set.iter() {
                            stack_traces.push((interval, StackTrace::new()));
                        }
                    }
                } else {
                    for interval in intervals {
                        if let Some(stack_traces) = stack_traces.as_mut() {
                            stack_traces.push((interval.clone(), StackTrace::new()));
                        }
                        range_set.add(interval);
                    }
                }
                let range_num_intervals_after = range_set.len();

                (range_num_intervals_before, range_num_intervals_after)
            },
        )
    }

    /// Call record_indexed_derived to take a read dependency when the user
    /// didn't directly initiate the read and the read didn't go to persistence,
    /// but we are taking a read dependency anyway.
    /// For example, when writing to a table, take a derived read on the table
    /// to make sure it still exists.
    pub fn record_indexed_derived(
        &mut self,
        index_name: TabletIndexName,
        fields: IndexedFields,
        interval: Interval,
    ) {
        self._record_indexed(index_name, fields, [interval]);
    }

    pub fn merge(
        &mut self,
        reads: ReadSet,
        num_intervals: usize,
        user_tx_size: TransactionReadSize,
        system_tx_size: TransactionReadSize,
    ) {
        let (index_reads, search_reads) = reads.consume();
        for (index_name, index_reads) in index_reads {
            self._record_indexed(index_name, index_reads.fields, index_reads.intervals);
        }
        for (index_name, search_reads) in search_reads {
            self.record_search(index_name, search_reads);
        }
        self.num_intervals += num_intervals;
        self.user_tx_size += user_tx_size;
        self.system_tx_size += system_tx_size;
    }

    pub fn record_read_document(
        &mut self,
        component_path: ComponentPath,
        table_name: TableName,
        document_size: usize,
        usage_tracker: &FunctionUsageTracker,
        virtual_system_mapping: &VirtualSystemMapping,
        limits: &TransactionLimits,
    ) -> anyhow::Result<()> {
        // Database bandwidth for document reads
        // TODO: Remove when we switch over to using egress_v2
        let skip_logging_usage = table_name.is_system();
        usage_tracker.track_database_egress(
            component_path.clone(),
            &table_name,
            document_size as u64,
            skip_logging_usage,
        );
        usage_tracker.track_database_egress_v2(
            component_path.clone(),
            &table_name,
            document_size as u64,
            table_name.is_system(),
        );
        if let Some(virtual_table_name) =
            virtual_system_mapping.associated_virtual_table_name(&table_name)
        {
            usage_tracker.track_virtual_table_egress(
                component_path.clone(),
                virtual_table_name,
                document_size as u64,
            );
        }
        usage_tracker.track_database_egress_rows(
            component_path,
            &table_name,
            1,
            skip_logging_usage,
        );

        let max_rows = limits.documents_read;
        let max_bytes = limits.bytes_read;

        let tx_size = if skip_logging_usage {
            &mut self.system_tx_size
        } else {
            &mut self.user_tx_size
        };

        // We always increment the size first, even if we throw,
        // we want the size to reflect the read, so that
        // we can tell that we threw and not issue a warning.
        tx_size.total_document_count += 1;
        tx_size.total_document_size += document_size;

        if !skip_logging_usage {
            anyhow::ensure!(
                tx_size.total_document_count <= max_rows,
                ErrorMetadata::pagination_limit(
                    "TooManyDocumentsRead",
                    format!(
                        "Too many documents read in a single function execution (limit: {}). \
                         {OVER_LIMIT_HELP}",
                        max_rows,
                    )
                ),
            );
            anyhow::ensure!(
                tx_size.total_document_size <= max_bytes,
                ErrorMetadata::pagination_limit(
                    "TooManyBytesRead",
                    format!(
                        "Too many bytes read in a single function execution (limit: {} bytes). \
                         {OVER_LIMIT_HELP}",
                        max_bytes,
                    )
                ),
            );
        }
        Ok(())
    }

    pub fn record_indexed_directly(
        &mut self,
        index_name: TabletIndexName,
        fields: IndexedFields,
        interval: Interval,
        limits: &TransactionLimits,
    ) -> anyhow::Result<()> {
        let _s = static_span!();

        let (num_intervals_before, num_intervals_after) =
            self._record_indexed(index_name, fields, [interval]);

        self.num_intervals = self.num_intervals.saturating_sub(num_intervals_before);
        self.num_intervals += num_intervals_after;
        let max_intervals = limits.database_queries;
        if self.num_intervals > max_intervals {
            anyhow::bail!(
                anyhow::anyhow!("top three: {}", self.top_three_intervals()).context(
                    ErrorMetadata::pagination_limit(
                        "TooManyReads",
                        format!(
                            "Too many reads in a single function execution (limit: {}). \
                             {OVER_LIMIT_HELP}",
                            max_intervals,
                        ),
                    )
                )
            );
        }
        Ok(())
    }

    pub fn top_three_intervals(&self) -> String {
        let mut intervals: Vec<_> = self
            .read_set
            .indexed
            .iter()
            .map(|(index, reads)| (reads.intervals.len(), index))
            .collect();
        intervals.sort_by_key(|(len, _)| *len);
        let top_three = intervals
            .iter()
            .rev()
            .take(3)
            .map(|(amt, index)| format!("{index}: {amt}"))
            .collect::<Vec<_>>();
        top_three.join(",")
    }

    pub fn record_search(&mut self, index_name: TabletIndexName, search_reads: SearchQueryReads) {
        self.read_set.search.mutate_entry_or_insert_with(
            index_name,
            SearchQueryReads::empty,
            |existing_reads| existing_reads.merge(search_reads),
        );
    }

    pub fn num_intervals(&self) -> usize {
        self.num_intervals
    }

    pub fn user_tx_size(&self) -> &TransactionReadSize {
        &self.user_tx_size
    }

    pub fn system_tx_size(&self) -> &TransactionReadSize {
        &self.system_tx_size
    }
}
