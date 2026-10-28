use std::{
    future::Future,
    sync::Arc,
    time::Duration,
};

use common::{
    backoff::Backoff,
    errors::report_error,
    runtime::Runtime,
};
use database::{
    Database,
    Token,
};
use file_storage::FileStorage;
use keybroker::Identity;
use model::snapshot_imports::{
    types::ImportState,
    SnapshotImportModel,
};
use storage::Storage;
use usage_tracking::UsageCounter;

use crate::{
    metrics::log_worker_starting,
    snapshot_import::{
        metrics::{
            log_snapshot_import_failed,
            snapshot_import_timer,
        },
        SnapshotImportExecutor,
    },
};

const INITIAL_BACKOFF: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

pub struct SnapshotImportWorker;

impl SnapshotImportWorker {
    pub fn start<RT: Runtime>(
        runtime: RT,
        database: Database<RT>,
        snapshot_imports_storage: Arc<dyn Storage>,
        file_storage: FileStorage<RT>,
        usage_tracking: UsageCounter,
    ) -> impl Future<Output = ()> + Send {
        let mut worker = SnapshotImportExecutor {
            runtime,
            database,
            snapshot_imports_storage,
            file_storage,
            usage_tracking,
            backoff: Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF),
        };
        async move {
            loop {
                let result: anyhow::Result<()> = async {
                    let token = Box::pin(Self::run_once(&mut worker)).await?;
                    worker
                        .database
                        .subscribe_and_wait_for_invalidation(token)
                        .await?;
                    Ok(())
                }
                .await;
                if let Err(e) = result {
                    log_snapshot_import_failed(&e);
                    report_error(&mut e.context("SnapshotImportWorker died")).await;
                    let delay = worker.backoff.fail(&mut worker.runtime.rng());
                    worker.runtime.wait(delay).await;
                } else {
                    worker.backoff.reset();
                }
            }
        }
    }

    /// Subscribe to the _snapshot_imports table.
    /// If an import has Uploaded, parse it and set to WaitingForConfirmation.
    /// If an import is InProgress, execute it.
    async fn run_once<RT: Runtime>(
        executor: &mut SnapshotImportExecutor<RT>,
    ) -> anyhow::Result<Token> {
        let _status = log_worker_starting("SnapshotImport");
        let mut tx = executor.database.begin(Identity::system()).await?;
        let mut import_model = SnapshotImportModel::new(&mut tx);
        let import_uploaded = import_model.import_in_state(ImportState::Uploaded).await?;
        let import_in_progress = import_model
            .import_in_state(ImportState::InProgress {
                progress_message: String::new(),
                checkpoint_messages: vec![],
            })
            .await?;
        let token = tx.into_token()?;

        if let Some(import_uploaded) = import_uploaded {
            executor.handle_uploaded_state(import_uploaded).await?;
        } else if let Some(import_in_progress) = import_in_progress {
            tracing::info!("Executing in-progress snapshot import");
            let timer = snapshot_import_timer();
            executor
                .handle_in_progress_state(import_in_progress)
                .await?;
            timer.finish();
        }
        Ok(token)
    }
}
