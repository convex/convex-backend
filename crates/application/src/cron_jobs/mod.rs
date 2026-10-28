use std::{
    collections::{
        BTreeMap,
        HashSet,
    },
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use common::{
    self,
    backoff::Backoff,
    components::{
        CanonicalizedComponentFunctionPath,
        ComponentId,
        ComponentPath,
        PublicFunctionPath,
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
    identity::InertIdentity,
    knobs::{
        SCHEDULED_JOB_EXECUTION_PARALLELISM,
        UDF_EXECUTOR_OCC_MAX_RETRIES,
    },
    log_lines::LogLines,
    pause::Fault,
    runtime::Runtime,
    types::{
        FunctionCaller,
        UdfType,
    },
    RequestId,
};
use database::{
    BootstrapComponentsModel,
    Database,
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
    FutureExt,
    TryStreamExt,
};
use keybroker::Identity;
use model::{
    backend_state::BackendStateModel,
    cron_jobs::{
        next_ts::compute_next_ts,
        stream_cron_jobs_to_run,
        types::{
            CronJob,
            CronJobLogLines,
            CronJobResult,
            CronJobState,
            CronJobStatus,
            CronNextRun,
        },
        CronModel,
    },
    modules::ModuleModel,
};
use sentry::SentryFutureExt;
use sync_types::Timestamp;
use tokio::sync::mpsc;
use udf::validation::ValidatedUdfOutcome;
use usage_tracking::FunctionUsageTracker;
use value::{
    JsonPackedValue,
    PendingValue,
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

const INITIAL_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(15);
pub(crate) const CRON_COMITTING: &str = "cron_committing";
pub(crate) const CRON_JOB_EXECUTING: &str = "cron_job_executing";
pub(crate) const CRON_JOB_EXECUTED: &str = "cron_job_executed";
pub(crate) const CRON_JOB_SUCCEEDED: &str = "cron_job_succeeded";

// Truncate result and log lines for cron job logs since they are only
// used for the dashboard
const CRON_LOG_MAX_RESULT_LENGTH: usize = 1000;
const CRON_LOG_MAX_LOG_LINE_LENGTH: usize = 1000;

// This code is very similar to ScheduledJobExecutor and could potentially be
// refactored later.
pub struct CronJobExecutor<RT: Runtime> {
    context: CronJobContext<RT>,
    running_job_ids: HashSet<ResolvedDocumentId>,
    /// Some if there's at least one pending job. May be in the past!
    next_job_ready_time: Option<Timestamp>,
    job_finished_tx: mpsc::Sender<ResolvedDocumentId>,
    job_finished_rx: mpsc::Receiver<ResolvedDocumentId>,
}

#[derive(Clone)]
pub struct CronJobContext<RT: Runtime> {
    rt: RT,
    deployment_name: String,
    database: Database<RT>,
    runner: Arc<ApplicationFunctionRunner<RT>>,
    function_log: FunctionExecutionLog<RT>,
}

impl<RT: Runtime> CronJobExecutor<RT> {
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
            context: CronJobContext {
                rt,
                deployment_name,
                database,
                runner,
                function_log,
            },
            running_job_ids: HashSet::new(),
            next_job_ready_time: None,
            job_finished_tx,
            job_finished_rx,
        };
        let mut backoff = Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF);
        tracing::info!("Starting cron job executor");
        loop {
            match executor.run_once().await {
                Ok(()) => backoff.reset(),
                Err(mut e) => {
                    metrics::log_cron_job_executor_error();
                    // Only report OCCs that happen repeatedly
                    if !e.is_occ() || (backoff.failures() as usize) > *UDF_EXECUTOR_OCC_MAX_RETRIES
                    {
                        report_error(&mut e).await;
                    }
                    let delay = backoff.fail(&mut executor.context.rt.rng());
                    tracing::error!("Cron job executor failed, sleeping {delay:?}");
                    executor.context.rt.wait(delay).await;
                },
            }
        }
    }

    async fn run_once(&mut self) -> anyhow::Result<()> {
        metrics::log_cron_job_executor_poll();
        let mut tx = self.context.database.begin(Identity::Unknown(None)).await?;
        let backend_state = BackendStateModel::new(&mut tx).get_backend_state().await?;
        let is_backend_stopped = backend_state.is_stopped();

        self.next_job_ready_time = if is_backend_stopped {
            None
        } else if self.running_job_ids.len() == *SCHEDULED_JOB_EXECUTION_PARALLELISM {
            self.next_job_ready_time
        } else {
            self.query_and_start_jobs(&mut tx).await?
        };

        metrics::log_running_jobs(self.running_job_ids.len());

        let next_job_future = if let Some(next_job_ts) = self.next_job_ready_time {
            let now = self.context.rt.generate_timestamp()?;
            Either::Left(if next_job_ts < now {
                metrics::log_cron_job_execution_lag(now - next_job_ts);
                metrics::log_cron_job_backlog(now - next_job_ts);
                // If we're behind, re-run this loop every 5 seconds to log the gauge above and
                // track how far we're behind in our metrics.
                self.context.rt.wait(Duration::from_secs(5))
            } else {
                metrics::log_cron_job_execution_lag(Duration::from_secs(0));
                metrics::log_cron_job_backlog(Duration::ZERO);
                self.context.rt.wait(next_job_ts - now)
            })
        } else {
            metrics::log_cron_job_execution_lag(Duration::from_secs(0));
            metrics::log_cron_job_backlog(Duration::ZERO);
            Either::Right(std::future::pending())
        };

        let token = tx.into_token()?;
        let subscription_fut = self
            .context
            .database
            .subscribe_and_wait_for_invalidation(token);
        select_biased! {
            job_id = self.job_finished_rx.recv().fuse() => {
                if let Some(job_id) = job_id {
                    self.context.rt.pause_client().wait(CRON_JOB_EXECUTED).await;
                    self.running_job_ids.remove(&job_id);
                } else {
                    anyhow::bail!("Job results channel closed, this is unexpected!");
                }
            },
            _ = next_job_future.fuse() => {
            }
            _ = subscription_fut.fuse() => {
            },
        }
        Ok(())
    }

    async fn query_and_start_jobs(
        &mut self,
        tx: &mut Transaction<RT>,
    ) -> anyhow::Result<Option<Timestamp>> {
        let now = self.context.rt.generate_timestamp()?;
        let mut job_stream = stream_cron_jobs_to_run(tx);
        while let Some(job) = job_stream.try_next().await? {
            let job_id = job.id;
            if self.running_job_ids.contains(&job_id) {
                continue;
            }
            let next_ts = job.next_ts;
            // If we can't execute the job return the job's target timestamp. If we're
            // caught up, we can sleep until the timestamp. If we're behind and
            // at our concurrency limit, we can use the timestamp to log how far
            // behind we get.
            if next_ts > now || self.running_job_ids.len() == *SCHEDULED_JOB_EXECUTION_PARALLELISM {
                return Ok(Some(next_ts));
            }
            let sentry_hub = sentry::Hub::with(|hub| sentry::Hub::new_from_top(hub));
            let context = self.context.clone();
            let tx = self.job_finished_tx.clone();
            // TODO: cancel this handle with the application
            self.context.rt.spawn_background(
                "spawn_cron_job",
                async move {
                    select_biased! {
                        _ = tx.closed().fuse() => {
                            tracing::error!("Cron job receiver closed");
                        },
                        result = context.execute_job(job).fuse() => {
                            let _ = tx.send(result).await;
                        },
                    }
                }
                .bind_hub(sentry_hub),
            );
            self.running_job_ids.insert(job_id);
        }
        Ok(None)
    }
}

impl<RT: Runtime> CronJobContext<RT> {
    // This handles re-running the cron job on transient errors. It
    // guarantees that the job was successfully run or the job state changed.
    pub async fn execute_job(&self, job: CronJob) -> ResolvedDocumentId {
        let mut function_backoff = Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF);
        loop {
            let mutation_retry_count = function_backoff.failures() as usize;
            let root = get_sampled_span(
                &self.deployment_name,
                "crons/run_function",
                &mut self.rt.rng(),
            )
            .with_properties(|| {
                [
                    ("job_id", job.id.to_string()),
                    ("component", format!("{:?}", job.component)),
                    ("job_name", job.name.to_string()),
                ]
            });
            let result = self
                .run_function(job.clone(), mutation_retry_count)
                .in_span(root)
                .await;
            match result {
                Ok(result) => {
                    self.rt.pause_client().wait(CRON_JOB_SUCCEEDED).await;
                    metrics::log_cron_job_success(function_backoff.failures());
                    return result;
                },
                Err(mut e) => {
                    let delay = function_backoff.fail(&mut self.rt.rng());
                    tracing::error!(
                        "System error executing job {} in {:?}: {}, sleeping {delay:?}",
                        job.id,
                        job.component,
                        job.name
                    );
                    report_error(&mut e).await;
                    metrics::log_cron_job_failure(&e);
                    self.rt.wait(delay).await;
                },
            }
        }
    }

    async fn run_function(
        &self,
        job: CronJob,
        mutation_retry_count: usize,
    ) -> anyhow::Result<ResolvedDocumentId> {
        let usage_tracker = FunctionUsageTracker::new();
        let Some(mut tx) = self
            .new_transaction_for_job_state(&job, usage_tracker.clone())
            .await?
        else {
            return Ok(job.id);
        };
        let (_, component_path) = self.get_job_component(&mut tx, job.id).await?;
        tracing::debug!("Executing {:?}!", job.cron_spec.udf_path);

        // Cron specs omit the function type; use the current module definition.
        let path = CanonicalizedComponentFunctionPath {
            component: component_path,
            udf_path: job.cron_spec.udf_path.clone(),
        };
        let udf_type = ModuleModel::new(&mut tx)
            .get_analyzed_function(&path)
            .await?
            .map_err(|e| {
                anyhow::anyhow!(
                    "Cron trying to execute missing function. This should have been checked \
                     during analyze. Error: {e}"
                )
            })?
            .udf_type;

        let job_id = job.id;
        match udf_type {
            UdfType::Mutation => {
                self.handle_mutation(tx, job, usage_tracker, mutation_retry_count)
                    .await?
            },
            UdfType::Action => {
                self.handle_action(tx, job, usage_tracker).await?;
            },
            UdfType::Query | UdfType::HttpAction => {
                anyhow::bail!(
                    "Cron trying to execute {} which is a {} function. This should have been \
                     checked during analyze.",
                    job.cron_spec.udf_path,
                    udf_type
                );
            },
        };
        Ok(job_id)
    }

    fn truncate_result(
        &self,
        result: JsonPackedValue<PendingValue>,
        udf_path: &CanonicalizedComponentFunctionPath,
    ) -> anyhow::Result<CronJobResult> {
        let value = result.unpack().map_err(|e| {
            e.wrap_error_message(|msg| {
                format!("Cron job {} result invalid: {msg}", udf_path.debug_str())
            })
        })?;
        let mut value_str = match &value {
            PendingValue::Concrete(value) => value.to_string(),
            // TODO: implement Display for PendingValue
            pending => serde_json::to_string(&pending.to_uncommitted_json_serializable())?,
        };
        if value_str.len() <= CRON_LOG_MAX_RESULT_LENGTH {
            Ok(CronJobResult::Default(value))
        } else {
            value_str =
                value_str[..value_str.floor_char_boundary(CRON_LOG_MAX_RESULT_LENGTH)].to_string();
            Ok(CronJobResult::Truncated(value_str))
        }
    }

    fn truncate_log_lines(&self, log_lines: LogLines) -> CronJobLogLines {
        let mut new_log_lines = Vec::new();
        let mut is_truncated = false;
        let mut size = 0;
        for log in log_lines
            .into_iter()
            .flat_map(|log| log.to_pretty_strings())
        {
            let line_len = log.len();
            if size + line_len <= CRON_LOG_MAX_LOG_LINE_LENGTH {
                new_log_lines.push(log);
                size += line_len;
            } else {
                is_truncated = true;
                break;
            }
        }
        CronJobLogLines {
            log_lines: new_log_lines.into(),
            is_truncated,
        }
    }

    async fn get_job_component(
        &self,
        tx: &mut Transaction<RT>,
        job_id: ResolvedDocumentId,
    ) -> anyhow::Result<(ComponentId, ComponentPath)> {
        let component = tx
            .table_mapping()
            .tablet_namespace(job_id.tablet_id)?
            .into();
        let component_path = BootstrapComponentsModel::new(tx).must_component_path(component)?;
        Ok((component, component_path))
    }

    async fn handle_mutation(
        &self,
        mut tx: Transaction<RT>,
        job: CronJob,
        usage_tracker: FunctionUsageTracker,
        mutation_retry_count: usize,
    ) -> anyhow::Result<()> {
        let permit = self.runner.acquire_mutation_permit().await;
        let start = self.rt.monotonic_now();
        let identity = tx.inert_identity();
        let caller = FunctionCaller::Cron;
        let (component, component_path) = self.get_job_component(&mut tx, job.id).await?;
        let request_id = RequestId::new();
        let context =
            ExecutionContext::new(RequestContext::new_for_system_request(request_id), &caller);
        sentry::configure_scope(|scope| context.add_sentry_tags(scope));
        let path = CanonicalizedComponentFunctionPath {
            component: component_path,
            udf_path: job.cron_spec.udf_path.clone(),
        };
        // Hacks: Clean up old logs *before* running the mutation to avoid a
        // system error if the user mutation leaves the transaction's read set
        // at exactly the max size
        CronModel::new(&mut tx, component)
            .prepare_insert_cron_job_log(&job)
            .await?;
        let mutation_result =
            if let Fault::Error(e) = self.rt.pause_client().wait(CRON_JOB_EXECUTING).await {
                tracing::info!("Injected error before running mutation");
                Err(e)
            } else {
                self.runner
                    .run_mutation_no_udf_log(
                        tx,
                        permit,
                        PublicFunctionPath::Component(path.clone()),
                        job.cron_spec.udf_args.clone(),
                        caller.allowed_visibility(),
                        context.clone(),
                        None,
                    )
                    .await
            };
        let (stats, execution_time, outcome) = match mutation_result {
            Ok((mut tx, mut outcome)) => {
                let stats = tx.take_stats();
                let execution_time = start.elapsed();
                if let Ok(ref result) = outcome.result {
                    let truncated_result = self.truncate_result(result.clone(), &path)?;
                    let status = CronJobStatus::Success(truncated_result);
                    CronModel::new(&mut tx, component)
                        .insert_cron_job_log(
                            &job,
                            status,
                            self.truncate_log_lines(outcome.log_lines.clone()),
                            execution_time.as_secs_f64(),
                        )
                        .await?;
                    self.complete_job_run(
                        identity.clone(),
                        &mut tx,
                        &job,
                        UdfType::Mutation,
                        context.clone(),
                        Some(mutation_retry_count),
                    )
                    .await?;
                    let commit_result = if let Fault::Error(e) =
                        self.rt.pause_client().wait(CRON_COMITTING).await
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
                            self.function_log
                                .log_mutation_occ_error(
                                    outcome,
                                    stats,
                                    execution_time,
                                    caller,
                                    usage_tracker,
                                    context,
                                    occ_info,
                                    None,
                                    mutation_retry_count,
                                    mutation_retry_count,
                                    true,
                                )
                                .await;
                            return Err(err);
                        } else {
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
                            job.cron_spec.udf_args.clone(),
                            identity,
                            start,
                            caller,
                            context,
                            None,
                            mutation_retry_count,
                            true,
                        )
                        .await?;
                    return Err(e);
                } else if e.is_deterministic_user_error() {
                    // A deterministic user error fails the same way on every attempt. Synthesize a
                    // failed outcome so the failure path below records the run
                    // and advances to the next scheduled run instead of
                    // retrying it like a transient system error.
                    let outcome = ValidatedUdfOutcome::from_error(
                        JsError::from_error(e),
                        path,
                        job.cron_spec.udf_args.clone(),
                        identity.clone(),
                        self.rt.clone(),
                        None,
                    )?;
                    (BTreeMap::new(), start.elapsed(), outcome)
                } else {
                    self.function_log
                        .log_mutation_system_error(
                            &e,
                            path,
                            job.cron_spec.udf_args.clone(),
                            identity,
                            start,
                            caller,
                            context,
                            None,
                            mutation_retry_count,
                        )
                        .await?;
                    return Err(e);
                }
            },
        };
        if let Err(ref e) = outcome.result {
            // UDF failed due to developer error. It is not safe to commit the
            // transaction it executed in. We should remove the job in a new
            // transaction.
            let Some(mut tx) = self
                .new_transaction_for_job_state(&job, usage_tracker.clone())
                .await?
            else {
                // Continue without updating since the job state has changed
                return Ok(());
            };
            let mut model = CronModel::new(&mut tx, component);
            let status = CronJobStatus::Err(e.to_string());
            model
                .insert_cron_job_log(
                    &job,
                    status,
                    self.truncate_log_lines(outcome.log_lines.clone()),
                    execution_time.as_secs_f64(),
                )
                .await?;
            self.complete_job_run(
                identity,
                &mut tx,
                &job,
                UdfType::Mutation,
                context.clone(),
                Some(mutation_retry_count),
            )
            .await?;
            // NOTE: We should not be getting developer errors here.
            self.database
                .commit_with_write_source(tx, "cron_save_mutation_error")
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

        Ok(())
    }

    async fn handle_action(
        &self,
        mut tx: Transaction<RT>,
        job: CronJob,
        usage_tracker: FunctionUsageTracker,
    ) -> anyhow::Result<()> {
        let component = tx
            .table_mapping()
            .tablet_namespace(job.id.tablet_id)?
            .into();
        let identity = tx.identity().clone();
        let (_, component_path) = self.get_job_component(&mut tx, job.id).await?;
        let caller = FunctionCaller::Cron;
        match job.state {
            CronJobState::Pending => {
                // Create a new request & execution ID
                let request_id = RequestId::new();
                let context = ExecutionContext::new(
                    RequestContext::new_for_system_request(request_id),
                    &caller,
                );
                sentry::configure_scope(|scope| context.add_sentry_tags(scope));

                // Set state to in progress
                let mut updated_job = job.clone();
                updated_job.state = CronJobState::InProgress {
                    request_id: context.request_id.clone(),
                    execution_id: context.execution_id,
                };
                CronModel::new(&mut tx, component)
                    .update_job_state(updated_job.cron_next_run())
                    .await?;
                self.database
                    .commit_with_write_source(tx, "cron_in_progress")
                    .await?;

                // Execute the action
                let path = CanonicalizedComponentFunctionPath {
                    component: component_path,
                    udf_path: job.cron_spec.udf_path.clone(),
                };
                let completion = self
                    .runner
                    .run_action_no_udf_log(
                        PublicFunctionPath::Component(path),
                        job.cron_spec.udf_args.clone(),
                        identity.clone(),
                        caller,
                        usage_tracker.clone(),
                        context.clone(),
                        PermitTimeout::WaitIndefinitely,
                    )
                    .await?;
                let execution_time_f64 = completion.execution_time.as_secs_f64();
                let truncated_log_lines = self.truncate_log_lines(completion.log_lines.clone());

                let status = match completion.outcome.result.clone() {
                    Ok(result) => {
                        let truncated_result =
                            self.truncate_result(result.into(), &completion.outcome.path)?;
                        CronJobStatus::Success(truncated_result)
                    },
                    Err(e) => CronJobStatus::Err(e.to_string()),
                };

                // Mark the job as completed. Keep trying until we succeed (or
                // detect the job state has changed). Don't bubble up the error
                // since otherwise we will lose the original execution logs.
                let mut backoff = Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF);
                let identity: InertIdentity = identity.into();
                while let Err(mut err) = self
                    .complete_action_run(
                        identity.clone(),
                        &updated_job,
                        status.clone(),
                        truncated_log_lines.clone(),
                        execution_time_f64,
                        usage_tracker.clone(),
                        context.clone(),
                    )
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
            CronJobState::InProgress {
                ref request_id,
                ref execution_id,
            } => {
                // This case can happen if there is a system error while executing
                // the action or if backend exits after executing the action but
                // before updating the state. Since we execute actions at most once,
                // complete this job and log the error.
                let err =
                    JsError::from_message("Transient error while executing action".to_string());
                let status = CronJobStatus::Err(err.to_string());
                let log_lines = CronJobLogLines {
                    log_lines: vec![].into(),
                    is_truncated: false,
                };
                // Restore the execution ID of the failed execution.
                let context = ExecutionContext::new_from_parts(
                    request_id.clone(),
                    *execution_id,
                    caller.parent_scheduled_job(),
                    caller.is_root(),
                    RequestMetadata::system(),
                );
                sentry::configure_scope(|scope| context.add_sentry_tags(scope));
                let mut model = CronModel::new(&mut tx, component);
                model
                    .insert_cron_job_log(&job, status, log_lines, 0.0)
                    .await?;
                let identity: InertIdentity = identity.into();
                self.complete_job_run(
                    identity.clone(),
                    &mut tx,
                    &job,
                    UdfType::Action,
                    context.clone(),
                    None,
                )
                .await?;
                self.database
                    .commit_with_write_source(tx, "cron_finish_action")
                    .await?;

                let path = CanonicalizedComponentFunctionPath {
                    component: component_path,
                    udf_path: job.cron_spec.udf_path,
                };
                let mut err = err.into();
                self.function_log
                    .log_action_system_error(
                        &err,
                        path,
                        job.cron_spec.udf_args.clone(),
                        identity,
                        self.rt.monotonic_now(),
                        caller,
                        vec![].into(),
                        context,
                    )
                    .await?;
                report_error(&mut err).await;
            },
        }
        Ok(())
    }

    // Creates a new transaction and verifies the job state matches the given one.
    async fn new_transaction_for_job_state(
        &self,
        expected_state: &CronJob,
        usage_tracker: FunctionUsageTracker,
    ) -> anyhow::Result<Option<Transaction<RT>>> {
        let mut tx = self
            .database
            .begin_with_usage(Identity::Unknown(None), usage_tracker)
            .await?;
        // Verify that the cron job has not changed.
        let new_job = CronModel::new(&mut tx, expected_state.component)
            .get(expected_state.id)
            .await?;
        Ok((new_job.as_ref() == Some(expected_state)).then_some(tx))
    }

    // Completes an action in separate transaction. Returns false if the action
    // state has changed.
    async fn complete_action_run(
        &self,
        identity: InertIdentity,
        expected_state: &CronJob,
        status: CronJobStatus,
        log_lines: CronJobLogLines,
        execution_time: f64,
        usage_tracker: FunctionUsageTracker,
        context: ExecutionContext,
    ) -> anyhow::Result<()> {
        let Some(mut tx) = self
            .new_transaction_for_job_state(expected_state, usage_tracker)
            .await?
        else {
            // Continue without updating since the job state has changed
            return Ok(());
        };
        let component = tx
            .table_mapping()
            .tablet_namespace(expected_state.id.tablet_id)?
            .into();
        let mut model = CronModel::new(&mut tx, component);
        model
            .insert_cron_job_log(expected_state, status, log_lines, execution_time)
            .await?;
        self.complete_job_run(
            identity,
            &mut tx,
            expected_state,
            UdfType::Action,
            context,
            None,
        )
        .await?;
        self.database
            .commit_with_write_source(tx, "cron_complete_action")
            .await?;
        Ok(())
    }

    async fn complete_job_run(
        &self,
        identity: InertIdentity,
        tx: &mut Transaction<RT>,
        job: &CronJob,
        udf_type: UdfType,
        context: ExecutionContext,
        mutation_retry_count: Option<usize>,
    ) -> anyhow::Result<()> {
        let now = self.rt.generate_timestamp()?;
        let prev_ts = job.next_ts;
        let mut next_ts = compute_next_ts(&job.cron_spec, Some(prev_ts), now, &mut self.rt.rng())?;
        let mut num_skipped = 0;
        let first_skipped_ts = next_ts;
        let (component, component_path) = self.get_job_component(tx, job.id).await?;
        let mut model = CronModel::new(tx, component);
        while next_ts < now {
            num_skipped += 1;
            next_ts = compute_next_ts(&job.cron_spec, Some(next_ts), now, &mut self.rt.rng())?;
        }
        if num_skipped > 0 {
            let job_id = job.id.developer_id;
            let job_name = &job.name;
            tracing::info!(
                "Skipping {num_skipped} run(s) of job {job_id} because multiple scheduled runs \
                 are in the past"
            );
            match udf_type {
                // These aren't system errors in the sense that they represent an issue with Convex
                // (e.g. they can occur due to the developer pausing their deployment)
                // but they get logged similarly, since they shouldn't count towards usage and
                // should appear as errors
                UdfType::Mutation => {
                    self.function_log
                        .log_mutation_system_error(
                            &anyhow::anyhow!(ErrorMetadata::bad_request(
                                "SkippingPastScheduledRuns",
                                format!(
                                    "Skipping {num_skipped} run(s) of cron job '{job_name}' \
                                     because multiple scheduled runs are in the past"
                                )
                            )),
                            CanonicalizedComponentFunctionPath {
                                component: component_path,
                                udf_path: job.cron_spec.udf_path.clone(),
                            },
                            job.cron_spec.udf_args.clone(),
                            identity,
                            self.rt.monotonic_now(),
                            FunctionCaller::Cron,
                            context,
                            None,
                            mutation_retry_count
                                .context("Mutations should have mutation_retry_count set")?,
                        )
                        .await?;
                },
                UdfType::Action => {
                    anyhow::ensure!(
                        mutation_retry_count.is_none(),
                        "Actions should not have mutation_retry_count set"
                    );
                    let err = anyhow::anyhow!(ErrorMetadata::bad_request(
                        "SkippingPastScheduledRuns",
                        format!(
                            "Skipping {num_skipped} run(s) of cron job '{job_name}' because \
                             multiple scheduled runs are in the past"
                        )
                    ));
                    self.function_log
                        .log_action_system_error(
                            &err,
                            CanonicalizedComponentFunctionPath {
                                component: component_path,
                                udf_path: job.cron_spec.udf_path.clone(),
                            },
                            job.cron_spec.udf_args.clone(),
                            identity,
                            self.rt.monotonic_now(),
                            FunctionCaller::Cron,
                            vec![].into(),
                            context,
                        )
                        .await?;
                    tracing::error!("{err:#}");
                },
                UdfType::Query | UdfType::HttpAction => {
                    anyhow::bail!("Executing unexpected function type as a cron")
                },
            }

            let status = CronJobStatus::Canceled {
                num_canceled: num_skipped,
            };
            let log_lines = CronJobLogLines {
                log_lines: vec![].into(),
                is_truncated: false,
            };
            let mut canceled_job = job.clone();
            canceled_job.next_ts = first_skipped_ts;
            model
                .insert_cron_job_log(&canceled_job, status, log_lines, 0.0)
                .await?;
        }

        let next_run = CronNextRun {
            cron_job_id: job.id.developer_id,
            state: CronJobState::Pending,
            prev_ts: Some(prev_ts),
            next_ts,
        };
        model.update_job_state(next_run).await?;
        Ok(())
    }
}
