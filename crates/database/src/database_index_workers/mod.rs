use std::{
    cmp,
    collections::{
        BTreeMap,
        HashSet,
    },
    future::Future,
    sync::Arc,
};

use common::{
    backoff::Backoff,
    bootstrap_model::index::{
        database_index::{
            DatabaseIndexState,
            IndexedFields,
        },
        IndexConfig,
        TabletIndexMetadata,
    },
    components::{
        ComponentId,
        ComponentPath,
    },
    errors::report_error,
    execution_context::ExecutionId,
    fastrace_helpers::get_sampled_span,
    knobs::{
        INDEX_BACKFILL_CONCURRENCY,
        INDEX_WORKERS_INITIAL_BACKOFF,
        INDEX_WORKERS_MAX_BACKOFF,
    },
    persistence::{
        Persistence,
        RetentionValidator,
    },
    runtime::{
        JoinMap,
        Runtime,
    },
    types::{
        IndexId,
        PersistenceIndexId,
        RepeatableTimestamp,
        TabletIndexName,
        UdfIdentifier,
    },
    RequestId,
};
use fastrace::future::FutureExt as _;
use futures::FutureExt as _;
use hashlink::LinkedHashMap;
use keybroker::Identity;
use tokio::{
    select,
    sync::mpsc,
};
use usage_tracking::{
    CallType,
    FunctionUsageTracker,
    UsageCounter,
};
use value::{
    DeveloperDocumentId,
    ResolvedDocumentId,
    TableNamespace,
    TabletId,
};

use crate::{
    bootstrap_model::index_backfills::{
        types::BackfillCursor,
        IndexBackfillModel,
    },
    database_index_workers::index_writer::{
        IndexRateLimit,
        IndexSelector,
        IndexWriter,
        TabletBackfillProgress,
    },
    metrics::{
        log_index_backfilled,
        log_num_indexes_to_backfill,
        tablet_index_backfill_timer,
    },
    system_tables::SystemIndex,
    Database,
    IndexTable,
    SystemMetadataModel,
    Transaction,
};

pub mod index_writer;
pub struct IndexWorker<RT: Runtime> {
    /// Index IDs that are currently being backfilled.
    in_progress_index_ids: HashSet<IndexId, ahash::RandomState>,
    /// The index backfill tasks
    in_progress: JoinMap<Vec<IndexId>, anyhow::Result<u64>>,
    /// Pending index backfills in the order they will be processed. Each index
    /// is either pending once or in progress once.
    pending: LinkedHashMap<IndexId, PendingBackfill, ahash::RandomState>,
    /// Receiver for progress updates from `IndexWriter`'s tablet backfills.
    progress_rx: mpsc::Receiver<TabletBackfillProgress>,
    /// Limit on the size of `in_progress`
    max_concurrency: usize,
    metadata_mutex: Arc<tokio::sync::Mutex<()>>,
    database: Database<RT>,
    persistence: Arc<dyn Persistence>,
    index_writer: IndexWriter<RT>,
    usage_tracking: UsageCounter,
    backoff: Backoff,
    runtime: RT,
}

struct PendingBackfill {
    tablet_id: TabletId,
    cursor: Option<BackfillCursor>,
}

struct BackfillTarget {
    name: TabletIndexName,
    retention_started: bool,
    /// Captured before the index can be dropped so its snapshot can be
    /// reconciled.
    persistence_index_id: Option<PersistenceIndexId>,
}

struct BackfillRetention {
    begin_ts: RepeatableTimestamp,
    indexes: BTreeMap<IndexId, (TabletIndexName, IndexedFields)>,
}

impl<RT: Runtime> IndexWorker<RT> {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        runtime: RT,
        persistence: Arc<dyn Persistence>,
        retention_validator: Arc<dyn RetentionValidator>,
        database: Database<RT>,
        deployment_name: String,
        usage_tracking: UsageCounter,
    ) -> impl Future<Output = ()> + Send {
        let reader = persistence.reader();
        let (progress_tx, progress_rx) = mpsc::channel(100);
        let index_writer = IndexWriter::new(
            persistence.clone(),
            reader.clone(),
            retention_validator,
            runtime.clone(),
            Some(progress_tx),
            IndexRateLimit::Default,
        );
        let mut worker = IndexWorker {
            in_progress_index_ids: Default::default(),
            in_progress: JoinMap::new(),
            pending: Default::default(),
            progress_rx,
            max_concurrency: *INDEX_BACKFILL_CONCURRENCY,
            metadata_mutex: Default::default(),
            database,
            persistence,
            index_writer,
            usage_tracking,
            backoff: Backoff::new(*INDEX_WORKERS_INITIAL_BACKOFF, *INDEX_WORKERS_MAX_BACKOFF),
            runtime,
        };

        tracing::info!("Starting IndexWorker");
        async move {
            loop {
                let root = get_sampled_span(
                    &deployment_name,
                    "database_index_worker/run",
                    &mut worker.runtime.rng(),
                );

                if let Err(e) = worker.run().in_span(root).await {
                    report_error(&mut e.context("IndexWorkerLoop died")).await;
                    let delay = worker.backoff.fail(&mut worker.runtime.rng());
                    tracing::error!(
                        "IndexIndexWorker died, num_failures: {}. Backing off for {}ms",
                        worker.backoff.failures(),
                        delay.as_millis()
                    );
                    worker.runtime.wait(delay).await;
                }
            }
        }
    }

    /// Runs the index worker one loop, either backfilling an index or writing
    /// index backfill progress and returning how many rows were indexed if
    /// backfilling.
    #[fastrace::trace]
    async fn run(&mut self) -> anyhow::Result<u64> {
        // This is a counter for tests
        let mut docs_indexed = 0;
        // Get all the documents from the `_index` table.
        let mut tx = self.database.begin(Identity::system()).await?;
        // _index doesn't have `by_creation_time` index, and thus must use `by_id`.
        let index_documents = tx
            .query_system(TableNamespace::Global, &SystemIndex::<IndexTable>::by_id())?
            .all()
            .await?;
        let existing_index_ids: HashSet<IndexId, ahash::RandomState> = index_documents
            .iter()
            .map(|index_metadata| index_metadata.id().internal_id().into())
            .collect();
        let mut num_to_backfill = 0;
        let backfill_metadata = IndexBackfillModel::new(&mut tx)
            .all_backfill_metadata()
            .await?;
        for index_metadata in &index_documents {
            if let IndexConfig::Database { on_disk_state, .. } = &index_metadata.config
                && matches!(on_disk_state, DatabaseIndexState::Backfilling(_))
            {
                let backfill_cursor = backfill_metadata
                    .get(&index_metadata.id().internal_id().into())
                    .and_then(|metadata| metadata.cursor.clone());
                let index_id = index_metadata.id().internal_id().into();
                let tablet_id = *index_metadata.name.table();
                if !self.in_progress_index_ids.contains(&index_id) {
                    // Preserve queue order when updating an index's cursor.
                    self.pending.replace(
                        index_id,
                        PendingBackfill {
                            tablet_id,
                            cursor: backfill_cursor,
                        },
                    );
                }
                num_to_backfill += 1;
            }
        }
        log_num_indexes_to_backfill(num_to_backfill);
        tracing::debug!(
            "{num_to_backfill} database indexes to backfill @ {}",
            tx.begin_timestamp()
        );

        // Cancel backfills for indexes that no longer exist
        self.pending
            .retain(|index_id, PendingBackfill { tablet_id, .. }| {
                let exists = existing_index_ids.contains(index_id);
                if !exists {
                    tracing::info!(
                        "Dropping pending backfill of dropped index {index_id} on tablet \
                         {tablet_id}"
                    );
                }
                exists
            });
        self.in_progress.abort_matching(|index_ids| {
            let abort = !index_ids
                .iter()
                .any(|index_id| existing_index_ids.contains(index_id));
            if abort {
                tracing::info!("Cancelling backfill of dropped indexes {index_ids:?}");
            }
            abort
        });

        let token = tx.into_token()?;

        // Start new work if allowed by the concurrency limit
        while self.in_progress.len() < self.max_concurrency
            && let Some((index_id, PendingBackfill { tablet_id, cursor })) =
                self.pending.pop_front()
        {
            tracing::info!(
                "Queuing index backfill for index ID {index_id} and tablet ID {tablet_id}"
            );
            self.queue_index_backfill(index_id, tablet_id, cursor);
        }
        let subscription_fut = self.database.subscribe_and_wait_for_invalidation(token);
        select! {
            biased;
            // Start by finding indexes that have finished backfilling
            res = self.in_progress.join_next(), if !self.in_progress.is_empty() => {
                let (index_ids, res) = res.expect("join_next cannot return None if nonempty");
                // First, make sure `in_progress_index_ids` is always consistent with `in_progress`
                for &index_id in &index_ids {
                    self.in_progress_index_ids.remove(&index_id);
                }
                match res {
                    Err(e) if e.is_cancelled() => {
                        tracing::info!("Cancelled backfilling {index_ids:?}");
                    },
                    res => {
                        // If backfill tasks are failing, return an error here so that we back off
                        docs_indexed += res??;
                        tracing::info!("Finished backfilling {index_ids:?}");
                    },
                }
                // Return so that we possibly queue up more work
            }
            maybe_progress = self.progress_rx.recv() => {
                let Some(
                    TabletBackfillProgress {
                        tablet_id,
                        index_ids,
                        cursor,
                        num_docs_indexed,
                        backfill_bytes_read,
                        backfill_bytes_written,
                    }) = maybe_progress else {
                    anyhow::bail!("Database index backfill progress channel closed");
                };
                let mut tx = self.database.begin_system().await?;
                let table_mapping = tx.table_mapping();
                let table_number = table_mapping.tablet_number(tablet_id)?;
                let table_name = table_mapping.tablet_name(tablet_id)?;
                let table_namespace = table_mapping.tablet_namespace(tablet_id)?;
                let component_id = ComponentId::from(table_namespace);
                let component_path = tx
                    .get_component_path(component_id)
                    .unwrap_or(ComponentPath::root());
                let mut model = IndexBackfillModel::new(&mut tx);
                let cursor = ResolvedDocumentId::new(
                    tablet_id,
                    DeveloperDocumentId::new(table_number, cursor.internal_id())
                );
                tracing::info!(
                    "IndexWorker got progress update for {} indexes in tablet {tablet_id} at cursor {}",
                    index_ids.len(),
                    cursor.developer_id,
                );
                for index_id in index_ids {
                    let Some(existing_backfill_metadata) =
                        model.existing_backfill_metadata_for_index(index_id).await?
                    else {
                        // The index was dropped while its backfill was running.
                        tracing::info!("Skipping backfill progress for dropped index {index_id}");
                        continue;
                    };
                    model
                        .update_database_index_backfill_progress(
                            &existing_backfill_metadata,
                            tablet_id,
                            num_docs_indexed,
                            cursor
                        )
                        .await?;
                }
                self.database.commit_with_write_source(tx, "index_worker_backfill_progress")
                    .await?;
                if backfill_bytes_written > 0 || backfill_bytes_read > 0 {
                    let usage = FunctionUsageTracker::new();
                    if backfill_bytes_read > 0 {
                        usage.track_database_egress_v2(
                            component_path.clone(),
                            &table_name,
                            backfill_bytes_read,
                            table_name.is_system(),
                        );
                    }
                    if backfill_bytes_written > 0 {
                        usage.track_database_ingress_v2(
                            component_path,
                            &table_name,
                            backfill_bytes_written,
                            table_name.is_system(),
                        );
                    }
                    self.usage_tracking
                        .track_call(
                            UdfIdentifier::SystemJob("database_index_backfill".to_string()),
                            ExecutionId::new(),
                            RequestId::new(),
                            CallType::IndexBackfill,
                            true,
                            usage.gather_user_stats(),
                        )
                        .await;
                }
            }
            // Alternatively, wait for invalidation
            ts = subscription_fut.fuse() => {
                tracing::debug!("Index backfill worker got invalidation at {ts:?}");
                self.backoff.reset();
            }
        }

        Ok(docs_indexed)
    }

    /// Spawns a task to process the next index backfill.
    #[fastrace::trace]
    fn queue_index_backfill(
        &mut self,
        index_id: IndexId,
        tablet_id: TabletId,
        backfill_cursor: Option<BackfillCursor>,
    ) {
        let mut index_ids = vec![index_id];
        // Since we're mainly limited by the speed of reading the table, let's
        // grab all the other pending indexes for this table at once
        self.pending.retain(|&other_index_id, other| {
            if other.tablet_id == tablet_id && other.cursor == backfill_cursor {
                index_ids.push(other_index_id);
                false
            } else {
                true
            }
        });

        for &index_id in &index_ids {
            self.in_progress_index_ids.insert(index_id);
        }
        self.in_progress.spawn(
            "backfill_tablet",
            index_ids.clone(),
            Self::backfill_tablet(
                tablet_id,
                index_ids,
                self.database.clone(),
                self.persistence.clone(),
                self.index_writer.clone(),
                self.metadata_mutex.clone(),
                backfill_cursor,
            ),
        );
    }

    /// Completes snapshot writes and reconciliation before setting
    /// `retention_started`. The metadata commit orders all `Scanning` writes
    /// before subsequent `ScanComplete` writes. Retention completes before the
    /// indexes are published. Each phase is idempotent.
    async fn backfill_tablet(
        tablet_id: TabletId,
        index_ids: Vec<IndexId>,
        database: Database<RT>,
        persistence: Arc<dyn Persistence>,
        index_writer: IndexWriter<RT>,
        metadata_mutex: Arc<tokio::sync::Mutex<()>>,
        backfill_cursor: Option<BackfillCursor>,
    ) -> anyhow::Result<u64> {
        let mut docs_indexed = 0;
        let _timer = tablet_index_backfill_timer();
        let mut backfills = BTreeMap::new();
        for index_id in &index_ids {
            let Some(target) = Self::begin_backfill(*index_id, &database).await? else {
                tracing::info!("Skipping backfill of index {index_id:?} that no longer exists");
                continue;
            };
            backfills.insert(*index_id, target);
        }
        let existing_index_ids: Vec<IndexId> = backfills.keys().copied().collect();

        let needs_backfill = backfills
            .iter()
            .filter(|(_, target)| !target.retention_started)
            .map(|(index_id, target)| (*index_id, target.name.clone()))
            .collect::<BTreeMap<_, _>>();

        if !needs_backfill.is_empty() {
            let mut tx = database.begin_system().await?;
            let ts = tx.begin_timestamp();
            let snapshot = database.snapshot(ts)?;
            let table_name = tx.table_mapping().tablet_name(tablet_id)?;
            let cursor = backfill_cursor
                .and_then(|backfill_cursor| backfill_cursor.cursor)
                .map(|cursor| ResolvedDocumentId::new(tablet_id, cursor));
            if let Some(cursor) = cursor {
                tracing::info!(
                    "Resuming backfill of {} indexes for {table_name} after {} at ts {ts}: \
                     {needs_backfill:?}",
                    needs_backfill.len(),
                    cursor.developer_id,
                );
                let mut index_backfill_model = IndexBackfillModel::new(&mut tx);
                for index_id in needs_backfill.keys() {
                    index_backfill_model
                        .resume_database_index_backfill(*index_id, *ts)
                        .await?;
                }
            } else {
                tracing::info!(
                    "Starting backfill of {} indexes for {table_name}: {needs_backfill:?}",
                    needs_backfill.len(),
                );
                let table_namespace = tx.table_mapping().tablet_namespace(tablet_id)?;
                let total_docs = snapshot
                    .table_count(table_namespace, &table_name)
                    .map(|count| count.num_values());
                let mut index_backfill_model = IndexBackfillModel::new(&mut tx);
                for index_id in needs_backfill.keys() {
                    index_backfill_model
                        .initialize_database_index_backfill(*index_id, total_docs, *ts)
                        .await?;
                }
            }
            database
                .commit_with_write_source(tx, "index_worker_backfill_initialization")
                .await?;
            let index_registry = snapshot.index_registry;
            let index_selector = IndexSelector::ManyIndexes {
                tablet_id,
                indexes: needs_backfill,
            };
            docs_indexed = index_writer
                .backfill_from_ts(ts, &index_registry, index_selector, 1, cursor, None)
                .await?;
            for index in backfills
                .values()
                .filter(|target| !target.retention_started)
                .filter_map(|target| target.persistence_index_id)
            {
                persistence.reconcile_index_backfill(index).await?;
            }
        }

        if let Some(retention) =
            Self::mark_retention_started(&database, &metadata_mutex, &existing_index_ids).await?
        {
            tracing::info!(
                "Started running retention for {} indexes",
                retention.indexes.len()
            );
            index_writer
                .run_retention(retention.begin_ts, retention.indexes)
                .await?;
        }

        let indexes_lock = metadata_mutex.lock().await;
        let mut tx = database.begin(Identity::system()).await?;
        for index_id in existing_index_ids {
            Self::finish_backfill(&mut tx, index_id).await?;
        }
        database
            .commit_with_write_source(tx, "index_worker_finish_backfill")
            .await?;
        drop(indexes_lock);

        Ok(docs_indexed)
    }

    /// Commits the boundary between `Scanning` and `ScanComplete` writes.
    async fn mark_retention_started(
        database: &Database<RT>,
        metadata_mutex: &tokio::sync::Mutex<()>,
        existing_index_ids: &[IndexId],
    ) -> anyhow::Result<Option<BackfillRetention>> {
        let mut min_begin_ts = None;
        let mut retention = BTreeMap::new();
        // The database currently does not allow concurrent writers to the
        // `_index` (or `_tables`) tables; see a TODO in
        // `Writes::record_reads_for_write`.
        // Since we run many `backfill_tablet` tasks concurrently, synchronize
        // here to avoid creating OCC conflicts with ourselves.
        let indexes_lock = metadata_mutex.lock().await;
        let mut tx = database.begin(Identity::system()).await?;
        for index_id in existing_index_ids {
            let Some((backfill_begin_ts, index_name, indexed_fields)) =
                Self::set_retention_started(&mut tx, *index_id).await?
            else {
                tracing::info!("Skipping retention for index {index_id:?} that no longer exists");
                continue;
            };

            min_begin_ts = min_begin_ts
                .map(|t| cmp::min(t, backfill_begin_ts))
                .or(Some(backfill_begin_ts));

            retention.insert(*index_id, (index_name, indexed_fields));
        }
        database
            .commit_with_write_source(tx, "index_worker_start_retention")
            .await?;
        drop(indexes_lock);
        Ok(min_begin_ts.map(|begin_ts| BackfillRetention {
            begin_ts,
            indexes: retention,
        }))
    }

    async fn begin_backfill(
        index_id: IndexId,
        database: &Database<RT>,
    ) -> anyhow::Result<Option<BackfillTarget>> {
        let mut tx = database.begin(Identity::system()).await?;
        let index_table_id = tx.bootstrap_tables().index_id;

        // Observing `Backfilling` at this transaction's timestamp ensures that
        // later document commits write the index with `IndexWriteMode::Scanning`.
        let Some(index_doc) = tx
            .get(ResolvedDocumentId::new(
                index_table_id.tablet_id,
                DeveloperDocumentId::new(index_table_id.table_number, index_id.0),
            ))
            .await?
        else {
            return Ok(None);
        };
        let index_metadata = TabletIndexMetadata::from_document(index_doc)?;

        // Assuming that the IndexWorker is the only writer of index state, we expect
        // the state to still be `Backfilling` here. If this assertion fails, we
        // somehow raced with another `IndexWorker`(!) or don't actually have the
        // database lease (!).
        let (retention_started, persistence_id) = match &index_metadata.config {
            IndexConfig::Database {
                on_disk_state,
                persistence_index_id,
                ..
            } => {
                let DatabaseIndexState::Backfilling(state) = on_disk_state else {
                    anyhow::bail!(
                        "IndexWorker started backfilling index {index_metadata:?} not in \
                         Backfilling state"
                    );
                };
                (state.retention_started, *persistence_index_id)
            },
            _ => anyhow::bail!(
                "IndexWorker attempted to backfill an index {index_metadata:?} which wasn't a \
                 database index."
            ),
        };

        Ok(Some(BackfillTarget {
            name: index_metadata.name.clone(),
            retention_started,
            persistence_index_id: persistence_id,
        }))
    }

    async fn set_retention_started(
        tx: &mut Transaction<RT>,
        index_id: IndexId,
    ) -> anyhow::Result<Option<(RepeatableTimestamp, TabletIndexName, IndexedFields)>> {
        let index_table_id = tx.bootstrap_tables().index_id;

        let Some(index_doc) = tx
            .get(ResolvedDocumentId::new(
                index_table_id.tablet_id,
                DeveloperDocumentId::new(index_table_id.table_number, index_id.0),
            ))
            .await?
        else {
            return Ok(None);
        };
        let mut index_metadata = TabletIndexMetadata::from_document(index_doc)?;

        // Assuming that the IndexWorker is the only writer of index state, we expect
        // the state to still be `Backfilling` here. If this assertion fails, we
        // somehow raced with another `IndexWorker`(!) or don't actually have the
        // database lease (!).
        let (index_ts, indexed_fields) = match &mut index_metadata.config {
            IndexConfig::Database {
                on_disk_state,
                spec,
                persistence_index_id: _,
            } => {
                let DatabaseIndexState::Backfilling(state) = on_disk_state else {
                    anyhow::bail!(
                        "IndexWorker started backfilling index {index_metadata:?} not in \
                         Backfilling state"
                    )
                };

                state.retention_started = true;
                (
                    tx.begin_timestamp()
                        .prior_ts(state.index_created_lower_bound)?,
                    spec.fields.clone(),
                )
            },
            _ => anyhow::bail!(
                "IndexWorker attempted to backfill an index {index_metadata:?} which wasn't a \
                 database index."
            ),
        };

        let name = index_metadata.name.clone();
        SystemMetadataModel::new_global(tx)
            .replace(index_metadata.id(), index_metadata.into_value().try_into()?)
            .await?;

        Ok(Some((index_ts, name, indexed_fields)))
    }

    async fn finish_backfill(tx: &mut Transaction<RT>, index_id: IndexId) -> anyhow::Result<()> {
        // Now that we're done, write that we've finished backfilling the index, sanity
        // checking that it wasn't written concurrently with our backfill.
        let index_table_id = tx.bootstrap_tables().index_id;
        let full_index_id = ResolvedDocumentId::new(
            index_table_id.tablet_id,
            DeveloperDocumentId::new(index_table_id.table_number, index_id.0),
        );
        let Some(index_doc) = tx.get(full_index_id).await? else {
            tracing::info!("Skipping finish of index {index_id:?} that no longer exists");
            return Ok(());
        };
        let mut index_metadata = TabletIndexMetadata::from_document(index_doc)?;
        let is_system_index_on_user_table = index_metadata.name.descriptor().is_reserved();
        let is_index_on_system_table = tx
            .table_mapping()
            .is_system_tablet(*index_metadata.name.table());
        match index_metadata.config {
            IndexConfig::Database {
                ref mut on_disk_state,
                ..
            } => {
                let DatabaseIndexState::Backfilling(ref backfilling_state) = *on_disk_state else {
                    anyhow::bail!(
                        "IndexWorker finished backfilling index {index_metadata:?} not in \
                         Backfilling state"
                    );
                };
                *on_disk_state = if is_system_index_on_user_table || is_index_on_system_table {
                    DatabaseIndexState::Enabled
                } else {
                    DatabaseIndexState::Backfilled {
                        staged: backfilling_state.staged,
                    }
                };
            },
            _ => anyhow::bail!(
                "IndexWorker finished backfilling index {index_metadata:?} which wasn't a \
                 database index"
            ),
        };

        let name = index_metadata.name.clone();

        SystemMetadataModel::new_global(tx)
            .replace(full_index_id, index_metadata.into_value().try_into()?)
            .await?;
        let table_name = tx.table_mapping().tablet_name(*name.table())?;
        tracing::info!("Finished backfill of index {}", name);
        if is_index_on_system_table || is_system_index_on_user_table {
            tracing::info!(
                "Finished backfill of system index {table_name}.{}",
                name.descriptor()
            );
        }
        log_index_backfilled();
        Ok(())
    }
}
