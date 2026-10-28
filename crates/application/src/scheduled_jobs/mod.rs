use std::{
    cmp,
    collections::{
        BTreeMap,
        HashSet,
    },
    ops::Bound,
    sync::Arc,
    time::{
        Duration,
        SystemTime,
    },
};

use anyhow::Context;
use common::{
    backoff::Backoff,
    components::{
        ComponentId,
        PublicFunctionPath,
    },
    document::{
        ParseDocument,
        ParsedDocument,
        MAX_USER_SIZE,
    },
    errors::{
        report_error,
        JsError,
    },
    execution_context::{
        ExecutionContext,
        RequestContext,
        RequestMetadata,
    },
    fastrace_helpers::get_sampled_span,
    knobs::{
        MAX_SCHEDULED_JOB_ARGUMENT_SIZE_BYTES,
        MAX_TRANSACTION_WINDOW,
        SCHEDULED_JOB_EXECUTION_PARALLELISM,
        SCHEDULED_JOB_GARBAGE_COLLECTION_BATCH_SIZE,
        SCHEDULED_JOB_GARBAGE_COLLECTION_DELAY,
        SCHEDULED_JOB_GARBAGE_COLLECTION_INITIAL_BACKOFF,
        SCHEDULED_JOB_GARBAGE_COLLECTION_MAX_BACKOFF,
        SCHEDULED_JOB_INITIAL_BACKOFF,
        SCHEDULED_JOB_MAX_BACKOFF,
        SCHEDULED_JOB_RETENTION,
        SCHEDULER_OCC_INITIAL_BACKOFF,
        SCHEDULER_OCC_MAX_BACKOFF,
        TRANSACTION_MAX_READ_SIZE_BYTES,
        UDF_EXECUTOR_OCC_MAX_RETRIES,
    },
    pause::Fault,
    query::{
        IndexRange,
        IndexRangeExpression,
        Order,
        Query,
    },
    runtime::{
        Runtime,
        SpawnHandle,
    },
    types::{
        FunctionCaller,
        QueryInvocation,
        UdfType,
    },
    RequestId,
};
use database::{
    Database,
    ResolvedQuery,
    TimestampedIndexCache,
    Transaction,
    WriteSource,
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use fastrace::future::FutureExt as _;
use futures::{
    future::Either,
    select_biased,
    Future,
    FutureExt,
    TryStreamExt,
};
use futures_async_stream::try_stream;
use keybroker::Identity;
use model::{
    backend_state::BackendStateModel,
    modules::ModuleModel,
    scheduled_jobs::{
        types::{
            MatchesScheduledJobMetadata,
            ScheduledJob,
            ScheduledJobMetadata,
            ScheduledJobState,
        },
        SchedulerModel,
        NEXT_TS_FIELD,
        SCHEDULED_JOBS_INDEX,
        SCHEDULED_JOBS_INDEX_BY_COMPLETED_TS,
        SCHEDULED_JOBS_TABLE,
    },
};
use parking_lot::Mutex;
use sentry::SentryFutureExt;
use sync_types::Timestamp;
use tokio::sync::mpsc;
use udf::validation::ValidatedUdfOutcome;
use usage_tracking::FunctionUsageTracker;
use value::{
    ConvexValue,
    ResolvedDocumentId,
};

use crate::{
    application_function_runner::{
        ApplicationFunctionRunner,
        PermitTimeout,
    },
    function_log::FunctionExecutionLog,
};

mod metrics;

pub(crate) const SCHEDULED_JOB_EXECUTED: &str = "scheduled_job_executed";
pub(crate) const SCHEDULED_JOB_EXECUTING: &str = "scheduled_job_executing";
pub(crate) const SCHEDULED_JOB_COMMITTING: &str = "scheduled_job_committing";
pub(crate) const SCHEDULED_JOB_WRITE_THROUGHPUT_ERROR: &str =
    "scheduled_job_write_throughput_error";
pub(crate) const SCHEDULED_JOB_SUCCEEDED: &str = "scheduled_job_succeeded";
pub(crate) const SCHEDULED_JOB_QUERIED: &str = "scheduled_job_queried";
pub(crate) const SCHEDULER_STARTED: &str = "scheduler_started";

#[derive(Clone)]
pub struct ScheduledJobRunner {
    executor: Arc<Mutex<Box<dyn SpawnHandle>>>,
    garbage_collector: Arc<Mutex<Box<dyn SpawnHandle>>>,
}

impl ScheduledJobRunner {
    pub fn start<RT: Runtime>(
        rt: RT,
        deployment_name: String,
        database: Database<RT>,
        runner: Arc<ApplicationFunctionRunner<RT>>,
        function_log: FunctionExecutionLog<RT>,
    ) -> Self {
        let executor_fut = ScheduledJobExecutor::run(
            rt.clone(),
            deployment_name,
            database.clone(),
            runner,
            function_log,
        );
        let executor = Arc::new(Mutex::new(rt.spawn("scheduled_job_executor", executor_fut)));

        let garbage_collector_fut = ScheduledJobGarbageCollector::start(rt.clone(), database);
        let garbage_collector = Arc::new(Mutex::new(
            rt.spawn("scheduled_job_garbage_collector", garbage_collector_fut),
        ));
        Self {
            executor,
            garbage_collector,
        }
    }

    pub fn shutdown(&self) {
        self.executor.lock().shutdown();
        self.garbage_collector.lock().shutdown();
    }
}

pub struct ScheduledJobExecutor<RT: Runtime> {
    context: ScheduledJobContext<RT>,
    deployment_name: String,
    running_job_ids: HashSet<ResolvedDocumentId>,
    /// Some if there's at least one pending job. May be in the past!
    next_job_ready_time: Option<Timestamp>,
    job_finished_tx: mpsc::Sender<ResolvedDocumentId>,
    job_finished_rx: mpsc::Receiver<ResolvedDocumentId>,
    /// The last time we logged stats, used to rate limit logging
    last_stats_log: SystemTime,
    /// The last logged value of `next_job_ready_time`
    last_logged_ready_time: Option<SystemTime>,
    /// Index cache from the previous loop that can be fast-forwarded and reused
    /// in the next transaction to query scheduled jobs to run.
    index_cache: Option<TimestampedIndexCache>,
}

#[derive(Clone)]
pub struct ScheduledJobContext<RT: Runtime> {
    rt: RT,
    database: Database<RT>,
    runner: Arc<ApplicationFunctionRunner<RT>>,
    function_log: FunctionExecutionLog<RT>,
}

impl<RT: Runtime> ScheduledJobContext<RT> {
}

impl<RT: Runtime> ScheduledJobExecutor<RT> {
    pub async fn run(
        rt: RT,
        deployment_name: String,
        database: Database<RT>,
        runner: Arc<ApplicationFunctionRunner<RT>>,
        function_log: FunctionExecutionLog<RT>,
    ) {
        let (job_finished_tx, job_finished_rx) =
            mpsc::channel(*SCHEDULED_JOB_EXECUTION_PARALLELISM);
        let mut executor = Self {
            context: ScheduledJobContext {
                rt: rt.clone(),
                database,
                runner,
                function_log,
            },
            deployment_name,
            running_job_ids: HashSet::new(),
            next_job_ready_time: None,
            job_finished_tx,
            job_finished_rx,
            last_stats_log: rt.system_time(),
            // This value will force the first call to `run_once` to log
            last_logged_ready_time: Some(SystemTime::UNIX_EPOCH),
            index_cache: None,
        };
        let mut backoff = Backoff::new(*SCHEDULED_JOB_INITIAL_BACKOFF, *SCHEDULED_JOB_MAX_BACKOFF);
        tracing::info!("Starting scheduled job executor");
        loop {
            match executor.run_once().await {
                Ok(()) => backoff.reset(),
                Err(mut e) => {
                    metrics::log_scheduled_job_executor_error();
                    let delay = backoff.fail(&mut executor.context.rt.rng());
                    tracing::error!("Scheduled job executor failed, sleeping {delay:?}");
                    report_error(&mut e).await;
                    executor.context.rt.wait(delay).await;
                },
            }
        }
    }

    async fn run_once(&mut self) -> anyhow::Result<()> {
        let pause_client = self.context.rt.pause_client();
        pause_client.wait(SCHEDULER_STARTED).await;
        let _timer = metrics::run_scheduled_jobs_loop();

        let maybe_index_cache = if let Some(index_cache) = self.index_cache.take() {
            let (end_ts, snapshot) = self.context.database.latest_ts_and_snapshot()?;
            self.context
                .database
                .log()
                .fast_forward_index_cache(index_cache, &snapshot.index_registry, end_ts)
                .await?
        } else {
            None
        };
        let mut tx = if let Some(fast_forwarded_index_cache) = maybe_index_cache {
            self.context
                .database
                .begin_with_index_cache(Identity::Unknown(None), fast_forwarded_index_cache)
                .await?
        } else {
            self.context.database.begin(Identity::Unknown(None)).await?
        };
        let backend_state = BackendStateModel::new(&mut tx).get_backend_state().await?;
        let is_backend_stopped = backend_state.is_stopped();

        self.next_job_ready_time = if is_backend_stopped {
            // If the backend is stopped we shouldn't poll. Our subscription will notify us
            // when the backend is started again.
            None
        } else if self.running_job_ids.len() == *SCHEDULED_JOB_EXECUTION_PARALLELISM {
            // A scheduled job may have been added, but we can't do anything because we're
            // still running jobs at our concurrency limit.
            self.next_job_ready_time
        } else {
            // Great! we have enough remaining concurrency and our backend is running, start
            // new job(s) if we can and update our next ready time.
            let root = get_sampled_span(
                &self.deployment_name,
                "scheduler/query_and_start_jobs",
                &mut self.context.rt.rng(),
            );
            self.query_and_start_jobs(&mut tx).in_span(root).await?
        };

        let now = self.context.rt.system_time();
        let next_job_ready_time = self.next_job_ready_time.map(SystemTime::from);
        metrics::log_running_jobs(self.running_job_ids.len());

        // Only log stats if:
        // - next_job_ready_time differs by >=30 seconds from the last logged value; or
        // - we're lagging and >=30 seconds have elapsed
        let should_log = match (self.last_logged_ready_time, next_job_ready_time) {
            (None, None) => false,
            (Some(t1), Some(t2)) => {
                let abs_diff = t1.duration_since(t2).unwrap_or_else(|e| e.duration());
                abs_diff >= Duration::from_secs(30)
            },
            // always log if transitioning between Some and None
            _ => true,
        } || (next_job_ready_time.is_some_and(|t| t <= now)
            && now
                .duration_since(self.last_stats_log)
                .is_ok_and(|d| d >= Duration::from_secs(30)));
        if should_log {
            self.log_scheduled_job_stats(next_job_ready_time, now);
            self.last_logged_ready_time = next_job_ready_time;
            self.last_stats_log = now;
        }
        let next_job_future = if let Some(next_job_ts) = next_job_ready_time {
            let wait_time = next_job_ts.duration_since(now).unwrap_or_else(|_| {
                // If we're behind, re-run this loop every 5 seconds to log the gauge above and
                // track how far we're behind in our metrics.
                Duration::from_secs(5)
            });
            Either::Left(self.context.rt.wait(wait_time))
        } else {
            Either::Right(std::future::pending())
        };

        let (token, index_cache) = tx.into_token_and_index_cache()?;
        self.index_cache = Some(index_cache);
        let subscription_fut = self
            .context
            .database
            .subscribe_and_wait_for_invalidation(token);

        let mut job_ids: Vec<_> = Vec::new();
        let drop_index_cache = async {
            self.context.rt.wait(*MAX_TRANSACTION_WINDOW).await;
            self.index_cache.take();
            std::future::pending::<()>().await
        };
        select_biased! {
            num_jobs = self.job_finished_rx
                .recv_many(&mut job_ids, *SCHEDULED_JOB_EXECUTION_PARALLELISM)
                .fuse() => {
                // `recv_many()` returns the number of jobs received. If this number is 0,
                // then the channel has been closed.
                if num_jobs > 0 {
                    for job_id in job_ids {
                        pause_client.wait(SCHEDULED_JOB_EXECUTED).await;
                        self.running_job_ids.remove(&job_id);
                    }
                } else {
                    anyhow::bail!("Job results channel closed, this is unexpected!");
                }
            },
            _ = next_job_future.fuse() => {
            },
            _ = subscription_fut.fuse() => {
            },
            _ = drop_index_cache.fuse() => {
            },
        }
        Ok(())
    }

    fn log_scheduled_job_stats(&self, next_job_ready_time: Option<SystemTime>, now: SystemTime) {
        metrics::log_num_running_jobs(self.running_job_ids.len());
        let backlog = next_job_ready_time.map_or(Duration::ZERO, |next_job_ts| {
            now.duration_since(next_job_ts).unwrap_or(Duration::ZERO)
        });
        metrics::log_scheduled_job_execution_lag(backlog);
        metrics::log_scheduled_job_backlog(backlog);
        self.context.function_log.log_scheduled_job_stats(
            next_job_ready_time,
            now,
            self.running_job_ids.len() as u64,
        );
    }

    /// Reads through scheduled jobs in timestamp ascending order and starts any
    /// that are allowed by our concurrency limit and the jobs' scheduled
    /// time.
    ///
    /// Returns the time at which the next job in the queue will be ready to
    /// run. If the scheduler is behind, the returned time may be in the
    /// past. Returns None if all jobs are finished or running.
    async fn query_and_start_jobs(
        &mut self,
        tx: &mut Transaction<RT>,
    ) -> anyhow::Result<Option<Timestamp>> {
        let now = self.context.rt.generate_timestamp()?;
        let mut job_stream = self.context.stream_jobs_to_run(tx);
        while let Some((job_id, job)) = job_stream.try_next().await? {
            self.context
                .rt
                .pause_client()
                .wait(SCHEDULED_JOB_QUERIED)
                .await;
            if self.running_job_ids.contains(&job_id) {
                continue;
            }
            let next_ts = job
                .next_ts
                .context("Could not get next_ts to run scheduled job at")?;
            // If we can't execute the job return the job's target timestamp. If we're
            // caught up, we can sleep until the timestamp. If we're behind and
            // at our concurrency limit, we can use the timestamp to log how far
            // behind we get.
            if next_ts > now || self.running_job_ids.len() == *SCHEDULED_JOB_EXECUTION_PARALLELISM {
                return Ok(Some(next_ts));
            }

            let context = self.context.clone();
            let tx = self.job_finished_tx.clone();

            let root = get_sampled_span(
                &self.deployment_name,
                "scheduler/execute_job",
                &mut self.context.rt.rng(),
            );
            let sentry_hub = sentry::Hub::with(|hub| sentry::Hub::new_from_top(hub));
            self.context.rt.spawn_background(
                "spawn_scheduled_job",
                async move {
                    select_biased! {
                        _ = tx.closed().fuse() => {
                            tracing::error!("Scheduled job receiver closed");
                        },
                        _ = context.execute_job(job, job_id).fuse() => {
                            let _ = tx.send(job_id).await;
                        },
                    }
                }
                .in_span(root)
                .bind_hub(sentry_hub),
            );

            self.running_job_ids.insert(job_id);

            // We might have hit the concurrency limit by adding the new job, so
            // we could check and break immediately if we have.
            // However we want to know the time the next job in the
            // queue (if any) is due, so instead we continue the loop one more
            // time.
        }
        Ok(None)
    }
}

impl<RT: Runtime> ScheduledJobContext<RT> {
    #[try_stream(boxed, ok = (ResolvedDocumentId, ScheduledJobMetadata), error = anyhow::Error)]
    async fn stream_jobs_to_run<'a>(&'a self, tx: &'a mut Transaction<RT>) {
        let namespaces: Vec<_> = tx
            .table_mapping()
            .iter()
            .filter(|(_, _, _, name)| **name == SCHEDULED_JOBS_TABLE)
            .map(|(_, namespace, ..)| namespace)
            .collect();
        let index_query = Query::index_range(IndexRange {
            index_name: SCHEDULED_JOBS_INDEX.name(),
            range: vec![IndexRangeExpression::Gt(
                NEXT_TS_FIELD.clone(),
                value::ConvexValue::Null.into(),
            )],
            order: Order::Asc,
        });
        // Key is (next_ts, namespace), where next_ts is for sorting and namespace
        // is for deduping.
        // Value is (job, query) where job is the job to run and query will get
        // the next job to run in that namespace.
        let mut queries = BTreeMap::new();
        for namespace in namespaces {
            let mut query = ResolvedQuery::new(tx, namespace, index_query.clone())?;
            let doc = {
                let timer = metrics::query_scheduled_jobs_timer();
                let doc = query.next(tx, None).await?;
                timer.finish();
                doc
            };
            if let Some(doc) = doc {
                let job_metadata: ParsedDocument<ScheduledJobMetadata> = doc.parse()?;
                let job_metadata_id = job_metadata.id();
                let next_ts = job_metadata.next_ts.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Could not get next_ts to run scheduled job {}",
                        job_metadata.id()
                    )
                })?;
                queries.insert(
                    (next_ts, namespace),
                    ((job_metadata_id, job_metadata.into_value()), query),
                );
            }
        }
        while let Some(((_min_next_ts, namespace), (min_job, mut query))) = queries.pop_first() {
            yield min_job;
            let doc = {
                let timer = metrics::query_scheduled_jobs_timer();
                let doc = query.next(tx, None).await?;
                timer.finish();
                doc
            };
            if let Some(doc) = doc {
                let job_metadata: ParsedDocument<ScheduledJobMetadata> = doc.parse()?;
                let job_metadata_id = job_metadata.id();
                let next_ts = job_metadata.next_ts.with_context(|| {
                    format!("Could not get next_ts to run scheduled job {job_metadata_id}",)
                })?;
                queries.insert(
                    (next_ts, namespace),
                    ((job_metadata_id, job_metadata.into_value()), query),
                );
            }
        }
    }

    // This handles re-running the scheduled function on transient errors. It
    // guarantees that the job was successfully run or the job state changed.
    pub async fn execute_job(&self, job: ScheduledJobMetadata, job_id: ResolvedDocumentId) {
        match self
            .run_function(job.clone(), job_id, job.attempts.count_failures() as usize)
            .await
        {
            Ok(()) => {
                self.rt.pause_client().wait(SCHEDULED_JOB_SUCCEEDED).await;
                metrics::log_scheduled_job_success(job.attempts.count_failures());
            },
            Err(e) => {
                metrics::log_scheduled_job_failure(&e, job.attempts.count_failures());
                match self.schedule_retry(job, job_id, e).await {
                    Ok(()) => {},
                    Err(mut retry_err) => {
                        // If scheduling a retry hits an error, nothing has
                        // changed so the job will remain at the head of the queue and
                        // will be picked up by the scheduler in the next cycle.
                        report_error(&mut retry_err).await;
                    },
                }
            },
        }
    }

    async fn schedule_retry(
        &self,
        job: ScheduledJobMetadata,
        job_id: ResolvedDocumentId,
        mut system_error: anyhow::Error,
    ) -> anyhow::Result<()> {
        let Some((mut tx, mut job)) = self
            .new_transaction_for_job_state(job_id, &job, FunctionUsageTracker::new())
            .await?
        else {
            // Continue without scheduling retry since the job state has changed
            // This can happen for actions that encounter a system error during
            // their execution.
            // TODO: we should not even get to this function in that case.
            report_error(&mut system_error).await;
            return Ok(());
        };
        let namespace = tx.table_mapping().tablet_namespace(job_id.tablet_id)?;
        let mut backoff = Backoff::new(*SCHEDULED_JOB_INITIAL_BACKOFF, *SCHEDULED_JOB_MAX_BACKOFF);
        let attempts = &mut job.attempts;
        backoff.set_failures(attempts.count_failures());
        report_error(&mut system_error).await;
        attempts.system_errors += 1;
        let delay = backoff.fail(&mut self.rt.rng());
        tracing::error!("System error executing job {job_id}, sleeping {delay:?}");
        job.next_ts = Some(self.rt.generate_timestamp()?.add(delay)?);
        SchedulerModel::new(&mut tx, namespace)
            .replace(job_id, job)
            .await?;
        self.database
            .commit_with_write_source(tx, "scheduled_job_system_error")
            .await?;
        Ok(())
    }

    async fn run_function(
        &self,
        job: ScheduledJobMetadata,
        job_id: ResolvedDocumentId,
        mutation_retry_count: usize,
    ) -> anyhow::Result<()> {
        let usage_tracker = FunctionUsageTracker::new();
        let Some((mut tx, job)) = self
            .new_transaction_for_job_state(job_id, &job, usage_tracker.clone())
            .await?
        else {
            // Continue without running function since the job state has changed
            return Ok(());
        };

        tracing::debug!(
            "Executing '{}'{}!",
            job.path.udf_path,
            job.path.component.in_component_str()
        );
        let identity = tx.inert_identity();
        let namespace = tx.table_mapping().tablet_namespace(job_id.tablet_id)?;
        let component_id = ComponentId::from(namespace);

        // Since we don't specify the function type when we schedule, we have to
        // use the analyzed result.
        let caller = FunctionCaller::Scheduler {
            job_id: job_id.into(),
            component_id,
        };
        let job = SchedulerModel::new(&mut tx, namespace)
            .scheduled_job_from_metadata(job_id, job)
            .await?;
        let path = job.path.clone();
        let udf_type = match ModuleModel::new(&mut tx)
            .get_analyzed_function(&path)
            .await?
        {
            Ok(analyzed_function) => analyzed_function.udf_type,
            Err(error) => {
                SchedulerModel::new(&mut tx, namespace)
                    .complete(
                        job_id,
                        ScheduledJobState::Failed(error.user_facing_message()),
                    )
                    .await?;
                self.database
                    .commit_with_write_source(tx, "scheduled_job_analyze_failure")
                    .await?;
                // NOTE: We didn't actually run anything, so we are creating a request context
                // just report the error.
                let request_id = RequestId::new();
                let context = ExecutionContext::new(
                    RequestContext::new_for_system_request(request_id),
                    &caller,
                );
                // We don't know what the UdfType is since this is an invalid module.
                // Log as mutation for now.
                self.function_log
                    .log_mutation_system_error(
                        &error,
                        path,
                        job.udf_args()?,
                        identity,
                        self.rt.monotonic_now(),
                        caller,
                        context,
                        None,
                        mutation_retry_count,
                    )
                    .await?;
                return Ok(());
            },
        };

        // Note that we do validate that the scheduled function execute during
        // scheduling, but the modules can have been modified since scheduling.
        match udf_type {
            UdfType::Mutation => self.handle_mutation(caller, job, job_id).await?,
            UdfType::Action => {
                self.handle_action(caller, tx, job, job_id, usage_tracker)
                    .await?
            },
            UdfType::Query | UdfType::HttpAction => {
                let message = format!(
                    r#"Unsupported function type. {:?} in module "{:?}"{} is defined as a {udf_type}. "
                            "Only {} and {} can be scheduled."#,
                    path.udf_path.function_name(),
                    path.udf_path.module(),
                    path.component.in_component_str(),
                    UdfType::Mutation,
                    UdfType::Action,
                );
                SchedulerModel::new(&mut tx, namespace)
                    .complete(job_id, ScheduledJobState::Failed(message.clone()))
                    .await?;
                self.database
                    .commit_with_write_source(tx, "scheduled_job_bad_udf")
                    .await?;
                // NOTE: We didn't actually run anything, so we are creating a request context
                // just report the error.
                let request_id = RequestId::new();
                let context = ExecutionContext::new(
                    RequestContext::new_for_system_request(request_id),
                    &caller,
                );
                match udf_type {
                    UdfType::Query => {
                        self.function_log
                            .log_query_system_error(
                                &ErrorMetadata::bad_request(
                                    "UnsupportedScheduledFunctionType",
                                    message,
                                )
                                .into(),
                                path,
                                job.udf_args()?,
                                identity,
                                self.rt.monotonic_now(),
                                caller,
                                context,
                                QueryInvocation::Fresh,
                            )
                            .await?;
                    },
                    UdfType::HttpAction => {
                        // It would be more correct to log this as an HTTP action, but
                        // we don't have things like a URL or method to log with, so log
                        // it as an action with an error message.
                        self.function_log
                            .log_action_system_error(
                                &ErrorMetadata::bad_request(
                                    "UnsupportedScheduledFunctionType",
                                    message,
                                )
                                .into(),
                                path,
                                job.udf_args()?,
                                identity,
                                self.rt.monotonic_now(),
                                caller,
                                vec![].into(),
                                context,
                            )
                            .await?;
                    },
                    // Should be unreachable given the outer match statement
                    UdfType::Mutation => unreachable!(),
                    UdfType::Action => unreachable!(),
                }
            },
        };

        Ok(())
    }

    async fn handle_mutation(
        &self,
        caller: FunctionCaller,
        job: ScheduledJob,
        job_id: ResolvedDocumentId,
    ) -> anyhow::Result<()> {
        let mut backoff = Backoff::new(*SCHEDULER_OCC_INITIAL_BACKOFF, *SCHEDULER_OCC_MAX_BACKOFF);
        let udf_args = job.udf_args()?;
        let request_id = RequestId::new();
        loop {
            let mutation_retry_count = backoff.failures() as usize;
            let usage_tracker = FunctionUsageTracker::new();
            let Some((mut tx, job)) = self
                .new_transaction_for_job_state(job_id, &job, usage_tracker.clone())
                .await?
            else {
                // Continue without running function since the job state has changed
                return Ok(());
            };
            let permit = self.runner.acquire_mutation_permit().await;
            let start = self.rt.monotonic_now();
            let context = ExecutionContext::new(
                RequestContext::new_for_system_request(request_id.clone()),
                &caller,
            );
            sentry::configure_scope(|scope| context.add_sentry_tags(scope));
            let identity = tx.inert_identity();
            let namespace = tx.table_mapping().tablet_namespace(job_id.tablet_id)?;
            let path = job.path.clone();
            let pause_client = self.rt.pause_client();

            // Stage a write so that the mutation can observe its own
            // `_scheduled_functions` document as `inProgress`. This is invisible to
            // other transactions, for whom the job stays `pending` until it
            // reaches a completion state.
            let mut in_progress_job = job.clone();
            in_progress_job.state = ScheduledJobState::InProgress {
                request_id: context.request_id.clone(),
                execution_id: context.execution_id,
            };
            SchedulerModel::new(&mut tx, namespace)
                .replace(job_id, in_progress_job)
                .await?;

            let result = if let Fault::Error(e) = pause_client.wait(SCHEDULED_JOB_EXECUTING).await {
                tracing::info!("Injected error before running mutation");
                Err(e)
            } else {
                self.runner
                    .run_mutation_no_udf_log(
                        tx,
                        permit,
                        PublicFunctionPath::Component(path.clone()),
                        udf_args.clone(),
                        caller.allowed_visibility(),
                        context.clone(),
                        None,
                    )
                    .await
            };
            let (stats, execution_time, outcome) = match result {
                Ok((mut tx, mut outcome)) => {
                    let stats = tx.take_stats();
                    let execution_time = start.elapsed();
                    if outcome.result.is_ok() {
                        SchedulerModel::new(&mut tx, namespace)
                            .complete(job_id, ScheduledJobState::Success)
                            .await?;
                        let commit_result = if let Fault::Error(e) =
                            pause_client.wait(SCHEDULED_JOB_COMMITTING).await
                        {
                            tracing::info!("Injected error before committing mutation");
                            Err(e)
                        } else {
                            self.database
                                .commit_with_write_source(tx, WriteSource::mutation(path.clone()))
                                .await
                        };
                        if let Err(err) = commit_result {
                            if err.is_deterministic_user_error() {
                                outcome.result = Err(JsError::from_error(err));
                            } else if let Some(occ_info) = err.occ_info() {
                                metrics::log_scheduled_job_failure(
                                    &err,
                                    mutation_retry_count as u32,
                                );
                                self.function_log
                                    .log_mutation_occ_error(
                                        outcome,
                                        stats,
                                        execution_time,
                                        caller.clone(),
                                        usage_tracker,
                                        context,
                                        occ_info,
                                        None,
                                        mutation_retry_count,
                                        mutation_retry_count,
                                        true,
                                    )
                                    .await;
                                let delay = backoff.fail(&mut self.rt.rng());
                                self.rt.wait(delay).await;
                                continue;
                            } else {
                                // Return an error instead of retrying indefinitely on system
                                // errors. The scheduled job will be
                                // rescheduled.
                                return Err(err);
                            }
                        }
                    }
                    (stats, execution_time, outcome)
                },
                Err(e) => {
                    if e.short_msg() == "TooManyWrites" {
                        self.function_log
                            .log_mutation_write_throughput_error(
                                &e,
                                path,
                                udf_args.clone(),
                                identity,
                                start,
                                caller.clone(),
                                context,
                                None,
                                mutation_retry_count,
                                true,
                            )
                            .await?;
                        pause_client
                            .wait(SCHEDULED_JOB_WRITE_THROUGHPUT_ERROR)
                            .await;
                        let delay = backoff.fail(&mut self.rt.rng());
                        self.rt.wait(delay).await;
                        continue;
                    } else if e.is_deterministic_user_error() {
                        // A deterministic user error fails the same way on every attempt.
                        // Synthesize a failed outcome so the failure path
                        // below completes the job as failed instead of
                        // rescheduling it like a transient system error.
                        let outcome = ValidatedUdfOutcome::from_error(
                            JsError::from_error(e),
                            path,
                            udf_args.clone(),
                            identity,
                            self.rt.clone(),
                            None,
                        )?;
                        (BTreeMap::new(), start.elapsed(), outcome)
                    } else {
                        self.function_log
                            .log_mutation_system_error(
                                &e,
                                path,
                                udf_args.clone(),
                                identity,
                                start,
                                caller.clone(),
                                context,
                                None,
                                mutation_retry_count,
                            )
                            .await?;
                        // Only retry in this loop on write throughput errors and OCC errors on
                        // commit (above), other system errors should cause
                        // the mutation to be rescheduled.
                        return Err(e);
                    }
                },
            };
            if outcome.result.is_err() {
                // UDF failed due to developer error. It is not safe to commit the
                // transaction it executed in. We should remove the job in a new
                // transaction.
                let Some((mut tx, _metadata)) = self
                    .new_transaction_for_job_state(job_id, &job, usage_tracker.clone())
                    .await?
                else {
                    // Continue without updating since the job state has changed
                    return Ok(());
                };
                SchedulerModel::new(&mut tx, namespace)
                    .complete(
                        job_id,
                        ScheduledJobState::Failed(outcome.result.clone().unwrap_err().to_string()),
                    )
                    .await?;
                // NOTE: We should not be getting developer errors here.
                self.database
                    .commit_with_write_source(tx, WriteSource::mutation(job.path.clone()))
                    .await?;
            }
            self.function_log
                .log_mutation(
                    outcome,
                    stats,
                    execution_time,
                    caller,
                    usage_tracker,
                    context,
                    None,
                    mutation_retry_count,
                )
                .await;
            return Ok(());
        }
    }

    async fn handle_action(
        &self,
        caller: FunctionCaller,
        tx: Transaction<RT>,
        job: ScheduledJob,
        job_id: ResolvedDocumentId,
        usage_tracker: FunctionUsageTracker,
    ) -> anyhow::Result<()> {
        let identity = tx.identity().clone();
        let Some((mut tx, metadata)) = self
            .new_transaction_for_job_state(job_id, &job, usage_tracker.clone())
            .await?
        else {
            // Continue without updating since the job state has changed
            return Ok(());
        };
        let namespace = tx.table_mapping().tablet_namespace(job_id.tablet_id)?;
        match job.state {
            ScheduledJobState::Pending => {
                // Create a new request & execution ID
                let request_id = RequestId::new();
                let context = ExecutionContext::new(
                    RequestContext::new_for_system_request(request_id),
                    &caller,
                );
                sentry::configure_scope(|scope| context.add_sentry_tags(scope));

                // Set state to in progress
                let mut updated_job = metadata.clone();
                updated_job.state = ScheduledJobState::InProgress {
                    request_id: context.request_id.clone(),
                    execution_id: context.execution_id,
                };
                SchedulerModel::new(&mut tx, namespace)
                    .replace(job_id, updated_job.clone())
                    .await?;
                self.database
                    .commit_with_write_source(tx, "scheduled_job_progress")
                    .await?;

                // Execute the action
                let path = job.path.clone();
                let completion = self
                    .runner
                    .run_action_no_udf_log(
                        PublicFunctionPath::Component(path),
                        job.udf_args()?,
                        identity,
                        caller,
                        usage_tracker.clone(),
                        context.clone(),
                        PermitTimeout::WaitIndefinitely,
                    )
                    .await?;
                let state = match &completion.outcome.result {
                    Ok(_) => ScheduledJobState::Success,
                    Err(e) => ScheduledJobState::Failed(e.to_string()),
                };

                // Mark the job as completed. Keep trying until we succeed (or
                // detect the job state has changed). Don't bubble up the error
                // since otherwise we will lose the original execution logs.
                let mut backoff =
                    Backoff::new(*SCHEDULED_JOB_INITIAL_BACKOFF, *SCHEDULED_JOB_MAX_BACKOFF);
                while let Err(mut err) = self
                    .complete_action(job_id, &updated_job, usage_tracker.clone(), state.clone())
                    .await
                {
                    let delay = backoff.fail(&mut self.rt.rng());
                    tracing::error!("Failed to update action state, sleeping {delay:?}");
                    report_error(&mut err).await;
                    self.rt.wait(delay).await;
                }
                self.function_log
                    .log_action(completion, usage_tracker)
                    .await;
            },
            ScheduledJobState::InProgress {
                ref request_id,
                ref execution_id,
            } => {
                // This case can happen if there is a system error while executing
                // the action or if backend exits after executing the action but
                // before updating the state. Since we execute actions at most once,
                // complete this job and log the error.
                let message = "Transient error while executing action".to_string();
                SchedulerModel::new(&mut tx, namespace)
                    .complete(job_id, ScheduledJobState::Failed(message.clone()))
                    .await?;
                self.database
                    .commit_with_write_source(tx, "scheduled_job_action_error")
                    .await?;
                // Restore the request & execution ID of the failed execution.
                let context = ExecutionContext::new_from_parts(
                    request_id.clone(),
                    *execution_id,
                    caller.parent_scheduled_job(),
                    caller.is_root(),
                    RequestMetadata::system(),
                );
                sentry::configure_scope(|scope| context.add_sentry_tags(scope));
                let path = job.path.clone();
                let mut err = JsError::from_message(message).into();
                self.function_log
                    .log_action_system_error(
                        &err,
                        path,
                        job.udf_args()?,
                        identity.into(),
                        self.rt.monotonic_now(),
                        caller,
                        vec![].into(),
                        context,
                    )
                    .await?;
                report_error(&mut err).await;
            },
            state => {
                anyhow::bail!(
                    "Invalid state for executing action. Expected Pending or InProgress, got {:?}",
                    state
                );
            },
        }
        Ok(())
    }

    // Creates a new transaction and verifies the job state matches the given one.
    async fn new_transaction_for_job_state<T: MatchesScheduledJobMetadata>(
        &self,
        job_id: ResolvedDocumentId,
        expected_state: &T,
        usage_tracker: FunctionUsageTracker,
    ) -> anyhow::Result<Option<(Transaction<RT>, ScheduledJobMetadata)>> {
        let mut tx = self
            .database
            .begin_with_usage(Identity::Unknown(None), usage_tracker)
            .await?;
        let Some(doc) = tx.get(job_id).await? else {
            if tx.table_mapping().tablet_name(job_id.tablet_id).is_err() {
                // The scheduled jobs table could have been deleted since we queried this
                // scheduled job.
                return Ok(None);
            }
            // If the scheduled jobs table still exists, we're missing a scheduled jobs
            // document and we don't know why.
            anyhow::bail!("Missing scheduled jobs metadata document with id {job_id}");
        };
        let metadata = ParseDocument::<ScheduledJobMetadata>::parse(doc)?.into_value();
        // Verify that the scheduled job has not changed.
        Ok(expected_state
            .matches_metadata(&metadata)
            .then_some((tx, metadata)))
    }

    // Completes an action in separate transaction. Returns false if the action
    // state has changed.
    async fn complete_action(
        &self,
        job_id: ResolvedDocumentId,
        expected_state: &ScheduledJobMetadata,
        usage_tracking: FunctionUsageTracker,
        job_state: ScheduledJobState,
    ) -> anyhow::Result<()> {
        let Some((mut tx, _metadata)) = self
            .new_transaction_for_job_state(job_id, expected_state, usage_tracking)
            .await?
        else {
            // Continue without updating since the job state has changed
            return Ok(());
        };
        let namespace = tx.table_mapping().tablet_namespace(job_id.tablet_id)?;

        // Remove from the scheduled jobs table
        SchedulerModel::new(&mut tx, namespace)
            .complete(job_id, job_state)
            .await?;
        self.database
            .commit_with_write_source(tx, "scheduled_job_complete_action")
            .await?;
        Ok(())
    }
}

pub struct ScheduledJobGarbageCollector<RT: Runtime> {
    rt: RT,
    database: Database<RT>,
}

impl<RT: Runtime> ScheduledJobGarbageCollector<RT> {
    pub fn start(rt: RT, database: Database<RT>) -> impl Future<Output = ()> + Send {
        let garbage_collector = Self { rt, database };
        async move {
            loop {
                let mut backoff = Backoff::new(
                    *SCHEDULED_JOB_GARBAGE_COLLECTION_INITIAL_BACKOFF,
                    *SCHEDULED_JOB_GARBAGE_COLLECTION_MAX_BACKOFF,
                );
                let mut batch_size = *SCHEDULED_JOB_GARBAGE_COLLECTION_BATCH_SIZE;
                while let Err(mut e) = garbage_collector.run(&mut backoff, batch_size).await {
                    if e.is_pagination_limit() {
                        // Retry with a smaller batch size
                        batch_size = batch_size.div_ceil(2);
                    }
                    let delay = backoff.fail(&mut garbage_collector.rt.rng());
                    tracing::error!("Scheduled job garbage collector failed, sleeping {delay:?}");
                    // Only report OCCs that happen repeatedly
                    if !e.is_occ() || (backoff.failures() as usize) > *UDF_EXECUTOR_OCC_MAX_RETRIES
                    {
                        report_error(&mut e).await;
                    }
                    garbage_collector.rt.wait(delay).await;
                }
            }
        }
    }

    async fn run(&self, backoff: &mut Backoff, batch_size: usize) -> anyhow::Result<()> {
        let mut tx = self.database.begin(Identity::system()).await?;
        let namespaces = tx
            .table_mapping()
            .namespaces_for_name(&SCHEDULED_JOBS_TABLE);
        let mut deleted_jobs = 0;
        let mut next_job_wait = None;
        'outer_loop: for namespace in namespaces {
            let now = self.rt.generate_timestamp()?;
            // query completed_ts > null
            let mut index_query = tx
                .query_system(namespace, &SCHEDULED_JOBS_INDEX_BY_COMPLETED_TS)?
                .range((Bound::Excluded([&ConvexValue::Null]), Bound::Unbounded))?
                .build();
            while let Some(job) = index_query.next().await? {
                match job.state {
                    ScheduledJobState::Success => (),
                    ScheduledJobState::Failed(_) => (),
                    ScheduledJobState::Canceled => (),
                    _ => anyhow::bail!("Scheduled job to be garbage collected has the wrong state"),
                }

                let completed_ts = match job.completed_ts {
                    Some(completed_ts) => completed_ts,
                    None => {
                        anyhow::bail!("Could not get completed_ts of finished scheduled job");
                    },
                };
                if completed_ts.add(*SCHEDULED_JOB_RETENTION)? > now {
                    let next_job_wait_ns = completed_ts.add(*SCHEDULED_JOB_RETENTION)? - now;
                    next_job_wait = match next_job_wait {
                        Some(next_job_wait) => Some(cmp::min(next_job_wait, next_job_wait_ns)),
                        None => Some(next_job_wait_ns),
                    };
                    break;
                }
                SchedulerModel::new(index_query.tx(), namespace)
                    .delete(job.id())
                    .await?;
                deleted_jobs += 1;
                // Avoid making this transaction too large.
                if deleted_jobs >= batch_size
                    || index_query.tx().user_tx_read_size().total_document_size
                        > TRANSACTION_MAX_READ_SIZE_BYTES
                            .saturating_sub(*MAX_SCHEDULED_JOB_ARGUMENT_SIZE_BYTES)
                            .saturating_sub(MAX_USER_SIZE)
                {
                    break 'outer_loop;
                }
            }
        }
        if deleted_jobs > 0 {
            tracing::debug!("Garbage collecting {deleted_jobs} finished scheduled jobs");
            self.database
                .commit_with_write_source(tx, "scheduled_job_gc")
                .await?;
            self.rt.wait(*SCHEDULED_JOB_GARBAGE_COLLECTION_DELAY).await;
        } else {
            let next_job_future = if let Some(next_job_wait) = next_job_wait {
                Either::Left(self.rt.wait(next_job_wait))
            } else {
                Either::Right(std::future::pending())
            };
            let token = tx.into_token()?;
            let subscription_fut = self.database.subscribe_and_wait_for_invalidation(token);
            select_biased! {
                _ = next_job_future.fuse() => {},
                _ = subscription_fut.fuse() => {},
            }
        }
        backoff.reset();
        Ok(())
    }
}
