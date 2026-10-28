//! `DataSyncIterator`: a streaming-export primitive that syncs many tables with
//! bounded reads/output per page and a small, self-contained cursor.
//!
//! It is generally preferable to `TableIterator` due to efficiency (only
//! iterating the document log once) as well as having a small cursor (no large
//! in-memory store).
//!
//! - `DataSyncIterator` can be used for continuous streaming export as well as
//!   to get a single snapshot.
//! - Unlike `TableIterator`, which selects the `snapshot_ts` ahead of time,
//!   `DataSyncIterator` can only select the `snapshot_ts` at the end of the
//!   sync.
//! - `DataSyncIterator` is designed to paginate with a small cursor.
//! - `DataSyncIterator` alternates iterating along `by_id` and advancing
//!   `synced_ts`.
//!
//! See
//! <https://app.notion.com/p/convex-dev/Robust-Streaming-Export-API-36db57ff32ab80c68d97e01c578518d4>
//!
//! # Guarantees
//!
//! - Transactions are not split across pages when advancing `synced_ts`
//! - Each page reads at most `max_rows_read` rows and targets `page_size_limit`
//!   emitted entries and `page_bytes_limit` bytes of emitted documents. May
//!   occasionally go over these limits in the presence of large transaction: a
//!   page's first transaction is always read in its entirety, plus the first
//!   row of the next one.
//! - Cursor + Page should be persisted atomically by caller
//!
//! # During Initial Sync phase
//!
//! - During initial sync, pages report [`DataSyncStatus::Snapshotting`]. During
//!   this phase, pages do not necessarily represent consistent snapshots.
//! - Once initial sync is complete, the final page reports a consistent
//!   snapshot ([`DataSyncStatus::Stale`] or [`DataSyncStatus::UpToDate`])
//! - Each document may be emitted more than once, at successive revisions.
//! - Each rev of a given document will be emitted in increasing timestamp
//!   order.
//!
//! # During Synced phase (CDC style)
//!
//! - The final emitted version of every captured document is the version as of
//!   `ts` (a consistent snapshot).
//! - The caller may continue iterating from [`DataSyncStatus::Stale`] or
//!   [`DataSyncStatus::UpToDate`] to continue a streaming sync to a newer
//!   consistent snapshot.
//! - Transactions are not split across pages.
//! - May switch back to Initial Sync phase if a large operation occurs (adding
//!   a table to the set of synced tables, or an `npx convex import` replacing a
//!   table). Removing a table just stops syncing it.
//!
//! # Algorithm
//!
//! A cursor `C` **captures** a document `(tablet, id)` (its latest version at
//! `ts <= C.synced_ts`) when `ts <= C.synced_ts` **and** either
//! `tablet` is in `C.synced_tables`, or `tablet == current_table` and
//! `id <= current_id` (the last id walked in the in-progress table).
//!
//! Each page runs one of two operations, chosen by a freshness heuristic:
//!
//! 1. **`by_id` dimension** — read a page of the `current_table`'s `by_id`
//!    index *at `synced_ts`* (requires `synced_ts` within index retention),
//!    emit each row, and advance `current_id`. On end-of-table, move
//!    `current_table` into `synced_tables` and pick the next target table (or
//!    finish → `Synced`). If the read falls out of index retention, the page
//!    runs along the `ts` dimension instead.
//! 2. **`ts` dimension** — walk the document log forward from `synced_ts`, emit
//!    captured documents at their new revision, and advance `synced_ts` to the
//!    last fully-consumed timestamp. Documents not yet captured are skipped
//!    (they will be read by `by_id` at the new `synced_ts`).
//!
//! ```text
//!     ◄══════════════════════════════ ID ════════════════════════════════=══►
//!  │
//!  │   ┌─────────┬─────────┐
//!  │   │   →1    │   →2    │   ← stable ts for ID iteration
//!  │   ├─────────┴─────────┤
//!  │   │                   │
//!  │   │        ↓3         │
//!  │   │                   ├─────────┬─────────┬─────────┐
//!  │   │                   │   →4    │   →5    │   →6    │
//!  │   ├───────────────────┴─────────┴─────────┴─────────┤
//!  │   │                                                 │
//! T│   │                       ↓7                        │
//! S│   │                                                 │
//!  │   ├─────────────────────────────────────────────────┤
//!  │   │                                                 │
//!  │   │                       ↓8                        │
//!  │   │                                                 ├─────────┬─────────┐
//!  │   │                                                 │   →9    │   →10   │
//!  │   ├─────────────────────────────────────────────────┴─────────┴─────────┤
//!  │   │                                                                     │
//!  │   │                                ↓11                                  │
//!  │   │                                                                     │
//!  │   ├─────────────────────────────────────────────────────────────────────┤
//!  │   │                                ↓12                                  │
//!  │   │       Once ID iteration completes, continue CDC style forever       │
//!  │   │                                                                     │
//!  │   └─────────────────────────────────────────────────────────────────────┘
//!  ▼
//! ```

use std::{
    cmp,
    collections::{
        BTreeMap,
        BTreeSet,
    },
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use common::{
    cover,
    document::ResolvedDocument,
    index::IndexKey,
    persistence::{
        new_static_repeatable_recent,
        DocumentLogEntry,
        DocumentPrevTsQuery,
        PersistenceReader,
        RepeatablePersistence,
        RetentionValidator,
        TimestampRange,
    },
    query::{
        CursorPosition,
        Order,
    },
    runtime::Runtime,
    types::{
        IndexRef,
        RepeatableTimestamp,
        Timestamp,
    },
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use futures::{
    pin_mut,
    TryStreamExt,
};
use value::{
    DeveloperDocumentId,
    InternalDocumentId,
    TabletId,
};

use crate::TableScanCursor;

/// Where a `DataSyncIterator` is in its traversal of the `by_id` (document ID)
/// dimension across the target tables.
#[derive(Clone, Debug, PartialEq, Eq)]
enum TableCursor {
    /// Every target table's ID space has been fully traversed. The view is a
    /// consistent snapshot at `synced_ts`.
    Synced,
    /// Mid-traversal of `current_table`. Documents in `current_table` with
    /// `id <= current_id` have been captured. `current_id == None` means no
    /// document in `current_table` has been walked yet.
    InProgress {
        current_table: TabletId,
        current_id: Option<DeveloperDocumentId>,
    },
}

/// A small, self-contained cursor that fully describes sync progress. It is
/// passed back into [`DataSyncIterator::next_page`] to continue, and is cheap
/// enough to serialize and persist between requests.
///
/// A cursor "captures" a document when its latest version at `ts <= synced_ts`
/// has been emitted — see the module docs for the exact predicate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataSyncCursor {
    /// All captured documents have a revision `<= synced_ts`.
    synced_ts: Timestamp,
    /// Tables whose entire ID space has been traversed at `synced_ts`.
    synced_tables: BTreeSet<TabletId>,
    /// Position within the `by_id` dimension.
    table_cursor: TableCursor,
    /// Documents emitted so far from the in-progress table's `by_id`
    /// traversal. Zero whenever `table_cursor` is `Synced` or a table's
    /// traversal hasn't started.
    current_table_docs_synced: u64,
    /// Documents (including tombstones and re-emitted revisions) emitted over
    /// the sync's lifetime.
    num_docs_synced: u64,
}

impl DataSyncCursor {
    /// The timestamp all captured documents have a revision `<=` of.
    pub fn synced_ts(&self) -> Timestamp {
        self.synced_ts
    }

    /// Tables whose entire ID space has been traversed at `synced_ts`.
    pub fn synced_tables(&self) -> &BTreeSet<TabletId> {
        &self.synced_tables
    }

    /// The table currently being traversed in the `by_id` dimension and the
    /// last id walked in it (`None` id means the traversal hasn't started), or
    /// `None` if every target table has been fully traversed (`Synced`).
    pub fn in_progress_table(&self) -> Option<(TabletId, Option<DeveloperDocumentId>)> {
        match &self.table_cursor {
            TableCursor::Synced => None,
            TableCursor::InProgress {
                current_table,
                current_id,
            } => Some((*current_table, *current_id)),
        }
    }

    /// Documents emitted so far from the in-progress table's `by_id`
    /// traversal.
    pub fn current_table_docs_synced(&self) -> u64 {
        self.current_table_docs_synced
    }

    /// Documents (including tombstones and re-emitted revisions) emitted over
    /// the sync's lifetime.
    pub fn num_docs_synced(&self) -> u64 {
        self.num_docs_synced
    }

    /// Reconstruct a cursor from its serialized parts. `in_progress` mirrors
    /// [`Self::in_progress_table`].
    pub fn from_parts(
        synced_ts: Timestamp,
        synced_tables: BTreeSet<TabletId>,
        in_progress: Option<(TabletId, Option<DeveloperDocumentId>)>,
        current_table_docs_synced: u64,
        num_docs_synced: u64,
    ) -> Self {
        let table_cursor = match in_progress {
            None => TableCursor::Synced,
            Some((current_table, current_id)) => TableCursor::InProgress {
                current_table,
                current_id,
            },
        };
        Self {
            synced_ts,
            synced_tables,
            table_cursor,
            current_table_docs_synced,
            num_docs_synced,
        }
    }
}

/// Progress indicator returned while a sync is still
/// [`DataSyncStatus::Snapshotting`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressStatus {
    pub num_tables_synced: u64,
    pub total_tables: u64,
    /// The table mid-traversal in the `by_id` dimension. An in-progress sync
    /// always has one: finishing a table either starts the next one or
    /// completes the sync ([`DataSyncStatus::Stale`] or
    /// [`DataSyncStatus::UpToDate`]).
    pub current_table: TabletId,
    /// Documents emitted so far from the current table's `by_id` traversal,
    /// across all pages of this sync.
    pub num_documents_in_current_table: u64,
    /// Documents (including tombstones and re-emitted revisions) emitted over
    /// the sync's lifetime, so this can exceed the number of documents in the
    /// target tables.
    pub num_documents_synced: u64,
}

/// The consistency state reported alongside a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataSyncStatus {
    /// More pages are required before the entries form a consistent snapshot.
    Snapshotting { progress: ProgressStatus },
    /// The entries emitted so far represent a consistent snapshot at `ts`, but
    /// `ts` is behind the latest timestamp. Note that the next page may be
    /// empty.
    Stale { ts: Timestamp },
    /// The entries emitted so far represent a consistent snapshot at `ts` that
    /// has read all the way to the latest commit; the caller can take a break
    /// before iterating again.
    UpToDate { ts: Timestamp },
}

/// A single emitted document revision, paired with its previous revision.
/// Produced by [`DataSyncIterator::next_page_with_prev_revs`].
pub struct DataSyncEntry {
    /// The emitted revision. `value: None` is a tombstone (delete); `prev_ts`
    /// is preserved for CDC/delete handling by consumers.
    pub log_entry: DocumentLogEntry,
    /// The document's previous revision, present *only* when this iterator
    /// already emitted that revision. Lets a consumer keep a running aggregate
    /// over documents (e.g. total file size) via deltas — add the new
    /// revision's contribution, subtract `prev_rev`'s — without materializing
    /// every document.
    ///
    /// `None` on `by_id` pages and on a document's first emission. `Some` when
    /// a `ts` page re-emits a *captured* document, whose predecessor was
    /// necessarily emitted first (on the `by_id` page that captured it, or — if
    /// created mid-sync — on an earlier `ts` page). Also `None` in the rare
    /// case the predecessor's value was garbage collected past retention.
    pub prev_rev: Option<ResolvedDocument>,
}

/// A single page of sync output. The entry type `E` is plain
/// [`DocumentLogEntry`] from [`DataSyncIterator::next_page`], or
/// [`DataSyncEntry`] from [`DataSyncIterator::next_page_with_prev_revs`].
pub struct DataSyncPage<E = DocumentLogEntry> {
    pub entries: Vec<E>,
    /// The cursor to pass to the next [`DataSyncIterator::next_page`] call.
    pub cursor: DataSyncCursor,
    pub status: DataSyncStatus,
}

pub struct DataSyncIterator<RT: Runtime> {
    runtime: RT,
    persistence: Arc<dyn PersistenceReader>,
    retention_validator: Arc<dyn RetentionValidator>,
    min_ts: RepeatableTimestamp,
    page_size_limit: usize,
    page_bytes_limit: usize,
    max_rows_read: usize,
    by_id_freshness: Duration,
}

impl<RT: Runtime> DataSyncIterator<RT> {
    /// `min_ts` is a floor on the snapshot each page reads at (see
    /// [`Self::latest_ts`]); pass a fresh timestamp like
    /// `Database::now_ts_for_reads` or a transaction's begin timestamp so the
    /// sync doesn't lag behind the committer's persisted repeatable timestamp.
    ///
    /// `page_size_limit` bounds entries emitted per page and `page_bytes_limit`
    /// bounds their total byte size (both soft: a single transaction is never
    /// split, so it may push a page over). `max_rows_read` is a soft limit on
    /// rows read from persistence per page. `by_id_freshness` is how close
    /// `synced_ts` must be to the latest timestamp before the iterator
    /// scans the `by_id` dimension (rather than catching up along `ts`).
    pub fn new(
        runtime: RT,
        persistence: Arc<dyn PersistenceReader>,
        retention_validator: Arc<dyn RetentionValidator>,
        min_ts: RepeatableTimestamp,
        page_size_limit: usize,
        page_bytes_limit: usize,
        max_rows_read: usize,
        by_id_freshness: Duration,
    ) -> anyhow::Result<Self> {
        // The `by_id` dimension reads up to `page_size_limit` rows in a single
        // page, so the row-read bound must be at least the page-size bound for
        // `by_id` pages to stay within `max_rows_read`.
        anyhow::ensure!(
            page_size_limit <= max_rows_read,
            "page_size_limit ({page_size_limit}) must be <= max_rows_read ({max_rows_read})"
        );
        Ok(Self {
            runtime,
            persistence,
            retention_validator,
            min_ts,
            page_size_limit,
            page_bytes_limit,
            max_rows_read,
            by_id_freshness,
        })
    }

    /// Produce the next page of the sync.
    ///
    /// `cursor: None` starts a fresh sync. `target_tables` is the desired set
    /// of tables to sync, mapping each tablet to its `by_id` index id; it
    /// is diffed against the cursor on every call so tables can be added or
    /// removed between pages.
    ///
    /// Never reads previous document revisions; consumers that need them for
    /// delta aggregation should use [`Self::next_page_with_prev_revs`].
    pub async fn next_page(
        &self,
        cursor: Option<DataSyncCursor>,
        target_tables: &BTreeMap<TabletId, IndexRef>,
    ) -> anyhow::Result<DataSyncPage> {
        let page = self.next_page_inner(cursor, target_tables, false).await?;
        Ok(DataSyncPage {
            entries: page.entries.into_iter().map(|e| e.log_entry).collect(),
            cursor: page.cursor,
            status: page.status,
        })
    }

    /// Like [`Self::next_page`], but pairs each entry with its previous
    /// revision (see [`DataSyncEntry::prev_rev`]), at the cost of one extra
    /// batched persistence read per `ts` page.
    pub async fn next_page_with_prev_revs(
        &self,
        cursor: Option<DataSyncCursor>,
        target_tables: &BTreeMap<TabletId, IndexRef>,
    ) -> anyhow::Result<DataSyncPage<DataSyncEntry>> {
        self.next_page_inner(cursor, target_tables, true).await
    }

    /// The timestamp each page reads at: the max of `min_ts` and the
    /// committer's persisted repeatable timestamp, which lags live writes by a
    /// few seconds. It bounds reads and feeds the freshness heuristic.
    ///
    /// The max satisfies every constraint the iterator needs:
    ///
    /// 1. Repeatable, since the max of two repeatable timestamps is repeatable.
    /// 2. Weakly monotonically increasing across pages, since `min_ts` is fixed
    ///    and the persisted repeatable timestamp only increases — so any
    ///    `synced_ts` produced by a prior page stays `<=` it. Callers that
    ///    build a fresh iterator per page (e.g. `/data/sync`, once per request)
    ///    also need each iterator's `min_ts` to be at least the previous one's.
    /// 3. Within retention, since the persisted repeatable timestamp is and the
    ///    max is at least as recent.
    async fn latest_ts(&self) -> anyhow::Result<RepeatableTimestamp> {
        Ok(cmp::max(
            self.min_ts,
            new_static_repeatable_recent(self.persistence.as_ref()).await?,
        ))
    }

    async fn next_page_inner(
        &self,
        cursor: Option<DataSyncCursor>,
        target_tables: &BTreeMap<TabletId, IndexRef>,
        include_prev_revs: bool,
    ) -> anyhow::Result<DataSyncPage<DataSyncEntry>> {
        self.runtime
            .pause_client()
            .wait("data_sync_before_page")
            .await;

        let latest = self.latest_ts().await?;

        // Cold start, or reconcile an existing cursor against `target_tables`.
        let mut cursor = match cursor {
            None => {
                cover!(coverage::COLD_START);
                DataSyncCursor {
                    synced_ts: *latest,
                    synced_tables: BTreeSet::new(),
                    table_cursor: advance_to_next_table(&BTreeSet::new(), target_tables),
                    current_table_docs_synced: 0,
                    num_docs_synced: 0,
                }
            },
            Some(mut cursor) => {
                reconcile_target_tables(&mut cursor, target_tables);
                cursor
            },
        };

        anyhow::ensure!(
            cursor.synced_ts <= *latest,
            ErrorMetadata::bad_request(
                "InvalidDataSyncCursor",
                "data sync cursor is ahead of the deployment's latest timestamp",
            )
        );

        let use_by_id = match &cursor.table_cursor {
            // Nothing left to traverse in the ID dimension; only the `ts`
            // dimension can make progress (or hold us at a consistent snapshot).
            TableCursor::Synced => {
                cover!(coverage::TS_SELECTED_SYNCED);
                false
            },
            TableCursor::InProgress { .. } => {
                // `synced_ts <= latest` is enforced above, so this subtraction is safe.
                let lag = *latest - cursor.synced_ts;
                // N.B.: this heuristic doesn't guarantee that we're within
                // index retention, since `latest` doesn't advance on idle
                // deployments.
                let fresh = lag < self.by_id_freshness;
                if !fresh {
                    cover!(coverage::TS_SELECTED_LAG);
                }
                fresh
            },
        };

        let (entries, status) = if use_by_id {
            let saved_cursor = cursor.clone();
            match self.by_id_page(&mut cursor, latest, target_tables).await {
                Ok(result) => result,
                // As noted above, we can be out of index retention even if
                // "fresh", so fall back if necessary
                Err(e) if e.is_out_of_retention() => {
                    cursor = saved_cursor;
                    cover!(coverage::BY_ID_OUT_OF_RETENTION);
                    tracing::warn!(
                        "data sync by_id page at {} fell out of index retention, falling back to \
                         the ts dimension: {e:#}",
                        cursor.synced_ts
                    );
                    self.ts_page(&mut cursor, latest, target_tables, include_prev_revs)
                        .await?
                },
                Err(e) => return Err(e),
            }
        } else {
            self.ts_page(&mut cursor, latest, target_tables, include_prev_revs)
                .await?
        };

        Ok(DataSyncPage {
            entries,
            cursor,
            status,
        })
    }

    /// Advance the `by_id` dimension: read a page of `current_table`'s `by_id`
    /// index at `synced_ts` and emit each document. `synced_ts` is unchanged.
    async fn by_id_page(
        &self,
        cursor: &mut DataSyncCursor,
        latest: RepeatableTimestamp,
        target_tables: &BTreeMap<TabletId, IndexRef>,
    ) -> anyhow::Result<(Vec<DataSyncEntry>, DataSyncStatus)> {
        let TableCursor::InProgress {
            current_table,
            current_id,
        } = cursor.table_cursor.clone()
        else {
            anyhow::bail!("by_id_page called while Synced");
        };
        cover!(coverage::BY_ID_PAGE);
        let by_id = *target_tables
            .get(&current_table)
            .context("current_table missing from target_tables")?;

        // `synced_ts` was `<= latest` when written and `latest` only increases,
        // so it remains repeatable. `read_snapshot` only checks retention
        // optimistically; the index scan validates it after reading.
        let synced_ts = latest.prior_ts(cursor.synced_ts)?;
        let repeatable_persistence = RepeatablePersistence::new(
            self.persistence.clone(),
            synced_ts,
            self.retention_validator.clone(),
        );
        let snapshot = repeatable_persistence.read_snapshot(synced_ts)?;

        let scan_cursor = TableScanCursor {
            index_key: current_id
                .map(|id| CursorPosition::After(IndexKey::new(vec![], id).to_bytes())),
        };
        let mut stream = snapshot.index_scan(
            by_id,
            current_table,
            &scan_cursor.interval(),
            Order::Asc,
            self.page_size_limit,
        );
        let mut count_limited = false;

        let mut entries = Vec::with_capacity(self.page_size_limit);
        let mut new_current_id = current_id;
        let mut page_bytes = 0usize;
        let mut bytes_limited = false;
        while let Some((_key, latest_doc)) = stream.try_next().await? {
            let value = latest_doc.value;
            page_bytes += value.size();
            let id = value.id_with_table_id();
            new_current_id = Some(value.id().developer_id);
            // Initial capture of this document, so it has no previously-emitted
            // revision.
            entries.push(DataSyncEntry {
                log_entry: DocumentLogEntry {
                    ts: latest_doc.ts,
                    id,
                    value: Some(value),
                    prev_ts: latest_doc.prev_ts,
                },
                prev_rev: None,
            });
            // Soft byte limit: stop once the page is large enough. We push before
            // checking, so a single document larger than the limit is still
            // emitted (and `current_id` advances past it).
            if page_bytes >= self.page_bytes_limit {
                bytes_limited = true;
                break;
            }
            if entries.len() >= self.page_size_limit {
                count_limited = true;
                break;
            }
        }

        if entries.is_empty() {
            cover!(coverage::BY_ID_EMPTY_PAGE);
        }

        cursor.num_docs_synced = cursor.num_docs_synced.saturating_add(entries.len() as u64);
        cursor.current_table_docs_synced = cursor
            .current_table_docs_synced
            .saturating_add(entries.len() as u64);

        // The table is exhausted only if we emitted the whole fetched page and it
        // wasn't a full page. If either limit stopped us, there is more to read.
        if count_limited {
            cover!(coverage::BY_ID_COUNT_LIMITED);
        }
        if bytes_limited {
            cover!(coverage::BY_ID_BYTES_LIMITED);
        }
        let reached_end = !count_limited && !bytes_limited;
        if reached_end {
            cover!(coverage::BY_ID_REACHED_END);
            cursor.synced_tables.insert(current_table);
            cursor.table_cursor = advance_to_next_table(&cursor.synced_tables, target_tables);
            cursor.current_table_docs_synced = 0;
            if matches!(cursor.table_cursor, TableCursor::Synced) {
                cover!(coverage::SYNC_COMPLETE);
            }
        } else {
            cursor.table_cursor = TableCursor::InProgress {
                current_table,
                current_id: new_current_id,
            };
        }

        let status = status(cursor, *latest, target_tables.len() as u64);
        Ok((entries, status))
    }

    /// Advance the `ts` dimension: walk the document log forward from
    /// `synced_ts`, emit captured documents at their new revision, and advance
    /// `synced_ts` to the last fully-consumed timestamp.
    async fn ts_page(
        &self,
        cursor: &mut DataSyncCursor,
        latest: RepeatableTimestamp,
        target_tables: &BTreeMap<TabletId, IndexRef>,
        include_prev_revs: bool,
    ) -> anyhow::Result<(Vec<DataSyncEntry>, DataSyncStatus)> {
        cover!(coverage::TS_PAGE);
        let repeatable_persistence = RepeatablePersistence::new(
            self.persistence.clone(),
            latest,
            self.retention_validator.clone(),
        );

        let mut entries = Vec::new();
        // Default: if the stream is exhausted without hitting a limit, we have
        // caught up to `latest`.
        let mut new_synced_ts = *latest;

        if let Some(start) = cursor.synced_ts.succ_opt()
            && start <= *latest
        {
            let range = TimestampRange::new(start..=*latest);
            let single_table = if target_tables.len() == 1 {
                target_tables.first_key_value().map(|(tablet, _)| tablet)
            } else {
                None
            };
            let stream = match single_table {
                Some(&tablet) => {
                    cover!(coverage::TS_SINGLE_TABLE_FILTER);
                    repeatable_persistence.load_documents_from_table(tablet, range, Order::Asc)
                },
                None => {
                    cover!(coverage::TS_MULTI_TABLE_SCAN);
                    repeatable_persistence.load_documents(range, Order::Asc)
                },
            };
            pin_mut!(stream);

            let mut rows_read = 0usize;
            // The timestamp of the batch we are currently accumulating, the
            // captured entries within it, and their byte size. We only commit a
            // timestamp once it is fully read (a later entry has a strictly
            // greater `ts`, or the stream ends), so a partially-read commit never
            // advances `synced_ts`.
            let mut cur_ts: Option<Timestamp> = None;
            let mut cur_batch: Vec<DocumentLogEntry> = Vec::new();
            let mut cur_batch_bytes = 0usize;
            // Total byte size of the entries committed so far this page.
            let mut page_bytes = 0usize;
            // Whether we've committed at least one complete timestamp this page.
            // Once true, a later transaction that crosses `max_rows_read` is cut
            // short; while false, the first transaction is allowed to exceed the
            // limit on its own (we must read it in full to make progress).
            let mut committed_any = false;
            let mut hit_limit = false;

            while let Some(entry) = stream.try_next().await? {
                if Some(entry.ts) != cur_ts {
                    // The previous timestamp is now fully read. Commit it.
                    if let Some(committed_ts) = cur_ts {
                        entries.append(&mut cur_batch);
                        page_bytes += cur_batch_bytes;
                        cur_batch_bytes = 0;
                        new_synced_ts = committed_ts;
                        committed_any = true;
                        // `page_size_limit`/`page_bytes_limit` are soft, checked
                        // only here so a transaction is never split across pages.
                        let over_rows = rows_read >= self.max_rows_read;
                        let over_size = entries.len() >= self.page_size_limit;
                        let over_bytes = page_bytes >= self.page_bytes_limit;
                        if over_rows || over_size || over_bytes {
                            if over_rows {
                                cover!(coverage::TS_LIMIT_ROWS);
                            }
                            if over_size {
                                cover!(coverage::TS_LIMIT_PAGE_SIZE);
                            }
                            if over_bytes {
                                cover!(coverage::TS_LIMIT_PAGE_BYTES);
                            }
                            // Stop on this complete-timestamp boundary. `entry`
                            // (the first row of the next timestamp) is dropped
                            // and re-read on the next page.
                            hit_limit = true;
                            break;
                        }
                    }
                    cur_ts = Some(entry.ts);
                }
                rows_read += 1;
                if is_captured(&entry.id, cursor) {
                    cover!(coverage::TS_CAPTURED_EMITTED);
                    cur_batch_bytes += entry.value.as_ref().map_or(0, |doc| doc.size());
                    cur_batch.push(entry);
                } else {
                    cover!(coverage::TS_SKIPPED_UNCAPTURED);
                }
                // Enforce `max_rows_read` mid-transaction: if reading this
                // transaction has taken us over the limit, cut the page short at
                // the previous complete timestamp, dropping this partial
                // transaction (re-read next page). The exception is a single
                // transaction larger than `max_rows_read`: with nothing yet
                // committed this page we must read it in full.
                if rows_read >= self.max_rows_read && committed_any {
                    cover!(coverage::TS_MIDTXN_CUT);
                    hit_limit = true;
                    break;
                }
                if rows_read > self.max_rows_read {
                    // Reaching here implies `!committed_any`, and strictly
                    // exceeding the budget means we read a row *past* it: a single
                    // transaction larger than `max_rows_read`, read in full. (The
                    // exactly-at-budget case is covered by `TS_LIMIT_ROWS`.)
                    cover!(coverage::TS_LARGE_TXN_OVERRUN);
                }
            }
            if !hit_limit {
                // Stream exhausted: the final batch is complete and we are
                // caught up all the way to `latest`.
                cover!(coverage::TS_CAUGHT_UP);
                entries.append(&mut cur_batch);
                new_synced_ts = *latest;
            }
        } else {
            // `synced_ts` is already at `latest`; nothing to scan this page.
            cover!(coverage::TS_NOOP);
        }

        cursor.synced_ts = new_synced_ts;
        cursor.num_docs_synced = cursor.num_docs_synced.saturating_add(entries.len() as u64);
        // Every re-emitted document is captured, so its predecessor was already
        // emitted by this iterator — attach it (when requested) so consumers
        // can compute deltas.
        let entries = if include_prev_revs {
            self.attach_prev_revs(&repeatable_persistence, entries)
                .await?
        } else {
            entries
                .into_iter()
                .map(|log_entry| DataSyncEntry {
                    log_entry,
                    prev_rev: None,
                })
                .collect()
        };
        let status = status(cursor, *latest, target_tables.len() as u64);
        Ok((entries, status))
    }

    /// Batch-fetch, for each emitted entry, the document revision at its
    /// `prev_ts` and pair it up as a [`DataSyncEntry`]. On a `ts` page every
    /// entry is a captured document, so a `Some(prev_ts)` points to a revision
    /// this iterator already emitted (see [`DataSyncEntry::prev_rev`]). One
    /// batched persistence lookup per page keeps this cheap.
    async fn attach_prev_revs(
        &self,
        repeatable_persistence: &RepeatablePersistence,
        entries: Vec<DocumentLogEntry>,
    ) -> anyhow::Result<Vec<DataSyncEntry>> {
        let queries: BTreeSet<_> = entries
            .iter()
            .filter_map(|entry| {
                entry.prev_ts.map(|prev_ts| DocumentPrevTsQuery {
                    id: entry.id,
                    ts: entry.ts,
                    prev_ts,
                })
            })
            .collect();
        let mut prev_revs = if queries.is_empty() {
            BTreeMap::new()
        } else {
            repeatable_persistence
                .previous_revisions_of_documents(queries)
                .await?
        };
        Ok(entries
            .into_iter()
            .map(|log_entry| {
                let prev_rev = log_entry.prev_ts.and_then(|prev_ts| {
                    prev_revs
                        .remove(&DocumentPrevTsQuery {
                            id: log_entry.id,
                            ts: log_entry.ts,
                            prev_ts,
                        })
                        .and_then(|entry| entry.value)
                });
                DataSyncEntry {
                    log_entry,
                    prev_rev,
                }
            })
            .collect())
    }
}

fn status(cursor: &DataSyncCursor, latest: Timestamp, total_tables: u64) -> DataSyncStatus {
    match &cursor.table_cursor {
        // `synced_ts <= latest` always holds. Even if `cursor.synced_ts <
        // latest`, there might be no commits to read, as `latest` also advances
        // when the committer bumps the max repeatable timestamp.
        TableCursor::Synced if cursor.synced_ts < latest => DataSyncStatus::Stale {
            ts: cursor.synced_ts,
        },
        TableCursor::Synced => DataSyncStatus::UpToDate {
            ts: cursor.synced_ts,
        },
        TableCursor::InProgress { current_table, .. } => DataSyncStatus::Snapshotting {
            progress: ProgressStatus {
                num_tables_synced: cursor.synced_tables.len() as u64,
                total_tables,
                current_table: *current_table,
                num_documents_in_current_table: cursor.current_table_docs_synced,
                num_documents_synced: cursor.num_docs_synced,
            },
        },
    }
}

/// Whether `cursor` is responsible for `id` — i.e. its position in the `by_id`
/// dimension has already been walked, so the sync has committed to emitting it
/// and must replay any later revisions to keep it current.
fn is_captured(id: &InternalDocumentId, cursor: &DataSyncCursor) -> bool {
    let tablet = id.table();
    if cursor.synced_tables.contains(&tablet) {
        cover!(coverage::CAPTURED_VIA_SYNCED_TABLE);
        return true;
    }
    match &cursor.table_cursor {
        TableCursor::InProgress {
            current_table,
            current_id: Some(current_id),
        } if *current_table == tablet => {
            // Within a tablet the `by_id` order matches `InternalId` order, so
            // comparing internal ids is equivalent to comparing index keys.
            let captured = id.internal_id() <= current_id.internal_id();
            if captured {
                cover!(coverage::CAPTURED_VIA_CURRENT_TABLE);
            }
            captured
        },
        _ => false,
    }
}

/// Pick the next target table that hasn't been fully synced, or `Synced` if all
/// targets are done.
fn advance_to_next_table(
    synced_tables: &BTreeSet<TabletId>,
    target_tables: &BTreeMap<TabletId, IndexRef>,
) -> TableCursor {
    match target_tables
        .keys()
        .find(|tablet| !synced_tables.contains(tablet))
    {
        Some(tablet) => {
            // At least one other table is already fully synced, so this is a
            // multi-table sync advancing to its next table.
            if !synced_tables.is_empty() {
                cover!(coverage::MULTI_TABLE_ADVANCE);
            }
            TableCursor::InProgress {
                current_table: *tablet,
                current_id: None,
            }
        },
        None => TableCursor::Synced,
    }
}

/// Reconcile a cursor against the current target set: drop synced tables that
/// are no longer targeted, cancel an in-progress table that was removed, and
/// start a newly-added table if we were otherwise `Synced`.
fn reconcile_target_tables(
    cursor: &mut DataSyncCursor,
    target_tables: &BTreeMap<TabletId, IndexRef>,
) {
    let synced_before = cursor.synced_tables.len();
    cursor
        .synced_tables
        .retain(|tablet| target_tables.contains_key(tablet));
    if cursor.synced_tables.len() != synced_before {
        cover!(coverage::RECONCILE_DROP_SYNCED_TABLE);
    }

    match &cursor.table_cursor {
        TableCursor::InProgress { current_table, .. }
            if !target_tables.contains_key(current_table) =>
        {
            // The in-progress table was removed; cancel it and move on.
            cover!(coverage::RECONCILE_CANCEL_IN_PROGRESS);
            cursor.table_cursor = advance_to_next_table(&cursor.synced_tables, target_tables);
            cursor.current_table_docs_synced = 0;
        },
        TableCursor::Synced => {
            // A new table may have been added since we last reached Synced.
            let next = advance_to_next_table(&cursor.synced_tables, target_tables);
            if matches!(next, TableCursor::InProgress { .. }) {
                cover!(coverage::RECONCILE_START_ADDED_TABLE);
            }
            cursor.table_cursor = next;
            cursor.current_table_docs_synced = 0;
        },
        TableCursor::InProgress { .. } => {},
    }
}
