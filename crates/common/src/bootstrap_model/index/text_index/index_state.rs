use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

use super::{
    index_snapshot::SerializedTextIndexSnapshot,
    TextIndexBackfillState,
    TextIndexSnapshot,
};
use crate::bootstrap_model::index::text_index::backfill_state::SerializedTextIndexBackfillState;

/// The state of a text search index.
/// Text search indexes begin in `Backfilling`. Once they finish backfilling,
/// but before they've been committed, they'll be in a `Backfilled` state with a
/// snapshot and timestamp that moves continually forward. Once the index change
/// is committed by the user, they advance to the `SnapshottedAt` state and can
/// be used in queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextIndexState {
    Backfilling(TextIndexBackfillState),
    Backfilled {
        snapshot: TextIndexSnapshot,
        staged: bool,
    },
    SnapshottedAt(TextIndexSnapshot),
}

impl TextIndexState {
    pub fn is_staged(&self) -> bool {
        match self {
            Self::Backfilling(index_state) => index_state.staged,
            Self::Backfilled { staged, .. } => *staged,
            Self::SnapshottedAt(_) => false,
        }
    }

    pub fn set_staged(&mut self, staged_new: bool) {
        match self {
            Self::Backfilling(index_state) => {
                index_state.staged = staged_new;
            },
            Self::Backfilled { staged, .. } => {
                *staged = staged_new;
            },
            Self::SnapshottedAt(_) => {},
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SerializedTextIndexState {
    Backfilling {
        staged: Option<bool>,
    },
    Backfilling2(SerializedTextIndexBackfillState),
    Backfilled(SerializedTextIndexSnapshot),
    /// New format for representing staged backfilled index state.
    Backfilled2 {
        snapshot: SerializedTextIndexSnapshot,
        staged: bool,
    },
    Snapshotted(SerializedTextIndexSnapshot),
}

impl TryFrom<TextIndexState> for SerializedTextIndexState {
    type Error = anyhow::Error;

    fn try_from(state: TextIndexState) -> Result<Self, Self::Error> {
        Ok(match state {
            TextIndexState::Backfilling(state) => {
                // Maintain rollback compatibility with the old format by writing empty
                // backfilling states using the old format. Since we don't
                // currently use the new format, all states should be empty, so
                // we should always write the old format. TODO(CX-6465): Clean
                // this up.
                if state.segments.is_empty() && state.cursor.is_none() {
                    SerializedTextIndexState::Backfilling {
                        staged: Some(state.staged),
                    }
                } else {
                    SerializedTextIndexState::Backfilling2(state.try_into()?)
                }
            },
            TextIndexState::Backfilled { snapshot, staged } => {
                SerializedTextIndexState::Backfilled2 {
                    snapshot: snapshot.try_into()?,
                    staged,
                }
            },
            TextIndexState::SnapshottedAt(snapshot) => {
                SerializedTextIndexState::Snapshotted(snapshot.try_into()?)
            },
        })
    }
}

impl TryFrom<SerializedTextIndexState> for TextIndexState {
    type Error = anyhow::Error;

    fn try_from(serialized: SerializedTextIndexState) -> Result<Self, Self::Error> {
        Ok(match serialized {
            SerializedTextIndexState::Backfilling { staged } => {
                TextIndexState::Backfilling(TextIndexBackfillState::new(staged.unwrap_or_default()))
            },
            SerializedTextIndexState::Backfilling2(backfill_state) => {
                TextIndexState::Backfilling(backfill_state.try_into()?)
            },
            SerializedTextIndexState::Backfilled(snapshot) => TextIndexState::Backfilled {
                snapshot: snapshot.try_into()?,
                staged: false,
            },
            SerializedTextIndexState::Backfilled2 { snapshot, staged } => {
                TextIndexState::Backfilled {
                    snapshot: snapshot.try_into()?,
                    staged,
                }
            },
            SerializedTextIndexState::Snapshotted(snapshot) => {
                TextIndexState::SnapshottedAt(snapshot.try_into()?)
            },
        })
    }
}

codegen_convex_serialization!(TextIndexState, SerializedTextIndexState);
