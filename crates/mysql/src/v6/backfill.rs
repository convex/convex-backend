//! V6 index backfill reconciliation under the writer lease.
use std::ops::Bound;

use anyhow::Context;
use common::{
    knobs::INDEX_RETENTION_DELETE_CHUNK,
    runtime::Runtime,
    types::PersistenceIndexId,
};

use super::{
    indexes::IndexEngine,
    sql,
    PersistenceDeploymentId,
};
use crate::{
    connection::MySqlTransaction,
    ConvexMySqlPool,
};

/// Transactions must fence the current writer lease and use READ COMMITTED
/// so reconciliation can delete stale rows without gap-locking index inserts.
pub(crate) trait BackfillLease: Sync {
    async fn transact_read_committed<F, T>(&self, f: F) -> anyhow::Result<T>
    where
        F: for<'a> AsyncFnOnce(&'a mut MySqlTransaction<'_>) -> anyhow::Result<T>;
}

pub(crate) struct Backfill<'a, RT: Runtime, L: BackfillLease> {
    pub(crate) pool: &'a ConvexMySqlPool<RT>,
    pub(crate) db_name: &'a str,
    pub(crate) deployment_id: PersistenceDeploymentId,
    pub(crate) engine: &'a IndexEngine,
    pub(crate) lease: &'a L,
}
impl<RT: Runtime, L: BackfillLease> Backfill<'_, RT, L> {
    /// READ COMMITTED keeps conditional deletes from gap-locking concurrent
    /// index inserts.
    pub(crate) async fn reconcile(&self, index: PersistenceIndexId) -> anyhow::Result<()> {
        let cluster_name = self.pool.cluster_name();
        let mut after: Bound<sql::SqlKey> = Bound::Unbounded;
        loop {
            // Backfill insertion has finished and markers only advance.
            // The cutoff remains valid after this read; the conditional
            // delete rechecks the current row while holding its record
            // lock, so a concurrent recreation remains visible.
            let page = {
                let mut connection = self
                    .pool
                    .acquire("v6_reconcile_backfill_page", self.db_name)
                    .await?;
                self.engine
                    .read_backfill_markers_page(&mut connection, index, after.clone())
                    .await?
            };
            self.lease
                .transact_read_committed(async |tx| {
                    self.engine.delete_stale(tx, &page, cluster_name).await
                })
                .await?;
            let Some(last) = page
                .last()
                .filter(|_| page.len() >= *INDEX_RETENTION_DELETE_CHUNK)
            else {
                break;
            };
            after = Bound::Excluded(last.key.clone());
        }
        Ok(())
    }

    pub(crate) async fn marker_indexes(&self) -> anyhow::Result<Vec<PersistenceIndexId>> {
        self.pool
            .acquire("v6_backfill_marker_indexes", self.db_name)
            .await?
            .query_collect(
                sql::LIST_BACKFILL_MARKER_INDEXES,
                vec![self.deployment_id.into()],
                16,
                |row| {
                    let id: u32 = row.get_opt(0).context("index_id")??;
                    PersistenceIndexId::try_from(id)
                },
            )
            .await
    }

    /// READ COMMITTED allows concurrent marker inserts for other indexes
    /// during the `LIMIT` scan by avoiding gap locks.
    pub(crate) async fn delete_chunk(&self, index: PersistenceIndexId) -> anyhow::Result<u64> {
        let chunk_size = u64::try_from(*INDEX_RETENTION_DELETE_CHUNK)?;
        self.lease
            .transact_read_committed(async |tx| {
                tx.exec_iter(
                    sql::DELETE_BACKFILL_MARKERS_CHUNK,
                    vec![
                        self.deployment_id.into(),
                        index.value().into(),
                        chunk_size.into(),
                    ],
                )
                .await
            })
            .await
    }
}
