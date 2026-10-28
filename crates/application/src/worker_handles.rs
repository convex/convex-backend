use std::sync::Arc;

use common::runtime::{
    shutdown_and_join,
    SpawnHandle,
};
use parking_lot::Mutex;
use search_index_workers::SearchIndexWorkers;
use usage_gauges_tracking_worker::UsageGaugesTrackingWorker;
use usage_limits::UsageLimitNotifier;

use crate::{
    scheduled_jobs::ScheduledJobRunner,
    table_summary_worker::TableSummaryClient,
};

#[derive(Clone)]
pub struct WorkerHandles {
    pub(crate) usage_gauges_tracking_worker: UsageGaugesTrackingWorker,
    pub(crate) scheduled_job_runner: ScheduledJobRunner,
    pub(crate) cron_job_executor: Arc<Mutex<Box<dyn SpawnHandle>>>,
    pub(crate) index_worker: Arc<Mutex<Option<Box<dyn SpawnHandle>>>>,
    pub(crate) fast_forward_worker: Arc<Mutex<Box<dyn SpawnHandle>>>,
    pub(crate) search_worker: Arc<Mutex<SearchIndexWorkers>>,
    pub(crate) search_and_vector_bootstrap_worker: Arc<Mutex<Box<dyn SpawnHandle>>>,
    pub(crate) table_summary_worker: TableSummaryClient,
    pub(crate) schema_worker: Arc<Mutex<Box<dyn SpawnHandle>>>,
    pub(crate) snapshot_import_worker: Arc<Mutex<Option<Box<dyn SpawnHandle>>>>,
    pub(crate) export_worker: Arc<Mutex<Option<Box<dyn SpawnHandle>>>>,
    pub(crate) s3_export_worker: Arc<Mutex<Option<Box<dyn SpawnHandle>>>>,
    pub(crate) system_table_cleanup_worker: Arc<Mutex<Box<dyn SpawnHandle>>>,
    pub(crate) migration_worker: Arc<Mutex<Option<Box<dyn SpawnHandle>>>>,
    pub(crate) usage_limit_worker: Arc<Mutex<Option<Box<dyn SpawnHandle>>>>,
    pub(crate) usage_limit_notifier: Arc<dyn UsageLimitNotifier>,
}

impl WorkerHandles {
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        self.usage_gauges_tracking_worker.shutdown().await?;
        self.table_summary_worker.shutdown().await?;
        self.system_table_cleanup_worker.lock().shutdown();
        self.schema_worker.lock().shutdown();
        let index_worker = self.index_worker.lock().take();
        if let Some(index_worker) = index_worker {
            shutdown_and_join(index_worker).await?;
        }
        self.search_worker.lock().shutdown();
        self.search_and_vector_bootstrap_worker.lock().shutdown();
        self.fast_forward_worker.lock().shutdown();
        let s3_export_worker = self.s3_export_worker.lock().take();
        if let Some(s3_export_worker) = s3_export_worker {
            shutdown_and_join(s3_export_worker).await?;
        }
        let export_worker = self.export_worker.lock().take();
        if let Some(export_worker) = export_worker {
            shutdown_and_join(export_worker).await?;
        }
        let snapshot_import_worker = self.snapshot_import_worker.lock().take();
        if let Some(snapshot_import_worker) = snapshot_import_worker {
            shutdown_and_join(snapshot_import_worker).await?;
        }
        self.scheduled_job_runner.shutdown();
        self.cron_job_executor.lock().shutdown();
        let migration_worker = self.migration_worker.lock().take();
        if let Some(migration_worker) = migration_worker {
            shutdown_and_join(migration_worker).await?;
        }
        let usage_limit_worker = self.usage_limit_worker.lock().take();
        if let Some(usage_limit_worker) = usage_limit_worker {
            shutdown_and_join(usage_limit_worker).await?;
        }
        self.usage_limit_notifier.shutdown().await;
        Ok(())
    }
}
