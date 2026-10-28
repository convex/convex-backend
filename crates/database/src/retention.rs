//! Retention deletes old versions of data that can no longer be accessed.
use std::{
    cmp::{
        self,
    },
    collections::{
        hash_map::DefaultHasher,
        BTreeMap,
        BTreeSet,
    },
    hash::{
        Hash,
        Hasher,
    },
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use anyhow::Context;
use async_trait::async_trait;
use common::{
    backoff::Backoff,
    bootstrap_model::{
        index::{
            database_index::{
                DatabaseIndexState,
                IndexedFields,
            },
            IndexConfig,
            IndexMetadata,
        },
        tables::{
            TableMetadata,
            TableState,
        },
    },
    document::{
        ParseDocument,
        ParsedDocument,
        ResolvedDocument,
    },
    errors::{
        lease_lost_error,
        report_error,
        LeaseLostError,
    },
    fastrace_helpers::get_sampled_span,
    interval::Interval,
    knobs::{
        DEFAULT_DOCUMENTS_PAGE_SIZE,
        DELETE_TABLET_CHUNK_SIZE,
        DOCUMENT_RETENTION_BATCH_INTERVAL_SECONDS,
        DOCUMENT_RETENTION_DELAY,
        DOCUMENT_RETENTION_DELETE_CHUNK,
        DOCUMENT_RETENTION_DELETE_PARALLEL,
        DOCUMENT_RETENTION_MAX_SCANNED_DOCUMENTS,
        INDEX_BACKFILL_MARKER_CLEANUP_BUSY_INTERVAL_SECONDS,
        INDEX_BACKFILL_MARKER_CLEANUP_IDLE_INTERVAL_SECONDS,
        INDEX_RETENTION_DELAY,
        MAX_RETENTION_DELAY_SECONDS,
        RETENTION_CHECKPOINT_PERIOD_SECS,
    },
    persistence::{
        new_static_repeatable_recent,
        DocumentLogEntry,
        IndexRetentionProgress,
        IndexRetentionRequest,
        NoopRetentionValidator,
        Persistence,
        PersistenceGlobalKey,
        PersistenceReader,
        RetentionValidator,
        TimestampRange,
    },
    query::Order,
    runtime::{
        new_rate_limiter,
        shutdown_and_join,
        RateLimiter,
        Runtime,
        SpawnHandle,
    },
    shutdown::ShutdownSignal,
    sync::split_rw_lock::{
        new_split_rw_lock,
        Reader,
        Writer,
    },
    try_anyhow,
    try_chunks::TryChunksExt,
    types::{
        GenericIndexName,
        IndexId,
        IndexWriteMode,
        PersistenceIndexId,
        RepeatableReason,
        RepeatableTimestamp,
        Timestamp,
    },
    value::{
        ConvexValue,
        TabletId,
    },
};
use errors::ErrorMetadata;
use fastrace::future::FutureExt as _;
use futures::{
    future::try_join_all,
    pin_mut,
    TryStreamExt,
};
use futures_async_stream::try_stream;
use governor::{
    InsufficientCapacity,
    Jitter,
    Quota,
};
use parking_lot::Mutex;
use tokio::{
    sync::{
        mpsc,
        watch::{
            self,
            Receiver,
            Sender,
        },
    },
    time::MissedTickBehavior,
};
use value::InternalDocumentId;

use crate::{
    metrics::{
        index_backfill_marker_cleanup_timer,
        index_retention_delete_timer,
        latest_min_document_snapshot_timer,
        latest_min_snapshot_timer,
        log_deleted_tablet_documents_deleted,
        log_document_retention_cursor_age,
        log_document_retention_cursor_lag,
        log_document_retention_no_cursor,
        log_document_retention_scanned_document,
        log_index_backfill_markers_deleted,
        log_index_retention_cursor_age,
        log_index_retention_cursor_lag,
        log_index_retention_no_cursor,
        log_retention_documents_deleted,
        log_retention_index_entries_deleted,
        log_retention_ts_advanced,
        log_retention_unique_indexes,
        log_snapshot_verification_age,
        retention_advance_timestamp_timer,
        retention_delete_document_chunk_timer,
        retention_delete_documents_timer,
    },
    snapshot_manager::SnapshotManager,
    BootstrapMetadata,
    Snapshot,
};

#[derive(Debug, Clone, Copy)]
pub enum RetentionType {
    Document,
    Index,
}

/// Which caller drove index retention, recorded as a metric label so the
/// catch-up deletes an index backfill runs can be told apart from the ongoing
/// background retention worker.
#[derive(Debug, Clone, Copy, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum IndexRetentionSource {
    /// The ongoing background `go_delete_indexes` worker.
    Live,
    /// Catch-up deletes run when an index backfill finishes.
    Backfill,
}

#[derive(Clone)]
pub struct SnapshotBounds {
    /// min_snapshot_ts is the earliest snapshot at which we are guaranteed
    /// to not have deleted data.
    min_index_snapshot_ts: RepeatableTimestamp,

    /// min_document_snapshot_ts is the earliest snapshot at which we are
    /// guaranteed to not have deleted views of data in the write-ahead log.
    min_document_snapshot_ts: RepeatableTimestamp,
}

impl SnapshotBounds {
    fn advance_min_snapshot_ts(&mut self, candidate: RepeatableTimestamp) {
        self.min_index_snapshot_ts = cmp::max(self.min_index_snapshot_ts, candidate);
    }

    fn advance_min_document_snapshot_ts(&mut self, candidate: RepeatableTimestamp) {
        self.min_document_snapshot_ts = cmp::max(self.min_document_snapshot_ts, candidate);
    }
}

pub struct Checkpoint {
    checkpoint: Option<RepeatableTimestamp>,
}

impl Checkpoint {
    fn advance_checkpoint(&mut self, candidate: RepeatableTimestamp) {
        match self.checkpoint {
            Some(ref mut checkpoint) => {
                *checkpoint = cmp::max(*checkpoint, candidate);
            },
            None => {
                self.checkpoint = Some(candidate);
            },
        }
    }
}

/// Implements [`RetentionValidator`] for a process that holds the lease.
pub struct LeaderRetentionManager<RT: Runtime> {
    rt: RT,
    bounds_reader: Reader<SnapshotBounds>,
    checkpoint_reader: Reader<Checkpoint>,
    #[allow(unused)]
    document_checkpoint_reader: Reader<Checkpoint>,
}

/// Owns the write halves of the readers in [`LeaderRetentionManager`].
/// Call [Self::start_retention] to start advancing retention and deleting
/// expired documents and index rows.
pub(crate) struct LeaderRetentionWorkerSeed<RT: Runtime> {
    retention_manager: Arc<LeaderRetentionManager<RT>>,
    persistence: Arc<dyn Persistence>,
    bounds_writer: Writer<SnapshotBounds>,
    checkpoint_writer: Writer<Checkpoint>,
    document_checkpoint_writer: Writer<Checkpoint>,
}

#[derive(Clone)]
pub struct LeaderRetentionWorkers {
    handles: Arc<Mutex<Vec<Box<dyn SpawnHandle>>>>,
}

pub async fn latest_retention_min_snapshot_ts(
    persistence: &dyn PersistenceReader,
    retention_type: RetentionType,
) -> anyhow::Result<RepeatableTimestamp> {
    let _timer = match retention_type {
        RetentionType::Document => latest_min_document_snapshot_timer(),
        RetentionType::Index => latest_min_snapshot_timer(),
    };
    let key = match retention_type {
        RetentionType::Document => PersistenceGlobalKey::DocumentRetentionMinSnapshotTimestamp,
        RetentionType::Index => PersistenceGlobalKey::IndexRetentionMinSnapshotTimestamp,
    };
    let min_snapshot_value = persistence
        .get_persistence_global(key)
        .await?
        .map(ConvexValue::try_from)
        .transpose()?;
    let min_snapshot_ts = match min_snapshot_value {
        Some(ConvexValue::Int64(ts)) => Timestamp::try_from(ts)?,
        None => Timestamp::MIN,
        _ => anyhow::bail!("invalid retention snapshot {min_snapshot_value:?}"),
    };
    // We have the invariant:
    //   min_document_snapshot_ts <= min_index_snapshot_ts <= max_repeatable_ts
    // and the latter ts is repeatable by definition, so min_snapshot_ts is
    // always repeatable.
    Ok(RepeatableTimestamp::new_validated(
        min_snapshot_ts,
        RepeatableReason::MinSnapshotTsPersistence,
    ))
}

const INITIAL_BACKOFF: Duration = Duration::from_millis(50);

impl<RT: Runtime> LeaderRetentionManager<RT> {
    /// Loads the current snapshot bounds from `persistence` and returns a
    /// [`LeaderRetentionManager`] that can be used right away as a
    /// [`RetentionValidator`].
    pub(crate) async fn new(
        rt: RT,
        persistence: Arc<dyn Persistence>,
        repeatable_ts: RepeatableTimestamp,
    ) -> anyhow::Result<(
        Arc<LeaderRetentionManager<RT>>,
        LeaderRetentionWorkerSeed<RT>,
    )> {
        let bounds = load_snapshot_bounds(persistence.reader().as_ref(), repeatable_ts).await?;
        let (bounds_reader, bounds_writer) = new_split_rw_lock(bounds);
        let checkpoint = Checkpoint { checkpoint: None };
        let document_checkpoint = Checkpoint { checkpoint: None };
        let (checkpoint_reader, checkpoint_writer) = new_split_rw_lock(checkpoint);
        let (document_checkpoint_reader, document_checkpoint_writer) =
            new_split_rw_lock(document_checkpoint);

        let this = Arc::new(Self {
            rt,
            bounds_reader,
            checkpoint_reader,
            document_checkpoint_reader,
        });

        Ok((
            this.clone(),
            LeaderRetentionWorkerSeed {
                retention_manager: this,
                persistence,
                bounds_writer,
                checkpoint_writer,
                document_checkpoint_writer,
            },
        ))
    }
}

impl<RT: Runtime> LeaderRetentionWorkerSeed<RT> {
    pub(crate) async fn start_retention(
        self,
        bootstrap_metadata: BootstrapMetadata,
        snapshot_reader: Reader<SnapshotManager>,
        lease_lost_shutdown: ShutdownSignal,
        retention_rate_limiter: Arc<RateLimiter<RT>>,
        deleted_tablet_sender: mpsc::Sender<TabletId>,
    ) -> anyhow::Result<LeaderRetentionWorkers> {
        let rt = &self.retention_manager.rt;
        let reader = self.persistence.reader();
        let needs_index_retention_deletes = reader.needs_index_retention_deletes();
        let SnapshotBounds {
            min_index_snapshot_ts,
            min_document_snapshot_ts,
        } = *self.bounds_writer.read();
        let index_table_id = bootstrap_metadata.index_tablet_id;

        let (send_min_index_snapshot, receive_min_index_snapshot) =
            watch::channel(min_index_snapshot_ts);
        let (send_min_document_snapshot, receive_min_document_snapshot) =
            watch::channel(min_document_snapshot_ts);
        let checkpoint_quota = Quota::with_period(*RETENTION_CHECKPOINT_PERIOD_SECS)
            .context("Checkpoint period cannot be zero")?;

        let index_deletion_handle = if needs_index_retention_deletes {
            // Scan _index.by_id before advancing retention so the deleter includes
            // every index that can still be queried.
            let mut all_indexes = {
                let mut meta_index_scan = reader.index_scan(
                    bootstrap_metadata.index_by_id,
                    bootstrap_metadata.index_tablet_id,
                    *min_index_snapshot_ts,
                    &Interval::all(),
                    Order::Asc,
                    usize::MAX,
                    self.retention_manager.clone(),
                );
                let mut indexes = BTreeMap::new();
                while let Some((_, rev)) = meta_index_scan.try_next().await? {
                    LeaderRetentionWorkers::accumulate_index_document(rev.value, &mut indexes)?;
                }
                indexes
            };

            let mut index_cursor = min_index_snapshot_ts;
            // Include indexes created since the snapshot before document retention
            // advances.
            let latest_ts = snapshot_reader.lock().latest_ts();
            LeaderRetentionWorkers::accumulate_indexes(
                self.persistence.as_ref(),
                &mut all_indexes,
                &mut index_cursor,
                latest_ts,
                index_table_id,
                self.retention_manager.clone(),
            )
            .await?;

            let index_deletion_cursor = LeaderRetentionWorkers::get_checkpoint(
                reader.as_ref(),
                snapshot_reader.clone(),
                RetentionType::Index,
            )
            .await?;

            Some(rt.spawn(
                "retention_delete",
                LeaderRetentionWorkers::go_delete_indexes(
                    self.retention_manager.bounds_reader.clone(),
                    rt.clone(),
                    self.persistence.clone(),
                    all_indexes,
                    index_table_id,
                    index_cursor,
                    self.retention_manager.clone(),
                    receive_min_index_snapshot,
                    self.checkpoint_writer,
                    snapshot_reader.clone(),
                    index_deletion_cursor,
                    checkpoint_quota,
                ),
            ))
        } else {
            None
        };
        let document_deletion_cursor = LeaderRetentionWorkers::get_checkpoint(
            reader.as_ref(),
            snapshot_reader.clone(),
            RetentionType::Document,
        )
        .await?;

        let document_deletion_handle = rt.spawn(
            "document_retention_delete",
            LeaderRetentionWorkers::go_delete_documents(
                self.retention_manager.bounds_reader.clone(),
                rt.clone(),
                self.persistence.clone(),
                receive_min_document_snapshot.clone(),
                self.document_checkpoint_writer,
                snapshot_reader.clone(),
                retention_rate_limiter.clone(),
                document_deletion_cursor,
                checkpoint_quota,
            ),
        );

        let snapshot = snapshot_reader.lock().latest_snapshot();
        let tablets_to_delete = LeaderRetentionWorkers::tablets_to_delete(
            snapshot,
            bootstrap_metadata.tables_by_id.id(),
        )?;

        let tablet_deletion_handle = rt.spawn(
            "retention_tablet_deletion",
            LeaderRetentionWorkers::go_delete_table_documents(
                self.retention_manager.bounds_reader.clone(),
                rt.clone(),
                self.persistence.clone(),
                receive_min_document_snapshot,
                retention_rate_limiter.clone(),
                tablets_to_delete,
                deleted_tablet_sender,
            ),
        );
        let advance_min_snapshot_handle = rt.spawn(
            "retention_advance_min_snapshot",
            LeaderRetentionWorkers::go_advance_min_snapshot(
                self.bounds_writer,
                self.retention_manager.checkpoint_reader.clone(),
                rt.clone(),
                self.persistence.clone(),
                send_min_index_snapshot,
                send_min_document_snapshot,
                snapshot_reader.clone(),
                lease_lost_shutdown.clone(),
            ),
        );
        let index_backfill_marker_cleanup_handle = rt.spawn(
            "index_backfill_marker_cleanup",
            LeaderRetentionWorkers::go_delete_index_backfill_markers(
                rt.clone(),
                self.persistence,
                snapshot_reader,
            ),
        );
        // Shut down receivers before senders so channel closure cannot stop a worker
        // early.
        let mut handles: Vec<Box<dyn SpawnHandle>> = vec![index_backfill_marker_cleanup_handle];
        handles.extend(index_deletion_handle);
        handles.push(document_deletion_handle);
        handles.push(tablet_deletion_handle);
        handles.push(advance_min_snapshot_handle);
        Ok(LeaderRetentionWorkers {
            handles: Arc::new(Mutex::new(handles)),
        })
    }
}

impl LeaderRetentionWorkers {
    pub(crate) async fn shutdown(&self) -> anyhow::Result<()> {
        let handles: Vec<_> = self.handles.lock().drain(..).collect();
        for handle in handles.into_iter() {
            shutdown_and_join(handle).await?;
        }
        Ok(())
    }

    pub(crate) async fn delete_index_backfill_markers_once(
        persistence: &dyn Persistence,
        snapshot_reader: &Reader<SnapshotManager>,
    ) -> anyhow::Result<bool> {
        let timer = index_backfill_marker_cleanup_timer();
        // Read marker index IDs before the snapshot: with this ordering, an ID
        // absent from the registry belongs to a dropped index. Persistence index
        // IDs are never reused.
        let marker_indexes = persistence.index_backfill_marker_indexes().await?;
        if marker_indexes.is_empty() {
            timer.finish();
            return Ok(false);
        }
        let scanning_indexes: BTreeSet<PersistenceIndexId> = snapshot_reader
            .lock()
            .latest_snapshot()
            .index_registry
            .all_indexes()
            .filter_map(|index| match &index.config {
                IndexConfig::Database {
                    on_disk_state,
                    persistence_index_id,
                    ..
                } if on_disk_state.write_mode() == IndexWriteMode::Scanning => {
                    *persistence_index_id
                },
                IndexConfig::Database { .. }
                | IndexConfig::Text { .. }
                | IndexConfig::Vector { .. } => None,
            })
            .collect();
        let indexes_with_obsolete_markers: Vec<_> = marker_indexes
            .into_iter()
            .filter(|index| !scanning_indexes.contains(index))
            .collect();
        let mut deleted = 0;
        // One chunk per index lets each pass make progress on every eligible
        // index, even when one index has many markers.
        for index in &indexes_with_obsolete_markers {
            deleted += persistence
                .delete_index_backfill_markers_chunk(*index)
                .await?;
        }
        if deleted > 0 {
            log_index_backfill_markers_deleted(deleted);
            tracing::debug!(
                "Deleted {deleted} index backfill markers for {} indexes",
                indexes_with_obsolete_markers.len()
            );
        }
        timer.finish();
        Ok(deleted > 0)
    }

    async fn go_delete_index_backfill_markers<RT: Runtime>(
        rt: RT,
        persistence: Arc<dyn Persistence>,
        snapshot_reader: Reader<SnapshotManager>,
    ) {
        let mut error_backoff = Backoff::new(INITIAL_BACKOFF, *MAX_RETENTION_DELAY_SECONDS);
        let mut delay = *INDEX_BACKFILL_MARKER_CLEANUP_IDLE_INTERVAL_SECONDS;
        loop {
            Self::wait_with_jitter(&rt, delay).await;
            delay = match Self::delete_index_backfill_markers_once(
                persistence.as_ref(),
                &snapshot_reader,
            )
            .await
            {
                Ok(deleted) => {
                    error_backoff.reset();
                    if deleted {
                        *INDEX_BACKFILL_MARKER_CLEANUP_BUSY_INTERVAL_SECONDS
                    } else {
                        *INDEX_BACKFILL_MARKER_CLEANUP_IDLE_INTERVAL_SECONDS
                    }
                },
                Err(mut error) => {
                    report_error(&mut error).await;
                    let delay = error_backoff.fail(&mut rt.rng());
                    tracing::debug!(
                        "index backfill marker cleanup failed, delaying {delay:?}: {error:?}"
                    );
                    delay
                },
            };
        }
    }

    /// Returns a list of tablets in `Deleting` state and the timestamp at which
    /// they were marked `Deleting`
    fn tablets_to_delete(
        snapshot: Snapshot,
        tables_by_id: IndexId,
    ) -> anyhow::Result<BTreeMap<TabletId, Timestamp>> {
        let range_docs = snapshot
            .in_memory_indexes
            .range(tables_by_id, &Interval::all(), Order::Asc)?
            .context("Expected to find `_tables.by_id` in in-memory index")?;
        let mut tables = BTreeMap::new();
        for (_, ts, doc) in range_docs {
            let table_metadata = doc.force::<TableMetadata>()?;
            if matches!(table_metadata.state, TableState::Deleting) {
                tables.insert(TabletId(table_metadata.id().internal_id()), ts);
            }
        }
        Ok(tables)
    }

    /// Returns the timestamp which we would like to use as min_snapshot_ts.
    /// This timestamp is created relative to the `max_repeatable_ts`.
    async fn candidate_min_snapshot_ts(
        snapshot_reader: &Reader<SnapshotManager>,
        checkpoint_reader: &Reader<Checkpoint>,
        retention_type: RetentionType,
        needs_index_retention_deletes: bool,
    ) -> anyhow::Result<RepeatableTimestamp> {
        let delay = match retention_type {
            RetentionType::Document => *DOCUMENT_RETENTION_DELAY,
            RetentionType::Index => *INDEX_RETENTION_DELAY,
        };
        let mut candidate = snapshot_reader
            .lock()
            .persisted_max_repeatable_ts()
            .sub(delay)
            .context("Cannot calculate retention timestamp")?;

        if matches!(retention_type, RetentionType::Document) && needs_index_retention_deletes {
            // Ensures the invariant that the index retention confirmed deleted timestamp
            // is always greater than or equal to the minimum document snapshot timestamp.
            // This prevents us from deleting documents before their indexes are deleted
            // and ensures that the index retention deleter reads from a valid snapshot.
            let index_confirmed_deleted = match checkpoint_reader.lock().checkpoint {
                Some(val) => val,
                None => RepeatableTimestamp::MIN,
            };
            candidate = cmp::min(candidate, index_confirmed_deleted);
        }

        Ok(candidate)
    }

    async fn advance_timestamp(
        bounds_writer: &mut Writer<SnapshotBounds>,
        persistence: &dyn Persistence,
        snapshot_reader: &Reader<SnapshotManager>,
        checkpoint_reader: &Reader<Checkpoint>,
        retention_type: RetentionType,
        lease_lost_shutdown: ShutdownSignal,
    ) -> anyhow::Result<Option<RepeatableTimestamp>> {
        let candidate = Self::candidate_min_snapshot_ts(
            snapshot_reader,
            checkpoint_reader,
            retention_type,
            persistence.reader().needs_index_retention_deletes(),
        )
        .await?;
        let min_snapshot_ts = match retention_type {
            RetentionType::Document => bounds_writer.read().min_document_snapshot_ts,
            RetentionType::Index => bounds_writer.read().min_index_snapshot_ts,
        };
        // Skip advancing the timestamp if the `max_repeatable_ts` hasn't increased
        if candidate <= min_snapshot_ts {
            return Ok(None);
        }
        let new_min_snapshot_ts = candidate;
        let persistence_key = match retention_type {
            RetentionType::Document => PersistenceGlobalKey::DocumentRetentionMinSnapshotTimestamp,
            RetentionType::Index => PersistenceGlobalKey::IndexRetentionMinSnapshotTimestamp,
        };
        // It's very important that we write to persistence before writing to memory,
        // because reads (follower reads and leader on restart) use persistence, while
        // the actual deletions use memory. With the invariant that persistence >=
        // memory, we will never read something that has been deleted.
        if let Err(e) = persistence
            .write_persistence_global(
                persistence_key,
                ConvexValue::from(i64::from(*new_min_snapshot_ts)).into(),
            )
            .await
        {
            // An idle instance with no commits at all may never notice that it has lost its
            // lease, except that we'll keep erroring here when we try to advance a
            // timestamp.
            // We want to signal that the instance should shut down if that's the case.
            if let Some(LeaseLostError) = e.downcast_ref() {
                lease_lost_shutdown
                    .signal(lease_lost_error().context("Failed to advance timestamp"));
            }
            return Err(e);
        }
        match retention_type {
            RetentionType::Document => bounds_writer
                .write()
                .advance_min_document_snapshot_ts(new_min_snapshot_ts),
            RetentionType::Index => bounds_writer
                .write()
                .advance_min_snapshot_ts(new_min_snapshot_ts),
        }
        tracing::debug!("Advance {retention_type:?} min snapshot to {new_min_snapshot_ts}");
        // Also log the deletion checkpoint here, so it is periodically reported
        // even if the deletion future is stuck.
        Self::get_checkpoint(
            persistence.reader().as_ref(),
            snapshot_reader.clone(),
            retention_type,
        )
        .await?;
        Ok(Some(new_min_snapshot_ts))
    }

    async fn emit_timestamp(
        snapshot_sender: Option<&Sender<RepeatableTimestamp>>,
        ts: anyhow::Result<Option<RepeatableTimestamp>>,
        retention_type: RetentionType,
    ) {
        match ts {
            Err(mut err) => {
                report_error(&mut err).await;
            },
            Ok(Some(ts)) => {
                log_retention_ts_advanced(retention_type);
                if let Some(snapshot_sender) = snapshot_sender
                    && let Err(err) = snapshot_sender.send(ts)
                {
                    report_error(&mut err.into()).await;
                }
            },
            Ok(None) => {},
        }
    }

    async fn go_advance_min_snapshot<RT: Runtime>(
        mut bounds_writer: Writer<SnapshotBounds>,
        checkpoint_reader: Reader<Checkpoint>,
        rt: RT,
        persistence: Arc<dyn Persistence>,
        min_snapshot_sender: Sender<RepeatableTimestamp>,
        min_document_snapshot_sender: Sender<RepeatableTimestamp>,
        snapshot_reader: Reader<SnapshotManager>,
        shutdown: ShutdownSignal,
    ) {
        let needs_index_retention_deletes = persistence.reader().needs_index_retention_deletes();
        loop {
            {
                let _timer = retention_advance_timestamp_timer();

                let index_ts = Self::advance_timestamp(
                    &mut bounds_writer,
                    persistence.as_ref(),
                    &snapshot_reader,
                    &checkpoint_reader,
                    RetentionType::Index,
                    shutdown.clone(),
                )
                .await;
                Self::emit_timestamp(
                    needs_index_retention_deletes.then_some(&min_snapshot_sender),
                    index_ts,
                    RetentionType::Index,
                )
                .await;

                let document_ts = Self::advance_timestamp(
                    &mut bounds_writer,
                    persistence.as_ref(),
                    &snapshot_reader,
                    &checkpoint_reader,
                    RetentionType::Document,
                    shutdown.clone(),
                )
                .await;
                Self::emit_timestamp(
                    Some(&min_document_snapshot_sender),
                    document_ts,
                    RetentionType::Document,
                )
                .await;
            }
            // We jitter every loop to avoid synchronization of polling the database
            // across different instances
            Self::wait_with_jitter(&rt, ADVANCE_RETENTION_TS_FREQUENCY).await;
        }
    }

    /// Deletes some index entries based on `bounds` which identify what may be
    /// deleted. Returns an [`IndexRetentionProgress`] with the new cursor and
    /// the total expired index entries processed. The cursor is a timestamp
    /// which has been fully deleted, along with all prior timestamps. The
    /// total expired index entries is the number of index entries we found
    /// were expired, not necessarily the total we deleted or wanted to
    /// delete, though they're correlated.
    #[fastrace::trace]
    async fn delete(
        min_snapshot_ts: RepeatableTimestamp,
        persistence: Arc<dyn Persistence>,
        cursor: RepeatableTimestamp,
        all_indexes: &BTreeMap<IndexId, (GenericIndexName<TabletId>, IndexedFields)>,
        retention_validator: Arc<dyn RetentionValidator>,
        source: IndexRetentionSource,
    ) -> anyhow::Result<IndexRetentionProgress> {
        if *min_snapshot_ts == Timestamp::MIN {
            return Ok(IndexRetentionProgress {
                cursor,
                expired_entries: 0,
                deleted_rows: 0,
                unique_indexes: 0,
            });
        }
        let progress = persistence
            .reclaim_index_history(IndexRetentionRequest {
                min_snapshot_ts,
                cursor,
                all_indexes,
                retention_validator,
            })
            .await?;
        log_retention_index_entries_deleted(progress.deleted_rows, source);
        log_retention_unique_indexes(progress.unique_indexes, source);
        Ok(progress)
    }

    pub async fn delete_all_no_checkpoint(
        mut cursor_ts: RepeatableTimestamp,
        min_snapshot_ts: RepeatableTimestamp,
        persistence: Arc<dyn Persistence>,
        all_indexes: &BTreeMap<IndexId, (GenericIndexName<TabletId>, IndexedFields)>,
        retention_validator: Arc<dyn RetentionValidator>,
        source: IndexRetentionSource,
    ) -> anyhow::Result<()> {
        let mut last_logged = Instant::now();
        while cursor_ts.succ()? < *min_snapshot_ts {
            let IndexRetentionProgress {
                cursor: new_cursor_ts,
                ..
            } = Self::delete(
                min_snapshot_ts,
                persistence.clone(),
                cursor_ts,
                all_indexes,
                retention_validator.clone(),
                source,
            )
            .await?;
            let now = Instant::now();
            let duration = now.saturating_duration_since(last_logged).as_secs_f64();
            let catchup_rate = new_cursor_ts.secs_since_f64(*cursor_ts) / duration;
            let lag = min_snapshot_ts.secs_since_f64(*new_cursor_ts);
            tracing::info!(
                "custom index retention completed between ts {cursor_ts} and {new_cursor_ts}; \
                 catchup rate: {catchup_rate:.1}s/s, {lag:.1}s behind snapshot"
            );
            cursor_ts = new_cursor_ts;
            last_logged = now;
        }
        Ok(())
    }

    /// Finds expired documents in the documents log and returns a tuple of the
    /// form (scanned_document_ts, (expired_document_ts,
    /// internal_document_ts))
    #[try_stream(ok = (Timestamp, Option<(Timestamp, InternalDocumentId)>), error = anyhow::Error)]
    async fn expired_documents(
        persistence: Arc<dyn PersistenceReader>,
        cursor: RepeatableTimestamp,
        min_document_snapshot_ts: RepeatableTimestamp,
    ) {
        tracing::trace!(
            "expired_documents: reading expired documents from {cursor:?} to {:?}",
            min_document_snapshot_ts,
        );
        let mut revs = persistence.load_documents(
            TimestampRange::new(*cursor..*min_document_snapshot_ts),
            Order::Asc,
            *DEFAULT_DOCUMENTS_PAGE_SIZE,
            // We are reading document log entries from outside of the retention
            // window (which we have possibly just shrunk ourselves); there is
            // no need to check retention.
            Arc::new(NoopRetentionValidator),
        );
        while let Some(rev) = revs.try_next().await? {
            // Prev revs are the documents we are deleting.
            // Each prev rev has 1 or 2 entries to delete per document -- one entry at
            // the prev rev's ts, and a tombstone at the current rev's ts if
            // the document was deleted.
            // A NoopRetentionValidator is used here because we are fetching revisions
            // outside of the document retention window.
            let DocumentLogEntry {
                id,
                ts,
                value: maybe_doc,
                prev_ts,
            } = rev;
            {
                // If there is no prev rev, there's nothing to delete.
                // If this happens for a tombstone, it means the document was created and
                // deleted in the same transaction.
                let Some(prev_rev_ts) = prev_ts else {
                    log_document_retention_scanned_document(maybe_doc.is_none(), false);
                    if maybe_doc.is_none() {
                        yield (ts, Some((ts, id)));
                    } else {
                        yield (ts, None);
                    }
                    continue;
                };

                log_document_retention_scanned_document(maybe_doc.is_none(), true);
                yield (ts, Some((prev_rev_ts, id)));

                // Deletes tombstones
                if maybe_doc.is_none() {
                    yield (ts, Some((ts, id)));
                }
            }
        }
    }

    /// Deletes some documents based on `bounds` which identify what may be
    /// deleted. Returns a pair of the new cursor and the total number of
    /// documents processed. The cursor is a timestamp which has been
    /// fully deleted, along with all prior timestamps. The total expired
    /// document count is the number of documents we found were expired, not
    /// necessarily the total we deleted or wanted to delete, though they're
    /// correlated.
    async fn delete_documents<RT: Runtime>(
        min_snapshot_ts: RepeatableTimestamp,
        persistence: Arc<dyn Persistence>,
        rt: &RT,
        cursor: RepeatableTimestamp,
        document_deletion_rate_limiter: Arc<RateLimiter<RT>>,
    ) -> anyhow::Result<(RepeatableTimestamp, usize)> {
        if *min_snapshot_ts == Timestamp::MIN {
            return Ok((cursor, 0));
        }
        // The number of rows we delete in persistence.
        let mut total_deleted_rows: usize = 0;
        // The number of expired entries we read from chunks.
        let mut total_expired_entries = 0;
        let mut new_cursor = cursor;
        // The number of scanned documents
        let mut scanned_documents = 0;

        let reader = persistence.reader();
        let snapshot_ts = min_snapshot_ts;

        tracing::trace!("delete_documents: about to grab chunks");
        let expired_chunks = Self::expired_documents(reader, cursor, min_snapshot_ts)
            .try_chunks2(DOCUMENT_RETENTION_DELETE_CHUNK.get() as usize);
        pin_mut!(expired_chunks);
        while let Some(scanned_chunk) = expired_chunks.try_next().await? {
            tracing::trace!(
                "delete_documents: got a chunk and finished waiting {:?}",
                scanned_chunk.len()
            );
            // Converts scanned documents to the actual documents we want to delete
            scanned_documents += scanned_chunk.len();
            let delete_chunk: Vec<(Timestamp, (Timestamp, InternalDocumentId))> = scanned_chunk
                .into_iter()
                .filter_map(
                    |doc: (Timestamp, Option<(Timestamp, InternalDocumentId)>)| {
                        if let Some(v) = doc.1 {
                            Some((doc.0, v))
                        } else {
                            None
                        }
                    },
                )
                .collect();
            total_expired_entries += delete_chunk.len();
            let results = try_join_all(
                Self::partition_document_chunk(delete_chunk)
                    .into_iter()
                    .map(|delete_chunk| async {
                        if delete_chunk.is_empty() {
                            return Ok((*new_cursor, 0));
                        }
                        let mut chunk_len = delete_chunk.len() as u32;
                        loop {
                            match document_deletion_rate_limiter
                                .check_n(chunk_len.try_into().unwrap())
                            {
                                Ok(Ok(())) => {
                                    break;
                                },
                                Ok(Err(not_until)) => {
                                    let wait_time = Jitter::up_to(Duration::from_secs(1))
                                        + not_until.wait_time_from(rt.monotonic_now().into());
                                    rt.wait(wait_time).await;
                                    continue;
                                },
                                Err(InsufficientCapacity(n)) => {
                                    tracing::warn!(
                                        "Retention rate limiter quota is insufficient for chunks \
                                         of {} documents (current quota: {n}/sec), rate limit \
                                         will be exceeded",
                                        delete_chunk.len()
                                    );
                                    chunk_len = n;
                                    continue;
                                },
                            }
                        }
                        Self::delete_document_chunk(delete_chunk, persistence.clone(), *new_cursor)
                            .await
                    }),
            )
            .await?;
            let (chunk_new_cursors, deleted_rows): (Vec<_>, Vec<_>) = results.into_iter().unzip();
            // We have successfully deleted all of delete_chunk, so update
            // total_deleted_rows and new_cursor to reflect the deletions.
            total_deleted_rows += deleted_rows.into_iter().sum::<usize>();
            if let Some(max_new_cursor) = chunk_new_cursors.into_iter().max() {
                new_cursor = snapshot_ts.prior_ts(max_new_cursor)?;
            }
            if new_cursor > cursor && scanned_documents >= *DOCUMENT_RETENTION_MAX_SCANNED_DOCUMENTS
            {
                tracing::debug!(
                    "delete_documents: returning early with {new_cursor:?}, total expired \
                     documents read: {total_expired_entries:?}, total rows deleted: \
                     {total_deleted_rows:?}"
                );
                // we're not done deleting everything.
                return Ok((new_cursor, scanned_documents));
            }
        }
        tracing::debug!(
            "delete: finished loop, returning {:?}",
            min_snapshot_ts.pred()
        );
        min_snapshot_ts
            .pred()
            .map(|timestamp| (timestamp, total_expired_entries))
    }

    /// Partitions documents into DOCUMENT_RETENTION_DELETE_PARALLEL parts where
    /// each document id only exists in one part
    fn partition_document_chunk(
        to_partition: Vec<(Timestamp, (Timestamp, InternalDocumentId))>,
    ) -> Vec<Vec<(Timestamp, (Timestamp, InternalDocumentId))>> {
        let mut parts = Vec::new();
        for _ in 0..*DOCUMENT_RETENTION_DELETE_PARALLEL {
            parts.push(vec![]);
        }
        for entry in to_partition {
            let mut hash = DefaultHasher::new();
            entry.1 .1.hash(&mut hash);
            let i = (hash.finish() as usize) % *DOCUMENT_RETENTION_DELETE_PARALLEL;
            parts[i].push(entry);
        }
        parts
    }

    #[fastrace::trace]

    async fn delete_document_chunk(
        delete_chunk: Vec<(Timestamp, (Timestamp, InternalDocumentId))>,
        persistence: Arc<dyn Persistence>,
        mut new_cursor: Timestamp,
    ) -> anyhow::Result<(Timestamp, usize)> {
        let _timer = retention_delete_document_chunk_timer();
        let documents_to_delete = delete_chunk.len();
        tracing::trace!("delete_documents: there are {documents_to_delete:?} documents to delete");
        for document_to_delete in delete_chunk.iter() {
            // If we're deleting the previous revision of a document, we've definitely
            // deleted entries for documents at all prior timestamps.
            if document_to_delete.0 > Timestamp::MIN {
                new_cursor = cmp::max(new_cursor, document_to_delete.0.pred()?);
            }
        }
        let deleted_rows = if documents_to_delete > 0 {
            persistence
                .delete(delete_chunk.into_iter().map(|doc| doc.1).collect())
                .await?
        } else {
            0
        };

        // If there are more documents to delete than we see in the delete chunk,
        // it means retention skipped deleting documents before, and we
        // incorrectly bumped DocumentRetentionConfirmedDeletedTimestamp anyway.
        if deleted_rows > documents_to_delete {
            report_error(&mut anyhow::anyhow!(
                "retention wanted to delete {documents_to_delete} documents but found \
                 {deleted_rows} to delete"
            ))
            .await;
        }

        tracing::trace!("delete_documents: deleted {deleted_rows:?} rows");
        log_retention_documents_deleted(deleted_rows);
        Ok((new_cursor, deleted_rows))
    }

    async fn wait_with_jitter<RT: Runtime>(rt: &RT, delay: Duration) {
        // Abuse backoff to get jitter by passing in the same constant for initial and
        // max backoff.
        let mut initial_backoff = Backoff::new(delay, delay);
        let delay = initial_backoff.fail(&mut rt.rng());
        rt.wait(delay).await;
    }

    async fn go_delete_indexes<RT: Runtime>(
        bounds_reader: Reader<SnapshotBounds>,
        rt: RT,
        persistence: Arc<dyn Persistence>,
        mut all_indexes: BTreeMap<IndexId, (GenericIndexName<TabletId>, IndexedFields)>,
        index_table_id: TabletId,
        mut index_cursor: RepeatableTimestamp,
        retention_validator: Arc<dyn RetentionValidator>,
        mut min_snapshot_rx: Receiver<RepeatableTimestamp>,
        mut checkpoint_writer: Writer<Checkpoint>,
        snapshot_reader: Reader<SnapshotManager>,
        mut cursor: RepeatableTimestamp,
        quota: Quota,
    ) {
        let checkpoint_rate_limiter = new_rate_limiter(rt.clone(), quota);
        let mut error_backoff = Backoff::new(INITIAL_BACKOFF, *MAX_RETENTION_DELAY_SECONDS);
        loop {
            // Note that when retention is caught up, the cursor advances to
            // min_snapshot_ts.pred()
            let min_snapshot_ts = match min_snapshot_rx
                .wait_for(|&min_snapshot_ts| {
                    cursor.succ_opt().is_some_and(|c1| c1 < *min_snapshot_ts)
                })
                .await
                .map(|ts| *ts)
            {
                Ok(ts) => ts,
                Err(err) => {
                    report_error(&mut err.into()).await;
                    // Channel error means we're shutting down.
                    return;
                },
            };

            tracing::trace!(
                "go_delete_indexes: running, current_bounds: {cursor}..{min_snapshot_ts}",
            );
            let span = get_sampled_span("", "delete_indexes", &mut rt.rng());
            let r: anyhow::Result<()> = async {
                let _timer = index_retention_delete_timer();
                tracing::trace!("go_delete_indexes: loaded checkpoint: {cursor:?}");
                let latest_ts = snapshot_reader.lock().persisted_max_repeatable_ts();
                Self::accumulate_indexes(
                    persistence.as_ref(),
                    &mut all_indexes,
                    &mut index_cursor,
                    latest_ts,
                    index_table_id,
                    retention_validator.clone(),
                )
                .await?;
                tracing::trace!("go_delete_indexes: Loaded initial indexes");
                let index_count_before = all_indexes.len();
                let IndexRetentionProgress {
                    cursor: new_cursor,
                    expired_entries: expired_index_entries_processed,
                    ..
                } = Self::delete(
                    min_snapshot_ts,
                    persistence.clone(),
                    cursor,
                    &all_indexes,
                    retention_validator.clone(),
                    IndexRetentionSource::Live,
                )
                .await?;
                tracing::trace!(
                    "go_delete_indexes: processed {expired_index_entries_processed:?} rows up to \
                     {new_cursor}"
                );
                let latest_ts = snapshot_reader.lock().persisted_max_repeatable_ts();
                Self::accumulate_indexes(
                    persistence.as_ref(),
                    &mut all_indexes,
                    &mut index_cursor,
                    latest_ts,
                    index_table_id,
                    retention_validator.clone(),
                )
                .await?;
                tracing::trace!("go_delete_indexes: loaded second round of indexes");
                if all_indexes.len() == index_count_before {
                    cursor = new_cursor;
                    match checkpoint_rate_limiter.check() {
                        Ok(_) => {
                            tracing::debug!("go_delete_indexes: Checkpointing at: {new_cursor:?}");
                            // No indexes were added while we were doing the delete.
                            // So the `delete` covered all index rows up to new_cursor.
                            Self::checkpoint(
                                persistence.as_ref(),
                                new_cursor,
                                &mut checkpoint_writer,
                                RetentionType::Index,
                                bounds_reader.clone(),
                                snapshot_reader.clone(),
                            )
                            .await?;
                        },
                        Err(not_until) => {
                            tracing::debug!(
                                "go_delete_indexes: Not checkpointing, not until: {not_until:?}"
                            );
                        },
                    }
                } else {
                    tracing::debug!(
                        "go_delete_indexes: Skipping checkpoint, index count changed, now: {:?}, \
                         before: {index_count_before:?}",
                        all_indexes.len()
                    );
                }

                Ok(())
            }
            .in_span(span)
            .await;
            if let Err(mut err) = r {
                report_error(&mut err).await;
                let delay = error_backoff.fail(&mut rt.rng());
                tracing::debug!("go_delete_indexes: error, {err:?}, delaying {delay:?}");
                rt.wait(delay).await;
            } else {
                error_backoff.reset();
            }
        }
    }

    /// Delete documents in tablets that were deleted outside the document
    /// retention window, prior to min_document_snapshot_ts.
    /// Returns remaining tablets that need to be deleted after
    /// `min_document_snapshot_ts` moves forward.
    async fn delete_documents_in_tablets<RT: Runtime>(
        tablets_to_delete: &BTreeMap<TabletId, Timestamp>,
        document_deletion_rate_limiter: Arc<RateLimiter<RT>>,
        min_document_snapshot_ts: RepeatableTimestamp,
        persistence: Arc<dyn Persistence>,
        rt: &RT,
        deleted_tablet_sender: mpsc::Sender<TabletId>,
    ) -> anyhow::Result<BTreeMap<TabletId, Timestamp>> {
        let mut skipped_tablets = BTreeMap::new();
        for (tablet_id, ts) in tablets_to_delete {
            // N.B. We must skip deleted tablets that are still within the retention period.
            if *ts >= *min_document_snapshot_ts {
                skipped_tablets.insert(*tablet_id, *ts);
                continue;
            }
            tracing::info!(
                "go_delete_table_documents: Deleting documents in tablet {tablet_id} deleted at \
                 timestamp {ts}"
            );
            let mut chunk_len = *DELETE_TABLET_CHUNK_SIZE as u32;
            loop {
                match document_deletion_rate_limiter.check_n(chunk_len.try_into().unwrap()) {
                    Ok(Ok(())) => {
                        let rows_deleted = persistence
                            .delete_tablet_documents(*tablet_id, *DELETE_TABLET_CHUNK_SIZE as usize)
                            .await?;
                        tracing::debug!(
                            "go_delete_table_documents: Deleted {rows_deleted} documents in \
                             tablet {tablet_id}"
                        );
                        log_deleted_tablet_documents_deleted(rows_deleted);
                        if rows_deleted == 0 {
                            deleted_tablet_sender.send(*tablet_id).await?;
                            tracing::debug!(
                                "go_delete_table_documents: Finished deleting documents in tablet \
                                 {tablet_id}"
                            );
                            break;
                        }
                    },
                    Ok(Err(not_until)) => {
                        let wait_time = Jitter::up_to(Duration::from_secs(1))
                            + not_until.wait_time_from(rt.monotonic_now().into());
                        rt.wait(wait_time).await;
                        continue;
                    },
                    Err(InsufficientCapacity(n)) => {
                        tracing::warn!(
                            "Retention rate limiter quota is insufficient for chunks of {} \
                             documents (current quota: {n}/sec), rate limit will be exceeded",
                            chunk_len
                        );
                        chunk_len = n;
                        continue;
                    },
                }
            }
        }
        Ok(skipped_tablets)
    }

    async fn go_delete_table_documents<RT: Runtime>(
        bounds_reader: Reader<SnapshotBounds>,
        rt: RT,
        persistence: Arc<dyn Persistence>,
        mut min_document_snapshot_rx: Receiver<RepeatableTimestamp>,
        document_deletion_rate_limiter: Arc<RateLimiter<RT>>,
        mut tablets_to_delete: BTreeMap<TabletId, Timestamp>,
        deleted_tablet_sender: mpsc::Sender<TabletId>,
    ) {
        let mut error_backoff =
            Backoff::new(INITIAL_BACKOFF, *DOCUMENT_RETENTION_BATCH_INTERVAL_SECONDS);
        min_document_snapshot_rx.mark_changed();
        // Note that in theory, we should rerun our query for `tablets_to_delete` if
        // `min_document_snapshot_ts` passes the ts that we
        // previously ran the query at, but this is unlikely to matter in practice
        // because `min_document_snapshot_ts` starts out weeks behind and we restart
        // conductors more frequently than that gap. The worst that happens from
        // not requerying `tablets_to_delete` is that we retain deleted tablets
        // a little bit longer than necessary.
        while !tablets_to_delete.is_empty() {
            let min_document_snapshot_ts = match min_document_snapshot_rx.changed().await {
                Err(err) => {
                    report_error(&mut err.into()).await;
                    // Fall back to polling if the channel is closed or falls over. This should
                    // really never happen.
                    Self::wait_with_jitter(&rt, *DOCUMENT_RETENTION_BATCH_INTERVAL_SECONDS).await;
                    bounds_reader.lock().min_document_snapshot_ts
                },
                Ok(()) => *min_document_snapshot_rx.borrow(),
            };
            match Self::delete_documents_in_tablets(
                &tablets_to_delete,
                document_deletion_rate_limiter.clone(),
                min_document_snapshot_ts,
                persistence.clone(),
                &rt,
                deleted_tablet_sender.clone(),
            )
            .await
            {
                Ok(skipped_tablets) => {
                    tablets_to_delete = skipped_tablets;
                    error_backoff.reset();
                },
                Err(mut err) => {
                    report_error(&mut err).await;
                    let delay = error_backoff.fail(&mut rt.rng());
                    tracing::debug!(
                        "go_delete_table_documents: error, {err:?}, delaying {delay:?}"
                    );
                    rt.wait(delay).await;
                },
            };
        }
        tracing::info!("No tablets left to delete");
    }

    async fn go_delete_documents<RT: Runtime>(
        bounds_reader: Reader<SnapshotBounds>,
        rt: RT,
        persistence: Arc<dyn Persistence>,
        mut min_document_snapshot_rx: Receiver<RepeatableTimestamp>,
        mut checkpoint_writer: Writer<Checkpoint>,
        snapshot_reader: Reader<SnapshotManager>,
        document_deletion_rate_limiter: Arc<RateLimiter<RT>>,
        mut cursor: RepeatableTimestamp,
        quota: Quota,
    ) {
        let checkpoint_rate_limiter = new_rate_limiter(rt.clone(), quota);
        // Wait with jitter on startup to avoid thundering herd
        Self::wait_with_jitter(&rt, *DOCUMENT_RETENTION_BATCH_INTERVAL_SECONDS).await;

        let mut error_backoff =
            Backoff::new(INITIAL_BACKOFF, *DOCUMENT_RETENTION_BATCH_INTERVAL_SECONDS);
        let mut interval = tokio::time::interval(*DOCUMENT_RETENTION_BATCH_INTERVAL_SECONDS);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            // Note that when retention is caught up, the cursor advances to
            // min_document_snapshot_ts.pred()
            let min_document_snapshot_ts = match min_document_snapshot_rx
                .wait_for(|&min_document_snapshot_ts| {
                    cursor
                        .succ_opt()
                        .is_some_and(|c1| c1 < *min_document_snapshot_ts)
                })
                .await
                .map(|ts| *ts)
            {
                Ok(ts) => ts,
                Err(err) => {
                    report_error(&mut err.into()).await;
                    // Channel error means we're shutting down.
                    return;
                },
            };

            // Rate limit so we don't overload the database
            interval.tick().await;

            tracing::trace!(
                "go_delete_documents: running, current_bounds: \
                 {cursor}..{min_document_snapshot_ts}",
            );
            let r: anyhow::Result<()> = try_anyhow!({
                // Only delete documents up to (now() -
                // DOCUMENT_RETENTION_DELAY), even if min_document_snapshot_ts
                // is ahead of that point.
                let max_end_cursor = min_document_snapshot_ts.prior_ts(
                    (*min_document_snapshot_ts)
                        .min(rt.generate_timestamp()?.sub(*DOCUMENT_RETENTION_DELAY)?),
                )?;
                if cursor >= max_end_cursor {
                    tracing::info!(
                        "go_delete_documents: cursor exceeds document retention delay ({cursor} > \
                         {max_end_cursor}); pausing"
                    );
                    tokio::time::sleep(*cursor - *max_end_cursor).await;
                    continue;
                }
                let _timer = retention_delete_documents_timer();
                let (new_cursor, scanned_documents) = Self::delete_documents(
                    max_end_cursor,
                    persistence.clone(),
                    &rt,
                    cursor,
                    document_deletion_rate_limiter.clone(),
                )
                .await?;
                if *new_cursor < *cursor {
                    Err(anyhow::anyhow!(
                        "document retention cursor went backward from {cursor} to {new_cursor}?"
                    ))?;
                }
                cursor = new_cursor;
                tracing::trace!(
                    "go_delete_documents: processed {scanned_documents:?} rows up to {new_cursor}"
                );
                max_end_cursor.prior_ts(*new_cursor)?;

                match checkpoint_rate_limiter.check() {
                    Ok(_) => {
                        tracing::debug!("go_delete_documents: Checkpointing at: {new_cursor:?}");
                        Self::checkpoint(
                            persistence.as_ref(),
                            new_cursor,
                            &mut checkpoint_writer,
                            RetentionType::Document,
                            bounds_reader.clone(),
                            snapshot_reader.clone(),
                        )
                        .await?;
                    },
                    Err(not_until) => {
                        tracing::debug!(
                            "go_delete_documents: Skipping checkpointing until: {not_until:?}"
                        );
                    },
                }
            });
            if let Err(mut err) = r {
                report_error(&mut err).await;
                let delay = error_backoff.fail(&mut rt.rng());
                tracing::debug!("go_delete_documents: error, {err:?}, delaying {delay:?}");
                rt.wait(delay).await;
            } else {
                error_backoff.reset();
            }
        }
    }

    async fn checkpoint(
        persistence: &dyn Persistence,
        cursor: RepeatableTimestamp,
        checkpoint_writer: &mut Writer<Checkpoint>,
        retention_type: RetentionType,
        bounds_reader: Reader<SnapshotBounds>,
        snapshot_reader: Reader<SnapshotManager>,
    ) -> anyhow::Result<()> {
        let key = match retention_type {
            RetentionType::Document => {
                PersistenceGlobalKey::DocumentRetentionConfirmedDeletedTimestamp
            },
            RetentionType::Index => PersistenceGlobalKey::IndexRetentionConfirmedDeletedTimestamp,
        };
        persistence
            .write_persistence_global(key, ConvexValue::from(i64::from(*cursor)).into())
            .await?;
        checkpoint_writer.write().advance_checkpoint(cursor);
        if *cursor > Timestamp::MIN {
            // Only log if the checkpoint has been written once, to avoid logging time since
            // epoch when the instance is first starting up.
            match retention_type {
                RetentionType::Document => {
                    log_document_retention_cursor_age(
                        (*snapshot_reader.lock().persisted_max_repeatable_ts())
                            .secs_since_f64(*cursor),
                    );
                    log_document_retention_cursor_lag(
                        bounds_reader
                            .lock()
                            .min_document_snapshot_ts
                            .secs_since_f64(*cursor),
                    );
                },
                RetentionType::Index => {
                    log_index_retention_cursor_age(
                        (*snapshot_reader.lock().persisted_max_repeatable_ts())
                            .secs_since_f64(*cursor),
                    );
                    log_index_retention_cursor_lag(
                        bounds_reader
                            .lock()
                            .min_index_snapshot_ts
                            .secs_since_f64(*cursor),
                    );
                },
            }
        } else {
            match retention_type {
                RetentionType::Document => log_document_retention_no_cursor(),
                RetentionType::Index => log_index_retention_no_cursor(),
            }
        }
        Ok(())
    }

    pub async fn get_checkpoint_not_repeatable(
        persistence: &dyn PersistenceReader,
        retention_type: RetentionType,
    ) -> anyhow::Result<Timestamp> {
        let key = match retention_type {
            RetentionType::Document => {
                PersistenceGlobalKey::DocumentRetentionConfirmedDeletedTimestamp
            },
            RetentionType::Index => PersistenceGlobalKey::IndexRetentionConfirmedDeletedTimestamp,
        };
        let checkpoint_value = persistence
            .get_persistence_global(key)
            .await?
            .map(ConvexValue::try_from)
            .transpose()?;
        let checkpoint = match checkpoint_value {
            Some(ConvexValue::Int64(ts)) => Timestamp::try_from(ts)?,
            None => Timestamp::MIN,
            _ => anyhow::bail!("invalid retention checkpoint {checkpoint_value:?}"),
        };
        Ok(checkpoint)
    }

    pub async fn get_checkpoint(
        persistence: &dyn PersistenceReader,
        snapshot_reader: Reader<SnapshotManager>,
        retention_type: RetentionType,
    ) -> anyhow::Result<RepeatableTimestamp> {
        let checkpoint = Self::get_checkpoint_not_repeatable(persistence, retention_type).await?;
        snapshot_reader
            .lock()
            .persisted_max_repeatable_ts()
            .prior_ts(checkpoint)
    }

    fn accumulate_index_document(
        doc: ResolvedDocument,
        all_indexes: &mut BTreeMap<IndexId, (GenericIndexName<TabletId>, IndexedFields)>,
    ) -> anyhow::Result<()> {
        let index_id = doc.id().internal_id().into();
        let index: ParsedDocument<IndexMetadata<TabletId>> = doc.parse()?;
        let index = index.into_value();
        let IndexConfig::Database {
            spec,
            on_disk_state,
            persistence_index_id: _,
        } = index.config
        else {
            return Ok(());
        };

        // Don't run retention for indexes that are still backfilling unless IndexWorker
        // has explicitly opted-in to running retention. This is important for
        // correctness since index backfill and retention interact poorly.
        // NOTE: accumulate only adds indexes. Thus we won't stop running
        // retention if index is deleted or changes from Enabled to Backfilling.
        if let DatabaseIndexState::Backfilling(state) = on_disk_state
            && !state.retention_started
        {
            return Ok(());
        }

        all_indexes.insert(index_id, (index.name, spec.fields));
        Ok(())
    }

    #[fastrace::trace]
    async fn accumulate_indexes(
        persistence: &dyn Persistence,
        all_indexes: &mut BTreeMap<IndexId, (GenericIndexName<TabletId>, IndexedFields)>,
        cursor: &mut RepeatableTimestamp,
        latest_ts: RepeatableTimestamp,
        index_table_id: TabletId,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<()> {
        let reader = persistence.reader();
        let mut document_stream = reader.load_documents_from_table(
            index_table_id,
            TimestampRange::new(**cursor..*latest_ts),
            Order::Asc,
            *DEFAULT_DOCUMENTS_PAGE_SIZE,
            retention_validator,
        );
        while let Some(entry) = document_stream.try_next().await? {
            if let Some(doc) = entry.value {
                anyhow::ensure!(doc.id().tablet_id == index_table_id);
                Self::accumulate_index_document(doc, all_indexes)?;
            }
        }
        *cursor = latest_ts;
        Ok(())
    }
}

const ADVANCE_RETENTION_TS_FREQUENCY: Duration = Duration::from_secs(30);

#[async_trait]
impl<RT: Runtime> RetentionValidator for LeaderRetentionManager<RT> {
    async fn validate_snapshot(&self, ts: Timestamp) -> anyhow::Result<()> {
        let min_snapshot_ts = self.bounds_reader.lock().min_index_snapshot_ts;
        log_snapshot_verification_age(&self.rt, ts, *min_snapshot_ts, false, true);
        if ts < *min_snapshot_ts {
            anyhow::bail!(snapshot_invalid_error(
                ts,
                *min_snapshot_ts,
                RetentionType::Index,
                "leader",
            ));
        }
        Ok(())
    }

    async fn validate_document_snapshot(&self, ts: Timestamp) -> anyhow::Result<()> {
        let min_snapshot_ts = self.bounds_reader.lock().min_document_snapshot_ts;
        if ts < *min_snapshot_ts {
            anyhow::bail!(snapshot_invalid_error(
                ts,
                *min_snapshot_ts,
                RetentionType::Document,
                "leader",
            ));
        }
        Ok(())
    }

    fn optimistic_validate_snapshot(&self, ts: Timestamp) -> anyhow::Result<()> {
        let min_snapshot_ts = self.bounds_reader.lock().min_index_snapshot_ts;
        log_snapshot_verification_age(&self.rt, ts, *min_snapshot_ts, true, true);
        anyhow::ensure!(
            ts >= *min_snapshot_ts,
            snapshot_invalid_error(
                ts,
                *min_snapshot_ts,
                RetentionType::Index,
                "optimistic leader",
            )
        );
        Ok(())
    }

    async fn min_snapshot_ts(&self) -> anyhow::Result<RepeatableTimestamp> {
        Ok(self.bounds_reader.lock().min_index_snapshot_ts)
    }

    async fn min_document_snapshot_ts(&self) -> anyhow::Result<RepeatableTimestamp> {
        Ok(self.bounds_reader.lock().min_document_snapshot_ts)
    }
}

#[derive(Clone)]
pub struct FollowerRetentionManager<RT: Runtime> {
    rt: RT,
    snapshot_bounds: Arc<Mutex<SnapshotBounds>>,
    persistence: Arc<dyn PersistenceReader>,
}

async fn load_snapshot_bounds(
    persistence: &dyn PersistenceReader,
    repeatable_ts: RepeatableTimestamp,
) -> anyhow::Result<SnapshotBounds> {
    let min_index_snapshot_ts =
        latest_retention_min_snapshot_ts(persistence, RetentionType::Index).await?;
    let min_document_snapshot_ts =
        latest_retention_min_snapshot_ts(persistence, RetentionType::Document).await?;
    if repeatable_ts < min_index_snapshot_ts {
        anyhow::bail!(snapshot_invalid_error(
            *repeatable_ts,
            *min_index_snapshot_ts,
            RetentionType::Index,
            "follower initialization",
        ));
    }
    if repeatable_ts < min_document_snapshot_ts {
        anyhow::bail!(snapshot_invalid_error(
            *repeatable_ts,
            *min_document_snapshot_ts,
            RetentionType::Document,
            "follower initialization",
        ));
    }
    Ok(SnapshotBounds {
        min_index_snapshot_ts,
        min_document_snapshot_ts,
    })
}

impl<RT: Runtime> FollowerRetentionManager<RT> {
    pub async fn new(rt: RT, persistence: Arc<dyn PersistenceReader>) -> anyhow::Result<Self> {
        let repeatable_ts = new_static_repeatable_recent(persistence.as_ref()).await?;
        Self::new_with_repeatable_ts(rt, persistence, repeatable_ts).await
    }

    pub async fn new_with_repeatable_ts(
        rt: RT,
        persistence: Arc<dyn PersistenceReader>,
        repeatable_ts: RepeatableTimestamp,
    ) -> anyhow::Result<Self> {
        let snapshot_bounds = Arc::new(Mutex::new(
            load_snapshot_bounds(persistence.as_ref(), repeatable_ts).await?,
        ));
        Ok(Self {
            rt,
            snapshot_bounds,
            persistence,
        })
    }
}

#[async_trait]
impl<RT: Runtime> RetentionValidator for FollowerRetentionManager<RT> {
    async fn validate_snapshot(&self, ts: Timestamp) -> anyhow::Result<()> {
        let min_snapshot_ts = self.min_snapshot_ts().await?;
        log_snapshot_verification_age(&self.rt, ts, *min_snapshot_ts, false, false);
        if ts < *min_snapshot_ts {
            anyhow::bail!(snapshot_invalid_error(
                ts,
                *min_snapshot_ts,
                RetentionType::Index,
                "follower"
            ));
        }
        Ok(())
    }

    async fn validate_document_snapshot(&self, ts: Timestamp) -> anyhow::Result<()> {
        let min_snapshot_ts = self.min_document_snapshot_ts().await?;
        if ts < *min_snapshot_ts {
            anyhow::bail!(snapshot_invalid_error(
                ts,
                *min_snapshot_ts,
                RetentionType::Document,
                "follower"
            ));
        }
        Ok(())
    }

    fn optimistic_validate_snapshot(&self, ts: Timestamp) -> anyhow::Result<()> {
        let min_snapshot_ts = self.snapshot_bounds.lock().min_index_snapshot_ts;
        log_snapshot_verification_age(&self.rt, ts, *min_snapshot_ts, true, false);
        anyhow::ensure!(
            ts >= *min_snapshot_ts,
            snapshot_invalid_error(
                ts,
                *min_snapshot_ts,
                RetentionType::Index,
                "optimistic follower"
            )
        );
        Ok(())
    }

    async fn min_snapshot_ts(&self) -> anyhow::Result<RepeatableTimestamp> {
        let latest =
            latest_retention_min_snapshot_ts(self.persistence.as_ref(), RetentionType::Index)
                .await?;
        let mut snapshot_bounds = self.snapshot_bounds.lock();
        snapshot_bounds.advance_min_snapshot_ts(latest);
        Ok(latest)
    }

    async fn min_document_snapshot_ts(&self) -> anyhow::Result<RepeatableTimestamp> {
        let latest =
            latest_retention_min_snapshot_ts(self.persistence.as_ref(), RetentionType::Document)
                .await?;
        let mut snapshot_bounds = self.snapshot_bounds.lock();
        snapshot_bounds.advance_min_document_snapshot_ts(latest);
        Ok(latest)
    }
}

fn snapshot_invalid_error(
    ts: Timestamp,
    min_snapshot_ts: Timestamp,
    retention_type: RetentionType,
    context: &str,
) -> anyhow::Error {
    anyhow::anyhow!(ErrorMetadata::out_of_retention()).context(format!(
        "{retention_type:?} snapshot timestamp out of {context} retention window: {ts} < \
         {min_snapshot_ts}"
    ))
}
