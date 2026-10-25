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
    sync::Arc,
    time::Instant,
};

use anyhow::Context as _;
use common::{
    components::{
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
    },
    source_packages::SourcePackageModel,
};
use parking_lot::Mutex;
use serde_json::Value as JsonValue;
use tokio::sync::{
    mpsc,
    oneshot,
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
        InvalidModule,
        ModuleHash,
        WasmCore,
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
};

pub mod backend;
pub mod core;
mod dispatch;
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
    executions: Semaphore,
}

/// Slack on top of `FUNCTION_MAX_RESULT_SIZE` for the result envelope's own
/// framing (`{"ok": ...}`, error messages).
const RESULT_ENVELOPE_SLACK: usize = 64 * 1024;

impl<RT: Runtime> RustActionRuntime<RT> {
    pub fn new(rt: RT) -> Self {
        Self {
            rt,
            core: Mutex::new(None),
            executions: Semaphore::new((*RUST_MAX_CONCURRENT_ACTIONS).max(1)),
        }
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

        let core = self.core()?;
        // Bound concurrent executions (queued callers wait here).
        let _permit = self
            .executions
            .acquire()
            .await
            .context("rust action semaphore closed")?;
        // The compiled-module cache is keyed by the content hash in the module
        // metadata, so a hit needs neither the module source nor a hash of it.
        let key: ModuleHash = *metadata.sha256;
        let prepared = match core.lookup(&key) {
            Some(hit) => hit,
            None => {
                let source_package = SourcePackageModel::new(&mut transaction, component.into())
                    .get_latest()
                    .await?
                    .context("source package not found")?;
                let module_source = module_loader
                    .get_module_with_metadata(&metadata, &source_package)
                    .await?;
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
                            return developer_error(format!(
                                "Invalid \"use rust\" module {}: {msg}",
                                path.udf_path.module().as_str()
                            ))
                        },
                        None => return Err(e),
                    },
                }
            },
        };
        let module_name = path.udf_path.module().as_str().to_string();
        let function_name = path.udf_path.function_name().to_string();
        let Some(export) = prepared.export_for(&module_name, &function_name) else {
            return developer_error(format!(
                "Could not find function {function_name} in module {module_name}"
            ));
        };
        let export = export.to_string();

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
        let SpawnedTaskExecutor {
            pending_task_sender,
            task_responses,
            running_tasks,
        } = spawn_task_executor(&self.rt, |task_retval_sender| TaskExecutor {
            rt: self.rt.clone(),
            identity: identity.clone(),
            file_storage,
            syscall_trace: syscall_trace.clone(),
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
            udf_path: path.udf_path.clone(),
            component_path: path.component_path.clone(),
            convex_origin_override,
            http_action_route: Default::default(),
            deployment,
            service_token: Arc::new(ServiceTokenCache::default()),
        });
        let backend = ActionBackend::new(
            self.rt.clone(),
            pending_task_sender,
            task_responses,
            running_tasks,
            log_line_sender,
            component_arguments,
        );

        if let Some(tx) = function_started_sender {
            _ = tx.send(());
        }
        // TODO: cancellation. `ActionRequest` carries no cancellation signal
        // (V8 gets one from `IsolateWorker`); dropping this future stops the
        // guest at its next await or epoch tick.
        let timeout = *V8_ACTION_USER_TIMEOUT;
        let started = Instant::now();
        // The wall-clock future covers a guest parked on `task_poll(block)`,
        // where the epoch hook never fires; the core still drains stdio then.
        let mut out = core
            .call(
                &prepared,
                &export,
                args_json,
                env,
                backend,
                timeout,
                self.rt.wait(timeout),
            )
            .await;
        // Dangling tasks are dropped, like unawaited promises in V8.
        out.backend.shutdown_tasks();
        let result: Result<GuestResult, CallFailure> = out.result;
        let user_execution_time = started.elapsed();
        let trace = syscall_trace.lock().clone();

        let result = match result {
            Ok(GuestResult::Ok(value)) => {
                match deserialize_udf_result(&path, value.get())? {
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
        };
        Ok(make_outcome(result, trace, Some(user_execution_time)))
    }
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
