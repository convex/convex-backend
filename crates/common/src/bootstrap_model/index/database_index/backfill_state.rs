use serde::{
    Deserialize,
    Serialize,
};
use sync_types::Timestamp;

/// State for an index that is currently backfilling.
/// Progress checkpoints are stored in `_index_backfills`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseIndexBackfillState {
    // A timestamp when the index was created. Note that this timestamp is slightly
    // before the index was committed because we don't know the commit timestamp.
    // We need to run retention from this timestamp, because live writes write to
    // the index the moment the index committed.
    pub index_created_lower_bound: Timestamp,
    // Once true, live writes use `ScanComplete` while retention catches up.
    pub retention_started: bool,
    /// Whether the index is staged.
    pub staged: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedDatabaseIndexBackfillState {
    // NOTE: Those fields should be populated for all latest indexes. We keep them
    // as option if we ever need to parse historical documents.
    index_created_lower_bound: Option<i64>,
    retention_started: Option<bool>,
    staged: Option<bool>,
}

impl TryFrom<DatabaseIndexBackfillState> for SerializedDatabaseIndexBackfillState {
    type Error = anyhow::Error;

    fn try_from(config: DatabaseIndexBackfillState) -> anyhow::Result<Self> {
        Ok(Self {
            index_created_lower_bound: Some(config.index_created_lower_bound.into()),
            retention_started: Some(config.retention_started),
            staged: Some(config.staged),
        })
    }
}

impl TryFrom<SerializedDatabaseIndexBackfillState> for DatabaseIndexBackfillState {
    type Error = anyhow::Error;

    fn try_from(config: SerializedDatabaseIndexBackfillState) -> anyhow::Result<Self> {
        Ok(Self {
            index_created_lower_bound: config
                .index_created_lower_bound
                .map(|ts| ts.try_into())
                .transpose()?
                .unwrap_or(Timestamp::MIN),
            retention_started: config.retention_started.unwrap_or(false),
            staged: config.staged.unwrap_or(false),
        })
    }
}
