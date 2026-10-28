use common::{
    components::ComponentPath,
    types::{
        FullyQualifiedObjectKey,
        MemberId,
        ObjectKey,
        TableName,
    },
};
use serde::{
    Deserialize,
    Serialize,
};
use sync_types::Timestamp;
use value::{
    codegen_convex_serialization,
    TabletId,
};

#[derive(Clone, Debug, PartialEq)]
pub struct SnapshotImport {
    pub state: ImportState,
    pub format: ImportFormat,
    pub mode: ImportMode,
    pub component_path: ComponentPath,
    // TODO: this should always be FullyQualifiedObjectKey
    pub object_key: Result<FullyQualifiedObjectKey, ObjectKey>,
    pub member_id: Option<MemberId>,
    pub checkpoints: Option<Vec<ImportTableCheckpoint>>,
    pub requestor: ImportRequestor,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SerializedSnapshotImport {
    state: SerializedImportState,
    format: SerializedImportFormat,
    mode: String,
    component_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    object_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    fq_object_key: Option<String>,
    member_id: Option<i64>,
    checkpoints: Option<Vec<SerializedImportTableCheckpoint>>,
    requestor: SerializedImportRequestor,
}

impl From<SnapshotImport> for SerializedSnapshotImport {
    fn from(import: SnapshotImport) -> SerializedSnapshotImport {
        let (object_key, fq_object_key) = match import.object_key {
            Ok(key) => (None, Some(key.into())),
            Err(key) => (Some(key.into()), None),
        };
        SerializedSnapshotImport {
            state: import.state.into(),
            format: import.format.into(),
            mode: import.mode.to_string(),
            component_path: import.component_path.serialize(),
            object_key,
            fq_object_key,
            member_id: import.member_id.map(|member_id| member_id.0 as i64),
            checkpoints: import
                .checkpoints
                .map(|checkpoints| checkpoints.into_iter().map(Into::into).collect()),
            requestor: import.requestor.into(),
        }
    }
}

impl TryFrom<SerializedSnapshotImport> for SnapshotImport {
    type Error = anyhow::Error;

    fn try_from(import: SerializedSnapshotImport) -> anyhow::Result<SnapshotImport> {
        let object_key = match (import.object_key, import.fq_object_key) {
            (None, None) => anyhow::bail!("missing object key"),
            (None, Some(key)) => Ok(key.into()),
            (Some(key), None) => Err(key.try_into()?),
            (Some(_), Some(_)) => anyhow::bail!("can't have both unqualified and fq object key"),
        };
        Ok(SnapshotImport {
            state: import.state.try_into()?,
            format: import.format.try_into()?,
            mode: import.mode.parse()?,
            component_path: ComponentPath::deserialize(import.component_path.as_deref())?,
            object_key,
            member_id: import.member_id.map(|member_id| MemberId(member_id as u64)),
            checkpoints: import
                .checkpoints
                .map(|checkpoints| checkpoints.into_iter().map(TryInto::try_into).try_collect())
                .transpose()?,
            requestor: import.requestor.into(),
        })
    }
}

codegen_convex_serialization!(SnapshotImport, SerializedSnapshotImport);

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ImportFormat {
    Csv(TableName),
    JsonLines(TableName),
    JsonArray(TableName),
    Zip,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "format")]
pub enum SerializedImportFormat {
    #[serde(rename = "csv")]
    Csv { table: String },
    #[serde(rename = "jsonl")]
    JsonLines { table: String },
    #[serde(rename = "json_array")]
    JsonArray { table: String },
    #[serde(rename = "zip")]
    Zip,
}

impl From<ImportFormat> for SerializedImportFormat {
    fn from(format: ImportFormat) -> SerializedImportFormat {
        match format {
            ImportFormat::Csv(table) => SerializedImportFormat::Csv {
                table: table.to_string(),
            },
            ImportFormat::JsonLines(table) => SerializedImportFormat::JsonLines {
                table: table.to_string(),
            },
            ImportFormat::JsonArray(table) => SerializedImportFormat::JsonArray {
                table: table.to_string(),
            },
            ImportFormat::Zip => SerializedImportFormat::Zip,
        }
    }
}

impl TryFrom<SerializedImportFormat> for ImportFormat {
    type Error = anyhow::Error;

    fn try_from(format: SerializedImportFormat) -> anyhow::Result<ImportFormat> {
        match format {
            SerializedImportFormat::Csv { table } => Ok(ImportFormat::Csv(table.parse()?)),
            SerializedImportFormat::JsonLines { table } => {
                Ok(ImportFormat::JsonLines(table.parse()?))
            },
            SerializedImportFormat::JsonArray { table } => {
                Ok(ImportFormat::JsonArray(table.parse()?))
            },
            SerializedImportFormat::Zip => Ok(ImportFormat::Zip),
        }
    }
}

mod import_format_serde {
    use value::codegen_convex_serialization;

    use super::{
        ImportFormat,
        SerializedImportFormat,
    };

    codegen_convex_serialization!(ImportFormat, SerializedImportFormat);
}

/*
      │
      │
   CLI│uploads
┌─────▼─────┐
│ Uploaded  │
└─────┬─────┘
      │
Import│Worker parses
      ├─────────────────────┐
      │                     │
┌─────▼────────────────┐    │
│WaitingForConfirmation│    │
└─────┬────────────────┘    │
      │                     │
CLI requests confirmation   │
      │                     │
┌─────▼──────┐              │
│ InProgress │              │
└─────┬──────┘              │
      │                     │
Import│Worker imports       │
      ├─────────────────┐   │
      │                 │   │
┌─────▼──────┐      ┌───▼───▼─┐
│ Completed  │      │ Failed  │
└────────────┘      └─────────┘
 */
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ImportState {
    Uploaded,
    WaitingForConfirmation {
        info_message: String,
        require_manual_confirmation: bool,
    },
    InProgress {
        progress_message: String,
        checkpoint_messages: Vec<String>,
    },
    Completed {
        ts: Timestamp,
        num_rows_written: i64,
    },
    Failed(String),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum SerializedImportState {
    Uploaded,
    WaitingForConfirmation {
        message_to_confirm: Option<String>,
        require_manual_confirmation: Option<bool>,
    },
    InProgress {
        progress_message: Option<String>,
        checkpoint_messages: Vec<String>,
    },
    Completed {
        timestamp: i64,
        num_rows_written: i64,
    },
    Failed {
        error_message: String,
    },
}

impl From<ImportState> for SerializedImportState {
    fn from(state: ImportState) -> SerializedImportState {
        match state {
            ImportState::Uploaded => SerializedImportState::Uploaded,
            ImportState::WaitingForConfirmation {
                info_message,
                require_manual_confirmation,
            } => SerializedImportState::WaitingForConfirmation {
                message_to_confirm: Some(info_message),
                require_manual_confirmation: Some(require_manual_confirmation),
            },
            ImportState::InProgress {
                progress_message,
                checkpoint_messages,
            } => SerializedImportState::InProgress {
                progress_message: Some(progress_message),
                checkpoint_messages,
            },
            ImportState::Completed {
                ts,
                num_rows_written,
            } => SerializedImportState::Completed {
                timestamp: i64::from(ts),
                num_rows_written,
            },
            ImportState::Failed(message) => SerializedImportState::Failed {
                error_message: message,
            },
        }
    }
}

impl TryFrom<SerializedImportState> for ImportState {
    type Error = anyhow::Error;

    fn try_from(state: SerializedImportState) -> anyhow::Result<ImportState> {
        match state {
            SerializedImportState::Uploaded => Ok(ImportState::Uploaded),
            SerializedImportState::WaitingForConfirmation {
                message_to_confirm,
                require_manual_confirmation,
            } => Ok(ImportState::WaitingForConfirmation {
                info_message: message_to_confirm.unwrap_or_default(),
                require_manual_confirmation: require_manual_confirmation.unwrap_or(true),
            }),
            SerializedImportState::InProgress {
                progress_message,
                checkpoint_messages,
            } => Ok(ImportState::InProgress {
                progress_message: progress_message.unwrap_or_else(|| "Importing".to_string()),
                checkpoint_messages,
            }),
            SerializedImportState::Completed {
                timestamp,
                num_rows_written,
            } => Ok(ImportState::Completed {
                ts: timestamp.try_into()?,
                num_rows_written,
            }),
            SerializedImportState::Failed { error_message } => {
                Ok(ImportState::Failed(error_message))
            },
        }
    }
}

mod import_state_serde {
    use value::codegen_convex_serialization;

    use super::{
        ImportState,
        SerializedImportState,
    };

    codegen_convex_serialization!(ImportState, SerializedImportState);
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ImportTableCheckpoint {
    pub component_path: ComponentPath,
    pub display_table_name: TableName,
    pub tablet_id: Option<TabletId>,
    pub total_num_rows_to_write: i64,
    // For progress message, so we can say "wrote 40 of 100 documents"
    // Also for checkpointing, this is the number of rows we know we have written,
    // so we can skip trying to insert them.
    pub num_rows_written: i64,
    // For warning message, so we can say "this will delete 100 of 100 documents"
    // or "this will delete 0 of 100 documents"
    pub existing_rows_in_table: i64,
    pub existing_rows_to_delete: i64,

    // Whether some objects to be imported are missing "_id" fields.
    // This matters because it means we cannot tell if an object has already
    // been imported by a previous attempt, which means we have to start over
    // on any transient errors.
    pub is_missing_id_field: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SerializedImportTableCheckpoint {
    pub component_path: Option<String>,
    pub display_table_name: String,
    pub tablet_id: Option<String>,
    pub total_num_rows_to_write: i64,
    pub num_rows_written: i64,
    pub existing_rows_in_table: i64,
    pub existing_rows_to_delete: i64,
    pub is_missing_id_field: bool,
}

impl From<ImportTableCheckpoint> for SerializedImportTableCheckpoint {
    fn from(checkpoint: ImportTableCheckpoint) -> Self {
        SerializedImportTableCheckpoint {
            component_path: checkpoint.component_path.serialize(),
            display_table_name: checkpoint.display_table_name.to_string(),
            tablet_id: checkpoint.tablet_id.map(|table| table.to_string()),
            total_num_rows_to_write: checkpoint.total_num_rows_to_write,
            num_rows_written: checkpoint.num_rows_written,
            existing_rows_in_table: checkpoint.existing_rows_in_table,
            existing_rows_to_delete: checkpoint.existing_rows_to_delete,
            is_missing_id_field: checkpoint.is_missing_id_field,
        }
    }
}

impl TryFrom<SerializedImportTableCheckpoint> for ImportTableCheckpoint {
    type Error = anyhow::Error;

    fn try_from(checkpoint: SerializedImportTableCheckpoint) -> anyhow::Result<Self> {
        Ok(ImportTableCheckpoint {
            component_path: ComponentPath::deserialize(checkpoint.component_path.as_deref())?,
            display_table_name: checkpoint.display_table_name.parse()?,
            tablet_id: checkpoint
                .tablet_id
                .map(|tablet_id| tablet_id.parse())
                .transpose()?,
            total_num_rows_to_write: checkpoint.total_num_rows_to_write,
            num_rows_written: checkpoint.num_rows_written,
            existing_rows_in_table: checkpoint.existing_rows_in_table,
            existing_rows_to_delete: checkpoint.existing_rows_to_delete,
            is_missing_id_field: checkpoint.is_missing_id_field,
        })
    }
}

#[derive(
    Debug, Default, Deserialize, Clone, Copy, Eq, PartialEq, strum::EnumString, strum::Display,
)]
#[serde(rename_all = "camelCase")]
pub enum ImportMode {
    Append,
    Replace,
    ReplaceAll,
    #[default]
    RequireEmpty,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum ImportRequestor {
    SnapshotImport,
    CloudRestore { source_cloud_backup_id: u64 },
    StreamingImport,
}

impl ImportRequestor {
    pub fn usage_tag(&self) -> &'static str {
        match self {
            ImportRequestor::SnapshotImport => "snapshot_import",
            ImportRequestor::CloudRestore { .. } => "cloud_restore",
            ImportRequestor::StreamingImport => "streaming_import",
        }
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SerializedImportRequestor {
    #[serde(rename_all = "camelCase")]
    SnapshotImport,
    #[serde(rename_all = "camelCase")]
    CloudRestore { source_cloud_backup_id: i64 },
    #[serde(rename_all = "camelCase")]
    StreamingImport,
}

impl From<ImportRequestor> for SerializedImportRequestor {
    fn from(value: ImportRequestor) -> Self {
        match value {
            ImportRequestor::SnapshotImport => SerializedImportRequestor::SnapshotImport,
            ImportRequestor::CloudRestore {
                source_cloud_backup_id,
            } => SerializedImportRequestor::CloudRestore {
                source_cloud_backup_id: source_cloud_backup_id as i64,
            },
            ImportRequestor::StreamingImport => SerializedImportRequestor::StreamingImport,
        }
    }
}
impl From<SerializedImportRequestor> for ImportRequestor {
    fn from(value: SerializedImportRequestor) -> Self {
        match value {
            SerializedImportRequestor::SnapshotImport => ImportRequestor::SnapshotImport,
            SerializedImportRequestor::CloudRestore {
                source_cloud_backup_id,
            } => ImportRequestor::CloudRestore {
                source_cloud_backup_id: source_cloud_backup_id as u64,
            },
            SerializedImportRequestor::StreamingImport => ImportRequestor::StreamingImport,
        }
    }
}

codegen_convex_serialization!(ImportRequestor, SerializedImportRequestor);
