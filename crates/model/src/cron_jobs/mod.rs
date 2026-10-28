use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        LazyLock,
    },
};

use anyhow::Context;
use common::{
    components::ComponentId,
    document::{
        ParseDocument,
        ParsedDocument,
        ResolvedDocument,
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
    ResolvedQuery,
    SystemMetadataModel,
    Transaction,
};
use futures_async_stream::try_stream;
use sync_types::CanonicalizedModulePath;
use types::CronJobMetadata;
use value::{
    heap_size::WithHeapSize,
    ConvexValue,
    DeveloperDocumentId,
    FieldPath,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

use crate::{
    config::types::CronDiff,
    cron_jobs::{
        next_ts::compute_next_ts,
        types::{
            CronIdentifier,
            CronJob,
            CronJobLog,
            CronJobLogLines,
            CronJobState,
            CronJobStatus,
            CronNextRun,
            CronSpec,
        },
    },
    modules::module_versions::AnalyzedModule,
    SystemIndex,
    SystemTable,
};

pub mod next_ts;
pub mod types;

pub const CRON_JOBS_TABLE: TableName = TableName::const_new("_cron_jobs");

// Used to find cron job by name
pub static CRON_JOBS_INDEX_BY_NAME: LazyLock<SystemIndex<CronJobsTable>> =
    LazyLock::new(|| SystemIndex::new("by_name", [&CRON_JOBS_NAME_FIELD]).unwrap());
static CRON_JOBS_NAME_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "name".parse().expect("invalid name field"));

pub const CRON_JOB_LOGS_TABLE: TableName = TableName::const_new("_cron_job_logs");

pub static CRON_JOB_LOGS_INDEX_BY_NAME_TS: LazyLock<SystemIndex<CronJobLogsTable>> =
    LazyLock::new(|| {
        SystemIndex::new(
            "by_name_and_ts",
            [&CRON_JOB_LOGS_NAME_FIELD, &CRON_JOB_LOGS_TS_FIELD],
        )
        .unwrap()
    });
pub static CRON_JOB_LOGS_NAME_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "name".parse().expect("invalid name field"));
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

pub struct CronJobsTable;
impl SystemTable for CronJobsTable {
    type Metadata = CronJobMetadata;

    const TABLE_NAME: TableName = CRON_JOBS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![CRON_JOBS_INDEX_BY_NAME.clone()]
    }
}

pub struct CronJobLogsTable;
impl SystemTable for CronJobLogsTable {
    type Metadata = CronJobLog;

    const TABLE_NAME: TableName = CRON_JOB_LOGS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![CRON_JOB_LOGS_INDEX_BY_NAME_TS.clone()]
    }
}

pub struct CronNextRunTable;
impl SystemTable for CronNextRunTable {
    type Metadata = CronNextRun;

    const TABLE_NAME: TableName = CRON_NEXT_RUN_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![
            CRON_NEXT_RUN_INDEX_BY_NEXT_TS.clone(),
            CRON_NEXT_RUN_INDEX_BY_CRON_JOB_ID.clone(),
        ]
    }
}

const MAX_LOGS_PER_CRON: usize = 5;

pub struct CronModel<'a, RT: Runtime> {
    pub tx: &'a mut Transaction<RT>,
    pub component: ComponentId,
}

impl<'a, RT: Runtime> CronModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>, component: ComponentId) -> Self {
        Self { tx, component }
    }

    #[fastrace::trace]
    pub async fn apply(
        &mut self,
        analyze_results: &BTreeMap<CanonicalizedModulePath, AnalyzedModule>,
    ) -> anyhow::Result<CronDiff> {
        let crons_js = "crons.js".parse()?;
        let new_crons: WithHeapSize<BTreeMap<CronIdentifier, CronSpec>> =
            if let Some(module) = analyze_results.get(&crons_js) {
                module.cron_specs.clone().unwrap_or_default()
            } else {
                WithHeapSize::default()
            };

        let old_crons = self.list_metadata().await?;
        let mut added_crons: Vec<&CronIdentifier> = vec![];
        let mut updated_crons: Vec<&CronIdentifier> = vec![];
        let mut deleted_crons: Vec<&CronIdentifier> = vec![];
        for (name, cron_spec) in &new_crons {
            match old_crons.get(&name.clone()) {
                Some(cron_job) => {
                    if cron_job.cron_spec != cron_spec.clone() {
                        self.update(cron_job.clone(), cron_spec.clone()).await?;
                        updated_crons.push(name);
                    }
                },
                None => {
                    self.create(name.clone(), cron_spec.clone()).await?;
                    added_crons.push(name);
                },
            }
        }
        for (name, cron_job) in &old_crons {
            match new_crons.get(&name.clone()) {
                Some(_) => {},
                None => {
                    self.delete(cron_job.clone()).await?;
                    deleted_crons.push(name);
                },
            }
        }
        tracing::info!(
            "Crons Added: {added_crons:?}, Updated: {updated_crons:?}, Deleted: {deleted_crons:?}"
        );
        let cron_diff = CronDiff::new(added_crons, updated_crons, deleted_crons);
        Ok(cron_diff)
    }

    pub async fn create(
        &mut self,
        name: CronIdentifier,
        cron_spec: CronSpec,
    ) -> anyhow::Result<()> {
        let now = self.runtime().generate_timestamp()?;
        let next_ts = compute_next_ts(&cron_spec, None, now, &mut self.runtime().rng())?;
        let cron = CronJobMetadata { name, cron_spec };

        let cron_job_id = SystemMetadataModel::new(self.tx, self.component.into())
            .insert(&CRON_JOBS_TABLE, cron.try_into()?)
            .await?
            .developer_id;

        let next_run = CronNextRun {
            cron_job_id,
            state: CronJobState::Pending,
            prev_ts: None,
            next_ts,
        };

        SystemMetadataModel::new(self.tx, self.component.into())
            .insert(&CRON_NEXT_RUN_TABLE, next_run.try_into()?)
            .await?;

        Ok(())
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
        query_stream
            .expect_at_most_one(self.tx)
            .await?
            .map(|v| v.parse())
            .transpose()
    }

    async fn update(
        &mut self,
        mut cron_job: ParsedDocument<CronJobMetadata>,
        new_cron_spec: CronSpec,
    ) -> anyhow::Result<()> {
        if new_cron_spec.cron_schedule != cron_job.cron_spec.cron_schedule {
            // Skip updating the next run ts, if the runs are close together on the old
            // schedule. This is a heuristic to avoid OCC with existing cron
            // jobs running/changing state. True solution would be to move this
            // logic to the async worker, but quickfix for now is to skip the
            // `update_job_state`.
            let now = self.runtime().generate_timestamp()?;
            let next_ts =
                compute_next_ts(&cron_job.cron_spec, None, now, &mut self.runtime().rng())?;
            let next_next_run = compute_next_ts(
                &cron_job.cron_spec,
                Some(next_ts),
                next_ts,
                &mut self.runtime().rng(),
            )?;
            if next_next_run.secs_since_f64(now) > 30.0 {
                // Read in next-run to the readset and update it.
                let mut next_run = self
                    .next_run(cron_job.id().developer_id)
                    .await?
                    .context("No next run found")?
                    .into_value();

                // Recalculate on the new schedule.
                let now = self.runtime().generate_timestamp()?;
                next_run.next_ts =
                    compute_next_ts(&new_cron_spec, None, now, &mut self.runtime().rng())?;
                self.update_job_state(next_run).await?;
            }
        }
        cron_job.cron_spec = new_cron_spec;
        SystemMetadataModel::new(self.tx, self.component.into())
            .replace(cron_job.id(), cron_job.into_value().try_into()?)
            .await?;
        Ok(())
    }

    pub async fn delete(
        &mut self,
        cron_job: ParsedDocument<CronJobMetadata>,
    ) -> anyhow::Result<()> {
        SystemMetadataModel::new(self.tx, self.component.into())
            .delete(cron_job.id())
            .await?;
        let next_run = self
            .next_run(cron_job.id().developer_id)
            .await?
            .context("No next run found")?;
        SystemMetadataModel::new(self.tx, self.component.into())
            .delete(next_run.id())
            .await?;
        self.apply_job_log_retention(&cron_job.name, 0).await?;
        Ok(())
    }

    pub async fn update_job_state(&mut self, next_run: CronNextRun) -> anyhow::Result<()> {
        let existing_next_run = self
            .next_run(next_run.cron_job_id)
            .await?
            .context("No next run found")?;
        SystemMetadataModel::new(self.tx, self.component.into())
            .replace(existing_next_run.id(), next_run.try_into()?)
            .await?;
        Ok(())
    }

    /// Create space in the CronJobLogsTable for a new run of `job`
    pub async fn prepare_insert_cron_job_log(&mut self, job: &CronJob) -> anyhow::Result<()> {
        self.apply_job_log_retention(&job.name, MAX_LOGS_PER_CRON - 1)
            .await?;
        Ok(())
    }

    pub async fn insert_cron_job_log(
        &mut self,
        job: &CronJob,
        status: CronJobStatus,
        log_lines: CronJobLogLines,
        execution_time: f64,
    ) -> anyhow::Result<()> {
        let cron_job_log = CronJobLog {
            name: job.name.clone(),
            ts: job.next_ts,
            udf_path: job.cron_spec.udf_path.clone(),
            udf_args: job.cron_spec.udf_args.clone(),
            status,
            log_lines,
            execution_time,
        };
        // A mutation's result may contain unresolved commit timestamps; the
        // log is written in the mutation's own transaction, so the committer
        // resolves them right alongside the mutation's writes.
        SystemMetadataModel::new(self.tx, self.component.into())
            .insert_metadata_pending(&CRON_JOB_LOGS_TABLE, cron_job_log.try_into()?)
            .await?;
        self.apply_job_log_retention(&job.name, MAX_LOGS_PER_CRON)
            .await?;
        Ok(())
    }

    pub async fn get(&mut self, id: ResolvedDocumentId) -> anyhow::Result<Option<CronJob>> {
        let Some(job) = self.tx.get(id).await? else {
            return Ok(None);
        };
        let cron: ParsedDocument<CronJobMetadata> = job.parse()?;
        let next_run = self
            .next_run(id.developer_id)
            .await?
            .context("No next run found")?
            .into_value();
        Ok(Some(CronJob::new(cron, self.component, next_run)))
    }

    pub async fn list(&mut self) -> anyhow::Result<BTreeMap<CronIdentifier, CronJob>> {
        let cron_query = Query::full_table_scan(CRON_JOBS_TABLE.clone(), Order::Asc);
        let mut query_stream = ResolvedQuery::new(self.tx, self.component.into(), cron_query)?;
        let mut cron_jobs = BTreeMap::new();
        while let Some(job) = query_stream.next(self.tx, None).await? {
            let cron: ParsedDocument<CronJobMetadata> = job.parse()?;
            let next_run = self
                .next_run(cron.id().developer_id)
                .await?
                .context("No next run found")?
                .into_value();
            cron_jobs.insert(
                cron.name.clone(),
                CronJob::new(cron, self.component, next_run),
            );
        }
        Ok(cron_jobs)
    }

    pub async fn list_metadata(
        &mut self,
    ) -> anyhow::Result<BTreeMap<CronIdentifier, ParsedDocument<CronJobMetadata>>> {
        let cron_query = Query::full_table_scan(CRON_JOBS_TABLE.clone(), Order::Asc);
        let mut query_stream = ResolvedQuery::new(self.tx, self.component.into(), cron_query)?;
        let mut cron_jobs = BTreeMap::new();
        while let Some(job) = query_stream.next(self.tx, None).await? {
            let cron: ParsedDocument<CronJobMetadata> = job.parse()?;
            cron_jobs.insert(cron.name.clone(), cron);
        }
        Ok(cron_jobs)
    }

    fn runtime(&self) -> &RT {
        self.tx.runtime()
    }

    // Keep up to `limit` of the newest logs per cron
    async fn apply_job_log_retention(
        &mut self,
        name: &CronIdentifier,
        limit: usize,
    ) -> anyhow::Result<()> {
        let index_query = Query::index_range(IndexRange {
            index_name: CRON_JOB_LOGS_INDEX_BY_NAME_TS.name(),
            range: vec![IndexRangeExpression::Eq(
                CRON_JOB_LOGS_NAME_FIELD.clone(),
                ConvexValue::try_from(name.to_string())?.into(),
            )],
            order: Order::Desc,
        });
        let mut query_stream = ResolvedQuery::new(self.tx, self.component.into(), index_query)?;
        let mut num_logs = 0;
        let mut to_delete = Vec::new();
        while let Some(doc) = query_stream.next(self.tx, None).await? {
            num_logs += 1;
            if num_logs > limit {
                to_delete.push(doc.id());
            }
        }
        for doc_id in to_delete.into_iter() {
            SystemMetadataModel::new(self.tx, self.component.into())
                .delete(doc_id)
                .await?;
        }
        Ok(())
    }
}

#[try_stream(boxed, ok = CronJob, error = anyhow::Error)]
pub async fn stream_cron_jobs_to_run<'a, RT: Runtime>(tx: &'a mut Transaction<RT>) {
    let namespaces: Vec<_> = tx
        .table_mapping()
        .iter()
        .filter(|(_, _, _, name)| **name == CRON_JOBS_TABLE)
        .map(|(_, namespace, ..)| namespace)
        .collect();
    let index_query = Query::index_range(IndexRange {
        index_name: CRON_NEXT_RUN_INDEX_BY_NEXT_TS.name(),
        range: vec![],
        order: Order::Asc,
    });
    // Key is (next_ts, namespace), where next_ts is for sorting and namespace
    // is for deduping.
    // Value is (job, query) where job is the job to run and query will get
    // the next job to run in that namespace.
    let mut queries = BTreeMap::new();
    async fn cron_from_doc<RT: Runtime>(
        namespace: TableNamespace,
        doc: ResolvedDocument,
        tx: &mut Transaction<RT>,
    ) -> anyhow::Result<CronJob> {
        let next_run: ParsedDocument<CronNextRun> = doc.parse()?;
        let job = tx
            .get_system::<CronJobsTable>(namespace, next_run.cron_job_id)
            .await?
            .context("No cron job found")?;
        Ok(CronJob::new(
            Arc::unwrap_or_clone(job),
            namespace.into(),
            next_run.into_value(),
        ))
    }

    // Initialize streaming query for each namespace
    for namespace in namespaces {
        let mut query = ResolvedQuery::new(tx, namespace, index_query.clone())?;
        if let Some(doc) = query.next(tx, None).await? {
            let cron_job = cron_from_doc(namespace, doc, tx).await?;
            queries.insert((cron_job.next_ts, namespace), (cron_job, query));
        }
    }

    // Process each namespace in order of next_ts
    while let Some(((_min_next_ts, namespace), (min_job, mut query))) = queries.pop_first() {
        yield min_job;
        if let Some(doc) = query.next(tx, None).await? {
            let cron_job = cron_from_doc(namespace, doc, tx).await?;
            queries.insert((cron_job.next_ts, namespace), (cron_job, query));
        }
    }
}
