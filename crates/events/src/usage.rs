use async_trait::async_trait;
use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UdfType {
    Action,
    Query,
    Mutation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionCallUsageFields {
    /// The ExecutionId of a particular UDF
    pub id: String,
    /// The RequestId of a particular UDF
    pub request_id: String,
    /// Whether the request succeeded or failed
    pub status: String,
    /// The path of a component. Uniquely identifies a component in a
    /// project.
    pub component_path: Option<String>,
    /// The path / name of the UDF
    pub udf_id: String,
    /// The type of the udf identifier (http, function, cli)
    pub udf_id_type: String,
    /// "storage", "mutation", "cached_query" etc.
    pub tag: String,
    /// The memory used in megabytes by the UDF, or 0 if we don't track
    /// memory for this tag type.
    pub memory_megabytes: u64,
    /// The duration in milliseconds of the UDF, or 0 if we don't track
    /// execution time for this tag type.
    pub duration_millis: u64,
    /// The duration in milliseconds of user execution time in the isolate.
    /// Excludes syscalls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_execution_millis: Option<u64>,
    /// Whether this was run in V8 or Node, or "unknown".
    pub environment: String,
    /// True if we think it's a call we should track in usage. Right now
    /// this is basically any UDF that's neither system nor
    /// triggered by the CLI.
    /// This could be derived from path and
    /// udf type, but it seems better to be explicit)
    pub is_tracked: bool,
    /// The sha256 of the response body. Only set for HTTP actions.
    pub response_sha256: Option<String>,
    /// Whether this function call resulted in an OCC.
    pub is_occ: bool,
    /// The name of the table that the OCC occurred on. Only set if is_occ is
    /// true.
    pub occ_table_name: Option<String>,
    /// The document ID of the document that the OCC occurred on. Only set if
    /// is_occ is true.
    pub occ_document_id: Option<String>,
    // The source of the OCC. Only set if is_occ is true.
    pub occ_write_source: Option<String>,
    /// The retry number of the OCC. Only set if is_occ is true.
    pub occ_retry_count: Option<u64>,
}

/// One storage call covered by a [`UsageEvent::StorageCall`]. A summary's
/// sample is an arbitrary one of its calls, since content type is not part of
/// the summary's grouping key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageCallSample {
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsightReadLimitCall {
    pub table_name: String,
    pub bytes_read: u64,
    pub documents_read: u64,
}

/// Reported in place of a table name for usage that isn't attributed to a
/// single table.
pub const UNKNOWN_TABLE_NAME: &str = "-unknown-";

fn serialize_unknown_table_name<S: serde::Serializer>(
    _: &(),
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(UNKNOWN_TABLE_NAME)
}

// TODO(CX-5845): Use proper serializable types for constants rather than
// Strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageEvent {
    FunctionCall {
        #[serde(flatten)]
        fields: FunctionCallUsageFields,
    },
    /// A set of storage calls originating from a single user function
    /// invocation.
    FunctionStorageCalls {
        id: String,
        component_path: Option<String>,
        udf_id: String,
        call: String,
        count: u64,
    },
    /// Bandwidth from one or more storage calls originating from a single user
    /// function invocation.
    FunctionStorageBandwidth {
        id: String,
        component_path: Option<String>,
        udf_id: String,
        ingress: u64,
        egress: u64,
    },
    /// A single storage call originating outside of a user function (e.g.
    /// snapshot import/export)
    StorageCall {
        id: String,
        component_path: Option<String>,
        call: String,
        /// Flattened so the payload keeps one flat schema for raw events and
        /// summaries.
        #[serde(flatten)]
        sample: StorageCallSample,
        /// Calls this event covers: one for a raw event, the minute's total for
        /// a summary.
        count: u64,
    },
    /// Bandwidth from a storage call outside of a user function (e.g. snapshot
    /// import/export).
    StorageBandwidth {
        id: String,
        component_path: Option<String>,
        tag: String,
        ingress: u64,
        egress: u64,
    },
    /// Database bandwidth for a function execution or an aggregated interval.
    DatabaseBandwidth {
        id: String,
        /// Correlates an individual execution's bandwidth with its request.
        /// Minute summaries span many requests and omit it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        component_path: Option<String>,
        udf_id: String,
        /// Bandwidth is aggregated across tables, but the downstream pipeline
        /// still expects a `table_name` column, so it's always
        /// [`UNKNOWN_TABLE_NAME`].
        #[serde(serialize_with = "serialize_unknown_table_name", skip_deserializing)]
        table_name: (),
        // Includes ingress for tables that have virtual tables
        ingress_v2: u64,
        egress_rows: u64,
        // Includes egress for tables that have virtual tables
        egress_v2: u64,
        #[serde(default)]
        virtual_table_ingress: u64,
        #[serde(default)]
        virtual_table_egress: u64,
    },
    NetworkBandwidth {
        id: String,
        request_id: String,
        component_path: Option<String>,
        udf_id: String,
        url: String,
        egress: u64,
    },
    InsightReadLimit {
        id: String,
        request_id: String,
        udf_id: String,
        component_path: Option<String>,
        calls: Vec<InsightReadLimitCall>,
        success: bool,
    },
    VectorBandwidth {
        id: String,
        component_path: Option<String>,
        udf_id: String,
        table_name: String,
        ingress: u64,
        egress: u64,
        ingress_v2: u64,
    },
    TextWrites {
        id: String,
        component_path: Option<String>,
        udf_id: String,
        table_name: String,
        size: u64,
    },
    TextQuery {
        id: String,
        component_path: Option<String>,
        udf_id: String,
        table_name: String,
        index_name: String,
        num_searches: u64,
        /// Total index size per search
        bytes_searched: u64,
        /// Index size scaled by the share of documents matching each search's
        /// filter conditions.
        filtered_bytes_searched: u64,
    },
    VectorQuery {
        id: String,
        component_path: Option<String>,
        udf_id: String,
        table_name: String,
        index_name: String,
        num_searches: u64,
        bytes_searched: u64,
    },
    AuditLogBandwidth {
        id: String,
        component_path: Option<String>,
        udf_id: String,
        egress: u64,
    },

    // Current* events record the current storage state as of a time, they're not incremental
    // deltas. So a new Current* value should replace the previous value. If a tables Vec is
    // empty, that means no tables have any usage of the type in question.
    CurrentVectorStorage {
        tables: Vec<TableVectorStorage>,
    },
    CurrentTextStorage {
        tables: Vec<TableTextStorage>,
    },
    CurrentDatabaseStorage {
        #[serde(rename = "tables")]
        user_tables: Vec<TableDatabaseStorage>,
        system_tables: Vec<TableDatabaseStorage>,
        virtual_tables: Vec<TableDatabaseStorage>,
    },
    CurrentFileStorage {
        // TODO(Rebecca): tag and total_size can be cleaned up after we start using the other
        // fields
        tag: String,
        total_size: u64,
        total_user_file_size: u64,
        total_cloud_backup_size: u64,
        total_snapshot_export_size: u64,
    },
    CurrentDocumentCounts {
        tables: Vec<TableDocumentCount>,
        system_tables: Vec<TableDocumentCount>,
    },
    /// The current backend state, broken down into its orthogonal stop states.
    /// Each field is the serialized state value (e.g. "none", "disabled").
    CurrentBackendState {
        system: String,
        usage_limit: String,
        user: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableDocumentCount {
    pub component_path: Option<String>,
    pub table_name: String,
    pub num_documents: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableDatabaseStorage {
    pub component_path: Option<String>,
    pub table_name: String,
    pub total_document_size: u64,
    pub total_index_size: u64,
    pub total_system_index_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableVectorStorage {
    pub component_path: Option<String>,
    pub table_name: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableTextStorage {
    pub component_path: Option<String>,
    pub table_name: String,
    pub size: u64,
}

/// Fire off usage events into the ether.
#[async_trait]
pub trait UsageEventLogger: Send + Sync + std::fmt::Debug {
    /// Dump events into a buffer, waiting for the buffer to empty if it's full.
    async fn record_async(&self, events: Vec<UsageEvent>);

    /// Cleanly shutdown, flushing events
    async fn shutdown(&self) -> anyhow::Result<()>;
}

#[derive(Debug, Clone)]
pub struct NoOpUsageEventLogger;

#[async_trait]
impl UsageEventLogger for NoOpUsageEventLogger {
    async fn record_async(&self, _events: Vec<UsageEvent>) {}

    async fn shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }
}