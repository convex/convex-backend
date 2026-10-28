use sync_types::Timestamp;

/// Generic backfill state for search indexes (text and vector).
/// Parameterized by the segment type `S`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackfillState<S> {
    pub segments: Vec<S>,
    /// None at the start of the backfill, set after the first iteration of the
    /// backfill.
    pub cursor: Option<SearchBackfillCursor>,
    pub staged: bool,
}

/// Backfilling cursor for the algorithm with two phases: walking a section
/// of the table at a recent snapshot and then walking the document log
/// since the last segment was written, filling in the updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchBackfillCursor {
    /// The timestamp of the last segment that was backfilled. We have to
    /// scan the document log for changes after this timestamp to documents
    /// that have already been backfilled.
    pub last_segment_ts: Timestamp,
    /// The last document by_id key we indexed in the last segment
    /// backfilled. We start the table scan after this cursor for the next
    /// segment.
    pub table_scan_cursor: Vec<u8>,
}

impl SearchBackfillCursor {
    pub fn backfill_ts(&self) -> Timestamp {
        self.last_segment_ts
    }
}

impl<S> BackfillState<S> {
    pub fn new(staged: bool) -> Self {
        Self {
            segments: vec![],
            cursor: None,
            staged,
        }
    }

    pub fn backfill_ts(&self) -> Option<Timestamp> {
        self.cursor.as_ref().map(|c| c.backfill_ts())
    }
}
