use std::collections::BTreeSet;

use anyhow::Context as _;
use common::{
    runtime::Runtime,
    types::{
        DeploymentId,
        PersistenceVersion,
    },
};
use mysql_async::Value;

use crate::{
    connection::is_no_such_table_error,
    v6::PersistenceDeploymentId,
    ConvexMySqlPool,
    DeploymentDeletionTarget,
};

/// The deployments holding a lease on a multitenant cluster.
pub enum ClusterLeases {
    /// V5: leases are keyed by human-readable deployment name.
    DeploymentNames(BTreeSet<String>),
    /// V6: leases are keyed by integer deployment id.
    DeploymentIds(BTreeSet<DeploymentId>),
}

impl<RT: Runtime> ConvexMySqlPool<RT> {
    /// Lists every deployment holding a lease in the multitenant database
    /// `db_name`. V5 keys `leases` by `instance_name`, V6 by `deployment_id`.
    pub async fn all_leases(
        &self,
        db_name: &str,
        version: PersistenceVersion,
    ) -> anyhow::Result<ClusterLeases> {
        let mut conn = self.acquire("all_leases", db_name).await?;
        Ok(match version {
            PersistenceVersion::V5 => {
                let names = conn
                    .query_collect(
                        "SELECT instance_name FROM @db_name.leases",
                        vec![],
                        0, /* size_hint */
                        |row| Ok(row.get_opt::<String, _>(0).context("row[0]")??),
                    )
                    .await;
                ClusterLeases::DeploymentNames(
                    or_empty_without_leases_table(names)?.into_iter().collect(),
                )
            },
            PersistenceVersion::V6 => {
                let ids = conn
                    .query_collect(
                        "SELECT deployment_id FROM @db_name.leases",
                        vec![],
                        0, /* size_hint */
                        |row| Ok(row.get_opt::<u32, _>(0).context("row[0]")??),
                    )
                    .await;
                ClusterLeases::DeploymentIds(
                    or_empty_without_leases_table(ids)?
                        .into_iter()
                        .map(|id| DeploymentId(u64::from(id)))
                        .collect(),
                )
            },
        })
    }

    /// Whether `target` holds a lease in the multitenant database `db_name`.
    pub async fn has_lease(
        &self,
        db_name: &str,
        target: &DeploymentDeletionTarget,
    ) -> anyhow::Result<bool> {
        let (statement, tenant) = match target {
            DeploymentDeletionTarget::V5(name) => (
                "SELECT 1 FROM @db_name.leases WHERE instance_name = ?",
                Value::from(&**name),
            ),
            DeploymentDeletionTarget::V6(id) => (
                "SELECT 1 FROM @db_name.leases WHERE deployment_id = ?",
                Value::from(PersistenceDeploymentId::try_from(*id)?),
            ),
        };
        match self
            .acquire("has_lease", db_name)
            .await?
            .query_optional(statement, vec![tenant])
            .await
        {
            Ok(row) => Ok(row.is_some()),
            // empty database case
            Err(e) if is_no_such_table_error(&e) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

fn or_empty_without_leases_table<T>(result: anyhow::Result<Vec<T>>) -> anyhow::Result<Vec<T>> {
    match result {
        Err(e) if is_no_such_table_error(&e) => Ok(vec![]),
        result => result,
    }
}
