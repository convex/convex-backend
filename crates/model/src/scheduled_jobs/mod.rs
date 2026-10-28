use std::{
    cmp,
    sync::{
        Arc,
        LazyLock,
    },
};

use anyhow::Context;
use common::{
    components::CanonicalizedComponentFunctionPath,
    document::{
        ParseDocument,
        ParsedDocument,
        MAX_USER_SIZE,
    },
    execution_context::ExecutionContext,
    maybe_val,
    query::{
        Expression,
        IndexRange,
        IndexRangeExpression,
        Order,
        Query,
    },
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    types::IndexName,
    version::Version,
    virtual_system_mapping::AssociatedVirtualTable,
};
use database::{
    query::TableFilter,
    unauthorized_error,
    ResolvedQuery,
    SystemMetadataModel,
    Transaction,
};
use errors::ErrorMetadata;
use imbl::ordmap;
use sync_types::Timestamp;
use value::{
    id_v6::DeveloperDocumentId,
    ConvexArray,
    ConvexValue,
    FieldPath,
    ResolvedDocumentId,
    Size,
    TableName,
    TableNamespace,
};

use self::{
    types::{
        ScheduledJobAttempts,
        ScheduledJobMetadata,
        ScheduledJobState,
    },
    virtual_table::ScheduledJobsDocMapper,
};
use crate::{
    scheduled_jobs::{
        args::{
            ScheduledJobArgsTable,
            SCHEDULED_JOBS_ARGS_TABLE,
        },
        types::{
            ScheduledJob,
            ScheduledJobArgs,
        },
    },
    SystemIndex,
    SystemTable,
};

pub mod args;
pub mod types;
pub mod virtual_table;

pub const SCHEDULED_JOBS_TABLE: TableName = TableName::const_new("_scheduled_jobs");

pub const SCHEDULED_JOBS_VIRTUAL_TABLE: TableName = TableName::const_new("_scheduled_functions");

/// Clients on this version or newer may not cancel themselves.
/// Older clients are warned that it will change.
pub static MIN_NPM_VERSION_MUTATION_SELF_CANCEL: LazyLock<Version> =
    LazyLock::new(|| Version::new(1, 50, 0));
const SCHEDULED_JOBS_INDEX_BY_ID: IndexName = IndexName::by_id(SCHEDULED_JOBS_TABLE);
const SCHEDULED_JOBS_INDEX_BY_CREATION_TIME: IndexName =
    IndexName::by_creation_time(SCHEDULED_JOBS_TABLE);
const SCHEDULED_JOBS_VIRTUAL_INDEX_BY_ID: IndexName =
    IndexName::by_id(SCHEDULED_JOBS_VIRTUAL_TABLE);
const SCHEDULED_JOBS_VIRTUAL_INDEX_BY_CREATION_TIME: IndexName =
    IndexName::by_creation_time(SCHEDULED_JOBS_VIRTUAL_TABLE);

/// By next ts. Used to efficiently find next jobs to execute next.
pub static SCHEDULED_JOBS_INDEX: LazyLock<SystemIndex<ScheduledJobsTable>> =
    LazyLock::new(|| SystemIndex::new("by_next_ts", [&NEXT_TS_FIELD]).unwrap());
/// By udf path and next ts. Used by the dashboard to group scheduled jobs by
/// udf function.
pub static SCHEDULED_JOBS_INDEX_BY_UDF_PATH: LazyLock<SystemIndex<ScheduledJobsTable>> =
    LazyLock::new(|| {
        SystemIndex::new(
            "by_udf_path_and_next_event_ts",
            [&UDF_PATH_FIELD, &NEXT_TS_FIELD],
        )
        .unwrap()
    });
/// By completed ts. Used to efficiently find jobs to garbage collect.
pub static SCHEDULED_JOBS_INDEX_BY_COMPLETED_TS: LazyLock<SystemIndex<ScheduledJobsTable>> =
    LazyLock::new(|| SystemIndex::new("by_completed_ts", [&COMPLETED_TS_FIELD]).unwrap());
pub static NEXT_TS_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "nextTs".parse().expect("invalid nextTs field"));
pub static COMPLETED_TS_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "completedTs".parse().expect("invalid completedTs field"));
static UDF_PATH_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "udfPath".parse().expect("invalid udfPath field"));
static COMPONENT_PATH_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "component".parse().expect("invalid component field"));

pub struct ScheduledJobsTable;
impl SystemTable for ScheduledJobsTable {
    type Metadata = ScheduledJobMetadata;

    const TABLE_NAME: TableName = SCHEDULED_JOBS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![
            SCHEDULED_JOBS_INDEX_BY_COMPLETED_TS.clone(),
            SCHEDULED_JOBS_INDEX.clone(),
            SCHEDULED_JOBS_INDEX_BY_UDF_PATH.clone(),
        ]
    }

    fn virtual_table() -> Option<AssociatedVirtualTable> {
        Some(AssociatedVirtualTable::Primary {
            virtual_table_name: SCHEDULED_JOBS_VIRTUAL_TABLE,
            virtual_to_system_indexes: ordmap! {
                SCHEDULED_JOBS_VIRTUAL_INDEX_BY_CREATION_TIME =>
                    SCHEDULED_JOBS_INDEX_BY_CREATION_TIME,
                SCHEDULED_JOBS_VIRTUAL_INDEX_BY_ID =>
                    SCHEDULED_JOBS_INDEX_BY_ID
            },
            doc_mapper: Arc::new(ScheduledJobsDocMapper),
        })
    }
}

// Maintains state for scheduling asynchronous functions (scheduled jobs).
pub struct SchedulerModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
    namespace: TableNamespace,
}

impl<'a, RT: Runtime> SchedulerModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>, namespace: TableNamespace) -> Self {
        Self { tx, namespace }
    }

    fn check_scheduling_limits(&mut self, args: &ConvexArray) -> anyhow::Result<()> {
        let size = args.size();
        let max_scheduled = self.tx.transaction_limits().functions_scheduled;
        let max_scheduled_bytes = self.tx.transaction_limits().scheduled_function_args_bytes;
        // Limit how much you can schedule from a single transaction.
        anyhow::ensure!(
            self.tx.scheduled_size.num_writes < max_scheduled,
            ErrorMetadata::bad_request(
                "TooManyFunctionsScheduled",
                format!(
                    "Too many functions scheduled by this mutation (limit: {})",
                    max_scheduled,
                )
            )
        );
        anyhow::ensure!(
            self.tx.scheduled_size.size + size <= max_scheduled_bytes,
            ErrorMetadata::bad_request(
                "ScheduledFunctionsArgumentsTooLarge",
                format!(
                    "Too large total size of the arguments of scheduled functions from this \
                     mutation (limit: {} bytes)",
                    max_scheduled_bytes,
                )
            ),
        );
        if size > MAX_USER_SIZE {
            tracing::warn!("Scheduling a job with argument size {size}");
        }
        // TODO: hard-enforce MAX_SCHEDULED_JOB_ARGUMENT_SIZE_BYTES
        self.tx.scheduled_size.num_writes += 1;
        self.tx.scheduled_size.size += size;
        self.tx.scheduled_size.max_args_size = cmp::max(self.tx.scheduled_size.max_args_size, size);
        Ok(())
    }

    /// Join with the `_scheduled_jobs_args` table in this namespace to get the
    /// arguments for the job.
    pub async fn scheduled_job_from_metadata(
        &mut self,
        job_id: ResolvedDocumentId,
        metadata: ScheduledJobMetadata,
    ) -> anyhow::Result<ScheduledJob> {
        let scheduled_job = if let Some(args_id) = metadata.args_id {
            let doc = self
                .tx
                .get_system::<ScheduledJobArgsTable>(self.namespace, args_id)
                .await?
                .with_context(|| format!("Missing scheduled job args document for id {args_id}"))?;
            let args = Arc::unwrap_or_clone(doc);
            ScheduledJob {
                path: metadata.path,
                udf_args_bytes: args.into_value().args,
                state: metadata.state,
                next_ts: metadata.next_ts,
                completed_ts: metadata.completed_ts,
                original_scheduled_ts: metadata.original_scheduled_ts,
                attempts: metadata.attempts,
            }
        } else {
            let args = metadata.udf_args_bytes.with_context(|| {
                format!("Missing udf_args_bytes in scheduled job metadata with id {job_id}",)
            })?;
            ScheduledJob {
                path: metadata.path,
                udf_args_bytes: args,
                state: metadata.state,
                next_ts: metadata.next_ts,
                completed_ts: metadata.completed_ts,
                original_scheduled_ts: metadata.original_scheduled_ts,
                attempts: metadata.attempts,
            }
        };
        Ok(scheduled_job)
    }

    pub async fn schedule(
        &mut self,
        path: CanonicalizedComponentFunctionPath,
        args: ConvexArray,
        ts: UnixTimestamp,
        context: ExecutionContext,
    ) -> anyhow::Result<ResolvedDocumentId> {
        if path.udf_path.is_system()
            && !(self.tx.identity().is_admin() || self.tx.identity().is_system())
        {
            anyhow::bail!(unauthorized_error("schedule"))
        }

        self.check_scheduling_limits(&args)?;

        let now: Timestamp = self.tx.runtime().generate_timestamp()?;
        let original_scheduled_ts: Timestamp = ts.as_system_time().try_into()?;
        let args_id = {
            let mut model = SystemMetadataModel::new(self.tx, self.namespace);
            model
                .insert_metadata(
                    &SCHEDULED_JOBS_ARGS_TABLE,
                    ScheduledJobArgs::try_from(args)?.try_into()?,
                )
                .await?
        };
        let scheduled_job = ScheduledJobMetadata::new(
            path.clone(),
            args_id.developer_id,
            ScheduledJobState::Pending,
            // Don't set next_ts in the past to avoid scheduler incorrectly logging
            // it is falling behind. We should keep `original_scheduled_ts` intact
            // since this is exposed to the developer via the virtual table.
            Some(original_scheduled_ts.max(now)),
            None,
            original_scheduled_ts,
            ScheduledJobAttempts::default(),
        )?;
        let job = if let Some((parent_component_id, parent_scheduled_job)) =
            context.parent_scheduled_job
        {
            let table_mapping = self.tx.table_mapping().clone();
            let current_namespace = self.namespace;
            let parent_namespace = TableNamespace::from(parent_component_id);
            let mut get_parent_scheduled_job_state = async |namespace: TableNamespace| {
                let Ok(parent_scheduled_job) = parent_scheduled_job
                    .to_resolved(table_mapping.namespace(namespace).number_to_tablet())
                else {
                    return anyhow::Ok(None);
                };
                self.check_status(parent_scheduled_job).await
            };
            // Try both `parent_namespace` and `self.namespace` because there may be
            // version skew where `parent_namespace` is incorrectly the Root namespace.
            // TODO: once version skew is not an issue, only check `parent_namespace`.
            let parent_scheduled_job_state =
                match get_parent_scheduled_job_state(parent_namespace).await? {
                    Some(state) => Some(state),
                    None => get_parent_scheduled_job_state(current_namespace).await?,
                };
            if let Some(parent_scheduled_job_state) = parent_scheduled_job_state {
                match parent_scheduled_job_state {
                    ScheduledJobState::Pending
                    | ScheduledJobState::InProgress { .. }
                    | ScheduledJobState::Failed(_)
                    | ScheduledJobState::Success => scheduled_job,
                    ScheduledJobState::Canceled => {
                        let scheduled_ts = self.tx.begin_timestamp();
                        ScheduledJobMetadata::new(
                            path,
                            args_id.developer_id,
                            ScheduledJobState::Canceled,
                            None,
                            Some(*scheduled_ts),
                            *scheduled_ts,
                            ScheduledJobAttempts::default(),
                        )?
                    },
                }
            } else {
                scheduled_job
            }
        } else {
            scheduled_job
        };
        let id = SystemMetadataModel::new(self.tx, self.namespace)
            .insert_metadata(&SCHEDULED_JOBS_TABLE, job.try_into()?)
            .await?;

        Ok(id)
    }

    pub async fn replace(
        &mut self,
        id: ResolvedDocumentId,
        job: ScheduledJobMetadata,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self
            .tx
            .table_mapping()
            .namespace(self.namespace)
            .tablet_matches_name(id.tablet_id, &SCHEDULED_JOBS_TABLE));
        SystemMetadataModel::new(self.tx, self.namespace)
            .replace(id, job.try_into()?)
            .await?;
        Ok(())
    }

    pub async fn complete(
        &mut self,
        id: ResolvedDocumentId,
        state: ScheduledJobState,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self
            .tx
            .table_mapping()
            .namespace(self.namespace)
            .tablet_matches_name(id.tablet_id, &SCHEDULED_JOBS_TABLE));
        match state {
            ScheduledJobState::InProgress { .. } | ScheduledJobState::Pending => {
                anyhow::bail!("invalid state for completing a scheduled job")
            },
            ScheduledJobState::Canceled
            | ScheduledJobState::Failed(_)
            | ScheduledJobState::Success => {},
        }
        let Some(job) = self.tx.get(id).await? else {
            anyhow::bail!("scheduled job not found")
        };
        let job: ParsedDocument<ScheduledJobMetadata> = job.parse()?;
        match job.state {
            ScheduledJobState::Pending | ScheduledJobState::InProgress { .. } => {},
            ScheduledJobState::Canceled => {
                // If the job is already canceled. Completing is a no-op. We
                // should proceed without throwing an error.
                return Ok(());
            },
            ScheduledJobState::Failed(_) | ScheduledJobState::Success => {
                anyhow::bail!(
                    "Scheduled job cannot be completed because it is in state {:?}",
                    job.state
                )
            },
        }

        let mut job: ScheduledJobMetadata = job.into_value();
        job.state = state;
        // Remove next_ts and set completed_ts so the scheduler knows that the
        // job has already been processed
        job.next_ts = None;
        job.completed_ts = Some(*self.tx.begin_timestamp());
        SystemMetadataModel::new(self.tx, self.namespace)
            .replace(id, job.try_into()?)
            .await?;

        Ok(())
    }

    /// Cancel a scheduled job if it is in Pending or InProgress state.
    /// Otherwise, it has already been completed in another transaction.
    pub async fn cancel(&mut self, id: ResolvedDocumentId) -> anyhow::Result<()> {
        if let Some(scheduled_job) = self.check_status(id).await? {
            match scheduled_job {
                ScheduledJobState::Pending | ScheduledJobState::InProgress { .. } => {
                    self.complete(id, ScheduledJobState::Canceled).await?;
                },
                ScheduledJobState::Canceled
                | ScheduledJobState::Success
                | ScheduledJobState::Failed(_) => {},
            }
        } else {
            tracing::error!("Tried to cancel a job with unknown state: {}", id)
        }
        Ok(())
    }

    pub async fn delete(&mut self, id: ResolvedDocumentId) -> anyhow::Result<()> {
        anyhow::ensure!(self
            .tx
            .table_mapping()
            .namespace(self.namespace)
            .tablet_matches_name(id.tablet_id, &SCHEDULED_JOBS_TABLE));
        let doc: ParsedDocument<ScheduledJobMetadata> = self.tx.delete_inner(id).await?.parse()?;
        if let Some(args_id) = doc.args_id {
            let scheduled_args_tablet = self
                .tx
                .table_mapping()
                .namespace(self.namespace)
                .number_to_tablet()(args_id.table())?;
            self.tx
                .delete_inner(ResolvedDocumentId::new(scheduled_args_tablet, args_id))
                .await?;
        }
        Ok(())
    }

    // Cancel up to `limit` jobs for the UDF and return how many were canceled.
    // Note: the caller will assume all have been canceled if Result < `limit`.
    pub async fn cancel_all(
        &mut self,
        path: Option<CanonicalizedComponentFunctionPath>,
        limit: usize,
        start_next_ts: Option<Timestamp>,
        end_next_ts: Option<Timestamp>,
    ) -> anyhow::Result<usize> {
        let index_query = match path {
            Some(path) => {
                let udf_path = path.udf_path;
                let component_path = path.component;
                let mut component_path_filter = Expression::Eq(
                    Expression::Field(COMPONENT_PATH_FIELD.clone()).into(),
                    Expression::Literal(maybe_val!(String::from(component_path.clone()))).into(),
                );
                if component_path.is_root() {
                    component_path_filter = Expression::Or(vec![
                        component_path_filter,
                        Expression::Eq(
                            Expression::Field(COMPONENT_PATH_FIELD.clone()).into(),
                            Expression::Literal(maybe_val!(undefined)).into(),
                        ),
                    ]);
                }
                let range = vec![
                    IndexRangeExpression::Eq(
                        UDF_PATH_FIELD.clone(),
                        ConvexValue::try_from(udf_path.to_string())?.into(),
                    ),
                    IndexRangeExpression::Gte(
                        NEXT_TS_FIELD.clone(),
                        i64::from(start_next_ts.unwrap_or(Timestamp::MIN)).into(),
                    ),
                    IndexRangeExpression::Lt(
                        NEXT_TS_FIELD.clone(),
                        i64::from(end_next_ts.unwrap_or(Timestamp::MAX)).into(),
                    ),
                ];
                Query::index_range(IndexRange {
                    index_name: SCHEDULED_JOBS_INDEX_BY_UDF_PATH.name(),
                    range,
                    order: Order::Asc,
                })
                .filter(component_path_filter)
            },
            None => {
                let range = vec![
                    IndexRangeExpression::Gte(
                        NEXT_TS_FIELD.clone(),
                        i64::from(start_next_ts.unwrap_or(Timestamp::MIN)).into(),
                    ),
                    IndexRangeExpression::Lt(
                        NEXT_TS_FIELD.clone(),
                        i64::from(end_next_ts.unwrap_or(Timestamp::MAX)).into(),
                    ),
                ];
                Query::index_range(IndexRange {
                    index_name: SCHEDULED_JOBS_INDEX.name(),
                    range,
                    order: Order::Asc,
                })
            },
        };
        let mut query_stream = ResolvedQuery::new(self.tx, self.namespace, index_query)?;
        let mut count = 0;
        while count < limit
            && let Some(doc) = query_stream.next(self.tx, None).await?
        {
            self.cancel(doc.id()).await?;
            count += 1;
        }
        Ok(count)
    }

    /// Checks the status of the scheduled job. If it has been garbage collected
    /// and the scheduled job is no longer in the table, it returns None.
    pub async fn check_status(
        &mut self,
        job_id: ResolvedDocumentId,
    ) -> anyhow::Result<Option<ScheduledJobState>> {
        anyhow::ensure!(self
            .tx
            .table_mapping()
            .namespace(self.namespace)
            .tablet_matches_name(job_id.tablet_id, &SCHEDULED_JOBS_TABLE));
        let state = self
            .tx
            .get(job_id)
            .await?
            .map(ParseDocument::<ScheduledJobMetadata>::parse)
            .transpose()?
            .map(|job| job.state.clone());
        Ok(state)
    }
}

fn invalid_scheduled_function_id() -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "InvalidArgument",
        "Invalid scheduled function ID. The ID must be an ID on the '_scheduled_functions' table.",
    )
}

/// Same as SchedulerModel but works with the respective virtual table instead
/// of the underlying system table.
pub struct VirtualSchedulerModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
    namespace: TableNamespace,
}

impl<'a, RT: Runtime> VirtualSchedulerModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>, namespace: TableNamespace) -> Self {
        Self { tx, namespace }
    }

    pub async fn schedule(
        &mut self,
        path: CanonicalizedComponentFunctionPath,
        args: ConvexArray,
        ts: UnixTimestamp,
        context: ExecutionContext,
    ) -> anyhow::Result<DeveloperDocumentId> {
        let system_id = SchedulerModel::new(self.tx, self.namespace)
            .schedule(path, args, ts, context)
            .await?;
        self.tx
            .virtual_system_mapping()
            .system_resolved_id_to_virtual_developer_id(system_id)
    }

    pub async fn cancel(&mut self, virtual_id: DeveloperDocumentId) -> anyhow::Result<()> {
        let table_name = self
            .tx
            .resolve_idv6(
                virtual_id,
                self.namespace,
                TableFilter::ExcludePrivateSystemTables,
            )
            .context(invalid_scheduled_function_id())?;
        anyhow::ensure!(
            table_name == SCHEDULED_JOBS_VIRTUAL_TABLE,
            invalid_scheduled_function_id()
        );
        let table_mapping = self.tx.table_mapping().clone();
        let system_id = self
            .tx
            .virtual_system_mapping()
            .virtual_id_v6_to_system_resolved_doc_id(self.namespace, &virtual_id, &table_mapping)?;
        SchedulerModel::new(self.tx, self.namespace)
            .cancel(system_id)
            .await
    }
}
