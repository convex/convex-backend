use std::{
    sync::Arc,
    time::Duration,
};

use common::{
    errors::{
        lease_lost_error,
        report_error,
        LeaseLostError,
    },
    knobs::{
        DATABASE_WORKERS_MIN_COMMITS,
        TABLE_SUMMARY_AGE_JITTER_SECONDS,
        TABLE_SUMMARY_BOOTSTRAP_RECENT_THRESHOLD,
        TABLE_SUMMARY_MAX_CHECKPOINT_AGE,
        TABLE_SUMMARY_MAX_STALENESS,
    },
    persistence::Persistence,
    runtime::{
        Runtime,
        SpawnHandle,
        UnixTimestamp,
    },
    shutdown::ShutdownSignal,
};
use database::{
    Database,
    TableSummaryWriter,
};
use futures::{
    pin_mut,
    select_biased,
    FutureExt,
};
use parking_lot::Mutex;
use rand::Rng;
use tokio::sync::oneshot;

use crate::metrics::{
    log_table_summary_checkpoint,
    log_worker_starting,
    table_summary_bootstrap_timer,
};

pub struct TableSummaryWorker<RT: Runtime> {
    runtime: RT,
    database: Database<RT>,
    persistence: Arc<dyn Persistence>,
}

struct Inner {
    handle: Box<dyn SpawnHandle>,
    cancel_sender: oneshot::Sender<()>,
}

#[derive(Clone)]
pub struct TableSummaryClient {
    inner: Arc<Mutex<Option<Inner>>>,
}

struct LastWriteInfo {
    ts: UnixTimestamp,
    observed_commits: usize,
}

impl<RT: Runtime> TableSummaryWorker<RT> {
    pub(crate) fn start(
        runtime: RT,
        database: Database<RT>,
        persistence: Arc<dyn Persistence>,
        lease_lost_shutdown: ShutdownSignal,
    ) -> TableSummaryClient {
        let table_summary_worker = Self {
            runtime: runtime.clone(),
            database,
            persistence,
        };
        let (cancel_sender, cancel_receiver) = oneshot::channel();
        let handle = runtime.spawn(
            "table_summary_worker",
            table_summary_worker.go(cancel_receiver, lease_lost_shutdown),
        );
        let inner = Inner {
            handle,
            cancel_sender,
        };
        TableSummaryClient {
            inner: Arc::new(Mutex::new(Some(inner))),
        }
    }

    async fn checkpoint_table_summaries(
        &self,
        last_write_info: &mut Option<LastWriteInfo>,
        has_bootstrapped: &mut bool,
        writer: &TableSummaryWriter<RT>,
        jittered_max_age: &mut Duration,
    ) -> anyhow::Result<()> {
        let _status = log_worker_starting("TableSummaryWorker");
        let commits_since_load = self.database.write_commits_since_load();
        let now = self.runtime.unix_timestamp();
        if let Some(last_write_info) = last_write_info
            && *has_bootstrapped
        {
            let new_commits = commits_since_load - last_write_info.observed_commits;
            let age = now - last_write_info.ts;
            let enough_commits = new_commits >= *DATABASE_WORKERS_MIN_COMMITS;
            // Bound the staleness of the published table shapes on deployments
            // with writes but not enough traffic to hit the commit threshold.
            let stale_with_writes = new_commits > 0 && age >= *TABLE_SUMMARY_MAX_STALENESS;
            // Even with no writes, the checkpoint must keep advancing so
            // document retention can make progress.
            let too_old = age >= *jittered_max_age;
            if !enough_commits && !stale_with_writes && !too_old {
                return Ok(());
            }
        }
        // The order of these is important -- we write the checkpoint, and then
        // we signal that we're ready to propagate the snapshot (+ any new
        // writes) to `Database` via `finish_table_summary_bootstrap`.
        // If we do this in the other order, `finish_table_summary_bootstrap`
        // will end up re-doing the work from the line above.
        log_table_summary_checkpoint(!*has_bootstrapped);
        let snapshot_ts = writer.checkpoint().await?;
        if !*has_bootstrapped {
            let is_recent = self.database.now_ts_for_reads().secs_since_f64(snapshot_ts)
                < (*TABLE_SUMMARY_BOOTSTRAP_RECENT_THRESHOLD).as_secs_f64();
            if is_recent {
                tracing::info!("Finishing table summary bootstrap");
                self.database.finish_table_summary_bootstrap().await?;
                *has_bootstrapped = true;
                tracing::info!("Table summary bootstrap finished");
            }
        }
        *last_write_info = Some(LastWriteInfo {
            observed_commits: commits_since_load,
            ts: now,
        });
        *jittered_max_age = self.jittered_max_age();
        Ok(())
    }

    fn jittered_max_age(&self) -> Duration {
        let max_age_jitter = (*TABLE_SUMMARY_AGE_JITTER_SECONDS)
            .min(TABLE_SUMMARY_MAX_CHECKPOINT_AGE.as_secs_f32() / 2.0)
            * self.runtime.rng().random_range(-1.0..=1.0);
        Duration::try_from_secs_f32(TABLE_SUMMARY_MAX_CHECKPOINT_AGE.as_secs_f32() + max_age_jitter)
            .unwrap_or_default()
    }

    async fn go(self, cancel_receiver: oneshot::Receiver<()>, lease_lost_shutdown: ShutdownSignal) {
        tracing::info!("Starting background table summary worker");
        let mut timer = Some(table_summary_bootstrap_timer());
        let cancel_fut = cancel_receiver.fuse();
        pin_mut!(cancel_fut);

        let writer = TableSummaryWriter::new(
            self.runtime.clone(),
            self.persistence.clone(),
            self.database.clone(),
            self.database.retention_validator(),
        );

        let mut last_write_info = None;
        let mut has_bootstrapped = false;
        let mut jittered_max_age = self.jittered_max_age();
        loop {
            let result = self
                .checkpoint_table_summaries(
                    &mut last_write_info,
                    &mut has_bootstrapped,
                    &writer,
                    &mut jittered_max_age,
                )
                .await;
            if timer.is_some() && has_bootstrapped {
                match timer.take() {
                    Some(inner) => {
                        let _ = inner.finish();
                    },
                    None => {},
                }
            }
            if let Err(mut err) = result {
                report_error(&mut err).await;
                if let Some(LeaseLostError) = err.downcast_ref() {
                    lease_lost_shutdown.signal(
                        lease_lost_error().context("Failed to write table summary checkpoint"),
                    );
                }
            }
            let wait_fut = self.runtime.wait(Duration::from_secs(10)).fuse();
            pin_mut!(wait_fut);
            select_biased! {
                _ = cancel_fut => {
                    tracing::info!("Shutting down table summary worker...");
                    break;
                }
                _ = wait_fut => {},
            }
        }
    }
}

impl TableSummaryClient {
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        let inner = { self.inner.lock().take() };
        if let Some(inner) = inner {
            let _ = inner.cancel_sender.send(());
            // NB: We don't want to use `shutdown_and_join` here since we actually want to
            // block on our flush completing successfully.
            inner.handle.join().await?;
        }
        Ok(())
    }
}
