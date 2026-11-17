use serde::{
    Deserialize,
    Serialize,
};
use sync_types::Timestamp;
use value::{
    codegen_convex_serialization,
    DeveloperDocumentId,
};

/// Cursor for a database index that has an in-progress backfill
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BackfillCursor {
    /// Snapshot used by the current scan, refreshed on resume for diagnostics.
    pub snapshot_ts: Timestamp,
    pub cursor: Option<DeveloperDocumentId>,
}

/// Metadata for tracking index backfill progress.
///
/// This structure stores the progress of an index backfill operation,
/// tracking how many documents and bytes have been processed out of the total.
/// NB: We don't track the progress for catching up from the snapshot. This
/// should be relatively short, and we can show that we're not yet complete in
/// the UI. We can add timestamp-based progress for that phase in the future,
/// but number of documents and bytes is not possible to track because we walk
/// the revision stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexBackfillMetadata {
    /// The ID of the index being backfilled. Should correspond to a document in
    /// the index table in `Backfilling` state.
    pub index_id: DeveloperDocumentId,
    /// Number of documents that have been indexed so far from the snapshot
    /// (does not include documents written since the backfill began).
    pub num_docs_indexed: u64,
    /// Total number of documents in the table from the snapshot
    /// (does not include documents written since the backfill began)
    /// This field is None if there is no table summary available.
    pub total_docs: Option<u64>,
    /// We only track the backfill cursor for database indexes because search
    /// index backfill state is stored in the _index table.
    pub cursor: Option<BackfillCursor>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedBackfillCursor {
    snapshot_ts: i64,
    cursor: Option<String>,
}

impl From<BackfillCursor> for SerializedBackfillCursor {
    fn from(cursor: BackfillCursor) -> Self {
        SerializedBackfillCursor {
            snapshot_ts: cursor.snapshot_ts.into(),
            cursor: cursor.cursor.map(|id| id.to_string()),
        }
    }
}

impl TryFrom<SerializedBackfillCursor> for BackfillCursor {
    type Error = anyhow::Error;

    fn try_from(cursor: SerializedBackfillCursor) -> Result<Self, Self::Error> {
        Ok(BackfillCursor {
            snapshot_ts: cursor.snapshot_ts.try_into()?,
            cursor: cursor.cursor.map(|id| id.parse()).transpose()?,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedIndexBackfillMetadata {
    index_id: String,
    num_docs_indexed: i64,
    total_docs: Option<i64>,
    cursor: Option<SerializedBackfillCursor>,
}

impl From<IndexBackfillMetadata> for SerializedIndexBackfillMetadata {
    fn from(metadata: IndexBackfillMetadata) -> Self {
        SerializedIndexBackfillMetadata {
            index_id: metadata.index_id.to_string(),
            num_docs_indexed: metadata.num_docs_indexed as i64,
            total_docs: metadata.total_docs.map(|v| v as i64),
            cursor: metadata.cursor.map(|cursor| cursor.into()),
        }
    }
}

impl TryFrom<SerializedIndexBackfillMetadata> for IndexBackfillMetadata {
    type Error = anyhow::Error;

    fn try_from(serialized: SerializedIndexBackfillMetadata) -> Result<Self, Self::Error> {
        Ok(Self {
            index_id: serialized.index_id.parse()?,
            num_docs_indexed: serialized.num_docs_indexed as u64,
            total_docs: serialized.total_docs.map(|v| v as u64),
            cursor: serialized.cursor.map(|c| c.try_into()).transpose()?,
        })
    }
}

codegen_convex_serialization!(IndexBackfillMetadata, SerializedIndexBackfillMetadata);
