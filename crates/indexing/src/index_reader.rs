use std::sync::Arc;

use async_trait::async_trait;
use common::{
    document::PackedDocument,
    index::IndexKeyBytes,
    interval::Interval,
    persistence::{
        index_scan_attribution::{
            attribute_index_scan,
            IndexScanCaller,
            IndexScanKind,
        },
        IndexStream,
        LatestDocument,
        PersistenceSnapshot,
    },
    query::{
        CursorPosition,
        Order,
    },
    types::{
        IndexName,
        IndexRef,
        RepeatableTimestamp,
        TabletIndexName,
        Timestamp,
    },
};
use futures::StreamExt as _;
use futures_async_stream::try_stream;
use value::{
    heap_size::HeapSize,
    TableMapping,
    TabletId,
};

use crate::{
    index_registry::IndexRegistry,
    metrics::{
        index_page_timer,
        log_index_page_point_lookup,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    pub key: IndexKeyBytes,
    pub ts: Timestamp,
    pub value: PackedDocument,
}

impl HeapSize for IndexEntry {
    fn heap_size(&self) -> usize {
        self.key.heap_size() + self.value.heap_size()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexPage {
    /// Entries have unique keys and are ordered according to the requested
    /// [`Order`].
    pub entries: Vec<Arc<IndexEntry>>,
    pub cursor: CursorPosition,
}
#[async_trait]
pub trait IndexReader: Send + Sync {
    async fn index_page(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        interval: &Interval,
        order: Order,
        max_results: usize,
    ) -> anyhow::Result<IndexPage>;

    fn timestamp(&self) -> RepeatableTimestamp;
}

#[async_trait]
impl IndexReader for PersistenceSnapshot {
    async fn index_page(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        interval: &Interval,
        order: Order,
        max_results: usize,
    ) -> anyhow::Result<IndexPage> {
        read_page(
            PersistenceSnapshot::index_scan(self, index, tablet_id, interval, order, max_results),
            interval,
            max_results,
        )
        .await
    }

    fn timestamp(&self) -> RepeatableTimestamp {
        PersistenceSnapshot::timestamp(self)
    }
}

/// Collects up to `max_results` entries of `stream`, a scan of `interval`.
async fn read_page(
    mut stream: IndexStream<'_>,
    interval: &Interval,
    max_results: usize,
) -> anyhow::Result<IndexPage> {
    let timer = index_page_timer("local");
    if interval.is_singleton().is_some() {
        log_index_page_point_lookup();
    }
    let result = async {
        let mut entries = vec![];
        while let Some(result) = stream.next().await {
            let (key, LatestDocument { ts, value, .. }) = result?;
            entries.push(Arc::new(IndexEntry {
                key,
                ts,
                value: PackedDocument::pack(&value).shrink(),
            }));
            if entries.len() >= max_results {
                let cursor = CursorPosition::After(entries.last().unwrap().key.clone());
                return Ok(IndexPage { entries, cursor });
            }
        }
        Ok(IndexPage {
            entries,
            cursor: CursorPosition::End,
        })
    }
    .await;
    if result.is_ok() {
        timer.finish();
    }
    result
}

/// Reads `snapshot`, attributing each page to `caller` and to the kind of
/// index and the table it reads.
pub struct AttributedIndexReader {
    snapshot: PersistenceSnapshot,
    caller: IndexScanCaller,
    index_registry: Option<IndexRegistry>,
    table_mapping: Option<TableMapping>,
}

impl AttributedIndexReader {
    /// Without `index_registry`, the kind of index each read is attributed to
    /// is unknown, and without `table_mapping`, so is its table.
    pub fn new(
        snapshot: PersistenceSnapshot,
        caller: IndexScanCaller,
        index_registry: Option<IndexRegistry>,
        table_mapping: Option<TableMapping>,
    ) -> Self {
        Self {
            snapshot,
            caller,
            index_registry,
            table_mapping,
        }
    }

    fn kind(&self, index: IndexRef, tablet_id: TabletId) -> IndexScanKind {
        let Some(index_registry) = &self.index_registry else {
            return IndexScanKind::Unknown;
        };
        let is = |name: TabletIndexName| {
            index_registry
                .get_enabled(&name)
                .is_some_and(|enabled| enabled.id() == index.id())
        };
        if is(TabletIndexName::by_id(tablet_id)) {
            IndexScanKind::ById
        } else if is(TabletIndexName::by_creation_time(tablet_id)) {
            IndexScanKind::ByCreationTime
        } else {
            IndexScanKind::Defined
        }
    }
}

#[async_trait]
impl IndexReader for AttributedIndexReader {
    async fn index_page(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        interval: &Interval,
        order: Order,
        max_results: usize,
    ) -> anyhow::Result<IndexPage> {
        let scan = attribute_index_scan(
            self.snapshot
                .index_scan(index, tablet_id, interval, order, max_results),
            self.caller,
            self.kind(index, tablet_id),
            self.table_mapping
                .as_ref()
                .and_then(|table_mapping| table_mapping.tablet_name(tablet_id).ok()),
        );
        read_page(scan, interval, max_results).await
    }

    fn timestamp(&self) -> RepeatableTimestamp {
        self.snapshot.timestamp()
    }
}

impl dyn IndexReader {
    /// Convenience wrapper around calling `index_page` repeatedly to scan an
    /// entire interval.
    #[try_stream(ok = IndexEntry, error = anyhow::Error)]
    pub async fn index_scan<'a>(
        &'a self,
        index: IndexRef,
        tablet_id: TabletId,
        mut interval: Interval,
        order: Order,
        page_size: usize,
    ) {
        while !interval.is_empty() {
            let page = self
                .index_page(index, tablet_id, &interval, order, page_size)
                .await?;
            for entry in page.entries {
                yield Arc::unwrap_or_clone(entry);
            }
            (_, interval) = interval.split(page.cursor, order);
        }
    }
}

pub type BatchKey = usize;

#[derive(Debug, Clone)]
pub struct RangeRequest {
    pub index_name: TabletIndexName,
    pub printable_index_name: IndexName,
    pub interval: Interval,
    pub order: Order,
    pub max_size: usize,
}
