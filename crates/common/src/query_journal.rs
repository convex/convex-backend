use value::heap_size::HeapSize;

use crate::query::Cursor;

/// A journal to keep track of decisions made while executing a query function.
///
/// The query journal is synced to the client and re-used whenever a query is
/// re-executed (even if the client recconects to a new backend). This can
/// ensure that re-executions make the same decisions as the initial one did.
///
/// Invariant:
/// At timestamp t, if a query function q produces:
/// `q(arguments, prev_journal) -> (result, next_journal)`
/// then at t,
/// `q(arguments, next_journal) -> (result, next_journal)`
/// Reusing a journal as an input at the same timestamp should
/// produce the same result and the same journal.
///
/// Because this journal is synced to the client, keep its size small!
/// The serialized size is tested in `broker.rs:test_query_journal_size`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct QueryJournal {
    /// If this query function ran a paginated database query, store a cursor
    /// for the end of the query so we can continue to sync to the same point
    /// if this function is re-executed.
    pub end_cursor: Option<Cursor>,
}

impl QueryJournal {
    pub fn new() -> QueryJournal {
        QueryJournal { end_cursor: None }
    }
}

impl HeapSize for QueryJournal {
    fn heap_size(&self) -> usize {
        match &self.end_cursor {
            Some(cursor) => cursor.heap_size(),
            None => 0,
        }
    }
}

impl From<QueryJournal> for pb::convex_query_journal::QueryJournal {
    fn from(QueryJournal { end_cursor }: QueryJournal) -> Self {
        Self {
            cursor: end_cursor.map(pb::convex_cursor::Cursor::from),
        }
    }
}

impl TryFrom<pb::convex_query_journal::QueryJournal> for QueryJournal {
    type Error = anyhow::Error;

    fn try_from(
        pb::convex_query_journal::QueryJournal { cursor }: pb::convex_query_journal::QueryJournal,
    ) -> anyhow::Result<Self> {
        let end_cursor = cursor.map(Cursor::try_from).transpose()?;
        Ok(QueryJournal { end_cursor })
    }
}
