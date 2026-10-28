//! V5 persistence with replay queued after successful source writes.
use std::{
    future::Future,
    sync::{
        atomic::{
            AtomicBool,
            Ordering,
        },
        Arc,
    },
};

use async_trait::async_trait;
use common::{
    persistence::{
        ConflictStrategy,
        DocumentLogEntry,
        IndexBackfillEntry,
        IndexRetentionProgress,
        IndexRetentionRequest,
        Persistence as PersistenceTrait,
        PersistenceGlobalKey,
        PersistenceIndexEntry,
        PersistenceReader,
    },
    runtime::Runtime,
    types::{
        PersistenceIndexId,
        Timestamp,
    },
    value::{
        InternalDocumentId,
        TabletId,
    },
};
use futures::stream::BoxStream;
use serde_json::Value as JsonValue;
use tokio::sync::{
    oneshot,
    RwLock,
    RwLockReadGuard,
};

use super::{
    comparison::ComparingV5Reader,
    metrics,
    replica::{
        ReplayOperation,
        ReplicaWriter,
    },
};
use crate::v5;

pub(crate) struct ReplicatedV5Persistence<RT: Runtime> {
    pub(super) v5: Arc<v5::Persistence<RT>>,
    pub(super) replica: Option<Arc<ReplicaWriter<RT>>>,
    _writer_state_gauge: Arc<metrics::WriterStateGauge>,
    pub(super) closing: AtomicBool,
    calls: Arc<RwLock<()>>,
}

/// A cancelled or failed source call may have committed before replay was
/// queued, leaving the replica unsafe to resume.
struct SourceWriteReplayGuard<'a, RT: Runtime>(Option<&'a Arc<ReplicaWriter<RT>>>);

impl<RT: Runtime> Drop for SourceWriteReplayGuard<'_, RT> {
    fn drop(&mut self) {
        if let Some(replica) = self.0 {
            replica.retire(
                metrics::RetireReason::UncleanHandoff,
                &anyhow::anyhow!("source call ended without guaranteed replay"),
            );
        }
    }
}

impl<RT: Runtime> ReplicatedV5Persistence<RT> {
    pub(super) fn new(
        v5: Arc<v5::Persistence<RT>>,
        replica: Option<Arc<ReplicaWriter<RT>>>,
        writer_state_gauge: Arc<metrics::WriterStateGauge>,
    ) -> Self {
        Self {
            v5,
            replica,
            _writer_state_gauge: writer_state_gauge,
            closing: AtomicBool::new(false),
            calls: Arc::new(RwLock::new(())),
        }
    }

    async fn admit(&self) -> anyhow::Result<RwLockReadGuard<'_, ()>> {
        let guard = self.calls.read().await;
        anyhow::ensure!(
            !self.closing.load(Ordering::Acquire),
            "persistence is shutting down"
        );
        Ok(guard)
    }

    /// Bulk writes require retirement because they bypass replay. Persisting
    /// retirement runs in the background so bulk writes tolerate replica
    /// outages.
    async fn admit_bulk_write(&self) -> anyhow::Result<RwLockReadGuard<'_, ()>> {
        let call = self.admit().await?;
        if let Some(replica) = &self.replica {
            replica.retire(
                metrics::RetireReason::BulkWrite,
                &anyhow::anyhow!("a bulk write bypassed replication"),
            );
        }
        self.v5.invalidate_shadow().await?;
        Ok(call)
    }

    async fn write_source_and_enqueue_replay<T>(
        &self,
        call: impl Future<Output = anyhow::Result<T>>,
        operation: impl FnOnce() -> ReplayOperation,
    ) -> anyhow::Result<(T, Option<oneshot::Receiver<anyhow::Result<u64>>>)> {
        let _call = self.admit().await?;
        let Some(replica) = &self.replica else {
            return Ok((call.await?, None));
        };
        let mut replay_guard = SourceWriteReplayGuard(Some(replica));
        let value = call.await?;
        let replay = replica.enqueue(operation());
        replay_guard.0 = None;
        Ok((value, replay))
    }

    /// Pace index backfill against replay, with a timeout to keep V5
    /// progressing when the replica is slow.
    async fn wait_for_replay_result(
        &self,
        result: Option<oneshot::Receiver<anyhow::Result<u64>>>,
    ) -> Option<u64> {
        let (Some(replica), Some(result)) = (&self.replica, result) else {
            return None;
        };
        tokio::time::timeout(replica.settings.apply_wait, result)
            .await
            .ok()?
            .ok()?
            .ok()
    }
}

#[async_trait]
impl<RT: Runtime> PersistenceTrait for ReplicatedV5Persistence<RT> {
    fn is_fresh(&self) -> bool {
        self.v5.is_fresh()
    }

    fn reader(&self) -> Arc<dyn PersistenceReader> {
        let reader = self.v5.reader();
        match &self.replica {
            Some(replica) => Arc::new(ComparingV5Reader::new(
                reader,
                replica.clone(),
                self.calls.clone(),
            )),
            None => reader,
        }
    }

    async fn write<'a>(
        &self,
        documents: &'a [DocumentLogEntry],
        indexes: &'a [PersistenceIndexEntry],
        conflict_strategy: ConflictStrategy,
    ) -> anyhow::Result<()> {
        self.write_source_and_enqueue_replay(
            self.v5.write(documents, indexes, conflict_strategy),
            || ReplayOperation::Write {
                documents: documents.to_vec(),
                indexes: indexes.to_vec(),
                conflict_strategy,
            },
        )
        .await?;
        Ok(())
    }

    async fn invalidate_shadow(&self) -> anyhow::Result<()> {
        let _call = self.admit_bulk_write().await?;
        Ok(())
    }

    async fn write_index_backfill(&self, entries: &[IndexBackfillEntry]) -> anyhow::Result<()> {
        let ((), replay_result) = self
            .write_source_and_enqueue_replay(self.v5.write_index_backfill(entries), || {
                ReplayOperation::WriteIndexBackfill {
                    entries: entries.to_vec(),
                }
            })
            .await?;
        self.wait_for_replay_result(replay_result).await;
        Ok(())
    }

    async fn reconcile_index_backfill(&self, index: PersistenceIndexId) -> anyhow::Result<()> {
        let ((), replay_result) = self
            .write_source_and_enqueue_replay(self.v5.reconcile_index_backfill(index), || {
                ReplayOperation::ReconcileIndexBackfill { index }
            })
            .await?;
        self.wait_for_replay_result(replay_result).await;
        Ok(())
    }

    async fn index_backfill_marker_indexes(&self) -> anyhow::Result<Vec<PersistenceIndexId>> {
        let mut indexes = self.v5.index_backfill_marker_indexes().await?;
        if let Some(replica) = &self.replica {
            indexes.extend(replica.index_backfill_marker_indexes().await);
            indexes.sort();
            indexes.dedup();
        }
        Ok(indexes)
    }

    async fn delete_index_backfill_markers_chunk(
        &self,
        index: PersistenceIndexId,
    ) -> anyhow::Result<u64> {
        let (deleted, replay_result) = self
            .write_source_and_enqueue_replay(
                self.v5.delete_index_backfill_markers_chunk(index),
                || ReplayOperation::DeleteIndexBackfillMarkers { index },
            )
            .await?;
        // A positive count from either database keeps retention deleting markers.
        Ok(deleted
            + self
                .wait_for_replay_result(replay_result)
                .await
                .unwrap_or(0))
    }

    async fn write_persistence_global(
        &self,
        key: PersistenceGlobalKey,
        value: JsonValue,
    ) -> anyhow::Result<()> {
        self.write_source_and_enqueue_replay(
            self.v5.write_persistence_global(key, value.clone()),
            || ReplayOperation::WritePersistenceGlobal {
                key,
                value: value.clone(),
            },
        )
        .await?;
        Ok(())
    }

    async fn has_index_entries(&self) -> anyhow::Result<bool> {
        self.v5.has_index_entries().await
    }

    async fn reclaim_index_history(
        &self,
        request: IndexRetentionRequest<'_>,
    ) -> anyhow::Result<IndexRetentionProgress> {
        self.v5.reclaim_index_history(request).await
    }

    async fn delete(
        &self,
        documents: Vec<(Timestamp, InternalDocumentId)>,
    ) -> anyhow::Result<usize> {
        let (deleted, _) = self
            .write_source_and_enqueue_replay(self.v5.delete(documents.clone()), || {
                ReplayOperation::DeleteDocuments { documents }
            })
            .await?;
        Ok(deleted)
    }

    async fn delete_tablet_documents(
        &self,
        tablet_id: TabletId,
        chunk_size: usize,
    ) -> anyhow::Result<usize> {
        let (deleted, _) = self
            .write_source_and_enqueue_replay(
                self.v5.delete_tablet_documents(tablet_id, chunk_size),
                || ReplayOperation::DeleteTabletDocuments {
                    tablet_id,
                    chunk_size,
                },
            )
            .await?;
        Ok(deleted)
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        if !self.closing.swap(true, Ordering::AcqRel)
            && let Some(replica) = &self.replica
        {
            // Source calls must finish enqueueing before replica shutdown can
            // wait for replay and set clean_shutdown.
            match tokio::time::timeout(replica.settings.apply_wait, self.calls.write()).await {
                Ok(_calls) => replica.close().await,
                Err(_) => replica.retire(
                    metrics::RetireReason::UncleanHandoff,
                    &anyhow::anyhow!("source calls did not finish before shutdown"),
                ),
            }
        }
        self.v5.shutdown().await
    }

    async fn import_documents_batch(
        &self,
        documents: BoxStream<'_, Vec<DocumentLogEntry>>,
    ) -> anyhow::Result<()> {
        let _call = self.admit_bulk_write().await?;
        self.v5.import_documents_batch(documents).await
    }

    async fn import_indexes_batch(
        &self,
        indexes: BoxStream<'_, Vec<PersistenceIndexEntry>>,
    ) -> anyhow::Result<()> {
        let _call = self.admit_bulk_write().await?;
        self.v5.import_indexes_batch(indexes).await
    }
}
