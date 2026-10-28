use std::{
    collections::{
        BTreeMap,
        BTreeSet,
        HashMap,
        VecDeque,
    },
    env,
    pin::pin,
    sync::{
        atomic::{
            AtomicUsize,
            Ordering,
        },
        Arc,
        Once,
    },
    time::Duration,
};

use ::metrics::{
    IntoLabel,
    Timer,
};
use anyhow::Context as _;
use async_trait::async_trait;
use common::{
    auth::AuthConfig,
    backoff::Backoff,
    bootstrap_model::components::definition::ComponentDefinitionMetadata,
    codel_queue::{
        CoDelQueue,
        CoDelQueueReceiver,
        CoDelQueueSender,
        ExpiredInQueue,
    },
    components::{
        CanonicalizedComponentModulePath,
        ComponentDefinitionPath,
        ComponentName,
        Resource,
    },
    errors::{
        recapture_stacktrace,
        JsError,
    },
    execution_context::ExecutionContext,
    fastrace_helpers::{
        initialize_root_from_parent,
        EncodedSpan,
    },
    http::{
        fetch::FetchClient,
        RoutedHttpPath,
    },
    knobs::{
        ANALYZE_CONCURRENCY,
        HEAP_WORKER_REPORT_INTERVAL_SECONDS,
        ISOLATE_IDLE_TIMEOUT,
        ISOLATE_MAX_LIFETIME,
        ISOLATE_MAX_USER_HEAP_SIZE,
        ISOLATE_QUEUE_CONGESTED_TIMEOUT,
        ISOLATE_QUEUE_IDLE_TIMEOUT,
        ISOLATE_QUEUE_SIZE,
        REUSE_ISOLATES,
        V8_THREADS,
    },
    log_lines::LogLine,
    query_journal::QueryJournal,
    runtime::{
        shutdown_and_join,
        Runtime,
        SpawnHandle,
        UnixTimestamp,
    },
    schemas::DatabaseSchema,
    static_span,
    types::{
        DeploymentMetadata,
        ModuleEnvironment,
        UdfType,
    },
    utils::ensure_utc,
};
use database::{
    shutdown_error,
    Transaction,
};
use deno_core::v8::{
    self,
    V8,
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use fastrace::{
    func_path,
    future::FutureExt as _,
    local::LocalSpan,
    Event,
};
use file_storage::{
    FileDownloads,
    TransactionalFileStorage,
};
use futures::{
    future::{
        self,
        Join,
        Ready,
    },
    stream::{
        self,
        FuturesUnordered,
        PollNext,
        StreamExt,
    },
    TryStreamExt as _,
};
use itertools::Either;
use keybroker::{
    FunctionRunnerKeyBroker,
    Identity,
};
use model::{
    config::types::ModuleConfig,
    environment_variables::types::{
        EnvVarName,
        EnvVarValue,
    },
    modules::module_versions::{
        AnalyzedModule,
        FullModuleSource,
        ModuleSource,
        SourceMap,
    },
    udf_config::types::UdfConfig,
};
use parking_lot::Mutex;
use prometheus::VMHistogram;
use sync_types::CanonicalizedModulePath;
use tokio::sync::{
    mpsc,
    oneshot,
};
use tokio_stream::wrappers::{
    ReceiverStream,
    UnboundedReceiverStream,
};
use tracing::Instrument as _;
use udf::{
    validation::{
        ValidatedHttpPath,
        ValidatedPathAndArgs,
    },
    ActionCallbacks,
    ActionOutcome,
    EvaluateAppDefinitionsResult,
    FunctionOutcome,
    HttpActionOutcome,
    HttpActionResponseStreamer,
    NestedUdfOutcome,
};
use value::identifier::Identifier;

use crate::{
    concurrency_limiter::ConcurrencyLimiter,
    context_cache::{
        CachedContexts,
        ContextCache,
    },
    isolate::{
        Isolate,
        IsolateHeapStats,
    },
    metrics::{
        self,
        log_aggregated_heap_stats,
        log_pool_max,
        log_pool_running_count,
        log_worker_stolen,
        queue_timer,
        rejected_before_execution_error,
        RejectedBeforeExecutionReason,
    },
    module_cache::{
        ModuleCache,
        V8ModuleSource,
    },
    ConcurrencyPermit,
};

pub const PAUSE_RECREATE_CLIENT: &str = "recreate_client";
pub const PAUSE_REQUEST: &str = "pause_request";
pub const NO_AVAILABLE_WORKERS: &str = "There is no capacity available to run this function";

#[derive(Clone)]
pub struct IsolateConfig {
    // Name of isolate pool, used in metrics.
    pub name: &'static str,

    // Typically, the user timeout is configured based on environment. This
    // allows us to set an upper bound to it that we use for tests.
    max_user_timeout: Option<Duration>,

    pub(crate) limiter: ConcurrencyLimiter,
}

impl IsolateConfig {
    pub fn new(name: &'static str, limiter: ConcurrencyLimiter) -> Self {
        Self {
            name,
            max_user_timeout: None,
            limiter,
        }
    }

    pub fn max_user_timeout(&self) -> Option<Duration> {
        self.max_user_timeout
    }

}

pub struct UdfRequest<RT: Runtime> {
    pub path_and_args: ValidatedPathAndArgs,
    pub udf_type: UdfType,
    pub transaction: Transaction<RT>,
    pub unix_timestamp: UnixTimestamp,
    pub journal: QueryJournal,
    pub context: ExecutionContext,
    pub environment_data: EnvironmentData<RT>,
    pub file_downloads: FileDownloads,
}

pub struct HttpActionRequest<RT: Runtime> {
    pub http_module_path: ValidatedHttpPath,
    pub routed_path: RoutedHttpPath,
    pub http_request: udf::HttpActionRequest,
    pub transaction: Transaction<RT>,
    pub identity: Identity,
    pub context: ExecutionContext,
    pub environment_data: EnvironmentData<RT>,
}

pub struct ActionRequest<RT: Runtime> {
    pub params: ActionRequestParams,
    pub transaction: Transaction<RT>,
    pub identity: Identity,
    pub context: ExecutionContext,
    pub environment_data: EnvironmentData<RT>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ActionRequestParams {
    pub path_and_args: ValidatedPathAndArgs,
}

#[derive(Clone)]
pub struct EnvironmentData<RT: Runtime> {
    pub key_broker: FunctionRunnerKeyBroker,
    pub default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
    pub file_storage: TransactionalFileStorage<RT>,
    pub module_loader: Arc<dyn ModuleCache<RT>>,
    pub deployment: DeploymentMetadata,
}

pub struct Request<RT: Runtime> {
    pub client_id: String,
    pub inner: RequestType<RT>,
    pub parent_trace: EncodedSpan,
}

impl<RT: Runtime> Request<RT> {
    pub fn new(client_id: String, inner: RequestType<RT>, parent_trace: EncodedSpan) -> Self {
        Self {
            client_id,
            inner,
            parent_trace,
        }
    }

    pub fn module(&self) -> Option<CanonicalizedComponentModulePath> {
        let function_path = match &self.inner {
            RequestType::Udf { request, .. } => request.path_and_args.path(),
            RequestType::Action { request, .. } => request.params.path_and_args.path(),
            RequestType::HttpAction { request, .. } => request.http_module_path.path(),
            RequestType::Analyze { .. }
            | RequestType::EvaluateSchema { .. }
            | RequestType::EvaluateAuthConfig { .. }
            | RequestType::EvaluateAppDefinitions { .. }
            | RequestType::EvaluateComponentInitializer { .. } => return None,
        };
        Some(CanonicalizedComponentModulePath {
            component: function_path.component,
            module_path: function_path.udf_path.module().clone(),
        })
    }
}

pub enum RequestType<RT: Runtime> {
    Udf {
        request: UdfRequest<RT>,
        response: oneshot::Sender<anyhow::Result<(Transaction<RT>, FunctionOutcome)>>,
        queue_timer: Timer<VMHistogram>,
        rng_seed: [u8; 32],
        reactor_depth: usize,
        udf_callback: Option<IsolateClient<RT>>,
        function_started_sender: Option<oneshot::Sender<()>>,
    },
    Action {
        request: ActionRequest<RT>,
        response: oneshot::Sender<anyhow::Result<ActionOutcome>>,
        queue_timer: Timer<VMHistogram>,
        action_callbacks: Arc<dyn ActionCallbacks>,
        fetch_client: Arc<dyn FetchClient>,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
        function_started_sender: Option<oneshot::Sender<()>>,
    },
    HttpAction {
        request: HttpActionRequest<RT>,
        response: oneshot::Sender<anyhow::Result<HttpActionOutcome>>,
        queue_timer: Timer<VMHistogram>,
        action_callbacks: Arc<dyn ActionCallbacks>,
        fetch_client: Arc<dyn FetchClient>,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
        http_response_streamer: HttpActionResponseStreamer,
        function_started_sender: Option<oneshot::Sender<()>>,
    },
    Analyze {
        udf_config: UdfConfig,
        modules: Arc<BTreeMap<CanonicalizedModulePath, Arc<V8ModuleSource>>>,
        to_analyze: CanonicalizedModulePath,
        environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        response: oneshot::Sender<anyhow::Result<Result<AnalyzedModule, JsError>>>,
    },
    EvaluateSchema {
        schema_bundle: ModuleSource,
        source_map: Option<SourceMap>,
        rng_seed: [u8; 32],
        unix_timestamp: UnixTimestamp,
        response: oneshot::Sender<anyhow::Result<DatabaseSchema>>,
    },
    EvaluateAuthConfig {
        auth_config_bundle: ModuleSource,
        source_map: Option<SourceMap>,
        environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        response: oneshot::Sender<anyhow::Result<AuthConfig>>,
    },
    EvaluateAppDefinitions {
        app_definition: ModuleConfig,
        component_definitions: BTreeMap<ComponentDefinitionPath, ModuleConfig>,
        dependency_graph: BTreeSet<(ComponentDefinitionPath, ComponentDefinitionPath)>,
        user_environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
        response: oneshot::Sender<anyhow::Result<EvaluateAppDefinitionsResult>>,
    },
    EvaluateComponentInitializer {
        evaluated_definitions: BTreeMap<ComponentDefinitionPath, ComponentDefinitionMetadata>,
        path: ComponentDefinitionPath,
        definition: ModuleConfig,
        args: BTreeMap<Identifier, Resource>,
        name: ComponentName,
        response: oneshot::Sender<anyhow::Result<BTreeMap<Identifier, Resource>>>,
    },
}

#[allow(async_fn_in_trait)]
pub trait UdfCallback<RT: Runtime> {
    /// Execute a subfunction in a new V8 context.
    /// This can either be in the same isolate (RunUdf), or another one
    /// (IsolateClient).
    async fn execute_nested_udf(
        self,
        client_id: String,
        udf_request: UdfRequest<RT>,
        rng_seed: [u8; 32],
        reactor_depth: usize,
    ) -> anyhow::Result<(Transaction<RT>, NestedUdfOutcome)>;
}

impl<RT: Runtime, T, U> UdfCallback<RT> for Either<T, U>
where
    T: UdfCallback<RT>,
    U: UdfCallback<RT>,
{
    async fn execute_nested_udf(
        self,
        client_id: String,
        udf_request: UdfRequest<RT>,
        rng_seed: [u8; 32],
        reactor_depth: usize,
    ) -> anyhow::Result<(Transaction<RT>, NestedUdfOutcome)> {
        match self {
            Either::Left(l) => {
                l.execute_nested_udf(client_id, udf_request, rng_seed, reactor_depth)
                    .await
            },
            Either::Right(r) => {
                r.execute_nested_udf(client_id, udf_request, rng_seed, reactor_depth)
                    .await
            },
        }
    }
}

impl<RT: Runtime> Request<RT> {
    fn expire(self, error: ExpiredInQueue) {
        let error = anyhow::anyhow!(error).context(rejected_before_execution_error(
            RejectedBeforeExecutionReason::ExpiredInQueue,
        ));
        self.send_error(error);
    }

    fn reject(self, reason: RejectedBeforeExecutionReason) {
        let error = rejected_before_execution_error(reason).into();
        self.send_error(error);
    }

    fn send_error(self, error: anyhow::Error) {
        match self.inner {
            RequestType::Udf { response, .. } => {
                let _ = response.send(Err(error));
            },
            RequestType::Action { response, .. } => {
                let _ = response.send(Err(error));
            },
            RequestType::HttpAction { response, .. } => {
                let _ = response.send(Err(error));
            },
            RequestType::Analyze { response, .. } => {
                let _ = response.send(Err(error));
            },
            RequestType::EvaluateSchema { response, .. } => {
                let _ = response.send(Err(error));
            },
            RequestType::EvaluateAuthConfig { response, .. } => {
                let _ = response.send(Err(error));
            },
            RequestType::EvaluateAppDefinitions { response, .. } => {
                let _ = response.send(Err(error));
            },
            RequestType::EvaluateComponentInitializer { response, .. } => {
                let _ = response.send(Err(error));
            },
        }
    }

    fn is_response_closed(&self) -> bool {
        match &self.inner {
            RequestType::Udf { response, .. } => response.is_closed(),
            RequestType::Action { response, .. } => response.is_closed(),
            RequestType::HttpAction { response, .. } => response.is_closed(),
            RequestType::Analyze { response, .. } => response.is_closed(),
            RequestType::EvaluateSchema { response, .. } => response.is_closed(),
            RequestType::EvaluateAuthConfig { response, .. } => response.is_closed(),
            RequestType::EvaluateAppDefinitions { response, .. } => response.is_closed(),
            RequestType::EvaluateComponentInitializer { response, .. } => response.is_closed(),
        }
    }
}

impl<RT: Runtime> Clone for IsolateClient<RT> {
    fn clone(&self) -> Self {
        Self {
            rt: self.rt.clone(),
            handles: self.handles.clone(),
            scheduler: self.scheduler.clone(),
            sender: self.sender.clone(),
            internal_sender: self.internal_sender.clone(),
            active_workers: self.active_workers.clone(),
            max_workers: self.max_workers,
        }
    }
}

pub fn initialize_v8() {
    ensure_utc().expect("Failed to setup timezone");
    static V8_INIT: Once = Once::new();
    V8_INIT.call_once(|| {
        let _s = static_span!("initialize_v8");

        // `deno_core_icudata` internally loads this with proper 16-byte alignment.
        assert!(v8::icu::set_common_data_77(deno_core_icudata::ICU_DATA).is_ok());

        // Calls into `v8::platform::v8__Platform__NewUnprotectedDefaultPlatform`
        // Can configure with...
        // - thread_pool_size (default: zero): number of worker threads for background
        //   jobs, picks a reasonable default based on number of cores if set to zero
        // - idle_task_support (default: false): platform will except idle tasks and
        //   will rely on embedder calling `v8::platform::RunIdleTasks`. Idle tasks are
        //   low-priority tasks that are run with a deadline indicating how long the
        //   scheduler expects to be idle (e.g. unused remainder of a frame budget)
        // - in_process_stack_dumping (default: false)
        // - tracing_controller (default: null): if null, the platform creates a
        //   `v8::platform::TracingController` instance and uses it
        // Why "unprotected"? The "protected" default platform utilizes Memory
        // Protection Keys (PKU), which requires that all threads utilizing V8 are
        // descendents of the thread that initialized V8. Unfortunately, this is
        // not compatible with how Rust tests run and additionally, the version of V8
        // used at the time of this comment has a bug with PKU on certain Intel CPUs.
        // See https://github.com/denoland/rusty_v8/issues/1381
        let platform = v8::new_unprotected_default_platform(*V8_THREADS, false).make_shared();

        // Calls into `v8::V8::InitializePlatform`, sets global platform.
        V8::initialize_platform(platform);

        // TODO: Figure out what V8 uses entropy for and set it here.
        // V8::set_entropy_source(...);

        // Set V8 command line flags.
        // https://github.com/v8/v8/blob/master/src/flags/flag-definitions.h
        let mut argv = vec![
            "".to_owned(), // first arg is ignored
            // See https://github.com/denoland/deno/issues/2544
            "--no-wasm-async-compilation".to_string(),
            // Disable `eval` or `new Function()`.
            "--disallow-code-generation-from-strings".to_string(),
            // We ensure 4MiB of stack space on all of our threads, so
            // tell V8 it can use up to 2MiB of stack space itself. The
            // default is 1MiB. Note that the flag is in KiB (https://github.com/v8/v8/blob/master/src/flags/flag-definitions.h#L1594).
            "--stack-size=2048".to_string(),
            "--js-base-64".to_string(),
            // Planned to be phased out
            "--no-experimental-wasm-legacy-eh".to_string(),
        ];
        if let Ok(flags) = env::var("ISOLATE_V8_FLAGS") {
            argv.extend(
                flags
                    .split(" ")
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_owned()),
            );
            tracing::info!("Final V8 flags: {:?}", argv);
        }
        // v8 returns the args that were misunderstood
        let misunderstood = V8::set_flags_from_command_line(argv);
        assert_eq!(misunderstood, vec![""]);

        // Calls into `v8::V8::Initialize`
        V8::initialize();

        crate::udf_runtime::initialize();
    });
}

/// The V8 code all expects to run on a single thread, which makes it ineligible
/// for Tokio's scheduler, which wants the ability to move work across scheduler
/// threads. Instead, we'll manage our V8 threads ourselves.
///
/// [`IsolateClient`] is the "client" entry point to our V8 threads.
pub struct IsolateClient<RT: Runtime> {
    rt: RT,
    handles: Arc<Mutex<Vec<IsolateWorkerHandle>>>,
    scheduler: Arc<Mutex<Option<Box<dyn SpawnHandle>>>>,
    sender: CoDelQueueSender<RT, Request<RT>>,
    internal_sender: mpsc::UnboundedSender<Request<RT>>,
    /// Shared with the scheduler. Tracks the total number of in-progress
    /// workers across all clients.
    active_workers: Arc<AtomicUsize>,
    max_workers: usize,
}

impl<RT: Runtime> IsolateClient<RT> {
    pub fn new<W: IsolateWorker<RT>>(
        rt: RT,
        max_percent_per_client: usize,
        max_isolate_workers: usize,
        isolate_worker: W,
    ) -> anyhow::Result<Self> {
        initialize_v8();
        // NB: We don't call V8::Dispose or V8::ShutdownPlatform since we just assume a
        // single V8 instance per process and don't need to clean up its
        // resources.
        let (sender, receiver) = CoDelQueue::new(
            rt.clone(),
            *ISOLATE_QUEUE_SIZE,
            *ISOLATE_QUEUE_IDLE_TIMEOUT,
            *ISOLATE_QUEUE_CONGESTED_TIMEOUT,
        )
        .into_sender_and_receiver();
        let (internal_sender, internal_receiver) = mpsc::unbounded_channel();
        let handles = Arc::new(Mutex::new(Vec::new()));
        let handles_clone = handles.clone();
        let active_workers = Arc::new(AtomicUsize::new(0));
        let _active_workers = active_workers.clone();
        let rt_clone = rt.clone();
        let scheduler = rt.spawn("shared_isolate_scheduler", async move {
            // The scheduler thread pops a worker from available_workers and
            // pops a request from the CoDelQueueReceiver. Then it sends the request
            // to the worker.
            let scheduler = SharedIsolateScheduler::new(
                rt_clone,
                isolate_worker,
                max_isolate_workers,
                handles_clone,
                max_percent_per_client,
                _active_workers,
            );
            scheduler.run(receiver, internal_receiver).await
        });
        Ok(Self {
            rt,
            sender,
            internal_sender,
            scheduler: Arc::new(Mutex::new(Some(scheduler))),
            handles,
            active_workers,
            max_workers: max_isolate_workers,
        })
    }

    /// Returns the total number of isolate workers currently servicing a
    /// request across all clients.
    pub fn active_workers(&self) -> usize {
        self.active_workers.load(Ordering::Relaxed)
    }

    /// Returns the maximum number of isolate workers this client's scheduler
    /// is permitted to create.
    pub fn max_workers(&self) -> usize {
        self.max_workers
    }

    pub fn aggregate_heap_stats(&self) -> IsolateHeapStats {
        let mut total = IsolateHeapStats::default();
        for handle in self.handles.lock().iter() {
            total += handle.heap_stats.get();
        }
        total
    }

    #[fastrace::trace]
    pub async fn execute_udf(
        &self,
        udf_type: UdfType,
        path_and_args: ValidatedPathAndArgs,
        transaction: Transaction<RT>,
        journal: QueryJournal,
        context: ExecutionContext,
        environment_data: EnvironmentData<RT>,
        rng_seed: [u8; 32],
        unix_timestamp: UnixTimestamp,
        reactor_depth: usize,
        instance_name: String,
        function_started_sender: Option<oneshot::Sender<()>>,
        subfunctions_in_same_isolate: bool,
    ) -> anyhow::Result<(Transaction<RT>, FunctionOutcome)> {
        let (tx, rx) = oneshot::channel();
        let request = RequestType::Udf {
            request: UdfRequest {
                path_and_args,
                udf_type,
                transaction,
                unix_timestamp,
                journal,
                context,
                environment_data,
                file_downloads: FileDownloads::default(),
            },
            response: tx,
            queue_timer: queue_timer(),
            rng_seed,
            reactor_depth,
            function_started_sender,
            udf_callback: if subfunctions_in_same_isolate {
                None
            } else {
                Some(self.clone())
            },
        };
        self.send_request(Request::new(
            instance_name,
            request,
            EncodedSpan::from_parent(),
        ))?;
        let (tx, outcome) = Self::receive_response(rx).await??;

        Ok((tx, outcome))
    }

    #[fastrace::trace]
    pub async fn execute_action(
        &self,
        path_and_args: ValidatedPathAndArgs,
        transaction: Transaction<RT>,
        action_callbacks: Arc<dyn ActionCallbacks>,
        fetch_client: Arc<dyn FetchClient>,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
        context: ExecutionContext,
        environment_data: EnvironmentData<RT>,
        instance_name: String,
        function_started_sender: Option<oneshot::Sender<()>>,
    ) -> anyhow::Result<ActionOutcome> {
        let (tx, rx) = oneshot::channel();
        let request = RequestType::Action {
            request: ActionRequest {
                params: ActionRequestParams { path_and_args },
                identity: transaction.identity().clone(),
                transaction,
                context,
                environment_data,
            },
            response: tx,
            queue_timer: queue_timer(),
            action_callbacks,
            fetch_client,
            log_line_sender,
            function_started_sender,
        };
        self.send_request(Request::new(
            instance_name,
            request,
            EncodedSpan::from_parent(),
        ))?;
        match Self::receive_response(rx).await? {
            Ok(outcome) => Ok(outcome),
            Err(e) => Err(recapture_stacktrace(e).await),
        }
    }

    /// Execute an HTTP action.
    /// HTTP actions can run other UDFs, so they take in a ActionCallbacks from
    /// the application layer. This creates a transient reference cycle.
    #[fastrace::trace]
    pub async fn execute_http_action(
        &self,
        http_module_path: ValidatedHttpPath,
        routed_path: RoutedHttpPath,
        http_request: udf::HttpActionRequest,
        identity: Identity,
        action_callbacks: Arc<dyn ActionCallbacks>,
        fetch_client: Arc<dyn FetchClient>,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
        http_response_streamer: HttpActionResponseStreamer,
        transaction: Transaction<RT>,
        context: ExecutionContext,
        environment_data: EnvironmentData<RT>,
        instance_name: String,
        function_started_sender: Option<oneshot::Sender<()>>,
    ) -> anyhow::Result<HttpActionOutcome> {
        let (tx, rx) = oneshot::channel();
        let request = RequestType::HttpAction {
            request: HttpActionRequest {
                http_module_path,
                routed_path,
                http_request,
                identity,
                transaction,
                context,
                environment_data,
            },
            response: tx,
            queue_timer: queue_timer(),
            action_callbacks,
            fetch_client,
            log_line_sender,
            http_response_streamer,
            function_started_sender,
        };
        self.send_request(Request::new(
            instance_name,
            request,
            EncodedSpan::from_parent(),
        ))?;
        match Self::receive_response(rx).await? {
            Ok(outcome) => Ok(outcome),
            Err(e) => Err(recapture_stacktrace(e).await),
        }
    }

    /// Analyze a set of user-defined modules.
    #[fastrace::trace]
    pub async fn analyze(
        &self,
        udf_config: UdfConfig,
        modules: BTreeMap<CanonicalizedModulePath, ModuleConfig>,
        environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        instance_name: String,
    ) -> anyhow::Result<Result<BTreeMap<CanonicalizedModulePath, AnalyzedModule>, JsError>> {
        anyhow::ensure!(
            modules
                .values()
                .all(|m| m.environment == ModuleEnvironment::Isolate),
            "Can only analyze Isolate modules"
        );
        let to_analyze: Vec<_> = modules
            .keys()
            .filter(|path| !path.is_deps())
            .cloned()
            .collect();
        let modules: Arc<BTreeMap<_, _>> = Arc::new(
            modules
                .into_iter()
                .map(|(path, module_config)| {
                    (
                        path,
                        Arc::new(V8ModuleSource::new(FullModuleSource {
                            source: module_config.source,
                            source_map: module_config.source_map,
                        })),
                    )
                })
                .collect(),
        );
        let mut stream = pin!(stream::iter(to_analyze)
            .map(|to_analyze| async {
                let mut backoff = Backoff::new(Duration::from_millis(500), Duration::from_secs(2));
                let mut attempt = 1;
                const MAX_ATTEMPTS: u32 = 3;
                loop {
                    let (tx, rx) = oneshot::channel();
                    let request = RequestType::Analyze {
                        modules: modules.clone(),
                        to_analyze: to_analyze.clone(),
                        response: tx,
                        udf_config: udf_config.clone(),
                        environment_variables: environment_variables.clone(),
                    };
                    self.send_request(Request::new(
                        instance_name.clone(),
                        request,
                        EncodedSpan::from_parent(),
                    ))?;
                    match IsolateClient::<RT>::receive_response(rx).await? {
                        Ok(outcome) => return Ok((to_analyze, outcome)),
                        Err(e)
                            if attempt < MAX_ATTEMPTS
                                && (e.is_rejected_before_execution() || e.is_overloaded()) =>
                        {
                            tracing::warn!("Retrying analyze after system error: {e:?}");
                            let wait = backoff.fail(&mut self.rt.rng());
                            self.rt.wait(wait).await;
                            attempt += 1;
                            continue;
                        },
                        Err(e) => return Err(recapture_stacktrace(e).await),
                    }
                }
            })
            .buffer_unordered(*ANALYZE_CONCURRENCY));
        let mut analyzed_modules = BTreeMap::new();
        while let Some((path, r)) = stream.try_next().await? {
            match r {
                Ok(analyzed_module) => analyzed_modules.insert(path, analyzed_module),
                Err(r) => return Ok(Err(r)),
            };
        }
        Ok(Ok(analyzed_modules))
    }

    #[fastrace::trace]
    pub async fn evaluate_app_definitions(
        &self,
        app_definition: ModuleConfig,
        component_definitions: BTreeMap<ComponentDefinitionPath, ModuleConfig>,
        dependency_graph: BTreeSet<(ComponentDefinitionPath, ComponentDefinitionPath)>,
        user_environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
        instance_name: String,
    ) -> anyhow::Result<EvaluateAppDefinitionsResult> {
        anyhow::ensure!(
            app_definition.environment == ModuleEnvironment::Isolate,
            "Can only evaluate Isolate modules"
        );
        anyhow::ensure!(
            component_definitions
                .values()
                .all(|m| m.environment == ModuleEnvironment::Isolate),
            "Can only evaluate Isolate modules"
        );
        let mut backoff = Backoff::new(Duration::from_millis(500), Duration::from_secs(2));
        let mut attempt = 1;
        const MAX_ATTEMPTS: u32 = 3;
        loop {
            let (tx, rx) = oneshot::channel();
            let request = RequestType::EvaluateAppDefinitions {
                app_definition: app_definition.clone(),
                component_definitions: component_definitions.clone(),
                dependency_graph: dependency_graph.clone(),
                user_environment_variables: user_environment_variables.clone(),
                system_env_vars: system_env_vars.clone(),
                response: tx,
            };
            self.send_request(Request::new(
                instance_name.clone(),
                request,
                EncodedSpan::from_parent(),
            ))?;
            match IsolateClient::<RT>::receive_response(rx).await? {
                Ok(outcome) => return Ok(outcome),
                Err(e)
                    if attempt < MAX_ATTEMPTS
                        && (e.is_rejected_before_execution() || e.is_overloaded()) =>
                {
                    tracing::warn!("Retrying evaluate_app_definitions after system error: {e:?}");
                    let wait = backoff.fail(&mut self.rt.rng());
                    self.rt.wait(wait).await;
                    attempt += 1;
                    continue;
                },
                Err(e) => return Err(recapture_stacktrace(e).await),
            }
        }
    }

    #[fastrace::trace]
    pub async fn evaluate_component_initializer(
        &self,
        evaluated_definitions: BTreeMap<ComponentDefinitionPath, ComponentDefinitionMetadata>,
        path: ComponentDefinitionPath,
        definition: ModuleConfig,
        args: BTreeMap<Identifier, Resource>,
        name: ComponentName,
        instance_name: String,
    ) -> anyhow::Result<BTreeMap<Identifier, Resource>> {
        let mut backoff = Backoff::new(Duration::from_millis(500), Duration::from_secs(2));
        let mut attempt = 1;
        const MAX_ATTEMPTS: u32 = 3;
        loop {
            let (tx, rx) = oneshot::channel();
            let request = RequestType::EvaluateComponentInitializer {
                evaluated_definitions: evaluated_definitions.clone(),
                path: path.clone(),
                definition: definition.clone(),
                args: args.clone(),
                name: name.clone(),
                response: tx,
            };
            self.send_request(Request::new(
                instance_name.clone(),
                request,
                EncodedSpan::from_parent(),
            ))?;
            match IsolateClient::<RT>::receive_response(rx).await? {
                Ok(outcome) => return Ok(outcome),
                Err(e)
                    if attempt < MAX_ATTEMPTS
                        && (e.is_rejected_before_execution() || e.is_overloaded()) =>
                {
                    tracing::warn!(
                        "Retrying evaluate_component_initializer after system error: {e:?}"
                    );
                    let wait = backoff.fail(&mut self.rt.rng());
                    self.rt.wait(wait).await;
                    attempt += 1;
                    continue;
                },
                Err(e) => return Err(recapture_stacktrace(e).await),
            }
        }
    }

    #[fastrace::trace]
    pub async fn evaluate_schema(
        &self,
        schema_bundle: ModuleSource,
        source_map: Option<SourceMap>,
        rng_seed: [u8; 32],
        unix_timestamp: UnixTimestamp,
        instance_name: String,
    ) -> anyhow::Result<DatabaseSchema> {
        let mut backoff = Backoff::new(Duration::from_millis(500), Duration::from_secs(2));
        let mut attempt = 1;
        const MAX_ATTEMPTS: u32 = 3;
        loop {
            let (tx, rx) = oneshot::channel();
            let request = RequestType::EvaluateSchema {
                schema_bundle: schema_bundle.clone(),
                source_map: source_map.clone(),
                rng_seed,
                unix_timestamp,
                response: tx,
            };
            self.send_request(Request::new(
                instance_name.clone(),
                request,
                EncodedSpan::from_parent(),
            ))?;
            match IsolateClient::<RT>::receive_response(rx).await? {
                Ok(outcome) => return Ok(outcome),
                Err(e)
                    if attempt < MAX_ATTEMPTS
                        && (e.is_rejected_before_execution() || e.is_overloaded()) =>
                {
                    tracing::warn!("Retrying evaluate_schema after system error: {e:?}");
                    let wait = backoff.fail(&mut self.rt.rng());
                    self.rt.wait(wait).await;
                    attempt += 1;
                    continue;
                },
                Err(e) => return Err(recapture_stacktrace(e).await),
            }
        }
    }

    #[fastrace::trace]
    pub async fn evaluate_auth_config(
        &self,
        auth_config_bundle: ModuleSource,
        source_map: Option<SourceMap>,
        environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
        explanation: &str,
        instance_name: String,
    ) -> anyhow::Result<AuthConfig> {
        let mut backoff = Backoff::new(Duration::from_millis(500), Duration::from_secs(2));
        let mut attempt = 1;
        const MAX_ATTEMPTS: u32 = 3;
        let result = loop {
            let (tx, rx) = oneshot::channel();
            let request = RequestType::EvaluateAuthConfig {
                auth_config_bundle: auth_config_bundle.clone(),
                source_map: source_map.clone(),
                environment_variables: environment_variables.clone(),
                response: tx,
            };
            self.send_request(Request::new(
                instance_name.clone(),
                request,
                EncodedSpan::from_parent(),
            ))?;
            match IsolateClient::<RT>::receive_response(rx).await? {
                Ok(outcome) => return Ok(outcome),
                Err(e)
                    if attempt < MAX_ATTEMPTS
                        && (e.is_rejected_before_execution() || e.is_overloaded()) =>
                {
                    tracing::warn!("Retrying evaluate_auth_config after system error: {e:?}");
                    let wait = backoff.fail(&mut self.rt.rng());
                    self.rt.wait(wait).await;
                    attempt += 1;
                    continue;
                },
                Err(e) => break e,
            }
        };
        let is_env_var_error = result
            .to_string()
            .starts_with("Uncaught Error: Environment variable");
        let err = recapture_stacktrace(result).await;
        if err.is_rejected_before_execution() {
            return Err(err);
        }
        let error = err.to_string();
        if is_env_var_error {
            // Reformatting the underlying message to be nicer
            // here. Since we lost the underlying ErrorMetadata into the JSError,
            // we do some string matching instead. CX-4531
            Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                "AuthConfigMissingEnvironmentVariable",
                error.trim_start_matches("Uncaught Error: ").to_string(),
            )))
        } else {
            Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                "InvalidAuthConfig",
                format!("{explanation}: {error}"),
            )))
        }
    }

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        {
            let handles: Vec<_> = {
                let mut handles = self.handles.lock();
                for handle in &mut *handles {
                    handle.handle.shutdown();
                }
                handles.drain(..).collect()
            };
            for handle in handles.into_iter() {
                shutdown_and_join(handle.handle).await?;
            }
        }
        if let Some(mut scheduler) = self.scheduler.lock().take() {
            scheduler.shutdown();
        }

        Ok(())
    }

    fn send_request(&self, request: Request<RT>) -> anyhow::Result<()> {
        self.sender
            .try_send(request)
            .map_err(|_| metrics::execute_full_error())?;
        Ok(())
    }

    async fn receive_response<T>(rx: oneshot::Receiver<T>) -> anyhow::Result<T> {
        // The only reason a oneshot response channel wil be dropped prematurely if the
        // isolate worker is shutting down.
        rx.await.map_err(|_| shutdown_error())
    }
}

impl<RT: Runtime> UdfCallback<RT> for &IsolateClient<RT> {
    async fn execute_nested_udf(
        self,
        client_id: String,
        udf_request: UdfRequest<RT>,
        rng_seed: [u8; 32],
        reactor_depth: usize,
    ) -> anyhow::Result<(Transaction<RT>, NestedUdfOutcome)> {
        let subquery_path = udf_request.path_and_args.path().clone();
        let (tx, rx) = oneshot::channel();
        let request = RequestType::Udf {
            request: udf_request,
            response: tx,
            queue_timer: queue_timer(),
            rng_seed,
            reactor_depth,
            function_started_sender: None,
            udf_callback: Some(self.clone()),
        };
        self.internal_sender
            .send(Request::new(client_id, request, EncodedSpan::from_parent()))
            .ok()
            .context("scheduler shut down")?;
        let (tx, outcome) = IsolateClient::<RT>::receive_response(rx).await??;
        let outcome = match outcome {
            FunctionOutcome::Query(outcome) | FunctionOutcome::Mutation(outcome) => {
                NestedUdfOutcome {
                    observed_identity: outcome.observed_identity,
                    observed_rng: outcome.observed_rng,
                    observed_time: outcome.observed_time,
                    log_lines: outcome.log_lines,
                    audit_log_lines: outcome.audit_log_lines,
                    journal: outcome.journal,
                    result: match outcome.result {
                        Ok(t) => Ok(t.unpack().map_err(|e| {
                            e.wrap_error_message(|msg| {
                                format!(
                                    "Subquery {} return value invalid: {msg}",
                                    subquery_path.for_logging().debug_str(),
                                )
                            })
                        })?),
                        Err(e) => Err(e),
                    },
                    syscall_trace: outcome.syscall_trace,
                }
            },
            FunctionOutcome::Action(_) | FunctionOutcome::HttpAction(_) => {
                anyhow::bail!("nested udf must be query or mutation")
            },
        };
        Ok((tx, outcome))
    }
}

pub struct SharedIsolateScheduler<RT: Runtime, W: IsolateWorker<RT>> {
    rt: RT,
    worker: W,
    /// Vec of channels for sending work to individual workers.
    worker_senders: Vec<
        mpsc::Sender<(
            Request<RT>,
            ConcurrencyPermit,
            oneshot::Sender<IdleWorkerInfo>,
        )>,
    >,
    /// Map from client_id to stack of workers (implemented with a deque). The
    /// most recently used worker for a given client is at the front of the
    /// deque. These workers were previously used by this client, but may
    /// safely be "stolen" for use by another client. A worker with a
    /// `last_used_ts` older than `ISOLATE_IDLE_TIMEOUT` has already been
    /// recreated and there will be no penalty for reassigning this worker to a
    /// new client.
    available_workers: HashMap<String, VecDeque<IdleWorkerState>>,
    /// Set of futures awaiting a response from an active worker.
    in_progress_workers:
        FuturesUnordered<Join<oneshot::Receiver<IdleWorkerInfo>, Ready<ActiveWorkerState>>>,
    /// Counts the number of active workers per client. Should only contain a
    /// key if the value is greater than 0.
    in_progress_count: HashMap<String, usize>,
    /// Total number of in-progress workers across all clients.
    active_workers: Arc<AtomicUsize>,
    /// The max number of workers this scheduler is permitted to create.
    max_workers: usize,
    handles: Arc<Mutex<Vec<IsolateWorkerHandle>>>,
    /// The max number of active workers (per `in_progress_count`) allowed for a
    /// single client_id.
    max_active_workers_per_client: usize,
}

pub struct IdleWorkerInfo {
    cached_contexts: Arc<CachedContexts>,
}
struct IdleWorkerState {
    worker_id: usize,
    last_used_ts: tokio::time::Instant,
    info: IdleWorkerInfo,
}
struct ActiveWorkerState {
    worker_id: usize,
    client_id: String,
}

impl<RT: Runtime, W: IsolateWorker<RT>> SharedIsolateScheduler<RT, W> {
    pub fn new(
        rt: RT,
        worker: W,
        max_workers: usize,
        handles: Arc<Mutex<Vec<IsolateWorkerHandle>>>,
        max_percent_per_client: usize,
        active_workers: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            rt,
            worker,
            worker_senders: Vec::new(),
            in_progress_workers: FuturesUnordered::new(),
            in_progress_count: HashMap::new(),
            active_workers,
            available_workers: HashMap::new(),
            max_workers,
            handles,
            max_active_workers_per_client: (max_workers * max_percent_per_client)
                .div_ceil(100)
                .max(1),
        }
    }

    fn handle_completed_worker(
        &mut self,
        completed_worker: ActiveWorkerState,
        info: IdleWorkerInfo,
    ) {
        let new_count = match self
            .in_progress_count
            .remove_entry(&completed_worker.client_id)
        {
            Some((client_id, count)) if count > 1 => {
                self.in_progress_count.insert(client_id, count - 1);
                count - 1
            },
            Some((_, 1)) => {
                // Nothing to do; we've already removed the entry above.
                0
            },
            _ => panic!(
                "Inconsistent state in `in_progress_count` map; the count of active workers for \
                 client {} must be >= 1",
                completed_worker.client_id
            ),
        };
        self.active_workers.fetch_sub(1, Ordering::Relaxed);
        log_pool_running_count(
            self.worker.config().name,
            new_count,
            &completed_worker.client_id,
        );

        self.available_workers
            .entry(completed_worker.client_id)
            .or_default()
            .push_front(IdleWorkerState {
                worker_id: completed_worker.worker_id,
                last_used_ts: self.rt.monotonic_now(),
                info,
            });
    }

    pub async fn run(
        mut self,
        mut receiver: CoDelQueueReceiver<RT, Request<RT>>,
        internal_receiver: mpsc::UnboundedReceiver<Request<RT>>,
    ) {
        log_pool_max(self.worker.config().name, self.max_workers);
        let mut report_stats = self.rt.wait(*HEAP_WORKER_REPORT_INTERVAL_SECONDS);
        let mut expired_receiver = receiver.expired_receiver();
        let limiter = self.worker.config().limiter.clone();
        let rt = self.rt.clone();
        let external_request_stream =
            stream::poll_fn(move |cx| receiver.poll_next_with_expiration(cx)).filter_map(
                async |(request, expiration)| {
                    match expiration {
                        Ok(expiration) => {
                            let permit = tokio::select! {
                                biased;
                                permit = limiter.acquire(
                                    request.client_id.clone().into(),
                                    // For newly executing functions, we acquire the
                                    // permit in "low priority" mode. This means
                                    // that we prioritize already-executing
                                    // functions over new ones and avoid piling more
                                    // work on if we're overloaded.
                                    false,
                                ) => permit,
                                () = rt.wait(
                                    expiration.saturating_duration_since(rt.monotonic_now()),
                                ) => {
                                    request.reject(
                                        RejectedBeforeExecutionReason::InitialPermitTimeout,
                                    );
                                    return None;
                                }
                            };
                            Some((request, permit))
                        },
                        Err(expired) => {
                            request.expire(expired);
                            None
                        },
                    }
                },
            );
        let internal_request_stream =
            UnboundedReceiverStream::new(internal_receiver).then(async |request| {
                // Internal requests (for nested UDFs) get priority because they
                // block workers.
                let permit = limiter
                    .acquire(request.client_id.clone().into(), true)
                    .await;
                (request, permit)
            });
        let mut next_request_stream = pin!(stream::select_with_strategy(
            internal_request_stream,
            external_request_stream,
            |&mut ()| PollNext::Left
        ));
        loop {
            let all_workers_busy = self.active_workers.load(Ordering::Relaxed) >= self.max_workers;
            tokio::select! {
                biased;
                completed_worker = self.in_progress_workers.next(),
                if !self.in_progress_workers.is_empty() => {
                    let Some((Ok(info), completed_worker)) = completed_worker
                    else {
                        tracing::warn!(
                            "Worker has shut down uncleanly. Shutting down {} scheduler.",
                            self.worker.config().name
                        );
                        return;
                    };
                    self.handle_completed_worker(completed_worker, info);
                }
                request = expired_receiver.next() => {
                    let Some((request, expired)) = request else {
                        break;
                    };
                    request.expire(expired);
                }
                request = next_request_stream.next(),
                if !all_workers_busy => {
                    let Some((request, permit)) = request else {
                        break;
                    };
                    if request.is_response_closed() {
                        continue;
                    }
                    let worker_id = match self.get_worker(&request) {
                        Ok(worker_id) => worker_id,
                        Err(reason) => {
                            request.reject(reason);
                            continue;
                        },
                    };
                    let (done_sender, done_receiver) = oneshot::channel();
                    let st = ActiveWorkerState {
                        client_id: request.client_id.clone(),
                        worker_id,
                    };
                    self.in_progress_workers
                        .push(future::join(done_receiver, future::ready(st)));
                    let entry = self
                        .in_progress_count
                        .entry(request.client_id.clone())
                        .or_default();
                    *entry += 1;
                    self.active_workers.fetch_add(1, Ordering::Relaxed);
                    log_pool_running_count(self.worker.config().name, *entry, &request.client_id);
                    if self.worker_senders[worker_id]
                        .try_send((request, permit, done_sender))
                        .is_err()
                    {
                        // Available worker should have an empty channel, so if we fail
                        // here it must be shut down. We should shut down too.
                        tracing::warn!(
                            "Worker died or dropped channel. Shutting down {} scheduler.",
                            self.worker.config().name
                        );
                        return;
                    }
                },
                _ = &mut report_stats => {
                    let heap_stats = self.aggregate_heap_stats();
                    log_aggregated_heap_stats(&heap_stats);
                    report_stats = self.rt.wait(*HEAP_WORKER_REPORT_INTERVAL_SECONDS);
                },
            }
        }
        tracing::warn!(
            "Request sender went away; {} scheduler shutting down",
            self.worker.config().name
        );
    }

    /// Find a worker for the given `client_id`.`
    /// Returns an error if no worker can be allocated for this client.
    ///
    /// Note that the returned worker id is removed from the
    /// `self.available_workers` state, so the caller is responsible for using
    /// the worker and returning it back to `self.available_workers` after it is
    /// done.
    fn get_worker(
        &mut self,
        request: &Request<RT>,
    ) -> Result<usize, RejectedBeforeExecutionReason> {
        let client_id = request.client_id.as_str();
        // Make sure this client isn't overloading the scheduler.
        let active_worker_count = self
            .in_progress_count
            .get(client_id)
            .copied()
            .unwrap_or_default();
        if active_worker_count >= self.max_active_workers_per_client {
            tracing::warn!(
                "Client {} is using >= {} of scheduler capacity; rejecting new request",
                client_id,
                self.max_active_workers_per_client,
            );
            return Err(RejectedBeforeExecutionReason::PerClientWorkerOverloaded);
        }
        // Try to find an existing worker for this client.
        if let Some((client_id, mut workers)) = self.available_workers.remove_entry(client_id) {
            // If there is a worker with an appropriate reusable context, pick that one
            // first.
            // This skips workers with inapplicable reused contexts.
            // TODO: just promote the saved context's module path into the hashmap key.
            let worker = workers
                .extract_if(.., |worker| {
                    worker.info.cached_contexts.can_serve_request(request)
                })
                .next();
            if !workers.is_empty() {
                self.available_workers.insert(client_id, workers);
            }
            if let Some(worker) = worker {
                return Ok(worker.worker_id);
            }
            // Otherwise all the workers have cached contexts for other modules
            // that we don't want to clobber; try to assign a new worker
            // instead.
            // It's possible that one of our own workers will end up being the
            // least-recently-used one.
        }
        // If we've recently started up and haven't yet created `max_workers` threads,
        // create a new worker instead of "stealing" some other client's worker.
        if self.worker_senders.len() < self.max_workers {
            let new_worker = self.worker.clone();
            let heap_stats = SharedIsolateHeapStats::new();
            let heap_stats_ = heap_stats.clone();
            let (work_sender, work_receiver) = mpsc::channel(1);
            let handle = self.rt.spawn_thread("isolate", move || {
                new_worker.service_requests(work_receiver, heap_stats_)
            });
            self.worker_senders.push(work_sender);
            self.handles
                .lock()
                .push(IsolateWorkerHandle { handle, heap_stats });
            tracing::info!(
                "Created {} isolate worker {}",
                self.worker.config().name,
                self.worker_senders.len() - 1
            );
            return Ok(self.worker_senders.len() - 1);
        }
        // No existing worker for this client and we've already started the max number
        // of workers -- just grab the least recently used worker. This worker is least
        // likely to be reused by its' previous client.
        let Some((key, workers)) = self
            .available_workers
            .iter_mut()
            .min_by_key(|(_, workers)| {
                workers
                    .back()
                    .expect("Available worker map should never contain an empty list")
                    .last_used_ts
            })
        else {
            // No available workers. This should be unreachable since we don't
            // pull a request from the queue until there is a free worker.
            tracing::error!("unexpected: couldn't find a worker?");
            return Err(RejectedBeforeExecutionReason::WorkerPoolOverloaded);
        };
        let worker = workers
            .pop_back()
            .expect("Available worker map should never contain an empty list");
        log_worker_stolen(worker.last_used_ts.elapsed());
        if workers.is_empty() {
            // This variable shadowing drops the mutable reference to
            // `self.available_workers`.
            let key = key.clone();
            self.available_workers.remove(&key);
        }
        Ok(worker.worker_id)
    }

    fn aggregate_heap_stats(&self) -> IsolateHeapStats {
        let mut total = IsolateHeapStats::default();
        for handle in self.handles.lock().iter() {
            total += handle.heap_stats.get();
        }
        total
    }
}

pub struct IsolateWorkerHandle {
    pub handle: Box<dyn SpawnHandle>,
    heap_stats: SharedIsolateHeapStats,
}

#[derive(Clone)]
pub struct SharedIsolateHeapStats(Arc<Mutex<IsolateHeapStats>>);

impl SharedIsolateHeapStats {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Mutex::new(IsolateHeapStats::default())))
    }

    pub(crate) fn get(&self) -> IsolateHeapStats {
        *self.0.lock()
    }

    pub fn store(&self, stats: IsolateHeapStats) {
        *self.0.lock() = stats;
    }
}

#[async_trait(?Send)]
pub trait IsolateWorker<RT: Runtime>: Clone + Send + 'static {
    async fn service_requests(
        self,
        reqs: mpsc::Receiver<(
            Request<RT>,
            ConcurrencyPermit,
            oneshot::Sender<IdleWorkerInfo>,
        )>,
        heap_stats: SharedIsolateHeapStats,
    ) {
        let IsolateConfig {
            max_user_timeout, ..
        } = self.config();
        let mut reqs = std::pin::pin!(ReceiverStream::new(reqs).peekable());
        let mut ready: Option<oneshot::Sender<_>> = None;
        'recreate_isolate: loop {
            let mut last_client_id: Option<String> = None;
            let mut last_request: Option<String> = None;
            let mut isolate = Isolate::new(
                self.rt().clone(),
                *max_user_timeout,
                *ISOLATE_MAX_USER_HEAP_SIZE,
            );
            isolate.set_heap_stats_handle(heap_stats.clone());
            let mut context_cache = ContextCache::new();
            heap_stats.store(isolate.heap_stats());
            loop {
                context_cache.prepare(isolate.isolate());
                // Check again whether the isolate has enough free heap memory
                // before starting the next request
                if let Some(debug_str) = &last_request
                    && should_recreate_isolate(&mut isolate, &mut context_cache, debug_str)
                {
                    continue 'recreate_isolate;
                }
                heap_stats.store(isolate.heap_stats());
                if let Some(done) = ready.take() {
                    // Inform the scheduler that this thread is ready to accept a new request.
                    let _ = done.send(IdleWorkerInfo {
                        cached_contexts: context_cache.cached_contexts().clone(),
                    });
                }
                tokio::select! {
                    // If the isolate isn't "tainted", no need to wait for the idle timeout.
                    _ = self.rt().wait(*ISOLATE_IDLE_TIMEOUT), if last_client_id.is_some() => {
                        tracing::debug!("Restarting isolate for {last_client_id:?} due to idle timeout");
                        metrics::log_recreate_isolate("idle_timeout");
                        continue 'recreate_isolate;
                    },
                    // First peek the request to decide if we need to make a new isolate.
                    req = reqs.as_mut().peek() => {
                        let Some((req, ..)) = req else {
                            return;
                        };
                        let reused = last_client_id.is_some();
                        // If we receive a request from a different client (i.e. a different instance),
                        // recreate the isolate. We don't allow an isolate to be reused
                        // across clients for security isolation.
                        if last_client_id.get_or_insert_with(|| {
                            req.client_id.clone()
                        }) != &req.client_id {
                            let pause_client = self.rt().pause_client();
                            pause_client.wait(PAUSE_RECREATE_CLIENT).await;
                            tracing::debug!("Restarting isolate due to client change, previous: {:?}, new: {:?}", last_client_id, req.client_id);
                            metrics::log_recreate_isolate("client_id_changed");
                            continue 'recreate_isolate;
                        } else if reused {
                            tracing::debug!("Reusing isolate for client {}", req.client_id);
                        }
                        // Ok, we're ready to accept the request for real.
                        let Some((req, permit, done)) = reqs.next().await else { return };
                        // Note that we won't reply to `done` until
                        // `context_cache` has been prepared. This improves
                        // latency in the common case since requests will be
                        // routed to a thread that has a context ready to go.
                        ready = Some(done);
                        let root = initialize_root_from_parent(
                            func_path!(),
                            req.parent_trace.clone(),
                        );
                        root.add_property(|| ("reused_isolate", reused.as_label()));
                        let (debug_str, isolate_clean) = self
                            .handle_request(
                                &mut isolate,
                                &mut context_cache,
                                req,
                                permit,
                            )
                            .in_span(root)
                            .await;
                        if !isolate_clean {
                            continue 'recreate_isolate;
                        }
                        last_request = Some(debug_str);
                    }
                }
            }
        }
    }

    #[fastrace::trace]
    async fn handle_request(
        &self,
        isolate: &mut Isolate<RT>,
        context_cache: &mut ContextCache,
        request: Request<RT>,
        permit: ConcurrencyPermit,
    ) -> (String, bool) {
        let pause_client = self.rt().pause_client();
        pause_client.wait(PAUSE_REQUEST).await;
        let client_id = &request.client_id;
        // Set the scope to be tagged with the client_id just for the duration of
        // handling the request. It would be nice to get sentry::with_scope to work, but
        // it uses a synchronous callback and we need `report_error` in the future to
        // have the client_id tag.
        sentry::configure_scope(|scope| scope.set_tag("client_id", client_id));
        // Also add the tag to tracing so it shows up in DataDog logs.
        let span = tracing::info_span!("isolate_worker_handle_request", instance_name = client_id);
        let result = self
            .handle_request_inner(isolate, context_cache, request, permit)
            .instrument(span)
            .await;
        sentry::configure_scope(|scope| scope.remove_tag("client_id"));
        result
    }

    async fn handle_request_inner(
        &self,
        isolate: &mut Isolate<RT>,
        context_cache: &mut ContextCache,
        request: Request<RT>,
        permit: ConcurrencyPermit,
    ) -> (String, bool);

    fn config(&self) -> &IsolateConfig;
    fn rt(&self) -> &RT;
}

pub fn should_recreate_isolate<RT: Runtime>(
    isolate: &mut Isolate<RT>,
    context_cache: &mut ContextCache,
    last_executed: &str,
) -> bool {
    if !*REUSE_ISOLATES {
        metrics::log_recreate_isolate("env_disabled");
        return true;
    }
    if let Err(e) = isolate.check_isolate_clean(context_cache) {
        tracing::debug!(
            "Restarting Isolate {}: {e:?}, last request: {last_executed:?}",
            e.reason()
        );
        metrics::log_recreate_isolate(e.reason());
        LocalSpan::add_event(
            Event::new("isolate_unclean")
                .with_property(|| ("reason", e.reason()))
                .with_property(|| ("last_executed", last_executed.to_owned())),
        );
        return true;
    }

    if isolate.created().elapsed() > *ISOLATE_MAX_LIFETIME {
        metrics::log_recreate_isolate("max_lifetime");
        return true;
    }

    false
}
