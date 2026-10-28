use std::mem::size_of;

use common::{
    document::ResolvedDocument,
    index::IndexEntry,
    knobs::{
        MYSQL_MAX_CHUNK_BYTES,
        MYSQL_MAX_DYNAMIC_SMART_CHUNK_SIZE,
        MYSQL_MAX_FILL_CHUNK_ROWS,
        MYSQL_SMART_CHUNK_MAX_SIZE,
    },
    persistence::{
        DocumentLogEntry,
        DocumentPrevTsQuery,
        IndexBackfillEntry,
        PersistenceIndexEntry,
    },
    types::Timestamp,
    value::InternalDocumentId,
};

pub trait ApproxSize {
    fn approx_size(&self) -> usize;
}

impl<T: ApproxSize> ApproxSize for &T {
    fn approx_size(&self) -> usize {
        (*self).approx_size()
    }
}

impl<T: ApproxSize> ApproxSize for Option<T> {
    fn approx_size(&self) -> usize {
        self.as_ref().map_or("null".len(), |v| v.approx_size())
    }
}

impl ApproxSize for Timestamp {
    fn approx_size(&self) -> usize {
        size_of::<Timestamp>()
    }
}

impl ApproxSize for DocumentPrevTsQuery {
    fn approx_size(&self) -> usize {
        self.id.approx_size() + self.ts.approx_size() + self.prev_ts.approx_size()
    }
}

impl ApproxSize for InternalDocumentId {
    fn approx_size(&self) -> usize {
        self.size()
    }
}

impl ApproxSize for ResolvedDocument {
    fn approx_size(&self) -> usize {
        self.size()
    }
}

impl<T: ApproxSize, U: ApproxSize> ApproxSize for (T, U) {
    fn approx_size(&self) -> usize {
        self.0.approx_size() + self.1.approx_size()
    }
}

impl<T: ApproxSize, U: ApproxSize, V: ApproxSize> ApproxSize for (T, U, V) {
    fn approx_size(&self) -> usize {
        self.0.approx_size() + self.1.approx_size() + self.2.approx_size()
    }
}

impl ApproxSize for DocumentLogEntry {
    fn approx_size(&self) -> usize {
        self.ts.approx_size()
            + self.id.approx_size()
            + self.value.approx_size()
            + self.prev_ts.approx_size()
    }
}

impl ApproxSize for PersistenceIndexEntry {
    fn approx_size(&self) -> usize {
        self.index.id().size() + self.key.len() + InternalDocumentId::MIN.size()
    }
}

impl ApproxSize for IndexBackfillEntry {
    fn approx_size(&self) -> usize {
        self.index.id().size() + self.key.len() + self.value.approx_size()
    }
}

impl ApproxSize for &[u8] {
    fn approx_size(&self) -> usize {
        self.len()
    }
}

impl ApproxSize for bool {
    fn approx_size(&self) -> usize {
        "false".len()
    }
}

impl ApproxSize for IndexEntry {
    fn approx_size(&self) -> usize {
        self.index_id.size()
            + (&*self.key_prefix).approx_size()
            + (&*self.key_sha256).approx_size()
            + self.ts.approx_size()
            + self.key_suffix.as_deref().approx_size()
            + self.deleted.approx_size()
    }
}

struct SmartChunkIter<'a, T: ApproxSize> {
    items: &'a [T],
    max_dynamic_size: usize,
    max_chunk_size: usize,
    max_bytes: usize,
}

impl<'a, T: ApproxSize> Iterator for SmartChunkIter<'a, T> {
    type Item = &'a [T];

    /// Returns the next item and its size in bytes
    fn next(&mut self) -> Option<Self::Item> {
        if self.items.is_empty() {
            return None;
        }
        let mut len = 0;
        let mut total_bytes = 0;
        for item in self.items {
            total_bytes += item.approx_size();
            if len > self.max_chunk_size || (len > 0 && total_bytes > self.max_bytes) {
                break;
            }
            len += 1;
        }
        let chunk_length = if len <= self.max_dynamic_size {
            len
        } else if len >= self.max_chunk_size {
            self.max_chunk_size
        } else {
            // Round down to power of 2.
            1 << (len.ilog2() as usize)
        };
        let next_chunk = &self.items[0..chunk_length];
        self.items = &self.items[chunk_length..];
        Some(next_chunk)
    }
}

pub fn smart_chunks<T: ApproxSize>(items: &[T]) -> impl Iterator<Item = &[T]> {
    SmartChunkIter {
        items,
        max_dynamic_size: *MYSQL_MAX_DYNAMIC_SMART_CHUNK_SIZE,
        max_chunk_size: *MYSQL_SMART_CHUNK_MAX_SIZE,
        max_bytes: *MYSQL_MAX_CHUNK_BYTES,
    }
}

/// Possible lengths of chunks returned by smart_chunks.
pub fn smart_chunk_sizes() -> impl Iterator<Item = usize> {
    (1..=*MYSQL_SMART_CHUNK_MAX_SIZE)
        .filter(|len| *len <= *MYSQL_MAX_DYNAMIC_SMART_CHUNK_SIZE || len.is_power_of_two())
}

struct FillChunkIter<'a, T: ApproxSize> {
    items: &'a [T],
    max_bytes: usize,
    max_rows: usize,
}

impl<'a, T: ApproxSize> Iterator for FillChunkIter<'a, T> {
    type Item = &'a [T];

    fn next(&mut self) -> Option<Self::Item> {
        if self.items.is_empty() {
            return None;
        }
        let mut len = 0;
        let mut total_bytes = 0;
        for item in self.items {
            total_bytes += item.approx_size();
            if len > 0 && (len >= self.max_rows || total_bytes > self.max_bytes) {
                break;
            }
            len += 1;
        }
        let (chunk, remaining) = self.items.split_at(len);
        self.items = remaining;
        Some(chunk)
    }
}

/// Fills write chunks up to the approximate byte budget or the row cap. An
/// oversized row gets a chunk of its own to ensure progress.
pub fn fill_chunks<T: ApproxSize>(items: &[T]) -> impl Iterator<Item = &[T]> {
    FillChunkIter {
        items,
        max_bytes: *MYSQL_MAX_CHUNK_BYTES,
        max_rows: *MYSQL_MAX_FILL_CHUNK_ROWS,
    }
}
