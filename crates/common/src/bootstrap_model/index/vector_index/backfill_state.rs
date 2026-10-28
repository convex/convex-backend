use serde::{
    Deserialize,
    Serialize,
};
use sync_types::Timestamp;
use value::codegen_convex_serialization;

use super::segment::{
    FragmentedVectorSegment,
    SerializedFragmentedVectorSegment,
};
use crate::bootstrap_model::index::search_index::{
    BackfillState,
    SearchBackfillCursor,
};

pub type VectorIndexBackfillState = BackfillState<FragmentedVectorSegment>;

#[derive(Serialize, Deserialize)]
pub struct SerializedVectorIndexBackfillState {
    segments: Option<Vec<SerializedFragmentedVectorSegment>>,
    table_scan_cursor: Option<Vec<u8>>,
    last_segment_ts: Option<i64>,
    staged: Option<bool>,
}

impl TryFrom<VectorIndexBackfillState> for SerializedVectorIndexBackfillState {
    type Error = anyhow::Error;

    fn try_from(backfill_state: VectorIndexBackfillState) -> Result<Self, Self::Error> {
        let last_segment_ts = backfill_state
            .cursor
            .as_ref()
            .map(|c| c.last_segment_ts.into());
        Ok(SerializedVectorIndexBackfillState {
            segments: Some(
                backfill_state
                    .segments
                    .into_iter()
                    .map(|s| s.try_into())
                    .collect::<anyhow::Result<Vec<_>>>()?,
            ),
            table_scan_cursor: backfill_state.cursor.map(|c| c.table_scan_cursor),
            last_segment_ts,
            staged: Some(backfill_state.staged),
        })
    }
}

impl TryFrom<SerializedVectorIndexBackfillState> for VectorIndexBackfillState {
    type Error = anyhow::Error;

    fn try_from(serialized: SerializedVectorIndexBackfillState) -> Result<Self, Self::Error> {
        let table_scan_cursor = serialized.table_scan_cursor;
        let last_segment_ts = serialized
            .last_segment_ts
            .map(Timestamp::try_from)
            .transpose()?;
        let cursor = match (table_scan_cursor, last_segment_ts) {
            (Some(table_scan_cursor), Some(last_segment_ts)) => Some(SearchBackfillCursor {
                last_segment_ts,
                table_scan_cursor,
            }),
            (None, None) => None,
            _ => anyhow::bail!(
                "VectorIndexBackfillState must have both table_scan_cursor and last_segment_ts"
            ),
        };
        Ok(VectorIndexBackfillState {
            segments: serialized
                .segments
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.try_into())
                .collect::<anyhow::Result<Vec<_>>>()?,
            cursor,
            staged: serialized.staged.unwrap_or_default(),
        })
    }
}

codegen_convex_serialization!(VectorIndexBackfillState, SerializedVectorIndexBackfillState);
