use serde::{
    Deserialize,
    Serialize,
};
use sync_types::Timestamp;
use value::codegen_convex_serialization;

use super::{
    DatabaseIndexBackfillState,
    SerializedDatabaseIndexBackfillState,
};
use crate::types::IndexWriteMode;

/// Represents the state of an index.
/// Table scan index for a newly created table starts at `Enabled`. All
/// other indexes start at `Backfilling` state and are transitioned to
/// `Enabled` by the index backfill routine. Disabled indexes are not
/// implicitly transitioned to any other state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseIndexState {
    // We are backfilling this index. All new writes should update the index.
    Backfilling(DatabaseIndexBackfillState),
    // The index is fully backfilled, but hasn't yet been committed and is not
    // yet available for reads.
    Backfilled { staged: bool },
    // Index is fully backfilled and ready to serve reads.
    Enabled,
}

impl DatabaseIndexState {
    pub fn is_staged(&self) -> bool {
        match self {
            Self::Backfilling(index_state) => index_state.staged,
            Self::Backfilled { staged } => *staged,
            Self::Enabled => false,
        }
    }

    pub fn set_staged(&mut self, staged_new: bool) {
        match self {
            Self::Backfilling(index_state) => {
                index_state.staged = staged_new;
            },
            Self::Backfilled { staged } => {
                *staged = staged_new;
            },
            Self::Enabled => {},
        }
    }

    /// `retention_started` means index backfill has inserted the existing
    /// entries, so writes can use `ScanComplete` while retention catches
    /// up.
    pub fn write_mode(&self) -> IndexWriteMode {
        match self {
            Self::Backfilling(backfill) if !backfill.retention_started => IndexWriteMode::Scanning,
            Self::Backfilling(_) | Self::Backfilled { .. } | Self::Enabled => {
                IndexWriteMode::ScanComplete
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum SerializedDatabaseIndexState {
    #[serde(rename_all = "camelCase")]
    Backfilling {
        backfill_state: SerializedDatabaseIndexBackfillState,
    },
    // Use Backfilled2 to distinguish between records impacted by CX-3897
    Backfilled2 {
        staged: Option<bool>,
    },
    Enabled,

    // We have historical records with Disabled state.
    Disabled,
}

impl TryFrom<DatabaseIndexState> for SerializedDatabaseIndexState {
    type Error = anyhow::Error;

    fn try_from(config: DatabaseIndexState) -> anyhow::Result<Self> {
        Ok(match config {
            DatabaseIndexState::Backfilling(st) => SerializedDatabaseIndexState::Backfilling {
                backfill_state: st.try_into()?,
            },
            DatabaseIndexState::Backfilled { staged } => {
                SerializedDatabaseIndexState::Backfilled2 {
                    staged: Some(staged),
                }
            },
            DatabaseIndexState::Enabled => SerializedDatabaseIndexState::Enabled,
        })
    }
}

impl TryFrom<SerializedDatabaseIndexState> for DatabaseIndexState {
    type Error = anyhow::Error;

    fn try_from(config: SerializedDatabaseIndexState) -> anyhow::Result<Self> {
        Ok(match config {
            SerializedDatabaseIndexState::Backfilling { backfill_state } => {
                DatabaseIndexState::Backfilling(backfill_state.try_into()?)
            },
            SerializedDatabaseIndexState::Backfilled2 { staged } => {
                DatabaseIndexState::Backfilled {
                    staged: staged.unwrap_or_default(),
                }
            },
            SerializedDatabaseIndexState::Enabled => DatabaseIndexState::Enabled,
            // None of the latest index documents should be in this state.
            SerializedDatabaseIndexState::Disabled => {
                DatabaseIndexState::Backfilling(DatabaseIndexBackfillState {
                    index_created_lower_bound: Timestamp::MIN,
                    retention_started: false,
                    staged: false,
                })
            },
        })
    }
}

codegen_convex_serialization!(DatabaseIndexState, SerializedDatabaseIndexState);
