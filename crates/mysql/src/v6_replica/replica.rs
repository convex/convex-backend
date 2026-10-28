//! A per-deployment worker that applies queued operations to V6 in FIFO order.
use std::{
    sync::{
        atomic::{
            AtomicBool,
            Ordering,
        },
        Arc,
    },
    time::Duration,
};

use common::{
    backoff::Backoff,
    errors::{
        is_db_deadlock_error,
        is_transient_db_error,
        LeaseLostError,
    },
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

enum ReplayQueueMessage<RT: Runtime> {
    Replay {
        // Queued operations keep the writer alive. The worker holds only the
        // receiver, so the channel closes once these and the source are dropped.
        replica: Arc<ReplicaWriter<RT>>,
        operation: ReplayOperation,
        reply: oneshot::Sender<anyhow::Result<u64>>,
    },
    NotifyAfterPriorReplay(oneshot::Sender<()>),
}

pub(crate) struct ReplicaWriter<RT: Runtime> {
    pub(super) v6: Arc<dyn PersistenceTrait>,
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
    queue: mpsc::Sender<ReplayQueueMessage<RT>>,
    pub(super) settings: Settings,
    pub(super) comparison_permits: Arc<Semaphore>,
    writer_state_gauge: Arc<metrics::WriterStateGauge>,
}

impl<RT: Runtime> ReplicaWriter<RT> {
    pub(super) fn new(
        v6: Arc<dyn PersistenceTrait>,
        target: ReplicaTarget<RT>,
        source: ReplicaHandoffState,
        settings: Settings,
        writer_state_gauge: Arc<metrics::WriterStateGauge>,
    ) -> Arc<Self> {
        let (queue, mut receiver) = mpsc::channel(settings.max_queued_jobs);
        let replica = Arc::new(Self {
            v6,
            target,
            cluster_name: source.cluster_name,
            deployment_id: source.deployment_id,
            instance_name: source.instance_name,
            lease_ts: source.lease_ts,
            stopped: AtomicBool::new(false),
            queue,
            comparison_permits: Arc::new(Semaphore::new(settings.max_concurrent_comparisons)),
            settings,
            writer_state_gauge,
        });
        common::runtime::tokio_spawn("v6_replica_worker", async move {
            while let Some(message) = receiver.recv().await {
                match message {
                    ReplayQueueMessage::Replay {
                        replica,
                        operation,
                        reply,
                    } => {
                        let outcome = replica.run(&operation).await;
                        metrics::add_queued(&replica.cluster_name, -1.0);
                        let _ = reply.send(outcome);
                    },
                    ReplayQueueMessage::NotifyAfterPriorReplay(reply) => {
                        let _ = reply.send(());
                    },
                }
            }
        });
        replica
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    /// Call before returning the V5 result so operations issued after that
    /// response enter the queue after this one.
    pub(crate) fn enqueue(
        self: &Arc<Self>,
        operation: ReplayOperation,
    ) -> Option<oneshot::Receiver<anyhow::Result<u64>>> {
        if self.is_stopped() {
            metrics::log_job(&self.cluster_name, metrics::JobOutcome::Skipped);
            return None;
        }
        let (reply, result) = oneshot::channel();
        metrics::add_queued(&self.cluster_name, 1.0);
        if let Err(error) = self.queue.try_send(ReplayQueueMessage::Replay {
            replica: self.clone(),
            operation,
            reply,
        }) {
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

    async fn run(self: &Arc<Self>, operation: &ReplayOperation) -> anyhow::Result<u64> {
        if self.is_stopped() {
            metrics::log_job(&self.cluster_name, metrics::JobOutcome::Skipped);
            return Ok(0);
        }
        let result = self.apply_with_retries(operation).await;
        match &result {
            Ok(_) => metrics::log_job(&self.cluster_name, metrics::JobOutcome::Applied),
            // A newer writer or deletion now controls the replica's lifetime.
            Err(error) if error.is::<LeaseLostError>() => {
                metrics::log_job(&self.cluster_name, metrics::JobOutcome::Skipped);
                if self.stop(metrics::WriterState::Displaced) {
                    tracing::info!(
                        instance_name = %self.instance_name,
                        "V6 replica displaced by another writer"
                    );
                }
            },
            Err(error) => {
                metrics::log_job(&self.cluster_name, metrics::JobOutcome::Failed);
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
        match operation {
            ReplayOperation::Write {
                documents,
                indexes,
                conflict_strategy,
            } => {
                self.v6
                    .write(documents, indexes, *conflict_strategy)
                    .await?;
                Ok(0)
            },
            ReplayOperation::WriteIndexBackfill { entries } => {
                self.v6.write_index_backfill(entries).await?;
                Ok(0)
            },
            ReplayOperation::ReconcileIndexBackfill { index } => {
                self.v6.reconcile_index_backfill(*index).await?;
                Ok(0)
            },
            ReplayOperation::DeleteIndexBackfillMarkers { index } => {
                self.v6.delete_index_backfill_markers_chunk(*index).await
            },
            ReplayOperation::DeleteDocuments { documents } => {
                Ok(self.v6.delete(documents.clone()).await? as u64)
            },
            ReplayOperation::DeleteTabletDocuments {
                tablet_id,
                chunk_size,
            } => Ok(self
                .v6
                .delete_tablet_documents(*tablet_id, *chunk_size)
                .await? as u64),
            ReplayOperation::WritePersistenceGlobal { key, value } => {
                self.v6
                    .write_persistence_global(*key, value.clone())
                    .await?;
                Ok(0)
            },
        }
    }

    /// Acknowledges earlier operations after success, failure, or skipping.
    pub(crate) async fn wait_for_prior_replay(&self, deadline: Option<Instant>) -> bool {
        let notification = async {
            let (reply, completed) = oneshot::channel();
            self.queue
                .send(ReplayQueueMessage::NotifyAfterPriorReplay(reply))
                .await
                .ok()?;
            completed.await.ok()
        };
        match deadline {
            Some(deadline) => matches!(
                tokio::time::timeout_at(deadline, notification).await,
                Ok(Some(()))
            ),
            None => notification.await.is_some(),
        }
    }

    /// Retention retries marker discovery periodically, so a stale result is
    /// safe. Marker deletions queue behind the reconciliation they depend on.
    pub(crate) async fn index_backfill_marker_indexes(self: &Arc<Self>) -> Vec<PersistenceIndexId> {
        if self.is_stopped() {
            return Vec::new();
        }
        match self.v6.index_backfill_marker_indexes().await {
            Ok(indexes) => indexes,
            Err(error) => {
                self.retire(metrics::RetireReason::JobFailed, &error);
                Vec::new()
            },
        }
    }

    /// The caller must finish all source calls before starting replica
    /// shutdown.
    pub(crate) async fn close(self: &Arc<Self>) {
        if self.is_stopped() {
            return;
        }
        let result = tokio::time::timeout(self.settings.apply_wait, async {
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
            },
            _ => self.retire(
                metrics::RetireReason::UncleanHandoff,
                &anyhow::anyhow!("replica shutdown did not complete"),
            ),
        }
    }

    fn stop(&self, state: metrics::WriterState) -> bool {
        if self.stopped.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.writer_state_gauge.set(state);
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
