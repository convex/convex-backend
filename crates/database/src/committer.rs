use std::{
    cmp,
    collections::{
        BTreeSet,
        VecDeque,
    },
    ops::Bound,
    sync::Arc,
};

use ::metrics::{
    StatusTimer,
    Subgauge,
    Timer,
};
use anyhow::Context as _;
use common::{
    backoff::Backoff,
    bootstrap_model::tables::{
        TableMetadata,
        TableState,
        TABLES_TABLE,
    },
    components::{
        ComponentId,
        ComponentPath,
    },
    document::{
        DocumentUpdateWithPrevTs,
        ParseDocument,
        ParsedDocument,
        ResolvedDocument,
    },
    errors::{
        recapture_stacktrace,
        report_error,
    },
    fastrace_helpers::get_sampled_span,
    knobs::{
        COMMITTER_MAX_CONCURRENT_COMMITS,
        COMMITTER_MAX_CONCURRENT_PRE_VALIDATIONS,
        COMMITTER_MAX_PRE_VALIDATE_BATCH_SIZE,
        COMMITTER_QUEUE_SIZE,
        COMMIT_TRACE_THRESHOLD,
        INITIAL_PERSISTENCE_WRITES_BACKOFF,
        MAX_PERSISTENCE_WRITES_BACKOFF,
        MAX_REPEATABLE_TIMESTAMP_COMMIT_DELAY,
        MAX_REPEATABLE_TIMESTAMP_IDLE_FREQUENCY,
        SEND_COMMIT_MESSAGE_TIMEOUT_MILLIS,
        TRANSACTION_WARN_READ_SET_INTERVALS,
    },
    persistence::{
        DocumentLogEntry,
        Persistence,
        PersistenceGlobalKey,
        PersistenceIndexEntry,
        PersistenceReader,
        RepeatablePersistence,
        RetentionValidator,
        TimestampRange,
    },
    runtime::{
        block_in_place,
        tokio_spawn,
        Runtime,
        SpawnHandle,
    },
    shutdown::ShutdownSignal,
    sync::split_rw_lock::{
        Reader,
        Writer,
    },
    types::{
        DatabaseIndexUpdate,
        DatabaseIndexValue,
        RepeatableTimestamp,
        Timestamp,
        WriteTimestamp,
    },
    virtual_system_mapping::VirtualSystemMapping,
};
use errors::ErrorMetadata;
use fastrace::prelude::*;
use futures::{
    future::{
        BoxFuture,
        Either,
    },
    select_biased,
    stream::{
        FuturesOrdered,
        FuturesUnordered,
    },
    FutureExt,
    StreamExt,
    TryStreamExt,
};
use indexing::{
    index_cache::IndexCacheHandle,
    index_registry::IndexRegistry,
};
use itertools::Itertools;
use parking_lot::Mutex;
use prometheus::VMHistogram;
use rand::Rng;
use search::TextIndexWriteSize;
use tokio::{
    sync::{
        mpsc::{
            self,
            error::TrySendError,
        },
        oneshot,
    },
    time::Instant,
};
use tokio_util::task::AbortOnDropHandle;
use usage_tracking::FunctionUsageTracker;
use value::{
    id_v6::DeveloperDocumentId,
    InternalDocumentId,
    TableMapping,
    TableName,
};
use vector::VectorIndexWriteSize;

use crate::{
    bootstrap_model::defaults::BootstrapTableIds,
    database::ConflictingReadWithWriteSource,
    metrics::{
        self,
        bootstrap_update_timer,
        concurrent_commits_subgauge,
        finish_bootstrap_update,
        next_commit_ts_seconds,
        table_summary_finish_bootstrap_timer,
        user_documents_size_subgauge,
    },
    reads::ReadSet,
    search_flusher_wake::SearchFlusherWakeSignals,
    search_index_bootstrap::{
        stream_revision_pairs_for_indexes,
        BootstrappedSearchIndexes,
    },
    snapshot_manager::SnapshotManager,
    table_summary::{
        self,
    },
    transaction::FinalTransaction,
    write_batcher::{
        WriteBatcher,
        WriteBatcherConfig,
    },
    write_log::{
        index_keys_from_full_documents,
        IndexKeyWrites,
        LogWriter,
        OrderedDocumentWrites,
        PackedDocumentUpdate,
        PendingWriteHandle,
        PendingWrites,
        WriteLogSnapshot,
        WriteSource,
    },
    write_throughput_limiter::WriteVolume,
    ComponentRegistry,
    Snapshot,
    Transaction,
    TransactionReadSet,
};

enum PersistenceWrite {
    Commit {
        pending_write: PendingWriteHandle,
        commit_timer: StatusTimer,
        result: oneshot::Sender<anyhow::Result<Timestamp>>,
        parent_trace: Option<SpanContext>,
        commit_id: usize,
        write_volume: WriteVolume,
        index_key_writes: IndexKeyWrites,
    },
    MaxRepeatableTimestamp {
        new_max_repeatable: Timestamp,
        timer: Timer<VMHistogram>,
        result: oneshot::Sender<Timestamp>,
        commit_id: usize,
    },
}

impl PersistenceWrite {
    fn commit_id(&self) -> usize {
        match self {
            Self::Commit { commit_id, .. } => *commit_id,
            Self::MaxRepeatableTimestamp { commit_id, .. } => *commit_id,
        }
    }
}

pub const AFTER_PENDING_WRITE_SNAPSHOT: &str = "after_pending_write_snapshot";

pub const AFTER_MAX_REPEATABLE_TIMESTAMP_PERSISTENCE: &str =
    "after_max_repeatable_timestamp_persistence";

/// Fires off the committer thread, once a batch's read sets have been checked
/// against the write log snapshot and before the committer takes the result
/// back.
pub const AFTER_COMMIT_PRE_VALIDATE: &str = "after_commit_pre_validate";

/// A commit that has been taken off the queue but whose read set has not been
/// conflict-checked yet.
struct CommitRequest {
    submitted_at: Instant,
    transaction: FinalTransaction,
    result: oneshot::Sender<anyhow::Result<Timestamp>>,
    write_source: WriteSource,
    parent_trace: Option<SpanContext>,
}

/// What the off-thread conflict check decided about a commit request.
enum PreValidation {
    Passed {
        request: CommitRequest,
        /// A timestamp through which no published write overlaps the read set,
        /// so the committer only has to check `validated_through` to whatever
        /// commit timestamp it chooses
        validated_through: Timestamp,
    },
    Failed {
        request: CommitRequest,
        error: anyhow::Error,
    },
}

/// Conflict-check a batch of commit requests against `snapshot`, returning how
/// far into the write log they have been validated
fn pre_validate_batch(
    snapshot: &WriteLogSnapshot,
    batch: Vec<CommitRequest>,
) -> Vec<PreValidation> {
    let max_ts = snapshot.max_ts();
    batch
        .into_iter()
        .map(|request| {
            let begin_ts = *request.transaction.begin_timestamp;
            // A transaction that began at or after the snapshot has nothing to
            // clear here; leave the whole range to the committer.
            if begin_ts >= max_ts {
                return PreValidation::Passed {
                    request,
                    validated_through: begin_ts,
                };
            }
            match snapshot.is_stale(request.transaction.reads.read_set(), begin_ts, max_ts) {
                Ok(None) => PreValidation::Passed {
                    request,
                    validated_through: max_ts,
                },
                Ok(Some(conflicting_read)) => {
                    let error = conflicting_read.into_error(
                        &request.transaction.table_mapping,
                        &request.transaction.component_registry,
                        &request.write_source,
                    );
                    PreValidation::Failed { request, error }
                },
                Err(error) => PreValidation::Failed { request, error },
            }
        })
        .collect()
}

pub struct Committer<RT: Runtime> {
    // Writes of commits that have a timestamp but have not finished writing to
    // persistence, checked for conflicts alongside `log`.
    pending_writes: PendingWrites,
    // External log of writes for subscriptions.
    log: LogWriter,

    snapshot_manager: Writer<SnapshotManager>,
    persistence: Arc<dyn Persistence>,
    runtime: RT,
    deployment_name: String,

    last_assigned_ts: Timestamp,
    persistence_writes: FuturesOrdered<BoxFuture<'static, anyhow::Result<PersistenceWrite>>>,

    write_batcher: WriteBatcher,
    _write_batcher_handle: AbortOnDropHandle<anyhow::Result<()>>,

    retention_validator: Arc<dyn RetentionValidator>,
    virtual_system_mapping: VirtualSystemMapping,

    user_documents_size_gauge: Subgauge,
    concurrent_commits_gauge: Subgauge,
    awaiting_pre_validation_commits_gauge: Subgauge,
    pre_validating_commits_gauge: Subgauge,

    index_cache_handle: IndexCacheHandle,
}

impl<RT: Runtime> Committer<RT> {
    pub(crate) fn start(
        log: LogWriter,
        snapshot_manager: Writer<SnapshotManager>,
        persistence: Arc<dyn Persistence>,
        runtime: RT,
        retention_validator: Arc<dyn RetentionValidator>,
        shutdown: ShutdownSignal,
        virtual_system_mapping: VirtualSystemMapping,
        index_cache_handle: IndexCacheHandle,
        deployment_name: String,
    ) -> CommitterClient {
        let persistence_reader = persistence.reader();
        let conflict_checker = PendingWrites::new();
        let (tx, rx) = mpsc::channel(*COMMITTER_QUEUE_SIZE);
        let snapshot_reader = snapshot_manager.reader();
        let (write_batcher, write_batcher_handle) = WriteBatcher::start(
            runtime.clone(),
            persistence.clone(),
            WriteBatcherConfig::from_knobs(),
        );
        let committer = Self {
            pending_writes: conflict_checker,
            log,
            snapshot_manager,
            persistence,
            runtime: runtime.clone(),
            last_assigned_ts: Timestamp::MIN,
            persistence_writes: FuturesOrdered::new(),
            write_batcher,
            _write_batcher_handle: write_batcher_handle,
            retention_validator: retention_validator.clone(),
            virtual_system_mapping,
            user_documents_size_gauge: user_documents_size_subgauge(),
            concurrent_commits_gauge: concurrent_commits_subgauge(),
            awaiting_pre_validation_commits_gauge:
                metrics::awaiting_pre_validation_commits_subgauge(),
            pre_validating_commits_gauge: metrics::pre_validating_commits_subgauge(),
            index_cache_handle,
            deployment_name,
        };
        let handle = runtime.spawn("committer", async move {
            if let Err(err) = committer.go(rx).await {
                // Committer hit a fatal error. This should only happen if a
                // persistence write fails or in case of unrecoverable logic
                // errors.
                shutdown.signal(err);
                tracing::error!("Shutting down committer");
            }
        });
        CommitterClient {
            handle: Arc::new(Mutex::new(handle)),
            sender: tx,
            persistence_reader,
            retention_validator,
            snapshot_reader,
            search_flusher_wake: SearchFlusherWakeSignals::new(),
        }
    }

    async fn go(mut self, mut rx: mpsc::Receiver<CommitterMessage>) -> anyhow::Result<()> {
        let mut last_bumped_repeatable_ts = self.runtime.monotonic_now();
        // Assume there were commits just before the backend restarted, so first do a
        // quick bump.
        // None means a bump is ongoing. Avoid parallel bumps in case they
        // commit out of order and regress the repeatable timestamp.
        let mut next_bump_wait = Some(*MAX_REPEATABLE_TIMESTAMP_COMMIT_DELAY);
        // A bump can intentionally stop before a pending commit. If that commit
        // publishes before the bump does, it needs another short-delay bump rather
        // than the normal idle-period bump.
        let mut needs_follow_up_bump = false;

        // This span starts when a commit comes back from pre-validation and ends
        // with that same commit getting published.
        let mut committer_span = None;
        // Keep a monotonically increasing id to keep track of honeycomb traces
        // Each commit_id tracks a single write to persistence, from the time the commit
        // message is received until the time the commit has been published. We skip
        // read-only transactions or commits that fail to validate.
        let mut commit_id = 0;
        // Keep track of the commit_id that is currently being traced.
        let mut span_commit_id = None;
        // Commits taken off the queue and awaiting conflict checking. Messages
        // that arrive while every pre-validation lane is busy accumulate here
        // and go out as the next batch, so batches grow with load.
        let mut awaiting_pre_validation: VecDeque<CommitRequest> = VecDeque::new();
        // Batches come back in whatever order they finish: commit timestamps are
        // assigned after pre-validation, so arrival order carries no meaning here.
        let mut pre_validations: FuturesUnordered<
            BoxFuture<'static, anyhow::Result<Vec<PreValidation>>>,
        > = FuturesUnordered::new();
        let mut pre_validating = 0;
        // When admission last paused, if it is still paused. A pause is
        // recorded once, when it ends.
        let mut admission_paused_since = None;
        loop {
            while !awaiting_pre_validation.is_empty()
                && pre_validations.len() < *COMMITTER_MAX_CONCURRENT_PRE_VALIDATIONS
            {
                let take = awaiting_pre_validation
                    .len()
                    .min(*COMMITTER_MAX_PRE_VALIDATE_BATCH_SIZE);
                let batch: Vec<_> = awaiting_pre_validation.drain(..take).collect();
                pre_validating += batch.len();
                pre_validations.push(self.start_pre_validation(batch));
            }
            let bump_fut = if let Some(wait) = &next_bump_wait {
                Either::Left(
                    self.runtime
                        .wait(wait.saturating_sub(last_bumped_repeatable_ts.elapsed())),
                )
            } else {
                Either::Right(std::future::pending())
            };
            // While the persistence-write pipeline is at capacity, pause
            // admitting messages. Commits that have left the queue but not yet
            // reached `persistence_writes` count against the cap too.
            let commits_in_flight =
                self.persistence_writes.len() + awaiting_pre_validation.len() + pre_validating;
            self.concurrent_commits_gauge.set(commits_in_flight as i64);
            self.awaiting_pre_validation_commits_gauge
                .set(awaiting_pre_validation.len() as i64);
            self.pre_validating_commits_gauge.set(pre_validating as i64);
            let recv_fut = if commits_in_flight < (*COMMITTER_MAX_CONCURRENT_COMMITS).max(1) {
                if let Some(paused_at) = admission_paused_since.take() {
                    metrics::log_commit_admission_pause(self.runtime.monotonic_now() - paused_at);
                }
                Either::Left(rx.recv())
            } else {
                if admission_paused_since.is_none() {
                    admission_paused_since = Some(self.runtime.monotonic_now());
                }
                Either::Right(std::future::pending())
            };
            select_biased! {
                _ = bump_fut.fuse() => {
                    let committer_span = committer_span.get_or_insert_with(|| {
                        span_commit_id = Some(commit_id);
                        get_sampled_span(&self.deployment_name, "bump_max_repeatable", &mut self.runtime.rng())
                    });
                    // Advance the repeatable read timestamp so non-leaders can
                    // establish a recent repeatable snapshot.
                    next_bump_wait = None;
                    needs_follow_up_bump = false;
                    let (tx, _rx) = oneshot::channel();
                    self.bump_max_repeatable_ts(tx, commit_id, committer_span);
                    commit_id += 1;
                    last_bumped_repeatable_ts = self.runtime.monotonic_now();
                },
                result = self.persistence_writes.select_next_some() => {
                    let pending_commit =
                        result.context("Write failed. Unsure if transaction committed to disk.")?;
                    let pending_commit_id = pending_commit.commit_id();
                    match pending_commit {
                        PersistenceWrite::Commit {
                            pending_write,
                            commit_timer,
                            result,
                            parent_trace,
                            write_volume,
                            index_key_writes,
                            ..
                        } => {
                            let publish_commit_span = make_committer_span(
                                "Committer::publish_commit",
                                committer_span.as_ref(),
                                parent_trace,
                            );
                            let _guard = publish_commit_span.set_local_parent();
                            let commit_ts = pending_write.must_commit_ts();
                            self.publish_commit(pending_write, write_volume, index_key_writes);
                            let _ = result.send(Ok(commit_ts));

                            // When we next get free cycles and there is no ongoing bump,
                            // bump max_repeatable_ts so followers can read this commit.
                            if next_bump_wait.is_some() {
                                next_bump_wait = Some(*MAX_REPEATABLE_TIMESTAMP_COMMIT_DELAY);
                            } else {
                                needs_follow_up_bump = true;
                            }
                            commit_timer.finish();
                        },
                        PersistenceWrite::MaxRepeatableTimestamp {
                            new_max_repeatable,
                            timer,
                            result,
                            ..
                        } => {
                            let span = committer_span
                                .as_ref()
                                .map(|root| {
                                    Span::enter_with_parent("publish_max_repeatable_ts", root)
                                })
                                .unwrap_or_else(Span::noop);
                            let _guard = span.set_local_parent();
                            self.publish_max_repeatable_ts(new_max_repeatable)?;
                            next_bump_wait = Some(if needs_follow_up_bump {
                                *MAX_REPEATABLE_TIMESTAMP_COMMIT_DELAY
                            } else {
                                let base_period = *MAX_REPEATABLE_TIMESTAMP_IDLE_FREQUENCY;
                                self.runtime
                                    .rng()
                                    .random_range(base_period..base_period * 2)
                            });
                            let _ = result.send(new_max_repeatable);
                            drop(timer);
                        },
                    }
                    // Report the trace if it is longer than the threshold
                    if let Some(id) = span_commit_id
                        && id == pending_commit_id
                        && let Some(span) = committer_span.take()
                        && span.elapsed() < Some(*COMMIT_TRACE_THRESHOLD) {
                        span.cancel();
                    }
                },
                batch = pre_validations.select_next_some() => {
                    let batch = batch?;
                    pre_validating -= batch.len();
                    for pre_validated in batch {
                        let (request, validated_through) = match pre_validated {
                            PreValidation::Passed { request, validated_through } => {
                                (request, validated_through)
                            },
                            PreValidation::Failed { request, error } => {
                                let CommitRequest {
                                    submitted_at,
                                    transaction,
                                    result,
                                    ..
                                } = request;
                                // A commit failed off-thread never reaches
                                // `start_commit`, which is where a commit records these.
                                // `commit_timer` is labeled `status=error` unless
                                // `finish()` is called, so dropping it records the
                                // failure.
                                let _commit_timer = metrics::commit_timer();
                                metrics::log_commit_dequeued(submitted_at.elapsed());
                                metrics::log_write_tx(&transaction);
                                // Release the read set before replying. See
                                // `CommitterClient::_commit`.
                                drop(transaction);
                                let _ = result.send(Err(error));
                                continue;
                            },
                        };
                        let CommitRequest {
                            submitted_at,
                            transaction,
                            result,
                            write_source,
                            parent_trace,
                        } = request;
                        let committer_span_ref = committer_span.get_or_insert_with(|| {
                            span_commit_id = Some(commit_id);
                            get_sampled_span(&self.deployment_name, "commit", &mut self.runtime.rng())
                        });
                        let time_in_queue = submitted_at.elapsed();
                        let start_commit_span = make_committer_span(
                            "handle_commit_message",
                            Some(committer_span_ref),
                            parent_trace,
                        ).with_property(|| {
                            (
                                "time_in_queue_ms",
                                format!("{}", time_in_queue.as_secs_f64() * 1000.0),
                            )
                        });
                        let _guard = start_commit_span.set_local_parent();
                        metrics::log_commit_dequeued(time_in_queue);
                        if let Some(persistence_write_future) = self.start_commit(
                            transaction,
                            result,
                            write_source,
                            parent_trace,
                            commit_id,
                            committer_span_ref,
                            validated_through,
                            submitted_at,
                        ) {
                            self.persistence_writes.push_back(persistence_write_future);
                            commit_id += 1;
                        } else if span_commit_id == Some(commit_id) {
                            // If the span_commit_id is the same as the commit_id, that means we
                            // created a root span in this block
                            // and it didn't get incremented, so it's not a write to persistence
                            // and we should not trace it.
                            // We also need to reset the span_commit_id and committer_span.
                            drop(_guard);
                            drop(start_commit_span);
                            committer_span_ref.cancel();
                            committer_span = None;
                            span_commit_id = None;
                        }
                    }
                },
                maybe_message = recv_fut.fuse() => {
                    match maybe_message {
                        None => {
                            tracing::info!(
                                "All clients have gone away, shutting down committer..."
                            );
                            return Ok(());
                        },
                        Some(CommitterMessage::Commit {
                            submitted_at,
                            transaction,
                            result,
                            write_source,
                            parent_trace,
                        }) => {
                            awaiting_pre_validation.push_back(CommitRequest {
                                submitted_at,
                                transaction,
                                result,
                                write_source,
                                parent_trace,
                            });
                        },
                        Some(CommitterMessage::FinishTextAndVectorBootstrap {
                            bootstrapped_indexes,
                            bootstrap_ts,
                            result,
                        }) => {
                            self.finish_search_and_vector_bootstrap(
                                bootstrapped_indexes,
                                bootstrap_ts,
                                result,
                            )
                            .await;
                        },
                        Some(CommitterMessage::FinishTableSummaryBootstrap { result }) => {
                            self.finish_table_summary_bootstrap(result).await;
                        },
                        Some(CommitterMessage::LoadIndexesIntoMemory { tables, result }) => {
                            let response = self.load_indexes_into_memory(tables).await;
                            let _ = result.send(response);
                        },
                    }
                },
            }
        }
    }

    async fn update_indexes_since_bootstrap(
        BootstrappedSearchIndexes {
            text_index_manager,
            vector_index_manager,
            tables_with_indexes,
        }: &mut BootstrappedSearchIndexes,
        bootstrap_ts: Timestamp,
        persistence: RepeatablePersistence,
        registry: &IndexRegistry,
    ) -> anyhow::Result<()> {
        let _timer = bootstrap_update_timer();
        anyhow::ensure!(
            !text_index_manager.is_bootstrapping(),
            "Trying to update search index while it's still bootstrapping"
        );
        anyhow::ensure!(
            !vector_index_manager.is_bootstrapping(),
            "Trying to update vector index while it's still bootstrapping"
        );
        let range = TimestampRange::new((Bound::Excluded(bootstrap_ts), Bound::Unbounded));

        let revision_stream =
            stream_revision_pairs_for_indexes(tables_with_indexes, &persistence, range);
        futures::pin_mut!(revision_stream);

        let mut num_revisions = 0;
        let mut total_size = 0;
        while let Some(revision_pair) = revision_stream.try_next().await? {
            num_revisions += 1;
            total_size += revision_pair.document().map(|d| d.size()).unwrap_or(0);
            text_index_manager.update(
                registry,
                revision_pair.prev_document(),
                revision_pair.document(),
                WriteTimestamp::Committed(revision_pair.ts()),
            )?;
            vector_index_manager.update(
                registry,
                revision_pair.prev_document(),
                revision_pair.document(),
                WriteTimestamp::Committed(revision_pair.ts()),
            )?;
        }
        finish_bootstrap_update(num_revisions, total_size);
        Ok(())
    }

    async fn finish_search_and_vector_bootstrap(
        &mut self,
        mut bootstrapped_indexes: BootstrappedSearchIndexes,
        bootstrap_ts: RepeatableTimestamp,
        result: oneshot::Sender<anyhow::Result<()>>,
    ) {
        let (last_snapshot, latest_ts) = {
            let snapshot_manager = self.snapshot_manager.read();
            (
                snapshot_manager.latest_snapshot(),
                snapshot_manager.latest_ts(),
            )
        };
        if latest_ts > bootstrap_ts {
            let repeatable_persistence = RepeatablePersistence::new(
                self.persistence.reader(),
                latest_ts,
                self.retention_validator.clone(),
            );

            let res = Self::update_indexes_since_bootstrap(
                &mut bootstrapped_indexes,
                *bootstrap_ts,
                repeatable_persistence,
                &last_snapshot.index_registry,
            )
            .await;
            if res.is_err() {
                let _ = result.send(res);
                return;
            }
        }
        // Committer is currently single threaded, so commits should be blocked until we
        // finish and the timestamp shouldn't be able to advance.
        let mut snapshot_manager = self.snapshot_manager.write();
        if latest_ts != snapshot_manager.latest_ts() {
            panic!("Snapshots were changed concurrently during commit?");
        }
        snapshot_manager.overwrite_last_snapshot_text_and_vector_indexes(
            bootstrapped_indexes.text_index_manager,
            bootstrapped_indexes.vector_index_manager,
            &mut self.pending_writes,
        );

        tracing::info!("Committed backfilled vector indexes");
        let _ = result.send(Ok(()));
    }

    async fn finish_table_summary_bootstrap(
        &mut self,
        result: oneshot::Sender<anyhow::Result<()>>,
    ) {
        let _timer = table_summary_finish_bootstrap_timer();
        let latest_ts = {
            let snapshot_manager = self.snapshot_manager.read();
            snapshot_manager.latest_ts()
        };
        // This gets called by the TableSummaryWorker when it has successfully
        // checkpointed a TableSummarySnapshot.
        // Walk any changes since the last checkpoint, and update the snapshot manager
        // with the new TableSummarySnapshot.
        let bootstrap_result = table_summary::bootstrap(
            self.runtime.clone(),
            self.persistence.reader(),
            self.retention_validator.clone(),
            latest_ts,
            table_summary::BootstrapKind::FromCheckpoint,
        )
        .await;
        let (table_summary_snapshot, _) = match bootstrap_result {
            Ok(res) => res,
            Err(err) => {
                let _ = result.send(Err(err));
                return;
            },
        };
        // Committer is currently single threaded, so commits should be blocked until we
        // finish and the timestamp shouldn't be able to advance.
        let mut snapshot_manager = self.snapshot_manager.write();
        if latest_ts != snapshot_manager.latest_ts() {
            panic!("Snapshots were changed concurrently during commit?");
        }
        if let Err(e) = snapshot_manager
            .overwrite_last_snapshot_table_counts(table_summary_snapshot, &mut self.pending_writes)
        {
            let _ = result.send(Err(e));
            return;
        }
        tracing::info!("Bootstrapped table summaries at ts {}", latest_ts);
        let _ = result.send(Ok(()));
    }

    // This blocks the committer and loads the in-memory indexes for the latest
    // snapshot in memory. A potential further improvement is to pick a base
    // timestamp and load the indexes at that timestamp outside of the committer.
    // The committer can then replay recent writes to derive the latest in-memory
    // indexes. This would either need to do another database query for the log
    // or rely on the write log to not have trimmed the base timestamp yet.
    async fn load_indexes_into_memory(
        &mut self,
        tables: BTreeSet<TableName>,
    ) -> anyhow::Result<()> {
        let (last_snapshot, latest_ts) = {
            let snapshot_manager = self.snapshot_manager.read();
            (
                snapshot_manager.latest_snapshot(),
                snapshot_manager.latest_ts(),
            )
        };

        let repeatable_persistence = RepeatablePersistence::new(
            self.persistence.reader(),
            latest_ts,
            self.retention_validator.clone(),
        );
        let mut in_memory_indexes = last_snapshot.in_memory_indexes.clone();
        in_memory_indexes
            .load_enabled_for_tables(
                &last_snapshot.index_registry,
                last_snapshot.table_mapping(),
                &repeatable_persistence.read_snapshot(latest_ts)?,
                &tables,
            )
            .await?;

        // Committer is currently single threaded, so commits should be blocked until we
        // finish and the timestamp shouldn't be able to advance.
        let mut snapshot_manager = self.snapshot_manager.write();
        if latest_ts != snapshot_manager.latest_ts() {
            panic!("Snapshots were changed concurrently during commit?");
        }
        snapshot_manager
            .overwrite_last_snapshot_in_memory_indexes(in_memory_indexes, &mut self.pending_writes);

        tracing::info!("Loaded indexes into memory");
        Ok(())
    }

    fn bump_max_repeatable_ts(
        &mut self,
        result: oneshot::Sender<Timestamp>,
        commit_id: usize,
        root_span: &Span,
    ) {
        let timer = metrics::bump_repeatable_ts_timer();
        // next_max_repeatable_ts bumps the last_assigned_ts, so all future commits on
        // this committer will be after new_max_repeatable.
        let new_max_repeatable = self
            .next_max_repeatable_ts()
            .expect("new_max_repeatable should exist");
        let persistence = self.persistence.clone();
        let span = Span::enter_with_parent("bump_max_repeatable_ts", root_span);
        let runtime = self.runtime.clone();
        self.persistence_writes.push_back(
            async move {
                // The MaxRepeatableTimestamp persistence global ensures all future
                // commits on future leaders will be after new_max_repeatable, and followers
                // can know this timestamp is repeatable.

                // If we fail to bump the timestamp, we'll backoff and retry
                // which will block the committer from making forward progress until we
                // succceed.  We don't want to kill the committer and reload the
                // instance if we can avoid it, as that would exacerbate any
                // load-related issues.
                let mut backoff = Backoff::new(
                    *INITIAL_PERSISTENCE_WRITES_BACKOFF,
                    *MAX_PERSISTENCE_WRITES_BACKOFF,
                );
                loop {
                    match persistence
                        .write_persistence_global(
                            PersistenceGlobalKey::MaxRepeatableTimestamp,
                            new_max_repeatable.into(),
                        )
                        .await
                    {
                        Ok(()) => {
                            backoff.reset();
                            runtime
                                .pause_client()
                                .wait(AFTER_MAX_REPEATABLE_TIMESTAMP_PERSISTENCE)
                                .await;
                            break;
                        },
                        Err(mut e) => {
                            let delay = backoff.fail(&mut runtime.rng());
                            report_error(&mut e).await;
                            tracing::error!(
                                "Failed to bump max repeatable timestamp, retrying after {:.2}s",
                                delay.as_secs_f32()
                            );
                            runtime.wait(delay).await;
                            continue;
                        },
                    }
                }
                Ok(PersistenceWrite::MaxRepeatableTimestamp {
                    new_max_repeatable,
                    timer,
                    result,
                    commit_id,
                })
            }
            .in_span(span)
            .boxed(),
        );
    }

    fn publish_max_repeatable_ts(&mut self, new_max_repeatable: Timestamp) -> anyhow::Result<()> {
        // Bump the latest snapshot in snapshot_manager so reads on this leader
        // can know this timestamp is repeatable.
        let mut snapshot_manager = self.snapshot_manager.write();
        if snapshot_manager.bump_persisted_max_repeatable_ts(new_max_repeatable)? {
            self.log
                .append(new_max_repeatable, IndexKeyWrites::empty(), || {});
        }
        Ok(())
    }

    /// First, check that it's valid to apply this transaction in-memory. If it
    /// passes validation, we can rebase the transaction to a new timestamp
    /// if other transactions have committed.
    #[fastrace::trace]
    fn validate_commit(
        &mut self,
        transaction: FinalTransaction,
        write_source: WriteSource,
        validated_through: Timestamp,
    ) -> anyhow::Result<ValidatedCommit> {
        let commit_ts = self.next_commit_ts()?;
        metrics::log_commit_validation_window(commit_ts.secs_since_f64(validated_through));
        let timer = metrics::commit_is_stale_timer();
        if let Some(conflicting_read) =
            self.commit_has_conflict(transaction.reads.read_set(), validated_through, commit_ts)?
        {
            anyhow::bail!(conflicting_read.into_error(
                &transaction.table_mapping,
                &transaction.component_registry,
                &write_source
            ));
        }
        timer.finish();

        // The commit timestamp is now known: produce the final updates, with
        // every unresolved commit timestamp in the write set replaced by it,
        // so nothing unresolved reaches persistence or the durable indexes.
        // Concrete documents move through without cloning.
        let mut ordered_updates: Vec<DocumentUpdateWithPrevTs> = transaction
            .writes
            .into_coalesced_writes()
            .map(|update| Arc::unwrap_or_clone(update).resolve(commit_ts))
            .collect::<anyhow::Result<_>>()?;
        // The updates are ordered using table_dependency_sort_key,
        // which is the same order they should be applied to database metadata
        // and index data structures
        let bootstrap_tables = BootstrapTableIds::new(&transaction.table_mapping);
        ordered_updates.sort_by_key(|update| {
            table_dependency_sort_key(
                bootstrap_tables,
                InternalDocumentId::from(update.id),
                update.new_document.as_ref(),
            )
        });

        let (document_writes, index_writes, snapshot) =
            self.compute_writes(commit_ts, &ordered_updates)?;

        // Append the updates to pending_writes, so future conflicting commits
        // will fail the `commit_has_conflict` check above, even before
        // this transaction writes to persistence or is visible to reads. Note that
        // this can cause theoretical false conflicts, where transaction has a conflict
        // with another one, and the latter never ended up committing. This
        // should be very rare, and false positives are acceptable by design.
        let timer = metrics::pending_writes_append_timer();
        let packed_updates: OrderedDocumentWrites = ordered_updates
            .into_iter()
            .map(|update| PackedDocumentUpdate::pack(&update))
            .collect();
        let index_registry = snapshot.index_registry.clone();
        let pending_write = self.pending_writes.push_back(
            commit_ts,
            packed_updates.clone(),
            write_source,
            snapshot,
        );
        drop(timer);

        Ok(ValidatedCommit {
            index_writes,
            document_writes,
            pending_write,
            ordered_updates: packed_updates,
            index_registry,
        })
    }

    #[fastrace::trace]
    fn compute_writes(
        &self,
        commit_ts: Timestamp,
        ordered_updates: &[DocumentUpdateWithPrevTs],
    ) -> anyhow::Result<(
        Vec<ValidatedDocumentWrite>,
        Vec<DatabaseIndexUpdate>,
        Snapshot,
    )> {
        let timer = metrics::commit_prepare_writes_timer();
        let mut document_writes = Vec::new();
        let mut index_writes = Vec::new();
        // We have to compute the new snapshot from the latest pending snapshot in case
        // there are pending writes that need to be included. We have already
        // checked for conflicts, so the latest pending snapshot must
        // have the same tables and indexes as the base snapshot and the final
        // publishing snapshot. Therefore index writes can be computed from the
        // latest pending snapshot.
        let mut latest_pending_snapshot = self
            .pending_writes
            .latest_snapshot()
            .unwrap_or_else(|| self.snapshot_manager.read().latest_snapshot());
        for document_update in ordered_updates.iter() {
            let (updates, vector_index_write_size, text_index_write_size) =
                latest_pending_snapshot.update(document_update, commit_ts)?;
            let num_index_writes = updates.len();
            index_writes.extend(updates);
            document_writes.push(ValidatedDocumentWrite {
                commit_ts,
                id: document_update.id.into(),
                write: document_update.new_document.clone(),
                num_index_writes,
                vector_index_write_size,
                text_index_write_size,
                prev_ts: document_update.old_document.as_ref().map(|&(_, ts)| ts),
            });
        }
        timer.finish();
        Ok((document_writes, index_writes, latest_pending_snapshot))
    }

    #[fastrace::trace]
    /// `validated_through` is a timestamp in `[begin_ts, commit_ts]` through
    /// which `pre_validate_batch` already found no conflicting write in the
    /// published log, so only `(validated_through, commit_ts]` is left to check
    /// there.
    fn commit_has_conflict(
        &self,
        reads: &ReadSet,
        validated_through: Timestamp,
        commit_ts: Timestamp,
    ) -> anyhow::Result<Option<ConflictingReadWithWriteSource>> {
        if let Some(conflicting_read) = self.log.is_stale(reads, validated_through, commit_ts)? {
            return Ok(Some(conflicting_read));
        }
        if let Some(conflicting_read) = self.pending_writes.is_stale(reads) {
            return Ok(Some(conflicting_read));
        }
        Ok(None)
    }

    /// Records usage, converts the validated writes into persistence entries,
    /// and submits them to the write batcher, which writes them to persistence
    /// as part of a batch, retrying on transient errors.
    ///
    /// The batcher acking the write is the commit point of a transaction: if
    /// this succeeds, the transaction must be published and made visible. If we
    /// are unsure whether the write went through, we crash the process and
    /// recover from whatever has been written to persistence.
    async fn track_and_write_to_persistence(
        write_batcher: WriteBatcher,
        usage_tracking: FunctionUsageTracker,
        commit_ts: Timestamp,
        index_writes: Vec<DatabaseIndexUpdate>,
        document_writes: Vec<ValidatedDocumentWrite>,
        ordered_updates: OrderedDocumentWrites,
        index_registry: IndexRegistry,
        table_mapping: TableMapping,
        component_registry: ComponentRegistry,
        virtual_system_mapping: VirtualSystemMapping,
        write_source: WriteSource,
        submitted_at: Instant,
    ) -> anyhow::Result<(WriteVolume, IndexKeyWrites)> {
        Self::track_commit(
            usage_tracking,
            &index_writes,
            &document_writes,
            &table_mapping,
            &component_registry,
            &virtual_system_mapping,
        );

        let index_key_writes = index_keys_from_full_documents(
            commit_ts,
            &ordered_updates,
            &write_source,
            &index_registry,
        );

        let mut write_bytes: u64 = 0;
        let document_writes = document_writes
            .into_iter()
            .map(|write| {
                let entry = DocumentLogEntry {
                    ts: write.commit_ts,
                    id: write.id,
                    value: write.write,
                    prev_ts: write.prev_ts,
                };
                write_bytes += entry.size();
                entry
            })
            .collect_vec();
        let mut index_write_metrics = metrics::CommitIndexWriteMetrics::default();
        let index_writes = index_writes
            .into_iter()
            .map(|update| {
                let entry = PersistenceIndexEntry::from_index_update(commit_ts, &update);
                index_write_metrics.record(&entry, update.is_system_index);
                write_bytes += entry.size();
                entry
            })
            .collect_vec();
        metrics::commit_index_rows(index_writes.len() as u64);
        let write_volume = WriteVolume {
            bytes: write_bytes,
            rows: (document_writes.len() + index_writes.len()) as u64,
        };
        write_batcher
            .write(document_writes, index_writes, write_bytes)
            .await
            .with_context(|| format!("Commit ({write_source:?}) failed to write to persistence"))?;
        metrics::log_commit_persisted(submitted_at.elapsed());
        index_write_metrics.log();
        Ok((write_volume, index_key_writes))
    }

    /// After writing the new rows to persistence, mark the commit as complete
    /// and allow the updated rows to be read by other transactions.
    #[fastrace::trace]
    fn publish_commit(
        &mut self,
        pending_write: PendingWriteHandle,
        write_volume: WriteVolume,
        writes: IndexKeyWrites,
    ) {
        let apply_timer = metrics::commit_apply_timer();
        let commit_ts = pending_write.must_commit_ts();

        let (ordered_updates, new_snapshot) = self.pending_writes.pop_first(pending_write);

        // Write transaction state at the commit ts to the document store.
        metrics::commit_rows(ordered_updates.len() as u64);
        drop(ordered_updates);

        metrics::write_log_commit_bytes(write_volume.bytes as usize);

        let timer = metrics::write_log_append_timer();
        let index_registry = &new_snapshot.index_registry;
        let database_writes = writes.database.clone();
        let apply_writes_callback = || {
            self.index_cache_handle.apply_writes(
                database_writes.iter().map(|(index_name, writes_in_index)| {
                    (index_name, writes_in_index.index_updates.iter())
                }),
                &|index_name| {
                    index_registry
                        .get_enabled(index_name)
                        .map(|index| index.id())
                },
            )
        };
        self.log.append(commit_ts, writes, apply_writes_callback);
        drop(timer);

        if let Some(table_counts) = new_snapshot.table_counts.as_ref() {
            metrics::log_num_keys(table_counts.num_user_documents);
            metrics::log_user_table_documents_size(table_counts.user_tables_size);
            self.user_documents_size_gauge
                .set(table_counts.user_docs_size as i64);
        }

        // Publish the new version of our database metadata and the index.
        let mut snapshot_manager = self.snapshot_manager.write();
        snapshot_manager.push(commit_ts, new_snapshot, write_volume);

        apply_timer.finish();
    }

    /// Hand every commit request's read set to the blocking pool to be checked
    /// against the published write log, so the committer thread only has to
    /// check the small suffix that lands afterwards.
    fn start_pre_validation(
        &self,
        batch: Vec<CommitRequest>,
    ) -> BoxFuture<'static, anyhow::Result<Vec<PreValidation>>> {
        metrics::log_commit_pre_validate_batch_size(batch.len());
        let snapshot = self.log.snapshot();
        let pause_client = self.runtime.pause_client();
        async move {
            tokio_spawn("commit_pre_validate", async move {
                let pre_validated = {
                    let _timer = metrics::commit_pre_validate_timer();
                    block_in_place(|| pre_validate_batch(&snapshot, batch))
                };
                pause_client.wait(AFTER_COMMIT_PRE_VALIDATE).await;
                pre_validated
            })
            .await
            .context("Commit pre-validation panicked")
        }
        .boxed()
    }

    #[fastrace::trace]
    /// Returns a future to add to the pending_writes queue, if the commit
    /// should be written.
    fn start_commit(
        &mut self,
        transaction: FinalTransaction,
        result: oneshot::Sender<anyhow::Result<Timestamp>>,
        write_source: WriteSource,
        parent_trace: Option<SpanContext>,
        commit_id: usize,
        root_span: &Span,
        validated_through: Timestamp,
        submitted_at: Instant,
    ) -> Option<BoxFuture<'static, anyhow::Result<PersistenceWrite>>> {
        let commit_timer = metrics::commit_timer();
        metrics::log_write_tx(&transaction);

        // Trace if the transaction has a lot of read intervals.
        if transaction.reads.num_intervals() > *TRANSACTION_WARN_READ_SET_INTERVALS {
            tracing::warn!(
                "Transaction with write source {write_source:?} has {} read intervals: {}",
                transaction.reads.num_intervals(),
                transaction.reads.top_three_intervals()
            );
        }

        let table_mapping = transaction.table_mapping.clone();
        let component_registry = transaction.component_registry.clone();
        let usage_tracking = transaction.usage_tracker.clone();
        let ValidatedCommit {
            index_writes,
            document_writes,
            pending_write,
            ordered_updates,
            index_registry,
        } = match block_in_place(|| {
            self.validate_commit(transaction, write_source.clone(), validated_through)
        }) {
            Ok(v) => v,
            Err(e) => {
                let _ = result.send(Err(e));
                return None;
            },
        };

        // necessary because this value is moved
        let write_batcher = self.write_batcher.clone();
        let outer_span = make_committer_span(
            "Committer::persistence_writes_future",
            Some(root_span),
            parent_trace,
        );
        let pause_client = self.runtime.pause_client();
        let virtual_system_mapping = self.virtual_system_mapping.clone();
        let commit_ts = pending_write.must_commit_ts();
        Some(
            async move {
                let name = "Commit::write_to_persistence";
                // Spawn a new task for tracking commit, serializing writes, and writing to
                // persistence so it doesn't block the committer thread
                let handle = AbortOnDropHandle::new(tokio_spawn(
                    name,
                    Self::track_and_write_to_persistence(
                        write_batcher,
                        usage_tracking,
                        commit_ts,
                        index_writes,
                        document_writes,
                        ordered_updates,
                        index_registry,
                        table_mapping,
                        component_registry,
                        virtual_system_mapping,
                        write_source,
                        submitted_at,
                    )
                    .in_span(Span::enter_with_local_parent(name)),
                ));
                let (write_volume, index_key_writes) = handle.await??;
                pause_client.wait(AFTER_PENDING_WRITE_SNAPSHOT).await;
                Ok(PersistenceWrite::Commit {
                    pending_write,
                    commit_timer,
                    result,
                    parent_trace,
                    commit_id,
                    write_volume,
                    index_key_writes,
                })
            }
            .in_span(outer_span)
            .boxed(),
        )
    }

    #[fastrace::trace]
    fn track_commit(
        usage_tracker: FunctionUsageTracker,
        index_writes: &[DatabaseIndexUpdate],
        document_writes: &Vec<ValidatedDocumentWrite>,
        table_mapping: &TableMapping,
        component_registry: &ComponentRegistry,
        virtual_system_mapping: &VirtualSystemMapping,
    ) {
        for index_write in index_writes {
            if let DatabaseIndexValue::NonClustered(doc) = index_write.value {
                let tablet_id = doc.tablet_id;
                let Ok(table_namespace) = table_mapping.tablet_namespace(tablet_id) else {
                    continue;
                };
                let component_id = ComponentId::from(table_namespace);
                let component_path = component_registry
                    .get_component_path(component_id, &mut TransactionReadSet::new())
                    // It's possible that the component gets deleted in this transaction. In that case, miscount the usage as root.
                    .unwrap_or(ComponentPath::root());
                if let Ok(table_name) = table_mapping.tablet_name(tablet_id) {
                    let index_key_size = index_write.key.size() as u64;
                    // Index metadata is never a vector
                    // Database bandwidth for index writes
                    usage_tracker.track_database_ingress(
                        component_path.clone(),
                        &table_name,
                        index_key_size,
                        // Exclude indexes on system tables or reserved system indexes on user
                        // tables
                        table_name.is_system() || index_write.is_system_index,
                    );
                    usage_tracker.track_database_ingress_v2(
                        component_path.clone(),
                        &table_name,
                        index_key_size,
                        table_name.is_system() || index_write.is_system_index,
                    );
                    if let Some(virtual_table_name) =
                        virtual_system_mapping.associated_virtual_table_name(&table_name)
                    {
                        usage_tracker.track_virtual_table_ingress(
                            component_path,
                            virtual_table_name,
                            index_key_size,
                        );
                    }
                }
            }
        }
        for validated_write in document_writes {
            let ValidatedDocumentWrite {
                id,
                write: document,
                num_index_writes,
                vector_index_write_size,
                text_index_write_size,
                ..
            } = validated_write;
            let tablet_id = id.table();
            let Ok(table_namespace) = table_mapping.tablet_namespace(tablet_id) else {
                continue;
            };
            let component_id = ComponentId::from(table_namespace);
            let component_path = component_registry
                .get_component_path(component_id, &mut TransactionReadSet::new())
                // It's possible that the component gets deleted in this transaction. In that case, miscount the usage as root.
                .unwrap_or(ComponentPath::root());
            let Ok(table_name) = table_mapping.tablet_name(tablet_id) else {
                continue;
            };
            // Deletions are counted as document writes even though they contribute no
            // ingress bytes.
            usage_tracker.track_database_ingress_rows(
                component_path.clone(),
                &table_name,
                1,
                table_name.is_system(),
            );
            usage_tracker.track_database_ingress_index_rows(
                *num_index_writes as u64,
                table_name.is_system(),
            );
            if let Some(document) = document {
                let document_write_size = document.size();
                // Database bandwidth for document writes
                usage_tracker.track_database_ingress(
                    component_path.clone(),
                    &table_name,
                    document_write_size as u64,
                    table_name.is_system(),
                );
                usage_tracker.track_database_ingress_v2(
                    component_path.clone(),
                    &table_name,
                    document_write_size as u64,
                    table_name.is_system(),
                );
                if let Some(virtual_table_name) =
                    virtual_system_mapping.associated_virtual_table_name(&table_name)
                {
                    usage_tracker.track_virtual_table_ingress(
                        component_path.clone(),
                        virtual_table_name,
                        document_write_size as u64,
                    );
                }
                if vector_index_write_size.0 > 0 {
                    usage_tracker.track_vector_ingress(
                        component_path.clone(),
                        &table_name,
                        document_write_size as u64,
                        vector_index_write_size.0,
                        table_name.is_system(),
                    );
                }
                if text_index_write_size.0 > 0 {
                    usage_tracker.track_text_ingress(
                        component_path.clone(),
                        &table_name,
                        text_index_write_size.0,
                        table_name.is_system(),
                    );
                }
            }
        }
    }

    fn next_commit_ts(&mut self) -> anyhow::Result<Timestamp> {
        let _timer = next_commit_ts_seconds();
        let latest_ts = self.snapshot_manager.read().latest_ts();
        let max = cmp::max(
            latest_ts.succ()?,
            cmp::max(
                self.runtime.generate_timestamp()?,
                self.last_assigned_ts.succ()?,
            ),
        );
        self.last_assigned_ts = max;
        Ok(max)
    }

    fn next_max_repeatable_ts(&mut self) -> anyhow::Result<Timestamp> {
        if let Some(min_pending) = self.pending_writes.min_ts() {
            // If there's a pending write, push max_repeatable_ts to be right
            // before the pending write, so followers can choose recent
            // timestamps but can't read at the timestamp of the pending write.
            anyhow::ensure!(min_pending <= self.last_assigned_ts);
            min_pending.pred()
        } else {
            // If there are no pending writes, bump last_assigned_ts and write
            // to persistence and snapshot manager as a commit would.
            self.next_commit_ts()
        }
    }
}

/// `root_span` is the cross-commit span maintained by the committer, while
/// `parent_trace` points to the span used by the commit client
fn make_committer_span(
    span_name: &'static str,
    root_span: Option<&Span>,
    parent_trace: Option<SpanContext>,
) -> Span {
    if let Some(parent) = parent_trace {
        let span = Span::root(span_name, parent);
        if let Some(root_span) = root_span
            && let Some(ctx) = SpanContext::from_span(root_span)
        {
            // this link may go nowhere if root_span is cancelled, but that's
            // harmless
            span.add_link(ctx);
        }
        span
    } else if let Some(root_span) = root_span {
        // `root_span` is tail-sampled, so it's important to use
        // `enter_with_parent` (not `Span::root`) to ensure that all the spans
        // are submitted or cancelled together
        Span::enter_with_parent(span_name, root_span)
    } else {
        Span::noop()
    }
}

struct ValidatedDocumentWrite {
    commit_ts: Timestamp,
    id: InternalDocumentId,
    write: Option<ResolvedDocument>,
    /// Entries this write adds to or removes from the table's database
    /// indexes, including the built-in `by_id` and `by_creation_time`.
    num_index_writes: usize,
    vector_index_write_size: VectorIndexWriteSize,
    text_index_write_size: TextIndexWriteSize,
    prev_ts: Option<Timestamp>,
}

#[derive(Clone)]
pub struct CommitterClient {
    handle: Arc<Mutex<Box<dyn SpawnHandle>>>,
    sender: mpsc::Sender<CommitterMessage>,
    persistence_reader: Arc<dyn PersistenceReader>,
    retention_validator: Arc<dyn RetentionValidator>,
    snapshot_reader: Reader<SnapshotManager>,
    search_flusher_wake: SearchFlusherWakeSignals,
}

impl CommitterClient {
    pub fn search_flusher_wake(&self) -> &SearchFlusherWakeSignals {
        &self.search_flusher_wake
    }

    pub async fn finish_search_and_vector_bootstrap(
        &self,
        bootstrapped_indexes: BootstrappedSearchIndexes,
        bootstrap_ts: RepeatableTimestamp,
    ) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        let message = CommitterMessage::FinishTextAndVectorBootstrap {
            bootstrapped_indexes,
            bootstrap_ts,
            result: tx,
        };
        self.sender.try_send(message).map_err(|e| match e {
            TrySendError::Full(..) => metrics::committer_full_error().into(),
            TrySendError::Closed(..) => metrics::shutdown_error(),
        })?;
        // The only reason we might fail here if the committer is shutting down.
        rx.await.map_err(|_| metrics::shutdown_error())?
    }

    pub async fn finish_table_summary_bootstrap(&self) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        let message = CommitterMessage::FinishTableSummaryBootstrap { result: tx };
        self.sender.try_send(message).map_err(|e| match e {
            TrySendError::Full(..) => metrics::committer_full_error().into(),
            TrySendError::Closed(..) => metrics::shutdown_error(),
        })?;
        // The only reason we might fail here if the committer is shutting down.
        rx.await.map_err(|_| metrics::shutdown_error())?
    }

    // Tell the committer to load all indexes for the given tables into memory.
    pub async fn load_indexes_into_memory(
        &self,
        tables: BTreeSet<TableName>,
    ) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        let message = CommitterMessage::LoadIndexesIntoMemory { tables, result: tx };
        self.sender.try_send(message).map_err(|e| match e {
            TrySendError::Full(..) => metrics::committer_full_error().into(),
            TrySendError::Closed(..) => metrics::shutdown_error(),
        })?;
        // The only reason we might fail here if the committer is shutting down.
        rx.await.map_err(|_| metrics::shutdown_error())?
    }

    pub fn commit<RT: Runtime>(
        &self,
        transaction: Transaction<RT>,
        write_source: WriteSource,
    ) -> BoxFuture<'_, anyhow::Result<Timestamp>> {
        self._commit(transaction, write_source).boxed()
    }

    #[fastrace::trace]
    async fn _commit<RT: Runtime>(
        &self,
        mut transaction: Transaction<RT>,
        write_source: WriteSource,
    ) -> anyhow::Result<Timestamp> {
        let _timer = metrics::commit_client_timer(transaction.identity());
        transaction.assign_missing_persistence_index_ids().await?;
        self.check_generated_ids(&transaction).await?;
        let rt = transaction.runtime().clone();

        // Finish reading everything from persistence.
        let transaction = transaction.finalize()?;

        if transaction.is_readonly() {
            // If there is nothing to write, skip the roundtrip through the
            // committer. The transaction is serialized at the _begin_ timestamp
            // rather than a freshly allocated one.
            return Ok(*transaction.begin_timestamp);
        }

        // Note that we do a best effort validation for memory index sizes. We
        // use the latest snapshot instead of the transaction base snapshot. This
        // is both more accurate and also avoids pedant hitting transient errors.
        let latest_snapshot = self.snapshot_reader.lock().latest_snapshot();
        transaction.validate_memory_index_sizes(&latest_snapshot, &self.search_flusher_wake)?;

        // Freeing a large read set is expensive, so it should happen on this
        // task rather than the single committer thread. The committer releases
        // its reference before replying, so holding this one until the reply
        // makes this task free the read set.
        let reads = transaction.reads.clone();
        let (tx, rx) = oneshot::channel();
        let message = CommitterMessage::Commit {
            submitted_at: rt.monotonic_now(),
            transaction,
            result: tx,
            write_source,
            parent_trace: SpanContext::current_local_parent(),
        };

        // Waits until the committer has space to send a message, with a timeout.
        // This makes it resilient to the committer being full.
        select_biased! {
            result = self.sender.send(message).fuse() => {
                result.map_err(|_| metrics::shutdown_error())?
            },
            _ = rt.wait(*SEND_COMMIT_MESSAGE_TIMEOUT_MILLIS) => {
                anyhow::bail!(metrics::committer_full_error());
            },
        };

        let Ok(result) = rx.await else {
            anyhow::bail!(metrics::shutdown_error());
        };
        drop(reads);
        if let Err(e) = result {
            return Err(recapture_stacktrace(e).await);
        }
        result
    }

    pub fn shutdown(&self) {
        self.handle.lock().shutdown();
    }

    async fn check_generated_ids<RT: Runtime>(
        &self,
        transaction: &Transaction<RT>,
    ) -> anyhow::Result<()> {
        // Check that none of the DocumentIds generated in this transaction
        // are already in use.
        // We can check at the begin_timestamp+1 because generated_ids are also
        // checked for conflict against all writes after begin_timestamp.
        let ts = transaction.begin_timestamp().succ()?;
        let timer = metrics::commit_id_reuse_timer();
        let generated_ids = transaction.writes.as_flat()?.generated_ids();
        if !generated_ids.is_empty() {
            let repeatable_persistence = RepeatablePersistence::new(
                self.persistence_reader.clone(),
                transaction.begin_timestamp(),
                self.retention_validator.clone(),
            );
            let generated_ids_with_ts: BTreeSet<_> = generated_ids
                .iter()
                .map(|id| (InternalDocumentId::from(*id), ts))
                .collect();
            let mut previous_revisions_of_ids = repeatable_persistence
                .previous_revisions(generated_ids_with_ts)
                .await?;
            if let Some((
                (document_id, _),
                DocumentLogEntry {
                    value: maybe_doc, ..
                },
            )) = previous_revisions_of_ids.pop_first()
            {
                let display_id = generated_ids
                    .iter()
                    .find(|id| InternalDocumentId::from(**id) == document_id)
                    .map(|id| DeveloperDocumentId::from(*id).encode())
                    .unwrap_or(document_id.to_string());
                if maybe_doc.is_none() {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "DocumentDeleted",
                        format!(
                            "Cannot recreate document with _id {display_id} that was deleted. Try \
                             to insert it without an _id or insert into another table."
                        ),
                    ));
                } else {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "DocumentExists",
                        format!(
                            "Cannot create document with _id {display_id} that already exists. \
                             Try to update it with `db.patch` or `db.replace`."
                        ),
                    ));
                }
            }
        }
        timer.finish();
        Ok(())
    }
}

enum CommitterMessage {
    Commit {
        submitted_at: Instant,
        transaction: FinalTransaction,
        result: oneshot::Sender<anyhow::Result<Timestamp>>,
        write_source: WriteSource,
        parent_trace: Option<SpanContext>,
    },
    LoadIndexesIntoMemory {
        tables: BTreeSet<TableName>,
        result: oneshot::Sender<anyhow::Result<()>>,
    },
    FinishTextAndVectorBootstrap {
        bootstrapped_indexes: BootstrappedSearchIndexes,
        bootstrap_ts: RepeatableTimestamp,
        result: oneshot::Sender<anyhow::Result<()>>,
    },
    FinishTableSummaryBootstrap {
        result: oneshot::Sender<anyhow::Result<()>>,
    },
}

// Within a single transaction that writes multiple documents, this is the order
// in which we write them.
//
// Dependencies:
// - _tables table created before other tables.
// - table created before its indexes.
// - indexes created before documents in the table.
// - indexes deleted before other indexes created, in case of naming conflicts.
// - tables deleted before other tables created, in case of naming conflicts.
// - indexes on a table deleted before the table itself.
pub fn table_dependency_sort_key(
    bootstrap_tables: BootstrapTableIds,
    id: InternalDocumentId,
    update: Option<&ResolvedDocument>,
) -> (usize, InternalDocumentId) {
    let table = id.table();
    let sort_key = if table == bootstrap_tables.tables_id.tablet_id {
        match update {
            Some(insertion) => {
                let table_metadata: ParsedDocument<TableMetadata> =
                    insertion.parse().unwrap_or_else(|e| {
                        panic!("Writing invalid TableMetadata {}: {e}", insertion.value().0)
                    });
                match table_metadata.state {
                    TableState::Active => {
                        if &table_metadata.name == &TABLES_TABLE {
                            // In bootstrapping, create _tables table first.
                            2
                        } else {
                            // Create other tables, especially the _index table, next.
                            3
                        }
                    },
                    TableState::Hidden => 3,
                    // Deleting index must come before table deletion,
                    // so we can delete the table.by_id index while the table still exists.
                    TableState::Deleting => 1,
                }
            },
            // Legacy method of deleting _tables, supported here when walking the log.
            None => 1,
        }
    } else if table == bootstrap_tables.index_id.tablet_id {
        if update.is_none() {
            // Index deletes come first, in case one is being deleted and another
            // created with the same name.
            0
        } else {
            4
        }
    } else {
        5
    };
    (sort_key, id)
}

struct ValidatedCommit {
    index_writes: Vec<DatabaseIndexUpdate>,
    document_writes: Vec<ValidatedDocumentWrite>,
    pending_write: PendingWriteHandle,
    ordered_updates: OrderedDocumentWrites,
    index_registry: IndexRegistry,
}
