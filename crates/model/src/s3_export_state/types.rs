use serde::{
    Deserialize,
    Serialize,
};
use value::{
    codegen_convex_serialization,
    DeveloperDocumentId,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum S3ExportProgress {
    #[serde(rename_all = "camelCase")]
    Snapshotting {
        num_tables_synced: i64,
        total_tables: i64,
        current_component: String,
        current_table: String,
        num_documents_in_current_table: i64,
        total_documents_in_current_table: Option<i64>,
        num_documents_synced: i64,
        total_documents: Option<i64>,
    },
    Stale {
        ts: i64,
    },
    UpToDate {
        ts: i64,
    },
}

codegen_convex_serialization!(S3ExportProgress, S3ExportProgress);

#[derive(Debug, Clone, PartialEq)]
pub struct S3ExportState {
    pub config_id: DeveloperDocumentId,
    /// Progress of the last page the destination committed.
    pub checkpoint: S3ExportCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3ExportCheckpoint {
    pub progress: S3ExportProgress,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedS3ExportState {
    pub config_id: String,
    pub checkpoint: S3ExportCheckpoint,
}

impl From<S3ExportState> for SerializedS3ExportState {
    fn from(value: S3ExportState) -> Self {
        Self {
            config_id: value.config_id.to_string(),
            checkpoint: value.checkpoint,
        }
    }
}

impl TryFrom<SerializedS3ExportState> for S3ExportState {
    type Error = anyhow::Error;

    fn try_from(value: SerializedS3ExportState) -> anyhow::Result<Self> {
        Ok(Self {
            config_id: DeveloperDocumentId::decode(&value.config_id)?,
            checkpoint: value.checkpoint,
        })
    }
}

codegen_convex_serialization!(S3ExportState, SerializedS3ExportState);
