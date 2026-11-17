//! Attributes index scans that reach persistence to what issued them and to
//! the kind of index and the table they read, so persistence read load can be
//! broken down by caller.

use std::{
    pin::Pin,
    task::{
        Context,
        Poll,
    },
    time::{
        Duration,
        Instant,
    },
};

use futures::{
    Stream,
    StreamExt,
};
use value::TableName;

use super::{
    IndexStream,
    LatestDocument,
};
use crate::{
    index::IndexKeyBytes,
    metrics::log_index_scan,
};

#[derive(Clone, Copy, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum IndexScanCaller {
    /// Transactions the conductor runs itself.
    Transaction,
    /// Functions' reads: funrun's, which the conductor serves through
    /// `Database::index_page`, and the in-process function runner's.
    FunctionRunner,
    /// Loading table and index metadata when the database loads, including
    /// the committer's first table summary, and in tools.
    Bootstrap,
    /// Reloading table and index metadata for each table summary checkpoint.
    TableSummaryCheckpoint,
    /// Reloading table and index metadata to catch table shapes up to a
    /// timestamp other than the published one.
    TableShapes,
    InMemoryIndexLoad,
    IndexWorker,
    Retention,
    SearchFlusher,
    TableIterator,
    DataSync,
}

#[derive(Clone, Copy, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum IndexScanKind {
    ById,
    ByCreationTime,
    /// An index a developer or a system table defines.
    Defined,
    Unknown,
}

/// Counts the documents `stream` yields, and the time its consumer spends
/// waiting for them, under `caller`, `kind` and `table` once the stream ends
/// or is dropped. `table` is the table `stream` reads, if the caller knows it.
pub fn attribute_index_scan(
    stream: IndexStream<'_>,
    caller: IndexScanCaller,
    kind: IndexScanKind,
    table: Option<TableName>,
) -> IndexStream<'_> {
    Box::pin(AttributedIndexScan {
        inner: stream,
        caller,
        kind,
        table,
        documents: 0,
        waited: Duration::ZERO,
        waiting_since: None,
        logged: false,
    })
}

struct AttributedIndexScan<'a> {
    inner: IndexStream<'a>,
    caller: IndexScanCaller,
    kind: IndexScanKind,
    table: Option<TableName>,
    documents: u64,
    waited: Duration,
    /// When the consumer asked for the document it is waiting for. Time the
    /// consumer spends processing a document is not waiting.
    waiting_since: Option<Instant>,
    logged: bool,
}

impl AttributedIndexScan<'_> {
    fn log(&mut self) {
        if !self.logged {
            self.logged = true;
            log_index_scan(
                self.caller,
                self.kind,
                self.table.as_ref(),
                self.documents,
                self.waited,
            );
        }
    }
}

impl Stream for AttributedIndexScan<'_> {
    type Item = anyhow::Result<(IndexKeyBytes, LatestDocument)>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let waiting_since = *self.waiting_since.get_or_insert_with(Instant::now);
        let polled = self.inner.poll_next_unpin(cx);
        if polled.is_ready() {
            self.waited += waiting_since.elapsed();
            self.waiting_since = None;
        }
        match &polled {
            Poll::Ready(Some(Ok(_))) => self.documents += 1,
            Poll::Ready(None) => self.log(),
            Poll::Ready(Some(Err(_))) | Poll::Pending => {},
        }
        polled
    }
}

impl Drop for AttributedIndexScan<'_> {
    fn drop(&mut self) {
        if let Some(waiting_since) = self.waiting_since.take() {
            self.waited += waiting_since.elapsed();
        }
        self.log();
    }
}
