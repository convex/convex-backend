use core::sync::atomic::Ordering;
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    sync::{
        atomic::AtomicUsize,
        Arc,
        LazyLock,
    },
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use authentication::token_to_authorization_header;
use common::{
    audit_log_lines::AuditLogVars,
    auth::AuthConfig,
    backoff::Backoff,
    bootstrap_model::components::{
        definition::ComponentDefinitionMetadata,
        handles::FunctionHandle,
    },
    components::{
        CanonicalizedComponentFunctionPath,
        ComponentDefinitionPath,
        ComponentId,
        ComponentName,
        ComponentPath,
        PublicFunctionPath,
        Resource,
    },
    errors::JsError,
    execution_context::{
        ExecutionContext,
        RequestContext,
        RequestId,
    },
    fastrace_helpers::EncodedSpan,
    knobs::{
        APPLICATION_FUNCTION_RUNNER_ACTION_SEMAPHORE_TIMEOUT,
        APPLICATION_FUNCTION_RUNNER_SEMAPHORE_TIMEOUT,
        APPLICATION_MAX_CONCURRENT_MUTATIONS,
        APPLICATION_MAX_CONCURRENT_NODE_ACTIONS,
        APPLICATION_MAX_CONCURRENT_QUERIES,
        APPLICATION_MAX_CONCURRENT_V8_ACTIONS,
        DEFAULT_APPLICATION_MAX_FUNCTION_CONCURRENCY,
        ISOLATE_MAX_USER_HEAP_SIZE,
        UDF_EXECUTOR_OCC_INITIAL_BACKOFF,
        UDF_EXECUTOR_OCC_MAX_BACKOFF,
        UDF_EXECUTOR_OCC_MAX_RETRIES,
        UDF_FILE_STORAGE_INITIAL_BACKOFF,
        UDF_FILE_STORAGE_MAX_BACKOFF,
        UDF_FILE_STORAGE_MAX_RETRIES,
    },
    log_lines::{
        run_function_and_collect_log_lines,
        LogLine,
        LogLines,
    },
    query_journal::QueryJournal,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    schemas::DatabaseSchema,
    tokio::sync::{
        Semaphore,
        SemaphorePermit,
    },
    types::{
        AllowedVisibility,
        AttributedCaller,
        AttributionClaims,
        DeploymentMetadata,
        FunctionCaller,
        ModuleEnvironment,
        NodeDependency,
        QueryInvocation,
        Timestamp,
        UdfType,
    },
};
use database::{
    unauthorized_error,
    Database,
    Transaction,
    WriteSource,
    MAX_OCC_FAILURES,
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use file_storage::{
    TransactionalFileStorage,
    FILE_DOWNLOAD_FAILED_SHORT_MSG,
    FILE_UPLOAD_FAILED_SHORT_MSG,
};
use function_runner::{
    server::{
        FunctionMetadata,
        HttpActionMetadata,
    },
    FunctionReads,
    FunctionRunner,
    FunctionWrites,
};
use futures::{
    future,
    FutureExt,
};
use keybroker::{
    DeploymentOp,
    Identity,
    KeyBroker,
};
use model::{
    backend_info::BackendInfoModel,
    backend_state::BackendStateModel,
    components::handles::FunctionHandlesModel,
    config::types::ModuleConfig,
    environment_variables::{
        types::{
            EnvVarName,
            EnvVarValue,
        },
        EnvironmentVariablesModel,
    },
    external_packages::{
        types::ExternalDepsPackage,
        ExternalPackagesModel,
    },
    file_storage::{
        types::FileStorageEntry,
        FileStorageId,
    },
    modules::{
        module_versions::{
            AnalyzedModule,
            ModuleSource,
            SourceMap,
        },
        ModuleModel,
    },
    scheduled_jobs::VirtualSchedulerModel,
    session_requests::{
        types::{
            SessionRequestIdentifier,
            SessionRequestOutcome,
            SessionRequestRecord,
        },
        SessionRequestModel,
    },
    source_packages::{
        types::SourcePackage,
        SourcePackageModel,
    },
    udf_config::types::UdfConfig,
};
use node_executor::{
    AnalyzeRequest,
    BuildDepsRequest,
    ExecuteRequest,
    NodeActions,
};
use roles::RequireDeploymentOp;
use serde_json::Value as JsonValue;
use storage::Storage;
use sync_types::{
    types::SerializedArgs,
    CanonicalizedModulePath,
};
use tokio::{
    select,
    sync::mpsc,
};
use udf::{
    environment::system_env_vars,
    validation::{
        validate_schedule_args,
        PendingArgsPolicy,
        ValidatedActionOutcome,
        ValidatedPathAndArgs,
        ValidatedUdfOutcome,
    },
    warnings::scheduled_arg_size_warning,
    ActionCallbacks,
    ActionOutcome,
    EvaluateAppDefinitionsResult,
    FunctionOutcome,
    FunctionResult,
    HttpActionOutcome,
    UdfOutcome,
};
use usage_tracking::{
    FunctionUsageStats,
    FunctionUsageTracker,
};
use value::{
    id_v6::DeveloperDocumentId,
    identifier::Identifier,
    serialized_args_ext::SerializedArgsExt,
    JsonPackedValue,
    Size as _,
    TableNamespace,
};
use vector::{
    PublicVectorSearchQueryResult,
    VectorSearch,
};

use self::metrics::{
    function_waiter_timer,
    log_occ_retries,
    log_outstanding_functions,
    log_udf_executor_result,
    mutation_timer,
    UdfExecutorResult,
};
use crate::{
    ai_gateway_jwt::AiGatewayJwtMinter,
    application_function_runner::metrics::{
        function_run_timer,
        function_total_timer,
        log_function_wait_timeout,
        log_mutation_already_committed,
    },
    audit_logging::AuditLogClient,
    cache::{
        CacheManager,
        QueryCache,
    },
    function_log::{
        ActionCompletion,
        FunctionExecutionLog,
        OutstandingFunctionState,
    },
    source_map_cache::SourceMapCache,
    ActionError,
    ActionReturn,
    MutationError,
    MutationReturn,
    QueryReturn,
};

mod http_routing;
mod metrics;

static BUILD_DEPS_TIMEOUT: LazyLock<Duration> = LazyLock::new(|| Duration::from_secs(600));

/// Wrapper for [IsolateClient]s and [FunctionRunner]s that determines where to
/// route requests.
#[derive(Clone)]
pub struct FunctionRouter<RT: Runtime> {
    pub(crate) function_runner: Arc<dyn FunctionRunner<RT>>,
    query_limiter: Arc<Limiter<RT>>,
    mutation_limiter: Arc<Limiter<RT>>,
    action_limiter: Arc<Limiter<RT>>,

    rt: RT,
    database: Database<RT>,
    default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
}

impl<RT: Runtime> FunctionRouter<RT> {
    pub fn new(
        function_runner: Arc<dyn FunctionRunner<RT>>,
        rt: RT,
        database: Database<RT>,
        default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
        function_log: FunctionExecutionLog<RT>,
    ) -> Self {
        let query_limiter = Arc::new(Limiter::new(
            ModuleEnvironment::Isolate,
            UdfType::Query,
            *APPLICATION_MAX_CONCURRENT_QUERIES,
            *APPLICATION_FUNCTION_RUNNER_SEMAPHORE_TIMEOUT,
            function_log.clone(),
        ));
        let mutation_limiter = Arc::new(Limiter::new(
            ModuleEnvironment::Isolate,
            UdfType::Mutation,
            *APPLICATION_MAX_CONCURRENT_MUTATIONS,
            *APPLICATION_FUNCTION_RUNNER_SEMAPHORE_TIMEOUT,
            function_log.clone(),
        ));
        let action_limiter = Arc::new(Limiter::new(
            ModuleEnvironment::Isolate,
            UdfType::Action,
            *APPLICATION_MAX_CONCURRENT_V8_ACTIONS,
            *APPLICATION_FUNCTION_RUNNER_ACTION_SEMAPHORE_TIMEOUT,
            function_log,
        ));
        Self {
            function_runner,
            rt,
            database,
            default_system_env_vars,
            query_limiter,
            mutation_limiter,
            action_limiter,
        }
    }
}

impl<RT: Runtime> FunctionRouter<RT> {
    pub(crate) async fn acquire_permit(
        &self,
        udf_type: UdfType,
    ) -> anyhow::Result<RequestGuard<'_, RT>> {
        self.limiter(udf_type)
            .acquire_permit(&self.rt, PermitTimeout::Enforce)
            .await
    }

    fn limiter(&self, udf_type: UdfType) -> &Limiter<RT> {
        match udf_type {
            UdfType::Query => &self.query_limiter,
            UdfType::Mutation => &self.mutation_limiter,
            UdfType::Action | UdfType::HttpAction => &self.action_limiter,
        }
    }

    #[fastrace::trace]
    pub(crate) async fn execute_query_or_mutation(
        &self,
        tx: Transaction<RT>,
        permit: RequestGuard<'_, RT>,
        path_and_args: ValidatedPathAndArgs,
        udf_type: UdfType,
        journal: QueryJournal,
        context: ExecutionContext,
    ) -> anyhow::Result<(Transaction<RT>, FunctionOutcome)> {
        anyhow::ensure!(udf_type == UdfType::Query || udf_type == UdfType::Mutation);
        anyhow::ensure!(
            permit.limiter.udf_type == udf_type,
            "Running a {udf_type} with a {} permit",
            permit.limiter.udf_type
        );
        // All queries and mutations are run in the isolate environment.
        let timer = function_total_timer(ModuleEnvironment::Isolate, udf_type);
        let (tx, outcome) = self
            .function_runner_execute(
                tx,
                permit,
                udf_type,
                context,
                None,
                Some(FunctionMetadata {
                    journal,
                    path_and_args,
                }),
                None,
            )
            .await?;
        let tx = tx.with_context(|| format!("Missing transaction in response for {udf_type}"))?;
        timer.finish();
        Ok((tx, outcome))
    }

    #[fastrace::trace]
    pub(crate) async fn execute_action(
        &self,
        tx: Transaction<RT>,
        path_and_args: ValidatedPathAndArgs,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
        context: ExecutionContext,
        permit_timeout: PermitTimeout,
    ) -> anyhow::Result<ActionOutcome> {
        let permit = self
            .action_limiter
            .acquire_permit(&self.rt, permit_timeout)
            .await?;
        let (_, outcome) = self
            .function_runner_execute(
                tx,
                permit,
                UdfType::Action,
                context,
                Some(log_line_sender),
                Some(FunctionMetadata {
                    journal: QueryJournal::new(),
                    path_and_args,
                }),
                None,
            )
            .await?;

        let FunctionOutcome::Action(outcome) = outcome else {
            anyhow::bail!("Calling an action returned an invalid outcome")
        };
        Ok(outcome)
    }

    #[fastrace::trace]
    pub(crate) async fn execute_http_action(
        &self,
        tx: Transaction<RT>,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
        http_action_metadata: HttpActionMetadata,
        context: ExecutionContext,
    ) -> anyhow::Result<HttpActionOutcome> {
        // All http actions are run in the isolate environment.
        let timer = function_total_timer(ModuleEnvironment::Isolate, UdfType::HttpAction);
        let permit = self.acquire_permit(UdfType::HttpAction).await?;
        let (_, outcome) = self
            .function_runner_execute(
                tx,
                permit,
                UdfType::HttpAction,
                context,
                Some(log_line_sender),
                None,
                Some(http_action_metadata),
            )
            .await?;

        let FunctionOutcome::HttpAction(outcome) = outcome else {
            anyhow::bail!("Calling an http action returned an invalid outcome")
        };
        timer.finish();
        Ok(outcome)
    }

    // Drain the concurrency permits for `udf_type` to deterministically
    // exercise the concurrency limit in tests.
    #[fastrace::trace]
    async fn function_runner_execute(
        &self,
        mut tx: Transaction<RT>,
        permit: RequestGuard<'_, RT>,
        udf_type: UdfType,
        context: ExecutionContext,
        log_line_sender: Option<mpsc::UnboundedSender<LogLine>>,
        function_metadata: Option<FunctionMetadata>,
        http_action_metadata: Option<HttpActionMetadata>,
    ) -> anyhow::Result<(Option<Transaction<RT>>, FunctionOutcome)> {
        // Read in this transaction so preference changes invalidate cached queries.
        let udf_runtime = BackendInfoModel::new(&mut tx)
            .get()
            .await?
            .and_then(|info| info.default_udf_runtime);
        let in_memory_index_last_modified = self
            .database
            .snapshot(tx.begin_timestamp())?
            .in_memory_indexes
            .in_memory_indexes_last_modified();

        let timer = function_run_timer(udf_type);
        let (function_tx, outcome, usage_stats) = self
            .function_runner
            .run_function(
                udf_type,
                udf_runtime,
                tx.identity().clone(),
                tx.begin_timestamp(),
                FunctionWrites {
                    updates: tx.writes().as_flat()?.coalesced_writes().cloned().collect(),
                },
                log_line_sender,
                function_metadata,
                http_action_metadata,
                self.default_system_env_vars.clone(),
                in_memory_index_last_modified,
                context,
            )
            .await?;
        timer.finish();
        drop(permit);

        // Add the usage stats to the current transaction tracker.
        tx.usage_tracker.add(usage_stats);

        // Apply the reads and writes to the current transaction
        let tx = if let Some(function_tx) = function_tx {
            let FunctionReads {
                reads,
                num_intervals,
                user_tx_size,
                system_tx_size,
            } = function_tx.reads;
            let FunctionWrites { updates } = function_tx.writes;
            tx.apply_function_runner_tx(
                function_tx.begin_timestamp,
                reads,
                num_intervals,
                user_tx_size,
                system_tx_size,
                updates,
                function_tx.rows_read_by_tablet,
                function_tx.uploaded_files,
            )?;
            Some(tx)
        } else {
            None
        };

        Ok((tx, outcome))
    }
}

#[derive(Clone, Copy)]
pub enum PermitTimeout {
    Enforce,
    WaitIndefinitely,
}

// Used to limit upstream concurrency for a given function type. It also tracks
// and log gauges for the number of waiting and currently running functions.
struct Limiter<RT: Runtime> {
    udf_type: UdfType,
    env: ModuleEnvironment,

    // Used to limit running functions.
    semaphore: Semaphore,
    total_permits: usize,

    // How long to wait for a permit before rejecting the request.
    semaphore_timeout: Duration,

    // Total function requests, including ones still waiting on the semaphore.
    total_outstanding: AtomicUsize,

    // Function execution log for recording outstanding function gauges.
    function_log: FunctionExecutionLog<RT>,
}

impl<RT: Runtime> Limiter<RT> {
    fn new(
        env: ModuleEnvironment,
        udf_type: UdfType,
        total_permits: usize,
        semaphore_timeout: Duration,
        function_log: FunctionExecutionLog<RT>,
    ) -> Self {
        let limiter = Self {
            udf_type,
            env,
            semaphore: Semaphore::new(total_permits),
            total_permits,
            semaphore_timeout,
            total_outstanding: AtomicUsize::new(0),
            function_log,
        };
        // Report metrics on startup.
        limiter.report_metrics();
        limiter
    }

    #[fastrace::trace]
    async fn acquire_permit<'a>(
        &'a self,
        rt: &'a RT,
        timeout: PermitTimeout,
    ) -> anyhow::Result<RequestGuard<'a, RT>> {
        let mut request_guard = self.start();
        select! {
            biased;
            result = request_guard.acquire_permit() => result?,
            x = async {
                // Report metrics while waiting for a permit. This captures the
                // queued state only when the semaphore is actually full.
                self.report_metrics();
                future::pending::<!>().await
            } => match x {},
            _ = async {
                match timeout {
                    PermitTimeout::Enforce => rt.wait(self.semaphore_timeout).await,
                    PermitTimeout::WaitIndefinitely => future::pending().await,
                }
            } => {
                log_function_wait_timeout(self.env, self.udf_type);
                let functions = match self.udf_type {
                    UdfType::Query => "queries",
                    UdfType::Mutation => "mutations",
                    UdfType::Action => "actions",
                    UdfType::HttpAction => "HTTP actions",
                };
                anyhow::bail!(ErrorMetadata::rate_limited(
                    "TooManyConcurrentRequests",
                    format!(
                        "Too many concurrent requests. Your backend is limited to {} concurrent {functions}. {}",
                        self.total_permits,
                        if self.total_permits > DEFAULT_APPLICATION_MAX_FUNCTION_CONCURRENCY {
                            "If you need more resources, please contact support."
                        } else {
                            "To get more resources, upgrade to Convex Pro. If you are already on Convex Pro, please contact support."
                        }
                    ),
                ));
            },
        }
        Ok(request_guard)
    }

    fn start(&self) -> RequestGuard<'_, RT> {
        self.total_outstanding.fetch_add(1, Ordering::SeqCst);
        RequestGuard {
            limiter: self,
            permit: None,
        }
    }

    // Permanently remove all permits so the next acquisition hits the concurrency
    // limit, simulating a saturated backend. Use `add_permits` to make capacity
    // available again. Test-only.
    // Reports metrics for the current waiting and running function gauges.
    fn report_metrics(&self) {
        let num_running_functions = self.total_permits - self.semaphore.available_permits();
        let num_queued_functions = if self.semaphore.available_permits() > 0 {
            0
        } else {
            self.total_outstanding
                .load(Ordering::SeqCst)
                .saturating_sub(num_running_functions)
        };

        // Log to prometheus
        log_outstanding_functions(
            num_running_functions,
            self.env,
            self.udf_type,
            OutstandingFunctionState::Running,
        );
        log_outstanding_functions(
            num_queued_functions,
            self.env,
            self.udf_type,
            OutstandingFunctionState::Queued,
        );

        // Log to udf_metrics as gauges
        self.function_log.log_outstanding_functions(
            num_running_functions,
            self.env,
            self.udf_type,
            OutstandingFunctionState::Running,
        );

        self.function_log.log_outstanding_functions(
            num_queued_functions,
            self.env,
            self.udf_type,
            OutstandingFunctionState::Queued,
        );
    }
}

// Wraps a request to guarantee we correctly update the waiting and running
// gauges even if dropped.
pub struct RequestGuard<'a, RT: Runtime> {
    limiter: &'a Limiter<RT>,
    permit: Option<SemaphorePermit<'a>>,
}

impl<RT: Runtime> RequestGuard<'_, RT> {
    async fn acquire_permit(&mut self) -> anyhow::Result<()> {
        let timer = function_waiter_timer(self.limiter.udf_type);
        assert!(
            self.permit.is_none(),
            "Called `acquire_permit` more than once"
        );
        self.permit = Some(self.limiter.semaphore.acquire().await?);
        timer.finish();
        // Report metrics to account for the newly running function.
        self.limiter.report_metrics();
        Ok(())
    }
}

impl<RT: Runtime> Drop for RequestGuard<'_, RT> {
    fn drop(&mut self) {
        // Drop the semaphore permit before reporting metrics.
        drop(self.permit.take());
        // Remove the request from the running ones.
        self.limiter
            .total_outstanding
            .fetch_sub(1, Ordering::SeqCst);
        // Report metrics to account for the newly finished request.
        self.limiter.report_metrics();
    }
}

/// Executes UDFs for backends.
///
/// This struct directly executes http and node actions. Queries, Mutations and
/// v8 Actions are instead routed through the FunctionRouter and its
/// FunctionRunner implementation.
pub struct ApplicationFunctionRunner<RT: Runtime> {
    runtime: RT,
    pub(crate) database: Database<RT>,

    key_broker: KeyBroker,

    isolate_functions: FunctionRouter<RT>,
    // Used for analyze, schema, etc.
    node_actions: NodeActions<RT>,

    source_map_cache: SourceMapCache<RT>,
    pub(crate) modules_storage: Arc<dyn Storage>,
    file_storage: TransactionalFileStorage<RT>,

    function_log: FunctionExecutionLog<RT>,
    audit_log_client: AuditLogClient,

    cache_manager: CacheManager<RT>,
    default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
    node_action_limiter: Limiter<RT>,
    ai_gateway_jwt_minter: Option<Arc<dyn AiGatewayJwtMinter>>,
    deployment: DeploymentMetadata,
}

impl<RT: Runtime> ApplicationFunctionRunner<RT> {
    pub fn new(
        runtime: RT,
        database: Database<RT>,
        key_broker: KeyBroker,
        function_runner: Arc<dyn FunctionRunner<RT>>,
        node_actions: NodeActions<RT>,
        file_storage: TransactionalFileStorage<RT>,
        modules_storage: Arc<dyn Storage>,
        source_map_cache: SourceMapCache<RT>,
        function_log: FunctionExecutionLog<RT>,
        audit_log_client: AuditLogClient,
        default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
        cache: QueryCache,
        ai_gateway_jwt_minter: Option<Arc<dyn AiGatewayJwtMinter>>,
        deployment: DeploymentMetadata,
    ) -> Self {
        let isolate_functions = FunctionRouter::new(
            function_runner,
            runtime.clone(),
            database.clone(),
            default_system_env_vars.clone(),
            function_log.clone(),
        );
        let cache_manager = CacheManager::new(
            runtime.clone(),
            database.clone(),
            isolate_functions.clone(),
            function_log.clone(),
            audit_log_client.clone(),
            cache,
        );

        let node_action_limiter = Limiter::new(
            ModuleEnvironment::Node,
            UdfType::Action,
            *APPLICATION_MAX_CONCURRENT_NODE_ACTIONS,
            *APPLICATION_FUNCTION_RUNNER_ACTION_SEMAPHORE_TIMEOUT,
            function_log.clone(),
        );

        Self {
            runtime,
            database,
            key_broker,
            isolate_functions,
            node_actions,
            source_map_cache,
            modules_storage,
            file_storage,
            function_log,
            audit_log_client,
            cache_manager,
            default_system_env_vars,
            node_action_limiter,
            ai_gateway_jwt_minter,
            deployment,
        }
    }

    /// Called with a typed caller via [`ActionCallbacks`], or with flat
    /// claims from Conductor's gRPC handler.
    pub async fn mint_ai_gateway_jwt(
        &self,
        identity: &Identity,
        attribution: AttributionClaims,
        request_id: &RequestId,
    ) -> anyhow::Result<String> {
        match identity {
            // Deploy keys and dashboard members carry an op set, from the key's
            // `allowed_operations` or the member's custom roles, so hold them to
            // it.
            Identity::DeploymentAdmin(_) | Identity::ActingUser(..) => {
                identity.require_operation(DeploymentOp::UseAiGateway)?
            },
            // End users authenticate against the app's own auth config and have
            // no op set. Gating them would reject the ordinary case of an app
            // user triggering an action that calls the gateway.
            Identity::System(_) | Identity::User(_) | Identity::Unknown(_) => {},
        }
        let mut tx = self.database.begin_system().await?;
        self.bail_if_backend_not_running(&mut tx).await?;
        let backend_info = BackendInfoModel::new(&mut tx).get().await?;
        let ai_gateway_disabled = backend_info
            .as_ref()
            .and_then(|backend_info| backend_info.ai_gateway_disabled)
            .unwrap_or(false);
        if ai_gateway_disabled {
            anyhow::bail!(ErrorMetadata::forbidden(
                "AiGatewayDisabled",
                "The Convex AI gateway is not enabled for your team. Upgrade to a paid plan to \
                 enable it, or contact support@convex.dev if you believe this is an error."
            ));
        }
        let Some(minter) = self.ai_gateway_jwt_minter.as_ref() else {
            // `bad_request` shows this message to the developer; a bare
            // anyhow error would show a generic internal error instead.
            anyhow::bail!(ErrorMetadata::bad_request(
                "AiGatewayUnavailable",
                "`getServiceToken(\"ai-gateway\")` isn't available on this deployment because the \
                 AI gateway is a Convex Cloud service. Deploy to Convex Cloud, or call your model \
                 provider directly with your own API key."
            ));
        };
        let team_id = backend_info.as_ref().map(|backend_info| backend_info.team);
        let project_id = backend_info
            .as_ref()
            .map(|backend_info| backend_info.project);
        minter
            .mint(
                &self.deployment,
                team_id,
                project_id,
                attribution,
                request_id,
            )
            .await
    }

    pub(crate) async fn shutdown(&self) -> anyhow::Result<()> {
        self.node_actions.shutdown().await?;
        Ok(())
    }

    // Only used for running queries from REPLs.
    pub async fn run_query_without_caching(
        &self,
        request_context: RequestContext,
        mut tx: Transaction<RT>,
        path: CanonicalizedComponentFunctionPath,
        arguments: SerializedArgs,
        caller: FunctionCaller,
    ) -> anyhow::Result<(Result<JsonPackedValue, JsError>, LogLines)> {
        if !(tx.identity().is_admin() || tx.identity().is_system()) {
            anyhow::bail!(unauthorized_error("query_without_caching"));
        }

        let identity = tx.inert_identity();
        let start = self.runtime.monotonic_now();
        let validate_result = ValidatedPathAndArgs::new(
            caller.allowed_visibility(),
            &mut tx,
            PublicFunctionPath::Component(path.clone()),
            arguments.clone(),
            UdfType::Query,
        )
        .await?;
        let context = ExecutionContext::new(request_context, &caller);
        let (mut tx, outcome) = match validate_result {
            Ok(path_and_args) => {
                // Preserve the caller's uncommitted module writes in this transaction.
                let permit = self
                    .isolate_functions
                    .acquire_permit(UdfType::Query)
                    .await?;
                self.isolate_functions
                    .execute_query_or_mutation(
                        tx,
                        permit,
                        path_and_args,
                        UdfType::Query,
                        QueryJournal::new(),
                        context.clone(),
                    )
                    .await?
            },
            Err(js_err) => {
                let query_outcome = UdfOutcome::from_error(
                    js_err,
                    path.clone(),
                    arguments.clone(),
                    identity.clone(),
                    self.runtime.clone(),
                    None,
                )?;
                (tx, FunctionOutcome::Query(query_outcome))
            },
        };
        let outcome = match outcome {
            FunctionOutcome::Query(o) => o,
            _ => anyhow::bail!("Received non-query outcome for query"),
        };

        let vars = AuditLogVars::from_context(
            context.clone(),
            tx.identity().convex_actor_var(),
            &self.runtime,
        )?;
        self.audit_log_client
            .send_logs(
                outcome.audit_log_lines.resolve_bodies(&vars)?,
                &tx.usage_tracker,
            )
            .await?;

        let stats = tx.take_stats();

        // A top-level query's result never contains unresolved commit
        // timestamps.
        let result = match outcome.result.clone() {
            Ok(value) => Ok(value.try_into()?),
            Err(e) => Err(e),
        };
        let log_lines = outcome.log_lines.clone();
        self.function_log
            .log_query(
                &outcome,
                stats,
                false,
                start.elapsed(),
                caller,
                tx.usage_tracker,
                context.clone(),
                QueryInvocation::Fresh,
            )
            .await;
        Ok((result, log_lines))
    }

    /// Runs a mutations and retries on OCC errors.
    #[fastrace::trace]
    pub async fn retry_mutation(
        &self,
        request_context: RequestContext,
        path: PublicFunctionPath,
        arguments: SerializedArgs,
        identity: Identity,
        mutation_identifier: Option<SessionRequestIdentifier>,
        caller: FunctionCaller,
        mutation_queue_length: Option<usize>,
    ) -> anyhow::Result<Result<MutationReturn, MutationError>> {
        let timer = mutation_timer();
        let result = self
            ._retry_mutation(
                request_context,
                path,
                arguments,
                identity,
                mutation_identifier,
                caller,
                mutation_queue_length,
            )
            .await;
        match &result {
            Ok(_) => timer.finish(),
            Err(e) => timer.finish_with(e.metric_status_label_value()),
        };
        result
    }

    /// Runs a mutations and retries on OCC errors.
    #[fastrace::trace]
    async fn _retry_mutation(
        &self,
        request_context: RequestContext,
        path: PublicFunctionPath,
        arguments: SerializedArgs,
        identity: Identity,
        mutation_identifier: Option<SessionRequestIdentifier>,
        caller: FunctionCaller,
        mutation_queue_length: Option<usize>,
    ) -> anyhow::Result<Result<MutationReturn, MutationError>> {
        if path.is_system() && !(identity.is_admin() || identity.is_system()) {
            anyhow::bail!(unauthorized_error("mutation"));
        }
        let write_source = WriteSource::mutation(path.clone().debug_into_component_path());

        let mut backoff = Backoff::new(
            *UDF_EXECUTOR_OCC_INITIAL_BACKOFF,
            *UDF_EXECUTOR_OCC_MAX_BACKOFF,
        );
        let mut file_storage_backoff = Backoff::new(
            *UDF_FILE_STORAGE_INITIAL_BACKOFF,
            *UDF_FILE_STORAGE_MAX_BACKOFF,
        );

        loop {
            let occ_retry_count = backoff.failures() as usize;
            let mutation_retry_count = occ_retry_count + file_storage_backoff.failures() as usize;
            let usage_tracker = FunctionUsageTracker::new();

            // Note that we use different context for every mutation attempt.
            // This so every JS function run gets a different executionId.
            let context = ExecutionContext::new(request_context.clone(), &caller);

            let start = self.runtime.monotonic_now();
            let permit = self.acquire_mutation_permit().await;
            let mut tx = self
                .database
                .begin_with_usage(identity.clone(), usage_tracker.clone())
                .await?;
            let pause_client = self.runtime.pause_client();
            pause_client.wait("retry_mutation_loop_start").await;
            let identity = tx.inert_identity();

            // Committed mutations can be replayed even if permit acquisition failed.
            if let Some(result) = self
                .check_mutation_status(&mut tx, &mutation_identifier)
                .await?
            {
                return Ok(result);
            }

            let result: Result<(Transaction<RT>, ValidatedUdfOutcome), anyhow::Error> = self
                .run_mutation_no_udf_log(
                    tx,
                    permit,
                    path.clone(),
                    arguments.clone(),
                    caller.allowed_visibility(),
                    context.clone(),
                    mutation_queue_length,
                )
                .await;
            let (mut tx, mut outcome) = match result {
                Ok(r) => r,
                Err(e) => {
                    if e.short_msg() == "TooManyWrites"
                        && (backoff.failures() as usize) < *UDF_EXECUTOR_OCC_MAX_RETRIES
                    {
                        let sleep = backoff.fail(&mut self.runtime.rng());
                        tracing::warn!(
                            "Write throughput limit exceeded, retrying {write_source:?} after \
                             {sleep:?}",
                        );
                        self.runtime.wait(sleep).await;
                        continue;
                    }
                    if matches!(
                        e.short_msg(),
                        FILE_UPLOAD_FAILED_SHORT_MSG | FILE_DOWNLOAD_FAILED_SHORT_MSG
                    ) && (file_storage_backoff.failures() as usize)
                        < *UDF_FILE_STORAGE_MAX_RETRIES
                    {
                        let sleep = file_storage_backoff.fail(&mut self.runtime.rng());
                        tracing::warn!(
                            "File storage failed ({e:#}), retrying {write_source:?} after \
                             {sleep:?}",
                        );
                        self.runtime.wait(sleep).await;
                        continue;
                    }
                    self.function_log
                        .log_mutation_system_error(
                            &e,
                            path.debug_into_component_path(),
                            arguments,
                            identity,
                            start,
                            caller,
                            context.clone(),
                            mutation_queue_length,
                            mutation_retry_count,
                        )
                        .await?;
                    return Err(e);
                },
            };

            // Save a CommittedMutation object so we won't rerun this mutation if
            // successful.
            self.write_mutation_status(&mut tx, &mutation_identifier, &outcome)
                .await?;

            let stats = tx.take_stats();
            let execution_time = start.elapsed();
            let log_lines = outcome.log_lines.clone();
            let value = match outcome.result {
                Ok(ref value) => value.clone(),
                // If it's an error inside the UDF, log the failed execution and return the
                // developer error.
                Err(ref error) => {
                    drop(tx);
                    self.function_log
                        .log_mutation(
                            outcome.clone(),
                            stats,
                            execution_time,
                            caller,
                            usage_tracker,
                            context.clone(),
                            mutation_queue_length,
                            mutation_retry_count,
                        )
                        .await;
                    return Ok(Err(MutationError {
                        error: error.to_owned(),
                        log_lines,
                    }));
                },
            };

            // Attempt to commit the transaction and log an error if commit failed,
            // even if it was an OCC error. We may decide later to suppress OCC
            // errors from the log.
            let result = match self
                .database
                .commit_with_write_source(tx, write_source.clone())
                .await
            {
                Ok(ts) => Ok(MutationReturn {
                    // The commit timestamp is known now, so unresolved commit
                    // timestamps in the return value resolve to it, matching
                    // the committer's resolution of the transaction's writes.
                    value: value.resolve_commit_ts(i64::from(ts))?,
                    log_lines,
                    ts,
                }),
                Err(e) => {
                    if e.is_deterministic_user_error() {
                        let js_error = JsError::from_error(e);
                        outcome.result = Err(js_error.clone());
                        Err(MutationError {
                            error: js_error,
                            log_lines,
                        })
                    } else {
                        if let Some(occ_info) = e.occ_info()
                            && (backoff.failures() as usize) < *UDF_EXECUTOR_OCC_MAX_RETRIES
                        {
                            let sleep = backoff.fail(&mut self.runtime.rng());
                            tracing::warn!(
                                "Optimistic concurrency control failed ({e}), retrying \
                                 {write_source:?} after {sleep:?}",
                            );
                            self.runtime.wait(sleep).await;
                            if let Some(write_ts_raw) = e.occ_write_ts()
                                && let Ok(write_ts) = Timestamp::try_from(write_ts_raw)
                            {
                                self.database.wait_for_write_ts(write_ts).await;
                            }
                            self.function_log
                                .log_mutation_occ_error(
                                    outcome,
                                    stats,
                                    execution_time,
                                    caller.clone(),
                                    usage_tracker,
                                    context.clone(),
                                    occ_info,
                                    mutation_queue_length,
                                    mutation_retry_count,
                                    occ_retry_count,
                                    true,
                                )
                                .await;
                            continue;
                        }
                        outcome.result = Err(JsError::from_error_ref(&e));

                        if let Some(occ_info) = e.occ_info() {
                            self.function_log
                                .log_mutation_occ_error(
                                    outcome,
                                    stats,
                                    execution_time,
                                    caller,
                                    usage_tracker,
                                    context.clone(),
                                    occ_info,
                                    mutation_queue_length,
                                    mutation_retry_count,
                                    occ_retry_count,
                                    false,
                                )
                                .await;
                        } else {
                            self.function_log
                                .log_mutation_system_error(
                                    &e,
                                    path.debug_into_component_path(),
                                    arguments,
                                    identity,
                                    start,
                                    caller,
                                    context,
                                    mutation_queue_length,
                                    mutation_retry_count,
                                )
                                .await?;
                        }
                        log_occ_retries(backoff.failures() as usize);
                        return Err(e);
                    }
                },
            };

            self.function_log
                .log_mutation(
                    outcome.clone(),
                    stats,
                    execution_time,
                    caller,
                    usage_tracker,
                    context.clone(),
                    mutation_queue_length,
                    mutation_retry_count,
                )
                .await;
            log_occ_retries(backoff.failures() as usize);
            return Ok(result);
        }
    }

    /// Ordinary mutations acquire before beginning their transaction to keep
    /// the snapshot fresh. Background jobs begin their transactions before
    /// waiting and rely on OCC retries for freshness.
    ///
    /// Fails with `TooManyWrites` instead of waiting while the write throughput
    /// limit is exceeded. The mutation checks the limit again after the wait.
    pub async fn acquire_mutation_permit(&self) -> anyhow::Result<RequestGuard<'_, RT>> {
        self.database.check_write_throughput_limit()?;
        self.isolate_functions
            .acquire_permit(UdfType::Mutation)
            .await
    }

    /// Attempts to run a mutation once using the given transaction.
    /// The caller handles retries and UDF logging.
    ///
    /// Accepts a permit acquisition error so it can be logged with other system
    /// errors.
    pub async fn run_mutation_no_udf_log(
        &self,
        tx: Transaction<RT>,
        permit: anyhow::Result<RequestGuard<'_, RT>>,
        path: PublicFunctionPath,
        arguments: SerializedArgs,
        allowed_visibility: AllowedVisibility,
        context: ExecutionContext,
        mutation_queue_length: Option<usize>,
    ) -> anyhow::Result<(Transaction<RT>, ValidatedUdfOutcome)> {
        let result = self
            .run_mutation_inner(
                tx,
                permit,
                path,
                arguments,
                allowed_visibility,
                context,
                mutation_queue_length,
            )
            .await;
        match result.as_ref() {
            Ok((_, udf_outcome)) => {
                let result = if udf_outcome.result.is_ok() {
                    UdfExecutorResult::Success
                } else {
                    UdfExecutorResult::UserError
                };
                log_udf_executor_result(UdfType::Mutation, result);
            },
            Err(e) => {
                log_udf_executor_result(
                    UdfType::Mutation,
                    UdfExecutorResult::SystemError(e.metric_status_label_value()),
                );
            },
        };
        result
    }

    /// Runs the mutation once without any logging.
    #[fastrace::trace]
    async fn run_mutation_inner(
        &self,
        mut tx: Transaction<RT>,
        permit: anyhow::Result<RequestGuard<'_, RT>>,
        path: PublicFunctionPath,
        arguments: SerializedArgs,
        allowed_visibility: AllowedVisibility,
        context: ExecutionContext,
        mutation_queue_length: Option<usize>,
    ) -> anyhow::Result<(Transaction<RT>, ValidatedUdfOutcome)> {
        let permit = permit?;
        self.database.check_write_throughput_limit()?;
        if path.is_system() && !(tx.identity().is_admin() || tx.identity().is_system()) {
            anyhow::bail!(unauthorized_error("mutation"));
        }
        let identity = tx.inert_identity();
        let validate_result = ValidatedPathAndArgs::new_with_returns_validator(
            allowed_visibility,
            &mut tx,
            path.clone(),
            arguments.clone(),
            UdfType::Mutation,
            PendingArgsPolicy::Reject,
        )
        .await?;

        let (path_and_args, returns_validator, _) = match validate_result {
            Ok(tuple) => tuple,
            Err(js_err) => {
                let mutation_outcome = ValidatedUdfOutcome::from_error(
                    js_err,
                    path.debug_into_component_path(),
                    arguments.clone(),
                    identity.clone(),
                    self.runtime.clone(),
                    None,
                )?;
                return Ok((tx, mutation_outcome));
            },
        };

        let path = path_and_args.path().clone();
        let (mut tx, outcome) = self
            .isolate_functions
            .execute_query_or_mutation(
                tx,
                permit,
                path_and_args,
                UdfType::Mutation,
                QueryJournal::new(),
                context.clone(),
            )
            .await?;
        let mutation_outcome = match outcome {
            FunctionOutcome::Mutation(o) => o,
            _ => anyhow::bail!("Received non-mutation outcome for mutation"),
        };

        let vars =
            AuditLogVars::from_context(context, tx.identity().convex_actor_var(), &self.runtime)?;
        self.audit_log_client
            .send_logs(
                mutation_outcome.audit_log_lines.resolve_bodies(&vars)?,
                &tx.usage_tracker,
            )
            .await?;

        let component = path.component;

        let table_mapping = tx.table_mapping().namespace(component.into());

        let outcome = ValidatedUdfOutcome::new(
            mutation_outcome,
            returns_validator,
            &table_mapping,
            mutation_queue_length,
        );

        Ok((tx, outcome))
    }

    #[fastrace::trace]
    pub async fn run_action(
        &self,
        request_context: RequestContext,
        path: PublicFunctionPath,
        arguments: SerializedArgs,
        identity: Identity,
        caller: FunctionCaller,
    ) -> anyhow::Result<Result<ActionReturn, ActionError>> {
        if path.is_system() && !(identity.is_admin() || identity.is_system()) {
            anyhow::bail!(unauthorized_error("action"));
        }
        let context = ExecutionContext::new(request_context, &caller);
        let usage_tracking = FunctionUsageTracker::new();
        let start = self.runtime.monotonic_now();
        let completion_result = self
            .run_action_no_udf_log(
                path.clone(),
                arguments.clone(),
                identity.clone(),
                caller.clone(),
                usage_tracking.clone(),
                context.clone(),
                PermitTimeout::Enforce,
            )
            .await;
        let completion = match completion_result {
            Ok(c) => c,
            Err(e) => {
                self.function_log
                    .log_action_system_error(
                        &e,
                        path.debug_into_component_path(),
                        arguments,
                        identity.into(),
                        start,
                        caller,
                        vec![].into(),
                        context,
                    )
                    .await?;
                anyhow::bail!(e)
            },
        };
        let log_lines = completion.log_lines().clone();
        let result = completion.outcome.result.clone();
        self.function_log
            .log_action(completion, usage_tracking)
            .await;

        let value = match result {
            Ok(value) => value,
            // If it's an error inside the UDF, log the failed execution and return the
            // developer error.
            Err(error) => return Ok(Err(ActionError { error, log_lines })),
        };

        Ok(Ok(ActionReturn { value, log_lines }))
    }

    /// Runs the actions without logging to the UDF log. It is the caller
    /// responsibility to log to the UDF log.
    #[fastrace::trace]
    pub async fn run_action_no_udf_log(
        &self,
        path: PublicFunctionPath,
        arguments: SerializedArgs,
        identity: Identity,
        caller: FunctionCaller,
        usage_tracking: FunctionUsageTracker,
        context: ExecutionContext,
        permit_timeout: PermitTimeout,
    ) -> anyhow::Result<ActionCompletion> {
        let result = self
            .run_action_inner(
                path,
                arguments,
                identity,
                caller,
                usage_tracking,
                context,
                permit_timeout,
            )
            .await;
        match result.as_ref() {
            Ok(completion) => {
                let result = if completion.outcome.result.is_ok() {
                    UdfExecutorResult::Success
                } else {
                    UdfExecutorResult::UserError
                };
                log_udf_executor_result(UdfType::Action, result);
            },
            Err(e) => {
                log_udf_executor_result(
                    UdfType::Action,
                    UdfExecutorResult::SystemError(e.metric_status_label_value()),
                );
            },
        };
        result
    }

    /// Runs the action without any logging.
    #[fastrace::trace]
    async fn run_action_inner(
        &self,
        path: PublicFunctionPath,
        arguments: SerializedArgs,
        identity: Identity,
        caller: FunctionCaller,
        usage_tracking: FunctionUsageTracker,
        context: ExecutionContext,
        permit_timeout: PermitTimeout,
    ) -> anyhow::Result<ActionCompletion> {
        if path.is_system() && !(identity.is_admin() || identity.is_system()) {
            anyhow::bail!(unauthorized_error("action"));
        }
        let unix_timestamp = self.runtime.unix_timestamp();
        let start = self.runtime.monotonic_now();
        let mut tx = self
            .database
            .begin_with_usage(identity.clone(), usage_tracking)
            .await?;
        let validate_result = ValidatedPathAndArgs::new_with_returns_validator(
            caller.allowed_visibility(),
            &mut tx,
            path.clone(),
            arguments.clone(),
            UdfType::Action,
            PendingArgsPolicy::Reject,
        )
        .await?;

        // Fetch the returns_validator now to be used at a later ts.
        let (path_and_args, returns_validator) = match validate_result {
            // We don't need to store visibility_info for non-queries.
            Ok((path_and_args, returns_validator, _visibility_info)) => {
                (path_and_args, returns_validator)
            },
            Err(js_error) => {
                return Ok(ActionCompletion {
                    outcome: ValidatedActionOutcome::from_error(
                        js_error,
                        path.debug_into_component_path(),
                        arguments,
                        identity.into(),
                        self.runtime.clone(),
                        None,
                    ),
                    environment: ModuleEnvironment::Invalid,
                    memory_in_mb: 0,
                    execution_time: Duration::from_secs(0),
                    context,
                    unix_timestamp,
                    caller,
                    log_lines: vec![].into(),
                });
            },
        };

        let component = path_and_args.path().component;

        // We should use table mappings from the same transaction as the output
        // validator was retrieved.
        let table_mapping = tx.table_mapping().namespace(component.into());
        let virtual_system_mapping = tx.virtual_system_mapping().clone();
        let udf_server_version = path_and_args.npm_version().clone();
        // We should not be missing the module given we validated the path above
        // which requires the module to exist.
        let path = path_and_args.path().clone();
        let module = ModuleModel::new(&mut tx)
            .get_metadata_for_function_by_id(&path)
            .await?
            .context("Missing a valid module")?;
        let (log_line_sender, log_line_receiver) = mpsc::unbounded_channel();

        let inert_identity = tx.inert_identity();
        let timer = function_total_timer(module.environment, UdfType::Action);
        let completion_result = match module.environment {
            ModuleEnvironment::Isolate => {
                let outcome_future = self
                    .isolate_functions
                    .execute_action(
                        tx,
                        path_and_args,
                        log_line_sender,
                        context.clone(),
                        permit_timeout,
                    )
                    .boxed();
                let (outcome_result, log_lines) = run_function_and_collect_log_lines(
                    outcome_future,
                    log_line_receiver,
                    |log_line| {
                        self.function_log.log_action_progress(
                            path.clone().for_logging(),
                            unix_timestamp,
                            context.clone(),
                            vec![log_line].into(),
                            module.environment,
                        )
                    },
                )
                .await;

                let memory_in_mb: u64 = (*ISOLATE_MAX_USER_HEAP_SIZE / (1 << 20))
                    .try_into()
                    .unwrap();

                let validated_outcome_result = outcome_result.map(|outcome| {
                    ValidatedActionOutcome::new(outcome, returns_validator, &table_mapping)
                });

                timer.finish();
                validated_outcome_result.map(|outcome| ActionCompletion {
                    outcome,
                    execution_time: start.elapsed(),
                    environment: ModuleEnvironment::Isolate,
                    memory_in_mb,
                    context: context.clone(),
                    unix_timestamp,
                    caller: caller.clone(),
                    log_lines,
                })
            },
            ModuleEnvironment::Node => {
                let source_package = SourcePackageModel::new(&mut tx, path.component.into())
                    .get_latest()
                    .await?
                    .context("no source package?")?;
                let source_maps_callback = async {
                    let mut source_maps = BTreeMap::new();
                    if let Some(source_map) = self
                        .source_map_cache
                        .get_source_map(
                            &self.deployment.name,
                            &self.modules_storage,
                            &module,
                            &source_package,
                        )
                        .await?
                    {
                        source_maps.insert(path.udf_path.module().clone(), source_map);
                    }
                    Ok(source_maps)
                };
                let _permit = self
                    .node_action_limiter
                    .acquire_permit(&self.runtime, permit_timeout)
                    .await?;

                let mut environment_variables =
                    system_env_vars(&mut tx, self.default_system_env_vars.clone()).await?;
                let user_environment_variables =
                    EnvironmentVariablesModel::new(&mut tx).get_all().await?;
                environment_variables.extend(user_environment_variables);

                // Fetch source and external_deps presigned URI first
                let source_uri_future = self
                    .modules_storage
                    .signed_url(source_package.storage_key.clone(), Duration::from_secs(60));
                let (source_uri, external_deps_package) = if let Some(external_deps_package_id) =
                    &source_package.external_deps_package_id
                {
                    let pkg = ExternalPackagesModel::new(&mut tx)
                        .get(external_deps_package_id.clone())
                        .await?
                        .into_value();
                    let external_uri_future = self
                        .modules_storage
                        .signed_url(pkg.storage_key.clone(), Duration::from_secs(60));

                    let (source_uri, external_deps_uri) =
                        tokio::try_join!(source_uri_future, external_uri_future)?;
                    (
                        source_uri,
                        Some(node_executor::Package {
                            uri: external_deps_uri,
                            key: pkg.storage_key,
                            sha256: pkg.sha256,
                        }),
                    )
                } else {
                    (source_uri_future.await?, None)
                };

                let udf_server_version = path_and_args.npm_version().clone();
                let request = ExecuteRequest {
                    path_and_args,
                    source_package: node_executor::SourcePackage {
                        bundled_source: node_executor::Package {
                            uri: source_uri,
                            key: source_package.storage_key.clone(),
                            sha256: source_package.sha256.clone(),
                        },
                        external_deps: external_deps_package,
                    },
                    source_package_id: source_package.developer_id().into(),
                    user_identity: tx.user_identity(),
                    auth_header: token_to_authorization_header(tx.authentication_token())?,
                    environment_variables,
                    callback_token: self.key_broker.issue_action_token(path.component),
                    context: context.clone(),
                    encoded_parent_trace: EncodedSpan::from_parent().0,
                };

                let node_outcome_future = self
                    .node_actions
                    .execute(request, log_line_sender, source_maps_callback)
                    .boxed();
                let (mut node_outcome_result, log_lines) = run_function_and_collect_log_lines(
                    node_outcome_future,
                    log_line_receiver,
                    |log_line| {
                        self.function_log.log_action_progress(
                            path.clone().for_logging(),
                            unix_timestamp,
                            context.clone(),
                            vec![log_line].into(),
                            module.environment,
                        )
                    },
                )
                .await;

                timer.finish();

                if let Ok(ref mut node_outcome) = node_outcome_result
                    && let Ok(ref output) = node_outcome.result
                    && let Some(js_err) = returns_validator.check_output(
                        output,
                        &table_mapping,
                        &virtual_system_mapping,
                    )
                {
                    node_outcome.result = Err(js_err);
                }

                node_outcome_result.map(|node_outcome| {
                    let outcome = ActionOutcome {
                        path: path.clone().for_logging(),
                        arguments: arguments.clone(),
                        identity: tx.inert_identity(),
                        unix_timestamp,
                        result: node_outcome.result.map(JsonPackedValue::pack),
                        syscall_trace: node_outcome.syscall_trace,
                        udf_server_version,
                        user_execution_time: None,
                    };
                    let outcome =
                        ValidatedActionOutcome::new(outcome, returns_validator, &table_mapping);

                    // Track all node actions egress under a single "url". If users want better
                    // breakdowns, they have to use v8 actions.
                    tx.usage_tracker
                        .track_fetch_egress("node_actions".to_string(), node_outcome.egress_bytes);

                    ActionCompletion {
                        outcome,
                        execution_time: start.elapsed(),
                        environment: ModuleEnvironment::Node,
                        memory_in_mb: node_outcome.memory_used_in_mb,
                        context: context.clone(),
                        unix_timestamp,
                        caller: caller.clone(),
                        log_lines,
                    }
                })
            },
            ModuleEnvironment::Invalid => {
                Err(anyhow::anyhow!("Attempting to run an invalid function"))
            },
        };
        match completion_result {
            Ok(c) => Ok(c),
            Err(e) if e.is_deterministic_user_error() => {
                let outcome = ValidatedActionOutcome::from_error(
                    JsError::from_error(e),
                    path.for_logging(),
                    arguments,
                    inert_identity,
                    self.runtime.clone(),
                    udf_server_version,
                );
                Ok(ActionCompletion {
                    outcome,
                    execution_time: start.elapsed(),
                    environment: module.environment,
                    memory_in_mb: match module.environment {
                        ModuleEnvironment::Isolate => (*ISOLATE_MAX_USER_HEAP_SIZE / (1 << 20))
                            .try_into()
                            .unwrap(),
                        // This isn't correct but we don't have a value to use here.
                        ModuleEnvironment::Node => 0,
                        ModuleEnvironment::Invalid => 0,
                    },
                    context,
                    unix_timestamp,
                    caller,
                    log_lines: vec![].into(),
                })
            },
            Err(e) => Err(e),
        }
    }

    // Drain the isolate concurrency permits for `udf_type` so tests can
    // deterministically force `TooManyConcurrentRequests` and then recover.
    #[fastrace::trace]
    pub async fn build_deps(
        &self,
        deps: Vec<NodeDependency>,
    ) -> anyhow::Result<Result<ExternalDepsPackage, JsError>> {
        let (object_key, upload_url) = self
            .modules_storage
            .presigned_upload_url(*BUILD_DEPS_TIMEOUT)
            .await?;
        let request = BuildDepsRequest {
            deps: deps.clone(),
            upload_url,
        };
        let build_deps_res = self.node_actions.build_deps(request).await?;
        Ok(
            build_deps_res.map(move |(digest, package_size)| ExternalDepsPackage {
                storage_key: object_key,
                sha256: digest,
                deps,
                package_size,
            }),
        )
    }

    #[fastrace::trace]
    pub async fn evaluate_app_definitions(
        &self,
        app_definition: ModuleConfig,
        component_definitions: BTreeMap<ComponentDefinitionPath, ModuleConfig>,
        dependency_graph: BTreeSet<(ComponentDefinitionPath, ComponentDefinitionPath)>,
        user_environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        system_env_var_overrides: BTreeMap<EnvVarName, EnvVarValue>,
    ) -> anyhow::Result<EvaluateAppDefinitionsResult> {
        let mut system_env_vars = self.default_system_env_vars.clone();
        system_env_vars.extend(system_env_var_overrides);
        self.isolate_functions
            .function_runner
            .evaluate_app_definitions(
                app_definition,
                component_definitions,
                dependency_graph,
                user_environment_variables,
                system_env_vars,
            )
            .await
    }

    #[fastrace::trace]
    pub async fn evaluate_component_initializer(
        &self,
        evaluated_definitions: BTreeMap<ComponentDefinitionPath, ComponentDefinitionMetadata>,
        path: ComponentDefinitionPath,
        definition: ModuleConfig,
        args: BTreeMap<Identifier, Resource>,
        name: ComponentName,
    ) -> anyhow::Result<BTreeMap<Identifier, Resource>> {
        self.isolate_functions
            .function_runner
            .evaluate_component_initializer(evaluated_definitions, path, definition, args, name)
            .await
    }

    #[fastrace::trace]
    pub async fn analyze(
        &self,
        udf_config: UdfConfig,
        new_modules: Vec<ModuleConfig>,
        source_package: SourcePackage,
        user_environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        system_env_var_overrides: BTreeMap<EnvVarName, EnvVarValue>,
    ) -> anyhow::Result<Result<BTreeMap<CanonicalizedModulePath, AnalyzedModule>, JsError>> {
        let mut environment_variables = self.default_system_env_vars.clone();
        environment_variables.extend(system_env_var_overrides);
        environment_variables.extend(user_environment_variables);

        let (node_modules, isolate_modules) = new_modules
            .into_iter()
            .map(|module| (module.path.clone().canonicalize(), module))
            .partition(|(_, config)| config.environment == ModuleEnvironment::Node);

        let mut result = BTreeMap::new();

        let isolate_future = self.isolate_functions.function_runner.analyze(
            udf_config,
            isolate_modules,
            environment_variables.clone(),
        );

        let node_future = async {
            if node_modules.is_empty() {
                return Ok(Ok(BTreeMap::new()));
            }
            for path_str in ["schema.js", "crons.js", "http.js"] {
                let path = path_str
                    .parse()
                    .expect("Failed to parse static module names");
                // The cli should not do this. Log as system error.
                anyhow::ensure!(
                    !node_modules.contains_key(&path),
                    "{path_str} can't be analyzed in Node.js!"
                );
            }
            let mut source_maps = BTreeMap::new();
            for (path, module) in node_modules.iter() {
                if let Some(source_map) = module.source_map.clone() {
                    source_maps.insert(path.clone(), source_map);
                }
            }
            // Fetch source and external_deps presigned URI first
            let source_uri_future = self
                .modules_storage
                .signed_url(source_package.storage_key.clone(), Duration::from_secs(60));
            let mut tx = self.database.begin_system().await?;
            let (source_uri, external_deps_package) =
                if let Some(external_deps_package_id) = source_package.external_deps_package_id {
                    let pkg = ExternalPackagesModel::new(&mut tx)
                        .get(external_deps_package_id)
                        .await?
                        .into_value();
                    let external_uri_future = self
                        .modules_storage
                        .signed_url(pkg.storage_key.clone(), Duration::from_secs(60));

                    let (source_uri, external_deps_uri) =
                        tokio::try_join!(source_uri_future, external_uri_future)?;
                    (
                        source_uri,
                        Some(node_executor::Package {
                            uri: external_deps_uri,
                            key: pkg.storage_key,
                            sha256: pkg.sha256,
                        }),
                    )
                } else {
                    (source_uri_future.await?, None)
                };

            let request = AnalyzeRequest {
                source_package: node_executor::SourcePackage {
                    bundled_source: node_executor::Package {
                        uri: source_uri,
                        key: source_package.storage_key,
                        sha256: source_package.sha256,
                    },
                    external_deps: external_deps_package,
                },
                environment_variables,
            };
            self.node_actions.analyze(request, &source_maps).await
        };

        let (isolate_result, node_result) = tokio::try_join!(isolate_future, node_future)?;
        match isolate_result {
            Ok(modules) => result.extend(modules),
            Err(e) => return Ok(Err(e)),
        }
        match node_result {
            Ok(modules) => {
                for (path, analyzed_module) in modules {
                    let exists = result.insert(path, analyzed_module).is_some();
                    // Note that although we send all modules to actions.analyze, it
                    // currently ignores isolate modules.
                    anyhow::ensure!(!exists, "actions.analyze returned isolate modules");
                }
            },
            Err(e) => return Ok(Err(e)),
        }

        self.validate_cron_jobs(&result)??;

        Ok(Ok(result))
    }

    #[fastrace::trace]
    fn validate_cron_jobs(
        &self,
        modules: &BTreeMap<CanonicalizedModulePath, AnalyzedModule>,
    ) -> anyhow::Result<Result<(), JsError>> {
        // Validate that every cron job schedules an action or mutation.
        for module in modules.values() {
            let Some(crons) = module.cron_specs.as_ref() else {
                continue;
            };
            for (identifier, cron_spec) in crons {
                let path = cron_spec.udf_path.module().clone();
                let Some(scheduled_module) = modules.get(&path) else {
                    return Ok(Err(JsError::from_message(format!(
                        "The cron job '{identifier}' schedules a function that does not exist: {}",
                        cron_spec.udf_path
                    ))));
                };
                let name = cron_spec.udf_path.function_name();
                let Some(scheduled_function) =
                    scheduled_module.functions.iter().find(|f| &f.name == name)
                else {
                    return Ok(Err(JsError::from_message(format!(
                        "The cron job '{identifier}' schedules a function that does not exist: {}",
                        cron_spec.udf_path
                    ))));
                };
                match scheduled_function.udf_type {
                    UdfType::Query => {
                        return Ok(Err(JsError::from_message(format!(
                            "The cron job '{identifier}' schedules a query function, only actions \
                             and mutations can be scheduled: {}",
                            cron_spec.udf_path
                        ))));
                    },
                    UdfType::HttpAction => {
                        return Ok(Err(JsError::from_message(format!(
                            "The cron job '{identifier}' schedules an HTTP action, only actions \
                             and mutations can be scheduled: {}",
                            cron_spec.udf_path
                        ))));
                    },
                    UdfType::Mutation => {},
                    UdfType::Action => {},
                }
            }
        }
        Ok(Ok(()))
    }

    #[fastrace::trace]
    pub async fn evaluate_schema(
        &self,
        schema_bundle: ModuleSource,
        source_map: Option<SourceMap>,
        rng_seed: [u8; 32],
        unix_timestamp: UnixTimestamp,
    ) -> anyhow::Result<DatabaseSchema> {
        self.isolate_functions
            .function_runner
            .evaluate_schema(schema_bundle, source_map, rng_seed, unix_timestamp)
            .await
    }

    pub async fn evaluate_auth_config(
        &self,
        auth_config_bundle: ModuleSource,
        source_map: Option<SourceMap>,
        user_environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        system_env_var_overrides: BTreeMap<EnvVarName, EnvVarValue>,
        explanation: &str,
    ) -> anyhow::Result<AuthConfig> {
        let mut environment_variables = self.default_system_env_vars.clone();
        environment_variables.extend(system_env_var_overrides);
        environment_variables.extend(user_environment_variables);
        self.isolate_functions
            .function_runner
            .evaluate_auth_config(
                auth_config_bundle,
                source_map,
                environment_variables,
                explanation,
            )
            .await
    }

    pub fn enable_actions(&self) -> anyhow::Result<()> {
        self.node_actions.enable()
    }

    #[fastrace::trace]
    pub async fn run_query_at_ts(
        &self,
        request_context: RequestContext,
        path: PublicFunctionPath,
        args: SerializedArgs,
        identity: Identity,
        ts: Timestamp,
        journal: Option<QueryJournal>,
        caller: FunctionCaller,
        invocation: QueryInvocation,
    ) -> anyhow::Result<QueryReturn> {
        let result = self
            .run_query_at_ts_inner(
                request_context,
                path,
                args,
                identity,
                ts,
                journal,
                caller,
                invocation,
            )
            .await;
        match result.as_ref() {
            Ok(udf_outcome) => {
                let result = if udf_outcome.result.is_ok() {
                    UdfExecutorResult::Success
                } else {
                    UdfExecutorResult::UserError
                };
                log_udf_executor_result(UdfType::Query, result);
            },
            Err(e) => {
                log_udf_executor_result(
                    UdfType::Query,
                    UdfExecutorResult::SystemError(e.metric_status_label_value()),
                );
            },
        };
        result
    }

    #[fastrace::trace]
    async fn run_query_at_ts_inner(
        &self,
        request_context: RequestContext,
        path: PublicFunctionPath,
        args: SerializedArgs,
        identity: Identity,
        ts: Timestamp,
        journal: Option<QueryJournal>,
        caller: FunctionCaller,
        invocation: QueryInvocation,
    ) -> anyhow::Result<QueryReturn> {
        if path.is_system() && !(identity.is_admin() || identity.is_system()) {
            anyhow::bail!(unauthorized_error("query"));
        }
        let start = self.runtime.monotonic_now();
        let context = ExecutionContext::new(request_context.clone(), &caller);
        let usage_tracker = FunctionUsageTracker::new();
        let result = self
            .cache_manager
            .get(
                request_context,
                path.clone(),
                args.clone(),
                identity.clone(),
                ts,
                journal,
                caller.clone(),
                usage_tracker.clone(),
                invocation,
            )
            .await;

        match result {
            Ok(query_return) => Ok(query_return),
            Err(e) => {
                // Log system errors to the function log, similar to how mutations are logged.
                // This ensures errors like rate-limiting (TooManyConcurrentRequests) appear
                // in the dashboard and log streams.
                self.function_log
                    .log_query_system_error(
                        &e,
                        path.debug_into_component_path(),
                        args,
                        identity.into(),
                        start,
                        caller,
                        context,
                        invocation,
                    )
                    .await?;
                Err(e)
            },
        }
    }

    #[fastrace::trace]
    async fn check_mutation_status(
        &self,
        tx: &mut Transaction<RT>,
        mutation_identifier: &Option<SessionRequestIdentifier>,
    ) -> anyhow::Result<Option<Result<MutationReturn, MutationError>>> {
        let Some(identifier) = mutation_identifier else {
            return Ok(None);
        };
        let mutation_status = SessionRequestModel::new(tx)
            .get_session_request_record(identifier, Identity::system())
            .await?;
        let result = match mutation_status {
            Some((ts, SessionRequestOutcome::Mutation { result, log_lines })) => {
                let age = tx.begin_timestamp().secs_since_f64(ts);
                tracing::info!(
                    "Mutation already executed {age:.3}s ago so skipping {:?}",
                    identifier
                );
                log_mutation_already_committed(age);
                // Sessions are recorded transactionally in mutations so can use the same commit
                // ts.
                Ok(MutationReturn {
                    value: result.resolve_commit_ts(i64::from(ts))?,
                    log_lines,
                    ts,
                })
            },
            None => return Ok(None),
        };
        Ok(Some(result))
    }

    #[fastrace::trace]
    async fn write_mutation_status(
        &self,
        tx: &mut Transaction<RT>,
        mutation_identifier: &Option<SessionRequestIdentifier>,
        outcome: &ValidatedUdfOutcome,
    ) -> anyhow::Result<()> {
        let Some(identifier) = mutation_identifier else {
            return Ok(());
        };
        if let Ok(ref value) = outcome.result {
            let record = SessionRequestRecord {
                session_id: identifier.session_id,
                request_id: identifier.request_id,
                outcome: SessionRequestOutcome::Mutation {
                    result: value.clone(),
                    log_lines: outcome.log_lines.clone(),
                },
                identity: outcome.identity.clone(),
            };
            SessionRequestModel::new(tx)
                .record_session_request(record, Identity::system())
                .await?;
        }
        Ok(())
    }

    async fn bail_if_backend_not_running(&self, tx: &mut Transaction<RT>) -> anyhow::Result<()> {
        let backend_state = BackendStateModel::new(tx).get_backend_state().await?;
        if backend_state.is_stopped() {
            anyhow::bail!(ErrorMetadata::bad_request(
                "BackendIsNotRunning",
                "Cannot perform this operation when the backend is not running"
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl<RT: Runtime> ActionCallbacks for ApplicationFunctionRunner<RT> {
    #[fastrace::trace]
    async fn create_ai_gateway_token(
        &self,
        identity: Identity,
        caller: AttributedCaller,
        request_id: RequestId,
    ) -> anyhow::Result<String> {
        self.mint_ai_gateway_jwt(&identity, AttributionClaims::from(caller), &request_id)
            .await
    }

    #[fastrace::trace]
    async fn execute_query(
        &self,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        context: ExecutionContext,
    ) -> anyhow::Result<FunctionResult> {
        let ts = self.database.now_ts_for_reads();
        let result = self
            .run_query_at_ts(
                RequestContext::new(context.request_id, context.request_metadata),
                PublicFunctionPath::Component(path),
                args,
                identity,
                *ts,
                None,
                FunctionCaller::Action {
                    parent_scheduled_job: context.parent_scheduled_job,
                    parent_execution_id: Some(context.execution_id),
                },
                QueryInvocation::Fresh,
            )
            .await?
            .result;
        Ok(FunctionResult { result })
    }

    #[fastrace::trace]
    async fn execute_mutation(
        &self,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        context: ExecutionContext,
    ) -> anyhow::Result<FunctionResult> {
        let result = self
            .retry_mutation(
                RequestContext::new(context.request_id, context.request_metadata),
                PublicFunctionPath::Component(path),
                args,
                identity,
                None,
                FunctionCaller::Action {
                    parent_scheduled_job: context.parent_scheduled_job,
                    parent_execution_id: Some(context.execution_id),
                },
                None,
            )
            .await
            .map(|r| match r {
                Ok(mutation_return) => Ok(mutation_return.value),
                Err(mutation_error) => Err(mutation_error.error),
            })?;
        Ok(FunctionResult { result })
    }

    #[fastrace::trace]
    async fn execute_action(
        &self,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
        args: SerializedArgs,
        context: ExecutionContext,
    ) -> anyhow::Result<FunctionResult> {
        let _tx = self.database.begin(identity.clone()).await?;
        let result = self
            .run_action(
                RequestContext::new(context.request_id, context.request_metadata),
                PublicFunctionPath::Component(path),
                args,
                identity,
                FunctionCaller::Action {
                    parent_scheduled_job: context.parent_scheduled_job,
                    parent_execution_id: Some(context.execution_id),
                },
            )
            .await
            .map(|r| match r {
                Ok(action_return) => Ok(action_return.value),
                Err(action_error) => Err(action_error.error),
            })?;
        Ok(FunctionResult { result })
    }

    async fn storage_get_url(
        &self,
        identity: Identity,
        component: ComponentId,
        storage_id: FileStorageId,
    ) -> anyhow::Result<Option<String>> {
        let mut tx = self.database.begin(identity).await?;
        self.bail_if_backend_not_running(&mut tx).await?;
        self.file_storage
            .get_url(&mut tx, component, storage_id)
            .await
    }

    async fn storage_get_file_entry(
        &self,
        identity: Identity,
        component: ComponentId,
        storage_id: FileStorageId,
    ) -> anyhow::Result<Option<(ComponentPath, FileStorageEntry)>> {
        let mut tx = self.database.begin(identity).await?;
        self.bail_if_backend_not_running(&mut tx).await?;
        let Some(component_path) = tx.get_component_path(component) else {
            return Ok(None);
        };
        let entry = self
            .file_storage
            .get_file_entry(&mut tx, component.into(), storage_id)
            .await?;
        Ok(entry.map(|e| (component_path, e)))
    }

    async fn storage_store_file_entry(
        &self,
        identity: Identity,
        component: ComponentId,
        entry: FileStorageEntry,
    ) -> anyhow::Result<(ComponentPath, DeveloperDocumentId)> {
        let mut tx = self.database.begin(identity.clone()).await?;
        self.bail_if_backend_not_running(&mut tx).await?;
        let (_ts, r, _stats) = self
            .database
            .execute_with_occ_retries(
                identity,
                FunctionUsageTracker::new(),
                MAX_OCC_FAILURES,
                "app_funrun_storage_store_file_entry",
                |tx| {
                    async {
                        let component_path = tx
                            .get_component_path(component)
                            .context(format!("Component {component:?} not found"))?;
                        let (_, id) = self
                            .file_storage
                            .store_file_entry(tx, component.into(), entry.clone())
                            .await?;
                        Ok((component_path, id))
                    }
                    .into()
                },
            )
            .await?;
        Ok(r)
    }

    async fn storage_delete(
        &self,
        identity: Identity,
        component: ComponentId,
        storage_id: FileStorageId,
    ) -> anyhow::Result<()> {
        let mut tx = self.database.begin(identity.clone()).await?;
        self.bail_if_backend_not_running(&mut tx).await?;
        self.database
            .execute_with_occ_retries(
                identity,
                FunctionUsageTracker::new(),
                MAX_OCC_FAILURES,
                "app_funrun_storage_delete",
                |tx| {
                    async {
                        self.file_storage
                            .delete(tx, component.into(), storage_id.clone())
                            .await?;
                        Ok(())
                    }
                    .into()
                },
            )
            .await?;

        Ok(())
    }

    async fn schedule_job(
        &self,
        identity: Identity,
        scheduling_component: ComponentId,
        scheduled_path: CanonicalizedComponentFunctionPath,
        udf_args: SerializedArgs,
        scheduled_ts: UnixTimestamp,
        context: ExecutionContext,
    ) -> anyhow::Result<DeveloperDocumentId> {
        let (_ts, (virtual_id, arg_size), _stats) = self
            .database
            .execute_with_occ_retries(
                identity,
                FunctionUsageTracker::new(),
                MAX_OCC_FAILURES,
                "app_funrun_schedule_job",
                |tx| {
                    let path = scheduled_path.clone();
                    let args = udf_args.clone();
                    let context = context.clone();
                    async move {
                        let (path, udf_args) = validate_schedule_args(
                            path,
                            args.into_args()?,
                            scheduled_ts,
                            // Scheduling from actions is not transactional and happens at latest
                            // timestamp.
                            self.database.runtime().unix_timestamp(),
                            tx,
                        )
                        .await?;
                        let arg_size = udf_args.size();
                        let virtual_id =
                            VirtualSchedulerModel::new(tx, scheduling_component.into())
                                .schedule(path, udf_args, scheduled_ts, context)
                                .await?;
                        Ok((virtual_id, arg_size))
                    }
                    .into()
                },
            )
            .await?;
        if let Some(warning) =
            scheduled_arg_size_warning(arg_size, &None /* system_udf_path */)
        {
            let timestamp = self.runtime.unix_timestamp();
            self.function_log.log_action_progress(
                // This is completely wrong - it should be the path of the caller function, but we
                // don't know that here and it's at least a hint
                scheduled_path,
                timestamp,
                context,
                LogLines::from(vec![warning.into_log_line(timestamp)]),
                ModuleEnvironment::Isolate,
            );
        }
        Ok(virtual_id)
    }

    async fn cancel_job(
        &self,
        identity: Identity,
        virtual_id: DeveloperDocumentId,
    ) -> anyhow::Result<()> {
        self.database
            .execute_with_occ_retries(
                identity,
                FunctionUsageTracker::new(),
                MAX_OCC_FAILURES,
                "app_funrun_cancel_job",
                |tx| {
                    async {
                        VirtualSchedulerModel::new(tx, TableNamespace::by_component_TODO())
                            .cancel(virtual_id)
                            .await
                    }
                    .into()
                },
            )
            .await?;
        Ok(())
    }

    async fn vector_search(
        &self,
        identity: Identity,
        query: JsonValue,
    ) -> anyhow::Result<(Vec<PublicVectorSearchQueryResult>, FunctionUsageStats)> {
        let query = VectorSearch::try_from(query).map_err(|e| {
            let message = e.to_string();
            e.context(ErrorMetadata::bad_request("InvalidVectorQuery", message))
        })?;
        self.database.vector_search(identity, query).await
    }

    async fn lookup_function_handle(
        &self,
        identity: Identity,
        handle: FunctionHandle,
    ) -> anyhow::Result<CanonicalizedComponentFunctionPath> {
        let mut tx = self.database.begin(identity).await?;
        FunctionHandlesModel::new(&mut tx).lookup(handle).await
    }

    async fn create_function_handle(
        &self,
        identity: Identity,
        path: CanonicalizedComponentFunctionPath,
    ) -> anyhow::Result<FunctionHandle> {
        let mut tx = self.database.begin(identity).await?;
        FunctionHandlesModel::new(&mut tx)
            .get_with_component_path(path)
            .await
    }
}
