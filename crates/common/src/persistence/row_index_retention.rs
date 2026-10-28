//! Reclaiming index history for a layout that stores one row per index
//! revision. A layout that keeps index history in units it can drop whole
//! reclaims them on its own and uses none of this.
//!
//! Which index entries expired is derived from the document log rather than
//! read back from index storage: walking revision pairs and re-deriving each
//! document's index keys says what the indexer would have written, and so what
//! is now superseded.

use std::{
    cmp,
    collections::{
        BTreeMap,
        HashSet,
    },
};

use async_trait::async_trait;
use futures::{
    future::try_join_all,
    pin_mut,
    TryStreamExt,
};
use futures_async_stream::try_stream;
use value::{
    sha256::Sha256,
    TabletId,
};

use crate::{
    bootstrap_model::index::database_index::IndexedFields,
    errors::report_error,
    index::{
        IndexEntry,
        SplitKey,
    },
    knobs::{
        INDEX_RETENTION_DELETE_CHUNK,
        INDEX_RETENTION_DELETE_PARALLEL,
        RETENTION_DELETE_BATCH,
    },
    metrics::{
        index_retention_delete_chunk_timer,
        log_index_retention_scanned_document,
        log_retention_expired_index_entry,
    },
    persistence::{
        IndexRetentionProgress,
        IndexRetentionRequest,
        Persistence,
        RepeatablePersistence,
        TimestampRange,
    },
    persistence_helpers::{
        DocumentRevision,
        RevisionPair,
    },
    query::Order,
    try_chunks::TryChunksExt,
    types::{
        GenericIndexName,
        IndexId,
        RepeatableTimestamp,
        Timestamp,
    },
};

/// A persistence that stores index history as one row per revision, giving
/// [`delete_expired_entries`] a checked primitive to remove the rows it
/// derives are expired -- replacing an inherent method matched by name and
/// signature alone.
#[async_trait]
pub trait IndexRowPersistence: Persistence {
    async fn delete_index_rows(&self, entries: Vec<IndexEntry>) -> anyhow::Result<usize>;
}

/// Finds the index entries that expired between `cursor` and `min_snapshot_ts`
/// and deletes them from `p` in chunks.
pub async fn delete_expired_entries<P: IndexRowPersistence + ?Sized>(
    p: &P,
    request: IndexRetentionRequest<'_>,
) -> anyhow::Result<IndexRetentionProgress> {
    let IndexRetentionRequest {
        min_snapshot_ts,
        cursor,
        all_indexes,
        retention_validator,
    } = request;
    if *min_snapshot_ts == Timestamp::MIN {
        return Ok(IndexRetentionProgress {
            cursor,
            expired_entries: 0,
            deleted_rows: 0,
            unique_indexes: 0,
        });
    }
    // The number of rows we delete in persistence.
    let mut total_deleted_rows: usize = 0;
    // The number of expired entries we read from chunks.
    let mut total_expired_entries = 0;
    // The distinct indexes among the expired entries we've read.
    let mut touched_indexes = HashSet::new();
    let mut new_cursor = cursor;

    let snapshot_ts = min_snapshot_ts;
    let reader = RepeatablePersistence::new(p.reader(), snapshot_ts, retention_validator.clone());

    tracing::trace!("delete: about to grab chunks");
    let expired_chunks = expired_index_entries(reader, cursor, min_snapshot_ts, all_indexes)
        .try_chunks2(*INDEX_RETENTION_DELETE_CHUNK);
    pin_mut!(expired_chunks);
    while let Some(delete_chunk) = expired_chunks.try_next().await? {
        tracing::trace!(
            "delete: got a chunk and finished waiting {:?}",
            delete_chunk.len()
        );
        total_expired_entries += delete_chunk.len();
        touched_indexes.extend(delete_chunk.iter().map(|(_, entry)| entry.index_id));
        let results = try_join_all(
            partition_chunk(
                delete_chunk,
                INDEX_RETENTION_DELETE_CHUNK.div_ceil(*INDEX_RETENTION_DELETE_PARALLEL),
            )
            .into_iter()
            .map(|part| delete_expired_chunk(part, p, *new_cursor)),
        )
        .await?;
        let (chunk_new_cursors, deleted_rows): (Vec<_>, Vec<_>) = results.into_iter().unzip();
        // We have successfully deleted all of delete_chunk, so update
        // total_deleted_rows and new_cursor to reflect the deletions.
        total_deleted_rows += deleted_rows.into_iter().sum::<usize>();
        if let Some(max_new_cursor) = chunk_new_cursors.into_iter().max() {
            new_cursor = snapshot_ts.prior_ts(max_new_cursor)?;
        }
        if new_cursor > cursor && total_expired_entries > *RETENTION_DELETE_BATCH {
            tracing::debug!(
                "delete: returning early with {new_cursor:?}, total expired index entries read: \
                 {total_expired_entries:?}, total rows deleted: {total_deleted_rows:?}"
            );
            // we're not done deleting everything.
            return Ok(IndexRetentionProgress {
                cursor: new_cursor,
                expired_entries: total_expired_entries,
                deleted_rows: total_deleted_rows,
                unique_indexes: touched_indexes.len(),
            });
        }
    }
    tracing::debug!(
        "delete: finished loop, returning {:?}",
        min_snapshot_ts.pred()
    );
    Ok(IndexRetentionProgress {
        cursor: min_snapshot_ts.pred()?,
        expired_entries: total_expired_entries,
        deleted_rows: total_deleted_rows,
        unique_indexes: touched_indexes.len(),
    })
}

/// The index entries superseded between `cursor` and `min_snapshot_ts`,
/// paired with the timestamp that superseded them.
#[try_stream(ok = (Timestamp, IndexEntry), error = anyhow::Error)]
pub async fn expired_index_entries(
    reader: RepeatablePersistence,
    cursor: RepeatableTimestamp,
    min_snapshot_ts: RepeatableTimestamp,
    all_indexes: &BTreeMap<IndexId, (GenericIndexName<TabletId>, IndexedFields)>,
) {
    tracing::trace!(
        "expired_index_entries: reading expired index entries from {cursor:?} to {:?}",
        min_snapshot_ts,
    );
    let mut revs = reader.load_revision_pairs(
        None, /* tablet_id */
        TimestampRange::new(*cursor..*min_snapshot_ts),
        Order::Asc,
    );
    while let Some(rev) = revs.try_next().await? {
        // Prev revs are the documents we are deleting.
        // Each prev rev has 1 or 2 index entries to delete per index -- one entry at
        // the prev rev's ts, and a tombstone at the current rev's ts if
        // the document was deleted or its index key changed.
        let RevisionPair {
            id,
            rev:
                DocumentRevision {
                    ts,
                    document: maybe_doc,
                },
            prev_rev,
        } = rev;
        // If there is no prev rev, there's nothing to delete.
        // If this happens for a tombstone, it means the document was created and
        // deleted in the same transaction, with no index rows.
        let Some(prev_rev) = prev_rev else {
            log_index_retention_scanned_document(maybe_doc.is_none(), false);
            continue;
        };
        let DocumentRevision {
            ts: prev_rev_ts,
            document: Some(prev_rev),
        } = prev_rev
        else {
            // This is unexpected: if there is a prev_ts, there should be a prev_rev.
            let mut e = anyhow::anyhow!(
                "Skipping deleting indexes for {id}@{ts}. It has a prev_ts of {prev_ts} but no \
                 previous revision.",
                prev_ts = prev_rev.ts
            );
            report_error(&mut e).await;
            log_index_retention_scanned_document(maybe_doc.is_none(), false);
            continue;
        };
        log_index_retention_scanned_document(maybe_doc.is_none(), true);
        for (index_id, (_, index_fields)) in all_indexes
            .iter()
            .filter(|(_, (index, _))| *index.table() == id.table())
        {
            let index_key = prev_rev.index_key(index_fields).to_bytes();
            let key_sha256 = Sha256::hash(&index_key);
            let key = SplitKey::new(index_key.clone().0);
            log_retention_expired_index_entry(false, false);
            yield (
                ts,
                IndexEntry {
                    index_id: *index_id,
                    key_prefix: key.prefix.clone(),
                    key_suffix: key.suffix.clone(),
                    key_sha256: key_sha256.to_vec(),
                    ts: prev_rev_ts,
                    deleted: false,
                },
            );
            match maybe_doc.as_ref() {
                Some(doc) => {
                    let next_index_key = doc.index_key(index_fields).to_bytes();
                    if index_key == next_index_key {
                        continue;
                    }
                    log_retention_expired_index_entry(true, true);
                },
                None => log_retention_expired_index_entry(true, false),
            }
            yield (
                ts,
                IndexEntry {
                    index_id: *index_id,
                    key_prefix: key.prefix,
                    key_suffix: key.suffix,
                    key_sha256: key_sha256.to_vec(),
                    ts,
                    deleted: true,
                },
            );
        }
    }
}

/// Splits a chunk into roughly `target_len`-sized parts for concurrent
/// deletion. Entries sharing a primary key must stay in the same part:
/// `delete_index_rows` deletes all prior timestamps for a key, so
/// splitting them would make one part's delete silently remove rows another
/// part expected to count itself.
pub fn partition_chunk(
    mut to_partition: Vec<(Timestamp, IndexEntry)>,
    target_len: usize,
) -> Vec<Vec<(Timestamp, IndexEntry)>> {
    // Group by primary key so that nearby entries land in the same part.
    to_partition.sort_unstable_by(|a, b| {
        Ord::cmp(
            &(&a.1.index_id, &a.1.key_prefix, &a.1.key_sha256, &a.1.ts),
            &(&b.1.index_id, &b.1.key_prefix, &b.1.key_sha256, &b.1.ts),
        )
    });
    let mut parts = vec![vec![]];
    for chunk in to_partition.chunk_by(|a, b| {
        (&a.1.index_id, &a.1.key_prefix, &a.1.key_sha256)
            == (&b.1.index_id, &b.1.key_prefix, &b.1.key_sha256)
    }) {
        if parts.last().unwrap().len() >= target_len {
            parts.push(vec![]);
        }
        parts.last_mut().unwrap().extend_from_slice(chunk);
    }
    parts
}

#[fastrace::trace]
async fn delete_expired_chunk<P: IndexRowPersistence + ?Sized>(
    delete_chunk: Vec<(Timestamp, IndexEntry)>,
    p: &P,
    mut new_cursor: Timestamp,
) -> anyhow::Result<(Timestamp, usize)> {
    let _timer = index_retention_delete_chunk_timer();
    let index_entries_to_delete = delete_chunk.len();
    tracing::trace!("delete: got entries to delete {index_entries_to_delete:?}");
    for index_entry_to_delete in delete_chunk.iter() {
        // If we're deleting the previous revision of an index entry, we've definitely
        // deleted index entries for documents at all prior timestamps.
        if index_entry_to_delete.0 > Timestamp::MIN {
            new_cursor = cmp::max(new_cursor, index_entry_to_delete.0.pred()?);
        }
    }
    let deleted_rows = if index_entries_to_delete > 0 {
        p.delete_index_rows(delete_chunk.into_iter().map(|ind| ind.1).collect())
            .await?
    } else {
        0
    };

    // If there are more entries to delete than we see in the delete chunk,
    // it means retention skipped deleting entries before, and we
    // incorrectly bumped RetentionConfirmedDeletedTimestamp anyway.
    if deleted_rows > index_entries_to_delete {
        report_error(&mut anyhow::anyhow!(
            "retention wanted to delete {index_entries_to_delete} entries but found \
             {deleted_rows} to delete"
        ))
        .await;
    }

    tracing::trace!("delete: deleted {deleted_rows:?} rows");
    Ok((new_cursor, deleted_rows))
}
