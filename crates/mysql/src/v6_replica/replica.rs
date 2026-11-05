//! A per-deployment worker that applies queued operations to V6 in FIFO order,
//! merging consecutive writes into one transaction.
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{
            AtomicBool,
            AtomicU64,
            Ordering,
        },
        Arc,
        OnceLock,
    },
    time::Duration,
};

use anyhow::Context as _;
use common::{
    backoff::Backoff,
    errors::{
        is_db_deadlock_error,
        is_transient_db_error,
        LeaseLostError,
    },
    index::IndexKeyBytes,
    persistence::{
        ConflictStrategy,
        DocumentLogEntry,
        IndexBackfillEntry,
        Persistence as PersistenceTrait,
        PersistenceGlobalKey,
        PersistenceIndexEntry,
    },
    runtime::Runtime,
    types::{
        DeploymentId,
        IndexRef,
        PersistenceIndexId,
        Timestamp,
    },
    value::{
        InternalDocumentId,
        TabletId,
    },
};
use serde_json::Value as JsonValue;
use tokio::{
    sync::{
        mpsc,
        oneshot,
        watch,
        Semaphore,
    },
    time::Instant,
};

use super::{
    config::Settings,
    metrics,
    state::{
        self,
        ReplicaHandoffState,
    },
    ReplicaTarget,
};
use crate::v5;

pub(crate) enum ReplayOperation {
    Write {
        documents: Vec<DocumentLogEntry>,
        indexes: Vec<PersistenceIndexEntry>,
        conflict_strategy: ConflictStrategy,
    },
    WriteIndexBackfill {
        entries: Vec<IndexBackfillEntry>,
    },
    ReconcileIndexBackfill {
        index: PersistenceIndexId,
    },
    DeleteIndexBackfillMarkers {
        index: PersistenceIndexId,
    },
    DeleteDocuments {
        documents: Vec<(Timestamp, InternalDocumentId)>,
    },
    DeleteTabletDocuments {
        tablet_id: TabletId,
        chunk_size: usize,
    },
    WritePersistenceGlobal {
        key: PersistenceGlobalKey,
        value: JsonValue,
    },
}

type Reply = oneshot::Sender<anyhow::Result<u64>>;

/// A queued operation. Queued operations keep the writer alive: the worker
/// holds only the receiver, so the channel closes once these and the source
/// are dropped.
struct ReplayJob<RT: Runtime> {
    replica: Arc<ReplicaWriter<RT>>,
    operation: ReplayOperation,
    reply: Reply,
}

/// Applies jobs in order, merging consecutive writes into one V6
/// transaction, until the last sender is dropped. `applied` publishes the
/// number of jobs settled so far, in queue order, for `wait_for_prior_replay`.
async fn replay_queue<RT: Runtime>(
    mut receiver: mpsc::Receiver<ReplayJob<RT>>,
    applied: watch::Sender<u64>,
) {
    let mut applied_jobs = 0;
    let mut next = receiver.recv().await;
    while let Some(job) = next.take() {
        // Scoped so the worker holds no writer reference while it waits for
        // the next job, letting the channel close once the source is dropped.
        {
            let ReplayJob {
                replica,
                operation,
                reply,
            } = job;
            let (operation, replies) = match operation {
                ReplayOperation::Write {
                    documents,
                    indexes,
                    conflict_strategy,
                } => {
                    let mut batch = WriteBatch::new(documents, indexes, conflict_strategy, reply);
                    next = batch.fill(&mut receiver, replica.settings.max_merged_writes);
                    batch.finish()
                },
                operation => (operation, vec![reply]),
            };
            let jobs = replies.len() as u64;
            let outcome = replica.run(&operation, jobs).await;
            metrics::add_queued(&replica.cluster_name, -(jobs as f64));
            send_outcome(replies, outcome);
            applied_jobs += jobs;
            applied.send_replace(applied_jobs);
        }
        if next.is_none() {
            next = receiver.recv().await;
        }
    }
}

/// Consecutive write jobs replayed in one V6 transaction. Their index keys
/// are disjoint: V6 changes a key's stored entry in place, so an entry naming
/// its predecessor needs that predecessor committed by an earlier transaction.
struct WriteBatch {
    documents: Vec<DocumentLogEntry>,
    indexes: Vec<PersistenceIndexEntry>,
    conflict_strategy: ConflictStrategy,
    keys: BTreeSet<(IndexRef, IndexKeyBytes)>,
    replies: Vec<Reply>,
}

impl WriteBatch {
    fn new(
        documents: Vec<DocumentLogEntry>,
        indexes: Vec<PersistenceIndexEntry>,
        conflict_strategy: ConflictStrategy,
        reply: Reply,
    ) -> Self {
        Self {
            keys: index_keys(&indexes).collect(),
            documents,
            indexes,
            conflict_strategy,
            replies: vec![reply],
        }
    }

    /// Takes queued jobs while they merge and `max_jobs` allows, returning the
    /// first job that must wait for the next transaction.
    fn fill<RT: Runtime>(
        &mut self,
        receiver: &mut mpsc::Receiver<ReplayJob<RT>>,
        max_jobs: usize,
    ) -> Option<ReplayJob<RT>> {
        while self.replies.len() < max_jobs {
            let ReplayJob {
                replica,
                operation,
                reply,
            } = receiver.try_recv().ok()?;
            match self.merge(operation) {
                Ok(()) => self.replies.push(reply),
                Err(operation) => {
                    return Some(ReplayJob {
                        replica,
                        operation,
                        reply,
                    })
                },
            }
        }
        None
    }

    fn merge(&mut self, operation: ReplayOperation) -> Result<(), ReplayOperation> {
        match operation {
            ReplayOperation::Write {
                documents,
                indexes,
                conflict_strategy,
            } if conflict_strategy == self.conflict_strategy
                && self.documents.len() + documents.len() <= crate::MAX_INSERT_SIZE
                && index_keys(&indexes).all(|key| !self.keys.contains(&key)) =>
            {
                self.keys.extend(index_keys(&indexes));
                self.documents.extend(documents);
                self.indexes.extend(indexes);
                Ok(())
            },
            operation => Err(operation),
        }
    }

    fn finish(self) -> (ReplayOperation, Vec<Reply>) {
        let operation = ReplayOperation::Write {
            documents: self.documents,
            indexes: self.indexes,
            conflict_strategy: self.conflict_strategy,
        };
        (operation, self.replies)
    }
}

fn index_keys(
    indexes: &[PersistenceIndexEntry],
) -> impl Iterator<Item = (IndexRef, IndexKeyBytes)> + '_ {
    indexes.iter().map(|entry| (entry.index, entry.key.clone()))
}

/// Merged jobs share one outcome; the error itself goes to the last reply and
/// the others get its text.
fn send_outcome(replies: Vec<Reply>, outcome: anyhow::Result<u64>) {
    let mut replies = replies.into_iter();
    let last = replies.next_back();
    for reply in replies {
        let _ = reply.send(
            outcome
                .as_ref()
                .copied()
                .map_err(|error| anyhow::anyhow!("{error:#}")),
        );
    }
    if let Some(last) = last {
        let _ = last.send(outcome);
    }
}

pub(crate) struct ReplicaWriter<RT: Runtime> {
    /// Set once this writer owns the registration; a pending writer queues
    /// operations until then.
    v6: OnceLock<Arc<dyn PersistenceTrait>>,
    /// Releases the worker once `v6` is set. Dropped on stop, so a pending
    /// worker skips its queue instead.
    activation: parking_lot::Mutex<Option<oneshot::Sender<()>>>,
    pub(super) target: ReplicaTarget<RT>,
    /// Metrics use the source V5 cluster's name.
    pub(super) cluster_name: String,
    pub(super) deployment_id: DeploymentId,
    pub(super) instance_name: String,
    /// Source V5 lease timestamp, checked when updating the handoff state to
    /// protect a newer writer's state from stale updates.
    pub(super) lease_ts: i64,
    /// Stops replay and comparisons permanently on retirement, lease loss,
    /// or clean shutdown.
    stopped: AtomicBool,
    queue: mpsc::Sender<ReplayJob<RT>>,
    /// Serializes `queue.try_send` with the `enqueued` increment, so the count
    /// never exceeds the jobs the worker will receive and a count read after a
    /// source call finished includes that call's job.
    enqueue_order: parking_lot::Mutex<()>,
    /// Jobs the queue has accepted.
    enqueued: AtomicU64,
    /// Jobs the worker has settled, in queue order.
    applied: watch::Receiver<u64>,
    /// Serializes closes from lease loss and shutdown; the second finds the
    /// writer stopped.
    close_lock: tokio::sync::Mutex<()>,
    /// Persistence globals this writer wrote to the source. Their queued
    /// replay supersedes the previous writer's values, so handoff
    /// verification skips them.
    rewritten_globals: parking_lot::Mutex<BTreeSet<PersistenceGlobalKey>>,
    pub(super) settings: Settings,
    /// Held by sampled reads from sampling through outcome logging.
    pub(super) comparison_permits: Arc<Semaphore>,
    /// Held only while a comparison reads V6.
    pub(super) replica_read_permits: Semaphore,
    writer_state_gauge: Arc<metrics::WriterStateGauge>,
}

impl<RT: Runtime> ReplicaWriter<RT> {
    pub(super) fn new(
        v6: Option<Arc<dyn PersistenceTrait>>,
        target: ReplicaTarget<RT>,
        source: ReplicaHandoffState,
        settings: Settings,
        writer_state_gauge: Arc<metrics::WriterStateGauge>,
    ) -> Arc<Self> {
        let (queue, receiver) = mpsc::channel(settings.max_queued_jobs);
        let (applied_sender, applied) = watch::channel(0);
        let (activate, activated) = oneshot::channel();
        let replica = Arc::new(Self {
            v6: OnceLock::new(),
            activation: parking_lot::Mutex::new(Some(activate)),
            target,
            cluster_name: source.cluster_name,
            deployment_id: source.deployment_id,
            instance_name: source.instance_name,
            lease_ts: source.lease_ts,
            stopped: AtomicBool::new(false),
            queue,
            enqueue_order: parking_lot::Mutex::new(()),
            enqueued: AtomicU64::new(0),
            applied,
            close_lock: tokio::sync::Mutex::new(()),
            rewritten_globals: parking_lot::Mutex::new(BTreeSet::new()),
            comparison_permits: Arc::new(Semaphore::new(settings.max_concurrent_comparisons)),
            replica_read_permits: Semaphore::new(settings.max_concurrent_replica_reads),
            settings,
            writer_state_gauge,
        });
        match v6 {
            Some(v6) => replica.activate(v6),
            None => replica
                .writer_state_gauge
                .set(metrics::WriterState::Pending),
        }
        common::runtime::tokio_spawn("v6_replica_worker", async move {
            // A pending writer holds its jobs until the handoff resolves. A
            // dropped sender means it stopped, and the loop skips them.
            let _ = activated.await;
            replay_queue(receiver, applied_sender).await;
        });
        replica
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    pub(crate) fn is_active(&self) -> bool {
        self.v6.get().is_some() && !self.is_stopped()
    }

    pub(super) fn v6(&self) -> anyhow::Result<&Arc<dyn PersistenceTrait>> {
        self.v6.get().context("the V6 replica is not active")
    }

    fn activate(&self, v6: Arc<dyn PersistenceTrait>) {
        let mut activation = self.activation.lock();
        if self.is_stopped() || self.v6.set(v6).is_err() {
            return;
        }
        self.writer_state_gauge.set(metrics::WriterState::Ready);
        if let Some(activate) = activation.take() {
            let _ = activate.send(());
        }
    }

    /// Call before returning the V5 result so operations issued after that
    /// response enter the queue after this one.
    pub(crate) fn enqueue(
        self: &Arc<Self>,
        operation: ReplayOperation,
    ) -> Option<oneshot::Receiver<anyhow::Result<u64>>> {
        if self.is_stopped() {
            metrics::log_jobs(&self.cluster_name, metrics::JobOutcome::Skipped, 1);
            return None;
        }
        let (reply, result) = oneshot::channel();
        metrics::add_queued(&self.cluster_name, 1.0);
        let sent = {
            let _order = self.enqueue_order.lock();
            let sent = self.queue.try_send(ReplayJob {
                replica: self.clone(),
                operation,
                reply,
            });
            if sent.is_ok() {
                self.enqueued.fetch_add(1, Ordering::SeqCst);
            }
            sent
        };
        if let Err(error) = sent {
            metrics::add_queued(&self.cluster_name, -1.0);
            let reason = match error {
                mpsc::error::TrySendError::Full(_) => metrics::RetireReason::QueueFull,
                mpsc::error::TrySendError::Closed(_) => metrics::RetireReason::JobFailed,
            };
            self.retire(
                reason,
                &anyhow::anyhow!("replica worker could not accept a job"),
            );
            return None;
        }
        Some(result)
    }

    pub(crate) fn note_global_write(&self, key: PersistenceGlobalKey) {
        self.rewritten_globals.lock().insert(key);
    }

    /// `jobs` is the number of source calls `operation` replays.
    async fn run(self: &Arc<Self>, operation: &ReplayOperation, jobs: u64) -> anyhow::Result<u64> {
        if self.is_stopped() {
            metrics::log_jobs(&self.cluster_name, metrics::JobOutcome::Skipped, jobs);
            return Ok(0);
        }
        let result = self.apply_with_retries(operation).await;
        match &result {
            Ok(_) => {
                metrics::log_jobs(&self.cluster_name, metrics::JobOutcome::Applied, jobs);
                if matches!(operation, ReplayOperation::Write { .. }) {
                    metrics::log_write_batch(&self.cluster_name, jobs);
                }
            },
            // A newer writer or deletion now controls the replica's lifetime.
            Err(error) if error.is::<LeaseLostError>() => {
                metrics::log_jobs(&self.cluster_name, metrics::JobOutcome::Skipped, jobs);
                if self.stop(metrics::WriterState::Displaced) {
                    tracing::info!(
                        instance_name = %self.instance_name,
                        "V6 replica displaced by another writer"
                    );
                }
            },
            Err(error) => {
                metrics::log_jobs(&self.cluster_name, metrics::JobOutcome::Failed, jobs);
                self.retire(metrics::RetireReason::JobFailed, error);
            },
        }
        result
    }

    /// An ambiguous commit may have written keys already; a duplicate-key
    /// error on retry retires the replica.
    async fn apply_with_retries(&self, operation: &ReplayOperation) -> anyhow::Result<u64> {
        let deadline = Instant::now() + self.settings.job_retry;
        let mut backoff = Backoff::new(Duration::from_millis(50), Duration::from_secs(2));
        loop {
            match self.apply(operation).await {
                Err(error)
                    if (is_transient_db_error(&error) || is_db_deadlock_error(&error))
                        && Instant::now() < deadline
                        && !self.is_stopped() =>
                {
                    tracing::warn!("Retrying a V6 replica job: {error:#}");
                    let delay = backoff.fail(&mut rand::rng());
                    tokio::time::sleep(delay).await;
                },
                result => return result,
            }
        }
    }

    async fn apply(&self, operation: &ReplayOperation) -> anyhow::Result<u64> {
        let v6 = self.v6()?;
        match operation {
            ReplayOperation::Write {
                documents,
                indexes,
                conflict_strategy,
            } => {
                v6.write(documents, indexes, *conflict_strategy).await?;
                Ok(0)
            },
            ReplayOperation::WriteIndexBackfill { entries } => {
                v6.write_index_backfill(entries).await?;
                Ok(0)
            },
            ReplayOperation::ReconcileIndexBackfill { index } => {
                v6.reconcile_index_backfill(*index).await?;
                Ok(0)
            },
            ReplayOperation::DeleteIndexBackfillMarkers { index } => {
                v6.delete_index_backfill_markers_chunk(*index).await
            },
            ReplayOperation::DeleteDocuments { documents } => {
                Ok(v6.delete(documents.clone()).await? as u64)
            },
            ReplayOperation::DeleteTabletDocuments {
                tablet_id,
                chunk_size,
            } => Ok(v6.delete_tablet_documents(*tablet_id, *chunk_size).await? as u64),
            ReplayOperation::WritePersistenceGlobal { key, value } => {
                v6.write_persistence_global(*key, value.clone()).await?;
                Ok(0)
            },
        }
    }

    /// Resolves once every job queued before this call has been applied,
    /// failed, or skipped, or returns false when the worker exited or
    /// `deadline` passed first. Waiting reads the worker's progress counter,
    /// so writes queued around a wait still merge into one transaction.
    pub(crate) async fn wait_for_prior_replay(&self, deadline: Option<Instant>) -> bool {
        let target = self.enqueued.load(Ordering::SeqCst);
        let mut applied = self.applied.clone();
        let settled = async move { applied.wait_for(|applied| *applied >= target).await.is_ok() };
        match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, settled)
                .await
                .unwrap_or(false),
            None => settled.await,
        }
    }

    /// Retention retries marker discovery periodically, so a stale result is
    /// safe and a failed read is only reported: a read cannot leave replay
    /// incomplete. Marker deletions queue behind the reconciliation they
    /// depend on.
    pub(crate) async fn index_backfill_marker_indexes(&self) -> Vec<PersistenceIndexId> {
        let Some(v6) = self.v6.get().filter(|_| !self.is_stopped()) else {
            return Vec::new();
        };
        match v6.index_backfill_marker_indexes().await {
            Ok(indexes) => indexes,
            Err(error) => self.report_marker_read_failure(error),
        }
    }

    fn report_marker_read_failure(&self, error: anyhow::Error) -> Vec<PersistenceIndexId> {
        metrics::log_marker_read_failure(&self.cluster_name);
        tracing::warn!(
            instance_name = %self.instance_name,
            "Could not list V6 replica backfill markers: {error:#}"
        );
        Vec::new()
    }

    /// The caller must finish all source calls before starting replica
    /// shutdown.
    pub(crate) async fn close(self: &Arc<Self>) {
        let _closing = self.close_lock.lock().await;
        if self.is_stopped() {
            return;
        }
        if !self.is_active() {
            // Queued writes were never applied, so no certificate is possible.
            self.retire(
                metrics::RetireReason::UncleanHandoff,
                &anyhow::anyhow!("shut down before the handoff resolved"),
            );
            return;
        }
        let result = tokio::time::timeout(self.settings.shutdown_wait, async {
            anyhow::ensure!(
                self.wait_for_prior_replay(None).await,
                "replica queue closed"
            );
            anyhow::ensure!(!self.is_stopped(), "replica stopped during shutdown");
            state::record_clean_shutdown(
                &self.target.pool,
                &self.target.db_name,
                self.deployment_id,
                self.lease_ts,
            )
            .await
        })
        .await;
        match result {
            Ok(Ok(())) => {
                self.stop(metrics::WriterState::Closed);
                tracing::info!(
                    instance_name = %self.instance_name,
                    "V6 replica certified a clean shutdown"
                );
            },
            _ => self.retire(
                metrics::RetireReason::UncleanHandoff,
                &anyhow::anyhow!("replica shutdown did not complete"),
            ),
        }
    }

    fn stop(&self, state: metrics::WriterState) -> bool {
        let mut activation = self.activation.lock();
        if self.stopped.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.writer_state_gauge.set(state);
        activation.take();
        true
    }

    /// Stopping immediately keeps `clean_shutdown` false even if persisting
    /// retirement fails; the next writer will then retire the replica.
    pub(crate) fn retire(self: &Arc<Self>, reason: metrics::RetireReason, error: &anyhow::Error) {
        if !self.stop(metrics::WriterState::Retired) {
            return;
        }
        metrics::log_retirement(&self.cluster_name, reason);
        tracing::warn!(
            instance_name = %self.instance_name,
            "Retiring the V6 replica ({}): {error:#}",
            <&'static str>::from(reason)
        );
        let replica = self.clone();
        common::runtime::tokio_spawn("v6_replica_retire", async move {
            if let Err(error) = replica.record_retirement().await {
                tracing::warn!("Could not record V6 replica retirement: {error:#}");
            }
        });
    }

    async fn record_retirement(&self) -> anyhow::Result<()> {
        if state::retire(
            &self.target.pool,
            &self.target.db_name,
            self.deployment_id,
            self.lease_ts,
        )
        .await?
        {
            super::spawn_delete_data(self.target.clone(), self.deployment_id);
        }
        Ok(())
    }

    pub(crate) async fn is_active_owner(&self) -> anyhow::Result<bool> {
        if self.is_stopped() {
            return Ok(false);
        }
        let state =
            state::read(&self.target.pool, &self.target.db_name, self.deployment_id).await?;
        Ok(
            state.is_some_and(|state| !state.retired && state.lease_ts == self.lease_ts)
                && !self.is_stopped(),
        )
    }
}

/// The source as of this writer taking its lease, for judging a replica whose
/// previous writer left no certificate.
pub(super) struct HandoffVerification<RT: Runtime> {
    pub(super) v5: Arc<v5::Persistence<RT>>,
    /// The source's latest timestamp before this writer's first commit. The
    /// previous writer's replay is complete once the replica reaches it.
    pub(super) boundary_ts: Option<Timestamp>,
}

impl<RT: Runtime> HandoffVerification<RT> {
    /// Replay is FIFO, so a replica at the boundary applied every earlier
    /// document write. Persistence globals carry no timestamp and are compared
    /// directly, except globals this writer has since rewritten. Index backfill
    /// chunks and reconciliation need no check: the backfill worker commits its
    /// cursor and its scan-complete boundary as documents only after those
    /// calls return, so work whose replay was lost with its process has no
    /// checkpoint behind it and the resumed backfill redoes it.
    async fn replica_matches_source(
        &self,
        v6: &Arc<dyn PersistenceTrait>,
        rewritten_globals: &BTreeSet<PersistenceGlobalKey>,
    ) -> anyhow::Result<bool> {
        let (source, replica) = (self.v5.reader(), v6.reader());
        if replica.max_ts().await? != self.boundary_ts {
            return Ok(false);
        }
        for key in enum_iterator::all::<PersistenceGlobalKey>() {
            if rewritten_globals.contains(&key) {
                continue;
            }
            let (source_value, replica_value) = tokio::try_join!(
                source.get_persistence_global(key),
                replica.get_persistence_global(key)
            )?;
            if source_value != replica_value {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

enum Handoff {
    /// The previous writer drained its replay and certified its shutdown.
    Certified,
    /// No certificate arrived in time, or another stale writer registered.
    Expired,
    /// A newer writer owns the registration.
    Superseded,
    /// Another process retired the replica.
    Retired,
    Stopped,
}

impl<RT: Runtime> ReplicaWriter<RT> {
    /// Rolling updates start the new conductor before stopping the old one,
    /// so the previous writer is usually still draining when this writer
    /// loads. Queue writes until it certifies a clean shutdown, then take the
    /// registration. Without a certificate in time, compare the replica with
    /// the source as of the lease change and resume only when they match.
    pub(super) fn resolve_handoff(
        self: &Arc<Self>,
        source: ReplicaHandoffState,
        previous_lease_ts: i64,
        verification: HandoffVerification<RT>,
    ) {
        let replica = self.clone();
        common::runtime::tokio_spawn("v6_replica_handoff", async move {
            match replica.await_certificate(previous_lease_ts).await {
                Handoff::Certified => {
                    metrics::log_handoff(&replica.cluster_name, metrics::HandoffOutcome::Certified);
                    replica
                        .claim(&source, state::Resume::IfCertified, None)
                        .await
                },
                Handoff::Expired => replica.verify_and_claim(&source, verification).await,
                Handoff::Superseded => {
                    metrics::log_handoff(
                        &replica.cluster_name,
                        metrics::HandoffOutcome::Superseded,
                    );
                    if replica.stop(metrics::WriterState::Displaced) {
                        tracing::info!(
                            instance_name = %replica.instance_name,
                            "V6 replica displaced by another writer"
                        );
                    }
                },
                Handoff::Retired => {
                    metrics::log_handoff(&replica.cluster_name, metrics::HandoffOutcome::Retired);
                    replica.stop(metrics::WriterState::Retired);
                },
                Handoff::Stopped => {},
            }
        });
    }

    /// Taking the V6 lease first ends any replay the previous writer is still
    /// running, so the comparison sees the replica's final state.
    async fn verify_and_claim(
        self: &Arc<Self>,
        source: &ReplicaHandoffState,
        verification: HandoffVerification<RT>,
    ) {
        let grace = self.settings.handoff_grace;
        let no_certificate =
            format!("the previous writer did not certify a clean shutdown within {grace:?}");
        let v6 = match super::connect_v6(&self.target, source).await {
            Ok(v6) => v6,
            Err(error) => {
                metrics::log_handoff(&self.cluster_name, metrics::HandoffOutcome::Expired);
                self.retire(
                    metrics::RetireReason::UncleanHandoff,
                    &error.context(format!(
                        "{no_certificate} and the replica could not be opened for verification"
                    )),
                );
                return;
            },
        };
        let rewritten_globals = self.rewritten_globals.lock().clone();
        match verification
            .replica_matches_source(&v6, &rewritten_globals)
            .await
        {
            Ok(true) => {
                metrics::log_handoff(&self.cluster_name, metrics::HandoffOutcome::Verified);
                tracing::info!(
                    instance_name = %self.instance_name,
                    "V6 replica matches the source at the handoff; resuming without a certificate"
                );
                self.claim(source, state::Resume::Verified, Some(v6)).await
            },
            Ok(false) => {
                metrics::log_handoff(&self.cluster_name, metrics::HandoffOutcome::Expired);
                self.retire(
                    metrics::RetireReason::UncleanHandoff,
                    &anyhow::anyhow!("{no_certificate} and the replica differs from the source"),
                )
            },
            Err(error) => {
                metrics::log_handoff(&self.cluster_name, metrics::HandoffOutcome::Expired);
                self.retire(
                    metrics::RetireReason::UncleanHandoff,
                    &error.context(format!(
                        "{no_certificate} and the replica could not be compared with the source"
                    )),
                )
            },
        }
    }

    async fn await_certificate(&self, previous_lease_ts: i64) -> Handoff {
        let deadline = Instant::now() + self.settings.handoff_grace;
        let poll = (self.settings.handoff_grace / 20)
            .clamp(Duration::from_millis(50), Duration::from_secs(2));
        loop {
            tokio::time::sleep(poll).await;
            if self.is_stopped() {
                return Handoff::Stopped;
            }
            match state::read(&self.target.pool, &self.target.db_name, self.deployment_id).await {
                Ok(Some(current)) if current.retired => return Handoff::Retired,
                Ok(Some(current)) if current.lease_ts >= self.lease_ts => {
                    return Handoff::Superseded;
                },
                Ok(Some(current)) if current.lease_ts != previous_lease_ts => {
                    return Handoff::Expired;
                },
                Ok(Some(current)) if current.clean_shutdown => return Handoff::Certified,
                Ok(Some(_)) => {},
                Ok(None) => return Handoff::Expired,
                Err(error) => {
                    tracing::warn!("Could not read the V6 replica handoff state: {error:#}");
                },
            }
            if Instant::now() >= deadline {
                return Handoff::Expired;
            }
        }
    }

    /// `v6` is a connection already holding the V6 lease, whose registration
    /// only needs confirming.
    async fn claim(
        self: &Arc<Self>,
        source: &ReplicaHandoffState,
        resume: state::Resume,
        v6: Option<Arc<dyn PersistenceTrait>>,
    ) {
        match state::register(&self.target.pool, &self.target.db_name, source, resume).await {
            Ok(true) => {
                let opened = match v6 {
                    Some(v6) => super::confirm_ownership(&self.target, source)
                        .await
                        .map(|()| v6),
                    None => super::open_v6(&self.target, source).await,
                };
                match opened {
                    Ok(v6) => self.activate(v6),
                    Err(error) => self.retire(metrics::RetireReason::InitializationFailed, &error),
                }
            },
            Ok(false) => {
                super::spawn_delete_data(self.target.clone(), self.deployment_id);
                self.retire(
                    metrics::RetireReason::UncleanHandoff,
                    &anyhow::anyhow!("the handoff state changed before this writer registered"),
                );
            },
            Err(error) => self.retire(metrics::RetireReason::UncleanHandoff, &error),
        }
    }
}
