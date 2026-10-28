use std::{
    collections::BTreeMap,
    sync::LazyLock,
};

use common::{
    components::ComponentId,
    document::{
        ParseDocument,
        ParsedDocument,
    },
    query::{
        IndexRange,
        IndexRangeExpression,
        Order,
        Query,
    },
    runtime::Runtime,
};
use database::{
    system_tables::{
        SystemIndex,
        SystemTable,
    },
    ResolvedQuery,
    Transaction,
};
use value::{
    ConvexValue,
    DeveloperDocumentId,
    FieldPath,
    TableName,
};

use crate::migr_119::cron_jobs::types::{
    CronIdentifier,
    CronJob,
    CronJobLog,
    CronNextRun,
};

pub mod types;

pub const CRON_JOBS_TABLE: TableName = TableName::const_new("_cron_jobs");

// Used to find next jobs to execute for crons.
#[allow(dead_code)] // TODO: remove
pub static CRON_JOBS_INDEX_BY_NEXT_TS: LazyLock<SystemIndex<CronJobsTable>> =
    LazyLock::new(|| SystemIndex::new("by_next_ts", [&CRON_JOBS_NEXT_TS_FIELD]).unwrap());
// Used to find cron job by name
#[allow(dead_code)] // TODO: remove
pub static CRON_JOBS_INDEX_BY_NAME: LazyLock<SystemIndex<CronJobsTable>> =
    LazyLock::new(|| SystemIndex::new("by_name", [&CRON_JOBS_NAME_FIELD]).unwrap());
#[allow(dead_code)] // TODO: remove
static CRON_JOBS_NAME_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "name".parse().expect("invalid name field"));
#[allow(dead_code)] // TODO: remove
static CRON_JOBS_NEXT_TS_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "nextTs".parse().expect("invalid nextTs field"));

#[allow(dead_code)] // TODO: remove
pub const CRON_JOB_LOGS_TABLE: TableName = TableName::const_new("_cron_job_logs");

#[allow(dead_code)] // TODO: remove
pub static CRON_JOB_LOGS_INDEX_BY_NAME_TS: LazyLock<SystemIndex<CronJobLogsTable>> =
    LazyLock::new(|| {
        SystemIndex::new(
            "by_name_and_ts",
            [&CRON_JOB_LOGS_NAME_FIELD, &CRON_JOB_LOGS_TS_FIELD],
        )
        .unwrap()
    });
#[allow(dead_code)] // TODO: remove
pub static CRON_JOB_LOGS_NAME_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "name".parse().expect("invalid name field"));
#[allow(dead_code)] // TODO: remove
static CRON_JOB_LOGS_TS_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "ts".parse().expect("invalid ts field"));

pub const CRON_NEXT_RUN_TABLE: TableName = TableName::const_new("_cron_next_run");

pub static CRON_NEXT_RUN_INDEX_BY_NEXT_TS: LazyLock<SystemIndex<CronNextRunTable>> =
    LazyLock::new(|| SystemIndex::new("by_next_ts", [&CRON_NEXT_RUN_NEXT_TS_FIELD]).unwrap());
pub static CRON_NEXT_RUN_INDEX_BY_CRON_JOB_ID: LazyLock<SystemIndex<CronNextRunTable>> =
    LazyLock::new(|| {
        SystemIndex::new("by_cron_job_id", [&CRON_NEXT_RUN_CRON_JOB_ID_FIELD]).unwrap()
    });
static CRON_NEXT_RUN_NEXT_TS_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "nextTs".parse().expect("invalid nextTs field"));
static CRON_NEXT_RUN_CRON_JOB_ID_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "cronJobId".parse().expect("invalid cronJobId field"));

#[allow(dead_code)]
pub struct CronJobsTable;
impl SystemTable for CronJobsTable {
    type Metadata = CronJob;

    const FOR_MIGRATION: bool = true;
    const TABLE_NAME: TableName = CRON_JOBS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![
            CRON_JOBS_INDEX_BY_NEXT_TS.clone(),
            CRON_JOBS_INDEX_BY_NAME.clone(),
        ]
    }
}

#[allow(dead_code)]
pub struct CronJobLogsTable;
impl SystemTable for CronJobLogsTable {
    type Metadata = CronJobLog;

    const FOR_MIGRATION: bool = true;
    const TABLE_NAME: TableName = CRON_JOB_LOGS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![CRON_JOB_LOGS_INDEX_BY_NAME_TS.clone()]
    }
}

#[allow(dead_code)]
pub struct CronNextRunTable;
impl SystemTable for CronNextRunTable {
    type Metadata = CronNextRun;

    const FOR_MIGRATION: bool = true;
    const TABLE_NAME: TableName = CRON_NEXT_RUN_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![
            CRON_NEXT_RUN_INDEX_BY_NEXT_TS.clone(),
            CRON_NEXT_RUN_INDEX_BY_CRON_JOB_ID.clone(),
        ]
    }
}

pub struct CronModel<'a, RT: Runtime> {
    pub tx: &'a mut Transaction<RT>,
    pub component: ComponentId,
}

impl<'a, RT: Runtime> CronModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>, component: ComponentId) -> Self {
        Self { tx, component }
    }

    pub async fn next_run(
        &mut self,
        cron_job_id: DeveloperDocumentId,
    ) -> anyhow::Result<Option<ParsedDocument<CronNextRun>>> {
        let query = Query::index_range(IndexRange {
            index_name: CRON_NEXT_RUN_INDEX_BY_CRON_JOB_ID.name(),
            range: vec![IndexRangeExpression::Eq(
                CRON_NEXT_RUN_CRON_JOB_ID_FIELD.clone(),
                ConvexValue::from(cron_job_id).into(),
            )],
            order: Order::Asc,
        });
        let mut query_stream = ResolvedQuery::new(self.tx, self.component.into(), query)?;
        let next_run = query_stream.expect_at_most_one(self.tx).await?;
        next_run.map(|v| v.parse()).transpose()
    }

    pub async fn list(
        &mut self,
    ) -> anyhow::Result<BTreeMap<CronIdentifier, ParsedDocument<CronJob>>> {
        let cron_query = Query::full_table_scan(CRON_JOBS_TABLE.clone(), Order::Asc);
        let mut query_stream = ResolvedQuery::new(self.tx, self.component.into(), cron_query)?;
        let mut cron_jobs = BTreeMap::new();
        while let Some(job) = query_stream.next(self.tx, None).await? {
            let cron: ParsedDocument<CronJob> = job.parse()?;
            cron_jobs.insert(cron.name.clone(), cron);
        }
        Ok(cron_jobs)
    }
}
