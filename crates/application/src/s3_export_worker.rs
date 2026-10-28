use std::{
    sync::Arc,
    time::{
        Duration,
        SystemTime,
    },
};

use common::{
    backoff::Backoff,
    document::ParsedDocument,
    errors::report_error,
    execution_context::ExecutionId,
    fastrace_helpers::get_sampled_span,
    runtime::Runtime,
    types::UdfIdentifier,
    RequestId,
};
use database::Database;
use errors::ErrorMetadataAnyhowExt;
use exports::interface::ExportProvider;
use fastrace::future::FutureExt as _;
use keybroker::Identity;
use model::{
    log_sinks::{
        types::{
            s3_export::{
                S3ExportCredentials,
                SyncPeriod,
            },
            LogSinksRow,
            SinkConfig,
            SinkState,
            SinkType,
        },
        LogSinksModel,
    },
    s3_export_state::{
        types::{
            S3ExportCheckpoint,
            S3ExportProgress,
        },
        S3ExportStateModel,
    },
};
use streaming_export::{
    managed::{
        ByoAwsCredentials,
        SyncDestination,
        SyncFormat,
    },
    SyncStatus,
};
use usage_tracking::{
    CallType,
    UsageCounter,
};

use crate::metrics::{
    log_s3_export_worker_failed,
    s3_export_sync_timer,
    S3ExportGauges,
};

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const CONTINUOUS_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub struct S3ExportWorker<RT: Runtime> {
    runtime: RT,
    database: Database<RT>,
    instance_name: String,
    export_provider: Arc<dyn ExportProvider<RT>>,
    gauges: S3ExportGauges,
    usage_counter: UsageCounter,
}

impl<RT: Runtime> S3ExportWorker<RT> {
    pub fn new(
        runtime: RT,
        database: Database<RT>,
        instance_name: String,
        export_provider: Arc<dyn ExportProvider<RT>>,
        usage_counter: UsageCounter,
    ) -> Self {
        Self {
            runtime,
            database,
            gauges: S3ExportGauges::new(instance_name.clone()),
            instance_name,
            export_provider,
            usage_counter,
        }
    }

    pub async fn run(self) {
        let mut backoff = Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF);
        loop {
            match self.run_once().await {
                Ok(()) => backoff.reset(),
                Err(error) => {
                    log_s3_export_worker_failed(&error);
                    report_error(&mut error.context("S3ExportWorker failed")).await;
                    let delay = backoff.fail(&mut self.runtime.rng());
                    self.runtime.wait(delay).await;
                },
            }
        }
    }

    async fn run_once(&self) -> anyhow::Result<()> {
        let mut tx = self.database.begin(Identity::system()).await?;
        let export = LogSinksModel::new(&mut tx)
            .get_by_provider(SinkType::S3Export)
            .await?;
        let Some(export) = export else {
            self.gauges.clear();
            let token = tx.into_token()?;
            self.database
                .subscribe_and_wait_for_invalidation(token)
                .await?;
            return Ok(());
        };
        let SinkConfig::S3Export(config) = &export.config else {
            anyhow::bail!("S3 export document has a different configuration type");
        };
        if !config.credentials.is_ready() {
            self.gauges.clear();
            let token = tx.into_token()?;
            self.database
                .subscribe_and_wait_for_invalidation(token)
                .await?;
            return Ok(());
        }
        // A page that hangs keeps aging past this.
        self.gauges.start(self.runtime.system_time());
        let state = S3ExportStateModel::new(&mut tx)
            .get(export.developer_id())
            .await?;
        let previous_checkpoint = state.map(|state| state.checkpoint.clone());
        // A page can outlast the write log's retention (30 s on a busy
        // deployment), and a transaction that old can no longer commit. So
        // this one ends here, and the progress transaction below re-reads the
        // export instead.
        drop(tx);
        let page = {
            let database_snapshot = self.database.latest_database_snapshot()?;
            let timer = s3_export_sync_timer();
            let span = get_sampled_span(
                &self.instance_name,
                "s3_export_worker/sync",
                &mut self.runtime.rng(),
            );
            let page = self
                .export_provider
                .sync(
                    &self.instance_name,
                    &database_snapshot,
                    SyncDestination::ByoAwsBucket {
                        bucket: config.bucket.clone(),
                        region: config.region.clone(),
                        prefix: config.prefix.clone(),
                        endpoint_url: None,
                        credentials: match &config.credentials {
                            S3ExportCredentials::AccessKey {
                                access_key_id,
                                secret_access_key,
                            } => ByoAwsCredentials::AccessKey {
                                access_key_id: access_key_id.0.clone(),
                                secret_access_key: secret_access_key.0.clone(),
                            },
                            S3ExportCredentials::Role {
                                role_arn,
                                external_id,
                            } => ByoAwsCredentials::Role {
                                role_arn: role_arn.clone(),
                                external_id: external_id.clone(),
                            },
                        },
                    },
                    SyncFormat::Iceberg,
                    &config.selection,
                    &export.developer_id().to_string(),
                )
                .in_span(span)
                .await;
            timer.finish();
            match page {
                Ok(page) => page,
                // A user error from the destination is the customer's to fix,
                // so the export shows it. The worker keeps retrying, and the
                // next page that lands marks the export active again.
                Err(error) if error.is_bad_request() => {
                    self.set_status(
                        &export,
                        SinkState::Failed {
                            reason: error.user_facing_message(),
                        },
                    )
                    .await?;
                    return Err(error);
                },
                Err(error) => return Err(error),
            }
        };
        // Porter commits the page in the destination before returning it, so
        // a returned page is a billed page. The progress checkpoint below is
        // for the dashboard and can lose an OCC race without unbilling anything.
        self.usage_counter
            .track_call(
                UdfIdentifier::SystemJob("s3_export".to_string()),
                ExecutionId::new(),
                RequestId::new(),
                CallType::Export,
                true,
                page.usage.clone(),
            )
            .await;
        // The page is already committed in the destination, so losing this
        // progress update to a concurrent configuration edit loses no data.
        let checkpoint = S3ExportCheckpoint {
            progress: export_progress(&page.status)?,
        };
        let mut tx = self.database.begin(Identity::system()).await?;
        // A page synced under a configuration that has since changed does not
        // describe the current export.
        let current = LogSinksModel::new(&mut tx)
            .get_by_provider(SinkType::S3Export)
            .await?;
        if !is_same_export(current.as_ref(), &export) {
            return Ok(());
        }
        if !S3ExportStateModel::new(&mut tx)
            .advance(
                export.developer_id(),
                previous_checkpoint.as_ref(),
                checkpoint.clone(),
            )
            .await?
        {
            return Ok(());
        }
        if previous_checkpoint.as_ref() != Some(&checkpoint) {
            self.database
                .commit_with_write_source(tx, "s3_export_worker_record_progress")
                .await?;
        } else {
            drop(tx);
        }
        // After the checkpoint commit, which reads this document.
        self.set_status(&export, SinkState::Active).await?;
        let interval = sync_interval(config.period);
        self.gauges.set_next_sync_due(next_sync_due(
            &page.status,
            interval,
            self.runtime.system_time(),
        ));
        self.gauges.set_snapshot_tables_synced(match &page.status {
            SyncStatus::Snapshotting { progress } => Some(progress.num_tables_synced),
            SyncStatus::Stale { .. } | SyncStatus::UpToDate { .. } => None,
        });

        if matches!(&page.status, SyncStatus::UpToDate { .. }) {
            let mut tx = self.database.begin(Identity::system()).await?;
            let current = LogSinksModel::new(&mut tx)
                .get_by_provider(SinkType::S3Export)
                .await?;
            if !is_same_export(current.as_ref(), &export) {
                return Ok(());
            }
            let current_state = S3ExportStateModel::new(&mut tx)
                .get(export.developer_id())
                .await?;
            if current_state.is_none_or(|current| current.checkpoint != checkpoint) {
                return Ok(());
            }
            let next_write_ts = (*self.database.now_ts_for_reads()).succ()?;
            let token = tx.into_token()?;
            let invalidated = self.database.subscribe_and_wait_for_invalidation(token);
            match interval {
                None => {
                    tokio::select! {
                        result = invalidated => { result?; },
                        _ = self.database.wait_for_write_ts(next_write_ts) => {},
                        _ = self.runtime.wait(CONTINUOUS_POLL_INTERVAL) => {},
                    }
                },
                Some(interval) => {
                    tokio::select! {
                        result = invalidated => { result?; },
                        _ = self.runtime.wait(interval) => {},
                    }
                },
            }
        }
        Ok(())
    }

    /// Sets the export's status unless it already has it, or the export
    /// changed since `export` was read.
    async fn set_status(
        &self,
        export: &ParsedDocument<LogSinksRow>,
        status: SinkState,
    ) -> anyhow::Result<()> {
        if export.status == status {
            return Ok(());
        }
        let mut tx = self.database.begin(Identity::system()).await?;
        let mut model = LogSinksModel::new(&mut tx);
        let current = model.get_by_provider(SinkType::S3Export).await?;
        if !is_same_export(current.as_ref(), export)
            || current.is_some_and(|current| current.status == status)
        {
            return Ok(());
        }
        model.patch_status(export.developer_id(), status).await?;
        self.database
            .commit_with_write_source(tx, "s3_export_worker_set_status")
            .await?;
        Ok(())
    }
}

/// Whether `current` is still the export a page was synced for.
fn is_same_export(
    current: Option<&ParsedDocument<LogSinksRow>>,
    export: &ParsedDocument<LogSinksRow>,
) -> bool {
    current.is_some_and(|current| {
        current.developer_id() == export.developer_id() && current.config == export.config
    })
}

fn sync_interval(period: SyncPeriod) -> Option<Duration> {
    match period {
        SyncPeriod::Continuous => None,
        SyncPeriod::Hourly => Some(Duration::from_secs(60 * 60)),
        SyncPeriod::Daily => Some(Duration::from_secs(24 * 60 * 60)),
    }
}

/// A caught-up export sleeps for the sync interval; every other page is
/// followed by another right away. Wall-clock time, because an idle
/// deployment's snapshot timestamp only moves with the
/// `MAX_REPEATABLE_TIMESTAMP_IDLE_FREQUENCY` heartbeat.
fn next_sync_due(status: &SyncStatus, interval: Option<Duration>, now: SystemTime) -> SystemTime {
    match status {
        SyncStatus::UpToDate { .. } => now + interval.unwrap_or_default(),
        SyncStatus::Snapshotting { .. } | SyncStatus::Stale { .. } => now,
    }
}

fn export_progress(status: &SyncStatus) -> anyhow::Result<S3ExportProgress> {
    Ok(match status {
        SyncStatus::Snapshotting { progress } => S3ExportProgress::Snapshotting {
            num_tables_synced: progress.num_tables_synced.try_into()?,
            total_tables: progress.total_tables.try_into()?,
            current_component: progress.current_component.to_string(),
            current_table: progress.current_table.to_string(),
            num_documents_in_current_table: progress.num_documents_in_current_table.try_into()?,
            total_documents_in_current_table: progress
                .total_documents_in_current_table
                .map(i64::try_from)
                .transpose()?,
            num_documents_synced: progress.num_documents_synced.try_into()?,
            total_documents: progress.total_documents.map(i64::try_from).transpose()?,
        },
        SyncStatus::Stale { ts } => S3ExportProgress::Stale { ts: (*ts).into() },
        SyncStatus::UpToDate { ts } => S3ExportProgress::UpToDate { ts: (*ts).into() },
    })
}
