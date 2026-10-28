use common::runtime::Runtime;
use cron_jobs::{
    types::CronNextRun,
    CronModel,
    CRON_JOBS_TABLE,
    CRON_NEXT_RUN_TABLE,
};
use database::{
    SystemMetadataModel,
    Transaction,
};

mod cron_jobs;

pub async fn run_migration<RT: Runtime>(tx: &mut Transaction<RT>) -> anyhow::Result<()> {
    let namespaces: Vec<_> = tx
        .table_mapping()
        .iter()
        .filter_map(|(_, namespace, _, table_name)| {
            if table_name == &CRON_JOBS_TABLE {
                Some(namespace)
            } else {
                None
            }
        })
        .collect();

    for namespace in namespaces {
        let crons = CronModel::new(tx, namespace.into()).list().await?;
        for cron in crons.values() {
            let next_run = CronNextRun {
                cron_job_id: cron.id().developer_id,
                state: cron.state,
                prev_ts: cron.prev_ts,
                next_ts: cron.next_ts,
            };
            if let Some(existing_next_run) = CronModel::new(tx, namespace.into())
                .next_run(cron.id().developer_id)
                .await?
                .map(|next_run| next_run.into_value())
            {
                if existing_next_run != next_run {
                    SystemMetadataModel::new(tx, namespace)
                        .replace(cron.id(), next_run.try_into()?)
                        .await?;
                }
            } else {
                // If there's no existing next run, create a new one.
                SystemMetadataModel::new(tx, namespace)
                    .insert(&CRON_NEXT_RUN_TABLE, next_run.try_into()?)
                    .await?;
            }
        }
    }
    Ok(())
}
