use serde::{
    Deserialize,
    Serialize,
};
use sync_types::Timestamp;
use value::codegen_convex_serialization;

use crate::bootstrap_model::index::{
    search_index::{
        BackfillState,
        SearchBackfillCursor,
    },
    text_index::{
        index_snapshot::SerializedFragmentedTextSegment,
        FragmentedTextSegment,
    },
};

pub type TextIndexBackfillState = BackfillState<FragmentedTextSegment>;

#[derive(Serialize, Deserialize)]
pub struct SerializedTextBackfillCursor {
    pub table_scan_cursor: Option<Vec<u8>>,
    pub last_segment_ts: Option<i64>,
}

#[derive(Serialize, Deserialize)]
pub struct SerializedTextIndexBackfillState {
    segments: Option<Vec<SerializedFragmentedTextSegment>>,
    cursor: Option<SerializedTextBackfillCursor>,
    staged: Option<bool>,
}

impl TryFrom<TextIndexBackfillState> for SerializedTextIndexBackfillState {
    type Error = anyhow::Error;

    fn try_from(backfill_state: TextIndexBackfillState) -> Result<Self, Self::Error> {
        let cursor = backfill_state.cursor.map(
            |SearchBackfillCursor {
                 last_segment_ts,
                 table_scan_cursor,
             }| {
                SerializedTextBackfillCursor {
                    table_scan_cursor: Some(table_scan_cursor),
                    last_segment_ts: Some(last_segment_ts.into()),
                }
            },
        );
        Ok(SerializedTextIndexBackfillState {
            segments: Some(
                backfill_state
                    .segments
                    .into_iter()
                    .map(|s| s.try_into())
                    .collect::<anyhow::Result<Vec<_>>>()?,
            ),
            cursor,
            staged: Some(backfill_state.staged),
        })
    }
}

impl TryFrom<SerializedTextIndexBackfillState> for TextIndexBackfillState {
    type Error = anyhow::Error;

    fn try_from(serialized: SerializedTextIndexBackfillState) -> Result<Self, Self::Error> {
        let cursor_data = serialized.cursor;
        let cursor = if let Some(c) = cursor_data {
            let table_scan_cursor = c.table_scan_cursor;
            let last_segment_ts = c.last_segment_ts.map(Timestamp::try_from).transpose()?;
            match (table_scan_cursor, last_segment_ts) {
                (Some(table_scan_cursor), Some(last_segment_ts)) => Some(SearchBackfillCursor {
                    last_segment_ts,
                    table_scan_cursor,
                }),
                (None, None) => None,
                _ => anyhow::bail!(
                    "TextIndexBackfillState must have both table_scan_cursor and last_segment_ts"
                ),
            }
        } else {
            None
        };
        Ok(TextIndexBackfillState {
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

codegen_convex_serialization!(TextIndexBackfillState, SerializedTextIndexBackfillState);
