//! `"use rust"` action runtime: runs `wasm32-wasip1` modules built with the
//! `convex_fn` SDK in-process under wasmtime (D-008), without a V8 isolate.
//!
//! The guest ABI is specified in `convex-rs/contracts/ABI.md`. Async work is
//! submitted as the same task shapes V8 actions use and executed by
//! [`task_spawn::spawn_task_executor`], so
//! `ctx.runQuery`, `fetch`, storage and the scheduler share one implementation
//! with JS actions.
//!
//! Layout: [`core`] drives wasmtime (engine, LRU, linker, call protocol) and
//! knows nothing about the backend; [`backend`] implements the guest's host
//! imports on top of the shared task executor.
//!
//! Seam (frozen for W2-02/W2-03):
//! - [`RustActionRuntime::new`] is called once per [`crate::IsolateClient`].
//! - [`RustActionRuntime::execute_action`] receives exactly what the V8 path
//!   receives in `RequestType::Action` and returns the same [`ActionOutcome`].

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        OnceLock,
    },
    time::Instant,
};

use anyhow::Context as _;
use common::{
    components::{
        CanonicalizedComponentModulePath,
        ComponentId,
        Reference,
        Resource,
    },
    document::ParsedDocument,
    errors::JsError,
    execution_context::ExecutionContext,
    http::{
        fetch::FetchClient,
        RequestDestination,
    },
    knobs::{
        FUNCTION_MAX_RESULT_SIZE,
        RUST_MAX_CONCURRENT_ACTIONS,
        RUST_MAX_OUTSTANDING_TASKS,
        RUST_MAX_TASK_REQUEST_BYTES,
        RUST_WASM_CACHE_DIR,
        RUST_WASM_DISK_CACHE,
        V8_ACTION_USER_TIMEOUT,
    },
    log_lines::LogLine,
    runtime::Runtime,
    types::{
        ConvexOrigin,
        ModuleEnvironment,
    },
};
use database::{
    BootstrapComponentsModel,
    Transaction,
};
use model::{
    canonical_urls::CanonicalUrlsModel,
    components::ComponentsModel,
    environment_variables::{
        types::{
            EnvVarName,
            EnvVarValue,
        },
        EnvironmentVariablesModel,
    },
    modules::{
        types::ModuleMetadata,
        ModuleModel,
    },
    source_packages::SourcePackageModel,
};
use parking_lot::Mutex;
use serde_json::Value as JsonValue;
use tokio::sync::{
    mpsc,
    oneshot,
    OwnedSemaphorePermit,
    Semaphore,
};
use udf::{
    environment::parse_system_env_var_overrides,
    ActionCallbacks,
    ActionOutcome,
    SyscallTrace,
};
use value::{
    identifier::Identifier,
    ConvexValue,
    JsonPackedValue,
};

use self::{
    backend::ActionBackend,
    core::{
        CallFailure,
        CoreLimits,
        GuestErrorKind,
        GuestResult,
        HostBackend,
        InvalidModule,
        ModuleHash,
        PollOutcome,
        PreparedModule,
        WasmCore,
    },
    native::{
        Artifact,
        NativeRuntime,
        NATIVE_PREFIX,
        NATIVE_REF_MAGIC,
    },
    task_spawn::{
        spawn_task_executor,
        SpawnedTaskExecutor,
    },
};
use crate::{
    client::{
        ActionRequest,
        EnvironmentData,
    },
    environment::action::{
        service_token::ServiceTokenCache,
        task_executor::TaskExecutor,
    },
    helpers::{
        deserialize_udf_result,
        format_uncaught_error,
    },
    module_cache::{
        ModuleCache,
        V8ExternalString,
    },
};

pub mod backend;
pub mod core;
mod dispatch;
pub mod native;
mod task_spawn;
#[cfg(test)]
mod tests;

// Optional `crypto/*` sync ops served to every guest (ABI.md "Optional sync ops").
mod host_crypto;

pub struct RustActionRuntime<RT: Runtime> {
    rt: RT,
    /// Built on first use so deployments without Rust functions never start
    /// the epoch ticker thread.
    core: Mutex<Option<Arc<WasmCore<ActionBackend<RT>>>>>,
    /// Bounds concurrently executing Rust actions (`RUST_MAX_CONCURRENT_ACTIONS`).
    executions: Arc<Semaphore>,
    /// `RUST_NATIVE_MODE=worker`. The runtime is built on first use, inside a
    /// tokio runtime.
    native_enabled: bool,
    native: OnceLock<Arc<NativeRuntime>>,
}

/// How a call is run, decided by the module source.
enum Engine<RT: Runtime> {
    Wasm {
        prepared: Arc<PreparedModule<ActionBackend<RT>>>,
        export: String,
        _permit: OwnedSemaphorePermit,
    },
    Native {
        runtime: Arc<NativeRuntime>,
        artifact: Arc<Artifact>,
        export: String,
    },
}

/// Slack on top of `FUNCTION_MAX_RESULT_SIZE` for the result envelope's own
/// framing (`{"ok": ...}`, error messages).
const RESULT_ENVELOPE_SLACK: usize = 64 * 1024;

impl<RT: Runtime> RustActionRuntime<RT> {
    pub fn new(rt: RT) -> Self {
        let native_enabled = native::target::native_mode().is_some();
        if native_enabled && let Err(e) = native::startup_check() {
            // Refuse to start rather than run user code unsandboxed by accident.
            tracing::error!("rust-native: {e:#}");
            panic!("rust-native: {e:#}");
        }
        Self {
            rt,
            core: Mutex::new(None),
            executions: Arc::new(Semaphore::new((*RUST_MAX_CONCURRENT_ACTIONS).max(1))),
            native_enabled,
            native: OnceLock::new(),
        }
    }

    fn native(&self) -> Option<&Arc<NativeRuntime>> {
        self.native_enabled
            .then(|| self.native.get_or_init(NativeRuntime::new))
    }

    fn core(&self) -> anyhow::Result<Arc<WasmCore<ActionBackend<RT>>>> {
        let mut guard = self.core.lock();
        if let Some(core) = &*guard {
            return Ok(core.clone());
        }
        let core = Arc::new(WasmCore::with_limits(CoreLimits {
            max_task_request_bytes: *RUST_MAX_TASK_REQUEST_BYTES,
            max_outstanding_tasks: *RUST_MAX_OUTSTANDING_TASKS,
            max_result_bytes: *FUNCTION_MAX_RESULT_SIZE + RESULT_ENVELOPE_SLACK,
            disk_cache_dir: (*RUST_WASM_DISK_CACHE)
                .then(|| std::path::PathBuf::from(&*RUST_WASM_CACHE_DIR)),
            ..Default::default()
        })?);
        *guard = Some(core.clone());
        Ok(core)
    }

    /// Picks the engine for a module and loads what it needs. The inner `Err`
    /// is a developer-facing message.
    async fn resolve_engine(
        &self,
        metadata: &Arc<ParsedDocument<ModuleMetadata>>,
        transaction: &mut Transaction<RT>,
        module_loader: &Arc<dyn ModuleCache<RT>>,
        component: ComponentId,
        module_name: &str,
        function_name: &str,
    ) -> anyhow::Result<Result<Engine<RT>, String>> {
        // The compiled-module caches are keyed by the content hash in the
        // module metadata, so a hit needs neither the module source nor a
        // hash of it.
        let key: ModuleHash = *metadata.sha256;
        let native = self.native();
        let export_of = |artifact: &Artifact| -> Result<String, String> {
            // The carrier header keys functions by the module name below
            // `rust/` (`hello:hello`), the wasm map by the full path.
            let module = module_name.strip_suffix(".js").unwrap_or(module_name);
            let module = module.strip_prefix("rust/").unwrap_or(module);
            artifact
                .functions
                .get(&format!("{module}:{function_name}"))
                .cloned()
                .ok_or_else(|| {
                    format!("Could not find function {function_name} in module {module_name}")
                })
        };
        if let Some(runtime) = native
            && let Some(artifact) = runtime.cached(&key)
        {
            return Ok(export_of(&artifact).map(|export| Engine::Native {
                runtime: runtime.clone(),
                artifact,
                export,
            }));
        }
        let core = self.core()?;
        // Bound concurrent executions (queued callers wait here).
        let permit = self
            .executions
            .clone()
            .acquire_owned()
            .await
            .context("rust action semaphore closed")?;
        let prepared = match core.lookup(&key) {
            Some(hit) => hit,
            None => {
                let source_package = SourcePackageModel::new(transaction, component.into())
                    .get_latest()
                    .await?
                    .context("source package not found")?;
                let module_source = module_loader
                    .get_module_with_metadata(metadata, &source_package)
                    .await?;
                if source_starts_with(module_source.source(), NATIVE_PREFIX) {
                    drop(permit);
                    let Some(runtime) = native else {
                        return Ok(Err(native::MODE_OFF_MESSAGE.to_string()));
                    };
                    let artifact = match self
                        .load_native(
                            runtime,
                            metadata,
                            transaction,
                            module_loader,
                            component,
                            module_source,
                        )
                        .await?
                    {
                        Ok(a) => a,
                        Err(msg) => {
                            return Ok(Err(format!(
                                "Invalid \"use rust\" module {module_name}: {msg}"
                            )))
                        },
                    };
                    runtime.prewarm(&artifact);
                    return Ok(export_of(&artifact).map(|export| Engine::Native {
                        runtime: runtime.clone(),
                        artifact,
                        export,
                    }));
                }
                // Base64 decode and compilation run on a blocking thread;
                // concurrent compiles of one module are coalesced by the core.
                let core = core.clone();
                let compiled = tokio::task::spawn_blocking(move || {
                    core.prepare_base64_keyed(key, &module_source.source().to_utf8())
                })
                .await
                .context("wasm compile task failed")?;
                match compiled {
                    Ok(p) => p,
                    // A bad module is the developer's problem, not a system error.
                    Err(e) => match e.downcast_ref::<InvalidModule>() {
                        Some(InvalidModule(msg)) => {
                            return Ok(Err(format!(
                                "Invalid \"use rust\" module {module_name}: {msg}"
                            )))
                        },
                        None => return Err(e),
                    },
                }
            },
        };
        let Some(export) = prepared.export_for(module_name, function_name) else {
            return Ok(Err(format!(
                "Could not find function {function_name} in module {module_name}"
            )));
        };
        let export = export.to_string();
        Ok(Ok(Engine::Wasm {
            prepared,
            export,
            _permit: permit,
        }))
    }

    /// Resolves a native carrier or ref module to its artifact (a cache miss).
    /// The inner `Err` is a developer-facing reason.
    async fn load_native(
        &self,
        runtime: &Arc<NativeRuntime>,
        metadata: &Arc<ParsedDocument<ModuleMetadata>>,
        transaction: &mut Transaction<RT>,
        module_loader: &Arc<dyn ModuleCache<RT>>,
        component: ComponentId,
        module_source: Arc<crate::module_cache::V8ModuleSource>,
    ) -> anyhow::Result<Result<Arc<Artifact>, String>> {
        let key: Vec<u8> = (*metadata.sha256).to_vec();
        let is_ref = source_starts_with(module_source.source(), NATIVE_REF_MAGIC);
        let mut carrier_source = module_source;
        let mut carrier_key = key.clone();
        let mut ref_sha = None;
        if is_ref {
            let text = carrier_source.source().to_utf8();
            let Some(header) = text.strip_prefix(NATIVE_REF_MAGIC) else {
                return Ok(Err("malformed native ref module".to_string()));
            };
            #[derive(serde::Deserialize)]
            struct RefHeader {
                sha256: String,
                carrier: String,
            }
            let header: RefHeader = match serde_json::from_str(header.trim()) {
                Ok(h) => h,
                Err(e) => return Ok(Err(format!("invalid native ref header: {e}"))),
            };
            if let Some(a) = runtime.store().cached(&header.sha256) {
                runtime.remember_ref(&key, &a);
                return Ok(Ok(a));
            }
            let carrier_path = match header.carrier.parse::<sync_types::ModulePath>() {
                Ok(p) => p.canonicalize(),
                Err(e) => return Ok(Err(format!("invalid carrier path in native ref: {e}"))),
            };
            let carrier_meta = ModuleModel::new(transaction)
                .get_metadata(CanonicalizedComponentModulePath {
                    component,
                    module_path: carrier_path,
                })
                .await?;
            let Some(carrier_meta) = carrier_meta else {
                return Ok(Err(format!(
                    "native carrier module {} was not found",
                    header.carrier
                )));
            };
            let source_package = SourcePackageModel::new(transaction, component.into())
                .get_latest()
                .await?
                .context("source package not found")?;
            carrier_source = module_loader
                .get_module_with_metadata(&carrier_meta, &source_package)
                .await?;
            carrier_key = (*carrier_meta.sha256).to_vec();
            ref_sha = Some(header.sha256);
        }
        let rt = runtime.clone();
        let resolved = tokio::task::spawn_blocking(move || {
            let text = carrier_source.source().to_utf8();
            rt.store().resolve_with_key(&carrier_key, &text)
        })
        .await
        .context("native artifact task failed")?;
        let artifact = match resolved {
            Ok(a) => a,
            Err(e) => return Ok(Err(format!("{e:#}"))),
        };
        if let Some(sha) = ref_sha {
            if !artifact.sha256.eq_ignore_ascii_case(&sha) {
                return Ok(Err(format!(
                    "native ref expects artifact {sha} but its carrier holds {}",
                    artifact.sha256
                )));
            }
            runtime.remember_ref(&key, &artifact);
        }
        // A new artifact: drop the ones no module references any more.
        let live_keys: Vec<Vec<u8>> = ModuleModel::new(transaction)
            .get_all_metadata(component)
            .await?
            .iter()
            .filter(|m| m.environment == ModuleEnvironment::Rust)
            .map(|m| (*m.sha256).to_vec())
            .collect();
        let extra = [artifact.sha256.clone()];
        let rt = runtime.clone();
        tokio::task::spawn_blocking(move || rt.gc(&live_keys, &extra));
        Ok(Ok(artifact))
    }

    pub async fn execute_action(
        &self,
        metadata: Arc<ParsedDocument<ModuleMetadata>>,
        request: ActionRequest<RT>,
        action_callbacks: Arc<dyn ActionCallbacks>,
        fetch_client: Arc<dyn FetchClient>,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
        function_started_sender: Option<oneshot::Sender<()>>,
    ) -> anyhow::Result<ActionOutcome> {
        let start_unix_timestamp = self.rt.unix_timestamp();
        let ActionRequest {
            params,
            mut transaction,
            identity,
            context,
            environment_data,
        } = request;
        let EnvironmentData {
            key_broker,
            default_system_env_vars,
            file_storage,
            module_loader,
            deployment,
        } = environment_data;
        let (path, arguments, udf_server_version) = params.path_and_args.consume();
        let component = path.component;
        let make_outcome = |result: Result<JsonPackedValue, JsError>,
                            syscall_trace: SyscallTrace,
                            user_execution_time: Option<std::time::Duration>|
         -> ActionOutcome {
            ActionOutcome {
                path: path.clone().for_logging(),
                arguments: arguments.clone(),
                identity: identity.clone().into(),
                unix_timestamp: start_unix_timestamp,
                result,
                syscall_trace,
                udf_server_version: udf_server_version.clone(),
                user_execution_time,
            }
        };
        let developer_error = |message: String| -> anyhow::Result<ActionOutcome> {
            Ok(make_outcome(
                Err(JsError::from_message(message)),
                SyscallTrace::new(),
                None,
            ))
        };

        if path.udf_path.module().is_deps() {
            anyhow::bail!(
                "Refusing to run {:?} within the '_deps' directory",
                path.udf_path
            );
        }

        anyhow::ensure!(
            metadata.environment == ModuleEnvironment::Rust,
            "Trying to execute {:?} as a Rust action, but it is bundled for {:?}.",
            path.udf_path.module().as_str(),
            metadata.environment
        );

        let module_name = path.udf_path.module().as_str().to_string();
        let function_name = path.udf_path.function_name().to_string();
        let engine = match self
            .resolve_engine(
                &metadata,
                &mut transaction,
                &module_loader,
                component,
                &module_name,
                &function_name,
            )
            .await?
        {
            Ok(engine) => engine,
            Err(message) => return developer_error(message),
        };

        // Arguments: the JSON array V8 passes, whose first element is the
        // args object.
        // Parsed as raw slices: the bytes are validated as JSON but passed to
        // the guest unchanged (no value tree, no re-serialisation).
        let args_array: Vec<&serde_json::value::RawValue> =
            serde_json::from_str(arguments.get()).context("arguments are not a JSON array")?;
        let args_json: &str = match args_array.first() {
            Some(v) => v.get(),
            None => "{}",
        };

        // Environment variables, as `process.env` would see them in a V8
        // action. TODO(DX-05): component env bindings (`EnvBinding`).
        let resources = Arc::new(Mutex::new(BTreeMap::<Reference, Resource>::new()));
        let convex_origin_override = Arc::new(Mutex::new(None::<ConvexOrigin>));
        let (env, component_arguments) = load_environment(
            &mut transaction,
            component,
            default_system_env_vars,
            &resources,
            &convex_origin_override,
        )
        .await?;

        let syscall_trace = Arc::new(Mutex::new(SyscallTrace::new()));
        let usage_tracker = transaction.usage_tracker.clone();
        // Builds the task executor and the backend that drives it. Wasm needs
        // it up front; a native call builds it on the guest's first backend
        // use (`LazyBackend`), which many actions never make.
        let make_backend = {
            let rt = self.rt.clone();
            let syscall_trace = syscall_trace.clone();
            let identity = identity.clone();
            let udf_path = path.udf_path.clone();
            let component_path = path.component_path.clone();
            move || {
                let SpawnedTaskExecutor {
                    pending_task_sender,
                    task_responses,
                    running_tasks,
                } = spawn_task_executor(&rt, |task_retval_sender| TaskExecutor {
                    rt: rt.clone(),
                    identity,
                    file_storage,
                    syscall_trace,
                    action_callbacks,
                    fetch_client,
                    _module_loader: module_loader,
                    key_broker,
                    task_order: Default::default(),
                    task_retval_sender,
                    usage_tracker,
                    context: context_clone(&context),
                    resources,
                    component_id: component,
                    udf_path,
                    component_path,
                    convex_origin_override,
                    http_action_route: Default::default(),
                    deployment,
                    service_token: Arc::new(ServiceTokenCache::default()),
                });
                ActionBackend::new(
                    rt.clone(),
                    pending_task_sender,
                    task_responses,
                    running_tasks,
                    log_line_sender,
                    component_arguments,
                )
            }
        };

        if let Some(tx) = function_started_sender {
            _ = tx.send(());
        }
        // TODO: cancellation. `ActionRequest` carries no cancellation signal
        // (V8 gets one from `IsolateWorker`); dropping this future stops the
        // guest at its next await or epoch tick (wasm) or kills the worker
        // (native).
        let timeout = *V8_ACTION_USER_TIMEOUT;
        let (result, user_execution_time): (Result<GuestResult, CallFailure>, _) = match engine {
            Engine::Wasm {
                prepared, export, ..
            } => {
                let started = Instant::now();
                // The wall-clock future covers a guest parked on `task_poll(block)`,
                // where the epoch hook never fires; the core still drains stdio then.
                let mut out = self
                    .core()?
                    .call(
                        &prepared,
                        &export,
                        args_json,
                        env,
                        make_backend(),
                        timeout,
                        self.rt.wait(timeout),
                    )
                    .await;
                // Dangling tasks are dropped, like unawaited promises in V8.
                out.backend.shutdown_tasks();
                (out.result, started.elapsed())
            },
            Engine::Native {
                runtime,
                artifact,
                export,
            } => {
                // The pump owns the backend and shuts its tasks down at the end.
                let out = runtime
                    .call(
                        &artifact,
                        export,
                        args_json.as_bytes().to_vec(),
                        env,
                        LazyBackend::new(Box::new(make_backend)),
                        timeout,
                    )
                    .await;
                (out.result, out.elapsed)
            },
        };
        let trace = syscall_trace.lock().clone();

        let result = match result {
            Ok(g @ (GuestResult::Ok(_) | GuestResult::OkText(_))) => {
                let text: &str = match &g {
                    GuestResult::Ok(v) => v.get(),
                    GuestResult::OkText(t) => t,
                    GuestResult::Error { .. } => unreachable!(),
                };
                match deserialize_udf_result(&path, text)? {
                    Ok(v) => Ok(JsonPackedValue::pack(v)),
                    Err(e) => Err(e),
                }
            },
            Ok(GuestResult::Error {
                message,
                data,
                kind,
            }) => Err(guest_error_to_js_error(message, data, kind)?),
            Err(CallFailure::Timeout) => Err(JsError::from_message(format!(
                "Function execution timed out (maximum duration: {timeout:?})"
            ))),
            Err(CallFailure::Panic { message, .. }) => Err(JsError::from_message(
                format_uncaught_error(message, "Error".to_string()),
            )),
            Err(CallFailure::Trap(msg)) => Err(JsError::from_message(format_uncaught_error(
                msg,
                "WebAssembly trap".to_string(),
            ))),
            // A guest that breaks the ABI is a bug in the developer's module,
            // deterministic for the same code and arguments.
            Err(CallFailure::Protocol(msg)) => Err(JsError::from_message(format!(
                "The \"use rust\" module broke the guest ABI: {msg}"
            ))),
            Err(CallFailure::ResultTooLarge { len, max }) => {
                Err(JsError::from_message(format!(
                    "Function result is too large ({len} bytes, the limit is {max} bytes)"
                )))
            },
            Err(CallFailure::OutOfMemory { limit }) => Err(JsError::from_message(format!(
                "Uncaught Error: Rust execution ran out of memory (maximum memory usage: {})",
                format_mib(limit)
            ))),
            Err(CallFailure::StackOverflow { stack }) => Err(JsError::from_message(format!(
                "Uncaught Error: Rust action overflowed its stack (stack size: {})",
                format_mib(stack)
            ))),
            Err(CallFailure::Crashed(msg)) => {
                Err(JsError::from_message(format!("Uncaught Error: {msg}")))
            },
            // The worker could not start: the module or the setup is broken,
            // not the system.
            Err(CallFailure::StartFailed(msg)) => Err(JsError::from_message(format!(
                "Could not start the native Rust worker: {msg}"
            ))),
        };
        Ok(make_outcome(result, trace, Some(user_execution_time)))
    }
}

/// Backend that builds the real [`ActionBackend`] (and its task executor) on
/// first use, so native calls that never touch the host (no task, syscall or
/// log) skip the setup.
struct LazyBackend<RT: Runtime> {
    init: Option<Box<dyn FnOnce() -> ActionBackend<RT> + Send>>,
    inner: Option<ActionBackend<RT>>,
}

impl<RT: Runtime> LazyBackend<RT> {
    fn new(init: Box<dyn FnOnce() -> ActionBackend<RT> + Send>) -> Self {
        Self {
            init: Some(init),
            inner: None,
        }
    }

    fn get(&mut self) -> &mut ActionBackend<RT> {
        if self.inner.is_none() {
            let init = self.init.take().expect("backend initialised once");
            self.inner = Some(init());
        }
        self.inner.as_mut().expect("just initialised")
    }
}

#[async_trait::async_trait]
impl<RT: Runtime> HostBackend for LazyBackend<RT> {
    fn submit(&mut self, request: JsonValue) -> Result<i64, String> {
        self.get().submit(request)
    }

    async fn poll(&mut self, block: bool) -> PollOutcome {
        match &mut self.inner {
            Some(b) => b.poll(block).await,
            // Nothing was submitted, so nothing can be outstanding.
            None if block => PollOutcome::NoTasks,
            None => PollOutcome::Empty,
        }
    }

    fn sync_syscall(&mut self, op: &str, args: JsonValue) -> JsonValue {
        self.get().sync_syscall(op, args)
    }

    fn log(&mut self, level: i32, message: String) {
        self.get().log(level, message)
    }

    fn outstanding_tasks(&self) -> usize {
        self.inner.as_ref().map_or(0, |b| b.outstanding_tasks())
    }
}

/// Native sources are ASCII; a two-byte source cannot be one.
fn source_starts_with(source: &V8ExternalString, prefix: &str) -> bool {
    match source {
        V8ExternalString::OneByte(s) => s.starts_with(prefix.as_bytes()),
        V8ExternalString::TwoByte(_) => false,
    }
}

fn format_mib(bytes: u64) -> String {
    format!("{} MiB", bytes >> 20)
}

fn context_clone(context: &ExecutionContext) -> ExecutionContext {
    context.clone()
}

fn guest_error_to_js_error(
    message: String,
    data: Option<JsonValue>,
    kind: GuestErrorKind,
) -> anyhow::Result<JsError> {
    Ok(match (kind, data) {
        (GuestErrorKind::Convex, Some(data)) => match ConvexValue::try_from(data) {
            Ok(data) => JsError::convex_error(
                format_uncaught_error(message, "ConvexError".to_string()),
                data,
            ),
            Err(e) => JsError::from_message(format!("ConvexError with invalid data: {e}")),
        },
        (GuestErrorKind::Panic, _) => {
            JsError::from_message(format_uncaught_error(message, "Error".to_string()))
        },
        (..) => JsError::from_message(format_uncaught_error(message, "Error".to_string())),
    })
}

/// Mirrors the env-var / resource preloading of `ActionPhase::initialize`.
async fn load_environment<RT: Runtime>(
    tx: &mut Transaction<RT>,
    component: ComponentId,
    default_system_env_vars: BTreeMap<EnvVarName, EnvVarValue>,
    resources: &Mutex<BTreeMap<Reference, Resource>>,
    convex_origin_override: &Mutex<Option<ConvexOrigin>>,
) -> anyhow::Result<(
    Vec<(String, String)>,
    Option<BTreeMap<Identifier, ConvexValue>>,
)> {
    *resources.lock() = ComponentsModel::new(tx)
        .preload_resources(component)
        .await?;
    let canonical_urls = CanonicalUrlsModel::new(tx).get_canonical_urls().await?;
    if let Some(cloud_url) = canonical_urls.get(&RequestDestination::ConvexCloud) {
        *convex_origin_override.lock() = Some(ConvexOrigin::from(&cloud_url.url));
    }
    let mut env_vars = default_system_env_vars;
    env_vars.extend(parse_system_env_var_overrides(canonical_urls)?);
    let component_arguments = if component.is_root() {
        env_vars.extend(EnvironmentVariablesModel::new(tx).get_all().await?);
        None
    } else {
        Some(
            BootstrapComponentsModel::new(tx)
                .load_component_args(component)
                .await?,
        )
    };
    let env = env_vars
        .into_iter()
        .map(|(k, v)| {
            (
                AsRef::<str>::as_ref(&k).to_string(),
                AsRef::<str>::as_ref(&v).to_string(),
            )
        })
        .collect();
    Ok((env, component_arguments))
}
