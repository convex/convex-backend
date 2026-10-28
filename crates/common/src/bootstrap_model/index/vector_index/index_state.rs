use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

use super::{
    backfill_state::SerializedVectorIndexBackfillState,
    index_snapshot::{
        SerializedVectorIndexSnapshot,
        VectorIndexSnapshot,
        VectorIndexSnapshotData,
    },
    segment::FragmentedVectorSegment,
    VectorIndexBackfillState,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorIndexState {
    Backfilling(VectorIndexBackfillState),
    Backfilled {
        snapshot: VectorIndexSnapshot,
        staged: bool,
    },
    SnapshottedAt(VectorIndexSnapshot),
}

impl VectorIndexState {
    pub fn is_staged(&self) -> bool {
        match self {
            VectorIndexState::Backfilling(backfill_state) => backfill_state.staged,
            VectorIndexState::Backfilled { staged, .. } => *staged,
            VectorIndexState::SnapshottedAt(_) => false,
        }
    }

    pub fn set_staged(&mut self, staged_new: bool) {
        match self {
            VectorIndexState::Backfilling(backfill_state) => {
                backfill_state.staged = staged_new;
            },
            VectorIndexState::Backfilled { staged, .. } => {
                *staged = staged_new;
            },
            VectorIndexState::SnapshottedAt(_) => {},
        }
    }

    pub fn segments(&self) -> anyhow::Result<&Vec<FragmentedVectorSegment>> {
        match self {
            VectorIndexState::Backfilling(backfill_state) => Ok(&backfill_state.segments),
            VectorIndexState::Backfilled { snapshot, .. }
            | VectorIndexState::SnapshottedAt(snapshot) => match snapshot.data {
                VectorIndexSnapshotData::MultiSegment(ref segments) => Ok(segments),
                VectorIndexSnapshotData::Unknown(_) => anyhow::bail!("Unknown snapshot data!"),
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SerializedVectorIndexState {
    Backfilling(SerializedVectorIndexBackfillState),
    Backfilled(SerializedVectorIndexSnapshot),
    /// New format for representing staged backfilled index state.
    Backfilled2 {
        snapshot: SerializedVectorIndexSnapshot,
        staged: bool,
    },
    Snapshotted(SerializedVectorIndexSnapshot),
}

impl TryFrom<VectorIndexState> for SerializedVectorIndexState {
    type Error = anyhow::Error;

    fn try_from(state: VectorIndexState) -> Result<Self, Self::Error> {
        Ok(match state {
            VectorIndexState::Backfilling(backfill_state) => {
                SerializedVectorIndexState::Backfilling(backfill_state.try_into()?)
            },
            VectorIndexState::Backfilled { snapshot, staged } => {
                SerializedVectorIndexState::Backfilled2 {
                    snapshot: snapshot.try_into()?,
                    staged,
                }
            },
            VectorIndexState::SnapshottedAt(snapshot) => {
                SerializedVectorIndexState::Snapshotted(snapshot.try_into()?)
            },
        })
    }
}

impl TryFrom<SerializedVectorIndexState> for VectorIndexState {
    type Error = anyhow::Error;

    fn try_from(serialized: SerializedVectorIndexState) -> Result<Self, Self::Error> {
        Ok(match serialized {
            SerializedVectorIndexState::Backfilling(backfill_state) => {
                VectorIndexState::Backfilling(backfill_state.try_into()?)
            },
            SerializedVectorIndexState::Backfilled(snapshot) => VectorIndexState::Backfilled {
                snapshot: snapshot.try_into()?,
                staged: false,
            },
            SerializedVectorIndexState::Backfilled2 { snapshot, staged } => {
                VectorIndexState::Backfilled {
                    snapshot: snapshot.try_into()?,
                    staged,
                }
            },
            SerializedVectorIndexState::Snapshotted(snapshot) => {
                VectorIndexState::SnapshottedAt(snapshot.try_into()?)
            },
        })
    }
}

codegen_convex_serialization!(
    VectorIndexState,
    SerializedVectorIndexState,
    test_cases = 64
);
