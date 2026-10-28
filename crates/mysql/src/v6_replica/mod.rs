//! Asynchronous replay of V5 writes into a separate V6 database.
//!
//! Each V5 writer must register in the replica database before serving writes.
//! If replay may be incomplete, retirement permanently disables replay,
//! persists that decision, and schedules deletion of the replica's data.
mod comparison;
mod config;
mod metrics;
mod persistence;
mod replica;
mod state;
use std::{
    collections::BTreeSet,
    sync::Arc,
    time::Duration,
};

use anyhow::Context as _;
use common::{
    knobs::DATABASE_USE_PREPARED_STATEMENTS,
    persistence::Persistence,
    runtime::Runtime,
    shutdown::ShutdownSignal,
    types::{
        DeploymentId,
        PersistenceVersion,
    },
};
use parking_lot::Mutex;
use tokio::sync::{
    OnceCell,
    Semaphore,
};

use self::{
    config::Settings,
    metrics::{
        RetireReason,
        WriterState,
    },
    persistence::ReplicatedV5Persistence,
    replica::ReplicaWriter,
    state::ReplicaHandoffState,
};
use crate::{
    v5,
    v6,
    ConvexMySqlPool,
    DeploymentDeletionPool,
    DeploymentDeletionTarget,
    IndexesLogMaintenance,
    MySqlOptions,
};

/// Share one target across a source pool's writers and maintenance, and attach
/// it before opening deployments.
#[derive(Clone)]
pub struct ReplicaTarget<RT: Runtime> {
    pub(crate) pool: Arc<ConvexMySqlPool<RT>>,
    pub(crate) db_name: String,
    state_table: Arc<OnceCell<()>>,
    deleting: Arc<Mutex<BTreeSet<DeploymentId>>>,
}

impl<RT: Runtime> ReplicaTarget<RT> {
    pub fn new(url: &str) -> anyhow::Result<Self> {
        let (parsed, db_name) = config::parse_url(url)?;
        Ok(Self {
            pool: Arc::new(ConvexMySqlPool::new(
                &parsed,
                *DATABASE_USE_PREPARED_STATEMENTS,
                true, /* require_leader */
                None,
            )?),
            db_name,
            state_table: Default::default(),
            deleting: Default::default(),
        })
    }

    pub fn cluster_name(&self) -> &str {
        self.pool.cluster_name()
    }

    async fn ensure_state_table(&self) -> anyhow::Result<()> {
        self.state_table
            .get_or_try_init(|| state::ensure_table(&self.pool, &self.db_name))
            .await?;
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct ReplicatedV5PersistenceFactory<RT: Runtime> {
    target: ReplicaTarget<RT>,
    settings: Settings,
}

impl<RT: Runtime> ReplicatedV5PersistenceFactory<RT> {
    pub(crate) fn configured(target: ReplicaTarget<RT>) -> Self {
        Self {
            target,
            settings: config::SETTINGS.clone(),
        }
    }

    pub(crate) async fn open(
        &self,
        pool: Arc<ConvexMySqlPool<RT>>,
        db_name: String,
        options: MySqlOptions,
        shutdown: ShutdownSignal,
    ) -> anyhow::Result<ReplicatedV5Persistence<RT>> {
        let deployment_id = options.deployment_id;
        let v5 = Arc::new(v5::Persistence::new(pool, db_name, options, shutdown).await?);
        let cluster_name = v5.lease.pool().cluster_name().to_owned();
        metrics::initialize(&cluster_name);
        let writer_state_gauge = Arc::new(metrics::WriterStateGauge::new(
            &cluster_name,
            WriterState::Ineligible,
        ));
        let replica = match tokio::time::timeout(
            self.settings.init_timeout,
            self.initialize(&v5, deployment_id, &writer_state_gauge),
        )
        .await
        .unwrap_or_else(|_| Err(anyhow::anyhow!("timed out")))
        {
            Ok(Some(replica)) => {
                writer_state_gauge.set(WriterState::Ready);
                Some(replica)
            },
            Ok(None) => None,
            Err(error) => {
                tracing::warn!(
                    instance_name = %v5.lease.instance_name(),
                    "Could not start the V6 replica: {error:#}"
                );
                writer_state_gauge.set(WriterState::InitializationFailed);
                return Err(error).context("failed to initialize the required V6 replica state");
            },
        };
        Ok(ReplicatedV5Persistence::new(
            v5,
            replica,
            writer_state_gauge,
        ))
    }

    /// `None` means the replica is persistently marked retired, so V5 can serve
    /// on its own.
    async fn initialize(
        &self,
        v5: &v5::Persistence<RT>,
        deployment_id: DeploymentId,
        writer_state_gauge: &Arc<metrics::WriterStateGauge>,
    ) -> anyhow::Result<Option<Arc<ReplicaWriter<RT>>>> {
        let lease = &v5.lease;
        let target = self.target.clone();
        target.ensure_state_table().await?;
        let source = ReplicaHandoffState {
            deployment_id,
            cluster_name: lease.pool().cluster_name().to_owned(),
            db_name: lease.db_name().to_owned(),
            instance_name: lease.instance_name().to_owned(),
            retired: false,
            lease_ts: lease.timestamp(),
            clean_shutdown: false,
        };
        let deadline = tokio::time::Instant::now() + self.settings.apply_wait;
        let previous = loop {
            let current = state::read(&target.pool, &target.db_name, deployment_id).await?;
            if current.as_ref().is_some_and(|current| {
                current.lease_ts < source.lease_ts
                    && current.same_source(&source)
                    && !current.retired
                    && !current.clean_shutdown
            }) && tokio::time::Instant::now() < deadline
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
                continue;
            }
            break current;
        };
        // An empty document log can still have globals or backfilled indexes
        // that a new replica would be missing.
        let source_is_fresh = previous.is_none()
            && v5.is_fresh()
            && !v5.has_index_entries().await?
            && lease
                .pool()
                .acquire("v6_replica_source_empty", lease.db_name())
                .await?
                .query_optional(
                    "SELECT 1 FROM @db_name.persistence_globals WHERE instance_name = ? LIMIT 1",
                    vec![lease.instance_name().into()],
                )
                .await?
                .is_none();
        if !state::register(&target.pool, &target.db_name, &source, source_is_fresh).await? {
            if let Some(current) = previous {
                if !current.retired {
                    metrics::log_retirement(&source.cluster_name, RetireReason::UncleanHandoff);
                }
                spawn_delete_data(target, deployment_id);
            }
            return Ok(None);
        }
        let (lease_lost, _) = tokio::sync::oneshot::channel();
        let v6 = Arc::new(
            v6::Persistence::new(
                target.pool.clone(),
                target.db_name.clone(),
                MySqlOptions {
                    allow_read_only: true,
                    version: PersistenceVersion::V6,
                    instance_name: source.instance_name.clone().into(),
                    multitenant: true,
                    deployment_id,
                },
                ShutdownSignal::new(lease_lost),
            )
            .await?,
        );
        anyhow::ensure!(
            state::read(&target.pool, &target.db_name, deployment_id)
                .await?
                .is_some_and(|current| current.lease_ts == source.lease_ts
                    && !current.retired
                    && !current.clean_shutdown),
            "replica ownership changed during initialization"
        );
        Ok(Some(ReplicaWriter::new(
            v6,
            target,
            source,
            self.settings.clone(),
            writer_state_gauge.clone(),
        )))
    }
}

pub fn maintenance<RT: Runtime>(
    pool: &ConvexMySqlPool<RT>,
    owner: String,
) -> Option<IndexesLogMaintenance<RT>> {
    let target = pool.v6_replica.get()?;
    Some(IndexesLogMaintenance::new(
        target.pool.clone(),
        target.db_name.clone(),
        owner,
    ))
}

/// Limit cleanup concurrency to reserve database capacity for replay.
static DELETION_PERMITS: Semaphore = Semaphore::const_new(2);

fn spawn_delete_data<RT: Runtime>(target: ReplicaTarget<RT>, deployment_id: DeploymentId) {
    if !target.deleting.lock().insert(deployment_id) {
        return;
    }
    common::runtime::tokio_spawn("v6_replica_delete", async move {
        let result = match DELETION_PERMITS.acquire().await {
            Ok(_permit) => delete_data(&target, deployment_id).await,
            Err(error) => Err(error.into()),
        };
        if let Err(error) = result {
            tracing::warn!("Could not delete V6 replica data for {deployment_id}: {error:#}");
        }
        target.deleting.lock().remove(&deployment_id);
    });
}

/// The V6 deleter revokes the replica's lease before deleting data, stopping
/// writes from any replay still in progress.
async fn delete_data<RT: Runtime>(
    target: &ReplicaTarget<RT>,
    deployment_id: DeploymentId,
) -> anyhow::Result<()> {
    DeploymentDeletionPool::from_pool(
        target.pool.clone(),
        target.db_name.clone(),
        PersistenceVersion::V6,
    )
    .deleter(DeploymentDeletionTarget::V6(deployment_id))?
    .delete_all()
    .await
}
