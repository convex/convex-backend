use std::{
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use common::{
    self,
    backoff::Backoff,
    components::ComponentPath,
    document::{
        ParseDocument,
        ParsedDocument,
    },
    errors::report_error,
    execution_context::ExecutionId,
    runtime::Runtime,
    types::UdfIdentifier,
    RequestId,
};
use database::{
    Database,
    SystemMetadataModel,
    Token,
    MAX_OCC_FAILURES,
};
use errors::ErrorMetadataAnyhowExt as _;
use exports::{
    interface::ExportProvider,
    ExportComponents,
    FILE_STORAGE_EXPORT_TOO_LARGE_SHORT_MSG,
};
use futures::{
    Future,
    FutureExt,
};
use keybroker::Identity;
use model::exports::{
    types::{
        Export,
        ExportRequestor,
    },
    ExportsModel,
};
use storage::Storage;
use usage_tracking::{
    CallType,
    FunctionUsageTracker,
    StorageCallTracker,
    UsageCounter,
};
use value::ResolvedDocumentId;

use crate::{
    exports::metrics::log_export_failed,
    metrics::log_worker_starting,
};

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(900); // 15 minutes

#[derive(thiserror::Error, Debug)]
#[error("Export canceled")]
struct ExportCanceled;

pub struct ExportWorker<RT: Runtime> {
    pub(super) runtime: RT,
    pub(super) database: Database<RT>,
    pub(super) exports_storage: Arc<dyn Storage>,
    pub(super) file_storage: Arc<dyn Storage>,
    pub(super) export_provider: Arc<dyn ExportProvider<RT>>,
    pub(super) backoff: Backoff,
    pub(super) usage_tracking: UsageCounter,
    pub(super) deployment_name: String,
}

impl<RT: Runtime> ExportWorker<RT> {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        runtime: RT,
        database: Database<RT>,
        exports_storage: Arc<dyn Storage>,
        file_storage: Arc<dyn Storage>,
        export_provider: Arc<dyn ExportProvider<RT>>,
        usage_tracking: UsageCounter,
        deployment_name: String,
    ) -> impl Future<Output = ()> + Send {
        let mut worker = Self {
            runtime,
            database,
            exports_storage,
            file_storage,
            export_provider,
            backoff: Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF),
            usage_tracking,
            deployment_name,
        };
        async move {
            loop {
                let result: anyhow::Result<()> = async {
                    if let Some(token) = Box::pin(worker.run()).await? {
                        worker
                            .database
                            .subscribe_and_wait_for_invalidation(token)
                            .await?;
                    }
                    Ok(())
                }
                .await;
                if let Err(e) = result {
                    report_error(&mut e.context("ExportWorker died")).await;
                    let delay = worker.backoff.fail(&mut worker.runtime.rng());
                    worker.runtime.wait(delay).await;
                } else {
                    worker.backoff.reset();
                }
            }
        }
    }

    // Subscribe to the export table. If there is a requested export, start
    // an export and mark as in_progress. If there's an export job that didn't
    // finish (it's in_progress), restart that export.
    pub async fn run(&mut self) -> anyhow::Result<Option<Token>> {
        let mut tx = self.database.begin(Identity::system()).await?;
        let mut exports_model = ExportsModel::new(&mut tx);
        let export_requested = exports_model.latest_requested().await?;
        let export_in_progress = exports_model.latest_in_progress().await?;
        match (export_requested, export_in_progress) {
            (Some(_), Some(_)) => {
                anyhow::bail!("Can only have one export requested or in progress at once.")
            },
            (Some(export), None) => {
                tracing::info!("Export requested.");
                let _status = log_worker_starting("ExportWorker");
                let ts = self.database.now_ts_for_reads();
                let in_progress_export = (*export).clone().in_progress(*ts)?;
                let in_progress_export_doc = SystemMetadataModel::new_global(&mut tx)
                    .replace(
                        export.id().to_owned(),
                        in_progress_export.clone().try_into()?,
                    )
                    .await?
                    .parse()?;
                self.database
                    .commit_with_write_source(tx, "export_worker_export_requested")
                    .await?;
                self.export(in_progress_export_doc).await?;
                return Ok(None);
            },
            (None, Some(export)) => {
                tracing::info!("In progress export restarting...");
                let _status = log_worker_starting("ExportWorker");
                self.export(export).await?;
                return Ok(None);
            },
            (None, None) => {
                tracing::info!("No exports requested or in progress.");
            },
        }
        Ok(Some(tx.into_token()?))
    }

    async fn export(&mut self, export: ParsedDocument<Export>) -> anyhow::Result<()> {
        loop {
            match self.export_and_mark_complete(export.clone()).await {
                Ok(()) => {
                    return Ok(());
                },
                Err(mut e) => {
                    if e.is::<ExportCanceled>() {
                        tracing::info!("Export {} canceled", export.id());
                        return Ok(());
                    }
                    if e.short_msg() == FILE_STORAGE_EXPORT_TOO_LARGE_SHORT_MSG {
                        log_export_failed(&e);
                        tracing::warn!(
                            "Export {} failed because its file storage exceeded the limit",
                            export.id()
                        );
                        if let Err(e) = self.mark_failed(export.id().to_owned()).await {
                            if e.is::<ExportCanceled>() {
                                tracing::info!("Export {} canceled", export.id());
                                return Ok(());
                            }
                            return Err(e);
                        }
                        return Ok(());
                    }
                    log_export_failed(&e);
                    report_error(&mut e).await;
                    let delay = self.backoff.fail(&mut self.runtime.rng());
                    tracing::error!("Export failed, retrying in {delay:?}");
                    self.runtime.wait(delay).await;
                },
            }
        }
    }

    async fn mark_failed(&self, id: ResolvedDocumentId) -> anyhow::Result<()> {
        self.database
            .execute_with_occ_retries(
                Identity::system(),
                FunctionUsageTracker::new(),
                MAX_OCC_FAILURES,
                "export_worker_mark_failed",
                |tx| {
                    async move {
                        let export: ParsedDocument<Export> =
                            tx.get(id).await?.context(ExportCanceled)?.parse()?;
                        if let Export::Canceled { .. } = *export {
                            anyhow::bail!(ExportCanceled);
                        }
                        let start_ts = match *export {
                            Export::InProgress { start_ts, .. } => start_ts,
                            _ => anyhow::bail!("Can only fail an in-progress export"),
                        };
                        let failed_export = export
                            .into_value()
                            .failed(start_ts, *tx.begin_timestamp())?;
                        SystemMetadataModel::new_global(tx)
                            .replace(id, failed_export.try_into()?)
                            .await?;
                        Ok(())
                    }
                    .boxed()
                    .into()
                },
            )
            .await?;
        Ok(())
    }

    async fn export_and_mark_complete(
        &mut self,
        export: ParsedDocument<Export>,
    ) -> anyhow::Result<()> {
        let id = export.id();
        let Export::InProgress {
            format,
            requestor,
            resumption_token,
            ..
        } = export.into_value()
        else {
            anyhow::bail!(
                "export_and_mark_complete should only be called with an InProgress export"
            );
        };
        // Drop the rest of `export` to prevent accidentally using stale state

        let database_snapshot = self.database.latest_database_snapshot()?;
        let snapshot_ts = *database_snapshot.timestamp();
        let components = ExportComponents {
            runtime: self.runtime.clone(),
            database: database_snapshot,
            exports_storage: self.exports_storage.clone(),
            file_storage: self.file_storage.clone(),
            deployment_name: self.deployment_name.clone(),
        };
        async fn modify_export<RT: Runtime>(
            database: &Database<RT>,
            id: ResolvedDocumentId,
            what: &'static str,
            f: impl FnOnce(Export) -> anyhow::Result<Export> + Send + Clone,
        ) -> anyhow::Result<()> {
            database
                .execute_with_occ_retries(
                    Identity::system(),
                    FunctionUsageTracker::new(),
                    MAX_OCC_FAILURES,
                    what,
                    move |tx| {
                        let f = f.clone();
                        async move {
                            let export: ParsedDocument<Export> =
                                tx.get(id).await?.context(ExportCanceled)?.parse()?;
                            let export = export.into_value();
                            if let Export::Canceled { .. } = export {
                                anyhow::bail!(ExportCanceled);
                            }
                            SystemMetadataModel::new_global(tx)
                                .replace(id, f(export)?.try_into()?)
                                .await?;
                            Ok(())
                        }
                        .boxed()
                        .into()
                    },
                )
                .await?;
            Ok(())
        }
        let update_progress = |msg| {
            let database_ = self.database.clone();
            async move {
                tracing::info!("Export {id} progress: {msg}");
                modify_export(&database_, id, "export_worker_update_progress", |export| {
                    export.update_progress(msg)
                })
                .await?;
                Ok(())
            }
            .boxed()
        };
        let save_resumption_token = |token| {
            let database_ = self.database.clone();
            async move {
                modify_export(
                    &database_,
                    id,
                    "export_worker_save_resumption_token",
                    |export| export.update_resumption_token(token),
                )
                .await?;
                Ok(())
            }
            .boxed()
        };
        let (object_key, usage) = {
            let export_future = async {
                if let Some(token) = resumption_token {
                    tracing::info!(?token, "Export {id} resuming...");
                    match self
                        .export_provider
                        .resume_export(self.deployment_name.clone(), token, id, &update_progress)
                        .await
                    {
                        Ok(Some(result)) => return Ok(result),
                        Ok(None) => {},
                        Err(mut err) => {
                            report_error(&mut err).await;
                        },
                    }
                    tracing::warn!("Export failed to resume");
                    // If we couldn't resume, just start a new export. We don't
                    // bother deleting the resumption token here - just assume
                    // it'll get rewritten soon.
                }
                tracing::info!(%snapshot_ts, "Export {id} beginning...");
                self.export_provider
                    .export(
                        &components,
                        format,
                        requestor,
                        id,
                        &update_progress,
                        &save_resumption_token,
                    )
                    .await
            };
            tokio::pin!(export_future);

            let database_ = self.database.clone();
            // In parallel, monitor the export document to check for cancellation
            let monitor_export = async move {
                loop {
                    let mut tx = database_.begin_system().await?;
                    let Some(export) = tx.get(id).await? else {
                        tracing::warn!("Export {id} disappeared");
                        return Err(ExportCanceled.into());
                    };
                    let export: ParsedDocument<Export> = export.parse()?;
                    match *export {
                        Export::InProgress { .. } => (),
                        Export::Canceled { .. } => return Err(ExportCanceled.into()),
                        Export::Requested { .. }
                        | Export::Failed { .. }
                        | Export::Completed { .. } => {
                            anyhow::bail!("Export {id} is in unexpected state: {export:?}");
                        },
                    }
                    let token = tx.into_token()?;
                    database_.subscribe_and_wait_for_invalidation(token).await?;
                }
            };
            tokio::pin!(monitor_export);

            futures::future::select(export_future, monitor_export)
                .await
                .factor_first()
                .0?
        };

        let object_attributes = self
            .exports_storage
            .get_object_attributes(&object_key)
            .await?
            .context("error getting export object attributes from S3")?;

        // Export is done; mark it as such.
        tracing::info!("Export {id} completed");
        self.database
            .execute_with_occ_retries(
                Identity::system(),
                FunctionUsageTracker::new(),
                MAX_OCC_FAILURES,
                "export_worker_mark_complete",
                |tx| {
                    let object_key = object_key.clone();
                    async move {
                        let Some(export) = tx.get(id).await? else {
                            tracing::warn!("Export {id} disappeared");
                            return Err(ExportCanceled.into());
                        };
                        let export: ParsedDocument<Export> = export.parse()?;
                        if let Export::Canceled { .. } = *export {
                            return Err(ExportCanceled.into());
                        }
                        let completed_export = export.into_value().completed(
                            snapshot_ts,
                            *tx.begin_timestamp(),
                            object_key,
                            object_attributes.size,
                        )?;
                        SystemMetadataModel::new_global(tx)
                            .replace(id, completed_export.try_into()?)
                            .await?;
                        Ok(())
                    }
                    .boxed()
                    .into()
                },
            )
            .await?;

        let tag = requestor.usage_tag().to_string();
        let call_type = match requestor {
            ExportRequestor::SnapshotExport => CallType::Export,
            ExportRequestor::CloudBackup => CallType::CloudBackup,
        };
        // Charge file bandwidth for the upload of the snapshot to exports storage
        usage
            .track_storage_ingress(ComponentPath::root(), tag.clone(), object_attributes.size)
            .await;
        // Charge database bandwidth accumulated during the export
        self.usage_tracking
            .track_call(
                UdfIdentifier::SystemJob(tag),
                ExecutionId::new(),
                RequestId::new(),
                call_type,
                true,
                usage.gather_user_stats(),
            )
            .await;
        Ok(())
    }
}
