use std::{
    collections::BTreeMap,
    fmt,
    fmt::Display,
    str::FromStr,
    time::Duration,
};

use serde::{
    ser::SerializeMap,
    Deserialize,
    Serialize,
    Serializer,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use tonic::async_trait;
use utoipa::ToSchema;
use value::heap_size::HeapSize;

use crate::{
    components::ComponentPath,
    errors::JsError,
    execution_context::ExecutionContext,
    log_lines::LogLineStructured,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    types::{
        FunctionCaller,
        ModuleEnvironment,
        QueryInvocation,
        UdfType,
        UdfTypeJson,
    },
    RequestMetadata,
};

/// Why a function was executed.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum FunctionRunReason {
    /// First execution of a query a client subscribed to.
    InitialSubscription,
    /// The query's subscription was no longer valid, so the query re-ran.
    DataChange,
    /// The client's auth identity changed, which reruns every query in the
    /// client's query set.
    IdentityChange,
    /// A mutation or action invoked by a client over the WebSocket sync
    /// protocol.
    WebSocket,
    /// Invoked via the HTTP API (e.g. `ConvexHttpClient`).
    HttpApi,
    /// An HTTP action serving an incoming request to a route in `http.ts`.
    HttpEndpoint,
    /// Invoked from a cron job.
    Cron,
    /// Invoked from a scheduled function.
    Scheduler,
    /// Invoked from an action via `ctx.runQuery`, `ctx.runMutation`, or
    /// `ctx.runAction`.
    Action,
    /// Invoked by the custom function tester (e.g. in the dashboard).
    Tester,
}

impl FunctionRunReason {
    /// `query_invocation` is `Some` exactly for queries, and distinguishes a
    /// first run from a rerun of an existing subscription.
    pub fn new(caller: &FunctionCaller, query_invocation: Option<QueryInvocation>) -> Self {
        let caller_reason = match caller {
            FunctionCaller::SyncWorker(_) => Self::WebSocket,
            FunctionCaller::HttpApi(_) => Self::HttpApi,
            FunctionCaller::HttpEndpoint => Self::HttpEndpoint,
            FunctionCaller::Tester(_) => Self::Tester,
            FunctionCaller::Cron => Self::Cron,
            FunctionCaller::Scheduler { .. } => Self::Scheduler,
            FunctionCaller::Action { .. } => Self::Action,
        };
        match query_invocation {
            None => caller_reason,
            Some(QueryInvocation::Invalidated) => Self::DataChange,
            Some(QueryInvocation::IdentityChange) => Self::IdentityChange,
            // Every caller other than a sync worker runs a query once rather
            // than holding a subscription to it.
            Some(QueryInvocation::Fresh) => {
                if caller_reason == Self::WebSocket {
                    Self::InitialSubscription
                } else {
                    caller_reason
                }
            },
        }
    }
}

/// Public worker for the LogManager.
#[async_trait]
pub trait LogSender: Send + Sync {
    fn send_logs(&self, logs: Vec<LogEvent>);
    async fn shutdown(&self) -> anyhow::Result<()>;
}

/// Structured log
#[derive(Debug, Clone)]
pub struct LogEvent {
    /// Rough timestamp of when this event was created, for the user's benefit.
    /// We provide no guarantees on the consistency of this timestamp across
    /// topics and log sources - it's best-effort.
    /// This timestamp is serialized to milliseconds.
    pub timestamp: UnixTimestamp,
    pub event: StructuredLogEvent,
}

/// User-facing UDF stats, that is logged in the UDF execution log
/// and might be used for debugging purposes.
///
/// TODO(sarah) this is nearly identical to the type in the `usage_tracking`
/// crate, but there's a dependency cycle preventing us from using it directly.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AggregatedFunctionUsageStats {
    pub database_read_bytes: u64,
    pub database_write_bytes: u64,
    pub database_io_read_bytes: u64,
    pub database_io_write_bytes: u64,
    pub database_read_documents: u64,
    pub database_write_documents: u64,
    pub database_write_index_rows: u64,
    pub storage_read_bytes: u64,
    pub storage_write_bytes: u64,
    pub vector_index_read_bytes: u64,
    pub vector_index_write_bytes: u64,
    pub text_index_write_query_bytes: u64,
    pub text_index_query_bytes: u64,
    pub vector_index_read_query_bytes: u64,
    pub vector_index_write_query_bytes: u64,
    pub network_egress_bytes: u64,
    pub memory_used_mb: u64,
    pub args_bytes: Option<u64>,
    pub return_bytes: Option<u64>,
    pub audit_log_egress_bytes: u64,
}

pub use errors::OccInfo;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OccInfoJson {
    pub table_name: Option<String>,
    pub document_id: Option<String>,
    pub write_source: Option<String>,
    pub component_path: Option<String>,
    pub retry_count: Option<u64>,
}

impl From<OccInfo> for OccInfoJson {
    fn from(info: OccInfo) -> Self {
        Self {
            table_name: info.table_name,
            document_id: info.document_id,
            write_source: info.write_source,
            component_path: info.component_path,
            retry_count: info.retry_count,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatsJson {
    pub database_read_bytes: u64,
    pub database_write_bytes: u64,
    pub database_io_read_bytes: u64,
    pub database_io_write_bytes: u64,
    pub database_read_documents: u64,
    pub database_write_documents: u64,
    pub database_write_index_rows: u64,
    pub storage_read_bytes: u64,
    pub storage_write_bytes: u64,
    pub vector_index_read_bytes: u64,
    pub vector_index_write_bytes: u64,
    pub text_index_query_bytes: u64,
    pub text_index_write_query_bytes: u64,
    pub vector_index_read_query_bytes: u64,
    pub vector_index_write_query_bytes: u64,
    pub network_egress_bytes: u64,
    pub memory_used_mb: u64,
}

// Nothing yet. Can add information like parent scheduled job, scheduler lag,
// etc.
#[derive(Serialize, Debug, Clone)]
pub struct SchedulerInfo {
    pub job_id: String,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct FunctionConcurrencyStats {
    pub num_running: u64,
    pub num_queued: u64,
}

// When adding a new event type:
// - add a Schema type in the tests at the bottom of this file
// - consider adding formatting of it in the CLI
// - add it to the docs
//
// Also consider getting rid of the V1 format!
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiGatewayFunctionEventSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub function_type: Option<String>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    ToSchema,
    strum::EnumString,
    strum::IntoStaticStr,
)]
pub enum AiGatewayProvider {
    #[serde(rename = "openRouter")]
    #[strum(serialize = "openRouter")]
    OpenRouter,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AiGatewayEndpoint {
    ChatCompletions,
    Decisions,
    Embeddings,
    Images,
    Messages,
    Responses,
    Speech,
    Transcriptions,
    Videos,
}

/// Combines the response outcome with usage availability.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    ToSchema,
    strum::EnumString,
    strum::IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AiGatewayOutcome {
    /// Finished with usage reported.
    Completed,
    /// Finished without usage; cost may still be recoverable via `upstream_id`.
    NoUsage,
    /// Upstream returned a non-success HTTP status.
    ProviderError,
    /// Transport failed while reading the body. Earlier chunks may contain
    /// an `upstream_id` for cost recovery.
    UpstreamError,
    /// Shutdown cancelled the read. Retains any usage and upstream ID received
    /// before cancellation; the provider may still have charged.
    Abandoned,
    /// Buffer ceiling discarded unparsed bytes that may have held usage.
    /// Recovery is via `upstream_id` from earlier chunks only.
    Oversized,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[derive(clap::ValueEnum, strum::IntoStaticStr)]
pub enum AiGatewayEnvironment {
    Production,
    Staging,
}

#[derive(Debug, Clone)]
pub enum StructuredLogEvent {
    /// Topic for verification logs. These are issued on sink startup and are
    /// used to test that the backend can authenticate with the sink.
    Verification,
    /// Topic for logs generated by `console.*` events. This is considered a
    /// `SystemLogTopic` since the topic is generated by the backend.
    Console {
        source: FunctionEventSource,
        log_line: LogLineStructured,
    },
    /// Topic that records UDF executions and provides information on the
    /// execution.
    FunctionExecution {
        source: FunctionEventSource,
        error: Option<JsError>,
        execution_time: Duration,
        user_execution_time: Option<Duration>,
        usage_stats: AggregatedFunctionUsageStats,
        occ_info: Option<OccInfo>,
        will_retry: bool,
        scheduler_info: Option<SchedulerInfo>,
        run_reason: FunctionRunReason,
    },
    /// Topic for exceptions. These happen when a UDF raises an exception from
    /// JS
    Exception {
        error: JsError,
        user_identifier: Option<sync_types::UserIdentifier>,
        source: FunctionEventSource,
        udf_server_version: Option<semver::Version>,
        request_metadata: RequestMetadata,
    },
    /// Topic for deployment audit logs. These are issued when developers
    /// interact with a deployment.
    DeploymentAuditLog {
        action: String,
        metadata: serde_json::Map<String, JsonValue>,
    },
    /// Topic for global stats from the scheduler. For function-specific stats,
    /// look in FunctionExecution
    SchedulerStats {
        lag_seconds: Duration,
        num_running_jobs: u64,
    },
    ScheduledJobLag {
        lag_seconds: Duration,
    },
    /// Topic for storage usage metrics. These are periodic snapshots of current
    /// storage state aggregated across all tables.
    CurrentStorageUsage {
        total_document_size_bytes: u64,
        total_index_size_bytes: u64,
        total_vector_storage_bytes: u64,
        total_text_storage_bytes: u64,
        total_file_storage_bytes: u64,
        total_backup_storage_bytes: u64,
        total_system_table_document_size_bytes: BTreeMap<String, u64>,
    },
    /// Topic for function concurrency metrics. These are periodic snapshots of
    /// running and queued functions for each function type.
    ConcurrencyStats {
        query: FunctionConcurrencyStats,
        mutation: FunctionConcurrencyStats,
        action: FunctionConcurrencyStats,
        node_action: FunctionConcurrencyStats,
        http_action: FunctionConcurrencyStats,
    },
    /// Topic for storage API bandwidth. These are emitted for direct
    /// /api/storage/* calls (not UDF-attributed storage operations).
    StorageApiBandwidth {
        storage_id: String,
        egress_bytes: u64,
    },
    /// Terminal usage reported by the AI gateway after an inference response
    /// has been drained.
    AiGatewayUsage {
        function: Option<AiGatewayFunctionEventSource>,
        inference_id: String,
        provider: AiGatewayProvider,
        endpoint: AiGatewayEndpoint,
        upstream_id: Option<String>,
        model: Option<String>,
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        total_tokens: Option<u64>,
        cached_prompt_tokens: Option<u64>,
        reasoning_tokens: Option<u64>,
        cost: Option<f64>,
        outcome: AiGatewayOutcome,
        environment: AiGatewayEnvironment,
    },
    /// Topic for log stream egress. Emitted when a integration sends a batch
    /// of events to an external service, reporting the egress bytes used.
    LogStreamEgress {
        egress_bytes: u64,
    },
    /// Topic for user-defined audit logs emitted via `audit.log` in UDFs.
    CustomAudit {
        body: JsonValue,
    },
    // User-specified topics -- not yet implemented.
    // See here for more details: https://www.notion.so/Log-Streaming-in-Convex-19a1dfadd6924c33b29b2796b0f5b2e2
    // User {
    //     topic: String,
    //     payload: serde_json::Map<String, JsonValue>
    // },
}

// The set of topics that a `StructuredLogEvent` can belong to. Each variant of
// `StructuredLogEvent` maps to exactly one `LogTopic` via
// `StructuredLogEvent::topic`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    ToSchema,
    strum::EnumString,
    strum::IntoStaticStr,
    strum::Display,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum LogTopic {
    Verification,
    Console,
    FunctionExecution,
    Exception,
    AuditLog,
    SchedulerStats,
    ScheduledJobLag,
    CurrentStorageUsage,
    ConcurrencyStats,
    StorageApiBandwidth,
    AiGatewayUsage,
    LogStreamEgress,
    CustomAudit,
}

impl LogTopic {
    /// The topics that can be configured on a log stream subscription.
    pub const SUBSCRIBABLE: &'static [LogTopic] = &[
        LogTopic::Console,
        LogTopic::FunctionExecution,
        LogTopic::AuditLog,
        LogTopic::SchedulerStats,
        LogTopic::ScheduledJobLag,
        LogTopic::CurrentStorageUsage,
        LogTopic::ConcurrencyStats,
        LogTopic::StorageApiBandwidth,
        LogTopic::AiGatewayUsage,
        LogTopic::LogStreamEgress,
        LogTopic::CustomAudit,
    ];

    /// Whether this topic can be configured on a log stream subscription.
    pub fn is_subscribable(&self) -> bool {
        Self::SUBSCRIBABLE.contains(self)
    }
}

impl StructuredLogEvent {
    pub fn topic(&self) -> LogTopic {
        match self {
            StructuredLogEvent::Verification => LogTopic::Verification,
            StructuredLogEvent::Console { .. } => LogTopic::Console,
            StructuredLogEvent::FunctionExecution { .. } => LogTopic::FunctionExecution,
            StructuredLogEvent::Exception { .. } => LogTopic::Exception,
            StructuredLogEvent::DeploymentAuditLog { .. } => LogTopic::AuditLog,
            StructuredLogEvent::SchedulerStats { .. } => LogTopic::SchedulerStats,
            StructuredLogEvent::ScheduledJobLag { .. } => LogTopic::ScheduledJobLag,
            StructuredLogEvent::CurrentStorageUsage { .. } => LogTopic::CurrentStorageUsage,
            StructuredLogEvent::ConcurrencyStats { .. } => LogTopic::ConcurrencyStats,
            StructuredLogEvent::StorageApiBandwidth { .. } => LogTopic::StorageApiBandwidth,
            StructuredLogEvent::AiGatewayUsage { .. } => LogTopic::AiGatewayUsage,
            StructuredLogEvent::LogStreamEgress { .. } => LogTopic::LogStreamEgress,
            StructuredLogEvent::CustomAudit { .. } => LogTopic::CustomAudit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ToSchema)]
pub enum LogEventFormatVersion {
    V1,
    V2,
}

impl FromStr for LogEventFormatVersion {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "1" => Ok(Self::V1),
            "2" => Ok(Self::V2),
            v => anyhow::bail!("Invalid LogEventFormatVersion: {v}"),
        }
    }
}

impl Display for LogEventFormatVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V1 => write!(f, "1"),
            Self::V2 => write!(f, "2"),
        }
    }
}

/// Structured log
impl LogEvent {
    pub fn default_for_verification<RT: Runtime>(runtime: &RT) -> anyhow::Result<Self> {
        Ok(Self {
            event: StructuredLogEvent::Verification,
            timestamp: runtime.unix_timestamp(),
        })
    }

    pub fn to_json_map(
        &self,
        format: LogEventFormatVersion,
    ) -> anyhow::Result<serde_json::Map<String, JsonValue>> {
        let object = self.to_json_serializer(format, serde_json::value::Serializer)?;
        let JsonValue::Object(fields) = object else {
            unreachable!();
        };
        Ok(fields)
    }

    pub fn to_json_serializer<S: Serializer>(
        &self,
        format: LogEventFormatVersion,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let ms = self
            .timestamp
            .as_ms_since_epoch()
            .map_err(serde::ser::Error::custom)?;
        macro_rules! serialize_map {
            ({$(
                $key:literal: $value:expr
            ),* $(,)?}) => {{
                let mut map_builder = serializer.serialize_map(None)?;
                $(map_builder.serialize_entry($key, &$value)?;)*
                map_builder.end()
            }}
        }
        match format {
            LogEventFormatVersion::V1 => match &self.event {
                StructuredLogEvent::Verification => {
                    serialize_map!({
                        "_timestamp": ms,
                        "_topic":  "_verification",
                        "message": "Convex connection test"
                    })
                },
                StructuredLogEvent::Console { source, log_line } => {
                    serialize_map!({
                        "_timestamp": ms,
                        "_topic":  "_console",
                        "_functionPath": source.udf_path,
                        "_functionType": source.udf_type,
                        "_functionCached": source.cached,
                        "message": log_line.to_pretty_string()
                    })
                },
                StructuredLogEvent::FunctionExecution {
                    source,
                    error,
                    execution_time,
                    user_execution_time,
                    usage_stats,
                    occ_info: _,
                    will_retry: _,
                    scheduler_info: _,
                    run_reason: _,
                } => {
                    let (reason, status) = match error {
                        Some(err) => (Some(err.to_string()), "failure"),
                        None => (None, "success"),
                    };
                    let execution_time_ms = execution_time.as_millis();
                    let user_execution_time_ms = user_execution_time.map(|d| d.as_millis());
                    serialize_map!({
                        "_timestamp": ms,
                        "_topic":  "_execution_record",
                        "_functionPath": source.udf_path,
                        "_functionType": source.udf_type,
                        "_functionCached": source.cached,
                        "status": status,
                        "reason": reason,
                        "executionTimeMs": execution_time_ms,
                        "userExecutionTimeMs": user_execution_time_ms,
                        "databaseReadBytes": usage_stats.database_read_bytes,
                        "databaseWriteBytes": usage_stats.database_write_bytes,
                        "databaseIoReadBytes": usage_stats.database_io_read_bytes,
                        "databaseIoWriteBytes": usage_stats.database_io_write_bytes,
                        "storageReadBytes": usage_stats.storage_read_bytes,
                        "storageWriteBytes": usage_stats.storage_write_bytes,
                        "networkEgressBytes": usage_stats.network_egress_bytes,
                    })
                },
                StructuredLogEvent::Exception {
                    error,
                    user_identifier,
                    source,
                    udf_server_version,
                    request_metadata: _,
                } => {
                    let message = &error.message;
                    let frames: Option<Vec<String>> = error
                        .frames
                        .as_ref()
                        .map(|frames| frames.0.iter().map(|frame| frame.to_string()).collect());
                    serialize_map!({
                        "_timestamp": ms,
                        "_topic":  "_exception",
                        "_functionPath": source.udf_path,
                        "_functionType": source.udf_type,
                        "_functionCached": source.cached,
                        "message": message,
                        "frames": frames,
                        "udfServerVersion": udf_server_version,
                        "userIdentifier": user_identifier,
                    })
                },
                StructuredLogEvent::DeploymentAuditLog { action, metadata } => {
                    serialize_map!({
                        "_timestamp": ms,
                        "_topic":  "_audit_log",
                        "action": action,
                        "actionMetadata": metadata
                    })
                },
                StructuredLogEvent::SchedulerStats {
                    lag_seconds,
                    num_running_jobs,
                } => serialize_map!({
                    "_timestamp": ms,
                    "_topic":  "_scheduler_stats",
                    "lag_seconds": lag_seconds.as_secs(), "num_running_jobs": num_running_jobs
                }),
                StructuredLogEvent::ScheduledJobLag { lag_seconds } => {
                    serialize_map!({
                        "_timestamp": ms,
                        "_topic":  "_scheduled_job_lag",
                        "lag_seconds": lag_seconds.as_secs()
                    })
                },
                StructuredLogEvent::CurrentStorageUsage {
                    total_document_size_bytes,
                    total_index_size_bytes,
                    total_vector_storage_bytes,
                    total_text_storage_bytes,
                    total_file_storage_bytes,
                    total_backup_storage_bytes,
                    total_system_table_document_size_bytes,
                } => serialize_map!({
                    "_timestamp": ms,
                    "_topic": "_current_storage_usage",
                    "total_document_size_bytes": total_document_size_bytes,
                    "total_index_size_bytes": total_index_size_bytes,
                    "total_vector_storage_bytes": total_vector_storage_bytes,
                    "total_text_storage_bytes": total_text_storage_bytes,
                    "total_file_storage_bytes": total_file_storage_bytes,
                    "total_backup_storage_bytes": total_backup_storage_bytes,
                    "total_system_table_document_size_bytes": total_system_table_document_size_bytes,
                }),
                StructuredLogEvent::ConcurrencyStats {
                    query,
                    mutation,
                    action,
                    node_action,
                    http_action,
                } => serialize_map!({
                    "_timestamp": ms,
                    "_topic": "_concurrency_stats",
                    "query": query,
                    "mutation": mutation,
                    "action": action,
                    "node_action": node_action,
                    "http_action": http_action,
                }),
                StructuredLogEvent::StorageApiBandwidth {
                    storage_id,
                    egress_bytes,
                } => serialize_map!({
                    "_timestamp": ms,
                    "_topic": "_storage_api_bandwidth",
                    "storage_id": storage_id,
                    "egress_bytes": egress_bytes
                }),
                StructuredLogEvent::AiGatewayUsage {
                    function,
                    inference_id,
                    provider,
                    endpoint,
                    upstream_id,
                    model,
                    prompt_tokens,
                    completion_tokens,
                    total_tokens,
                    cached_prompt_tokens,
                    reasoning_tokens,
                    cost,
                    outcome,
                    environment,
                } => serialize_map!({
                    "_timestamp": ms,
                    "_topic": "_ai_gateway_usage",
                    "function": function,
                    "inference_id": inference_id,
                    "provider": provider,
                    "endpoint": endpoint,
                    "upstream_id": upstream_id,
                    "model": model,
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "total_tokens": total_tokens,
                    "cached_prompt_tokens": cached_prompt_tokens,
                    "reasoning_tokens": reasoning_tokens,
                    "cost": cost,
                    "cost_unit": "USD",
                    "outcome": outcome,
                    "environment": environment,
                }),
                StructuredLogEvent::LogStreamEgress { egress_bytes } => serialize_map!({
                    "_timestamp": ms,
                    "_topic": "_log_stream_egress",
                    "egress_bytes": egress_bytes
                }),
                StructuredLogEvent::CustomAudit { body } => {
                    serialize_map!({
                        "_timestamp": ms,
                        "_topic": "_custom_audit",
                        "body": body
                    })
                },
            },
            LogEventFormatVersion::V2 => match &self.event {
                StructuredLogEvent::Verification => {
                    serialize_map!({
                        "timestamp": ms,
                        "topic": "verification",
                        "message": "Convex connection test"
                    })
                },
                StructuredLogEvent::Console { source, log_line } => {
                    let function_source = source.to_json_map();
                    let LogLineStructured {
                        messages,
                        level,
                        timestamp,
                        is_truncated,
                        system_metadata,
                    } = log_line;
                    let timestamp_ms = timestamp
                        .as_ms_since_epoch()
                        .map_err(serde::ser::Error::custom)?;
                    serialize_map!({
                        "timestamp": timestamp_ms,
                        "topic": "console",
                        "function": function_source,
                        "log_level": level.to_string(),
                        "message": messages.join(" "),
                        "is_truncated": is_truncated,
                        "system_code": system_metadata.as_ref().map(|s| &s.code)
                    })
                },
                StructuredLogEvent::FunctionExecution {
                    source,
                    error,
                    execution_time,
                    user_execution_time,
                    usage_stats,
                    occ_info,
                    will_retry,
                    scheduler_info,
                    run_reason,
                } => {
                    let function_source = source.to_json_map();
                    let (status, error_message) = match error {
                        Some(error) => ("failure", Some(error.to_string())),
                        None => ("success", None),
                    };
                    #[derive(Serialize)]
                    struct Usage {
                        database_read_bytes: u64,
                        database_write_bytes: u64,
                        database_io_read_bytes: u64,
                        database_io_write_bytes: u64,
                        database_read_documents: u64,
                        database_write_documents: u64,
                        database_write_index_rows: u64,
                        file_storage_read_bytes: u64,
                        file_storage_write_bytes: u64,
                        vector_storage_read_bytes: u64,
                        vector_storage_write_bytes: u64,
                        text_search_query_bytes: u64,
                        text_search_write_query_bytes: u64,
                        vector_search_query_bytes: u64,
                        vector_search_write_query_bytes: u64,
                        network_egress_bytes: u64,
                        memory_used_mb: u64,
                        action_memory_used_mb: Option<u64>,
                        audit_log_egress_bytes: u64,
                        function_args_bytes: Option<u64>,
                        function_returns_bytes: Option<u64>,
                    }
                    let action_memory_used_mb = if source.udf_type == UdfType::Action
                        || source.udf_type == UdfType::HttpAction
                    {
                        Some(usage_stats.memory_used_mb)
                    } else {
                        None
                    };
                    let user_execution_time_ms = user_execution_time.map(|d| d.as_millis());
                    serialize_map!({
                        "timestamp": ms,
                        "topic": "function_execution",
                        "function": function_source,
                        "execution_time_ms": execution_time.as_millis(),
                        "user_execution_time_ms": user_execution_time_ms,
                        "status": status,
                        "error_message": error_message,
                        "occ_info": occ_info,
                        "will_retry": will_retry,
                        "scheduler_info": scheduler_info,
                        "run_reason": run_reason,
                        "usage": Usage {
                            database_read_bytes: usage_stats.database_read_bytes,
                            database_write_bytes: usage_stats.database_write_bytes,
                            database_io_read_bytes: usage_stats.database_io_read_bytes,
                            database_io_write_bytes: usage_stats.database_io_write_bytes,
                            database_read_documents: usage_stats.database_read_documents,
                            database_write_documents: usage_stats.database_write_documents,
                            database_write_index_rows: usage_stats.database_write_index_rows,
                            file_storage_read_bytes: usage_stats.storage_read_bytes,
                            file_storage_write_bytes: usage_stats.storage_write_bytes,
                            vector_storage_read_bytes: usage_stats.vector_index_read_bytes,
                            vector_storage_write_bytes: usage_stats.vector_index_write_bytes,
                            text_search_query_bytes: usage_stats.text_index_query_bytes,
                            text_search_write_query_bytes: usage_stats.text_index_write_query_bytes,
                            vector_search_query_bytes: usage_stats.vector_index_read_query_bytes,
                            vector_search_write_query_bytes: usage_stats
                                .vector_index_write_query_bytes,
                            network_egress_bytes: usage_stats.network_egress_bytes,
                            memory_used_mb: usage_stats.memory_used_mb,
                            action_memory_used_mb,
                            audit_log_egress_bytes: usage_stats.audit_log_egress_bytes,
                            function_args_bytes: usage_stats.args_bytes,
                            function_returns_bytes: usage_stats.return_bytes,
                        }
                    })
                },
                // This codepath is unused because we filter out logs in default_log_filter and
                // construct exception logs in the Sentry sink
                StructuredLogEvent::Exception {
                    error,
                    user_identifier,
                    source,
                    udf_server_version,
                    request_metadata: _,
                } => {
                    let message = &error.message;
                    let frames: Option<Vec<String>> = error
                        .frames
                        .as_ref()
                        .map(|frames| frames.0.iter().map(|frame| frame.to_string()).collect());
                    serialize_map!({
                        "_timestamp": ms,
                        "_topic":  "_exception",
                        "_functionPath": source.udf_path,
                        "_functionType": source.udf_type,
                        "_functionCached": source.cached,
                        "message": message,
                        "frames": frames,
                        "udfServerVersion": udf_server_version,
                        "userIdentifier": user_identifier,
                    })
                },
                StructuredLogEvent::DeploymentAuditLog { action, metadata } => {
                    serialize_map!({
                        "timestamp": ms,
                        "topic": "audit_log",
                        "audit_log_action": action,
                        // stringified JSON to avoid
                        "audit_log_metadata": serde_json::to_string(metadata).map_err(serde::ser::Error::custom)?
                    })
                },
                StructuredLogEvent::SchedulerStats {
                    lag_seconds,
                    num_running_jobs,
                } => {
                    serialize_map!({
                        "topic": "scheduler_stats",
                        "timestamp": ms,
                        "lag_seconds": lag_seconds.as_secs(),
                        "num_running_jobs": num_running_jobs
                    })
                },
                StructuredLogEvent::ScheduledJobLag { lag_seconds } => {
                    serialize_map!({
                        "timestamp": ms,
                        "topic": "scheduled_job_lag",
                        "lag_seconds": lag_seconds.as_secs()
                    })
                },
                StructuredLogEvent::CurrentStorageUsage {
                    total_document_size_bytes,
                    total_index_size_bytes,
                    total_vector_storage_bytes,
                    total_text_storage_bytes,
                    total_file_storage_bytes,
                    total_backup_storage_bytes,
                    total_system_table_document_size_bytes,
                } => {
                    serialize_map!({
                        "timestamp": ms,
                        "topic": "current_storage_usage",
                        "total_document_size_bytes": total_document_size_bytes,
                        "total_index_size_bytes": total_index_size_bytes,
                        "total_vector_storage_bytes": total_vector_storage_bytes,
                        "total_text_storage_bytes": total_text_storage_bytes,
                        "total_file_storage_bytes": total_file_storage_bytes,
                        "total_backup_storage_bytes": total_backup_storage_bytes,
                        "total_system_table_document_size_bytes": total_system_table_document_size_bytes,
                    })
                },
                StructuredLogEvent::ConcurrencyStats {
                    query,
                    mutation,
                    action,
                    node_action,
                    http_action,
                } => {
                    serialize_map!({
                        "timestamp": ms,
                        "topic": "concurrency_stats",
                        "query": query,
                        "mutation": mutation,
                        "action": action,
                        "node_action": node_action,
                        "http_action": http_action,
                    })
                },
                StructuredLogEvent::StorageApiBandwidth {
                    storage_id,
                    egress_bytes,
                } => {
                    serialize_map!({
                        "timestamp": ms,
                        "topic": "storage_api_bandwidth",
                        "storage_id": storage_id,
                        "egress_bytes": egress_bytes
                    })
                },
                StructuredLogEvent::AiGatewayUsage {
                    function,
                    inference_id,
                    provider,
                    endpoint,
                    upstream_id,
                    model,
                    prompt_tokens,
                    completion_tokens,
                    total_tokens,
                    cached_prompt_tokens,
                    reasoning_tokens,
                    cost,
                    outcome,
                    environment,
                } => {
                    serialize_map!({
                        "timestamp": ms,
                        "topic": "ai_gateway_usage",
                        "function": function,
                        "inference_id": inference_id,
                        "provider": provider,
                        "endpoint": endpoint,
                        "upstream_id": upstream_id,
                        "model": model,
                        "prompt_tokens": prompt_tokens,
                        "completion_tokens": completion_tokens,
                        "total_tokens": total_tokens,
                        "cached_prompt_tokens": cached_prompt_tokens,
                        "reasoning_tokens": reasoning_tokens,
                        "cost": cost,
                        "cost_unit": "USD",
                        "outcome": outcome,
                        "environment": environment,
                    })
                },
                StructuredLogEvent::LogStreamEgress { egress_bytes } => {
                    serialize_map!({
                        "timestamp": ms,
                        "topic": "log_stream_egress",
                        "egress_bytes": egress_bytes
                    })
                },
                StructuredLogEvent::CustomAudit { body } => {
                    serialize_map!({
                        "timestamp": ms,
                        "topic": "custom_audit",
                        "body": body
                    })
                },
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventSource {
    Function(FunctionEventSource),
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionEventSource {
    pub context: ExecutionContext,
    pub component_path: ComponentPath,
    pub udf_path: String,
    pub udf_type: UdfType,
    pub module_environment: ModuleEnvironment,
    // Only queries can be cached, so this is only Some for queries. This is important
    // information to transmit to the client to distinguish from logs users explicitly created
    // and logs that we created for by redoing a query when its readset changes.
    pub cached: Option<bool>,
    // For mutations, this is the length of the mutation queue at the time the mutation was
    // executed. This is useful for monitoring and debugging mutation queue backlogs.
    pub mutation_queue_length: Option<usize>,
    // For mutations, this is the number of previous failed executions before a successful one.
    pub mutation_retry_count: Option<usize>,
}

impl FunctionEventSource {
    pub fn to_json_map(&self) -> serde_json::Map<String, JsonValue> {
        let udf_type = match self.udf_type {
            UdfType::Query => "query",
            UdfType::Mutation => "mutation",
            UdfType::Action => "action",
            UdfType::HttpAction => "http_action",
        };
        let runtime = match self.module_environment {
            ModuleEnvironment::Isolate => Some("convex"),
            ModuleEnvironment::Node => Some("node"),
            ModuleEnvironment::Rust => Some("rust"),
            ModuleEnvironment::Invalid => None,
        };
        let JsonValue::Object(mut fields) = json!({
            "path": self.udf_path,
            "type": udf_type,
            "runtime": runtime,
            "cached": self.cached,
            "request_id": self.context.request_id.to_string(),
            "mutation_queue_length": self.mutation_queue_length,
            "mutation_retry_count": self.mutation_retry_count,
        }) else {
            unreachable!()
        };
        if let Some(component_path_str) = self.component_path.clone().serialize() {
            fields.insert(
                "component_path".to_string(),
                JsonValue::String(component_path_str),
            );
        }
        fields
    }
}

impl HeapSize for FunctionEventSource {
    fn heap_size(&self) -> usize {
        self.component_path.heap_size()
            + self.udf_path.heap_size()
            + self.udf_type.heap_size()
            + self.cached.heap_size()
            + self.mutation_queue_length.heap_size()
    }
}

// Types for the /api/stream_function_logs
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "kind")]
pub enum FunctionExecutionJson {
    #[serde(rename_all = "camelCase")]
    Completion {
        udf_type: UdfTypeJson,
        component_path: Option<String>,
        identifier: String,
        log_lines: Vec<JsonValue>,
        timestamp: f64,
        cached_result: bool,
        caller: String,
        parent_execution_id: Option<String>,
        execution_time: f64,
        user_execution_time: Option<f64>,
        success: Option<JsonValue>,
        error: Option<String>,
        request_id: String,
        execution_id: String,
        usage_stats: UsageStatsJson,
        return_bytes: Option<f64>,
        occ_info: Option<OccInfoJson>,
        will_retry: bool,
        execution_timestamp: f64,
        identity_type: String,
        environment: String,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        udf_type: UdfTypeJson,
        component_path: Option<String>,
        identifier: String,
        timestamp: f64,
        log_lines: Vec<JsonValue>,
        request_id: String,
        execution_id: String,
    },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StreamUdfExecutionResponse {
    pub entries: Vec<FunctionExecutionJson>,
    pub new_cursor: f64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StreamFunctionLogs {
    pub cursor: f64,
    pub session_id: Option<String>,
    pub client_request_counter: Option<u32>,
}

#[derive(Deserialize, Debug)]
pub struct StreamUdfExecutionQueryArgs {
    pub cursor: f64,
}
