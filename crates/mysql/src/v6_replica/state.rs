//! Persistent state for handing a replica from one V5 writer to the next.
//!
//! A writer sets `clean_shutdown` after closing source writes and completing
//! their replay. The next writer clears the flag when registering. A crash
//! before setting the flag causes the next writer to retire the replica.
//!
//! The `v5_replica_handoffs` row survives data deletion so retirement remains
//! permanent across restarts. Updates check the source lease timestamp to
//! protect a newer writer's state. Resumption also requires the same source
//! cluster, database, and instance, since moving a deployment can leave a gap
//! in replay.
use anyhow::Context as _;
use common::{
    runtime::Runtime,
    types::DeploymentId,
};
use mysql_async::Row;

use crate::ConvexMySqlPool;

// A separate table limits resumption to replicas using clean_shutdown tracking.
const STATE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS @db_name.v5_replica_handoffs (
    deployment_id INT UNSIGNED NOT NULL,
    cluster_name VARCHAR(128) NOT NULL,
    db_name VARCHAR(128) NOT NULL,
    instance_name VARCHAR(128) NOT NULL,
    retired BOOLEAN NOT NULL,
    lease_ts BIGINT NOT NULL,
    clean_shutdown BOOLEAN NOT NULL,
    PRIMARY KEY (deployment_id),
    UNIQUE KEY v5_replica_handoffs_by_source (cluster_name, db_name, instance_name)
) ROW_FORMAT=DYNAMIC;
"#;

const COLUMNS: &str =
    "deployment_id, cluster_name, db_name, instance_name, retired, lease_ts, clean_shutdown";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReplicaHandoffState {
    pub(crate) deployment_id: DeploymentId,
    pub(crate) cluster_name: String,
    pub(crate) db_name: String,
    pub(crate) instance_name: String,
    pub(crate) retired: bool,
    pub(crate) lease_ts: i64,
    pub(crate) clean_shutdown: bool,
}

impl ReplicaHandoffState {
    fn from_row(row: Row) -> anyhow::Result<Self> {
        Ok(Self {
            deployment_id: DeploymentId(row.get_opt(0).context("deployment_id")??),
            cluster_name: row.get_opt(1).context("cluster_name")??,
            db_name: row.get_opt(2).context("db_name")??,
            instance_name: row.get_opt(3).context("instance_name")??,
            retired: row.get_opt(4).context("retired")??,
            lease_ts: row.get_opt(5).context("lease_ts")??,
            clean_shutdown: row.get_opt(6).context("clean_shutdown")??,
        })
    }

    pub(super) fn same_source(&self, other: &Self) -> bool {
        self.cluster_name == other.cluster_name
            && self.db_name == other.db_name
            && self.instance_name == other.instance_name
    }
}

pub(super) async fn ensure_table<RT: Runtime>(
    pool: &ConvexMySqlPool<RT>,
    db_name: &str,
) -> anyhow::Result<()> {
    pool.acquire("v6_replica_state_ddl", db_name)
        .await?
        .execute_many(STATE_DDL)
        .await
}

pub(super) async fn read<RT: Runtime>(
    pool: &ConvexMySqlPool<RT>,
    db_name: &str,
    deployment_id: DeploymentId,
) -> anyhow::Result<Option<ReplicaHandoffState>> {
    pool.acquire("v6_replica_state_read", db_name)
        .await?
        .query_optional(
            &format!("SELECT {COLUMNS} FROM @db_name.v5_replica_handoffs WHERE deployment_id = ?"),
            vec![deployment_id.0.into()],
        )
        .await?
        .map(ReplicaHandoffState::from_row)
        .transpose()
}

/// Returns whether replay is safe; false means retirement has been persisted.
pub(super) async fn register<RT: Runtime>(
    pool: &ConvexMySqlPool<RT>,
    db_name: &str,
    state: &ReplicaHandoffState,
    source_is_fresh: bool,
) -> anyhow::Result<bool> {
    let mut connection = pool.acquire("v6_replica_state_register", db_name).await?;
    let result = async {
        let mut tx = connection
            .transaction(
                pool.cluster_name(),
                Some(mysql_async::IsolationLevel::ReadCommitted),
            )
            .await?;
        let current = tx
            .exec_first(
                &format!(
                    "SELECT {COLUMNS} FROM @db_name.v5_replica_handoffs WHERE deployment_id = ? \
                     FOR UPDATE"
                ),
                vec![state.deployment_id.0.into()],
            )
            .await?
            .map(ReplicaHandoffState::from_row)
            .transpose()?;
        let replay = match &current {
            Some(current) => {
                anyhow::ensure!(
                    state.lease_ts > current.lease_ts,
                    "replica already registered this writer or a newer writer"
                );
                !current.retired && current.clean_shutdown && current.same_source(state)
            },
            None => source_is_fresh,
        };
        if current.is_some() {
            tx.exec_drop(
                "UPDATE @db_name.v5_replica_handoffs SET lease_ts = ?, retired = ?, \
                 clean_shutdown = FALSE WHERE deployment_id = ?",
                vec![
                    state.lease_ts.into(),
                    (!replay).into(),
                    state.deployment_id.0.into(),
                ],
            )
            .await?;
        } else {
            tx.exec_drop(
                &format!(
                    "INSERT INTO @db_name.v5_replica_handoffs ({COLUMNS}) VALUES (?, ?, ?, ?, ?, \
                     ?, FALSE)"
                ),
                vec![
                    state.deployment_id.0.into(),
                    state.cluster_name.clone().into(),
                    state.db_name.clone().into(),
                    state.instance_name.clone().into(),
                    (!replay).into(),
                    state.lease_ts.into(),
                ],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(replay)
    }
    .await;
    connection.handle_errors(result).await
}

/// Call after source writes have stopped and all replay has succeeded.
pub(super) async fn record_clean_shutdown<RT: Runtime>(
    pool: &ConvexMySqlPool<RT>,
    db_name: &str,
    deployment_id: DeploymentId,
    lease_ts: i64,
) -> anyhow::Result<()> {
    let changed = pool
        .acquire("v6_replica_clean_shutdown", db_name)
        .await?
        .exec_iter(
            "UPDATE @db_name.v5_replica_handoffs SET clean_shutdown = TRUE WHERE deployment_id = \
             ? AND lease_ts = ? AND NOT retired AND NOT clean_shutdown",
            vec![deployment_id.0.into(), lease_ts.into()],
        )
        .await?;
    anyhow::ensure!(
        changed == 1,
        "replica ownership changed before clean shutdown"
    );
    Ok(())
}

/// Returns true when this call marks the replica retired and may start cleanup.
pub(super) async fn retire<RT: Runtime>(
    pool: &ConvexMySqlPool<RT>,
    db_name: &str,
    deployment_id: DeploymentId,
    lease_ts: i64,
) -> anyhow::Result<bool> {
    let changed = pool
        .acquire("v6_replica_state_retire", db_name)
        .await?
        .exec_iter(
            "UPDATE @db_name.v5_replica_handoffs SET retired = TRUE, clean_shutdown = FALSE WHERE \
             deployment_id = ? AND lease_ts <= ? AND NOT retired",
            vec![deployment_id.0.into(), lease_ts.into()],
        )
        .await?;
    Ok(changed == 1)
}
