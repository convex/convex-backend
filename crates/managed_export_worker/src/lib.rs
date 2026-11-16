#![cfg_attr(test, feature(try_blocks_heterogeneous))]

use std::{
    sync::Arc,
    time::Duration,
};

use common::{
    backoff::Backoff,
    document::ParsedDocument,
    errors::report_error,
    execution_context::ExecutionId,
    fastrace_helpers::get_sampled_span,
    log_streaming::{
        LogEvent,
        LogSender,
        S3ExportStatus,
        StructuredLogEvent,
    },
    runtime::Runtime,
    types::{
        Timestamp,
        UdfIdentifier,
    },
    value::DeveloperDocumentId,
    RequestId,
};
use database::Database;
use errors::ErrorMetadataAnyhowExt;
use exports::interface::ExportProvider;
use fastrace::future::FutureExt as _;
use keybroker::{
    Identity,
    KeyBroker,
};
use model::{
    integrations::{
        types::{
            IntegrationConfig,
            IntegrationDocument,
            IntegrationState,
            IntegrationType,
        },
        IntegrationsModel,
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
    log_s3_export_progress,
    log_s3_export_worker_failed,
    s3_export_sync_timer,
};

mod metrics;

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const CONTINUOUS_POLL_INTERVAL: Duration = Duration::from_secs(5);
/// About five minutes of retries, with the backoff capped at a minute.
const MAX_CONSECUTIVE_FAILED_SYNCS: u32 = 10;
/// A caught-up export syncs every few seconds, and customers pay log stream
/// egress per event.
const STATS_INTERVAL: Duration = Duration::from_secs(60);

pub struct S3ExportWorker<RT: Runtime> {
    runtime: RT,
    database: Database<RT>,
    instance_name: String,
    export_provider: Arc<dyn ExportProvider<RT>>,
    usage_counter: UsageCounter,
    key_broker: KeyBroker,
    log_sender: Arc<dyn LogSender>,
    /// Consecutive failed syncs, and the export they belong to, so a
    /// recreated export starts from zero.
    failed_syncs: Option<(DeveloperDocumentId, u32)>,
    /// The export and status last sent to log streams, and when.
    last_stats: Option<(DeveloperDocumentId, S3ExportStatus, tokio::time::Instant)>,
}

impl<RT: Runtime> S3ExportWorker<RT> {
    pub fn new(
        runtime: RT,
        database: Database<RT>,
        instance_name: String,
        export_provider: Arc<dyn ExportProvider<RT>>,
        usage_counter: UsageCounter,
        key_broker: KeyBroker,
        log_sender: Arc<dyn LogSender>,
    ) -> Self {
        Self {
            runtime,
            database,
            instance_name,
            export_provider,
            usage_counter,
            key_broker,
            log_sender,
            failed_syncs: None,
            last_stats: None,
        }
    }

    pub async fn run(mut self) {
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

    async fn run_once(&mut self) -> anyhow::Result<()> {
        let mut tx = self.database.begin(Identity::system()).await?;
        let export = IntegrationsModel::new(&mut tx)
            .get_by_provider(IntegrationType::S3Export)
            .await?;
        let Some(export) = export else {
            let token = tx.into_token()?;
            self.database
                .subscribe_and_wait_for_invalidation(token)
                .await?;
            return Ok(());
        };
        let IntegrationConfig::S3Export(config) = &export.config else {
            anyhow::bail!("S3 export document has a different configuration type");
        };
        if config.credentials.role_arn.is_empty() {
            let token = tx.into_token()?;
            self.database
                .subscribe_and_wait_for_invalidation(token)
                .await?;
            return Ok(());
        }
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
                        credentials: ByoAwsCredentials {
                            role_arn: config.credentials.role_arn.clone(),
                            external_id: config.credentials.external_id.clone(),
                        },
                    },
                    SyncFormat::Iceberg,
                    &config.selection,
                    &export.developer_id().to_string(),
                    self.key_broker.s3_export_state_encryptor().derived_key(),
                )
                .in_span(span)
                .await;
            match &page {
                Ok(_) => {
                    timer.finish();
                },
                Err(error) => {
                    timer.finish_with(error.metric_status_label_value());
                },
            }
            if let Err(error) = &page {
                self.send_stats(
                    export.developer_id(),
                    S3ExportStatus::Failed,
                    previous_checkpoint.as_ref(),
                    Some(error.user_facing_message()),
                );
            }
            match page {
                Ok(page) => page,
                // A user error from the destination is the customer's to fix,
                // so the export shows it. The worker keeps retrying, and the
                // next page that lands marks the export active again.
                Err(error) if error.is_bad_request() => {
                    self.set_status(
                        &export,
                        IntegrationState::Failed {
                            reason: error.user_facing_message(),
                        },
                    )
                    .await?;
                    return Err(error);
                },
                // Anything else retries on its own. Once it has failed for
                // minutes the export says so instead of looking healthy. A
                // user error already showing says more, so it stays.
                Err(error) => {
                    let id = export.developer_id();
                    let failed = match self.failed_syncs {
                        Some((counted, failed)) if counted == id => failed + 1,
                        _ => 1,
                    };
                    self.failed_syncs = Some((id, failed));
                    if failed >= MAX_CONSECUTIVE_FAILED_SYNCS
                        && !matches!(export.status, IntegrationState::Failed { .. })
                    {
                        let status = IntegrationState::Failed {
                            reason: format!(
                                "Convex couldn't sync this export after {failed} attempts. We've \
                                 been alerted and it keeps retrying."
                            ),
                        };
                        if let Err(mut write) = self.set_status(&export, status).await {
                            report_error(&mut write).await;
                        }
                    }
                    return Err(error);
                },
            }
        };
        self.failed_syncs = None;
        log_s3_export_progress(&page.status, *self.database.now_ts_for_reads());
        let checkpoint = S3ExportCheckpoint {
            progress: export_progress(&page.status)?,
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
        let mut tx = self.database.begin(Identity::system()).await?;
        // A page synced under a configuration that has since changed does not
        // describe the current export.
        let current = IntegrationsModel::new(&mut tx)
            .get_by_provider(IntegrationType::S3Export)
            .await?;
        if !is_same_export(current.as_ref(), &export) {
            return Ok(());
        }
        self.send_stats(
            export.developer_id(),
            match &page.status {
                SyncStatus::Snapshotting { .. } => S3ExportStatus::Snapshotting,
                SyncStatus::Stale { .. } => S3ExportStatus::CatchingUp,
                SyncStatus::UpToDate { .. } => S3ExportStatus::UpToDate,
            },
            Some(&checkpoint),
            None,
        );
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
                .commit(tx, "s3_export_worker_record_progress")
                .await?;
        } else {
            drop(tx);
        }
        // After the checkpoint commit, which reads this document.
        self.set_status(&export, IntegrationState::Active).await?;
        if matches!(&page.status, SyncStatus::UpToDate { .. }) {
            let mut tx = self.database.begin(Identity::system()).await?;
            let current = IntegrationsModel::new(&mut tx)
                .get_by_provider(IntegrationType::S3Export)
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
            tokio::select! {
                result = invalidated => { result?; },
                _ = self.database.wait_for_write_ts(next_write_ts) => {},
                _ = self.runtime.wait(CONTINUOUS_POLL_INTERVAL) => {},
            }
        }
        Ok(())
    }

    /// Sends the export's status to log streams when it changes, and every
    /// [`STATS_INTERVAL`] while it stays the same. The lag is measured from
    /// the last consistent snapshot in `checkpoint`.
    fn send_stats(
        &mut self,
        export: DeveloperDocumentId,
        status: S3ExportStatus,
        checkpoint: Option<&S3ExportCheckpoint>,
        last_error: Option<String>,
    ) {
        let now = self.runtime.monotonic_now();
        if let Some((last_export, last_status, sent)) = self.last_stats
            && last_export == export
            && last_status == status
            && now.duration_since(sent) < STATS_INTERVAL
        {
            return;
        }
        self.last_stats = Some((export, status, now));
        let synced_ts = match checkpoint.map(|checkpoint| &checkpoint.progress) {
            Some(S3ExportProgress::Stale { ts } | S3ExportProgress::UpToDate { ts }) => {
                Timestamp::try_from(*ts).ok()
            },
            Some(S3ExportProgress::Snapshotting { .. }) | None => None,
        };
        let lag_seconds = synced_ts.map(|ts| {
            Duration::from_secs_f64(self.database.now_ts_for_reads().secs_since_f64(ts).max(0.0))
        });
        self.log_sender.send_logs(vec![LogEvent {
            timestamp: self.runtime.unix_timestamp(),
            event: StructuredLogEvent::S3ExportStats {
                status,
                lag_seconds,
                last_error,
            },
        }]);
    }

    /// Sets the export's status unless it already has it, or the export
    /// changed since `export` was read.
    async fn set_status(
        &self,
        export: &ParsedDocument<IntegrationDocument>,
        status: IntegrationState,
    ) -> anyhow::Result<()> {
        if export.status == status {
            return Ok(());
        }
        let mut tx = self.database.begin(Identity::system()).await?;
        let mut model = IntegrationsModel::new(&mut tx);
        let current = model.get_by_provider(IntegrationType::S3Export).await?;
        if !is_same_export(current.as_ref(), export)
            || current.is_some_and(|current| current.status == status)
        {
            return Ok(());
        }
        model.patch_status(export.developer_id(), status).await?;
        self.database
            .commit(tx, "s3_export_worker_set_status")
            .await?;
        Ok(())
    }
}

/// Whether `current` is still the export a page was synced for.
fn is_same_export(
    current: Option<&ParsedDocument<IntegrationDocument>>,
    export: &ParsedDocument<IntegrationDocument>,
) -> bool {
    current.is_some_and(|current| {
        current.developer_id() == export.developer_id() && current.config == export.config
    })
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
