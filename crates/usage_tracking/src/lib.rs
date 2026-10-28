#![feature(iterator_try_collect)]

use std::{
    collections::BTreeMap,
    fmt::Debug,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use common::{
    components::ComponentPath,
    execution_context::ExecutionId,
    knobs::{
        FUNCTION_LIMIT_WARNING_RATIO,
        TRANSACTION_MAX_READ_SIZE_BYTES,
        TRANSACTION_MAX_READ_SIZE_ROWS,
    },
    types::{
        IndexName,
        ModuleEnvironment,
        UdfIdentifier,
    },
    RequestId,
};
use derive_more::{
    Add,
    AddAssign,
};
use events::usage::{
    FunctionCallUsageFields,
    InsightReadLimitCall,
    StorageCallSample,
    UsageEvent,
    UsageEventLogger,
};
use headers::ContentType;
use parking_lot::Mutex;
use pb::usage::{
    CounterWithComponent as CounterWithComponentProto,
    CounterWithTag as CounterWithTagProto,
    CounterWithUrl as CounterWithUrlProto,
    FunctionUsageStats as FunctionUsageStatsProto,
};
use value::{
    sha256::Sha256Digest,
    TableName,
};

mod metrics;

/// The core usage stats aggregator that is cheaply cloneable
#[derive(Clone, Debug)]
pub struct UsageCounter {
    usage_logger: Arc<dyn UsageEventLogger>,
}

pub use common::log_streaming::OccInfo;

pub enum CallType {
    Action {
        env: ModuleEnvironment,
        duration: Duration,
        user_execution_time: Option<Duration>,
        memory_in_mb: u64,
    },
    HttpAction {
        duration: Duration,
        user_execution_time: Option<Duration>,
        memory_in_mb: u64,

        /// Sha256 of the response body
        response_sha256: Sha256Digest,
    },
    Export,
    CachedQuery,
    UncachedQuery {
        duration: Duration,
        user_execution_time: Option<Duration>,
        memory_in_mb: u64,
    },
    Mutation {
        duration: Duration,
        user_execution_time: Option<Duration>,
        memory_in_mb: u64,
        occ_info: Option<OccInfo>,
    },
    Import,
    CloudBackup,
    CloudRestore,
    LogStreamPayload,
    IndexBackfill,
}

impl CallType {
    fn tag(&self) -> &'static str {
        match self {
            Self::Action { .. } => "action",
            Self::Export => "export",
            Self::CachedQuery => "cached_query",
            Self::UncachedQuery { .. } => "uncached_query",
            Self::Mutation { .. } => "mutation",
            Self::HttpAction { .. } => "http_action",
            Self::Import => "import",
            Self::CloudBackup => "cloud_backup",
            Self::CloudRestore => "cloud_restore",
            Self::LogStreamPayload => "log_stream_payload",
            Self::IndexBackfill => "index_backfill",
        }
    }

    fn is_occ(&self) -> bool {
        match self {
            Self::Mutation { occ_info, .. } => occ_info.is_some(),
            _ => false,
        }
    }

    fn occ_document_id(&self) -> Option<String> {
        match self {
            Self::Mutation { occ_info, .. } => {
                occ_info.as_ref().and_then(|info| info.document_id.clone())
            },
            _ => None,
        }
    }

    fn occ_table_name(&self) -> Option<String> {
        match self {
            Self::Mutation { occ_info, .. } => {
                occ_info.as_ref().and_then(|info| info.table_name.clone())
            },
            _ => None,
        }
    }

    fn occ_write_source(&self) -> Option<String> {
        match self {
            Self::Mutation { occ_info, .. } => {
                occ_info.as_ref().and_then(|info| info.write_source.clone())
            },
            _ => None,
        }
    }

    fn occ_retry_count(&self) -> Option<u64> {
        match self {
            Self::Mutation { occ_info, .. } => occ_info.as_ref().and_then(|info| info.retry_count),
            _ => None,
        }
    }

    fn memory_megabytes(&self) -> u64 {
        match self {
            CallType::UncachedQuery { memory_in_mb, .. }
            | CallType::Mutation { memory_in_mb, .. }
            | CallType::Action { memory_in_mb, .. }
            | CallType::HttpAction { memory_in_mb, .. } => *memory_in_mb,
            _ => 0,
        }
    }

    /// Total wall-clock time from the start of executing a request to its
    /// completion.
    fn duration_millis(&self) -> u64 {
        match self {
            CallType::UncachedQuery { duration, .. }
            | CallType::Mutation { duration, .. }
            | CallType::Action { duration, .. }
            | CallType::HttpAction { duration, .. } => u64::try_from(duration.as_millis())
                .expect("Function was running for over 584 billion years??"),
            _ => 0,
        }
    }

    /// Time spent executing user code in the isolate. Does not include
    /// syscalls, waiting for fetch, etc.
    fn user_execution_millis(&self) -> Option<u64> {
        match self {
            CallType::UncachedQuery {
                user_execution_time,
                ..
            }
            | CallType::Mutation {
                user_execution_time,
                ..
            }
            | CallType::Action {
                user_execution_time,
                ..
            }
            | CallType::HttpAction {
                user_execution_time,
                ..
            } => user_execution_time.map(|t| t.as_millis() as u64),
            CallType::CachedQuery
            | CallType::CloudBackup
            | CallType::CloudRestore
            | CallType::Export
            | CallType::Import
            | CallType::LogStreamPayload
            | CallType::IndexBackfill => None,
        }
    }

    fn environment(&self) -> String {
        match self {
            CallType::Action { env, .. } => env,
            // All other UDF types, including HTTP actions run on the isolate
            // only.
            _ => &ModuleEnvironment::Isolate,
        }
        .to_string()
    }

    fn response_sha256(&self) -> Option<String> {
        match self {
            CallType::HttpAction {
                response_sha256, ..
            } => Some(response_sha256.as_hex()),
            _ => None,
        }
    }
}

impl UsageCounter {
    pub fn new(usage_logger: Arc<dyn UsageEventLogger>) -> Self {
        Self { usage_logger }
    }

    pub async fn track_call(
        &self,
        udf_path: UdfIdentifier,
        execution_id: ExecutionId,
        request_id: RequestId,
        call_type: CallType,
        success: bool,
        stats: FunctionUsageStats,
    ) {
        let mut usage_metrics = Vec::new();

        // Because system udfs might cause usage before any data is added by the user,
        // we do not count their calls. We do count their bandwidth.
        let (should_track_calls, udf_id_type) = match &udf_path {
            UdfIdentifier::Function(path) => (!path.udf_path.is_system(), "function"),
            UdfIdentifier::Http(_) => (true, "http"),
            UdfIdentifier::SystemJob(_) => (false, "_system_job"),
        };

        let (component_path, udf_id) = udf_path.clone().into_component_and_udf_path();
        usage_metrics.push(UsageEvent::FunctionCall {
            fields: FunctionCallUsageFields {
                id: execution_id.to_string(),
                request_id: request_id.to_string(),
                status: if success { "success" } else { "failure" }.to_string(),
                component_path,
                udf_id,
                udf_id_type: udf_id_type.to_string(),
                tag: call_type.tag().to_string(),
                memory_megabytes: call_type.memory_megabytes(),
                duration_millis: call_type.duration_millis(),
                user_execution_millis: call_type.user_execution_millis(),
                environment: call_type.environment(),
                is_tracked: should_track_calls,
                response_sha256: call_type.response_sha256(),
                is_occ: call_type.is_occ(),
                occ_table_name: call_type.occ_table_name(),
                occ_document_id: call_type.occ_document_id(),
                occ_write_source: call_type.occ_write_source(),
                occ_retry_count: call_type.occ_retry_count(),
            },
        });

        // We always track bandwidth, even for system udfs.
        self._track_function_usage(
            udf_path,
            stats,
            execution_id,
            request_id,
            success,
            &mut usage_metrics,
        );
        self.usage_logger.record_async(usage_metrics).await;
    }

    // TODO: The existence of this function is a hack due to shortcuts we have
    // done in Node.js usage tracking. It should only be used by Node.js action
    // callbacks. We should only be using track_call() and never calling this
    // this directly. Otherwise, we will have the usage reflected in the usage
    // stats for billing but not in the UDF execution log counters.
    pub async fn track_function_usage(
        &self,
        udf_path: UdfIdentifier,
        execution_id: ExecutionId,
        request_id: RequestId,
        stats: FunctionUsageStats,
    ) {
        let mut usage_metrics = Vec::new();
        self._track_function_usage(
            udf_path,
            stats,
            execution_id,
            request_id,
            true,
            &mut usage_metrics,
        );
        self.usage_logger.record_async(usage_metrics).await;
    }

    pub fn _track_function_usage(
        &self,
        udf_path: UdfIdentifier,
        stats: FunctionUsageStats,
        execution_id: ExecutionId,
        request_id: RequestId,
        success: bool,
        usage_metrics: &mut Vec<UsageEvent>,
    ) {
        // Merge the storage stats.
        let (global_component_path, udf_id) = udf_path.into_component_and_udf_path();
        for ((component_path, storage_api), function_count) in stats.storage_calls {
            usage_metrics.push(UsageEvent::FunctionStorageCalls {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                call: storage_api,
                count: function_count,
            });
        }

        for (component_path, ingress) in stats.storage_ingress {
            usage_metrics.push(UsageEvent::FunctionStorageBandwidth {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                ingress,
                egress: 0,
            });
        }
        for (component_path, egress) in stats.storage_egress {
            usage_metrics.push(UsageEvent::FunctionStorageBandwidth {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                ingress: 0,
                egress,
            });
        }
        // Merge the "by table" bandwidth stats. They're tracked in separate
        // maps but describe the same access.
        #[derive(Default)]
        struct DatabaseBandwidthCounters {
            ingress_v2: u64,
            egress: u64,
            egress_rows: u64,
            egress_v2: u64,
            virtual_table_ingress: u64,
            virtual_table_egress: u64,
        }
        let mut database_bandwidth: BTreeMap<
            (ComponentPath, TableNameString),
            DatabaseBandwidthCounters,
        > = BTreeMap::new();
        for (key, ingress) in stats.database_ingress_v2 {
            database_bandwidth.entry(key).or_default().ingress_v2 += ingress;
        }
        for (key, egress) in stats.database_egress {
            database_bandwidth.entry(key).or_default().egress += egress;
        }
        for (key, egress) in stats.database_egress_v2 {
            database_bandwidth.entry(key).or_default().egress_v2 += egress;
        }
        for (key, egress_rows) in stats.database_egress_rows {
            database_bandwidth.entry(key).or_default().egress_rows += egress_rows;
        }
        for (key, ingress) in stats.virtual_table_ingress {
            database_bandwidth
                .entry(key)
                .or_default()
                .virtual_table_ingress += ingress;
        }
        for (key, egress) in stats.virtual_table_egress {
            database_bandwidth
                .entry(key)
                .or_default()
                .virtual_table_egress += egress;
        }
        let mut bandwidth_by_component: BTreeMap<ComponentPath, DatabaseBandwidthCounters> =
            BTreeMap::new();
        for ((component_path, _), bandwidth) in &database_bandwidth {
            let component_bandwidth = bandwidth_by_component
                .entry(component_path.clone())
                .or_default();
            component_bandwidth.ingress_v2 += bandwidth.ingress_v2;
            component_bandwidth.egress += bandwidth.egress;
            component_bandwidth.egress_rows += bandwidth.egress_rows;
            component_bandwidth.egress_v2 += bandwidth.egress_v2;
            component_bandwidth.virtual_table_ingress += bandwidth.virtual_table_ingress;
            component_bandwidth.virtual_table_egress += bandwidth.virtual_table_egress;
        }
        for (component_path, bandwidth) in bandwidth_by_component {
            usage_metrics.push(UsageEvent::DatabaseBandwidth {
                id: execution_id.to_string(),
                request_id: Some(request_id.to_string()),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                table_name: (),
                ingress_v2: bandwidth.ingress_v2,
                egress_rows: bandwidth.egress_rows,
                egress_v2: bandwidth.egress_v2,
                virtual_table_ingress: bandwidth.virtual_table_ingress,
                virtual_table_egress: bandwidth.virtual_table_egress,
            });
        }

        // Check read limits and add InsightReadLimit event if thresholds are exceeded
        let total_rows: u64 = database_bandwidth.values().map(|b| b.egress_rows).sum();
        let total_bytes: u64 = database_bandwidth.values().map(|b| b.egress).sum();

        let row_threshold =
            (*TRANSACTION_MAX_READ_SIZE_ROWS as f64 * *FUNCTION_LIMIT_WARNING_RATIO) as u64;
        let byte_threshold =
            (*TRANSACTION_MAX_READ_SIZE_BYTES as f64 * *FUNCTION_LIMIT_WARNING_RATIO) as u64;

        let did_exceed_document_threshold = total_rows >= row_threshold;
        let did_exceed_byte_threshold = total_bytes >= byte_threshold;

        if did_exceed_document_threshold || did_exceed_byte_threshold {
            let mut calls = Vec::new();
            let component_path = database_bandwidth
                .iter()
                .find(|(_, bandwidth)| bandwidth.egress_rows > 0)
                .map(|((component_path, _), _)| component_path.clone());
            if component_path.is_none() {
                tracing::error!("Failed to find component path despite thresholds being exceeded");
            }

            if let Some(component_path) = component_path {
                for ((_, table_name), bandwidth) in &database_bandwidth {
                    if bandwidth.egress_rows == 0 {
                        continue;
                    }
                    calls.push(InsightReadLimitCall {
                        table_name: table_name.clone(),
                        bytes_read: bandwidth.egress,
                        documents_read: bandwidth.egress_rows,
                    });
                }

                usage_metrics.push(UsageEvent::InsightReadLimit {
                    id: execution_id.to_string(),
                    request_id: request_id.to_string(),
                    udf_id: udf_id.clone(),
                    component_path: component_path.serialize(),
                    calls,
                    success,
                });
            }
        }

        for ((component_path, table_name), ingress) in stats.vector_ingress {
            usage_metrics.push(UsageEvent::VectorBandwidth {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                table_name,
                ingress,
                egress: 0,
                ingress_v2: 0,
            });
        }
        for ((component_path, table_name), egress) in stats.vector_egress {
            usage_metrics.push(UsageEvent::VectorBandwidth {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                table_name,
                ingress: 0,
                egress,
                ingress_v2: 0,
            });
        }
        for ((component_path, table_name), ingress) in stats.vector_ingress_v2 {
            usage_metrics.push(UsageEvent::VectorBandwidth {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                table_name,
                ingress: 0,
                egress: 0,
                ingress_v2: ingress,
            });
        }
        for ((component_path, table_name), ingress) in stats.text_ingress {
            usage_metrics.push(UsageEvent::TextWrites {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                table_name,
                size: ingress,
            });
        }
        for (
            (component_path, index_name),
            TextIndexQueryUsage {
                num_searches,
                bytes_searched,
                filtered_bytes_searched,
            },
        ) in stats.text_query_usage
        {
            usage_metrics.push(UsageEvent::TextQuery {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                table_name: index_name.table().to_string(),
                index_name: index_name.to_string(),
                num_searches,
                bytes_searched,
                filtered_bytes_searched,
            })
        }
        for (
            (component_path, index_name),
            VectorIndexQueryUsage {
                num_searches,
                bytes_searched,
            },
        ) in stats.vector_query_usage
        {
            usage_metrics.push(UsageEvent::VectorQuery {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                udf_id: udf_id.clone(),
                table_name: index_name.table().to_string(),
                index_name: index_name.to_string(),
                num_searches,
                bytes_searched,
            })
        }

        for (url, egress) in stats.fetch_egress {
            usage_metrics.push(UsageEvent::NetworkBandwidth {
                id: execution_id.to_string(),
                request_id: request_id.to_string(),
                component_path: global_component_path.clone(),
                udf_id: udf_id.clone(),
                url,
                egress,
            });
        }

        if stats.audit_log_egress > 0 {
            usage_metrics.push(UsageEvent::AuditLogBandwidth {
                id: execution_id.to_string(),
                component_path: global_component_path.clone(),
                udf_id: udf_id.clone(),
                egress: stats.audit_log_egress,
            });
        }
    }
}

// We can track storage attributed by UDF or not. This is why unlike database
// and vector search egress/ingress those methods are both on
// FunctionUsageTracker and UsageCounters directly.
#[async_trait]
pub trait StorageUsageTracker: Send + Sync {
    async fn track_storage_call(
        &self,
        component_path: ComponentPath,
        storage_api: &'static str,
        content_type: Option<ContentType>,
    ) -> Box<dyn StorageCallTracker>;
}

#[async_trait]
pub trait StorageCallTracker: Send + Sync {
    async fn track_storage_ingress(&self, component_path: ComponentPath, tag: String, ingress: u64);
    async fn track_storage_egress(&self, component_path: ComponentPath, tag: String, egress: u64);
}

struct IndependentStorageCallTracker {
    execution_id: ExecutionId,
    usage_logger: Arc<dyn UsageEventLogger>,
}

impl IndependentStorageCallTracker {
    fn new(execution_id: ExecutionId, usage_logger: Arc<dyn UsageEventLogger>) -> Self {
        Self {
            execution_id,
            usage_logger,
        }
    }
}

#[async_trait]
impl StorageCallTracker for IndependentStorageCallTracker {
    async fn track_storage_ingress(
        &self,
        component_path: ComponentPath,
        tag: String,
        ingress: u64,
    ) {
        metrics::storage::log_storage_ingress(ingress);
        self.usage_logger
            .record_async(vec![UsageEvent::StorageBandwidth {
                id: self.execution_id.to_string(),
                component_path: component_path.serialize(),
                tag,
                ingress,
                egress: 0,
            }])
            .await;
    }

    async fn track_storage_egress(&self, component_path: ComponentPath, tag: String, egress: u64) {
        metrics::storage::log_storage_egress(egress);
        self.usage_logger
            .record_async(vec![UsageEvent::StorageBandwidth {
                id: self.execution_id.to_string(),
                component_path: component_path.serialize(),
                tag,
                ingress: 0,
                egress,
            }])
            .await;
    }
}

#[async_trait]
impl StorageUsageTracker for UsageCounter {
    async fn track_storage_call(
        &self,
        component_path: ComponentPath,
        storage_api: &'static str,
        content_type: Option<ContentType>,
    ) -> Box<dyn StorageCallTracker> {
        let execution_id = ExecutionId::new();
        metrics::storage::log_storage_call();
        self.usage_logger
            .record_async(vec![UsageEvent::StorageCall {
                id: execution_id.to_string(),
                component_path: component_path.serialize(),
                call: storage_api.to_string(),
                sample: StorageCallSample {
                    content_type: content_type.map(|c| c.to_string()),
                },
                count: 1,
            }])
            .await;

        Box::new(IndependentStorageCallTracker::new(
            execution_id,
            self.usage_logger.clone(),
        ))
    }
}

/// Usage tracker used within a Transaction. Note that this structure does not
/// directly report to the backend global counters and instead only buffers the
/// counters locally. The counters get rolled into the global ones via
/// UsageCounters::track_call() at the end of each UDF. This provides a
/// consistent way to account for usage, where we only bill people for usage
/// that makes it to the UdfExecution log.
#[derive(Debug, Clone)]
pub struct FunctionUsageTracker {
    // TODO: We should ideally not use an Arc<Mutex> here. The best way to achieve
    // this is to move the logic for accounting ingress out of the Committer into
    // the Transaction. Then Transaction can solely own the counters and we can
    // remove clone(). The alternative is for the Committer to take ownership of
    // the usage tracker and then return it, but this will make it complicated if
    // we later decide to charge people for OCC bandwidth.
    state: Arc<Mutex<FunctionUsageStats>>,
}

impl FunctionUsageTracker {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(FunctionUsageStats::default())),
        }
    }

    /// Calculate FunctionUsageStats here
    pub fn gather_user_stats(self) -> FunctionUsageStats {
        self.state.lock().clone()
    }

    /// Adds the given usage stats to the current tracker.
    pub fn add(&self, stats: FunctionUsageStats) {
        self.state.lock().merge(stats);
    }

    // Tracks database usage from write operations (insert/update/delete) for
    // documents that are not in vector indexes. If the document has one or more
    // vectors in a vector index, call `track_vector_ingress` instead of
    // this method.
    //
    // You must always check to see if a document is a vector index before
    // calling this method.
    pub fn track_database_ingress(
        &self,
        component_path: ComponentPath,
        table_name: &TableName,
        ingress: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        let mut state = self.state.lock();
        // Skip v1 ingress tracking if configured (e.g., for streaming imports)
        if state.skip_v1_database_ingress {
            return;
        }
        *state
            .database_ingress
            .entry((component_path, table_name.to_string()))
            .or_default() += ingress;
    }

    pub fn track_database_ingress_v2(
        &self,
        component_path: ComponentPath,
        table_name: &TableName,
        ingress: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        let mut state = self.state.lock();
        *state
            .database_ingress_v2
            .entry((component_path, table_name.to_string()))
            .or_default() += ingress;
    }

    pub fn track_database_egress(
        &self,
        component_path: ComponentPath,
        table_name: &TableName,
        egress: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        let mut state = self.state.lock();
        *state
            .database_egress
            .entry((component_path, table_name.to_string()))
            .or_default() += egress;
    }

    pub fn track_database_egress_v2(
        &self,
        component_path: ComponentPath,
        table_name: &TableName,
        egress: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        let mut state = self.state.lock();
        *state
            .database_egress_v2
            .entry((component_path, table_name.to_string()))
            .or_default() += egress;
    }

    pub fn track_virtual_table_ingress(
        &self,
        component_path: ComponentPath,
        table_name: &TableName,
        ingress: u64,
    ) {
        let mut state = self.state.lock();
        *state
            .virtual_table_ingress
            .entry((component_path, table_name.to_string()))
            .or_default() += ingress;
    }

    pub fn track_virtual_table_egress(
        &self,
        component_path: ComponentPath,
        table_name: &TableName,
        egress: u64,
    ) {
        let mut state = self.state.lock();
        *state
            .virtual_table_egress
            .entry((component_path, table_name.to_string()))
            .or_default() += egress;
    }

    pub fn track_database_egress_rows(
        &self,
        component_path: ComponentPath,
        table_name: &TableName,
        egress_rows: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        let mut state = self.state.lock();
        *state
            .database_egress_rows
            .entry((component_path, table_name.to_string()))
            .or_default() += egress_rows;
    }

    pub fn track_database_ingress_rows(
        &self,
        component_path: ComponentPath,
        table_name: &TableName,
        ingress_rows: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        let mut state = self.state.lock();
        *state
            .database_ingress_rows
            .entry((component_path, table_name.to_string()))
            .or_default() += ingress_rows;
    }

    pub fn track_database_ingress_index_rows(&self, ingress_index_rows: u64, skip_logging: bool) {
        if skip_logging {
            return;
        }

        self.state.lock().database_ingress_index_rows += ingress_index_rows;
    }

    // Tracks the vector ingress surcharge for documents
    // that have one or more vectors in a vector index.
    //
    // Vector bandwidth is a surcharge on vector related bandwidth usage.
    // Ingress is a bit trickier than egress because vector ingress needs to be
    // updated whenever the mutated document is in a vector index. To be in a
    // vector index the document must both be in a table with a vector index and
    // have at least one vector that's actually used in the index.
    pub fn track_vector_ingress(
        &self,
        component_path: ComponentPath,
        table_name: &TableName,
        ingress: u64,
        ingress_v2: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        let mut state = self.state.lock();
        let key = (component_path, table_name.to_string());
        *state.vector_ingress.entry(key.clone()).or_default() += ingress;
        *state.vector_ingress_v2.entry(key).or_default() += ingress_v2;
    }

    // Tracks bandwidth usage from vector searches
    //
    // Vector bandwidth is a surcharge on vector related bandwidth usage. As a
    // result it counts against both bandwidth egress and vector egress. It's an
    // error to increment vector egress without also incrementing database
    // egress. The reverse is not true however, it's totally fine to increment
    // general database egress without incrementing vector egress if the operation
    // is not a vector search.
    //
    // Unlike track_database_ingress, this method is explicitly vector related
    // because we should always know that the relevant operation is a vector
    // search. In contrast, for ingress any insert/update/delete could happen to
    // impact a vector index.
    // TODO: This will be deprecated after the business plan change, but
    // we still need to track database egress.
    pub fn track_vector_egress(
        &self,
        component_path: ComponentPath,
        table_name: &TableName,
        egress: u64,
    ) {
        // Note that vector search counts as both database and vector bandwidth
        // per the comment above.
        let mut state = self.state.lock();
        let key = (component_path, table_name.to_string());
        *state.database_egress.entry(key.clone()).or_default() += egress;
        *state.vector_egress.entry(key).or_default() += egress;
    }

    pub fn track_vector_query(
        &self,
        component_path: ComponentPath,
        index_name: IndexName,
        index_size: u64,
    ) {
        let mut state = self.state.lock();
        let key = (component_path, index_name);
        *state.vector_query_usage.entry(key).or_default() += VectorIndexQueryUsage {
            num_searches: 1,
            bytes_searched: index_size,
        };
    }

    pub fn track_text_ingress(
        &self,
        component_path: ComponentPath,
        table_name: &TableName,
        ingress: u64,
        skip_logging: bool,
    ) {
        if skip_logging {
            return;
        }

        let mut state = self.state.lock();
        *state
            .text_ingress
            .entry((component_path, table_name.to_string()))
            .or_default() += ingress;
    }

    pub fn track_text_query(
        &self,
        component_path: ComponentPath,
        index_name: IndexName,
        index_size: u64,
        filtered_bytes_searched: u64,
    ) {
        let mut state = self.state.lock();
        let key = (component_path, index_name);
        *state.text_query_usage.entry(key).or_default() += TextIndexQueryUsage {
            num_searches: 1,
            bytes_searched: index_size,
            filtered_bytes_searched,
        };
    }

    /// Only track egress - because AWS only charges egress
    pub fn track_fetch_egress(&self, url: String, egress: u64) {
        let mut state = self.state.lock();
        *state.fetch_egress.entry(url).or_default() += egress;
    }

    pub fn track_audit_log_egress(&self, egress: u64) {
        let mut state = self.state.lock();
        state.audit_log_egress += egress;
    }

    /// Configure this tracker to skip v1 database ingress tracking.
    /// Used for streaming imports which should only track v2 ingress.
    pub fn without_v1_database_ingress(self) -> Self {
        {
            let mut state = self.state.lock();
            state.skip_v1_database_ingress = true;
        }
        self
    }
}

// For UDFs, we track storage at the per UDF level, no finer. So we can just
// aggregate over the entire UDF and not worry about sending usage events or
// creating unique execution ids.
// Note: If we want finer-grained breakdown of file bandwidth, we can thread the
// tag through FunctionUsageStats. For now we're just interested in the
// breakdown of file bandwidth from functions vs external sources like snapshot
// export/cloud backups.
#[async_trait]
impl StorageCallTracker for FunctionUsageTracker {
    async fn track_storage_ingress(
        &self,
        component_path: ComponentPath,
        _tag: String,
        ingress: u64,
    ) {
        let mut state = self.state.lock();
        metrics::storage::log_storage_ingress(ingress);
        *state.storage_ingress.entry(component_path).or_default() += ingress;
    }

    async fn track_storage_egress(&self, component_path: ComponentPath, _tag: String, egress: u64) {
        let mut state = self.state.lock();
        metrics::storage::log_storage_egress(egress);
        *state.storage_egress.entry(component_path).or_default() += egress;
    }
}

#[async_trait]
impl StorageUsageTracker for FunctionUsageTracker {
    async fn track_storage_call(
        &self,
        component_path: ComponentPath,
        storage_api: &'static str,
        _content_type: Option<ContentType>,
    ) -> Box<dyn StorageCallTracker> {
        let mut state = self.state.lock();
        metrics::storage::log_storage_call();
        *state
            .storage_calls
            .entry((component_path, storage_api.to_string()))
            .or_default() += 1;
        Box::new(self.clone())
    }
}

type TableNameString = String;
type StorageAPI = String;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Add, Default, AddAssign)]
pub struct TextIndexQueryUsage {
    pub num_searches: u64,
    /// Total size of the index's disk segments, once per search.
    pub bytes_searched: u64,
    /// Segment bytes scaled by the share of documents matching each search's
    /// filter conditions. Reported alongside `bytes_searched` so the two can
    /// be compared; billing reads `bytes_searched`.
    pub filtered_bytes_searched: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Add, Default, AddAssign)]
pub struct VectorIndexQueryUsage {
    pub num_searches: u64,
    pub bytes_searched: u64,
}

/// User-facing UDF stats, built
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FunctionUsageStats {
    pub storage_calls: BTreeMap<(ComponentPath, StorageAPI), u64>,
    pub storage_ingress: BTreeMap<ComponentPath, u64>,
    pub storage_egress: BTreeMap<ComponentPath, u64>,
    pub database_ingress: BTreeMap<(ComponentPath, TableNameString), u64>,
    /// Includes ingress for tables that have virtual tables
    pub database_ingress_v2: BTreeMap<(ComponentPath, TableNameString), u64>,
    pub database_egress: BTreeMap<(ComponentPath, TableNameString), u64>,
    /// Includes egress for tables that have virtual tables
    pub database_egress_v2: BTreeMap<(ComponentPath, TableNameString), u64>,
    /// Ingress for virtual tables, keyed by virtual table name
    pub virtual_table_ingress: BTreeMap<(ComponentPath, TableNameString), u64>,
    /// Egress for virtual tables, keyed by virtual table name
    pub virtual_table_egress: BTreeMap<(ComponentPath, TableNameString), u64>,
    pub database_egress_rows: BTreeMap<(ComponentPath, TableNameString), u64>,
    pub database_ingress_rows: BTreeMap<(ComponentPath, TableNameString), u64>,
    /// Entries written to user tables' database indexes. Counted per document
    /// write, so it covers the same writes as `database_ingress_rows`, but not
    /// broken down per table: nothing reads that breakdown for either counter.
    pub database_ingress_index_rows: u64,
    pub vector_ingress: BTreeMap<(ComponentPath, TableNameString), u64>,
    pub vector_ingress_v2: BTreeMap<(ComponentPath, TableNameString), u64>,
    pub vector_egress: BTreeMap<(ComponentPath, TableNameString), u64>,
    pub text_ingress: BTreeMap<(ComponentPath, TableNameString), u64>,

    pub text_query_usage: BTreeMap<(ComponentPath, IndexName), TextIndexQueryUsage>,
    pub vector_query_usage: BTreeMap<(ComponentPath, IndexName), VectorIndexQueryUsage>,
    pub fetch_egress: BTreeMap<String, u64>,
    pub audit_log_egress: u64,

    /// If true, skip tracking v1 database ingress.
    /// Used for streaming imports which should only track v2 ingress.
    pub skip_v1_database_ingress: bool,
}

impl FunctionUsageStats {
    pub fn aggregate(&self) -> AggregatedFunctionUsageStats {
        AggregatedFunctionUsageStats {
            database_read_bytes: self.database_egress.values().sum(),
            database_write_bytes: self.database_ingress.values().sum(),
            database_io_read_bytes: self.database_egress_v2.values().sum(),
            database_io_write_bytes: self.database_ingress_v2.values().sum(),
            database_read_documents: self.database_egress_rows.values().sum(),
            database_write_documents: self.database_ingress_rows.values().sum(),
            database_write_index_rows: self.database_ingress_index_rows,
            storage_read_bytes: self.storage_egress.values().sum(),
            storage_write_bytes: self.storage_ingress.values().sum(),
            vector_index_read_bytes: self.vector_egress.values().sum(),
            vector_index_write_bytes: self.vector_ingress.values().sum(),
            text_index_write_query_bytes: self.text_ingress.values().sum(),
            text_index_query_bytes: self
                .text_query_usage
                .values()
                .map(|u| u.bytes_searched)
                .sum(),
            vector_index_read_query_bytes: self
                .vector_query_usage
                .values()
                .map(|u| u.bytes_searched)
                .sum(),
            network_egress_bytes: self.fetch_egress.values().sum(),
            vector_index_write_query_bytes: self.vector_ingress_v2.values().sum(),
            audit_log_egress_bytes: self.audit_log_egress,
        }
    }

    fn merge(
        &mut self,
        Self {
            storage_calls,
            storage_ingress,
            storage_egress,
            database_ingress,
            database_ingress_v2,
            database_egress,
            database_egress_v2,
            virtual_table_ingress,
            virtual_table_egress,
            database_egress_rows,
            database_ingress_rows,
            database_ingress_index_rows,
            vector_ingress,
            vector_ingress_v2,
            vector_egress,
            text_ingress,
            text_query_usage,
            vector_query_usage,
            fetch_egress,
            skip_v1_database_ingress: _,
            audit_log_egress,
        }: Self,
    ) {
        for (key, function_count) in storage_calls {
            *self.storage_calls.entry(key).or_default() += function_count;
        }
        for (key, ingress) in storage_ingress {
            *self.storage_ingress.entry(key).or_default() += ingress;
        }
        for (key, egress) in storage_egress {
            *self.storage_egress.entry(key).or_default() += egress;
        }
        for (key, ingress) in database_ingress {
            *self.database_ingress.entry(key).or_default() += ingress;
        }
        for (key, ingress) in database_ingress_v2 {
            *self.database_ingress_v2.entry(key).or_default() += ingress;
        }
        for (key, egress) in database_egress {
            *self.database_egress.entry(key).or_default() += egress;
        }
        for (key, egress) in database_egress_v2 {
            *self.database_egress_v2.entry(key).or_default() += egress;
        }
        for (key, ingress) in virtual_table_ingress {
            *self.virtual_table_ingress.entry(key).or_default() += ingress;
        }
        for (key, egress) in virtual_table_egress {
            *self.virtual_table_egress.entry(key).or_default() += egress;
        }
        for (key, egress_rows) in database_egress_rows {
            *self.database_egress_rows.entry(key.clone()).or_default() += egress_rows;
        }
        for (key, ingress_rows) in database_ingress_rows {
            *self.database_ingress_rows.entry(key.clone()).or_default() += ingress_rows;
        }
        for (key, ingress) in vector_ingress {
            *self.vector_ingress.entry(key.clone()).or_default() += ingress;
        }
        for (key, egress) in vector_egress {
            *self.vector_egress.entry(key.clone()).or_default() += egress;
        }
        for (key, ingress) in vector_ingress_v2 {
            *self.vector_ingress_v2.entry(key.clone()).or_default() += ingress;
        }
        for (key, ingress) in text_ingress {
            *self.text_ingress.entry(key.clone()).or_default() += ingress;
        }
        for (key, text_index_usage) in text_query_usage {
            *self.text_query_usage.entry(key).or_default() += text_index_usage;
        }
        for (key, vector_index_usage) in vector_query_usage {
            *self.vector_query_usage.entry(key).or_default() += vector_index_usage;
        }
        for (key, egress) in fetch_egress {
            *self.fetch_egress.entry(key.clone()).or_default() += egress;
        }
        self.audit_log_egress += audit_log_egress;
        self.database_ingress_index_rows += database_ingress_index_rows;
    }
}

fn to_by_tag_count(
    counts: impl Iterator<Item = ((ComponentPath, String), u64)>,
) -> Vec<CounterWithTagProto> {
    counts
        .map(
            |((component_path, table_name), count)| CounterWithTagProto {
                component_path: component_path.serialize(),
                table_name: Some(table_name),
                count: Some(count),
            },
        )
        .collect()
}

fn to_by_component_count(
    counts: impl Iterator<Item = (ComponentPath, u64)>,
) -> Vec<CounterWithComponentProto> {
    counts
        .map(|(component_path, count)| CounterWithComponentProto {
            component_path: component_path.serialize(),
            count: Some(count),
        })
        .collect()
}

fn to_by_url_count(counts: impl Iterator<Item = (String, u64)>) -> Vec<CounterWithUrlProto> {
    counts
        .map(|(url, count)| CounterWithUrlProto {
            url: Some(url),
            count: Some(count),
        })
        .collect()
}

fn from_by_tag_count(
    counts: Vec<CounterWithTagProto>,
) -> anyhow::Result<impl Iterator<Item = ((ComponentPath, String), u64)>> {
    let counts: Vec<_> = counts
        .into_iter()
        .map(|c| -> anyhow::Result<_> {
            let component_path = ComponentPath::deserialize(c.component_path.as_deref())?;
            let name = c.table_name.context("Missing `table_name` field")?;
            let count = c.count.context("Missing `count` field")?;
            Ok(((component_path, name), count))
        })
        .try_collect()?;
    Ok(counts.into_iter())
}

fn from_by_url_count(
    counts: Vec<CounterWithUrlProto>,
) -> anyhow::Result<impl Iterator<Item = (String, u64)>> {
    let counts: Vec<_> = counts
        .into_iter()
        .map(|c| -> anyhow::Result<_> {
            let url = c.url.context("Missing `url` field")?;
            let count = c.count.context("Missing `count` field")?;
            Ok((url, count))
        })
        .try_collect()?;
    Ok(counts.into_iter())
}

fn from_by_component_tag_count(
    counts: Vec<CounterWithComponentProto>,
) -> anyhow::Result<impl Iterator<Item = (ComponentPath, u64)>> {
    let counts: Vec<_> = counts
        .into_iter()
        .map(|c| -> anyhow::Result<_> {
            let component_path = ComponentPath::deserialize(c.component_path.as_deref())?;
            let count = c.count.context("Missing `count` field")?;
            Ok((component_path, count))
        })
        .try_collect()?;
    Ok(counts.into_iter())
}

fn to_text_query_usage(
    usage: impl Iterator<Item = ((ComponentPath, IndexName), TextIndexQueryUsage)>,
) -> Vec<pb::usage::TextQueryUsage> {
    usage
        .map(
            |((component_path, index_name), usage)| pb::usage::TextQueryUsage {
                component_path: component_path.serialize(),
                table_name: Some(index_name.table().to_string()),
                index_name: Some(index_name.to_string()),
                num_searches: Some(usage.num_searches),
                bytes_searched: Some(usage.bytes_searched),
                filtered_bytes_searched: Some(usage.filtered_bytes_searched),
            },
        )
        .collect()
}

fn to_vector_query_usage(
    usage: impl Iterator<Item = ((ComponentPath, IndexName), VectorIndexQueryUsage)>,
) -> Vec<pb::usage::VectorQueryUsage> {
    usage
        .map(
            |((component_path, index_name), usage)| pb::usage::VectorQueryUsage {
                component_path: component_path.serialize(),
                table_name: Some(index_name.table().to_string()),
                index_name: Some(index_name.to_string()),
                num_searches: Some(usage.num_searches),
                bytes_searched: Some(usage.bytes_searched),
                // Older readers require field 6 to be present during rolling upgrades.
                dimensions: Some(0),
            },
        )
        .collect()
}

fn from_text_query_usage(
    usage: Vec<pb::usage::TextQueryUsage>,
) -> anyhow::Result<impl Iterator<Item = ((ComponentPath, IndexName), TextIndexQueryUsage)>> {
    let usage: Vec<_> = usage
        .into_iter()
        .map(|u| -> anyhow::Result<_> {
            let component_path = ComponentPath::deserialize(u.component_path.as_deref())?;
            let index_name: IndexName = u
                .index_name
                .context("Missing `index_name` field")?
                .parse()?;
            let num_searches = u.num_searches.context("Missing `num_searches` field")?;
            let bytes_searched = u
                .bytes_searched
                .context("Missing `num_segment_searches` field")?;
            // Absent from records written before the field existed.
            let filtered_bytes_searched = u.filtered_bytes_searched.unwrap_or(0);
            Ok((
                (component_path, index_name),
                TextIndexQueryUsage {
                    num_searches,
                    bytes_searched,
                    filtered_bytes_searched,
                },
            ))
        })
        .try_collect()?;
    Ok(usage.into_iter())
}

fn from_vector_query_usage(
    usage: Vec<pb::usage::VectorQueryUsage>,
) -> anyhow::Result<impl Iterator<Item = ((ComponentPath, IndexName), VectorIndexQueryUsage)>> {
    let usage: Vec<_> = usage
        .into_iter()
        .map(|u| -> anyhow::Result<_> {
            let component_path = ComponentPath::deserialize(u.component_path.as_deref())?;
            let index_name: IndexName = u
                .index_name
                .context("Missing `index_name` field")?
                .parse()?;
            let num_searches = u.num_searches.context("Missing `num_searches` field")?;
            let bytes_searched = u
                .bytes_searched
                .context("Missing `num_segment_searches` field")?;
            Ok((
                (component_path, index_name),
                VectorIndexQueryUsage {
                    num_searches,
                    bytes_searched,
                },
            ))
        })
        .try_collect()?;
    Ok(usage.into_iter())
}

impl From<FunctionUsageStats> for FunctionUsageStatsProto {
    fn from(stats: FunctionUsageStats) -> Self {
        FunctionUsageStatsProto {
            storage_calls: to_by_tag_count(stats.storage_calls.into_iter()),
            storage_ingress_by_component: to_by_component_count(stats.storage_ingress.into_iter()),
            storage_egress_by_component: to_by_component_count(stats.storage_egress.into_iter()),
            database_ingress: to_by_tag_count(stats.database_ingress.into_iter()),
            database_ingress_v2: to_by_tag_count(stats.database_ingress_v2.into_iter()),
            database_egress: to_by_tag_count(stats.database_egress.into_iter()),
            database_egress_v2: to_by_tag_count(stats.database_egress_v2.into_iter()),
            database_egress_rows: to_by_tag_count(stats.database_egress_rows.into_iter()),
            database_ingress_rows: to_by_tag_count(stats.database_ingress_rows.into_iter()),
            database_ingress_index_rows: stats.database_ingress_index_rows,
            vector_ingress: to_by_tag_count(stats.vector_ingress.into_iter()),
            vector_egress: to_by_tag_count(stats.vector_egress.into_iter()),
            text_ingress: to_by_tag_count(stats.text_ingress.into_iter()),
            text_query_usage: to_text_query_usage(stats.text_query_usage.into_iter()),
            vector_query_usage: to_vector_query_usage(stats.vector_query_usage.into_iter()),
            vector_ingress_v2: to_by_tag_count(stats.vector_ingress_v2.into_iter()),
            fetch_egress: to_by_url_count(stats.fetch_egress.into_iter()),
            virtual_table_ingress: to_by_tag_count(stats.virtual_table_ingress.into_iter()),
            virtual_table_egress: to_by_tag_count(stats.virtual_table_egress.into_iter()),
            audit_log_egress: stats.audit_log_egress,
        }
    }
}

impl TryFrom<FunctionUsageStatsProto> for FunctionUsageStats {
    type Error = anyhow::Error;

    fn try_from(stats: FunctionUsageStatsProto) -> anyhow::Result<Self> {
        let storage_calls = from_by_tag_count(stats.storage_calls)?.collect();
        let storage_ingress =
            from_by_component_tag_count(stats.storage_ingress_by_component)?.collect();
        let storage_egress =
            from_by_component_tag_count(stats.storage_egress_by_component)?.collect();
        let database_ingress = from_by_tag_count(stats.database_ingress)?.collect();
        let database_ingress_v2 = from_by_tag_count(stats.database_ingress_v2)?.collect();
        let database_egress = from_by_tag_count(stats.database_egress)?.collect();
        let database_egress_v2 = from_by_tag_count(stats.database_egress_v2)?.collect();
        let database_egress_rows = from_by_tag_count(stats.database_egress_rows)?.collect();
        let database_ingress_rows = from_by_tag_count(stats.database_ingress_rows)?.collect();
        let database_ingress_index_rows = stats.database_ingress_index_rows;
        let vector_ingress = from_by_tag_count(stats.vector_ingress)?.collect();
        let vector_egress = from_by_tag_count(stats.vector_egress)?.collect();
        let text_ingress = from_by_tag_count(stats.text_ingress)?.collect();
        let text_query_usage = from_text_query_usage(stats.text_query_usage)?.collect();
        let vector_query_usage = from_vector_query_usage(stats.vector_query_usage)?.collect();
        let vector_ingress_v2 = from_by_tag_count(stats.vector_ingress_v2)?.collect();
        let fetch_egress = from_by_url_count(stats.fetch_egress)?.collect();
        let virtual_table_ingress = from_by_tag_count(stats.virtual_table_ingress)?.collect();
        let virtual_table_egress = from_by_tag_count(stats.virtual_table_egress)?.collect();
        let audit_log_egress = stats.audit_log_egress;

        Ok(FunctionUsageStats {
            storage_calls,
            storage_ingress,
            storage_egress,
            database_ingress,
            database_ingress_v2,
            database_egress_rows,
            database_ingress_rows,
            database_ingress_index_rows,
            database_egress,
            database_egress_v2,
            virtual_table_ingress,
            virtual_table_egress,
            vector_ingress,
            vector_egress,
            text_ingress,
            text_query_usage,
            vector_query_usage,
            vector_ingress_v2,
            fetch_egress,
            skip_v1_database_ingress: false,
            audit_log_egress,
        })
    }
}

/// User-facing UDF stats, that is logged in the UDF execution log
/// and might be used for debugging purposes.
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
    pub network_egress_bytes: u64,
    pub vector_index_write_query_bytes: u64,
    pub audit_log_egress_bytes: u64,
}
