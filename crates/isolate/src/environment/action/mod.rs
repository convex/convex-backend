mod async_syscall;
mod fetch;
mod phase;
pub(crate) mod service_token;
mod storage;
mod stream;
mod syscall;
pub(crate) mod task;
pub(crate) mod task_executor;
mod task_order;

use std::{
    cmp::Ordering,
    collections::BTreeMap,
    marker::PhantomData,
    sync::{
        Arc,
        OnceLock,
    },
    time::Duration,
};

use anyhow::{
    anyhow,
    Context as _,
};
use common::{
    components::{
        ComponentId,
        ComponentPath,
    },
    errors::JsError,
    execution_context::ExecutionContext,
    fastrace_helpers::EncodedSpan,
    http::{
        fetch::FetchClient,
        RoutedHttpPath,
    },
    knobs::{
        FUNCTION_MAX_ARGS_SIZE,
        FUNCTION_MAX_RESULT_SIZE,
        MAX_LOG_LINES_V8_ACTIONS,
        V8_ACTION_SYSTEM_TIMEOUT,
        V8_ACTION_USER_TIMEOUT,
    },
    log_lines::{
        LogLevel,
        LogLine,
        SystemLogMetadata,
    },
    runtime::{
        Runtime,
        SpawnHandle,
        UnixTimestamp,
    },
    sync::spsc,
    types::{
        HttpActionRoute,
        UdfType,
    },
    value::ConvexValue,
};
use database::Transaction;
use deno_core::v8::{
    self,
    scope,
};
use errors::ErrorMetadata;
use futures::{
    future::BoxFuture,
    select_biased,
    stream::BoxStream,
    FutureExt,
    Stream,
    StreamExt,
    TryStreamExt,
};
use http::StatusCode;
use humansize::{
    FormatSize,
    BINARY,
};
use itertools::Itertools;
use keybroker::Identity;
use model::{
    environment_variables::types::{
        EnvVarName,
        EnvVarValue,
    },
    modules::user_error::FunctionNotFoundError,
};
use parking_lot::Mutex;
use rand_chacha::ChaCha12Rng;
use serde_json::Value as JsonValue;
use sync_types::{
    CanonicalizedUdfPath,
    ModulePath,
};
use tokio::sync::{
    mpsc,
    oneshot,
};
use udf::{
    helpers::parse_udf_args,
    validation::ValidatedHttpPath,
    warnings::{
        approaching_duration_limit_warning,
        approaching_limit_warning,
        SystemWarning,
    },
    ActionCallbacks,
    ActionOutcome,
    HttpActionOutcome,
    HttpActionRequest,
    HttpActionRequestHead,
    HttpActionResponseHead,
    HttpActionResponsePart,
    HttpActionResponseStreamer,
    HttpActionResult,
    SyscallTrace,
    HTTP_ACTION_RESPONSE_BODY_LIMIT,
};
use value::{
    heap_size::HeapSize,
    serialized_args_ext::SerializedArgsExt,
    ConvexArray,
    JsonPackedValue,
    NamespacedTableMapping,
    Size,
};

pub use self::{
    async_syscall::parse_name_or_reference,
    task::{
        FileResponse,
        FormPart,
        FormPartFile,
        TaskId,
        TaskRequestEnum,
        TaskResponse,
        TaskResponseEnum,
        TaskType,
    },
};
use self::{
    phase::ActionPhase,
    service_token::ServiceTokenCache,
    task::TaskRequest,
    task_executor::TaskExecutor,
};
use super::{
    crypto_rng::CryptoRng,
    ModuleCodeCacheResult,
};
use crate::{
    client::{
        ActionRequestParams,
        EnvironmentData,
    },
    context_cache::ContextCache,
    environment::{
        helpers::{
            log_overflow_log_line,
            module_loader::module_specifier_from_path,
            resolve_promise,
            resolve_promise_allow_all_errors,
        },
        AsyncOpRequest,
        JsEnvironment,
        OpProvider,
        SyscallProvider,
    },
    execution_scope::ExecutionScope,
    helpers::{
        self,
        deserialize_udf_result,
        pump_message_loop,
    },
    http::{
        HttpRequestV8,
        HttpResponseV8,
    },
    isolate::Isolate,
    metrics::{
        self,
        log_isolate_request_cancelled,
        log_unawaited_pending_op,
    },
    module_cache::V8ModuleSource,
    ops::V8OpProvider,
    request_scope::{
        RequestScope,
        StreamListener,
    },
    strings,
    termination::{
        ContextTerminationReason,
        ExecutionHandle,
        IsolateTerminationReason,
    },
    timeout::{
        FunctionExecutionTime,
        Timeout,
    },
    ConcurrencyPermit,
};

// `CollectResult` starts off as a future that is forever pending,
// so it never triggers the `select_biased!` until we are actually
// collecting a result. Using None would be nice, but `select_biased!`
// does not like Options.
struct CollectResult<'a, T: Send + 'a> {
    has_started: bool,
    result_stream: BoxStream<'a, anyhow::Result<Result<T, JsError>>>,
}

impl<'a, T: Send> CollectResult<'a, T> {
    fn new() -> Self {
        Self {
            has_started: false,
            result_stream: futures::stream::pending().boxed(),
        }
    }

    fn start(&mut self, stream: BoxStream<'a, anyhow::Result<Result<T, JsError>>>) {
        self.has_started = true;
        self.result_stream = stream;
    }
}

pub struct ActionSyscallProvider<RT: Runtime> {
    identity: Identity,
    total_log_lines: usize,
    log_line_sender: mpsc::UnboundedSender<LogLine>,
    http_response_streamer: Option<HttpActionResponseStreamer>,
    /// Set once a body chunk would push the response past
    /// `HTTP_ACTION_RESPONSE_BODY_LIMIT`. Chunks over the limit are dropped,
    /// so `total_bytes_sent` alone never exceeds the limit.
    http_response_too_large: bool,

    rt: RT,

    next_task_id: TaskId,
    pending_task_sender: spsc::UnboundedSender<TaskRequest>,

    running_tasks: Option<Box<dyn SpawnHandle>>,

    task_responses: mpsc::UnboundedReceiver<TaskResponse>,
    phase: ActionPhase<RT>,
    syscall_trace: Arc<Mutex<SyscallTrace>>,
    http_action_route: Arc<OnceLock<HttpActionRoute>>,
}

pub struct ActionEnvironment<RT: Runtime, S = ActionSyscallProvider<RT>> {
    provider: S,

    // We have to store PromiseResolvers separate from TaskRequests because
    // TaskRequests will be executed in parallel, but PromiseResolvers are not Send.
    task_promise_resolvers: BTreeMap<TaskId, (v8::Global<v8::PromiseResolver>, TaskType)>,
    _phantom: PhantomData<RT>,
}

impl<RT: Runtime> Drop for ActionSyscallProvider<RT> {
    fn drop(&mut self) {
        self.shutdown_tasks();
    }
}

/// The interface between an action's JS and the outside world
#[allow(async_fn_in_trait)]
pub trait ActionInnerProvider<RT: Runtime>: SyscallProvider<RT> + HeapSize + 'static {
    type Outcome;
    type HttpOutcome;

    fn rt(&self) -> &RT;
    fn identity(&self) -> &Identity;
    fn component(&self) -> ComponentId;
    fn syscall_trace(&self) -> SyscallTrace;

    async fn initialize(&mut self, timeout: &mut Timeout<RT>) -> anyhow::Result<()>;
    fn begin_execution(&mut self) -> anyhow::Result<()>;

    /// Hands `request` to the task executor, returning the ID that its
    /// `TaskResponse::TaskDone` will carry.
    fn start_task(&mut self, request: TaskRequestEnum) -> anyhow::Result<TaskId>;
    /// Forwards `stream` to the isolate as `TaskResponse::StreamExtend`s for
    /// `stream_id`.
    fn send_stream(
        &mut self,
        stream_id: uuid::Uuid,
        stream: Option<BoxStream<'static, anyhow::Result<bytes::Bytes>>>,
    ) -> anyhow::Result<()>;
    async fn next_task_response(&mut self) -> anyhow::Result<TaskResponse>;
    /// Stops running tasks and drops any that haven't started.
    fn shutdown_tasks(&mut self);

    /// Called only after a lookup succeeds. An unmatched route's path is the
    /// raw request path, which no token or usage record should keep.
    fn set_http_action_route(&self, route: HttpActionRoute);
    /// Resolves when the client that requested the HTTP action goes away.
    fn http_response_closed(&self) -> anyhow::Result<BoxFuture<'static, ()>>;
    fn http_response_has_started(&self) -> anyhow::Result<bool>;
    fn handle_http_streamed_part(
        &mut self,
        part: Result<HttpActionResponsePart, JsError>,
    ) -> anyhow::Result<()>;
    fn add_http_response_size_warning(&mut self) -> anyhow::Result<()>;

    fn trace_system(&mut self, warning: SystemWarning) -> anyhow::Result<()>;

    fn into_outcome(self, outcome: ActionOutcome) -> Self::Outcome;
    fn into_http_outcome(self, outcome: HttpActionOutcome) -> Self::HttpOutcome;
}

impl<RT: Runtime> ActionEnvironment<RT> {
    pub fn new(
        rt: RT,
        component: ComponentId,
        udf_path: CanonicalizedUdfPath,
        component_path: ComponentPath,
        EnvironmentData {
            key_broker,
            default_system_env_vars,
            file_storage,
            module_loader,
            deployment,
        }: EnvironmentData<RT>,
        identity: Identity,
        transaction: Transaction<RT>,
        action_callbacks: Arc<dyn ActionCallbacks>,
        fetch_client: Arc<dyn FetchClient>,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
        http_response_streamer: Option<HttpActionResponseStreamer>,
        context: ExecutionContext,
    ) -> Self {
        let syscall_trace = Arc::new(Mutex::new(SyscallTrace::new()));
        let (task_retval_sender, task_responses) = mpsc::unbounded_channel();
        let resources = Arc::new(Mutex::new(BTreeMap::new()));
        let convex_origin_override = Arc::new(Mutex::new(None));
        let http_action_route = Arc::new(OnceLock::new());
        let task_executor = TaskExecutor {
            rt: rt.clone(),
            identity: identity.clone(),
            file_storage,
            syscall_trace: syscall_trace.clone(),
            action_callbacks,
            fetch_client,
            _module_loader: module_loader.clone(),
            key_broker,
            task_order: Default::default(),
            task_retval_sender,
            usage_tracker: transaction.usage_tracker.clone(),
            context,
            resources: resources.clone(),
            component_id: component,
            udf_path,
            component_path,
            convex_origin_override: convex_origin_override.clone(),
            http_action_route: http_action_route.clone(),
            deployment,
            service_token: Arc::new(ServiceTokenCache::default()),
        };
        let (pending_task_sender, pending_task_receiver) = spsc::unbounded_channel();
        let running_tasks = rt.spawn("task_executor", task_executor.go(pending_task_receiver));
        Self {
            provider: ActionSyscallProvider {
                identity,
                rt: rt.clone(),
                total_log_lines: 0,
                log_line_sender,
                http_response_streamer,
                http_response_too_large: false,

                next_task_id: TaskId(0),
                pending_task_sender,
                task_responses,
                running_tasks: Some(running_tasks),
                phase: ActionPhase::new(
                    rt.clone(),
                    component,
                    transaction,
                    module_loader,
                    default_system_env_vars,
                    resources,
                    convex_origin_override,
                ),
                syscall_trace,
                http_action_route,
            },
            task_promise_resolvers: BTreeMap::new(),
            _phantom: PhantomData,
        }
    }
}

impl<RT: Runtime, S: ActionInnerProvider<RT>> ActionEnvironment<RT, S> {
    pub fn from_provider(provider: S) -> Self {
        Self {
            provider,
            task_promise_resolvers: BTreeMap::new(),
            _phantom: PhantomData,
        }
    }

    pub fn provider(&self) -> &S {
        &self.provider
    }

    pub fn into_provider(self) -> S {
        self.provider
    }

    /// Replaces the syscall provider, e.g. to wrap it in one that records the
    /// action's inputs.
    pub fn map_provider<S2: ActionInnerProvider<RT>>(
        self,
        f: impl FnOnce(S) -> S2,
    ) -> ActionEnvironment<RT, S2> {
        ActionEnvironment {
            provider: f(self.provider),
            task_promise_resolvers: self.task_promise_resolvers,
            _phantom: PhantomData,
        }
    }

    #[fastrace::trace]
    pub async fn run_http_action(
        mut self,
        isolate: &mut Isolate<RT>,
        context_cache: &mut ContextCache,
        permit: ConcurrencyPermit,
        isolate_clean: &mut bool,
        http_module_path: ValidatedHttpPath,
        routed_path: RoutedHttpPath,
        request: HttpActionRequest,
        function_started: Option<oneshot::Sender<()>>,
    ) -> anyhow::Result<S::HttpOutcome> {
        let start_unix_timestamp = self.provider.rt().unix_timestamp();

        // Double check that we correctly initialized `ActionEnvironment` with the right
        // component path and then pass a bare `CanonicalizedUdfPath` to
        // `run_http_action_inner`.
        let component_function_path = http_module_path.path();
        anyhow::ensure!(component_function_path.component == self.provider.component());
        let udf_path = &component_function_path.udf_path;

        let (handle, state, mut timeout) =
            isolate.start_request(context_cache, permit, self).await?;
        if let Some(tx) = function_started {
            _ = tx.send(());
        }
        scope!(let handle_scope, isolate.isolate());
        let v8_context = context_cache.get_or_create_fresh_context(handle_scope);
        let context_scope = &mut v8::ContextScope::new(handle_scope, v8_context);

        let mut isolate_context = RequestScope::new(context_scope, handle.clone(), state, true)?;

        let request_head = request.head.clone();

        let mut result = Self::run_http_action_inner(
            &mut isolate_context,
            &mut timeout,
            udf_path,
            routed_path,
            request,
        )
        .await;
        // Override the returned result if we hit a termination error.
        let termination_error = handle.take_termination_error(&format!("http action: {udf_path}"));

        // Perform a microtask checkpoint one last time before taking the environment
        // to ensure the microtask queue is empty. Otherwise, JS from this request may
        // leak to a subsequent one on isolate reuse.
        isolate_context.checkpoint();
        *isolate_clean = true;

        self = isolate_context.take_environment();
        let execution_time = timeout.into_function_execution_time(UdfType::HttpAction);
        match termination_error {
            Ok(Ok(..)) => (),
            Ok(Err(e)) => {
                // Preserve the route from the inner result if lookup already
                // succeeded, so usage tracking reports the parameterized route
                // pattern instead of the raw URL path.
                let route = result.as_ref().ok().and_then(|(route, _)| route.clone());
                if !self.provider.http_response_has_started()? {
                    result = Ok((route, HttpActionResult::Error(e)));
                } else {
                    self.provider.handle_http_streamed_part(Err(e))?;
                    result = Ok((route, HttpActionResult::Streamed))
                }
            },
            Err(e) => {
                result = Err(e);
            },
        }
        let user_execution_time = execution_time.elapsed;
        self.provider.add_http_response_size_warning()?;
        let dangling_task_counts = self.dangling_task_counts();
        add_warnings_to_log_lines(&mut self.provider, dangling_task_counts, execution_time)?;
        let (route, result) = result?;
        let outcome = HttpActionOutcome::new(
            route,
            request_head,
            self.provider.identity().clone().into(),
            start_unix_timestamp,
            result,
            Some(self.provider.syscall_trace()),
            http_module_path.npm_version().clone(),
            user_execution_time,
        );
        Ok(self.provider.into_http_outcome(outcome))
    }

    #[fastrace::trace]
    #[convex_macro::instrument_future]
    async fn run_http_action_inner(
        isolate: &mut RequestScope<'_, '_, '_, RT, Self>,
        timeout: &mut Timeout<RT>,
        http_module_path: &CanonicalizedUdfPath,
        routed_path: RoutedHttpPath,
        http_request: HttpActionRequest,
    ) -> anyhow::Result<(Option<HttpActionRoute>, HttpActionResult)> {
        let handle = isolate.handle();
        scope!(let v8_scope, isolate.scope());
        let mut scope = RequestScope::<RT, Self>::enter(v8_scope);

        {
            let state = scope.state_mut()?;
            state.environment.provider.initialize(timeout).await?;
        }

        /*
         * Running an HTTP handler is a two-step process.
         * 1) Call `router.lookup()` to find the route name.
         * 2) Call `router.runRequest()` to execute the request.
         *
         * It is the responsibility of the JavaScript `Router` object
         * to ensure that `router.runRequest()` actually routes the request
         * to the same route reported by `router.lookup()`, i.e. it should
         * use `lookup()` in its implementation.
         *
         * The JavaScript `Router` object is application code and cannot be
         * updated after a developer pushes code to a deployment. New NPM packages
         * can implement new behavior in `Router` (and developers can even
         * implement their own `Routers` although this is not recommended)
         * but this interface must be backward compatible.
         */
        let router: Result<_, JsError> =
            Self::get_router(&mut scope, timeout, http_module_path.clone()).await?;

        if let Err(e) = router {
            return Ok((None, HttpActionResult::Error(e)));
        };
        let router = router?;

        let route_lookup = Self::lookup_route(
            &mut scope,
            &router,
            http_module_path.clone(),
            routed_path.clone(),
            http_request.head.clone(),
        )?;
        let route = match route_lookup {
            None => {
                handle.check_terminated()?;
                let state = scope.state_mut()?;
                let provider = &mut state.environment.provider;
                let response_parts = HttpActionResponsePart::from_text(
                    StatusCode::NOT_FOUND,
                    "No matching routes found".into(),
                );
                for part in response_parts {
                    provider.handle_http_streamed_part(Ok(part))?;
                }
                return Ok((None, HttpActionResult::Streamed));
            },
            Some(route) => route,
        };
        scope
            .state_mut()?
            .environment
            .provider
            .set_http_action_route(route.clone());

        let run_str = strings::runRequest.create(&scope)?.into();
        let v8_function: v8::Local<v8::Function> = router
            .get(&scope, run_str)
            .ok_or_else(|| {
                anyhow!(
                    "Couldn't find runRequest method of router in {:?}",
                    http_module_path
                )
            })?
            .try_into()?;

        let stream_id = match http_request.body {
            Some(body) => {
                let stream_id = scope.state_mut()?.create_request_stream()?;
                scope
                    .state_mut()?
                    .environment
                    .provider
                    .send_stream(stream_id, Some(body))?;
                Some(stream_id)
            },
            None => None,
        };
        let signal = Self::signal_http_action_abort(&mut scope)?;
        let request_str = serde_json::to_value(HttpRequestV8::from_request(
            http_request.head,
            stream_id,
            signal,
        )?)?
        .to_string();
        metrics::log_argument_length(&request_str);
        let args_v8_str = v8::String::new(&scope, &request_str)
            .ok_or_else(|| anyhow!("Failed to create argument string"))?;

        // Pass in `request_route` as a second argument so old clients can ignore it if
        // they're not component aware.
        let request_route_v8_str = v8::String::new(&scope, &routed_path)
            .ok_or_else(|| anyhow!("Failed to create request route string"))?;

        let v8_args = [args_v8_str.into(), request_route_v8_str.into()];

        let result = Self::run_inner(
            &mut scope,
            timeout,
            handle,
            UdfType::HttpAction,
            v8_function,
            &v8_args,
            Box::pin(futures::future::pending()),
            Self::stream_http_result,
            |environment, part| environment.provider.handle_http_streamed_part(part),
        )
        .await?;
        match result {
            Ok(()) => Ok((Some(route), HttpActionResult::Streamed)),
            Err(e) => Ok((Some(route), HttpActionResult::Error(e))),
        }
    }

    // AbortSignal passed to HTTP action request is implemented as a
    // ReadableStream which gets closed when the client requesting the HTTP action
    // goes away.
    fn signal_http_action_abort(
        scope: &mut ExecutionScope<'_, '_, '_, RT, Self>,
    ) -> anyhow::Result<uuid::Uuid> {
        let state = scope.state_mut()?;
        let sender_closed_fut = state.environment.provider.http_response_closed()?;
        let sender_closed =
            Box::pin(futures::stream::once(sender_closed_fut).filter_map(|_| async move { None }));
        let stream_id = state.create_request_stream()?;
        state
            .environment
            .provider
            .send_stream(stream_id, Some(sender_closed))?;

        Ok(stream_id)
    }

    fn stream_http_result(
        scope: &mut ExecutionScope<'_, '_, '_, RT, Self>,
        result_str: String,
    ) -> anyhow::Result<
        impl Stream<Item = anyhow::Result<Result<HttpActionResponsePart, JsError>>> + use<RT, S>,
    > {
        let json_value: JsonValue = serde_json::from_str(&result_str)?;
        let v8_response: HttpResponseV8 = serde_json::from_value(json_value)?;
        let (raw_response, stream_id) = v8_response.into_response()?;
        let (body_sender, body_receiver) = spsc::unbounded_channel();
        match stream_id {
            Some(stream_id) => {
                scope.new_stream_listener(stream_id, StreamListener::RustStream(body_sender))?
            },
            None => drop(body_sender),
        };
        let head = futures::stream::once(async move {
            Ok(Ok(HttpActionResponsePart::Head(HttpActionResponseHead {
                status: raw_response.status,
                headers: raw_response.headers,
            })))
        });

        Ok(head.chain(
            body_receiver
                .into_stream()
                .map_ok(|b| Ok(HttpActionResponsePart::BodyChunk(b))),
        ))
    }

    #[fastrace::trace]
    pub async fn run_action(
        mut self,
        isolate: &mut Isolate<RT>,
        context_cache: &mut ContextCache,
        permit: ConcurrencyPermit,
        isolate_clean: &mut bool,
        request_params: ActionRequestParams,
        cancellation: BoxFuture<'_, ()>,
        function_started: Option<oneshot::Sender<()>>,
    ) -> anyhow::Result<S::Outcome> {
        let start_unix_timestamp = self.provider.rt().unix_timestamp();

        let (handle, state, mut timeout) =
            isolate.start_request(context_cache, permit, self).await?;
        if let Some(tx) = function_started {
            _ = tx.send(());
        }
        scope!(let handle_scope, isolate.isolate());
        let v8_context = context_cache.get_or_create_fresh_context(handle_scope);
        let context_scope = &mut v8::ContextScope::new(handle_scope, v8_context);

        let mut isolate_context = RequestScope::new(context_scope, handle.clone(), state, true)?;
        let mut result = Self::run_action_inner(
            &mut isolate_context,
            &mut timeout,
            request_params.clone(),
            cancellation,
        )
        .await;

        // Perform a microtask checkpoint one last time before taking the environment
        // to ensure the microtask queue is empty. Otherwise, JS from this request may
        // leak to a subsequent one on isolate reuse.
        isolate_context.checkpoint();
        *isolate_clean = true;

        match handle.take_termination_error(&format!(
            "{:?}",
            request_params.path_and_args.path().clone().for_logging()
        )) {
            Ok(Ok(..)) => (),
            Ok(Err(e)) => {
                result = Ok(Err(e));
            },
            Err(e) => {
                result = Err(e);
            },
        }
        self = isolate_context.take_environment();
        let execution_time = timeout.into_function_execution_time(UdfType::Action);
        let dangling_task_counts = self.dangling_task_counts();
        finish_action(
            self.provider,
            dangling_task_counts,
            request_params,
            start_unix_timestamp,
            execution_time,
            result,
        )
    }

    #[fastrace::trace]
    async fn run_action_inner(
        isolate: &mut RequestScope<'_, '_, '_, RT, Self>,
        timeout: &mut Timeout<RT>,
        request_params: ActionRequestParams,
        cancellation: BoxFuture<'_, ()>,
    ) -> anyhow::Result<Result<ConvexValue, JsError>> {
        let handle = isolate.handle();
        scope!(let v8_scope, isolate.scope());
        let mut scope = RequestScope::<RT, Self>::enter(v8_scope);
        {
            let state = scope.state_mut()?;
            state.environment.provider.initialize(timeout).await?;
        }
        let (path, arguments, _) = request_params.path_and_args.consume();

        // Don't allow directly running a UDF within the `_deps` directory. We don't
        // really expect users to hit this unless someone is trying to exploit
        // an app written on Convex by calling directly into a compromised
        // dependency. So, consider it a system error so we can just
        // keep a watch on it.
        if path.udf_path.module().is_deps() {
            anyhow::bail!(
                "Refusing to run {:?} within the '_deps' directory",
                path.udf_path
            );
        }

        // First, load the user's module and find the specified function.
        let module_path = path.udf_path.module();
        let Ok(module_specifier) = module_specifier_from_path(module_path) else {
            let message = format!("Invalid module path: {module_path:?}");
            return Ok(Err(JsError::from_message(message)));
        };

        let module = match scope
            .eval_user_module(UdfType::Action, false, &module_specifier, timeout)
            .await?
        {
            Ok(id) => id,
            Err(e) => return Ok(Err(e)),
        };
        let namespace = module
            .get_module_namespace()
            .to_object(&scope)
            .ok_or_else(|| anyhow!("Module namespace wasn't an object?"))?;
        let function_name = path.udf_path.function_name();
        let function_str: v8::Local<'_, v8::Value> = v8::String::new(&scope, function_name)
            .ok_or_else(|| anyhow!("Failed to create function name string"))?
            .into();

        if namespace.has(&scope, function_str) != Some(true) {
            let message = format!(
                "{}",
                FunctionNotFoundError::new(function_name, path.udf_path.module().as_str())
            );
            return Ok(Err(JsError::from_message(message)));
        }
        let function: v8::Local<v8::Object> = namespace
            .get(&scope, function_str)
            .ok_or_else(|| anyhow!("Did not find function in module after checking?"))?
            .try_into()?;

        let run_str = strings::invokeAction.create(&scope)?.into();
        let v8_function: v8::Local<v8::Function> = function
            .get(&scope, run_str)
            .ok_or_else(|| anyhow!("Couldn't find invoke function in {:?}", path.udf_path))?
            .try_into()?;
        let args_str = arguments.get();
        metrics::log_argument_length(args_str);
        let args_v8_str = v8::String::new(&scope, args_str)
            .ok_or_else(|| anyhow!("Failed to create argument string"))?;
        // TODO(rebecca): generate uuid4 here
        let request_id_str = "dummy_request_id";
        let request_id_v8_str = v8::String::new(&scope, request_id_str)
            .ok_or_else(|| anyhow!("Failed to create request id string"))?;
        let v8_args = [request_id_v8_str.into(), args_v8_str.into()];

        let mut result = None;

        let run_inner_result = Self::run_inner(
            &mut scope,
            timeout,
            handle,
            UdfType::Action,
            v8_function,
            &v8_args,
            cancellation,
            |_, result_str| {
                let result = deserialize_udf_result(&path, &result_str)?;
                Ok(futures::stream::once(async move { Ok(result) }))
            },
            |_, r| {
                result = Some(r);
                Ok(())
            },
        )
        .await?;

        match run_inner_result {
            Ok(()) => (),
            Err(e) => result = Some(Err(e)),
        }
        result.ok_or_else(|| anyhow::anyhow!("`run_inner` did not populate a result"))
    }

    #[fastrace::trace]
    fn lookup_route(
        scope: &mut ExecutionScope<RT, Self>,
        router: &v8::Local<v8::Object>,
        http_module_path: CanonicalizedUdfPath,
        routed_path: RoutedHttpPath,
        http_request: HttpActionRequestHead,
    ) -> anyhow::Result<Option<HttpActionRoute>> {
        let lookup_str = strings::lookup.create(scope)?.into();
        let routed_path_str = v8::String::new(scope, &routed_path)
            .ok_or_else(|| anyhow!("Failed to create argument string"))?;
        let method_str = v8::String::new(scope, http_request.method.as_str())
            .ok_or_else(|| anyhow!("Failed to create argument string"))?;

        let lookup: v8::Local<v8::Function> = router
            .get(scope, lookup_str)
            .ok_or_else(|| {
                anyhow!(
                    "Couldn't find lookup method of router in {:?}",
                    http_module_path
                )
            })?
            .try_into()?;
        let global = scope.get_current_context().global(scope);
        let r = scope
            .with_try_catch(|s| {
                lookup.call(
                    s,
                    global.into(),
                    &[routed_path_str.into(), method_str.into()],
                )
            })??
            .expect("lookup.call() returned None");
        if r.is_null() {
            return Ok(None);
        }

        // function lookup(path: string, method: string): [handler, method, path] | null
        // Drop the handler result at index 0 of the return value on the floor here,
        // it is only part of the return type so that `runRequest` can use it when
        // it calls `lookup` from JavaScript.
        let lookup_result = r.to_object(scope).expect("lookup result");

        let route_method: v8::Local<v8::String> = lookup_result
            .get_index(scope, 1)
            .expect("Failed to get index 1 of lookup result")
            .try_into()?;
        let route_method_s = helpers::to_rust_string(scope, &route_method)?;
        let route_path: v8::Local<v8::String> = lookup_result
            .get_index(scope, 2)
            .expect("Failed to get index 2 of lookup result")
            .try_into()?;
        let route_path_s = helpers::to_rust_string(scope, &route_path)?;

        Ok(Some(format!("{route_method_s} {route_path_s}").parse()?))
    }

    async fn get_router<'s>(
        scope: &mut ExecutionScope<'_, 's, '_, RT, Self>,
        timeout: &mut Timeout<RT>,
        http_module_path: CanonicalizedUdfPath,
    ) -> anyhow::Result<Result<v8::Local<'s, v8::Object>, JsError>> {
        // Except in tests, `http.js` will always be the udf_path.
        // We'll never hit these as long as this HTTP path only runs for
        // `convex/http.js`.
        if http_module_path.module().is_deps() {
            anyhow::bail!("Refusing to run {http_module_path:?} within the '_deps' directory");
        }

        // First, load the user's module and find the specified function.
        let module_path = http_module_path.module().clone();
        let Ok(module_specifier) = module_specifier_from_path(&module_path) else {
            let message = format!("Invalid module path: {module_path:?}");
            return Ok(Err(JsError::from_message(message)));
        };

        let module = match scope
            .eval_user_module(UdfType::HttpAction, false, &module_specifier, timeout)
            .await?
        {
            Ok(id) => id,
            Err(e) => return Ok(Err(e)),
        };
        let namespace = module
            .get_module_namespace()
            .to_object(scope)
            .ok_or_else(|| anyhow!("Module namespace wasn't an object?"))?;
        let export_name = "default";
        let export_str: v8::Local<'_, v8::Value> = v8::String::new(scope, export_name)
            .ok_or_else(|| anyhow!("Failed to create function name string"))?
            .into();

        if namespace.has(scope, export_str) != Some(true) {
            let message = format!(
                r#"Couldn't find default export in module "{:?}"."#,
                http_module_path.module()
            );
            return Ok(Err(JsError::from_message(message)));
        }
        let router: v8::Local<v8::Object> = namespace
            .get(scope, export_str)
            .ok_or_else(|| anyhow!("Did not find router in module"))?
            .try_into()?;

        let is_router_str = strings::isRouter.create(scope)?.into();
        let mut is_router = false;
        if let Some(true) = router.has(scope, is_router_str) {
            is_router = router
                .get(scope, is_router_str)
                .ok_or_else(|| anyhow!("Missing `isRouter` after explicit check"))?
                .is_true();
        }

        if !is_router {
            let message = "The default export of `convex/http.js` is not a Router.".to_string();
            Ok(Err(JsError::from_message(message)))
        } else {
            Ok(Ok(router))
        }
    }

    /// This method is shared between HTTP and non-HTTP actions with
    /// functionality injected via `get_result_stream` and
    /// `handle_result_part`.
    ///
    /// In particular, HTTP actions allow streaming the response while normal
    /// actions do not.
    ///
    /// The outer `Result` in the return type holds any system errors while the
    /// inner `Result` holds any developer errors (JsErrors) that happen
    /// before collecting the result.
    ///
    /// Errors from collecting the result will be surfaced via
    /// `get_result_stream` -> `handle_result_part`
    #[fastrace::trace]
    async fn run_inner<'a, 's, 'i, T, St>(
        scope: &mut ExecutionScope<'a, 's, 'i, RT, Self>,
        timeout: &mut Timeout<RT>,
        handle: ExecutionHandle,
        udf_type: UdfType,
        v8_function: v8::Local<'_, v8::Function>,
        v8_args: &[v8::Local<'_, v8::Value>],
        cancellation: BoxFuture<'_, ()>,
        get_result_stream: impl FnOnce(
            &mut ExecutionScope<'a, 's, 'i, RT, Self>,
            String,
        ) -> anyhow::Result<St>,
        mut handle_result_part: impl FnMut(&mut Self, Result<T, JsError>) -> anyhow::Result<()>,
    ) -> anyhow::Result<Result<(), JsError>>
    where
        T: Send,
        St: Stream<Item = anyhow::Result<Result<T, JsError>>> + Send + 'static,
    {
        // Switch our phase to executing right before calling into the UDF.
        {
            let state = scope.state_mut()?;
            // This enforces on database access in the router.
            // We might relax this to e.g. implement a JavaScript router with
            // auth middleware which affected the matched route.
            state.environment.provider.begin_execution()?;
        }
        let global = scope.get_current_context().global(scope);
        let promise_r = scope.with_try_catch(|s| v8_function.call(s, global.into(), v8_args));
        // If we hit a system error within a syscall, return `Err`, even if JS thinks it
        // returned successfully. The syscall layer uses
        // `scope.terminate_execution()` when we hit a system error, which
        // unfortunately doesn't actually terminate execution immediately. So, it's
        // possible for JS after the failed syscall to keep running and return a result
        // here before checking the termination flag.
        handle.check_terminated()?;
        let promise: v8::Local<v8::Promise> = match promise_r? {
            Ok(Some(v)) => v.try_into()?,
            Ok(None) => anyhow::bail!("Successful invocation returned None"),
            Err(e) => {
                return Ok(Err(e));
            },
        };
        let mut get_result_stream = Some(get_result_stream);

        let mut collecting_result = CollectResult::new();
        let mut cancellation = cancellation.fuse();
        let result: Result<(), JsError> = loop {
            // Advance the user's promise as far as it can go by draining the microtask
            // queue.
            scope.perform_microtask_checkpoint();
            pump_message_loop(scope);
            scope.record_heap_stats(&handle)?;
            let request_stream_state = scope.state()?.request_stream_state.as_ref();
            if let Some(request_stream_state) = request_stream_state {
                handle.update_request_stream_bytes(request_stream_state.bytes_read());
            }
            handle.check_terminated()?;

            // Check for rejected promises still unhandled, if so terminate.
            let rejections = scope.pending_unhandled_promise_rejections_mut();
            if let Some(promise) = rejections.exceptions.keys().next().cloned() {
                let error = rejections.exceptions.remove(&promise).unwrap();

                let as_local = v8::Local::new(scope, error);
                let err = match scope.format_traceback(as_local) {
                    Ok(e) => e,
                    Err(e) => {
                        handle.terminate_and_throw(
                            IsolateTerminationReason::SystemError(Some(e)).into(),
                        )?;
                    },
                };
                handle.terminate_and_throw(
                    ContextTerminationReason::UnhandledPromiseRejection(err).into(),
                )?;
            }

            // Check for dynamic import requests.
            let dynamic_imports = {
                let pending_dynamic_imports = scope.pending_dynamic_imports_mut();
                pending_dynamic_imports.take()
            };
            if !dynamic_imports.is_empty() {
                for (specifier, resolver) in dynamic_imports {
                    match scope
                        .eval_user_module(udf_type, true, &specifier, timeout)
                        .await?
                    {
                        Ok(module) => {
                            let namespace = module.get_module_namespace();
                            resolve_promise(scope, resolver, Ok(namespace))?;
                        },
                        Err(e) => {
                            resolve_promise(scope, resolver, Err(anyhow::anyhow!(e)))?;
                        },
                    }
                }
                // Go back to the top and perform a microtask checkpoint now that we know we've
                // made progress.
                continue;
            }

            // Check to see if the user's code is blocked.
            match promise.state() {
                v8::PromiseState::Pending => (),
                v8::PromiseState::Fulfilled => {
                    // Call `collect_result` if we haven't already done so, and advance the future
                    // `collecting_result` it returns. If the future is pending,
                    // proceed as if the js future hasn't resolved.
                    // If the future is done, that's our result.
                    if let Some(get_result_stream) = get_result_stream.take() {
                        let promise_result_v8 = promise.result(scope);
                        let result_v8_str: v8::Local<v8::String> = promise_result_v8.try_into()?;
                        let result_str = helpers::to_rust_string(scope, &result_v8_str)?;
                        metrics::log_result_length(&result_str);
                        collecting_result.start(get_result_stream(scope, result_str)?.boxed());
                        // collect_result may have fulfilled promises, so we can go back to
                        // JS now.
                        continue;
                    }
                },
                v8::PromiseState::Rejected => {
                    let e = promise.result(scope);
                    let err = scope.format_traceback(e)?;
                    if collecting_result.has_started {
                        let state = scope.state_mut()?;
                        let environment = &mut state.environment;
                        handle_result_part(environment, Err(err))?;
                        break Ok(());
                    } else {
                        break Err(err);
                    }
                },
            };

            // If the user's promise is blocked, something must be pending:
            // 1. An async syscall, in which case we can execute one syscall before
            //    reentering into JS.
            // 2. Collecting_result, so we try to advance that.
            // 3. In case collecting_result or syscalls are taking too long or deadlocked,
            //    we should timeout.

            let timeout_future = timeout.wait_until_completed();
            // Release the permit while we wait on task executor.
            let permit = timeout
                .permit
                .take()
                .ok_or_else(|| anyhow::anyhow!("Running function without permit"))?;
            let regain_permit = permit.suspend();

            let environment = &mut scope.state_mut()?.environment;
            select_biased! {
                result = collecting_result.result_stream.next().fuse() => {
                    match result {
                        None => break Ok(()),
                        Some(inner_result) => {
                            handle_result_part(environment, inner_result?)?;
                        }
                    }
                },
                // Normally we'd pause the user-code timeout for the duration of
                // the syscall.
                // However, actions can call queries, mutations, and other actions
                // as syscalls, so these should still count towards the user-code
                // timeout.
                task_response = environment.provider.next_task_response().fuse() => {
                    match task_response? {
                        TaskResponse::StreamExtend { stream_id, chunk } => {
                            match chunk {
                                Ok(chunk) => {
                                    let done = chunk.is_none();
                                    scope.extend_stream(stream_id, chunk, done)?;
                                    // If done, add the total accumulated size to the isolate handle inner.
                                },
                                Err(e) => scope.error_stream(stream_id, e)?,
                            };
                        },
                        TaskResponse::TaskDone { task_id, variant } => {
                            let Some((resolver, _)) = environment
                                .task_promise_resolvers
                                .remove(&task_id) else {
                                    anyhow::bail!("Task with id {} did not have a promise", task_id);
                                };
                            scope!(let result_scope, &mut **scope);
                            let result_v8 = match variant {
                                Ok(v) => Ok(v.into_v8(result_scope)?),
                                Err(e) => Err(e),
                            };
                            resolve_promise_allow_all_errors(
                                result_scope,
                                resolver,
                                result_v8,
                            )?;
                        },
                    };
                },
                // If we the isolate is terminated due to timeout, we start the
                // isolate loop over to run js to handle the timeout.
                _ = timeout_future.fuse() => {
                    continue;
                },
                _ = cancellation => {
                    log_isolate_request_cancelled();
                    anyhow::bail!("Cancelled");
                },
            }
            let permit = timeout.with_timeout()(regain_permit.acquire()).await?;
            timeout.permit = Some(permit);
            handle.check_terminated()?;
        };
        // Drain all remaining async syscalls that are not sleeps in case the
        // developer forgot to await them.
        scope.state_mut()?.environment.provider.shutdown_tasks();
        Ok(result)
    }

    fn dangling_task_counts(&self) -> BTreeMap<String, usize> {
        dangling_task_counts(self.task_promise_resolvers.values().map(|(_, task)| task))
    }

    fn start_task(
        &mut self,
        request: TaskRequestEnum,
        resolver: v8::Global<v8::PromiseResolver>,
    ) -> anyhow::Result<()> {
        let task_type = request.to_type();
        let task_id = self.provider.start_task(request)?;
        self.task_promise_resolvers
            .insert(task_id, (resolver, task_type));
        Ok(())
    }
}

pub fn finish_action<RT: Runtime, S: ActionInnerProvider<RT>>(
    mut provider: S,
    dangling_task_counts: BTreeMap<String, usize>,
    request_params: ActionRequestParams,
    start_unix_timestamp: UnixTimestamp,
    execution_time: FunctionExecutionTime,
    result: anyhow::Result<Result<ConvexValue, JsError>>,
) -> anyhow::Result<S::Outcome> {
    let user_execution_time = execution_time.elapsed;
    let (path, arguments, udf_server_version) = request_params.path_and_args.consume();
    // Arguments that don't parse have already failed the action before it did
    // any work, so there's nothing to warn about.
    if let Ok(udf_args) = parse_udf_args(&path.udf_path, arguments.clone().into_args()?) {
        add_warnings_to_log_lines_action(
            &mut provider,
            dangling_task_counts,
            execution_time,
            &udf_args,
            result.as_ref().ok().and_then(|r| r.as_ref().ok()),
        )?;
    }
    let outcome = ActionOutcome {
        path: path.for_logging(),
        arguments,
        unix_timestamp: start_unix_timestamp,
        identity: provider.identity().clone().into(),
        result: match result? {
            Ok(v) => Ok(JsonPackedValue::pack(v)),
            Err(e) => Err(e),
        },
        syscall_trace: provider.syscall_trace(),
        udf_server_version,
        user_execution_time: Some(user_execution_time),
    };
    Ok(provider.into_outcome(outcome))
}

fn add_warnings_to_log_lines_action<RT: Runtime, S: ActionInnerProvider<RT>>(
    provider: &mut S,
    dangling_task_counts: BTreeMap<String, usize>,
    execution_time: FunctionExecutionTime,
    arguments: &ConvexArray,
    result: Option<&ConvexValue>,
) -> anyhow::Result<()> {
    if let Some(warning) = approaching_limit_warning(
        arguments.size(),
        *FUNCTION_MAX_ARGS_SIZE,
        "FunctionArgumentsTooLarge",
        || "Large size of the action arguments".to_string(),
        None,
        Some(" bytes"),
        None,
    ) {
        provider.trace_system(warning)?;
    }
    add_warnings_to_log_lines(provider, dangling_task_counts, execution_time)?;

    if let Some(result) = result
        && let Some(warning) = approaching_limit_warning(
            result.size(),
            *FUNCTION_MAX_RESULT_SIZE,
            "TooLargeFunctionResult",
            || "Large size of the action return value".to_string(),
            None,
            Some(" bytes"),
            None,
        )
    {
        provider.trace_system(warning)?;
    };
    Ok(())
}

fn add_warnings_to_log_lines<RT: Runtime, S: ActionInnerProvider<RT>>(
    provider: &mut S,
    dangling_task_counts: BTreeMap<String, usize>,
    execution_time: FunctionExecutionTime,
) -> anyhow::Result<()> {
    if !dangling_task_counts.is_empty() {
        let total_dangling_tasks = dangling_task_counts.values().sum();
        let task_names = dangling_task_counts.keys().join(", ");
        log_unawaited_pending_op(total_dangling_tasks, "action");
        let message = format!(
            "{total_dangling_tasks} unawaited operation{}: [{task_names}]. Async operations should be awaited or they might not run. \
             See https://docs.convex.dev/functions/actions#dangling-promises for more information.",
            if total_dangling_tasks == 1 { "" } else { "s" },
        );
        let warning = SystemWarning {
            level: LogLevel::Warn,
            messages: vec![message],
            system_log_metadata: SystemLogMetadata {
                code: "UnawaitedOperations".to_string(),
            },
        };
        provider.trace_system(warning)?;
    }
    if let Some(warning) = approaching_duration_limit_warning(
        execution_time.elapsed,
        execution_time.limit,
        "UserTimeout",
        "Function execution took a long time",
        None,
    )? {
        provider.trace_system(warning)?;
    }
    Ok(())
}

pub fn dangling_task_counts<'a>(
    tasks: impl IntoIterator<Item = &'a TaskType>,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for task in tasks
        .into_iter()
        .filter(|task| !matches!(task, TaskType::Sleep))
    {
        *counts.entry(task.name_when_dangling()).or_default() += 1;
    }
    counts
}

impl<RT: Runtime> ActionInnerProvider<RT> for ActionSyscallProvider<RT> {
    type HttpOutcome = HttpActionOutcome;
    type Outcome = ActionOutcome;

    fn rt(&self) -> &RT {
        &self.rt
    }

    fn identity(&self) -> &Identity {
        &self.identity
    }

    fn component(&self) -> ComponentId {
        self.phase.component()
    }

    fn syscall_trace(&self) -> SyscallTrace {
        self.syscall_trace.lock().clone()
    }

    async fn initialize(&mut self, timeout: &mut Timeout<RT>) -> anyhow::Result<()> {
        self.phase.initialize(timeout).await
    }

    fn begin_execution(&mut self) -> anyhow::Result<()> {
        self.phase.begin_execution()
    }

    fn start_task(&mut self, request: TaskRequestEnum) -> anyhow::Result<TaskId> {
        self.phase.require_executing(&request)?;
        let task_id = self.next_task_id.increment();
        self.pending_task_sender
            .send(TaskRequest {
                task_id,
                variant: request,
                parent_trace: EncodedSpan::from_parent(),
            })
            .map_err(|_| {
                anyhow!(ErrorMetadata::operational_internal_server_error())
                    .context("TaskExecutor went away")
            })?;
        Ok(task_id)
    }

    fn send_stream(
        &mut self,
        stream_id: uuid::Uuid,
        stream: Option<BoxStream<'static, anyhow::Result<bytes::Bytes>>>,
    ) -> anyhow::Result<()> {
        let task_id = self.next_task_id.increment();
        self.pending_task_sender
            .send(TaskRequest {
                task_id,
                variant: TaskRequestEnum::AsyncOp(AsyncOpRequest::SendStream { stream, stream_id }),
                parent_trace: EncodedSpan::from_parent(),
            })
            .map_err(|_| {
                anyhow!(ErrorMetadata::operational_internal_server_error())
                    .context("TaskExecutor went away")
            })?;
        Ok(())
    }

    async fn next_task_response(&mut self) -> anyhow::Result<TaskResponse> {
        self.task_responses
            .recv()
            .await
            .context("Task executor went away?")
    }

    fn shutdown_tasks(&mut self) {
        self.pending_task_sender.close();
        if let Some(mut running_tasks) = self.running_tasks.take() {
            running_tasks.shutdown();
        }
    }

    fn set_http_action_route(&self, route: HttpActionRoute) {
        if self.http_action_route.set(route).is_err() {
            tracing::warn!("HTTP action route was already set for this request");
        }
    }

    fn http_response_closed(&self) -> anyhow::Result<BoxFuture<'static, ()>> {
        // NOTE: this clone extends the lifetime of the response_streamer sender to the
        // lifetime of the TaskExecutor thread. Make sure that thread gets
        // shutdown before waiting for the response_streamer's receiver to
        // close. Currently the thread is shutdown in Drop for ActionSyscallProvider.
        let response_streamer = self.http_response_streamer()?.clone();
        Ok(async move {
            response_streamer.sender.closed().await;
        }
        .boxed())
    }

    fn http_response_has_started(&self) -> anyhow::Result<bool> {
        Ok(self.http_response_streamer()?.has_started())
    }

    fn handle_http_streamed_part(
        &mut self,
        part: Result<HttpActionResponsePart, JsError>,
    ) -> anyhow::Result<()> {
        let streamer = self
            .http_response_streamer
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No HTTP response streamer for HTTP action"))?;
        match part {
            Ok(HttpActionResponsePart::Head(h)) => {
                streamer.send_part(HttpActionResponsePart::Head(h))??;
            },
            Ok(HttpActionResponsePart::BodyChunk(b)) => {
                if self.http_response_too_large {
                    return Ok(());
                }
                if streamer.total_bytes_sent() + b.len() > HTTP_ACTION_RESPONSE_BODY_LIMIT {
                    self.http_response_too_large = true;
                    let e = JsError::from_message(format!(
                        "HttpResponseTooLarge: HTTP actions support responses up to {}",
                        HTTP_ACTION_RESPONSE_BODY_LIMIT.format_size(BINARY)
                    ));
                    self.trace_system(SystemWarning {
                        level: LogLevel::Error,
                        messages: vec![e.to_string()],
                        system_log_metadata: SystemLogMetadata {
                            code: "error:httpAction".to_string(),
                        },
                    })?;
                } else {
                    // If the `streamer` is closed, the inner Result
                    // will have an error. That's fine; we want to keep letting
                    // the isolate send data.
                    let _ = streamer.send_part(HttpActionResponsePart::BodyChunk(b))?;
                }
            },
            Err(e) => self.trace_system(SystemWarning {
                level: LogLevel::Error,
                messages: vec![e.to_string()],
                system_log_metadata: SystemLogMetadata {
                    code: "error:httpAction".to_string(),
                },
            })?,
        };
        Ok(())
    }

    fn add_http_response_size_warning(&mut self) -> anyhow::Result<()> {
        let total_bytes_sent = self.http_response_streamer()?.total_bytes_sent();
        if !self.http_response_too_large
            && let Some(warning) = approaching_limit_warning(
                total_bytes_sent,
                HTTP_ACTION_RESPONSE_BODY_LIMIT,
                "HttpResponseTooLarge",
                || "Large response returned from an HTTP action".to_string(),
                None,
                Some(" bytes"),
                None,
            )
        {
            self.trace_system(warning)?;
        }
        Ok(())
    }

    fn trace_system(&mut self, warning: SystemWarning) -> anyhow::Result<()> {
        self.log_line_sender
            .send(warning.into_log_line(self.rt.unix_timestamp()))?;
        Ok(())
    }

    fn into_outcome(self, outcome: ActionOutcome) -> Self::Outcome {
        outcome
    }

    fn into_http_outcome(self, outcome: HttpActionOutcome) -> Self::HttpOutcome {
        outcome
    }
}

impl<RT: Runtime> ActionSyscallProvider<RT> {
    fn http_response_streamer(&self) -> anyhow::Result<&HttpActionResponseStreamer> {
        self.http_response_streamer
            .as_ref()
            .context("No HTTP response streamer for HTTP action")
    }
}

impl<RT: Runtime> HeapSize for ActionSyscallProvider<RT> {
    fn heap_size(&self) -> usize {
        self.syscall_trace.lock().heap_size()
    }
}

impl<RT: Runtime> OpProvider for ActionSyscallProvider<RT> {
    fn trace(&mut self, level: LogLevel, messages: Vec<String>) -> anyhow::Result<()> {
        let max_log_lines = *MAX_LOG_LINES_V8_ACTIONS;
        // - 1 to reserve for the [ERROR] log line
        match self.total_log_lines.cmp(&(max_log_lines - 1)) {
            // We are explicitly dropping errors in actions in case the log line sender goes away.
            // We should throw errors again once we correctly handle clients going away in HTTP
            // actions.
            Ordering::Less => {
                let _ = self.log_line_sender.send(LogLine::new_developer_log_line(
                    level,
                    messages,
                    self.rt.unix_timestamp(),
                ));
                self.total_log_lines += 1;
            },
            Ordering::Equal => {
                let _ = self.log_line_sender.send(log_overflow_log_line(
                    max_log_lines,
                    self.rt.unix_timestamp(),
                ));
                self.total_log_lines += 1;
            },
            Ordering::Greater => (),
        };
        Ok(())
    }

    fn rng(&mut self) -> anyhow::Result<&mut ChaCha12Rng> {
        self.phase.rng()
    }

    fn crypto_rng(&mut self) -> anyhow::Result<CryptoRng> {
        Ok(CryptoRng::new())
    }

    fn unix_timestamp(&mut self) -> anyhow::Result<UnixTimestamp> {
        self.phase.unix_timestamp()
    }

    fn performance_now(&mut self) -> anyhow::Result<Duration> {
        self.phase.performance_now()
    }

    fn performance_time_origin(&mut self) -> anyhow::Result<UnixTimestamp> {
        self.phase.performance_time_origin()
    }

    fn get_environment_variable(
        &mut self,
        name: EnvVarName,
    ) -> anyhow::Result<Option<EnvVarValue>> {
        self.phase.get_environment_variable(name)
    }

    fn get_all_table_mappings(&mut self) -> anyhow::Result<NamespacedTableMapping> {
        anyhow::bail!("get_all_table_mappings unsupported in actions")
    }
}

impl<RT: Runtime> SyscallProvider<RT> for ActionSyscallProvider<RT> {
    // We lookup all modules' sources upfront when initializing the action
    // environment, so this function always returns immediately.
    async fn lookup_source(
        &mut self,
        path: &str,
        timeout: &mut Timeout<RT>,
    ) -> anyhow::Result<Option<(Arc<V8ModuleSource>, ModuleCodeCacheResult)>> {
        let user_module_path: ModulePath = path.parse()?;
        let result = self.phase.get_module(&user_module_path, timeout)?;
        Ok(result)
    }

    fn syscall(&mut self, name: &str, args: JsonValue) -> anyhow::Result<JsonValue> {
        self.syscall_impl(name, args)
    }
}

impl<RT: Runtime, S: ActionInnerProvider<RT>> JsEnvironment<RT> for ActionEnvironment<RT, S> {
    type AsyncResolver = v8::Global<v8::PromiseResolver>;
    type SyscallProvider = S;

    fn syscall_provider(&mut self) -> &mut Self::SyscallProvider {
        &mut self.provider
    }

    fn start_async_syscall(
        &mut self,
        name: String,
        args: JsonValue,
        resolver: Self::AsyncResolver,
    ) -> anyhow::Result<()> {
        self.start_task(TaskRequestEnum::AsyncSyscall { name, args }, resolver)
    }

    fn start_async_op(
        &mut self,
        request: AsyncOpRequest,
        resolver: Self::AsyncResolver,
    ) -> anyhow::Result<()> {
        self.start_task(TaskRequestEnum::AsyncOp(request), resolver)
    }

    fn environment_heap_size(&self) -> usize {
        self.provider.heap_size()
    }

    fn user_timeout(&self) -> std::time::Duration {
        *V8_ACTION_USER_TIMEOUT
    }

    fn system_timeout(&self) -> std::time::Duration {
        *V8_ACTION_SYSTEM_TIMEOUT
    }
}
