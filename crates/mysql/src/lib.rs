#![feature(coroutines)]
#![feature(proc_macro_hygiene)]
#![feature(impl_trait_in_assoc_type)]
#![feature(try_blocks_heterogeneous)]
mod chunks;
mod connection;
mod deployment_deletion;
mod document_encoding;
mod metrics;
mod sql;
mod v5;
mod v6;
mod v6_replica;
use std::{
    ops::Deref,
    sync::Arc,
};

use common::{
    persistence::{
        Persistence,
        PersistenceReader,
    },
    runtime::Runtime,
    shutdown::ShutdownSignal,
    types::{
        DeploymentId,
        PersistenceVersion,
    },
};
pub use connection::ConvexMySqlPool;
pub use v6::maintenance::{
    IndexesLogMaintenance,
    MaintenanceRound,
};
pub use v6_replica::{
    maintenance as v6_replica_maintenance,
    ReplicaTarget as V6ReplicaTarget,
};

/// Maximum number of documents in one write: the sum of
/// TRANSACTION_MAX_SYSTEM_NUM_WRITES and TRANSACTION_MAX_NUM_USER_WRITES.
pub(crate) const MAX_INSERT_SIZE: usize = 56000;

pub type MySqlPersistence<RT> = v5::Persistence<RT>;
pub type MySqlReader<RT> = v5::Reader<RT>;
pub use deployment_deletion::{
    DeploymentDeleter,
    DeploymentDeletionBatch,
    DeploymentDeletionCursor,
    DeploymentDeletionPool,
    DeploymentDeletionTable,
    DeploymentDeletionTarget,
};

#[derive(Clone, Debug)]
pub struct MySqlInstanceName {
    raw: String,
}

impl Deref for MySqlInstanceName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

impl<T: ToString> From<T> for MySqlInstanceName {
    fn from(raw: T) -> Self {
        Self::new(raw.to_string())
    }
}

impl MySqlInstanceName {
    pub fn new(raw: String) -> Self {
        Self { raw }
    }
}

#[derive(Clone, Debug)]
pub struct MySqlOptions {
    pub allow_read_only: bool,
    pub version: PersistenceVersion,
    pub instance_name: MySqlInstanceName,
    pub multitenant: bool,
    pub deployment_id: DeploymentId,
}

#[derive(Debug)]
pub struct MySqlReaderOptions {
    pub version: PersistenceVersion,
    pub instance_name: MySqlInstanceName,
    pub multitenant: bool,
    pub deployment_id: DeploymentId,
}

pub async fn connect_persistence<RT: Runtime>(
    pool: Arc<ConvexMySqlPool<RT>>,
    db_name: String,
    options: MySqlOptions,
    lease_lost_shutdown: ShutdownSignal,
) -> anyhow::Result<Arc<dyn Persistence>> {
    match options.version {
        PersistenceVersion::V5 => {
            if options.multitenant
                && let Some(target) = pool.v6_replica.get().cloned()
            {
                Ok(Arc::new(
                    v6_replica::ReplicatedV5PersistenceFactory::configured(target)
                        .open(pool, db_name, options, lease_lost_shutdown)
                        .await?,
                ))
            } else {
                v5::connect(pool, db_name, options, lease_lost_shutdown).await
            }
        },
        PersistenceVersion::V6 => v6::connect(pool, db_name, options, lease_lost_shutdown).await,
    }
}

pub fn connect_persistence_reader<RT: Runtime>(
    pool: Arc<ConvexMySqlPool<RT>>,
    db_name: String,
    options: MySqlReaderOptions,
) -> anyhow::Result<Arc<dyn PersistenceReader>> {
    match options.version {
        PersistenceVersion::V5 => v5::connect_reader(pool, db_name, options),
        PersistenceVersion::V6 => v6::connect_reader(pool, db_name, options),
    }
}

pub async fn set_persistence_read_only<RT: Runtime>(
    pool: Arc<ConvexMySqlPool<RT>>,
    db_name: String,
    options: MySqlOptions,
    read_only: bool,
) -> anyhow::Result<()> {
    match options.version {
        PersistenceVersion::V5 => {
            v5::set_persistence_read_only(pool, db_name, options, read_only).await
        },
        PersistenceVersion::V6 => {
            v6::set_persistence_read_only(pool, db_name, options, read_only).await
        },
    }
}
