use std::{
    collections::{
        BTreeMap,
        VecDeque,
    },
    future::Future,
};

use anyhow::Context;
use async_trait::async_trait;
use common::{
    bootstrap_model::tables::TableState,
    components::{
        ComponentId,
        ComponentPath,
    },
    document::{
        DocumentUpdateWithPrevTs,
        ResolvedDocument,
    },
    knobs::{
        MAX_BYTES_WRITTEN_PER_SECOND,
        MAX_ROWS_WRITTEN_PER_SECOND,
        MAX_TRANSACTION_WINDOW,
    },
    runtime::block_in_place,
    types::{
        DatabaseIndexUpdate,
        RepeatableReason,
        RepeatableTimestamp,
        Timestamp,
        WriteTimestamp,
    },
    virtual_system_mapping::VirtualSystemMapping,
};
use errors::ErrorMetadata;
use imbl::OrdMap;
use indexing::{
    in_memory_indexes::BackendInMemoryIndexes,
    index_registry::IndexRegistry,
};
use parking_lot::Mutex;
use search::{
    TextIndexManager,
    TextIndexWriteSize,
};
use tokio::sync::oneshot;
use value::{
    ResolvedDocumentId,
    TableMapping,
    TableName,
    TableNamespace,
    TabletId,
};
use vector::{
    VectorIndexManager,
    VectorIndexWriteSize,
};

use crate::{
    schema_registry::SchemaRegistry,
    table_registry::{
        TableUpdate,
        TableUpdateMode,
    },
    table_summary::{
        table_summary_bootstrapping_error,
        TableSummarySnapshot,
    },
    transaction::TableCountSnapshot,
    write_log::PendingWrites,
    write_throughput_limiter::{
        WriteThroughputLimit,
        WriteThroughputLimiter,
        WriteVolume,
    },
    ComponentRegistry,
    TableCount,
    TableRegistry,
    TableUsage,
    TablesUsage,
    TransactionReadSet,
};

/// The [SnapshotManager] maintains multiple versions of the [Snapshot]s at
/// different timestamps. The snapshots internally use immutable data structures
/// with copy-on-write semantics to provide memory-efficient multiversioning.
///
/// We maintain a bounded time range of versions,
/// determined by `MAX_TRANSACTION_WINDOW`, allowing the `Database` layer to
/// begin a transaction in any timestamp within that range.
pub struct SnapshotManager {
    persisted_max_repeatable_ts: Timestamp,
    versions: VecDeque<(Timestamp, Snapshot)>,
    write_throughput_limiter: WriteThroughputLimiter,
    waiters: Mutex<VecDeque<(Timestamp, oneshot::Sender<()>)>>,
}

#[derive(Clone)]
/// Per-table document counts and sizes, filtered to tables that exist, plus
/// aggregate user document and size counts.
pub struct TableCounts {
    pub tables: OrdMap<TabletId, TableCount>,
    pub num_user_documents: u64,
    /// Size of user documents in user tables
    pub user_tables_size: u64,
    /// Size of user documents in user tables and virtual tables
    pub user_docs_size: u64,
}

#[async_trait]
impl TableCountSnapshot for Option<TableCounts> {
    async fn count(&self, table: TabletId) -> anyhow::Result<Option<u64>> {
        let result = match self {
            Some(table_counts) => {
                let count = table_counts
                    .tables
                    .get(&table)
                    .map_or(0, |count| count.num_values());
                Some(count)
            },
            None => None,
        };
        Ok(result)
    }
}

impl TableCounts {
    pub fn new(
        TableSummarySnapshot { tables, ts: _ }: TableSummarySnapshot,
        table_mapping: &TableMapping,
        virtual_table_mapping: &VirtualSystemMapping,
    ) -> anyhow::Result<Self> {
        // Filter out non-existent tables before counting. Otherwise is_system_table
        // will return false and count non-existent tables toward user document counts.
        let tables: OrdMap<TabletId, TableCount> = tables
            .into_iter()
            .filter(|(table_id, _table_summary)| table_mapping.tablet_id_exists(*table_id))
            .map(|(table_id, table_summary)| (table_id, *table_summary.count()))
            .collect::<OrdMap<_, _>>();
        let (num_user_documents, user_tables_size) = tables
            .iter()
            .filter(|(table_id, _count)| !table_mapping.is_system_tablet(**table_id))
            .fold((0, 0), |(acc_docs, acc_size), (_, count)| {
                (acc_docs + count.num_values(), acc_size + count.total_size())
            });
        let user_docs_size = tables.iter().try_fold(0, |acc_size, (table_id, count)| {
            let is_system_table = table_mapping.is_system_tablet(*table_id);
            let has_virtual_table =
                virtual_table_mapping.has_virtual_table(&table_mapping.tablet_name(*table_id)?);
            anyhow::Ok(if has_virtual_table || !is_system_table {
                acc_size + count.total_size()
            } else {
                acc_size
            })
        })?;
        Ok(Self {
            tables,
            num_user_documents,
            user_tables_size,
            user_docs_size,
        })
    }

    pub fn tablet_count(&self, table: &TabletId) -> TableCount {
        self.tables
            .get(table)
            .copied()
            .unwrap_or_else(TableCount::empty)
    }

    pub(crate) fn update(
        &mut self,
        document_id: ResolvedDocumentId,
        old: Option<&ResolvedDocument>,
        new: Option<&ResolvedDocument>,
        table_update: Option<&TableUpdate>,
        table_mapping: &TableMapping,
        virtual_system_mapping: &VirtualSystemMapping,
    ) -> anyhow::Result<()> {
        let _timer = crate::metrics::table_summary_update_timer();
        let mut table_count = *self
            .tables
            .get(&document_id.tablet_id)
            .with_context(|| format!("Updating non-existent table {}", document_id.tablet_id))?;
        if let Some(old_value) = old {
            table_count
                .remove(&old_value.value().0)
                .with_context(|| format!("removing from table {}", document_id.tablet_id))?;
        }
        if let Some(new_value) = new {
            table_count.insert(&new_value.value().0);
        }
        if let Some(TableUpdate {
            namespace: _,
            table_id_and_number,
            table_name: _,
            mode,
        }) = table_update
        {
            match mode {
                TableUpdateMode::Create(_) => {
                    assert!(self
                        .tables
                        .insert(table_id_and_number.tablet_id, TableCount::empty())
                        .is_none());
                },
                TableUpdateMode::Activate => {},
                TableUpdateMode::Drop => {
                    self.tables.remove(&table_id_and_number.tablet_id);
                },
                TableUpdateMode::HardDelete => {
                    assert!(
                        self.tables.remove(&table_id_and_number.tablet_id).is_none(),
                        "Hard-deleted tablet found in table counts, but it should have already \
                         been removed when it was marked Deleting"
                    );
                },
            }
        }
        let new_info_num_values = table_count.num_values();
        let new_info_total_size = table_count.total_size();
        match self.tables.insert(document_id.tablet_id, table_count) {
            Some(old_count) => {
                let is_system_table = table_mapping.is_system_tablet(document_id.tablet_id);
                if !is_system_table {
                    self.num_user_documents =
                        self.num_user_documents + new_info_num_values - old_count.num_values();
                    self.user_tables_size =
                        self.user_tables_size + new_info_total_size - old_count.total_size();
                }
                if virtual_system_mapping
                    .has_virtual_table(&table_mapping.tablet_name(document_id.tablet_id)?)
                    || !is_system_table
                {
                    self.user_docs_size =
                        self.user_docs_size + new_info_total_size - old_count.total_size();
                }
            },
            None => panic!("Applying update for non-existent table!"),
        }
        Ok(())
    }
}
/// A snapshot of the database indexes and metadata at a certain timestamp.
#[derive(Clone)]
pub struct Snapshot {
    pub table_registry: TableRegistry,
    pub schema_registry: SchemaRegistry,
    pub component_registry: ComponentRegistry,
    pub table_counts: Option<TableCounts>,
    pub index_registry: IndexRegistry,
    pub in_memory_indexes: BackendInMemoryIndexes,
    pub virtual_system_mapping: VirtualSystemMapping,
    pub text_indexes: TextIndexManager,
    pub vector_indexes: VectorIndexManager,
}

impl Snapshot {
    pub fn update(
        &mut self,
        document_update: &DocumentUpdateWithPrevTs,
        commit_ts: Timestamp,
    ) -> anyhow::Result<(
        Vec<DatabaseIndexUpdate>,
        VectorIndexWriteSize,
        TextIndexWriteSize,
    )> {
        block_in_place(|| {
            let removal = document_update
                .old_document
                .as_ref()
                .map(|(document, _)| document);
            let insertion = document_update.new_document.as_ref();
            let document_id = document_update.id;
            let table_update = self
                .table_registry
                .update(
                    &self.index_registry,
                    document_id,
                    removal.map(|d| &d.value().0),
                    insertion.map(|d| &d.value().0),
                )
                .context("Table registry update failed")?;
            self.schema_registry.update(
                self.table_registry.table_mapping(),
                document_id,
                removal,
                insertion,
            )?;
            self.component_registry.update(
                self.table_registry.table_mapping(),
                document_id,
                removal,
                insertion,
            )?;
            if let Some(table_counts) = self.table_counts.as_mut() {
                table_counts
                    .update(
                        document_id,
                        removal,
                        insertion,
                        table_update.as_ref(),
                        self.table_registry.table_mapping(),
                        &self.virtual_system_mapping,
                    )
                    .context("Table counts update failed")?;
            };

            self.index_registry
                .update(removal, insertion)
                .context("Index update failed")?;
            let in_memory_index_updates = self.in_memory_indexes.update(
                &self.index_registry,
                commit_ts,
                document_update
                    .old_document
                    .clone()
                    .map(|(document, ts)| (document, WriteTimestamp::Committed(ts))),
                insertion.cloned(),
            );

            let text_index_write_size = self
                .text_indexes
                .update(
                    &self.index_registry,
                    removal,
                    insertion,
                    WriteTimestamp::Committed(commit_ts),
                )
                .context("Search index update failed")?;

            let vector_index_write_size = self
                .vector_indexes
                .update(
                    &self.index_registry,
                    removal,
                    insertion,
                    WriteTimestamp::Committed(commit_ts),
                )
                .context("Vector index update failed")?;
            Ok((
                in_memory_index_updates,
                vector_index_write_size,
                text_index_write_size,
            ))
        })
    }

    pub fn must_table_counts(&self) -> anyhow::Result<&TableCounts> {
        self.table_counts
            .as_ref()
            .ok_or_else(|| table_summary_bootstrapping_error(None))
    }

    pub fn iter_table_counts(
        &self,
    ) -> anyhow::Result<
        impl Iterator<
                Item = anyhow::Result<(
                    TableNamespace,
                    Option<ComponentPath>,
                    TableName,
                    &'_ TableCount,
                )>,
            > + '_,
    > {
        let result = self
            .must_table_counts()?
            .tables
            .iter()
            .filter(|(table_id, _)| {
                matches!(
                    self.table_registry.tablet_states().get(table_id),
                    Some(TableState::Active)
                )
            })
            .map(|(table_id, table_count)| {
                let namespace = self
                    .table_mapping()
                    .tablet_namespace(*table_id)
                    .expect("active table should have namespace");
                let table_name = self
                    .table_mapping()
                    .tablet_name(*table_id)
                    .expect("active table should have name");
                let component_path = self.component_registry.get_component_path(
                    ComponentId::from(namespace),
                    &mut TransactionReadSet::new(),
                );

                if component_path.is_none() && !table_name.is_system() {
                    let count = table_count.num_values();
                    let table_size = table_count.total_size();
                    if count != 0 {
                        // If there is no component path for this table namespace, this must be an
                        // empty user table left over from incomplete
                        // components push. System tables may be created
                        // earlier (e.g. `_schemas`), so they may be
                        // legitimately nonempty in that case.
                        anyhow::bail!(
                            "Table {table_name} is in an orphaned TableNamespace without a \
                             component, but has document count {count}",
                        );
                    }
                    if table_size != 0 {
                        anyhow::bail!(
                            "Table {table_name} is in an orphaned TableNamespace without a \
                             component, but has document size {table_size}",
                        );
                    }
                }
                Ok((namespace, component_path, table_name, table_count))
            });
        Ok(result)
    }

    pub fn table_mapping(&self) -> &TableMapping {
        self.table_registry.table_mapping()
    }

    pub fn table_count(&self, namespace: TableNamespace, table: &TableName) -> Option<TableCount> {
        let table_id = match self.table_mapping().namespace(namespace).id(table) {
            Ok(table_id) => table_id,
            Err(_) => return Some(TableCount::empty()),
        };
        let table_counts = self.table_counts.as_ref()?;
        Some(table_counts.tablet_count(&table_id.tablet_id))
    }

    pub fn must_table_count(
        &self,
        namespace: TableNamespace,
        table: &TableName,
    ) -> anyhow::Result<TableCount> {
        self.table_count(namespace, table)
            .context(table_summary_bootstrapping_error(None))
    }

    /// Counts storage space used by all tables, including system tables
    pub fn get_document_and_index_storage(&self) -> anyhow::Result<TablesUsage> {
        let table_mapping: TableMapping = self.table_mapping().clone();

        let mut user_document_storage_by_table = BTreeMap::new();
        let mut system_document_storage_by_table = BTreeMap::new();
        let mut orphaned_document_storage_by_table = BTreeMap::new();
        for entry in self.iter_table_counts()? {
            let (namespace, component_path, table_name, table_count) = entry?;
            let usage = TableUsage {
                document_size: table_count.total_size(),
                index_size: 0,
                system_index_size: 0,
            };
            if let Some(component_path) = component_path {
                if table_name.is_system() {
                    system_document_storage_by_table
                        .insert((namespace, table_name), (usage, component_path));
                } else {
                    user_document_storage_by_table
                        .insert((namespace, table_name), (usage, component_path));
                }
            } else {
                orphaned_document_storage_by_table.insert((namespace, table_name), usage);
            }
        }

        // TODO: We are currently using document size * index count as a rough
        // approximation for how much storage indexes use, but we should fix this to
        // only charge for the fields that are indexed.
        for index in self.index_registry.all_indexes().filter(|index| {
            // Only count storage for active tables (not hidden)
            // Only count storage for indexes that are fully backfilled or enabled.
            let active = table_mapping.is_active(*index.name.table());
            let enabled = index.config.is_enabled();
            let backfilled_staged = index.config.is_backfilled() && index.config.is_staged();
            active && (enabled || backfilled_staged)
        }) {
            let table_namespace = table_mapping.tablet_namespace(*index.name.table())?;
            let index_name = index
                .name
                .clone()
                .map_table(&table_mapping.tablet_to_name())
                .unwrap();
            let table_usage = if let Some(component_path) =
                self.component_registry.get_component_path(
                    ComponentId::from(table_namespace),
                    &mut TransactionReadSet::new(),
                ) {
                let key = (table_namespace, index_name.table().clone());
                let document_storage_by_table = if index_name.table().is_system() {
                    &mut system_document_storage_by_table
                } else {
                    &mut user_document_storage_by_table
                };
                let table_usage = document_storage_by_table.get_mut(&key).with_context(|| {
                    format!(
                        "Index {index_name} on a nonexistent table {:?} in namespace \
                         {table_namespace:?}",
                        index_name.table(),
                    )
                })?;
                anyhow::ensure!(table_usage.1 == component_path);
                &mut table_usage.0
            } else {
                // This can happen if component push did not complete. Becomes orphaned.
                let key = (table_namespace, index_name.table().clone());
                orphaned_document_storage_by_table
                    .get_mut(&key)
                    .with_context(|| {
                        format!(
                            "Index {index_name} on a nonexistent table {table_name} in namespace \
                             {table_namespace:?}",
                            table_name = key.1
                        )
                    })?
            };
            if index_name.is_system_owned() {
                table_usage.system_index_size += table_usage.document_size;
            } else {
                table_usage.index_size += table_usage.document_size;
            }
        }

        let mut virtual_tables = BTreeMap::new();
        for ((table_namespace, table_name), (usage, component_path)) in
            &system_document_storage_by_table
        {
            if let Some(virtual_table_name) = self
                .virtual_system_mapping
                .associated_virtual_table_name(table_name)
            {
                let key = (*table_namespace, virtual_table_name.clone());
                virtual_tables
                    .entry(key)
                    .and_modify(|(existing_usage, _): &mut (TableUsage, ComponentPath)| {
                        *existing_usage += *usage
                    })
                    .or_insert_with(|| (*usage, component_path.clone()));
            }
        }

        Ok(TablesUsage {
            user_tables: user_document_storage_by_table,
            system_tables: system_document_storage_by_table,
            orphaned_tables: orphaned_document_storage_by_table,
            virtual_tables,
        })
    }

    pub fn component_ids_to_paths(&self) -> BTreeMap<ComponentId, ComponentPath> {
        self.component_registry
            .all_component_paths(&mut TransactionReadSet::new())
    }
}

impl SnapshotManager {
    pub fn new(initial_ts: Timestamp, initial_snapshot: Snapshot) -> Self {
        let mut versions = VecDeque::new();
        versions.push_back((initial_ts, initial_snapshot));
        Self {
            versions,
            persisted_max_repeatable_ts: initial_ts,
            write_throughput_limiter: WriteThroughputLimiter::new(),
            waiters: Mutex::new(VecDeque::new()),
        }
    }

    pub fn latest(&self) -> (RepeatableTimestamp, Snapshot) {
        let (ts, snapshot) = self
            .versions
            .back()
            .cloned()
            .expect("snapshot versions empty");
        (
            RepeatableTimestamp::new_validated(ts, RepeatableReason::SnapshotManagerLatest),
            snapshot,
        )
    }

    pub fn latest_snapshot(&self) -> Snapshot {
        let (_, snapshot) = self.latest();
        snapshot
    }

    pub fn earliest_ts(&self) -> Timestamp {
        self.versions
            .front()
            .map(|(ts, ..)| *ts)
            .expect("snapshot versions empty")
    }

    /// latest_ts has been committed to persistence and no earlier
    /// timestamps may be used for future commits, so it is safe to read from
    /// this snapshot.
    pub fn latest_ts(&self) -> RepeatableTimestamp {
        let ts = self
            .versions
            .back()
            .map(|(ts, ..)| *ts)
            .expect("snapshot versions empty");
        RepeatableTimestamp::new_validated(ts, RepeatableReason::SnapshotManagerLatest)
    }

    fn notify_waiters(&self) {
        let ts = *self.latest_ts();
        let mut waiters = self.waiters.lock();
        let mut i = 0;
        while i < waiters.len() {
            if ts > waiters[i].0 || waiters[i].1.is_closed() {
                let w = waiters.swap_remove_back(i).expect("checked above");
                let _ = w.1.send(());
                continue;
            }
            i += 1;
        }
    }

    /// Returns a future that blocks until the snapshot manager has advanced
    /// past the given timestamp.
    pub fn wait_for_higher_ts(&self, target_ts: Timestamp) -> impl Future<Output = ()> + use<> {
        // Clean up waiters that are canceled.
        self.notify_waiters();

        let receiver = if *self.latest_ts() <= target_ts {
            let (sender, receiver) = oneshot::channel();
            self.waiters.lock().push_back((target_ts, sender));
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

    /// While latest_ts has been part of some commit and the backend process is
    /// aware that it is repeatable, other processes like db-verifier
    /// may not know yet that it is safe to read from that snapshot.
    ///
    /// In contrast, persisted_max_repeatable_ts has been written as
    /// max_repeatable_ts to persistence, so every process knows it is safe
    /// to read from.
    ///
    /// Retention guarantees that min_snapshot_ts <=
    /// persisted_max_repeatable_ts.
    ///
    /// If Retention only ensured that min_snapshot_ts <= latest_ts, then
    /// we might persist some min_snapshot_ts > persisted_max_repeatable_ts.
    /// Then other processes (that don't have access to latest_ts) would not
    /// be able to read from any snapshot safely.
    pub fn persisted_max_repeatable_ts(&self) -> RepeatableTimestamp {
        self.latest_ts()
            .prior_ts(self.persisted_max_repeatable_ts)
            .expect(
                "persisted_max_repeatable_ts is always bumped after latest_ts and both are \
                 monotonic",
            )
    }

    // Note that timestamp must be within MAX_TRANSACTION_WINDOW from the latest
    // timestamp
    pub fn snapshot(&self, ts: Timestamp) -> anyhow::Result<Snapshot> {
        if ts < self.earliest_ts() {
            return Err(
                anyhow::anyhow!(ErrorMetadata::out_of_retention()).context(format!(
                    "Timestamp {ts} is too early, retry with a higher timestamp"
                )),
            );
        }
        anyhow::ensure!(
            ts <= *self.latest_ts(),
            "Timestamp {ts} is more recent than latest_ts {}",
            self.latest_ts(),
        );
        let i = match self.versions.binary_search_by_key(&ts, |(ts, ..)| *ts) {
            Ok(i) => i,
            // This is the index where insertion would preserve sorted order. That is, it's the
            // first index that has a timestamp greater than `ts`. So, we'd like the immediately
            // preceding element.
            Err(i) => i.checked_sub(1).unwrap(),
        };
        let (_, snapshot) = &self.versions[i];
        Ok(snapshot.clone())
    }

    /// Overwrites the state of the latest snapshot to include the given set of
    /// backfilled in memory vector changes.
    ///
    /// This is a bit sketchy. We want asynchronously load data that lives in
    /// the snapshot. These asynchronous loads are not data mutations, so we
    /// don't want to advance the timestamp. That leaves us with mutating
    /// the existing snapshot, which means a transaction at a given
    /// timestamp may fail initially, then succeed later when our async load
    /// completes.
    ///
    /// This pattern works as long as transactions that depend on the data that
    /// we're amending treat the failures while that data is loading as
    /// transient system errors (timeouts, database issues etc) and retry.
    pub fn overwrite_last_snapshot_text_and_vector_indexes(
        &mut self,
        text_indexes: TextIndexManager,
        vector_indexes: VectorIndexManager,
        pending_writes: &mut PendingWrites,
    ) {
        let (_ts, snapshot) = self.versions.back_mut().expect("snapshot versions empty");
        snapshot.text_indexes = text_indexes;
        snapshot.vector_indexes = vector_indexes;
        pending_writes.recompute_pending_snapshots(snapshot.clone());
    }

    pub fn overwrite_last_snapshot_table_counts(
        &mut self,
        table_summary: TableSummarySnapshot,
        pending_writes: &mut PendingWrites,
    ) -> anyhow::Result<()> {
        let (_ts, snapshot) = self.versions.back_mut().expect("snapshot versions empty");
        let table_mapping = snapshot.table_mapping();
        let table_counts = TableCounts::new(
            table_summary,
            table_mapping,
            &snapshot.virtual_system_mapping,
        )?;
        snapshot.table_counts = Some(table_counts);
        pending_writes.recompute_pending_snapshots(snapshot.clone());
        Ok(())
    }

    /// Overwrites the in-memory indexes for the latest snapshot.
    ///
    /// This is a bit sketchy but it allows us to asynchronously load indexes
    /// into memory instead of blocking startup. This model also helps us
    /// correctly layer application specific system tables that the database
    /// depend on.
    ///
    /// This pattern works since the transaction fallbacks to reading from the
    /// database if the in-memory indexes are not present. Thus they have no
    /// effect other than performance.
    pub fn overwrite_last_snapshot_in_memory_indexes(
        &mut self,
        in_memory_indexes: BackendInMemoryIndexes,
        pending_writes: &mut PendingWrites,
    ) {
        let (_ts, snapshot) = self.versions.back_mut().expect("snapshot versions empty");
        snapshot.in_memory_indexes = in_memory_indexes;
        pending_writes.recompute_pending_snapshots(snapshot.clone());
    }

    pub fn push(&mut self, ts: Timestamp, snapshot: Snapshot, write_volume: WriteVolume) {
        assert!(*self.latest_ts() < ts);
        // Note that we only drop a version if its *successor* leaves the transaction
        // window. That's because the gap between versions could be significant,
        // and we want any call to `latest_ts()` to return a timestamp that is
        // still valid for at least `MAX_TRANSACTION_WINDOW`.
        while let Some((successor_ts, _)) = self.versions.get(1)
            && (ts - *successor_ts) > *MAX_TRANSACTION_WINDOW
        {
            self.versions.pop_front();
        }
        self.versions.push_back((ts, snapshot));
        self.notify_waiters();
        self.write_throughput_limiter.record_write(ts, write_volume);
    }

    pub fn check_write_throughput_limit(&self, ts: Timestamp) -> anyhow::Result<()> {
        match self.write_throughput_limiter.exceeded_limit(ts) {
            None => Ok(()),
            Some(WriteThroughputLimit::Bytes) => Err(ErrorMetadata::rate_limited(
                "TooManyWrites",
                format!(
                    "Too many writes per second. Your deployment is limited to {} bytes written \
                     per second. Reduce your write rate or upgrade to a larger deployment.",
                    common::fmt::format_bytes(*MAX_BYTES_WRITTEN_PER_SECOND),
                ),
            )
            .into()),
            Some(WriteThroughputLimit::Rows) => Err(ErrorMetadata::rate_limited(
                "TooManyWrites",
                format!(
                    "Too many writes per second. Your deployment is limited to {} document and \
                     index rows written per second. Reduce your write rate, remove unused \
                     indexes, or upgrade to a larger deployment.",
                    MAX_ROWS_WRITTEN_PER_SECOND.unwrap_or(u64::MAX),
                ),
            )
            .into()),
        }
    }

    pub fn bump_persisted_max_repeatable_ts(&mut self, ts: Timestamp) -> anyhow::Result<bool> {
        anyhow::ensure!(
            ts >= self.persisted_max_repeatable_ts,
            "persisted_max_repeatable_ts went backward from {:?} to {:?}",
            self.persisted_max_repeatable_ts,
            ts
        );
        self.persisted_max_repeatable_ts = ts;
        let (latest_ts, snapshot) = self.latest();
        if ts > *latest_ts {
            self.push(ts, snapshot, WriteVolume::default());
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
