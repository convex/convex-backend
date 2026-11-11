use std::{
    collections::BTreeMap,
    future::Future,
    path::Path,
    sync::{
        Arc,
        LazyLock,
    },
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use bytes::{
    Buf,
    Bytes,
};
use common::{
    backoff::Backoff,
    errors::{
        FrameData,
        JsError,
    },
    execution_context::ExecutionContext,
    knobs::{
        GET_SERVICE_TOKEN_GUARANTEED_LIFETIME,
        MAX_LOG_LINES_NODE_ACTIONS,
        NODE_ANALYZE_MAX_RETRIES,
    },
    log_lines::{
        run_function_and_collect_log_lines,
        LogLine,
        LogLineStructured,
        LogLines,
    },
    runtime::Runtime,
    sha256::Sha256Digest,
    types::{
        ActionCallbackToken,
        ConvexOrigin,
        DeploymentMetadata,
        NodeDependency,
        ObjectKey,
        UdfType,
    },
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use futures::{
    FutureExt,
    Stream,
    StreamExt,
};
use isolate::{
    deserialize_udf_custom_error,
    deserialize_udf_result,
    format_uncaught_error,
    helpers::source_map_from_slice,
};
use model::{
    environment_variables::types::{
        EnvVarName,
        EnvVarValue,
        EnvironmentVariable,
        PersistedEnvironmentVariable,
    },
    modules::{
        function_validators::{
            ArgsValidator,
            ArgsValidatorJson,
            ReturnsValidator,
            ReturnsValidatorJson,
        },
        module_versions::{
            invalid_function_name_error,
            AnalyzedFunction,
            AnalyzedModule,
            AnalyzedSourcePosition,
            SourceMap,
            Visibility,
        },
    },
    source_packages::types::{
        PackageSize,
        SourcePackageId,
    },
};
use serde::Deserialize;
use serde_json::{
    json,
    Value as JsonValue,
};
use sync_types::{
    CanonicalizedModulePath,
    FunctionName,
    UserIdentityAttributes,
};
use tokio::sync::mpsc;
use udf::{
    validation::ValidatedPathAndArgs,
    SyscallStats,
    SyscallTrace,
};
use value::{
    base64,
    heap_size::WithHeapSize,
    ConvexObject,
    ConvexValue,
};

use crate::metrics::{
    log_download_time,
    log_external_deps_size_bytes_total,
    log_function_execution,
    log_import_time,
    log_node_source_map_missing,
    log_node_source_map_token_lookup_failed,
    log_overhead,
    log_total_executor_time,
    log_udf_time,
    node_executor,
};

pub fn error_response_json(message: &str) -> JsonValue {
    json!({
        "type": "error",
        "message": message,
    })
}

pub static EXECUTE_TIMEOUT_RESPONSE_JSON: LazyLock<JsonValue> = LazyLock::new(|| {
    error_response_json(
        "Function execution unexpectedly timed out. Check your function for infinite loops or \
         other long-running operations.",
    )
});

pub const ARGS_TOO_LARGE_RESPONSE_MESSAGE: &str =
    "Node actions arguments size is too large. The maximum size is 5 MiB. Reduce the size of the \
     arguments or consider using Convex runtime actions instead, which have a 16 MiB limit. See https://docs.convex.dev/functions/runtimes";

const NODE_ANALYZE_INITIAL_BACKOFF: Duration = Duration::from_millis(100);
const NODE_ANALYZE_MAX_BACKOFF: Duration = Duration::from_secs(5);

#[async_trait]
pub trait NodeExecutor: Sync + Send {
    fn enable(&self) -> anyhow::Result<()>;
    async fn invoke(
        &self,
        request: ExecutorRequest,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
    ) -> anyhow::Result<InvokeResponse>;
    async fn shutdown(&self) -> anyhow::Result<()>;
}

pub struct InvokeResponse {
    pub response: JsonValue,
    pub aws_request_id: Option<String>,
}

#[derive(Clone)]
pub struct NodeActions<RT: Runtime> {
    executor: Arc<dyn NodeExecutor>,
    convex_origin: ConvexOrigin,
    user_timeout: Duration,
    runtime: RT,
    deployment: DeploymentMetadata,
}

fn construct_js_error(
    error_message: String,
    error_name: String,
    serialized_custom_data: Option<String>,
    frames: Option<Vec<FrameData>>,
    source_maps: &BTreeMap<CanonicalizedModulePath, SourceMap>,
) -> anyhow::Result<JsError> {
    // Only format the error message if we have frames,
    // as errors without frames come from outside the
    // action execution.
    let message = if frames.is_some() {
        format_uncaught_error(error_message, error_name)
    } else {
        error_message
    };
    let (message, custom_data) = deserialize_udf_custom_error(message, serialized_custom_data)?;
    let error = match frames {
        Some(mut frames) => {
            // Discard frames that aren't in the convex scheme, which may correspond
            // either to our node-executor's source or node's main.
            frames.retain(|frame| match frame.file_name {
                Some(ref f) => f.starts_with("convex:/"),
                None => false,
            });
            JsError::from_frames(message, frames, custom_data, |specifier| {
                if !specifier.as_str().starts_with("convex:/") {
                    return Ok(None);
                }
                let Some(path) = specifier.path().strip_prefix("/user/") else {
                    return Ok(None);
                };
                let module_path = path.parse()?;
                let Some(source_map) = source_maps.get(&module_path) else {
                    return Ok(None);
                };
                Ok(source_map_from_slice(source_map.as_bytes()))
            })
        },
        None => JsError::from_message(message),
    };
    Ok(error)
}

impl<RT: Runtime> NodeActions<RT> {
    pub fn new(
        executor: Arc<dyn NodeExecutor>,
        convex_origin: ConvexOrigin,
        user_timeout: Duration,
        runtime: RT,
        deployment: DeploymentMetadata,
    ) -> Self {
        Self {
            executor,
            convex_origin,
            user_timeout,
            runtime,
            deployment,
        }
    }

    pub fn enable(&self) -> anyhow::Result<()> {
        self.executor.enable()
    }

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        self.executor.shutdown().await
    }

    #[rustfmt::skip]
    pub async fn execute(
        &self,
        request: ExecuteRequest,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
        source_maps_callback: impl Future<Output = anyhow::Result<
            BTreeMap<CanonicalizedModulePath, SourceMap>>>
            + Send,
    ) -> anyhow::Result<NodeActionOutcome> {
        let path = request.path_and_args.path().clone();
        let timer = node_executor("execute");
        let request = ExecutorRequest::Execute {
            request,
            backend_address: self.convex_origin.clone(),
            // Use the user facing timeout here, which should be less than the
            // total Node timeout. This allows us to preempt early and give
            // better error message and logs in the common case.
            timeout: self.user_timeout,
            deployment: self.deployment.clone(),
        };
        let InvokeResponse {
            response,
            aws_request_id,
        } = self.executor.invoke(request, log_line_sender).await?;
        let execute_result = ExecuteResponse::try_from(response.clone()).with_context(|| {
            format!("Failed to deserialize execute (aws_request_id: {aws_request_id:?}) Response: {response}")
        })?;

        tracing::debug!(
            "Total:{:?}, executor:{:?}, download:{:?}, import:{:?}, udf:{:?}, \
             env_invocations:{:?}, memory_allocated_mb:{:?}, egress_bytes:{:?}, aws_request_id:{:?}",
            timer.elapsed(),
            execute_result.total_executor_time,
            execute_result.download_time,
            execute_result.import_time,
            execute_result.udf_time,
            execute_result.num_invocations,
            execute_result.memory_allocated_mb,
            execute_result.egress_bytes,
            aws_request_id,
        );
        let total_time = timer.finish();
        if let Some(download_time) = execute_result.download_time {
            log_download_time(download_time);
        }
        if let Some(import_time) = execute_result.import_time {
            log_import_time(import_time);
        }
        if let Some(udf_time) = execute_result.udf_time {
            log_udf_time(udf_time);
            if total_time > udf_time {
                log_overhead(total_time - udf_time);
            } else {
                log_overhead(Duration::new(0, 0));
            }
        }
        if let Some(total_executor_time) = execute_result.total_executor_time {
            log_total_executor_time(total_executor_time);
        }
        let cold_start = execute_result.num_invocations.map(|n| n == 1);
        log_function_execution(cold_start);

        let syscall_trace = execute_result.syscall_trace;

        let result = match execute_result.result {
            ExecuteResponseResult::Success { udf_return, .. } => {
                deserialize_udf_result(&path, &udf_return)?
            },
            ExecuteResponseResult::Error {
                message,
                name,
                data,
                frames,
                ..
            } => {
                let source_maps = source_maps_callback.await?;
                let error = construct_js_error(message, name, data, frames, &source_maps)?;
                Err(error)
            },
        };
        Ok(NodeActionOutcome {
            result,
            syscall_trace,
            memory_used_in_mb: execute_result.memory_allocated_mb,
            egress_bytes: execute_result.egress_bytes,
        })
    }

    #[fastrace::trace]
    pub async fn build_deps(
        &self,
        request: BuildDepsRequest,
    ) -> anyhow::Result<Result<(Sha256Digest, PackageSize), JsError>> {
        let timer = node_executor("build_deps");
        let (log_line_sender, _log_line_receiver) = mpsc::unbounded_channel();
        let request = ExecutorRequest::BuildDeps(request);
        let InvokeResponse {
            response,
            aws_request_id,
        } = self.executor.invoke(request, log_line_sender).await?;
        let response: BuildDepsResponse =
            serde_json::from_value(response.clone()).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to deserialize build_deps response: {e}. Response: {response}",
                )
            })?;

        let result = match response {
            BuildDepsResponse::Success {
                sha256_digest,
                zipped_size_bytes,
                unzipped_size_bytes,
            } => {
                let pkg_size = PackageSize {
                    zipped_size_bytes,
                    unzipped_size_bytes,
                };
                tracing::info!("External deps package size: {}", pkg_size);
                log_external_deps_size_bytes_total(pkg_size);

                Ok(Ok((Sha256Digest::from(sha256_digest), pkg_size)))
            },
            BuildDepsResponse::Error { message, frames } => {
                if let Some(frames) = frames {
                    Ok(Err(JsError::from_frames(message, frames, None, |_| {
                        Ok(None)
                    })))
                } else {
                    Ok(Err(JsError::from_message(message)))
                }
            },
        };

        tracing::info!(
            "build_deps took {:?}. aws_request_id={:?}",
            timer.elapsed(),
            aws_request_id
        );
        timer.finish();

        result
    }

    async fn invoke_analyze(
        &self,
        request: AnalyzeRequest,
    ) -> anyhow::Result<(InvokeResponse, LogLines)> {
        let mut backoff = Backoff::new(NODE_ANALYZE_INITIAL_BACKOFF, NODE_ANALYZE_MAX_BACKOFF);
        let mut retries = 0;
        loop {
            let (log_line_sender, log_line_receiver) = mpsc::unbounded_channel();
            let request = ExecutorRequest::Analyze(request.clone());
            let (response, log_lines) = run_function_and_collect_log_lines(
                self.executor.invoke(request, log_line_sender).boxed(),
                log_line_receiver,
                |_| {},
            )
            .await;
            match response {
                Ok(response) => return Ok((response, log_lines)),
                Err(e) => {
                    if retries >= *NODE_ANALYZE_MAX_RETRIES || e.is_deterministic_user_error() {
                        return Err(e);
                    }
                    tracing::warn!("Failed to invoke analyze: {:?}", e);
                    retries += 1;
                    let duration = backoff.fail(&mut self.runtime.rng());
                    self.runtime.wait(duration).await;
                },
            }
        }
    }

    #[fastrace::trace]
    pub async fn analyze(
        &self,
        request: AnalyzeRequest,
        source_maps: &BTreeMap<CanonicalizedModulePath, SourceMap>,
    ) -> anyhow::Result<Result<BTreeMap<CanonicalizedModulePath, AnalyzedModule>, JsError>> {
        let timer = node_executor("analyze");

        let (
            InvokeResponse {
                response,
                aws_request_id,
            },
            log_lines,
        ) = self.invoke_analyze(request).await?;
        let response: AnalyzeResponse = serde_json::from_value(response.clone()).map_err(|e| {
            anyhow::anyhow!("Failed to deserialize analyze response: {e}. Response: {response}")
        })?;
        tracing::info!(
            "Analyze took {:?}. aws_request_id={:?}",
            timer.elapsed(),
            aws_request_id
        );
        timer.finish();

        let modules = match response {
            AnalyzeResponse::Success { modules } => modules,
            AnalyzeResponse::Error { message, frames } => {
                let error = construct_js_error(message, "".to_string(), None, frames, source_maps)?;
                return Ok(Err(append_logs_to_error(error, log_lines)));
            },
        };
        let mut result = BTreeMap::new();
        for (path, node_functions) in modules {
            let path = path.parse()?;
            // We have no concept of the origin of a Function in the Node environment, so
            // this logic just assumes the line number of the Function belongs to the
            // current module.

            // See if we have a source map that has a 'sources' field that matches the
            // current module path. We also extract the source_index which
            // corresponds to the index of the matching 'sources' field so that
            // we can access the source_contents field at the same index when
            // rebuilding the source.
            let mut source_map = None;
            let mut source_index = None;
            if let Some(buf) = source_maps.get(&path) {
                let Some(candidate_source_map) = source_map_from_slice(buf.as_bytes()) else {
                    continue;
                };
                for (i, filename) in candidate_source_map.sources().enumerate() {
                    let filename = Path::new(filename);
                    let module_path = Path::new(path.as_str());
                    if filename.file_stem() != module_path.file_stem() {
                        continue;
                    }
                    if candidate_source_map.get_source_contents(i as u32).is_some() {
                        source_index = Some(i as u32);
                    }
                    source_map = Some(candidate_source_map);
                    break;
                }
            }

            // For each analyzed function, extract analyzed function and line numbers
            let mut functions = vec![];
            for f in node_functions {
                let udf_type = f.udf_type.as_str().parse()?;
                if udf_type != UdfType::Action {
                    return Ok(Err(JsError::from_message(format!(
                        "`{}` defined in `{:?}` is a {} function. Only \
                         actions can be defined in Node.js. See https://docs.convex.dev/functions/actions for more details.",
                        f.name, path, udf_type,
                    ))));
                }
                let args = match f.args {
                    Some(json_args) => match ArgsValidator::try_from(json_args) {
                        Ok(validator) => validator,
                        Err(parse_error) => {
                            let message =
                                format!("Unable to parse JSON from `exportArgs`:\n{parse_error}");
                            return Ok(Err(JsError::from_message(message)));
                        },
                    },
                    None => ArgsValidator::Unvalidated,
                };
                let returns = match f.returns {
                    Some(json_returns) => {
                        ReturnsValidator::try_from(json_returns).map_err(|e| {
                            ErrorMetadata::bad_request(
                                "InvalidNodeActionReturnsValidator",
                                format!(
                                    "The return validator of `{}` defined in `{:?}` is \
                                     invalid:\n{e}",
                                    f.name, path
                                ),
                            )
                        })?
                    },
                    None => ReturnsValidator::Unvalidated,
                };
                let visibility = f.visibility.map(Visibility::from);

                // Extract source position
                let pos = if let Some(Some(token)) =
                    source_map.as_ref().map(|map| map.lookup_token(f.lineno, 0))
                {
                    Some(AnalyzedSourcePosition {
                        path: path.clone(),
                        start_lineno: token.get_src_line(),
                        start_col: token.get_src_col(),
                    })
                } else {
                    if source_map.is_none() {
                        log_node_source_map_missing();
                    } else {
                        log_node_source_map_token_lookup_failed();
                    }

                    None
                };
                let function_name: FunctionName = f
                    .name
                    .parse()
                    .map_err(|e| invalid_function_name_error(&path, &e))?;
                functions.push(AnalyzedFunction::new(
                    function_name,
                    pos,
                    udf_type,
                    visibility,
                    args,
                    returns,
                )?);
            }

            // Sort by line number where source position of None compares least
            functions.sort_by(|a, b| a.pos.cmp(&b.pos));

            let functions = WithHeapSize::from(functions);
            let module = AnalyzedModule {
                functions,
                source_index,
                http_routes: None,
                cron_specs: None,
                reuse_context: false,
            };
            result.insert(path, module);
        }
        Ok(Ok(result))
    }
}

pub enum ExecutorRequest {
    Execute {
        request: ExecuteRequest,
        backend_address: ConvexOrigin,
        timeout: Duration,
        deployment: DeploymentMetadata,
    },
    Analyze(AnalyzeRequest),
    BuildDeps(BuildDepsRequest),
}

impl TryFrom<ExecutorRequest> for JsonValue {
    type Error = anyhow::Error;

    fn try_from(request: ExecutorRequest) -> anyhow::Result<Self> {
        let json = match request {
            ExecutorRequest::Execute {
                request: r,
                backend_address,
                timeout,
                deployment,
            } => {
                let environment_variables: Vec<JsonValue> = r
                    .environment_variables
                    .into_iter()
                    .map(|(name, value)| {
                        ConvexObject::try_from(PersistedEnvironmentVariable(
                            EnvironmentVariable::new(name, value),
                        ))
                        .map(JsonValue::from)
                    })
                    .collect::<anyhow::Result<_>>()?;
                let (path, args, npm_version) = r.path_and_args.consume();
                // TODO(lee)
                anyhow::ensure!(path.component.is_root());
                let udf_path = path.udf_path;

                json!({
                    "type": "execute",
                    "udfPath": {
                        "canonicalizedPath": udf_path.module().as_str(),
                        "function": &udf_path.function_name()[..],
                    },
                    // The executor expects the args to be a serialized string.
                    "args": args.get(),
                    "sourcePackage": JsonValue::from(r.source_package),
                    "backendAddress": backend_address,
                    "timeoutSecs": timeout.as_secs_f64(),
                    "maxLogLines": *MAX_LOG_LINES_NODE_ACTIONS,
                    "serviceTokenGuaranteedLifetimeSecs":
                        GET_SERVICE_TOKEN_GUARANTEED_LIFETIME.as_secs_f64(),
                    "backendCallbackToken": r.callback_token,
                    "authHeader": r.auth_header,
                    "userIdentity": r.user_identity.map(JsonValue::try_from).transpose()?,
                    "environmentVariables": JsonValue::Array(environment_variables),
                    "npmVersion": npm_version.map(|v| v.to_string()),
                    "executionContext": JsonValue::from(r.context),
                    "encodedParentTrace": JsonValue::from(r.encoded_parent_trace),
                    "deployment": {
                        "name": deployment.name,
                        "region": deployment.region,
                        "class": deployment.class,
                    },
                })
            },
            ExecutorRequest::Analyze(r) => {
                let environment_variables: Vec<JsonValue> = r
                    .environment_variables
                    .into_iter()
                    .map(|(name, value)| {
                        ConvexObject::try_from(PersistedEnvironmentVariable(
                            EnvironmentVariable::new(name, value),
                        ))
                        .map(JsonValue::from)
                    })
                    .collect::<anyhow::Result<_>>()?;
                json!({
                    "type": "analyze",
                    "sourcePackage": JsonValue::from(r.source_package),
                    "environmentVariables": JsonValue::Array(environment_variables),
                })
            },
            ExecutorRequest::BuildDeps(r) => {
                let deps: Vec<JsonValue> = r.deps.into_iter().map(JsonValue::from).collect();

                json!({
                    "type": "build_deps",
                    "uploadUrl": JsonValue::String(r.upload_url),
                    "deps": JsonValue::Array(deps),
                })
            },
        };
        Ok(json)
    }
}

#[derive(Debug, Clone)]
pub struct SourcePackage {
    pub bundled_source: Package,

    // Info of external package if external dependencies were specified.
    pub external_deps: Option<Package>,
}

impl From<SourcePackage> for JsonValue {
    fn from(value: SourcePackage) -> Self {
        let source_package: JsonValue = value.bundled_source.clone().into();
        let external_package: JsonValue = value
            .external_deps
            .map(JsonValue::from)
            .unwrap_or(JsonValue::Null);

        json!({
            "uri": value.bundled_source.uri.to_string(),
            "key": String::from(value.bundled_source.key),
            "sha256": base64::encode_urlsafe(&*value.bundled_source.sha256),
            "bundled_source": source_package,
            "external_deps": external_package,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Package {
    // Short-lived URI for fetching the source package, if desired.
    pub uri: String,

    // Stable key for caching the package.
    pub key: ObjectKey,

    // Checksum of the compressed package file.
    pub sha256: Sha256Digest,
}

impl From<Package> for JsonValue {
    fn from(value: Package) -> Self {
        // TODO: this is missing sha256 field
        json!({
            "uri": value.uri,
            "key": String::from(value.key),
            "sha256": base64::encode_urlsafe(&*value.sha256),
        })
    }
}

pub struct ExecuteRequest {
    // Note that the lambda executor expects arguments as string, which
    // then directly passes to invokeAction()
    pub path_and_args: ValidatedPathAndArgs,

    pub source_package: SourcePackage,
    pub source_package_id: SourcePackageId,
    pub user_identity: Option<UserIdentityAttributes>,
    pub auth_header: Option<String>,
    pub environment_variables: BTreeMap<EnvVarName, EnvVarValue>,

    pub callback_token: ActionCallbackToken,
    pub context: ExecutionContext,
    pub encoded_parent_trace: Option<String>,
}

#[derive(Debug, PartialEq)]
struct ExecuteResponse {
    result: ExecuteResponseResult,
    num_invocations: Option<usize>,
    download_time: Option<Duration>,
    import_time: Option<Duration>,
    udf_time: Option<Duration>,
    total_executor_time: Option<Duration>,
    syscall_trace: SyscallTrace,
    memory_allocated_mb: u64,
    egress_bytes: u64,
}

#[derive(Debug, PartialEq)]
enum ExecuteResponseResult {
    Success {
        udf_return: String,
    },
    Error {
        message: String,
        name: String,
        data: Option<String>,
        frames: Option<Vec<FrameData>>,
    },
}

#[derive(Debug)]
pub struct NodeActionOutcome {
    pub result: Result<ConvexValue, JsError>,
    pub syscall_trace: SyscallTrace,
    pub memory_used_in_mb: u64,
    pub egress_bytes: u64,
}

fn duration_from_millis_float(t: f64) -> Duration {
    Duration::from_micros((t * 1000.0) as u64)
}

impl TryFrom<JsonValue> for ExecuteResponse {
    type Error = anyhow::Error;

    fn try_from(v: JsonValue) -> anyhow::Result<Self> {
        #[derive(Deserialize, Debug, PartialEq)]
        #[serde(rename_all = "camelCase")]
        struct SyscallStatsJson {
            invocations: u32,
            errors: u32,
            total_duration_ms: f64,
        }
        impl From<SyscallStatsJson> for SyscallStats {
            fn from(value: SyscallStatsJson) -> Self {
                SyscallStats {
                    invocations: value.invocations,
                    errors: value.errors,
                    total_duration: duration_from_millis_float(value.total_duration_ms),
                }
            }
        }

        #[derive(Deserialize, Debug, PartialEq)]
        #[serde(tag = "type")]
        #[serde(rename_all = "camelCase")]
        enum ExecuteResponseJson {
            #[serde(rename_all = "camelCase")]
            Success {
                udf_return: String,
                num_invocations: usize,
                download_time_ms: Option<f64>,
                import_time_ms: Option<f64>,
                udf_time_ms: Option<f64>,
                total_executor_time_ms: Option<f64>,
                syscall_trace: Option<BTreeMap<String, SyscallStatsJson>>,
                memory_allocated_mb: u64,
                egress_bytes: u64,
            },
            #[serde(rename_all = "camelCase")]
            Error {
                message: String,
                name: Option<String>,
                data: Option<String>,
                frames: Option<Vec<FrameData>>,
                num_invocations: Option<usize>,
                download_time_ms: Option<f64>,
                import_time_ms: Option<f64>,
                udf_time_ms: Option<f64>,
                total_executor_time_ms: Option<f64>,
                syscall_trace: Option<BTreeMap<String, SyscallStatsJson>>,
                memory_allocated_mb: Option<u64>,
                egress_bytes: Option<u64>,
            },
        }
        let resp_json: ExecuteResponseJson = serde_json::from_value(v)?;
        let result = match resp_json {
            ExecuteResponseJson::Success {
                udf_return,
                num_invocations,
                download_time_ms,
                import_time_ms,
                udf_time_ms,
                total_executor_time_ms,
                syscall_trace,
                memory_allocated_mb,
                egress_bytes,
            } => ExecuteResponse {
                result: ExecuteResponseResult::Success { udf_return },
                num_invocations: Some(num_invocations),
                download_time: download_time_ms.map(duration_from_millis_float),
                import_time: import_time_ms.map(duration_from_millis_float),
                udf_time: udf_time_ms.map(duration_from_millis_float),
                total_executor_time: total_executor_time_ms.map(duration_from_millis_float),
                syscall_trace: syscall_trace
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(k, v)| (k, v.into()))
                    .collect::<BTreeMap<_, SyscallStats>>()
                    .into(),
                memory_allocated_mb,
                egress_bytes,
            },
            ExecuteResponseJson::Error {
                message,
                name,
                data,
                frames,
                num_invocations,
                download_time_ms,
                import_time_ms,
                udf_time_ms,
                total_executor_time_ms,
                syscall_trace,
                memory_allocated_mb,
                egress_bytes,
            } => ExecuteResponse {
                result: ExecuteResponseResult::Error {
                    message,
                    name: name.unwrap_or_default(),
                    data,
                    frames,
                },
                num_invocations,
                download_time: download_time_ms.map(duration_from_millis_float),
                import_time: import_time_ms.map(duration_from_millis_float),
                udf_time: udf_time_ms.map(duration_from_millis_float),
                total_executor_time: total_executor_time_ms.map(duration_from_millis_float),
                syscall_trace: syscall_trace
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(k, v)| (k, v.into()))
                    .collect::<BTreeMap<_, SyscallStats>>()
                    .into(),
                memory_allocated_mb: memory_allocated_mb.unwrap_or(512),
                egress_bytes: egress_bytes.unwrap_or(0),
            },
        };
        Ok(result)
    }
}

#[derive(Debug, Clone)]
pub struct AnalyzeRequest {
    pub source_package: SourcePackage,
    pub environment_variables: BTreeMap<EnvVarName, EnvVarValue>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum AnalyzeResponse {
    #[serde(rename_all = "camelCase")]
    Success {
        modules: BTreeMap<String, Vec<AnalyzedNodeFunction>>,
    },
    #[serde(rename_all = "camelCase")]
    Error {
        message: String,
        frames: Option<Vec<FrameData>>,
    },
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedNodeFunction {
    name: String,
    lineno: u32,
    udf_type: String,
    visibility: Option<VisibilityJson>,
    args: Option<ArgsValidatorJson>,
    returns: Option<ReturnsValidatorJson>,
}

#[derive(Debug)]
pub struct BuildDepsRequest {
    pub deps: Vec<NodeDependency>,
    pub upload_url: String,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum BuildDepsResponse {
    #[serde(rename_all = "camelCase")]
    Success {
        sha256_digest: [u8; 32],
        zipped_size_bytes: usize,
        unzipped_size_bytes: usize,
    },
    #[serde(rename_all = "camelCase")]
    Error {
        message: String,
        frames: Option<Vec<FrameData>>,
    },
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
#[serde(tag = "kind")]
#[serde(rename_all = "camelCase")]
pub enum VisibilityJson {
    Public,
    Internal,
}

impl From<VisibilityJson> for Visibility {
    fn from(value: VisibilityJson) -> Self {
        match value {
            VisibilityJson::Public => Visibility::Public,
            VisibilityJson::Internal => Visibility::Internal,
        }
    }
}

pub enum ResponsePart {
    LogLine(LogLine),
    Result(JsonValue),
}

fn parse_streamed_response(s: &str) -> anyhow::Result<Vec<ResponsePart>> {
    let parts = s.trim().split('\n');
    parts
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            let json_val = serde_json::from_str(part)?;
            if let JsonValue::Object(mut o) = json_val {
                if o.get("kind") == Some(&JsonValue::String("LogLine".to_string())) {
                    if let Some(value) = o.remove("data") {
                        return Ok(ResponsePart::LogLine(LogLine::Structured(
                            LogLineStructured::try_from(value)?,
                        )));
                    };
                } else {
                    return Ok(ResponsePart::Result(JsonValue::Object(o)));
                }
            }
            anyhow::bail!("Invalid part")
        })
        .try_collect()
}

pub enum NodeExecutorStreamPart {
    Chunk(Bytes),
    InvokeComplete(InvokeCompletion),
}

/// How the executor transport reported the end of an invocation.
pub enum InvokeCompletion {
    /// The invocation completed without any failure signal.
    Success,
    /// The transport authoritatively reported a failure (e.g. AWS Lambda set
    /// an error code). Returned to the caller, discarding any streamed
    /// payload.
    ExplicitError(InvokeResponse),
    /// A failure inferred from out-of-band signals (e.g. the AWS Lambda
    /// platform log tail). Returned to the caller only when the stream
    /// produced no result payload: the signal may describe a crash that
    /// happened after a complete response was already streamed.
    ImplicitError(InvokeResponse),
}

pub async fn handle_node_executor_stream(
    log_line_sender: mpsc::UnboundedSender<LogLine>,
    mut stream: impl Stream<Item = anyhow::Result<NodeExecutorStreamPart>> + Unpin,
) -> anyhow::Result<Result<JsonValue, InvokeResponse>> {
    let mut remaining_chunks: Vec<Bytes> = vec![];
    let mut result_values = vec![];
    while let Some(part) = stream.next().await {
        let part = part.with_context(|| "Error in node executor stream")?;
        match part {
            NodeExecutorStreamPart::Chunk(mut chunk) => {
                // Split any bytes from the previous chunk + the body of this chunk
                // into new lines and parse them as JSON objects.
                loop {
                    match memchr::memchr(b'\n', &chunk) {
                        None => {
                            if !chunk.is_empty() {
                                remaining_chunks.push(chunk);
                            }
                            break;
                        },
                        Some(pos) => {
                            let before_newline = &chunk[..pos];
                            let combined;
                            let line = if remaining_chunks.is_empty() {
                                before_newline
                            } else {
                                remaining_chunks.push(chunk.slice_ref(before_newline));
                                combined = remaining_chunks.concat();
                                remaining_chunks.clear();
                                &combined
                            };
                            let decoded_str = str::from_utf8(line)?;
                            let parts = parse_streamed_response(decoded_str)?;
                            for part in parts {
                                match part {
                                    ResponsePart::LogLine(log_line) => {
                                        log_line_sender.send(log_line)?;
                                    },
                                    ResponsePart::Result(result) => result_values.push(result),
                                };
                            }
                            chunk.advance(pos + 1);
                        },
                    }
                }
            },
            NodeExecutorStreamPart::InvokeComplete(completion) => {
                let error_if_no_result = match completion {
                    InvokeCompletion::ExplicitError(e) => return Ok(Err(e)),
                    InvokeCompletion::ImplicitError(e) => Some(e),
                    InvokeCompletion::Success => None,
                };
                let parsed = String::from_utf8(remaining_chunks.concat())
                    .map_err(anyhow::Error::from)
                    .and_then(|decoded_str| parse_streamed_response(&decoded_str));
                match parsed {
                    Ok(parts) => {
                        for part in parts {
                            match part {
                                ResponsePart::LogLine(log_line) => {
                                    log_line_sender.send(log_line)?;
                                },
                                ResponsePart::Result(result) => result_values.push(result),
                            };
                        }
                    },
                    // A crash can truncate the stream mid-line or mid-character;
                    // prefer the inferred failure over the decode error when we
                    // have one.
                    Err(e) if error_if_no_result.is_none() => return Err(e),
                    Err(_) => {},
                }
                if result_values.is_empty()
                    && let Some(e) = error_if_no_result
                {
                    return Ok(Err(e));
                }
                break;
            },
        }
    }
    anyhow::ensure!(
        result_values.len() <= 1,
        "Received more than one result from node executor response"
    );
    let payload = result_values
        .pop()
        .ok_or_else(|| anyhow::anyhow!("Received no result from node executor response"))?;
    Ok(Ok(payload))
}

fn append_logs_to_error(mut error: JsError, log_lines: LogLines) -> JsError {
    if log_lines.is_empty() {
        return error;
    }

    let len = log_lines.len();
    let logs_text = log_lines
        .into_iter()
        // Keep only the last 100 log entries
        .skip(len.saturating_sub(100))
        .flat_map(|l| l.to_pretty_strings())
        .collect::<Vec<_>>()
        .join("\n");

    error.message = format!("{}\n\n{}", error.message, logs_text);
    error
}
