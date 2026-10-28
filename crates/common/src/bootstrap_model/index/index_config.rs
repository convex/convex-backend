use std::fmt::Debug;

use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

use super::{
    database_index::{
        DatabaseIndexSpec,
        DatabaseIndexState,
        SerializedDatabaseIndexSpec,
        SerializedDatabaseIndexState,
    },
    text_index::{
        SerializedTextIndexSpec,
        SerializedTextIndexState,
        TextIndexSpec,
        TextIndexState,
    },
    vector_index::{
        SerializedVectorIndexSpec,
        SerializedVectorIndexState,
        VectorIndexSnapshotData,
        VectorIndexSpec,
        VectorIndexState,
    },
};
use crate::{
    bootstrap_model::index::text_index::TextIndexSnapshotData,
    types::PersistenceIndexId,
};

/// Configuration that depends on the type of index.
///
/// Split into two parts:
///   spec: Specification of the identity of index.
///   state: State of index that can change over time.
///
/// If spec changes (eg fields), it's a *different* index.
/// State can change over time (eg. backfill state or staged flag)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexConfig {
    /// Standard database index.
    Database {
        spec: DatabaseIndexSpec,
        on_disk_state: DatabaseIndexState,
        persistence_index_id: Option<PersistenceIndexId>,
    },

    /// Full text search index.
    Text {
        spec: TextIndexSpec,
        on_disk_state: TextIndexState,
    },

    Vector {
        spec: VectorIndexSpec,
        on_disk_state: VectorIndexState,
    },
}

impl IndexConfig {
    pub fn is_staged(&self) -> bool {
        match self {
            Self::Database { on_disk_state, .. } => on_disk_state.is_staged(),
            Self::Text { on_disk_state, .. } => on_disk_state.is_staged(),
            Self::Vector { on_disk_state, .. } => on_disk_state.is_staged(),
        }
    }

    pub fn set_staged(&mut self, staged: bool) {
        match self {
            Self::Database { on_disk_state, .. } => on_disk_state.set_staged(staged),
            Self::Text { on_disk_state, .. } => on_disk_state.set_staged(staged),
            Self::Vector { on_disk_state, .. } => on_disk_state.set_staged(staged),
        }
    }

    pub fn is_enabled(&self) -> bool {
        match self {
            IndexConfig::Database { on_disk_state, .. } => {
                matches!(on_disk_state, DatabaseIndexState::Enabled)
            },
            IndexConfig::Text { on_disk_state, .. } => {
                matches!(on_disk_state, TextIndexState::SnapshottedAt(_))
            },
            IndexConfig::Vector { on_disk_state, .. } => {
                matches!(on_disk_state, VectorIndexState::SnapshottedAt(_))
            },
        }
    }

    pub fn is_backfilling(&self) -> bool {
        match self {
            IndexConfig::Database { on_disk_state, .. } => {
                matches!(on_disk_state, DatabaseIndexState::Backfilling(_))
            },
            IndexConfig::Text { on_disk_state, .. } => {
                matches!(on_disk_state, TextIndexState::Backfilling(_))
            },
            IndexConfig::Vector { on_disk_state, .. } => {
                matches!(on_disk_state, VectorIndexState::Backfilling(_))
            },
        }
    }

    pub fn is_backfilled(&self) -> bool {
        match self {
            IndexConfig::Database { on_disk_state, .. } => {
                matches!(on_disk_state, DatabaseIndexState::Backfilled { .. })
            },
            IndexConfig::Text { on_disk_state, .. } => {
                matches!(on_disk_state, TextIndexState::Backfilled { .. })
            },
            IndexConfig::Vector { on_disk_state, .. } => {
                matches!(on_disk_state, VectorIndexState::Backfilled { .. })
            },
        }
    }

    pub fn same_spec(&self, config: &IndexConfig) -> bool {
        match (self, config) {
            (
                IndexConfig::Database { spec, .. },
                IndexConfig::Database {
                    spec: spec_to_compare,
                    ..
                },
            ) => spec == spec_to_compare,
            (
                IndexConfig::Text { spec, .. },
                IndexConfig::Text {
                    spec: spec_to_compare,
                    ..
                },
            ) => spec == spec_to_compare,
            (
                IndexConfig::Vector { spec, .. },
                IndexConfig::Vector {
                    spec: spec_to_compare,
                    ..
                },
            ) => spec == spec_to_compare,
            (..) => false,
        }
    }

    /// Returns the estimated size of the index in bytes in a manner suitable
    /// for usage and pricing.
    ///
    /// The estimate here may not accurately reflect the actual number of
    /// stored bytes and may not be appropriate for estimate resource usage. For
    /// example, small dimension vector indexes may have 20% overhead from
    /// HNSW indexes that won't be reflected here, but would require
    /// additional RAM or disk space to serve.
    ///
    /// This is only implemented for vector indexes for now. Calling this method
    /// on other index types will panic.
    pub fn estimate_pricing_size_bytes(&self) -> anyhow::Result<u64> {
        let size = match self {
            IndexConfig::Database { .. } => {
                // TODO: We should support this for all index types in the future. Right
                // now we estimate the size of database indexes. This could instead track usage
                // in their metadata, similar to vector indexes.
                anyhow::bail!("Not supported for database indexes!")
            },
            IndexConfig::Text { on_disk_state, .. } => {
                let snapshot = match on_disk_state {
                    TextIndexState::SnapshottedAt(snapshot) => Some(snapshot),
                    TextIndexState::Backfilled { snapshot, staged } if *staged => Some(snapshot),
                    TextIndexState::Backfilling(_) | TextIndexState::Backfilled { .. } => None,
                };
                snapshot
                    .map(|snapshot| match &snapshot.data {
                        TextIndexSnapshotData::MultiSegment(segments) => segments
                            .iter()
                            .map(|segment| segment.size_bytes_total)
                            .sum(),
                        TextIndexSnapshotData::Unknown(_) => 0,
                    })
                    .unwrap_or(0)
            },
            IndexConfig::Vector {
                on_disk_state,
                spec,
            } => {
                let snapshot = match on_disk_state {
                    VectorIndexState::SnapshottedAt(snapshot) => Some(snapshot),
                    VectorIndexState::Backfilled { snapshot, staged } if *staged => Some(snapshot),
                    VectorIndexState::Backfilling(_) | VectorIndexState::Backfilled { .. } => None,
                };
                snapshot
                    .map(|snapshot| match &snapshot.data {
                        VectorIndexSnapshotData::MultiSegment(segments) => segments
                            .iter()
                            .map(|segment| segment.non_deleted_size_bytes(spec.dimensions))
                            .sum::<anyhow::Result<_>>(),
                        VectorIndexSnapshotData::Unknown(_) => Ok(0),
                    })
                    .transpose()?
                    .unwrap_or(0)
            },
        };
        Ok(size)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SerializedIndexConfig {
    #[serde(rename_all = "camelCase")]
    Database {
        #[serde(flatten)]
        spec: SerializedDatabaseIndexSpec,
        on_disk_state: SerializedDatabaseIndexState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        persistence_index_id: Option<i64>,
    },
    #[serde(rename_all = "camelCase")]
    Search {
        #[serde(flatten)]
        spec: SerializedTextIndexSpec,
        on_disk_state: SerializedTextIndexState,
    },
    #[serde(rename_all = "camelCase")]
    Vector {
        #[serde(flatten)]
        spec: SerializedVectorIndexSpec,
        on_disk_state: SerializedVectorIndexState,
    },
}

impl TryFrom<IndexConfig> for SerializedIndexConfig {
    type Error = anyhow::Error;

    fn try_from(config: IndexConfig) -> anyhow::Result<Self> {
        Ok(match config {
            IndexConfig::Database {
                spec,
                on_disk_state,
                persistence_index_id,
            } => SerializedIndexConfig::Database {
                spec: spec.into(),
                on_disk_state: on_disk_state.try_into()?,
                persistence_index_id: persistence_index_id.map(|id| i64::from(id.value())),
            },
            IndexConfig::Text {
                spec,
                on_disk_state,
            } => SerializedIndexConfig::Search {
                spec: spec.into(),
                on_disk_state: on_disk_state.try_into()?,
            },
            IndexConfig::Vector {
                spec,
                on_disk_state,
            } => SerializedIndexConfig::Vector {
                spec: spec.into(),
                on_disk_state: on_disk_state.try_into()?,
            },
        })
    }
}

impl TryFrom<SerializedIndexConfig> for IndexConfig {
    type Error = anyhow::Error;

    fn try_from(config: SerializedIndexConfig) -> anyhow::Result<Self> {
        Ok(match config {
            SerializedIndexConfig::Database {
                spec,
                on_disk_state,
                persistence_index_id,
            } => IndexConfig::Database {
                spec: spec.try_into()?,
                on_disk_state: on_disk_state.try_into()?,
                persistence_index_id: persistence_index_id
                    .and_then(|id| u32::try_from(id).ok())
                    .and_then(PersistenceIndexId::new),
            },
            SerializedIndexConfig::Search {
                spec,
                on_disk_state,
            } => IndexConfig::Text {
                spec: spec.try_into()?,
                on_disk_state: on_disk_state.try_into()?,
            },
            SerializedIndexConfig::Vector {
                spec,
                on_disk_state,
            } => IndexConfig::Vector {
                spec: spec.try_into()?,
                on_disk_state: on_disk_state.try_into()?,
            },
        })
    }
}

codegen_convex_serialization!(IndexConfig, SerializedIndexConfig, test_cases = 64);
