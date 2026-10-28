use std::sync::Arc;

use async_trait::async_trait;
use common::{
    document::PackedDocument,
    index::IndexKeyBytes,
    interval::Interval,
    persistence::{
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
    TabletId,
};

use crate::metrics::{
    index_page_timer,
    log_index_page_point_lookup,
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
        let timer = index_page_timer("local");
        if interval.is_singleton().is_some() {
            log_index_page_point_lookup();
        }
        let result = async {
            let mut stream = PersistenceSnapshot::index_scan(
                self,
                index,
                tablet_id,
                interval,
                order,
                max_results,
            );
            let mut entries = vec![];
            while let Some(result) = stream.next().await {
                let (key, LatestDocument { ts, value, .. }) = result?;
                entries.push(Arc::new(IndexEntry {
                    key,
                    ts,
                    value: PackedDocument::pack(&value),
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

    fn timestamp(&self) -> RepeatableTimestamp {
        PersistenceSnapshot::timestamp(self)
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
