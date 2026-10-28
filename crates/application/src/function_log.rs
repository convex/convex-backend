use std::{
    cell::Cell,
    collections::{
        BTreeMap,
        HashMap,
        VecDeque,
    },
    str::FromStr,
    sync::Arc,
    time::{
        Duration,
        SystemTime,
    },
};

use anyhow::Context as _;
use common::{
    components::{
        CanonicalizedComponentFunctionPath,
        ComponentPath,
    },
    errors::{
        report_error_sync,
        JsError,
    },
    execution_context::ExecutionContext,
    identity::InertIdentity,
    knobs::{
        self,
    },
    log_lines::{
        LogLine,
        LogLines,
    },
    log_streaming::{
        self,
        FunctionConcurrencyStats,
        FunctionEventSource,
        FunctionRunReason,
        LogEvent,
        LogSender,
        SchedulerInfo,
        StructuredLogEvent,
    },
    runtime::{
        Runtime,
        SpawnHandle,
        UnixTimestamp,
    },
    types::{
        CursorMs,
        FunctionCaller,
        HttpActionRoute,
        ModuleEnvironment,
        QueryInvocation,
        TableName,
        TableStats,
        UdfIdentifier,
        UdfType,
    },
};
use http::StatusCode;
use itertools::Itertools;
use parking_lot::Mutex;
use serde_json::{
    json,
    Value as JsonValue,
};
use sync_types::types::SerializedArgs;
use tokio::sync::oneshot;
use udf::{
    validation::{
        ValidatedActionOutcome,
        ValidatedUdfOutcome,
    },
    HttpActionOutcome,
    HttpActionRequestHead,
    SyscallTrace,
    UdfOutcome,
};
use udf_metrics::{
    CounterBucket,
    MetricName,
    MetricStore,
    MetricStoreConfig,
    MetricType,
    MetricsWindow,
    Percentile,
    Timeseries,
    UdfMetricsError,
};
use usage_tracking::{
    AggregatedFunctionUsageStats,
    CallType,
    FunctionUsageTracker,
    OccInfo,
    UsageCounter,
};
use value::{
    heap_size::{
        HeapSize,
        WithHeapSize,
    },
    sha256::Sha256Digest,
};

pub enum OutstandingFunctionState {
    Running,
    Queued,
}

/// A function's execution is summarized by this structure and stored in the
/// UdfExecutionLog
#[derive(Debug, Clone)]
pub struct FunctionExecution {
    pub params: UdfParams,

    /// When we return the function result and log it. For cached
    // queries this can be long after the query was actually
    // executed.
    pub unix_timestamp: UnixTimestamp,

    /// When the function ran. For cached queries this can be long before
    // this execution is requested and is logged.
    pub execution_timestamp: UnixTimestamp,

    /// How this UDF was executed, with read-only or read-write permissions
    pub udf_type: UdfType,

    /// Log lines that the UDF emitted via the `Console` API.
    pub log_lines: LogLines,

    /// Which tables were read or written to in the UDF execution?
    pub tables_touched: WithHeapSize<BTreeMap<TableName, TableStats>>,

    /// Was this UDF execution computed from scratch or cached?
    pub cached_result: bool,
    /// How long (in seconds) did executing this UDF take?
    pub execution_time: f64,
    /// How long (in seconds) did the user's code execute?
    pub user_execution_time: Option<Duration>,

    /// Who called this UDF?
    pub caller: FunctionCaller,

    /// What type of environment was this UDF run in? This can be `Invalid` if
    /// the user specified an invalid path for an action, and then we won't
    /// know whether the action was intended to run in V8 or Node.
    pub environment: ModuleEnvironment,

    /// What syscalls did this function execute?
    pub syscall_trace: SyscallTrace,

    /// Usage statistics for this instance
    pub usage_stats: AggregatedFunctionUsageStats,
    pub memory_used_mb: u64,
    /// Size of the serialized arguments in bytes, excluding HTTP actions.
    pub args_bytes: Option<u64>,
    /// Size of the returned value in bytes if the function execution was
    /// successful, excluding HTTP actions.
    pub return_bytes: Option<u64>,

    /// The Convex NPM package version pushed with the module version executed.
    pub udf_server_version: Option<semver::Version>,

    /// The identity under which the udf was executed. Must be inert - since it
    /// can only be for logging purposes - not giving any authorization
    /// power.
    pub identity: InertIdentity,

    pub context: ExecutionContext,

    /// The length of the mutation queue at the time the mutation was executed.
    /// Only applicable for mutations.
    pub mutation_queue_length: Option<usize>,

    // Number of retries prior to a successful execution. Only applicable for mutations.
    pub mutation_retry_count: Option<usize>,

    // If this execution resulted in an OCC error, this will be Some.
    pub occ_info: Option<OccInfo>,

    /// Whether this function will be retried (e.g. a mutation that OCCs or hits
    /// write throughput limits)
    pub will_retry: bool,

    // Whether this was a fresh call or a rerun. Only applicable for queries.
    pub query_invocation: Option<QueryInvocation>,
}

impl HeapSize for FunctionExecution {
    fn heap_size(&self) -> usize {
        self.params.heap_size()
            + self.log_lines.heap_size()
            + self.tables_touched.heap_size()
            + self.syscall_trace.heap_size()
            + self.context.heap_size()
    }
}

impl FunctionExecution {
    fn identifier(&self) -> UdfIdentifier {
        match &self.params {
            UdfParams::Function { identifier, .. } => UdfIdentifier::Function(identifier.clone()),
            UdfParams::Http { identifier, .. } => UdfIdentifier::Http(identifier.clone()),
        }
    }

    fn event_source(
        &self,
        sub_function_path: Option<&CanonicalizedComponentFunctionPath>,
    ) -> FunctionEventSource {
        let cached = if self.udf_type == UdfType::Query {
            Some(self.cached_result)
        } else {
            None
        };
        let (component_path, udf_path) = match sub_function_path {
            Some(path) => (path.component.clone(), path.udf_path.to_string()),
            None => {
                let udf_id = self.params.identifier_str();
                let component_path = match &self.params {
                    UdfParams::Function { identifier, .. } => identifier.component.clone(),
                    // TODO(ENG-7612): Support HTTP actions in components.
                    UdfParams::Http { .. } => ComponentPath::root(),
                };
                (component_path, udf_id)
            },
        };

        FunctionEventSource {
            component_path,
            udf_path,
            udf_type: self.udf_type,
            module_environment: self.environment,
            cached,
            context: self.context.clone(),
            mutation_queue_length: self.mutation_queue_length,
            mutation_retry_count: self.mutation_retry_count,
        }
    }

    fn console_log_events_for_log_line(
        &self,
        log_line: &LogLine,
        sub_function_path: Option<&CanonicalizedComponentFunctionPath>,
    ) -> Vec<LogEvent> {
        match log_line {
            LogLine::Structured(log_line) => {
                vec![LogEvent {
                    timestamp: self.unix_timestamp,
                    event: StructuredLogEvent::Console {
                        source: self.event_source(sub_function_path),
                        log_line: log_line.clone(),
                    },
                }]
            },
            LogLine::SubFunction { path, log_lines } => log_lines
                .iter()
                .flat_map(|log_line| self.console_log_events_for_log_line(log_line, Some(path)))
                .collect(),
        }
    }

    fn console_log_events(&self) -> Vec<LogEvent> {
        self.log_lines
            .iter()
            .flat_map(|line| self.console_log_events_for_log_line(line, None))
            .collect()
    }

    fn udf_execution_record_log_events(&self) -> anyhow::Result<Vec<LogEvent>> {
        let execution_time = Duration::from_secs_f64(self.execution_time);

        let mut events = vec![LogEvent {
            timestamp: self.unix_timestamp,
            event: StructuredLogEvent::FunctionExecution {
                source: self.event_source(None),
                error: self.params.err().cloned(),
                execution_time,
                user_execution_time: self.user_execution_time,
                occ_info: self.occ_info.clone(),
                will_retry: self.will_retry,
                scheduler_info: match self.caller {
                    FunctionCaller::Scheduler { job_id, .. } => Some(SchedulerInfo {
                        job_id: job_id.to_string(),
                    }),
                    _ => None,
                },
                run_reason: FunctionRunReason::new(&self.caller, self.query_invocation),
                usage_stats: log_streaming::AggregatedFunctionUsageStats {
                    database_read_bytes: self.usage_stats.database_read_bytes,
                    database_write_bytes: self.usage_stats.database_write_bytes,
                    database_io_read_bytes: self.usage_stats.database_io_read_bytes,
                    database_io_write_bytes: self.usage_stats.database_io_write_bytes,
                    database_read_documents: self.usage_stats.database_read_documents,
                    database_write_documents: self.usage_stats.database_write_documents,
                    database_write_index_rows: self.usage_stats.database_write_index_rows,
                    storage_read_bytes: self.usage_stats.storage_read_bytes,
                    storage_write_bytes: self.usage_stats.storage_write_bytes,
                    vector_index_read_bytes: self.usage_stats.vector_index_read_bytes,
                    vector_index_write_bytes: self.usage_stats.vector_index_write_bytes,
                    text_index_write_query_bytes: self.usage_stats.text_index_write_query_bytes,
                    text_index_query_bytes: self.usage_stats.text_index_query_bytes,
                    vector_index_read_query_bytes: self.usage_stats.vector_index_read_query_bytes,
                    vector_index_write_query_bytes: self.usage_stats.vector_index_write_query_bytes,
                    network_egress_bytes: self.usage_stats.network_egress_bytes,
                    memory_used_mb: self.memory_used_mb,
                    args_bytes: self.args_bytes,
                    return_bytes: self.return_bytes,
                    audit_log_egress_bytes: self.usage_stats.audit_log_egress_bytes,
                },
            },
        }];

        if let Some(err) = self.params.err() {
            events.push(LogEvent {
                timestamp: self.unix_timestamp,
                event: StructuredLogEvent::Exception {
                    error: err.clone(),
                    user_identifier: self.identity.user_identifier().cloned(),
                    source: self.event_source(None),
                    udf_server_version: self.udf_server_version.clone(),
                    request_metadata: self.context.request_metadata.clone(),
                },
            });
        }

        Ok(events)
    }
}

#[derive(Debug, Clone)]
pub struct FunctionExecutionProgress {
    /// Log lines that the UDF emitted via the `Console` API.
    pub log_lines: LogLines,

    pub event_source: FunctionEventSource,
    pub function_start_timestamp: UnixTimestamp,
}

impl HeapSize for FunctionExecutionProgress {
    fn heap_size(&self) -> usize {
        self.log_lines.heap_size() + self.event_source.heap_size()
    }
}

impl FunctionExecutionProgress {
    fn console_log_events_for_log_line(
        &self,
        log_line: &LogLine,
        sub_function_path: Option<&CanonicalizedComponentFunctionPath>,
    ) -> Vec<LogEvent> {
        match log_line {
            LogLine::Structured(log_line) => {
                let mut event_source = self.event_source.clone();
                if let Some(sub_function_path) = sub_function_path {
                    event_source.component_path = sub_function_path.component.clone();
                    event_source.udf_path = sub_function_path.udf_path.to_string();
                };
                vec![LogEvent {
                    timestamp: log_line.timestamp,
                    event: StructuredLogEvent::Console {
                        source: event_source,
                        log_line: log_line.clone(),
                    },
                }]
            },
            LogLine::SubFunction { path, log_lines } => log_lines
                .iter()
                .flat_map(|log_line| self.console_log_events_for_log_line(log_line, Some(path)))
                .collect(),
        }
    }

    fn console_log_events(&self) -> Vec<LogEvent> {
        self.log_lines
            .iter()
            .flat_map(|line| self.console_log_events_for_log_line(line, None))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub enum FunctionExecutionPart {
    Completion(FunctionExecution),
    Progress(FunctionExecutionProgress),
}

impl HeapSize for FunctionExecutionPart {
    fn heap_size(&self) -> usize {
        match self {
            FunctionExecutionPart::Completion(i) => i.heap_size(),
            FunctionExecutionPart::Progress(i) => i.heap_size(),
        }
    }
}

#[derive(Clone)]
pub struct ActionCompletion {
    pub outcome: ValidatedActionOutcome,
    pub execution_time: Duration,
    pub environment: ModuleEnvironment,
    pub memory_in_mb: u64,
    pub context: ExecutionContext,
    pub unix_timestamp: UnixTimestamp,
    pub caller: FunctionCaller,
    pub log_lines: LogLines,
}

impl ActionCompletion {
    pub fn log_lines(&self) -> &LogLines {
        &self.log_lines
    }
}

#[derive(Debug, Clone)]
pub enum UdfParams {
    Function {
        // Avoid storing the actual result because the json can be quite large.
        // Instead only store the error if there was one. If error is None, the
        // function succeeded.
        error: Option<JsError>,
        /// Path of the component and UDF that was executed.
        identifier: CanonicalizedComponentFunctionPath,
    },
    Http {
        result: Result<HttpActionStatusCode, JsError>,
        identifier: HttpActionRoute,
    },
}

impl HeapSize for UdfParams {
    fn heap_size(&self) -> usize {
        match self {
            UdfParams::Function { error, identifier } => error.heap_size() + identifier.heap_size(),
            UdfParams::Http { result, identifier } => result.heap_size() + identifier.heap_size(),
        }
    }
}

impl UdfParams {
    pub fn is_err(&self) -> bool {
        match self {
            UdfParams::Function { error, .. } => error.is_some(),
            UdfParams::Http { result, .. } => result.is_err(),
        }
    }

    fn err(&self) -> Option<&JsError> {
        match self {
            UdfParams::Function { error: Some(e), .. } => Some(e),
            UdfParams::Http { result: Err(e), .. } => Some(e),
            _ => None,
        }
    }

    pub fn identifier_str(&self) -> String {
        match self {
            Self::Function { identifier, .. } => identifier.udf_path.clone().strip().to_string(),
            Self::Http { identifier, .. } => identifier.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpActionStatusCode(pub StatusCode);

impl HeapSize for HttpActionStatusCode {
    fn heap_size(&self) -> usize {
        // StatusCode is a wrapper around u16
        0
    }
}

impl From<HttpActionStatusCode> for serde_json::Value {
    fn from(value: HttpActionStatusCode) -> Self {
        json!({
            "status": value.0.as_u16().to_string(),
        })
    }
}

pub enum TrackUsage {
    Track(FunctionUsageTracker),
    // We don't count usage for system errors since they're not the user's fault
    SystemError,
}

#[derive(PartialEq)]
pub enum UdfRate {
    Invocations,
    Errors,
    CacheHits,
    CacheMisses,
    SubscriptionInvalidations,
}

impl FromStr for UdfRate {
    type Err = anyhow::Error;

    fn from_str(r: &str) -> anyhow::Result<Self> {
        let udf_rate = match r {
            "invocations" => UdfRate::Invocations,
            "errors" => UdfRate::Errors,
            "cacheHits" => UdfRate::CacheHits,
            "cacheMisses" => UdfRate::CacheMisses,
            "subscriptionInvalidations" => UdfRate::SubscriptionInvalidations,
            _ => anyhow::bail!("Invalid UDF rate: {}", r),
        };
        Ok(udf_rate)
    }
}

pub enum TableRate {
    RowsRead,
    RowsWritten,
}

impl FromStr for TableRate {
    type Err = anyhow::Error;

    fn from_str(r: &str) -> anyhow::Result<Self> {
        let table_rate = match r {
            "rowsRead" => TableRate::RowsRead,
            "rowsWritten" => TableRate::RowsWritten,
            _ => anyhow::bail!("Invalid table rate: {}", r),
        };
        Ok(table_rate)
    }
}

#[derive(Clone)]
pub struct FunctionExecutionLog<RT: Runtime> {
    inner: Arc<Mutex<Inner<RT>>>,
    usage_tracking: UsageCounter,
    rt: RT,
    concurrency_stats_logger: Arc<Mutex<Box<dyn SpawnHandle>>>,
}

impl<RT: Runtime> FunctionExecutionLog<RT> {
    pub fn new(rt: RT, usage_tracking: UsageCounter, log_manager: Arc<dyn LogSender>) -> Self {
        let base_ts = rt.system_time();
        let inner = Arc::new(Mutex::new(Inner {
            rt: rt.clone(),
            num_execution_completions: 0,
            log: WithHeapSize::default(),
            log_waiters: vec![].into(),
            log_manager,
            metrics: MetricStore::new(
                base_ts,
                MetricStoreConfig {
                    bucket_width: *knobs::UDF_METRICS_BUCKET_WIDTH,
                    max_buckets: *knobs::UDF_METRICS_MAX_BUCKETS,
                    histogram_min_duration: *knobs::UDF_METRICS_MIN_DURATION,
                    histogram_max_duration: *knobs::UDF_METRICS_MAX_DURATION,
                    histogram_significant_figures: *knobs::UDF_METRICS_SIGNIFICANT_FIGURES,
                },
            ),
        }));

        let inner_for_task = inner.clone();

        // Spawn a background task to periodically send concurrency stats
        let concurrency_stats_logger = Arc::new(Mutex::new(rt.spawn(
            "concurrency_stats_logger",
            async move {
                let runtime = inner_for_task.lock().rt.clone();
                loop {
                    let bucket_width = *knobs::UDF_METRICS_BUCKET_WIDTH;
                    runtime.wait(bucket_width).await;

                    let now = runtime.system_time();

                    // Query all outstanding_functions gauges to get current concurrency stats
                    let inner = inner_for_task.lock();
                    let metrics = inner.metrics.clone();
                    drop(inner);

                    let start_time = now - bucket_width;

                    // Get stats for current and previous buckets
                    let current_stats =
                        Self::query_all_concurrency_stats(&metrics, start_time, now);
                    let prev_start_time = start_time - bucket_width;
                    let previous_stats =
                        Self::query_all_concurrency_stats(&metrics, prev_start_time, start_time);

                    // Only send event if stats have changed
                    if current_stats != previous_stats {
                        let (query, mutation, action, node_action, http_action) = current_stats;
                        let event = LogEvent {
                            timestamp: UnixTimestamp::from_system_time(start_time)
                                .unwrap_or_else(|| UnixTimestamp::from_system_time(now).unwrap()),
                            event: StructuredLogEvent::ConcurrencyStats {
                                query,
                                mutation,
                                action,
                                node_action,
                                http_action,
                            },
                        };

                        let inner = inner_for_task.lock();
                        inner.log_manager.send_logs(vec![event]);
                    }
                }
            },
        )));

        Self {
            inner,
            rt,
            usage_tracking,
            concurrency_stats_logger,
        }
    }

    pub fn shutdown(&self) {
        self.concurrency_stats_logger.lock().shutdown();
    }

    pub async fn log_query(
        &self,
        outcome: &UdfOutcome,
        tables_touched: BTreeMap<TableName, TableStats>,
        was_cached: bool,
        execution_time: Duration,
        caller: FunctionCaller,
        usage_tracking: FunctionUsageTracker,
        context: ExecutionContext,
        query_invocation: QueryInvocation,
    ) {
        self._log_query(
            outcome,
            tables_touched,
            was_cached,
            execution_time,
            caller,
            TrackUsage::Track(usage_tracking),
            context,
            query_invocation,
        )
        .await
    }

    pub async fn log_query_system_error(
        &self,
        e: &anyhow::Error,
        path: CanonicalizedComponentFunctionPath,
        arguments: SerializedArgs,
        identity: InertIdentity,
        start: tokio::time::Instant,
        caller: FunctionCaller,
        context: ExecutionContext,
        query_invocation: QueryInvocation,
    ) -> anyhow::Result<()> {
        // TODO: We currently synthesize a `UdfOutcome` for
        // an internal system error. If we decide we want to keep internal system errors
        // in the UDF execution log, we may want to plumb through stuff like log lines.
        let outcome = UdfOutcome::from_error(
            JsError::from_error_ref(e),
            path,
            arguments,
            identity,
            self.rt.clone(),
            None,
        )?;
        self._log_query(
            &outcome,
            BTreeMap::new(),
            false,
            start.elapsed(),
            caller,
            TrackUsage::SystemError,
            context,
            query_invocation,
        )
        .await;
        Ok(())
    }

    #[fastrace::trace]
    async fn _log_query(
        &self,
        outcome: &UdfOutcome,
        tables_touched: BTreeMap<TableName, TableStats>,
        was_cached: bool,
        execution_time: Duration,
        caller: FunctionCaller,
        usage: TrackUsage,
        context: ExecutionContext,
        query_invocation: QueryInvocation,
    ) {
        let aggregated = match usage {
            TrackUsage::Track(usage_tracker) => {
                let usage_stats = usage_tracker.gather_user_stats();
                let aggregated = usage_stats.aggregate();
                self.usage_tracking
                    .track_call(
                        UdfIdentifier::Function(outcome.path.clone()),
                        context.execution_id,
                        context.request_id.clone(),
                        if was_cached {
                            CallType::CachedQuery
                        } else {
                            CallType::UncachedQuery {
                                duration: execution_time,
                                user_execution_time: outcome.user_execution_time,
                                memory_in_mb: outcome.memory_in_mb,
                            }
                        },
                        outcome.result.is_ok(),
                        usage_stats,
                    )
                    .await;
                aggregated
            },
            TrackUsage::SystemError => AggregatedFunctionUsageStats::default(),
        };
        if outcome.path.is_system() {
            return;
        }
        let args_bytes = Some(outcome.arguments.heap_size() as u64);
        let return_bytes = outcome.result.as_ref().ok().map(|v| v.heap_size() as u64);
        let execution = FunctionExecution {
            params: UdfParams::Function {
                error: match &outcome.result {
                    Ok(_) => None,
                    Err(e) => Some(e.clone()),
                },
                identifier: outcome.path.clone(),
            },
            unix_timestamp: self.rt.unix_timestamp(),
            execution_timestamp: outcome.unix_timestamp,
            mutation_queue_length: None,
            udf_type: UdfType::Query,
            log_lines: outcome.log_lines.clone(),
            tables_touched: tables_touched.into(),
            cached_result: was_cached,
            execution_time: execution_time.as_secs_f64(),
            user_execution_time: outcome.user_execution_time,
            caller,
            environment: ModuleEnvironment::Isolate,
            syscall_trace: outcome.syscall_trace.clone(),
            usage_stats: aggregated,
            memory_used_mb: outcome.memory_in_mb,
            args_bytes,
            return_bytes,
            udf_server_version: outcome.udf_server_version.clone(),
            identity: outcome.identity.clone(),
            context,
            mutation_retry_count: None,
            occ_info: None,
            will_retry: false,
            query_invocation: Some(query_invocation),
        };
        self.log_execution(execution, true, true);
    }

    pub async fn log_mutation(
        &self,
        outcome: ValidatedUdfOutcome,
        tables_touched: BTreeMap<TableName, TableStats>,
        execution_time: Duration,
        caller: FunctionCaller,
        usage: FunctionUsageTracker,
        context: ExecutionContext,
        mutation_queue_length: Option<usize>,
        mutation_retry_count: usize,
    ) {
        self._log_mutation(
            outcome,
            tables_touched,
            execution_time,
            caller,
            TrackUsage::Track(usage),
            context,
            None,
            mutation_queue_length,
            mutation_retry_count,
            false,
        )
        .await
    }

    pub async fn log_mutation_system_error(
        &self,
        e: &anyhow::Error,
        path: CanonicalizedComponentFunctionPath,
        arguments: SerializedArgs,
        identity: InertIdentity,
        start: tokio::time::Instant,
        caller: FunctionCaller,
        context: ExecutionContext,
        mutation_queue_length: Option<usize>,
        mutation_retry_count: usize,
    ) -> anyhow::Result<()> {
        // TODO: We currently synthesize a `UdfOutcome` for
        // an internal system error. If we decide we want to keep internal system errors
        // in the UDF execution log, we may want to plumb through stuff like log lines.
        let outcome = ValidatedUdfOutcome::from_error(
            JsError::from_error_ref(e),
            path,
            arguments,
            identity,
            self.rt.clone(),
            None,
        )?;
        self._log_mutation(
            outcome,
            BTreeMap::new(),
            start.elapsed(),
            caller,
            TrackUsage::SystemError,
            context,
            None,
            mutation_queue_length,
            mutation_retry_count,
            false,
        )
        .await;
        Ok(())
    }

    pub async fn log_mutation_write_throughput_error(
        &self,
        e: &anyhow::Error,
        path: CanonicalizedComponentFunctionPath,
        arguments: SerializedArgs,
        identity: InertIdentity,
        start: tokio::time::Instant,
        caller: FunctionCaller,
        context: ExecutionContext,
        mutation_queue_length: Option<usize>,
        mutation_retry_count: usize,
        will_retry: bool,
    ) -> anyhow::Result<()> {
        let outcome = ValidatedUdfOutcome::from_error(
            JsError::from_error_ref(e),
            path,
            arguments,
            identity,
            self.rt.clone(),
            None,
        )?;
        self._log_mutation(
            outcome,
            Default::default(),
            start.elapsed(),
            caller,
            TrackUsage::SystemError,
            context,
            None,
            mutation_queue_length,
            mutation_retry_count,
            will_retry,
        )
        .await;
        Ok(())
    }

    pub async fn log_mutation_occ_error(
        &self,
        outcome: ValidatedUdfOutcome,
        tables_touched: BTreeMap<TableName, TableStats>,
        execution_time: Duration,
        caller: FunctionCaller,
        usage: FunctionUsageTracker,
        context: ExecutionContext,
        mut occ_info: OccInfo,
        mutation_queue_length: Option<usize>,
        mutation_retry_count: usize,
        occ_retry_count: usize,
        will_retry: bool,
    ) {
        occ_info.retry_count = Some(occ_retry_count as u64);
        self._log_mutation(
            outcome,
            tables_touched,
            execution_time,
            caller,
            TrackUsage::Track(usage),
            context,
            Some(occ_info),
            mutation_queue_length,
            mutation_retry_count,
            will_retry,
        )
        .await;
    }

    async fn _log_mutation(
        &self,
        outcome: ValidatedUdfOutcome,
        tables_touched: BTreeMap<TableName, TableStats>,
        execution_time: Duration,
        caller: FunctionCaller,
        usage: TrackUsage,
        context: ExecutionContext,
        occ_info: Option<OccInfo>,
        mutation_queue_length: Option<usize>,
        mutation_retry_count: usize,
        will_retry: bool,
    ) {
        let aggregated = match usage {
            TrackUsage::Track(usage_tracker) => {
                let usage_stats = usage_tracker.gather_user_stats();
                let aggregated = usage_stats.aggregate();
                self.usage_tracking
                    .track_call(
                        UdfIdentifier::Function(outcome.path.clone()),
                        context.execution_id,
                        context.request_id.clone(),
                        CallType::Mutation {
                            duration: execution_time,
                            user_execution_time: outcome.user_execution_time,
                            memory_in_mb: outcome.memory_in_mb,
                            occ_info: occ_info.clone(),
                        },
                        outcome.result.is_ok(),
                        usage_stats,
                    )
                    .await;
                aggregated
            },
            TrackUsage::SystemError => AggregatedFunctionUsageStats::default(),
        };
        if outcome.path.udf_path.is_system() {
            return;
        }
        let args_bytes = Some(outcome.arguments.heap_size() as u64);
        let return_bytes = outcome.result.as_ref().ok().map(|v| v.heap_size() as u64);
        let execution = FunctionExecution {
            params: UdfParams::Function {
                error: outcome.result.err(),
                identifier: outcome.path,
            },
            unix_timestamp: self.rt.unix_timestamp(),
            mutation_queue_length,
            execution_timestamp: outcome.unix_timestamp,
            udf_type: UdfType::Mutation,
            log_lines: outcome.log_lines,
            tables_touched: tables_touched.into(),
            cached_result: false,
            execution_time: execution_time.as_secs_f64(),
            user_execution_time: outcome.user_execution_time,
            caller,
            environment: ModuleEnvironment::Isolate,
            syscall_trace: outcome.syscall_trace,
            usage_stats: aggregated,
            memory_used_mb: outcome.memory_in_mb,
            args_bytes,
            return_bytes,
            udf_server_version: outcome.udf_server_version,
            identity: outcome.identity,
            context,
            mutation_retry_count: Some(mutation_retry_count),
            occ_info,
            will_retry,
            query_invocation: None,
        };
        self.log_execution(execution, true, true);
    }

    pub async fn log_action(&self, completion: ActionCompletion, usage: FunctionUsageTracker) {
        self._log_action(completion, TrackUsage::Track(usage)).await
    }

    pub async fn log_action_system_error(
        &self,
        e: &anyhow::Error,
        path: CanonicalizedComponentFunctionPath,
        arguments: SerializedArgs,
        identity: InertIdentity,
        start: tokio::time::Instant,
        caller: FunctionCaller,
        log_lines: LogLines,
        context: ExecutionContext,
    ) -> anyhow::Result<()> {
        // Synthesize an `ActionCompletion` for system errors.
        let unix_timestamp = self.rt.unix_timestamp();
        let completion = ActionCompletion {
            outcome: ValidatedActionOutcome::from_system_error(
                path,
                arguments,
                identity,
                unix_timestamp,
                e,
            ),
            execution_time: start.elapsed(),
            environment: ModuleEnvironment::Invalid,
            memory_in_mb: 0,
            context,
            unix_timestamp: self.rt.unix_timestamp(),
            caller,
            log_lines,
        };
        self._log_action(completion, TrackUsage::SystemError).await;
        Ok(())
    }

    async fn _log_action(&self, completion: ActionCompletion, usage: TrackUsage) {
        let outcome = completion.outcome;
        let log_lines = completion.log_lines;
        let aggregated = match usage {
            TrackUsage::Track(usage_tracker) => {
                let usage_stats = usage_tracker.gather_user_stats();
                let aggregated = usage_stats.aggregate();
                self.usage_tracking
                    .track_call(
                        UdfIdentifier::Function(outcome.path.clone()),
                        completion.context.execution_id,
                        completion.context.request_id.clone(),
                        CallType::Action {
                            env: completion.environment,
                            duration: completion.execution_time,
                            user_execution_time: outcome.user_execution_time,
                            memory_in_mb: completion.memory_in_mb,
                        },
                        outcome.result.is_ok(),
                        usage_stats,
                    )
                    .await;
                aggregated
            },
            TrackUsage::SystemError => AggregatedFunctionUsageStats::default(),
        };
        if outcome.path.udf_path.is_system() {
            return;
        }
        let args_bytes = Some(outcome.arguments.heap_size() as u64);
        let return_bytes = outcome.result.as_ref().ok().map(|v| v.heap_size() as u64);
        let execution = FunctionExecution {
            params: UdfParams::Function {
                error: outcome.result.err(),
                identifier: outcome.path,
            },
            unix_timestamp: self.rt.unix_timestamp(),
            mutation_queue_length: None,
            execution_timestamp: outcome.unix_timestamp,
            udf_type: UdfType::Action,
            log_lines,
            tables_touched: WithHeapSize::default(),
            cached_result: false,
            execution_time: completion.execution_time.as_secs_f64(),
            user_execution_time: outcome.user_execution_time,
            caller: completion.caller,
            environment: completion.environment,
            syscall_trace: outcome.syscall_trace,
            usage_stats: aggregated,
            memory_used_mb: completion.memory_in_mb,
            args_bytes,
            return_bytes,
            udf_server_version: outcome.udf_server_version,
            identity: outcome.identity,
            context: completion.context,
            mutation_retry_count: None,
            occ_info: None,
            will_retry: false,
            query_invocation: None,
        };
        self.log_execution(execution, /* send_console_events */ false, true)
    }

    pub fn log_action_progress(
        &self,
        path: CanonicalizedComponentFunctionPath,
        unix_timestamp: UnixTimestamp,
        context: ExecutionContext,
        log_lines: LogLines,
        module_environment: ModuleEnvironment,
    ) {
        if path.is_system() {
            return;
        }
        let event_source = FunctionEventSource {
            component_path: path.component,
            udf_path: path.udf_path.strip().to_string(),
            udf_type: UdfType::Action,
            module_environment,
            cached: Some(false),
            context,
            mutation_queue_length: None,
            mutation_retry_count: None,
        };

        self.log_execution_progress(log_lines, event_source, unix_timestamp)
    }

    pub async fn log_http_action(
        &self,
        outcome: HttpActionOutcome,
        result: Result<HttpActionStatusCode, JsError>,
        log_lines: LogLines,
        execution_time: Duration,
        caller: FunctionCaller,
        usage: FunctionUsageTracker,
        context: ExecutionContext,
        response_sha256: Sha256Digest,
    ) {
        self._log_http_action(
            outcome,
            result,
            log_lines,
            execution_time,
            caller,
            TrackUsage::Track(usage),
            context,
            response_sha256,
        )
        .await
    }

    pub async fn log_http_action_system_error(
        &self,
        error: &anyhow::Error,
        http_request: HttpActionRequestHead,
        identity: InertIdentity,
        start: tokio::time::Instant,
        caller: FunctionCaller,
        log_lines: LogLines,
        context: ExecutionContext,
        response_sha256: Sha256Digest,
    ) {
        let js_err = JsError::from_error_ref(error);
        let outcome = HttpActionOutcome::new(
            None,
            http_request,
            identity,
            self.rt.unix_timestamp(),
            udf::HttpActionResult::Error(js_err.clone()),
            None,
            None,
            Duration::ZERO,
        );
        self._log_http_action(
            outcome,
            Err(js_err),
            log_lines,
            start.elapsed(),
            caller,
            TrackUsage::SystemError,
            context,
            response_sha256,
        )
        .await
    }

    async fn _log_http_action(
        &self,
        outcome: HttpActionOutcome,
        result: Result<HttpActionStatusCode, JsError>,
        log_lines: LogLines,
        execution_time: Duration,
        caller: FunctionCaller,
        usage: TrackUsage,
        context: ExecutionContext,
        response_sha256: Sha256Digest,
    ) {
        // For usage tracking, substitute "[unmatched]" when the JS router
        // didn't match a route. This avoids route explosion from every unique
        // 404 URL becoming a separate billing entry. Function execution logs
        // preserve the actual URL for debugging.
        let usage_route = if outcome.route.matched {
            outcome.route.clone()
        } else {
            HttpActionRoute {
                method: outcome.route.method,
                path: "[unmatched]".to_string(),
                matched: false,
            }
        };
        let aggregated = match usage {
            TrackUsage::Track(usage_tracker) => {
                let usage_stats = usage_tracker.gather_user_stats();
                let aggregated = usage_stats.aggregate();
                self.usage_tracking
                    .track_call(
                        UdfIdentifier::Http(usage_route),
                        context.execution_id,
                        context.request_id.clone(),
                        CallType::HttpAction {
                            duration: execution_time,
                            user_execution_time: outcome.user_execution_time,
                            memory_in_mb: outcome.memory_in_mb(),
                            response_sha256,
                        },
                        result.clone().is_ok_and(|code| code.0.as_u16() < 400),
                        usage_stats,
                    )
                    .await;
                aggregated
            },
            TrackUsage::SystemError => AggregatedFunctionUsageStats::default(),
        };
        let execution = FunctionExecution {
            params: UdfParams::Http {
                result,
                identifier: outcome.route.clone(),
            },
            unix_timestamp: self.rt.unix_timestamp(),
            execution_timestamp: outcome.unix_timestamp,
            mutation_queue_length: None,
            udf_type: UdfType::HttpAction,
            log_lines,
            tables_touched: WithHeapSize::default(),
            cached_result: false,
            execution_time: execution_time.as_secs_f64(),
            user_execution_time: outcome.user_execution_time,
            caller,
            environment: ModuleEnvironment::Isolate,
            usage_stats: aggregated,
            memory_used_mb: outcome.memory_in_mb(),
            args_bytes: None,
            return_bytes: None,
            syscall_trace: outcome.syscall_trace,
            udf_server_version: outcome.udf_server_version,
            identity: outcome.identity,
            context,
            mutation_retry_count: None,
            occ_info: None,
            will_retry: false,
            query_invocation: None,
        };
        self.log_execution(
            execution,
            /* send_console_events */ false,
            /* log_app_metrics */ outcome.route.matched,
        );
    }

    pub fn log_http_action_progress(
        &self,
        identifier: HttpActionRoute,
        unix_timestamp: UnixTimestamp,
        context: ExecutionContext,
        log_lines: LogLines,
        module_environment: ModuleEnvironment,
    ) {
        let event_source = FunctionEventSource {
            // TODO(ENG-7612): Support HTTP actions in components.
            component_path: ComponentPath::root(),
            udf_path: identifier.to_string(),
            udf_type: UdfType::HttpAction,
            module_environment,
            cached: Some(false),
            context,
            mutation_queue_length: None,
            mutation_retry_count: None,
        };

        self.log_execution_progress(log_lines, event_source, unix_timestamp)
    }

    fn log_execution(
        &self,
        execution: FunctionExecution,
        send_console_events: bool,
        log_app_metrics: bool,
    ) {
        if let Err(mut e) =
            self.inner
                .lock()
                .log_execution(execution, send_console_events, log_app_metrics)
        {
            report_error_sync(&mut e);
        }
    }

    fn log_execution_progress(
        &self,
        log_lines: LogLines,
        event_source: FunctionEventSource,
        timestamp: UnixTimestamp,
    ) {
        if let Err(mut e) =
            self.inner
                .lock()
                .log_execution_progress(log_lines, event_source, timestamp)
        {
            report_error_sync(&mut e);
        }
    }

    /// Indicates that as of now (`timestamp`), the next scheduled job is at
    /// `next_job_ts` (None if there are no pending jobs)
    pub fn log_scheduled_job_stats(
        &self,
        next_job_ts: Option<SystemTime>,
        timestamp: SystemTime,
        num_running_jobs: u64,
    ) {
        if let Err(mut e) =
            self.inner
                .lock()
                .log_scheduled_job_stats(next_job_ts, timestamp, num_running_jobs)
        {
            report_error_sync(&mut e);
        }
    }

    pub fn record_subscription_invalidations(&self, events: Vec<database::InvalidationEvent>) {
        let ts = self.rt.system_time();
        let mut inner = self.inner.lock();
        for event in &events {
            if let Some(display_name) = event.write_source.as_ref().and_then(|ws| ws.display_name())
            {
                let name: MetricName = format!(
                    "subscription_invalidations:{display_name}:{}",
                    event.tablet_id
                );
                if let Err(e) = inner.metrics.add_counter(&name, ts, event.count as f32) {
                    Inner::<RT>::log_metrics_error(e);
                }
            }
        }
    }

    pub fn udf_rate(
        &self,
        identifier: UdfIdentifier,
        metric: UdfRate,
        window: MetricsWindow,
    ) -> anyhow::Result<Timeseries> {
        let metrics = {
            let inner = self.inner.lock();
            inner.metrics.clone()
        };
        let name = match metric {
            UdfRate::Invocations => udf_invocations_metric(&identifier),
            UdfRate::Errors => udf_errors_metric(&identifier),
            UdfRate::CacheHits => udf_cache_hits_metric(&identifier),
            UdfRate::CacheMisses => udf_cache_misses_metric(&identifier),
            UdfRate::SubscriptionInvalidations => {
                // Aggregate across all tablets for this mutation.
                let mutation_name = udf_metric_name(&identifier);
                let by_table = Self::get_subscription_invalidation_counter(
                    &window,
                    &metrics,
                    Some(&mutation_name),
                )?;
                if by_table.is_empty() {
                    return window.resample_counters(&metrics, vec![], true);
                }
                return sum_timeseries(&window, by_table.values());
            },
        };
        let buckets = metrics.query_counter(&name, window.start..window.end)?;
        window.resample_counters(&metrics, buckets, true)
    }

    pub fn cache_hit_percentage(
        &self,
        identifier: UdfIdentifier,
        window: MetricsWindow,
    ) -> anyhow::Result<Timeseries> {
        let metrics = {
            let inner = self.inner.lock();
            inner.metrics.clone()
        };
        let hits = metrics.query_counter(
            &udf_cache_hits_metric(&identifier),
            window.start..window.end,
        )?;
        let hits = window.resample_counters(&metrics, hits, false /* is_rate */)?;
        let misses = metrics.query_counter(
            &udf_cache_misses_metric(&identifier),
            window.start..window.end,
        )?;
        let misses = window.resample_counters(&metrics, misses, false /* is_rate */)?;

        merge_series(&hits, &misses, cache_hit_percentage)
    }

    pub fn failure_percentage_top_k(
        &self,
        window: MetricsWindow,
        k: usize,
    ) -> anyhow::Result<Vec<(String, Timeseries)>> {
        let metrics = {
            let inner = self.inner.lock();
            inner.metrics.clone()
        };

        // Get the invocations and errors
        let invocations = Self::get_udf_metric_counter(&window, &metrics, "invocations")?;
        let errors = Self::get_udf_metric_counter(&window, &metrics, "errors")?;

        Self::top_k_for_rate(&window, errors, invocations, k, percentage, false)
    }

    pub fn cache_hit_percentage_top_k(
        &self,
        window: MetricsWindow,
        k: usize,
    ) -> anyhow::Result<Vec<(String, Timeseries)>> {
        let metrics = {
            let inner = self.inner.lock();
            inner.metrics.clone()
        };

        // Get the invocations and hits
        let hits = Self::get_udf_metric_counter(&window, &metrics, "cache_hits")?;
        let misses = Self::get_udf_metric_counter(&window, &metrics, "cache_misses")?;

        Self::top_k_for_rate(&window, hits, misses, k, cache_hit_percentage, true)
    }

    pub fn get_all_function_calls(
        &self,
        window: &MetricsWindow,
    ) -> anyhow::Result<HashMap<String, Timeseries>> {
        let metrics = {
            let inner = self.inner.lock();
            inner.metrics.clone()
        };
        Self::get_udf_metric_counter(window, &metrics, "invocations")
    }

    pub fn function_call_count_top_k(
        &self,
        window: MetricsWindow,
        k: usize,
    ) -> anyhow::Result<Vec<(String, Timeseries)>> {
        let metrics = {
            let inner = self.inner.lock();
            inner.metrics.clone()
        };

        let mut invocations = Self::get_udf_metric_counter(&window, &metrics, "invocations")?;

        let mut overall: HashMap<&str, f64> = HashMap::new();
        for (udf_id, series) in invocations.iter() {
            let sum = series.iter().filter_map(|&(_, value)| value).sum1();
            if let Some(total) = sum {
                overall.insert(udf_id, total);
            }
        }

        let top_k = Self::top_k(&overall, k, false);

        let mut ret = vec![];
        for udf_id in top_k {
            let series = invocations
                .remove(&udf_id)
                .expect("everything in topk came from invocations");
            ret.push((udf_id.to_string(), series));
        }

        if !invocations.is_empty() {
            let rest = sum_timeseries(&window, invocations.values())?;
            ret.push(("_rest".to_string(), rest));
        }

        Ok(ret)
    }

    /// Returns the top-k subscription invalidation pairs.
    /// If `identifier` is provided, filters to that mutation and returns
    /// top-k by tablet. Otherwise returns top-k by (mutation, tablet) pairs.
    pub fn subscription_invalidations_top_k(
        &self,
        window: MetricsWindow,
        k: usize,
        identifier: Option<&UdfIdentifier>,
    ) -> anyhow::Result<Vec<(String, Timeseries)>> {
        let metrics = {
            let inner = self.inner.lock();
            inner.metrics.clone()
        };
        let mutation_filter = identifier.map(udf_metric_name);
        let mut counters = Self::get_subscription_invalidation_counter(
            &window,
            &metrics,
            mutation_filter.as_deref(),
        )?;

        let mut overall: HashMap<&str, f64> = HashMap::new();
        for (key, series) in counters.iter() {
            let sum = series.iter().filter_map(|&(_, value)| value).sum1();
            if let Some(total) = sum {
                overall.insert(key, total);
            }
        }

        let top_k = Self::top_k(&overall, k, false);

        let mut ret = vec![];
        for key in top_k {
            let series = counters
                .remove(&key)
                .expect("everything in topk came from counters");
            ret.push((key.to_string(), series));
        }

        if !counters.is_empty() {
            let rest = sum_timeseries(&window, counters.values())?;
            ret.push(("_rest".to_string(), rest));
        }

        Ok(ret)
    }

    /// Queries `subscription_invalidations:{mutation}:{tablet_id}` metrics.
    /// If `mutation_filter` is Some, only returns metrics for that mutation
    /// (with the mutation prefix stripped, so keys are tablet IDs).
    /// If None, returns all pairs (keys are `{mutation}:{tablet_id}`).
    fn get_subscription_invalidation_counter(
        window: &MetricsWindow,
        metrics: &MetricStore,
        mutation_filter: Option<&str>,
    ) -> anyhow::Result<HashMap<String, Timeseries>> {
        let metric_names = metrics.metric_names_for_type(MetricType::Counter);
        let prefix = match mutation_filter {
            Some(mutation) => format!("subscription_invalidations:{mutation}:"),
            None => "subscription_invalidations:".to_string(),
        };

        let filtered: Vec<_> = metric_names
            .iter()
            .filter(|name| name.starts_with(&prefix))
            .collect();

        let mut results: HashMap<String, Vec<&CounterBucket>> = HashMap::new();
        for name in filtered {
            let result = metrics.query_counter(name, window.start..window.end)?;
            let key = if mutation_filter.is_some() {
                // Strip prefix → tablet_id
                name[prefix.len()..].to_string()
            } else {
                // Strip "subscription_invalidations:" → "{mutation}:{tablet_id}"
                name["subscription_invalidations:".len()..].to_string()
            };
            results.insert(key, result);
        }

        results
            .into_iter()
            .map(|(k, v)| {
                Ok((
                    k,
                    window.resample_counters(metrics, v, false /* is_rate */)?,
                ))
            })
            .collect()
    }

    fn get_udf_metric_counter(
        window: &MetricsWindow,
        metrics: &MetricStore,
        metric_name: &str,
    ) -> anyhow::Result<HashMap<String, Timeseries>> {
        let metric_names = metrics.metric_names_for_type(MetricType::Counter);

        let filtered_metric_names: Vec<_> = metric_names
            .iter()
            .filter(|name| name.starts_with("udf") && name.ends_with(metric_name))
            .collect();

        let mut results: HashMap<String, Vec<&CounterBucket>> = HashMap::new();
        for metric_name in filtered_metric_names {
            let result = metrics.query_counter(metric_name, window.start..window.end)?;
            let metric_name_parts: Vec<&str> = metric_name.split(':').collect();
            // UDF names can have colons in them, so we need to only exclude
            // the metric type and metric name
            let metric_name = metric_name_parts[1..metric_name_parts.len() - 1].join(":");
            results.insert(metric_name, result);
        }

        results
            .into_iter()
            .map(|(k, v)| {
                Ok((
                    k,
                    window.resample_counters(metrics, v, false /* is_rate */)?,
                ))
            })
            .collect()
    }

    fn top_k(map: &HashMap<&str, f64>, k: usize, ascending: bool) -> Vec<String> {
        let mut top_k: Vec<_> = map.iter().map(|(&key, &sum)| (key, sum)).collect();
        top_k.sort_unstable_by(|(name1, a), (name2, b)| {
            if ascending {
                a.total_cmp(b)
            } else {
                b.total_cmp(a)
            }
            .then(name1.cmp(name2)) // tiebreak by name
        });
        top_k.truncate(k);
        top_k.into_iter().map(|(name, _)| name.to_owned()).collect()
    }

    // Compute the top k rates for the given UDFs
    fn top_k_for_rate(
        window: &MetricsWindow,
        mut ts1: HashMap<String, Timeseries>,
        mut ts2: HashMap<String, Timeseries>,
        k: usize,
        merge: impl Fn(Option<f64>, Option<f64>) -> Option<f64> + Copy,
        ascending: bool,
    ) -> anyhow::Result<Vec<(String, Timeseries)>> {
        // First, calculate the overall rates summed over the entire time window
        let mut overall: HashMap<&str, f64> = HashMap::new();
        for (udf_id, ts1_series) in ts1.iter() {
            let ts1_sum = ts1_series.iter().filter_map(|&(_, value)| value).sum1();
            let ts2_sum = ts2
                .get(udf_id)
                .and_then(|ts2_series| ts2_series.iter().filter_map(|&(_, value)| value).sum1());
            if let Some(ratio) = merge(ts1_sum, ts2_sum) {
                overall.insert(udf_id, ratio);
            }
        }
        let top_k = Self::top_k(&overall, k, ascending);

        let mut ret = vec![];

        for udf_id in top_k {
            // Remove the top k from the hits and misses timeseries
            // so we can sum up everything that's left over.
            let ts1_series = ts1
                .remove(&udf_id)
                .expect("everything in topk came from ts1");
            let ts2_series = ts2
                .remove(&udf_id)
                .unwrap_or_else(|| ts1_series.iter().map(|&(ts, _)| (ts, None)).collect());
            let merged: Timeseries = merge_series(&ts1_series, &ts2_series, merge)?;
            ret.push((udf_id.to_string(), merged));
        }

        // Sum up the rest of the rates
        if !ts2.is_empty() || !ts1.is_empty() {
            let rest_ts1 = sum_timeseries(window, ts1.values())?;
            let rest_ts2 = sum_timeseries(window, ts2.values())?;
            let rest = merge_series(&rest_ts1, &rest_ts2, merge)?;
            ret.push(("_rest".to_string(), rest));
        }

        Ok(ret)
    }

    pub fn latency_percentiles(
        &self,
        identifier: UdfIdentifier,
        percentiles: Vec<Percentile>,
        window: MetricsWindow,
    ) -> anyhow::Result<BTreeMap<Percentile, Timeseries>> {
        let metrics = {
            let inner = self.inner.lock();
            inner.metrics.clone()
        };
        let buckets = metrics.query_histogram(
            &udf_execution_time_metric(&identifier),
            window.start..window.end,
        )?;
        window.resample_histograms(&metrics, buckets, &percentiles)
    }

    pub fn table_rate(
        &self,
        table_name: TableName,
        metric: TableRate,
        window: MetricsWindow,
    ) -> anyhow::Result<Timeseries> {
        let metrics = {
            let inner = self.inner.lock();
            inner.metrics.clone()
        };
        let name = match metric {
            TableRate::RowsRead => table_rows_read_metric(&table_name),
            TableRate::RowsWritten => table_rows_written_metric(&table_name),
        };
        let buckets = metrics.query_counter(&name, window.start..window.end)?;
        window.resample_counters(&metrics, buckets, true)
    }

    pub fn udf_summary(
        &self,
        cursor: Option<CursorMs>,
    ) -> (Option<UdfMetricSummary>, Option<CursorMs>) {
        let inner = self.inner.lock();
        let new_cursor = inner.log.back().map(|(ts, _)| *ts);

        let first_entry_ix = inner.log.partition_point(|(ts, _)| Some(*ts) <= cursor);
        if first_entry_ix >= inner.log.len() {
            return (None, new_cursor);
        }
        let mut summary = UdfMetricSummary::default();
        for i in first_entry_ix..inner.log.len() {
            let (_, entry) = &inner.log[i];
            let FunctionExecutionPart::Completion(entry) = entry else {
                continue;
            };
            let function_summary = summary
                .function_calls
                .entry(entry.caller.clone())
                .or_default()
                .entry(entry.udf_type)
                .or_default()
                .entry(entry.environment)
                .or_default();
            let error_count = if entry.params.is_err() { 1 } else { 0 };
            let entry_duration = Duration::from_secs_f64(entry.execution_time);

            function_summary.invocations += 1;
            function_summary.errors += error_count;
            function_summary.execution_time += entry_duration;
            function_summary.syscalls.merge(&entry.syscall_trace);

            summary.invocations += 1;
            summary.errors += error_count;
            summary.execution_time += entry_duration;
        }

        (Some(summary), new_cursor)
    }

    pub async fn stream(&self, cursor: CursorMs) -> (Vec<FunctionExecution>, CursorMs) {
        loop {
            let rx = {
                let mut inner = self.inner.lock();
                let first_entry_ix = inner.log.partition_point(|(ts, _)| *ts <= cursor);
                if first_entry_ix < inner.log.len() {
                    let entries = (first_entry_ix..inner.log.len())
                        .map(|i| &inner.log[i])
                        .filter_map(|(_, entry)| match entry {
                            FunctionExecutionPart::Completion(completion) => {
                                Some(completion.clone())
                            },
                            _ => None,
                        })
                        .collect();
                    let (new_cursor, _) = inner.log.back().unwrap();
                    return (entries, *new_cursor);
                }
                let (tx, rx) = oneshot::channel();
                inner.log_waiters.push(tx);
                rx
            };
            let _ = rx.await;
        }
    }

    pub async fn stream_parts(&self, cursor: CursorMs) -> (Vec<FunctionExecutionPart>, CursorMs) {
        loop {
            let rx = {
                let mut inner = self.inner.lock();
                let first_entry_ix = inner.log.partition_point(|(ts, _)| *ts <= cursor);
                if first_entry_ix < inner.log.len() {
                    let entries = (first_entry_ix..inner.log.len())
                        .map(|i| &inner.log[i])
                        .map(|(_, entry)| match entry {
                            FunctionExecutionPart::Completion(c) => {
                                let with_stripped_log_lines = match c.udf_type {
                                    UdfType::Query | UdfType::Mutation => c.clone(),
                                    UdfType::Action | UdfType::HttpAction => {
                                        let mut cloned = c.clone();
                                        cloned.log_lines = vec![].into();
                                        cloned
                                    },
                                };
                                FunctionExecutionPart::Completion(with_stripped_log_lines)
                            },
                            FunctionExecutionPart::Progress(c) => {
                                FunctionExecutionPart::Progress(c.clone())
                            },
                        })
                        .collect();
                    let (new_cursor, _) = inner.log.back().unwrap();
                    return (entries, *new_cursor);
                }
                let (tx, rx) = oneshot::channel();
                inner.log_waiters.push(tx);
                rx
            };
            let _ = rx.await;
        }
    }

    pub fn latest_cursor(&self) -> CursorMs {
        let inner = self.inner.lock();
        if let Some((new_cursor, _)) = inner.log.back() {
            *new_cursor
        } else {
            0.0
        }
    }

    /// Get the gauge value from the first complete bucket in the time range
    fn get_gauge_value(
        metrics: &MetricStore,
        env: &ModuleEnvironment,
        udf_type: &UdfType,
        state: &OutstandingFunctionState,
        start_time: SystemTime,
        now: SystemTime,
    ) -> u64 {
        let metric_name = outstanding_functions_metric(env, udf_type, state);
        metrics
            .query_gauge(&metric_name, start_time..now)
            .ok()
            .and_then(|buckets| buckets.first().map(|b| b.value.max(0.0) as u64))
            .unwrap_or(0)
    }

    /// Get both running and queued stats for a function type
    fn get_concurrency_stats(
        metrics: &MetricStore,
        env: ModuleEnvironment,
        udf_type: UdfType,
        start_time: SystemTime,
        now: SystemTime,
    ) -> FunctionConcurrencyStats {
        FunctionConcurrencyStats {
            num_running: Self::get_gauge_value(
                metrics,
                &env,
                &udf_type,
                &OutstandingFunctionState::Running,
                start_time,
                now,
            ),
            num_queued: Self::get_gauge_value(
                metrics,
                &env,
                &udf_type,
                &OutstandingFunctionState::Queued,
                start_time,
                now,
            ),
        }
    }

    /// Query concurrency stats for all function types within a time window.
    /// Returns a tuple of (query, mutation, action, node_action, http_action)
    /// stats.
    fn query_all_concurrency_stats(
        metrics: &MetricStore,
        start_time: SystemTime,
        end_time: SystemTime,
    ) -> (
        FunctionConcurrencyStats,
        FunctionConcurrencyStats,
        FunctionConcurrencyStats,
        FunctionConcurrencyStats,
        FunctionConcurrencyStats,
    ) {
        (
            Self::get_concurrency_stats(
                metrics,
                ModuleEnvironment::Isolate,
                UdfType::Query,
                start_time,
                end_time,
            ),
            Self::get_concurrency_stats(
                metrics,
                ModuleEnvironment::Isolate,
                UdfType::Mutation,
                start_time,
                end_time,
            ),
            Self::get_concurrency_stats(
                metrics,
                ModuleEnvironment::Isolate,
                UdfType::Action,
                start_time,
                end_time,
            ),
            Self::get_concurrency_stats(
                metrics,
                ModuleEnvironment::Node,
                UdfType::Action,
                start_time,
                end_time,
            ),
            Self::get_concurrency_stats(
                metrics,
                ModuleEnvironment::Isolate,
                UdfType::HttpAction,
                start_time,
                end_time,
            ),
        )
    }

    pub fn scheduled_job_lag(&self, window: MetricsWindow) -> anyhow::Result<Timeseries> {
        let metrics = {
            let inner = self.inner.lock();
            inner.metrics.clone()
        };
        let buckets = metrics
            .query_gauge(scheduled_job_next_ts_metric(), window.start..window.end)?
            .into_iter()
            .collect();
        let mut timeseries = window.resample_gauges(&metrics, buckets)?;
        // For buckets where no value is recorded, interpolate the previous known value
        // but increase it by the timestep (since lag naturally increases over
        // time)
        for i in 1..timeseries.len() {
            if timeseries[i].1.is_none()
                && let Some(prev) = timeseries[i - 1].1
            {
                let offset = timeseries[i].0.duration_since(timeseries[i - 1].0)?;
                timeseries[i].1 = Some(prev + offset.as_secs_f64());
            }
        }
        // Then clamp negative values to zero
        for (_, bucket) in &mut timeseries {
            if let Some(value) = bucket {
                *value = value.max(0.);
            }
        }
        Ok(timeseries)
    }

    /// Log the current number of outstanding functions as a gauge that tracks
    /// the maximum.
    pub fn log_outstanding_functions(
        &self,
        total: usize,
        env: ModuleEnvironment,
        udf_type: UdfType,
        state: OutstandingFunctionState,
    ) {
        let now = self.rt.system_time();
        let name = outstanding_functions_metric(&env, &udf_type, &state);
        // Use gauge with max operation to track maximum value within each bucket.
        let _ = self
            .inner
            .lock()
            .metrics
            .add_gauge_max(&name, now, total as f32);
    }

    pub fn function_concurrency(
        &self,
        window: MetricsWindow,
    ) -> anyhow::Result<BTreeMap<String, Timeseries>> {
        let metrics = {
            let inner = self.inner.lock();
            inner.metrics.clone()
        };

        let mut result = BTreeMap::new();

        // Query all outstanding_functions metrics
        let metric_names = metrics.metric_names_for_type(MetricType::Gauge);
        for metric_name in metric_names {
            if metric_name.starts_with("outstanding_functions:") {
                let buckets = metrics
                    .query_gauge(&metric_name, window.start..window.end)?
                    .into_iter()
                    .collect();
                let timeseries = window.resample_gauges(&metrics, buckets)?;
                result.insert(metric_name.to_string(), timeseries);
            }
        }

        Ok(result)
    }
}

fn sum_timeseries<'a>(
    window: &MetricsWindow,
    series: impl Iterator<Item = &'a Timeseries>,
) -> anyhow::Result<Timeseries> {
    let mut result = (0..window.num_buckets)
        .map(|i| Ok((window.bucket_start(i)?, None)))
        .collect::<anyhow::Result<Timeseries>>()?;
    for timeseries in series {
        anyhow::ensure!(timeseries.len() == result.len());
        for (i, &(ts, value)) in timeseries.iter().enumerate() {
            anyhow::ensure!(ts == result[i].0);
            if let Some(value) = value {
                *result[i].1.get_or_insert(0.) += value;
            }
        }
    }
    Ok(result)
}

fn merge_series(
    ts1: &Timeseries,
    ts2: &Timeseries,
    merge: impl Fn(Option<f64>, Option<f64>) -> Option<f64>,
) -> anyhow::Result<Timeseries> {
    anyhow::ensure!(ts2.len() == ts1.len());
    ts1.iter()
        .zip(ts2)
        .map(|(&(t1, t1_value), &(t2, t2_value))| {
            anyhow::ensure!(t1 == t2);
            Ok((t1, merge(t1_value, t2_value)))
        })
        .collect()
}

/// numerator / denominator * 100
fn percentage(numerator: Option<f64>, denominator: Option<f64>) -> Option<f64> {
    match (numerator, denominator) {
        (Some(numerator), Some(denominator)) => Some(numerator / denominator * 100.),
        (None, Some(_denominator)) => Some(0.),
        // This doesn't make sense, but could happen with mismatched timeseries or missing data
        (Some(_numerator), None) => Some(100.),
        (None, None) => None,
    }
}

/// hits / (hits + misses) * 100
fn cache_hit_percentage(hits: Option<f64>, misses: Option<f64>) -> Option<f64> {
    match (hits, misses) {
        (Some(hits), Some(misses)) => Some(hits / (hits + misses) * 100.),
        // There are hits but not misses, so the hit rate is 100.
        (Some(_hits), None) => Some(100.),
        // There are misses but not hits, so the hit rate is 0.
        (None, Some(_misses)) => Some(0.),
        (None, None) => None,
    }
}

struct Inner<RT: Runtime> {
    rt: RT,

    log: WithHeapSize<VecDeque<(CursorMs, FunctionExecutionPart)>>,
    num_execution_completions: usize,
    log_waiters: WithHeapSize<Vec<oneshot::Sender<()>>>,
    log_manager: Arc<dyn LogSender>,
    metrics: MetricStore,
}

impl<RT: Runtime> Inner<RT> {
    fn log_execution(
        &mut self,
        execution: FunctionExecution,
        send_console_events: bool,
        log_app_metrics: bool,
    ) -> anyhow::Result<()> {
        if log_app_metrics && let Err(e) = self.log_execution_app_metrics(&execution) {
            Self::log_metrics_error(e);
        }
        let next_time = self.next_time()?;

        // Gather log lines
        let mut log_events = if send_console_events {
            execution.console_log_events()
        } else {
            vec![]
        };
        // Gather UDF execution record
        match execution.udf_execution_record_log_events() {
            Ok(records) => log_events.extend(records),
            Err(mut e) => {
                // Don't let failing to construct the UDF execution record block sending
                // the other log events
                tracing::error!("failed to create UDF execution record: {}", e);
                report_error_sync(&mut e);
            },
        }

        self.log_manager.send_logs(log_events);

        self.log
            .push_back((next_time, FunctionExecutionPart::Completion(execution)));
        self.num_execution_completions += 1;
        while self.num_execution_completions > *knobs::MAX_UDF_EXECUTION {
            let front = self.log.pop_front();
            if let Some((_, FunctionExecutionPart::Completion(_))) = front {
                self.num_execution_completions -= 1;
            }
        }
        for waiter in self.log_waiters.drain(..) {
            let _ = waiter.send(());
        }
        Ok(())
    }

    fn log_metrics_error(error: UdfMetricsError) {
        // Only log an error to tracing and/or Sentry at most once every 10 seconds per
        // thread.
        thread_local! {
            static LAST_LOGGED_ERROR: Cell<Option<SystemTime>> = const { Cell::new(None) };
        }
        let now = SystemTime::now();
        let should_log = LAST_LOGGED_ERROR.get().is_none_or(|last_logged| {
            now.duration_since(last_logged).unwrap_or(Duration::ZERO) >= Duration::from_secs(10)
        });
        if !should_log {
            return;
        }
        LAST_LOGGED_ERROR.set(Some(now));
        tracing::error!("Failed to log execution metrics: {}", error);
        if let UdfMetricsError::InternalError(mut e) = error {
            report_error_sync(&mut e);
        }
    }

    fn log_execution_app_metrics(
        &mut self,
        execution: &FunctionExecution,
    ) -> Result<(), UdfMetricsError> {
        let ts = execution.unix_timestamp.as_system_time();

        let identifier = execution.identifier();

        let name = udf_invocations_metric(&identifier);
        self.metrics.add_counter(&name, ts, 1.0)?;

        let is_err = match &execution.params {
            UdfParams::Function { error, .. } => error.is_some(),
            UdfParams::Http { result, .. } => result.is_err(),
        };
        if is_err {
            let name = udf_errors_metric(&identifier);
            self.metrics.add_counter(&name, ts, 1.0)?;
        }
        if execution.udf_type == UdfType::Query {
            if execution.cached_result {
                let name = udf_cache_hits_metric(&identifier);
                self.metrics.add_counter(&name, ts, 1.0)?;
            } else {
                let name = udf_cache_misses_metric(&identifier);
                self.metrics.add_counter(&name, ts, 1.0)?;
            }
        }

        let name = udf_execution_time_metric(&identifier);
        self.metrics
            .add_histogram(&name, ts, Duration::from_secs_f64(execution.execution_time))?;

        for (table_name, table_stats) in &execution.tables_touched {
            let name = table_rows_read_metric(table_name);
            self.metrics
                .add_counter(&name, ts, table_stats.rows_read as f32)?;
            let name = table_rows_written_metric(table_name);
            self.metrics
                .add_counter(&name, ts, table_stats.rows_written as f32)?;
        }
        Ok(())
    }

    fn log_execution_progress(
        &mut self,
        log_lines: LogLines,
        event_source: FunctionEventSource,
        function_start_timestamp: UnixTimestamp,
    ) -> anyhow::Result<()> {
        let next_time = self.next_time()?;
        let progress = FunctionExecutionProgress {
            log_lines,
            event_source,
            function_start_timestamp,
        };

        let log_events = progress.console_log_events();
        self.log_manager.send_logs(log_events);
        self.log
            .push_back((next_time, FunctionExecutionPart::Progress(progress)));
        for waiter in self.log_waiters.drain(..) {
            let _ = waiter.send(());
        }
        Ok(())
    }

    fn log_scheduled_job_stats(
        &mut self,
        next_job_ts: Option<SystemTime>,
        now: SystemTime,
        num_running_jobs: u64,
    ) -> anyhow::Result<()> {
        let name = scheduled_job_next_ts_metric();
        // -Infinity means there is no scheduled job
        let value = next_job_ts.map_or(-f32::INFINITY, |ts| signed_duration_since(now, ts));
        if value > 0.0 {
            self.log_manager.send_logs(vec![LogEvent {
                timestamp: UnixTimestamp::from_system_time(now).context("now < UNIX_EPOCH?")?,
                event: StructuredLogEvent::ScheduledJobLag {
                    lag_seconds: Duration::from_secs_f32(value.max(0.0)),
                },
            }]);
        }
        if value > 0.0 || num_running_jobs > 0 {
            self.log_manager.send_logs(vec![LogEvent {
                timestamp: UnixTimestamp::from_system_time(now).context("now < UNIX_EPOCH?")?,
                event: StructuredLogEvent::SchedulerStats {
                    lag_seconds: Duration::from_secs_f32(value.max(0.0)),
                    num_running_jobs,
                },
            }]);
        }
        match self.metrics.add_gauge(name, now, value) {
            Ok(()) => (),
            Err(UdfMetricsError::SamplePrecedesCutoff { ts: _, cutoff }) => {
                // `now` was too old; automatically promote the sample to the cutoff time
                // instead
                let value =
                    next_job_ts.map_or(-f32::INFINITY, |ts| signed_duration_since(cutoff, ts));
                self.metrics.add_gauge(name, cutoff, value)?;
            },
            Err(err) => return Err(err.into()),
        }
        Ok(())
    }

    fn next_time(&self) -> anyhow::Result<CursorMs> {
        let since_epoch = self
            .rt
            .system_time()
            .duration_since(SystemTime::UNIX_EPOCH)?;
        let mut next_time =
            (since_epoch.as_secs() as f64 * 1e3) + (since_epoch.subsec_nanos() as f64 * 1e-6);
        if let Some((last_time, _)) = self.log.back() {
            let lower_bound = last_time.next_up();
            if lower_bound > next_time {
                next_time = lower_bound;
            }
        }
        Ok(next_time)
    }
}

/// `t1 - t2` in seconds, possibly negative
fn signed_duration_since(t1: SystemTime, t2: SystemTime) -> f32 {
    match t1.duration_since(t2) {
        Ok(d) => d.as_secs_f32(),
        Err(e) => -e.duration().as_secs_f32(),
    }
}

#[derive(Default)]
pub struct UdfMetricSummary {
    // Aggregated metrics for backwards compatibility.
    pub invocations: u32,
    pub errors: u32,
    pub execution_time: Duration,

    pub function_calls:
        BTreeMap<FunctionCaller, BTreeMap<UdfType, BTreeMap<ModuleEnvironment, FunctionSummary>>>,
}

impl From<UdfMetricSummary> for JsonValue {
    fn from(value: UdfMetricSummary) -> Self {
        json!({
            "invocations": value.invocations,
            "errors": value.errors,
            "executionTime": value.execution_time.as_secs_f64(),

            "functionCalls": value
                .function_calls
                .into_iter()
                .map(|(caller, v)| {
                    let map1 = v.into_iter()
                        .map(|(udf_type, v)| {
                            let map2 = v.into_iter()
                                .map(|(environment, summary)| {
                                    let key = environment.to_string();
                                    let value = JsonValue::from(summary);
                                    (key, value)
                                })
                                .collect::<serde_json::Map<_, _>>();
                            (format!("{udf_type}"), JsonValue::Object(map2))
                        })
                        .collect::<serde_json::Map<_, _>>();
                    (format!("{caller}"), JsonValue::Object(map1))
                })
                .collect::<serde_json::Map<_, _>>(),
        })
    }
}

#[derive(Default)]
pub struct FunctionSummary {
    pub invocations: u32,
    pub errors: u32,
    pub execution_time: Duration,
    pub syscalls: SyscallTrace,
}

impl From<FunctionSummary> for JsonValue {
    fn from(value: FunctionSummary) -> Self {
        json!({
            "invocations": value.invocations,
            "errors": value.errors,
            "executionTime": value.execution_time.as_secs_f64(),
            "syscalls": JsonValue::from(value.syscalls),
        })
    }
}

fn udf_invocations_metric(identifier: &UdfIdentifier) -> MetricName {
    format!("udf:{}:invocations", udf_metric_name(identifier))
}

fn udf_errors_metric(identifier: &UdfIdentifier) -> MetricName {
    format!("udf:{}:errors", udf_metric_name(identifier))
}

fn udf_cache_hits_metric(identifier: &UdfIdentifier) -> MetricName {
    format!("udf:{}:cache_hits", udf_metric_name(identifier))
}

fn udf_cache_misses_metric(identifier: &UdfIdentifier) -> MetricName {
    format!("udf:{}:cache_misses", udf_metric_name(identifier))
}

fn udf_execution_time_metric(identifier: &UdfIdentifier) -> MetricName {
    format!("udf:{}:execution_time", udf_metric_name(identifier))
}

// TODO: Thread component path through here.
fn table_rows_read_metric(table_name: &TableName) -> MetricName {
    format!("table:{table_name}:rows_read")
}

fn table_rows_written_metric(table_name: &TableName) -> MetricName {
    format!("table:{table_name}:rows_written")
}

fn scheduled_job_next_ts_metric() -> &'static str {
    "scheduled_jobs:next_ts"
}

fn outstanding_functions_metric(
    env: &ModuleEnvironment,
    udf_type: &UdfType,
    state: &OutstandingFunctionState,
) -> MetricName {
    let env_str = match env {
        ModuleEnvironment::Isolate => "isolate",
        ModuleEnvironment::Node => "node",
        ModuleEnvironment::Invalid => "invalid",
    };
    let state_str = match state {
        OutstandingFunctionState::Running => "running",
        OutstandingFunctionState::Queued => "queued",
    };
    format!("outstanding_functions:{env_str}:{udf_type}:{state_str}")
}

/// View over `FunctionExecutionLog` that only exposes metrics methods.
/// Obtained by checking `DeploymentOp::ViewMetrics`.
pub struct FunctionMetricsLog<'a, RT: Runtime> {
    log: &'a FunctionExecutionLog<RT>,
}

impl<'a, RT: Runtime> FunctionMetricsLog<'a, RT> {
    pub(crate) fn new(log: &'a FunctionExecutionLog<RT>) -> Self {
        Self { log }
    }

    pub fn udf_rate(
        &self,
        identifier: UdfIdentifier,
        metric: UdfRate,
        window: MetricsWindow,
    ) -> anyhow::Result<Timeseries> {
        self.log.udf_rate(identifier, metric, window)
    }

    pub fn cache_hit_percentage(
        &self,
        identifier: UdfIdentifier,
        window: MetricsWindow,
    ) -> anyhow::Result<Timeseries> {
        self.log.cache_hit_percentage(identifier, window)
    }

    pub fn failure_percentage_top_k(
        &self,
        window: MetricsWindow,
        k: usize,
    ) -> anyhow::Result<Vec<(String, Timeseries)>> {
        self.log.failure_percentage_top_k(window, k)
    }

    pub fn cache_hit_percentage_top_k(
        &self,
        window: MetricsWindow,
        k: usize,
    ) -> anyhow::Result<Vec<(String, Timeseries)>> {
        self.log.cache_hit_percentage_top_k(window, k)
    }

    pub fn get_all_function_calls(
        &self,
        window: &MetricsWindow,
    ) -> anyhow::Result<HashMap<String, Timeseries>> {
        self.log.get_all_function_calls(window)
    }

    pub fn function_call_count_top_k(
        &self,
        window: MetricsWindow,
        k: usize,
    ) -> anyhow::Result<Vec<(String, Timeseries)>> {
        self.log.function_call_count_top_k(window, k)
    }

    pub fn subscription_invalidations_top_k(
        &self,
        window: MetricsWindow,
        k: usize,
        identifier: Option<&UdfIdentifier>,
    ) -> anyhow::Result<Vec<(String, Timeseries)>> {
        self.log
            .subscription_invalidations_top_k(window, k, identifier)
    }

    pub fn latency_percentiles(
        &self,
        identifier: UdfIdentifier,
        percentiles: Vec<Percentile>,
        window: MetricsWindow,
    ) -> anyhow::Result<BTreeMap<Percentile, Timeseries>> {
        self.log
            .latency_percentiles(identifier, percentiles, window)
    }

    pub fn table_rate(
        &self,
        table_name: TableName,
        metric: TableRate,
        window: MetricsWindow,
    ) -> anyhow::Result<Timeseries> {
        self.log.table_rate(table_name, metric, window)
    }

    pub fn scheduled_job_lag(&self, window: MetricsWindow) -> anyhow::Result<Timeseries> {
        self.log.scheduled_job_lag(window)
    }

    pub fn function_concurrency(
        &self,
        window: MetricsWindow,
    ) -> anyhow::Result<BTreeMap<String, Timeseries>> {
        self.log.function_concurrency(window)
    }

    pub fn udf_summary(
        &self,
        cursor: Option<CursorMs>,
    ) -> (Option<UdfMetricSummary>, Option<CursorMs>) {
        self.log.udf_summary(cursor)
    }
}

/// View over `FunctionExecutionLog` that only exposes log entry streaming
/// methods. Obtained by checking `DeploymentOp::ViewLogs`.
pub struct FunctionEntriesLog<'a, RT: Runtime> {
    log: &'a FunctionExecutionLog<RT>,
}

impl<'a, RT: Runtime> FunctionEntriesLog<'a, RT> {
    pub(crate) fn new(log: &'a FunctionExecutionLog<RT>) -> Self {
        Self { log }
    }

    pub async fn stream(&self, cursor: CursorMs) -> (Vec<FunctionExecution>, CursorMs) {
        self.log.stream(cursor).await
    }

    pub async fn stream_parts(&self, cursor: CursorMs) -> (Vec<FunctionExecutionPart>, CursorMs) {
        self.log.stream_parts(cursor).await
    }

    pub fn latest_cursor(&self) -> CursorMs {
        self.log.latest_cursor()
    }
}

fn udf_metric_name(identifier: &UdfIdentifier) -> String {
    let (component, id) = identifier.clone().into_component_and_udf_path();
    match component {
        Some(component) => {
            format!("{component}/{id}")
        },
        None => id,
    }
}
