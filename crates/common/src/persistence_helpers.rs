use std::sync::Arc;

use anyhow::Context as _;
use futures::{
    Stream,
    TryStreamExt,
};
use futures_async_stream::try_stream;
use value::InternalDocumentId;

use crate::{
    document::ResolvedDocument,
    knobs::DOCUMENTS_IN_MEMORY,
    persistence::{
        DocumentLogEntry,
        DocumentPrevTsQuery,
        PersistenceReader,
        RetentionValidator,
    },
    try_chunks::TryChunksExt,
    types::Timestamp,
};

#[derive(Debug)]
pub struct DocumentRevision {
    pub ts: Timestamp,
    pub document: Option<ResolvedDocument>,
}

#[derive(Debug)]
pub struct RevisionPair {
    pub id: InternalDocumentId,
    pub rev: DocumentRevision,
    /// Note that `prev_rev` cannot be a tombstone. If `prev_rev` is `Some`, but
    /// its `document` is `None`, that means that the revision *has* a
    /// predecessor, but its value was garbage collected.
    pub prev_rev: Option<DocumentRevision>,
}

impl RevisionPair {
    pub fn ts(&self) -> Timestamp {
        self.rev.ts
    }

    pub fn document(&self) -> Option<&ResolvedDocument> {
        self.rev.document.as_ref()
    }

    pub fn prev_document(&self) -> Option<&ResolvedDocument> {
        self.prev_rev.as_ref().and_then(|r| r.document.as_ref())
    }

    /// Throws away the prev_rev's value.
    pub fn into_log_entry(self) -> DocumentLogEntry {
        DocumentLogEntry {
            ts: self.rev.ts,
            id: self.id,
            value: self.rev.document,
            prev_ts: self.prev_rev.map(|rev| rev.ts),
        }
    }
}

type RevisionStreamEntry = anyhow::Result<DocumentLogEntry>;

/// Exposed as PersistenceReader::load_revision_pairs
#[allow(clippy::needless_lifetimes)]
#[try_stream(ok = RevisionPair, error = anyhow::Error)]
pub(crate) async fn persistence_reader_stream_revision_pairs<'a, P: PersistenceReader + ?Sized>(
    documents: impl Stream<Item = RevisionStreamEntry> + 'a,
    reader: &'a P,
    retention_validator: Arc<dyn RetentionValidator>,
) {
    let documents = documents.try_chunks2(*DOCUMENTS_IN_MEMORY);
    futures::pin_mut!(documents);

    while let Some(read_chunk) = documents.try_next().await? {
        let queries = read_chunk
            .iter()
            .filter_map(|entry| {
                entry.prev_ts.map(|prev_ts| DocumentPrevTsQuery {
                    id: entry.id,
                    ts: entry.ts,
                    prev_ts,
                })
            })
            .collect();
        let mut prev_revs = reader
            .previous_revisions_of_documents(queries, retention_validator.clone())
            .await?;
        for DocumentLogEntry {
            ts,
            prev_ts,
            id,
            value: document,
            ..
        } in read_chunk
        {
            let rev = DocumentRevision { ts, document };
            let prev_rev = prev_ts
                .map(|prev_ts| {
                    let document = prev_revs
                        .remove(&DocumentPrevTsQuery { id, ts, prev_ts })
                        .map(|entry| {
                            entry.value.with_context(|| {
                                format!("prev_ts {prev_ts} of {id}@{ts} points to a deleted value?")
                            })
                        })
                        .transpose()?;
                    anyhow::Ok(DocumentRevision {
                        ts: prev_ts,
                        document,
                    })
                })
                .transpose()?;
            yield RevisionPair { id, rev, prev_rev };
        }
    }
}
