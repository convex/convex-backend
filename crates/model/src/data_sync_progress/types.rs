use common::components::ComponentPath;
use serde::{
    Deserialize,
    Serialize,
};
use value::{
    codegen_convex_serialization,
    TableName,
};

/// Progress of one data sync (streaming export) session, updated after each
/// page served by `/api/v1/data/sync`. One row per sync, keyed by `sync_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSyncProgressMetadata {
    /// Unique id of the sync, generated when the sync starts and carried in
    /// its cursor.
    pub sync_id: String,
    /// Unix ms when the sync last completed a page. Drives the
    /// "active within the last 3 days" filter and garbage collection.
    pub last_updated_ms: u64,
    pub state: DataSyncState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataSyncState {
    /// The sync is still traversing its target tables' ID space; the data
    /// synced so far is not yet a consistent snapshot.
    Snapshotting {
        num_tables_synced: u64,
        total_tables: u64,
        current_component: ComponentPath,
        current_table: TableName,
        num_documents_synced_in_current_table: u64,
        total_documents_in_current_table: u64,
        /// Documents (including tombstones and re-emitted revisions) emitted
        /// over the sync's lifetime, so this can slightly exceed
        /// `total_documents`.
        num_documents_synced: u64,
        /// Documents across all target tables.
        total_documents: u64,
    },
    /// The sync reached a consistent snapshot as of `synced_ts`, but newer data
    /// is already available; it is streaming later changes (CDC).
    Stale {
        total_tables: u64,
        /// Documents (including tombstones and re-emitted revisions) emitted
        /// over the sync's lifetime.
        num_documents_synced: u64,
        /// The commit timestamp the synced data is consistent as of.
        synced_ts: i64,
    },
    /// The sync reached a consistent snapshot as of `synced_ts` and has caught
    /// up to the latest data.
    UpToDate {
        total_tables: u64,
        /// Documents (including tombstones and re-emitted revisions) emitted
        /// over the sync's lifetime.
        num_documents_synced: u64,
        /// The commit timestamp the synced data is consistent as of.
        synced_ts: i64,
    },
}

impl DataSyncState {
    /// Documents (including tombstones and re-emitted revisions) emitted over
    /// the sync's lifetime, regardless of which phase the sync is in.
    pub fn num_documents_synced(&self) -> u64 {
        match self {
            Self::Snapshotting {
                num_documents_synced,
                ..
            }
            | Self::Stale {
                num_documents_synced,
                ..
            }
            | Self::UpToDate {
                num_documents_synced,
                ..
            } => *num_documents_synced,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedDataSyncProgressMetadata {
    pub sync_id: String,
    pub last_updated_ms: u64,
    pub state: SerializedDataSyncState,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SerializedDataSyncState {
    #[serde(rename_all = "camelCase")]
    Snapshotting {
        num_tables_synced: u64,
        total_tables: u64,
        current_component: String,
        current_table: String,
        num_documents_synced_in_current_table: u64,
        total_documents_in_current_table: u64,
        num_documents_synced: u64,
        total_documents: u64,
    },
    #[serde(rename_all = "camelCase")]
    Stale {
        total_tables: u64,
        num_documents_synced: u64,
        synced_ts: i64,
    },
    #[serde(rename_all = "camelCase")]
    UpToDate {
        total_tables: u64,
        num_documents_synced: u64,
        synced_ts: i64,
    },
}

impl TryFrom<DataSyncProgressMetadata> for SerializedDataSyncProgressMetadata {
    type Error = anyhow::Error;

    fn try_from(value: DataSyncProgressMetadata) -> Result<Self, Self::Error> {
        Ok(Self {
            sync_id: value.sync_id,
            last_updated_ms: value.last_updated_ms,
            state: match value.state {
                DataSyncState::Snapshotting {
                    num_tables_synced,
                    total_tables,
                    current_component,
                    current_table,
                    num_documents_synced_in_current_table,
                    total_documents_in_current_table,
                    num_documents_synced,
                    total_documents,
                } => SerializedDataSyncState::Snapshotting {
                    num_tables_synced,
                    total_tables,
                    current_component: String::from(current_component),
                    current_table: current_table.to_string(),
                    num_documents_synced_in_current_table,
                    total_documents_in_current_table,
                    num_documents_synced,
                    total_documents,
                },
                DataSyncState::Stale {
                    total_tables,
                    num_documents_synced,
                    synced_ts,
                } => SerializedDataSyncState::Stale {
                    total_tables,
                    num_documents_synced,
                    synced_ts,
                },
                DataSyncState::UpToDate {
                    total_tables,
                    num_documents_synced,
                    synced_ts,
                } => SerializedDataSyncState::UpToDate {
                    total_tables,
                    num_documents_synced,
                    synced_ts,
                },
            },
        })
    }
}

impl TryFrom<SerializedDataSyncProgressMetadata> for DataSyncProgressMetadata {
    type Error = anyhow::Error;

    fn try_from(value: SerializedDataSyncProgressMetadata) -> Result<Self, Self::Error> {
        Ok(Self {
            sync_id: value.sync_id,
            last_updated_ms: value.last_updated_ms,
            state: match value.state {
                SerializedDataSyncState::Snapshotting {
                    num_tables_synced,
                    total_tables,
                    current_component,
                    current_table,
                    num_documents_synced_in_current_table,
                    total_documents_in_current_table,
                    num_documents_synced,
                    total_documents,
                } => DataSyncState::Snapshotting {
                    num_tables_synced,
                    total_tables,
                    current_component: current_component.parse()?,
                    current_table: current_table.parse()?,
                    num_documents_synced_in_current_table,
                    total_documents_in_current_table,
                    num_documents_synced,
                    total_documents,
                },
                SerializedDataSyncState::Stale {
                    total_tables,
                    num_documents_synced,
                    synced_ts,
                } => DataSyncState::Stale {
                    total_tables,
                    num_documents_synced,
                    synced_ts,
                },
                SerializedDataSyncState::UpToDate {
                    total_tables,
                    num_documents_synced,
                    synced_ts,
                } => DataSyncState::UpToDate {
                    total_tables,
                    num_documents_synced,
                    synced_ts,
                },
            },
        })
    }
}

codegen_convex_serialization!(DataSyncProgressMetadata, SerializedDataSyncProgressMetadata);
