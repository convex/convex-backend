use std::{
    collections::BTreeMap,
    time::Duration,
};

use ::search::metrics::{
    SearchType,
    SEARCH_TYPE_LABEL,
};
use common::{
    identity::IDENTITY_LABEL,
    persistence::PersistenceIndexEntry,
    runtime::Runtime,
    types::{
        IndexWriteMode,
        Timestamp,
    },
};
use errors::ErrorMetadata;
use keybroker::Identity;
use metrics::{
    log_counter,
    log_counter_with_labels,
    log_distribution,
    log_distribution_with_labels,
    register_convex_counter,
    register_convex_gauge,
    register_convex_histogram,
    register_convex_int_gauge,
    IntoLabel,
    StaticMetricLabel,
    StatusTimer,
    Subgauge,
    Timer,
    STATUS_LABEL,
};
use prometheus::{
    VMHistogram,
    VMHistogramVec,
};

use crate::{
    transaction::FinalTransaction,
    IndexRetentionSource,
    RetentionType,
    Transaction,
};

register_convex_histogram!(
    DOCUMENTS_SIZE_BYTES,
    "Total size of documents in user tables in bytes"
);
pub fn log_user_table_documents_size(total_size: u64) {
    log_distribution(&DOCUMENTS_SIZE_BYTES, total_size as f64);
}

register_convex_int_gauge!(
    USER_DOCUMENTS_SIZE_BYTES,
    "Total size of user documents including virtual tables in bytes"
);
pub fn user_documents_size_subgauge() -> Subgauge {
    Subgauge::new(USER_DOCUMENTS_SIZE_BYTES.clone())
}

register_convex_int_gauge!(
    COMMITTER_CONCURRENT_COMMITS,
    "Number of commits the committer has admitted but not yet published: awaiting pre-validation, \
     pre-validating, or writing to persistence. This is the quantity compared against \
     COMMITTER_MAX_CONCURRENT_COMMITS to pause admission. An in-flight max_repeatable_ts bump \
     also counts as one"
);
pub fn concurrent_commits_subgauge() -> Subgauge {
    Subgauge::new(COMMITTER_CONCURRENT_COMMITS.clone())
}

register_convex_int_gauge!(
    COMMITTER_AWAITING_PRE_VALIDATION_COMMITS,
    "Number of admitted commits waiting for a free pre-validation lane"
);
pub fn awaiting_pre_validation_commits_subgauge() -> Subgauge {
    Subgauge::new(COMMITTER_AWAITING_PRE_VALIDATION_COMMITS.clone())
}

register_convex_int_gauge!(
    COMMITTER_PRE_VALIDATING_COMMITS,
    "Number of commits in in-flight pre-validation batches"
);
pub fn pre_validating_commits_subgauge() -> Subgauge {
    Subgauge::new(COMMITTER_PRE_VALIDATING_COMMITS.clone())
}

register_convex_histogram!(
    DATABASE_COMMIT_ADMISSION_PAUSE_SECONDS,
    "How long the committer went without accepting new messages because commits in flight were at \
     COMMITTER_MAX_CONCURRENT_COMMITS. One observation per contiguous pause"
);
pub fn log_commit_admission_pause(pause: Duration) {
    log_distribution(
        &DATABASE_COMMIT_ADMISSION_PAUSE_SECONDS,
        pause.as_secs_f64(),
    );
}

register_convex_histogram!(
    DATABASE_COMMIT_VALIDATION_WINDOW_SECONDS,
    "Width of the (validated_through, commit_ts] range the committer conflict-checks on its own \
     thread. Pre-validation exists to keep this narrow, so the full (begin_ts, commit_ts] width \
     shows up here whenever pre-validation could not narrow the range"
);
pub fn log_commit_validation_window(window_seconds: f64) {
    log_distribution(&DATABASE_COMMIT_VALIDATION_WINDOW_SECONDS, window_seconds);
}

register_convex_histogram!(DOCUMENTS_KEYS_TOTAL, "Total number of document keys");
pub fn log_num_keys(num_keys: u64) {
    log_distribution(&DOCUMENTS_KEYS_TOTAL, num_keys as f64);
}

register_convex_histogram!(
    INDEXES_TO_BACKFILL_TOTAL,
    "Number of indexes needing backfill"
);
pub fn log_num_indexes_to_backfill(num_indexes: usize) {
    log_distribution(&INDEXES_TO_BACKFILL_TOTAL, num_indexes as f64);
}

register_convex_counter!(INDEXES_BACKFILLED_TOTAL, "Number of indexes backfilled");
pub fn log_index_backfilled() {
    log_counter(&INDEXES_BACKFILLED_TOTAL, 1);
}

register_convex_histogram!(
    DB_INDEX_BACKFILL_SECONDS,
    "Time for database indexes to backfill",
    &STATUS_LABEL
);

register_convex_histogram!(
    TABLET_DB_INDEX_BACKFILL_SECONDS,
    "Time for database indexes to backfill",
);
pub fn tablet_index_backfill_timer() -> Timer<VMHistogram> {
    Timer::new(&TABLET_DB_INDEX_BACKFILL_SECONDS)
}

register_convex_histogram!(
    DATABASE_WRITE_TX_READ_INTERVALS_TOTAL,
    "Number of read intervals in a write transaction"
);

register_convex_histogram!(
    DATABASE_WRITE_TX_WRITES_TOTAL,
    "Total size of writes in a write transaction"
);
register_convex_histogram!(
    DATABASE_WRITE_TX_NUM_WRITES_TOTAL,
    "Total number of writes in a write transaction"
);

register_convex_histogram!(
    DATABASE_USER_WRITE_TX_WRITES_TOTAL,
    "Size of writes to a user table in a transaction"
);

register_convex_histogram!(
    DATABASE_USER_WRITE_TX_NUM_WRITES_TOTAL,
    "Number of writes to a user table in a transaction"
);

register_convex_histogram!(
    DATABASE_SYSTEM_WRITE_TX_WRITES_TOTAL,
    "Size of writes to a system table in a transaction"
);

register_convex_histogram!(
    DATABASE_SYSTEM_WRITE_TX_NUM_WRITES_TOTAL,
    "Number of writes to a system table in a transaction"
);

pub fn log_write_tx(tx: &FinalTransaction) {
    log_distribution(
        &DATABASE_WRITE_TX_READ_INTERVALS_TOTAL,
        tx.reads.num_intervals() as f64,
    );

    let user_size = tx.writes.user_size();
    let system_size = tx.writes.system_size();

    // Combined
    log_distribution(
        &DATABASE_WRITE_TX_WRITES_TOTAL,
        (user_size.size + system_size.size) as f64,
    );
    log_distribution(
        &DATABASE_WRITE_TX_NUM_WRITES_TOTAL,
        (user_size.num_writes + system_size.num_writes) as f64,
    );

    // User tables
    log_distribution(&DATABASE_USER_WRITE_TX_WRITES_TOTAL, user_size.size as f64);
    log_distribution(
        &DATABASE_USER_WRITE_TX_NUM_WRITES_TOTAL,
        user_size.num_writes as f64,
    );

    // System tables
    log_distribution(
        &DATABASE_SYSTEM_WRITE_TX_WRITES_TOTAL,
        system_size.size as f64,
    );
    log_distribution(
        &DATABASE_SYSTEM_WRITE_TX_NUM_WRITES_TOTAL,
        system_size.num_writes as f64,
    );
}

register_convex_histogram!(
    DATABASE_READ_TX_READ_INTERVALS_TOTAL,
    "Number of read intervals in a read transaction"
);
pub fn log_read_tx<RT: Runtime>(tx: &Transaction<RT>) {
    log_distribution(
        &DATABASE_READ_TX_READ_INTERVALS_TOTAL,
        tx.reads.num_intervals() as f64,
    );
}

register_convex_histogram!(
    DATABASE_SUBSCRIPTION_SECONDS,
    "Duration of a database subscription"
);
pub fn subscription_timer() -> Timer<VMHistogram> {
    Timer::new(&DATABASE_SUBSCRIPTION_SECONDS)
}

register_convex_histogram!(
    DATABASE_SUBSCRIPTION_INVALIDATION_LAG_SECONDS,
    "Time between the commit ts of the invalidating write and the ts the invalidation was sent",
);
register_convex_counter!(
    DATABASE_SUBSCRIPTION_INVALIDATION_UNKNOWN_TOTAL,
    "Count of query subscriptions invalidated where the correspoding invalidating write timestamp \
     was unknown",
);
pub fn log_subscription_invalidation_lag(invalid_ts: Option<Timestamp>, current_ts: Timestamp) {
    if let Some(invalid_ts) = invalid_ts {
        log_distribution(
            &DATABASE_SUBSCRIPTION_INVALIDATION_LAG_SECONDS,
            current_ts.secs_since_f64(invalid_ts),
        );
    } else {
        log_counter(&DATABASE_SUBSCRIPTION_INVALIDATION_UNKNOWN_TOTAL, 1);
    }
}

register_convex_histogram!(
    DATABASE_REFRESH_TOKEN_SECONDS,
    "time taken to refresh a database token"
);
pub fn refresh_token_timer() -> Timer<VMHistogram> {
    Timer::new(&DATABASE_REFRESH_TOKEN_SECONDS)
}

register_convex_histogram!(
    DATABASE_BOOTSTRAP_SHAPES_SECONDS,
    "Time taken to bootstrap shapes"
);
pub fn bootstrap_table_summaries_timer() -> Timer<VMHistogram> {
    Timer::new(&DATABASE_BOOTSTRAP_SHAPES_SECONDS)
}

register_convex_histogram!(
    TABLE_SUMMARY_UPDATE_SECONDS,
    "Time taken to update the table summary (shape inference + counts) for a single document \
     update on the commit path"
);
pub fn table_summary_update_timer() -> Timer<VMHistogram> {
    Timer::new(&TABLE_SUMMARY_UPDATE_SECONDS)
}

register_convex_histogram!(DATABASE_LOAD_SECONDS, "Time to load the database");
pub fn load_database_timer() -> Timer<VMHistogram> {
    Timer::new(&DATABASE_LOAD_SECONDS)
}

register_convex_histogram!(
    DB_SNAPSHOT_TABLE_AND_INDEX_METADATA_LOAD_SECONDS,
    "Time to load table and index metadata"
);
pub fn load_table_and_index_metadata_timer() -> Timer<VMHistogram> {
    Timer::new(&DB_SNAPSHOT_TABLE_AND_INDEX_METADATA_LOAD_SECONDS)
}

register_convex_histogram!(
    DB_SNAPSHOT_LOAD_INDEXES_INTO_MEMORY_SECONDS,
    "Time to load indexes into memory"
);
pub fn load_indexes_into_memory_timer() -> Timer<VMHistogram> {
    Timer::new(&DB_SNAPSHOT_LOAD_INDEXES_INTO_MEMORY_SECONDS)
}

register_convex_histogram!(
    DB_SNAPSHOT_BOOTSTRAP_TABLE_REGISTRY_SECONDS,
    "Time to bootstrap table registry"
);
pub fn bootstrap_table_registry_timer() -> Timer<VMHistogram> {
    Timer::new(&DB_SNAPSHOT_BOOTSTRAP_TABLE_REGISTRY_SECONDS)
}

register_convex_histogram!(
    DB_SNAPSHOT_VERIFY_INVARIANTS_SECONDS,
    "Time to verify invariants when loading a DatabaseSnapshot"
);
pub fn verify_invariants_timer() -> Timer<VMHistogram> {
    Timer::new(&DB_SNAPSHOT_VERIFY_INVARIANTS_SECONDS)
}

register_convex_histogram!(
    DATABASE_COMMIT_CLIENT_SECONDS,
    "Time taken to submit a commit",
    &[IDENTITY_LABEL],
);
/// Includes time waiting in queue for the committer thread.
pub fn commit_client_timer(identity: &Identity) -> Timer<VMHistogramVec> {
    let mut timer = Timer::new_with_labels(&DATABASE_COMMIT_CLIENT_SECONDS);
    timer.add_label(identity.tag());
    timer
}

register_convex_histogram!(
    DATABASE_COMMIT_QUEUE_SECONDS,
    "Time from enqueueing a commit to it reaching the committer thread: the committer queue wait, \
     plus the wait for a pre-validation lane and the off-thread conflict check itself"
);
pub fn log_commit_dequeued(time_in_queue: Duration) {
    log_distribution(&DATABASE_COMMIT_QUEUE_SECONDS, time_in_queue.as_secs_f64());
}

register_convex_histogram!(
    DATABASE_COMMIT_SECONDS,
    "Time taken for a database commit",
    &STATUS_LABEL
);
pub fn commit_timer() -> StatusTimer {
    StatusTimer::new(&DATABASE_COMMIT_SECONDS)
}

register_convex_histogram!(
    DATABASE_COMMIT_PERSISTED_SECONDS,
    "Time from a commit's submission until persistence acknowledges its write. Excludes time \
     waiting for the committed timestamp to become repeatable."
);
pub fn log_commit_persisted(elapsed: Duration) {
    log_distribution(&DATABASE_COMMIT_PERSISTED_SECONDS, elapsed.as_secs_f64());
}

register_convex_histogram!(
    DATABASE_COMMIT_ID_REUSE_SECONDS,
    "Time to check if IDs have been reused",
    &STATUS_LABEL
);
pub fn commit_id_reuse_timer() -> StatusTimer {
    StatusTimer::new(&DATABASE_COMMIT_ID_REUSE_SECONDS)
}

register_convex_histogram!(
    DATABASE_COMMIT_IS_STALE_SECONDS,
    "Time to check if a commit is stale",
    &STATUS_LABEL
);
pub fn commit_is_stale_timer() -> StatusTimer {
    StatusTimer::new(&DATABASE_COMMIT_IS_STALE_SECONDS)
}

register_convex_histogram!(
    DATABASE_COMMIT_PRE_VALIDATE_SECONDS,
    "Time spent conflict-checking a batch of staged commits against the write log, off the \
     committer thread"
);
pub fn commit_pre_validate_timer() -> Timer<VMHistogram> {
    Timer::new(&DATABASE_COMMIT_PRE_VALIDATE_SECONDS)
}

register_convex_histogram!(
    DATABASE_COMMIT_PRE_VALIDATE_BATCH_TOTAL,
    "Number of commits pre-validated together in one batch"
);
pub fn log_commit_pre_validate_batch_size(size: usize) {
    log_distribution(&DATABASE_COMMIT_PRE_VALIDATE_BATCH_TOTAL, size as f64);
}

register_convex_histogram!(
    DATABASE_COMMIT_PREPARE_WRITES_SECONDS,
    "Time to prepare writes",
    &STATUS_LABEL
);
pub fn commit_prepare_writes_timer() -> StatusTimer {
    StatusTimer::new(&DATABASE_COMMIT_PREPARE_WRITES_SECONDS)
}

register_convex_histogram!(
    DATABASE_COMMIT_PERSISTENCE_WRITE_SECONDS,
    "Time to write one batch of commits to persistence. Sampled once per batch, so a batch \
     carrying several commits contributes a single observation",
    &STATUS_LABEL
);
pub fn commit_persistence_write_timer() -> StatusTimer {
    StatusTimer::new(&DATABASE_COMMIT_PERSISTENCE_WRITE_SECONDS)
}

register_convex_histogram!(
    DATABASE_WRITE_BATCH_COMMITS,
    "Number of commits combined into one batched persistence write"
);
register_convex_histogram!(
    DATABASE_WRITE_BATCH_DOCUMENTS,
    "Number of document rows in one batched persistence write"
);
register_convex_histogram!(
    DATABASE_WRITE_BATCH_BYTES,
    "Serialized size of one batched persistence write"
);
pub fn log_write_batch(num_commits: usize, num_documents: usize, size_bytes: u64) {
    log_distribution(&DATABASE_WRITE_BATCH_COMMITS, num_commits as f64);
    log_distribution(&DATABASE_WRITE_BATCH_DOCUMENTS, num_documents as f64);
    log_distribution(&DATABASE_WRITE_BATCH_BYTES, size_bytes as f64);
}

register_convex_histogram!(
    DATABASE_COMMIT_APPLY_SECONDS,
    "Time to apply a commit",
    &STATUS_LABEL
);
pub fn commit_apply_timer() -> StatusTimer {
    StatusTimer::new(&DATABASE_COMMIT_APPLY_SECONDS)
}

register_convex_histogram!(
    DATABASE_CONFLICT_CHECKER_APPEND_SECONDS,
    "Time to update pending writes when validating a commit"
);
pub fn pending_writes_append_timer() -> Timer<VMHistogram> {
    Timer::new(&DATABASE_CONFLICT_CHECKER_APPEND_SECONDS)
}

register_convex_histogram!(
    DATABASE_APPLY_DOCUMENT_STORE_APPEND_SECONDS,
    "Time to apply updates to the document store log"
);
pub fn write_log_append_timer() -> Timer<VMHistogram> {
    Timer::new(&DATABASE_APPLY_DOCUMENT_STORE_APPEND_SECONDS)
}

register_convex_histogram!(
    DATABASE_PENDING_WRITES_TO_WRITE_LOG_SECONDS,
    "Time to convert writes from PendingWrites to WriteLog"
);
pub fn pending_writes_to_write_log_timer() -> Timer<VMHistogram> {
    Timer::new(&DATABASE_PENDING_WRITES_TO_WRITE_LOG_SECONDS)
}

register_convex_histogram!(
    DATABASE_WRITE_LOG_COMMIT_BYTES,
    "Total size of all write log entries for a commit"
);
pub fn write_log_commit_bytes(bytes: usize) {
    log_distribution(&DATABASE_WRITE_LOG_COMMIT_BYTES, bytes as f64);
}

register_convex_counter!(DATABASE_COMMIT_ROWS, "Number of commits to database");
pub fn commit_rows(num_rows: u64) {
    log_counter(&DATABASE_COMMIT_ROWS, num_rows);
}

register_convex_counter!(
    DATABASE_COMMIT_INDEX_ROWS,
    "Number of index rows written to persistence"
);
pub fn commit_index_rows(num_rows: u64) {
    log_counter(&DATABASE_COMMIT_INDEX_ROWS, num_rows);
}

register_convex_counter!(
    DATABASE_COMMIT_INDEX_ENTRIES_TOTAL,
    "Logical index entries in commits acknowledged by persistence",
    &["operation", "is_system_index", "mode"]
);
register_convex_counter!(
    DATABASE_COMMIT_INDEX_BYTES_TOTAL,
    "Logical index bytes in commits acknowledged by persistence, measured with \
     PersistenceIndexEntry::size",
    &["operation", "is_system_index", "mode"]
);

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, strum::IntoStaticStr)]
enum IndexWriteOperation {
    #[strum(serialize = "unchanged_key")]
    Unchanged,
    #[strum(serialize = "new_key")]
    New,
    #[strum(serialize = "removed_key")]
    Removed,
}

impl From<&PersistenceIndexEntry> for IndexWriteOperation {
    fn from(entry: &PersistenceIndexEntry) -> Self {
        match (entry.value, entry.prev) {
            (Some(_), Some(_)) => Self::Unchanged,
            (Some(_), None) => Self::New,
            (None, _) => Self::Removed,
        }
    }
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct IndexWriteLabels {
    operation: IndexWriteOperation,
    is_system_index: bool,
    mode: IndexWriteMode,
}

#[derive(Default)]
struct IndexWriteTotals {
    entries: u64,
    bytes: u64,
}

#[derive(Default)]
pub(crate) struct CommitIndexWriteMetrics {
    totals: BTreeMap<IndexWriteLabels, IndexWriteTotals>,
}

impl CommitIndexWriteMetrics {
    pub(crate) fn record(&mut self, entry: &PersistenceIndexEntry, is_system_index: bool) {
        let totals = self
            .totals
            .entry(IndexWriteLabels {
                operation: entry.into(),
                is_system_index,
                mode: entry.mode,
            })
            .or_default();
        totals.entries += 1;
        totals.bytes += entry.size();
    }

    pub(crate) fn log(self) {
        for (key, totals) in self.totals {
            let labels = vec![
                StaticMetricLabel::new("operation", <&'static str>::from(key.operation)),
                StaticMetricLabel::new("is_system_index", key.is_system_index.as_label()),
                StaticMetricLabel::new("mode", <&'static str>::from(key.mode)),
            ];
            log_counter_with_labels(
                &DATABASE_COMMIT_INDEX_ENTRIES_TOTAL,
                totals.entries,
                labels.clone(),
            );
            log_counter_with_labels(&DATABASE_COMMIT_INDEX_BYTES_TOTAL, totals.bytes, labels);
        }
    }
}

register_convex_histogram!(
    DATABASE_SUBSCRIPTIONS_UPDATE_SECONDS,
    "Time to advance the SubscriptionManager's log"
);
pub fn subscriptions_update_timer() -> Timer<VMHistogram> {
    Timer::new(&DATABASE_SUBSCRIPTIONS_UPDATE_SECONDS)
}

register_convex_counter!(DATABASE_COMMITTER_FULL_TOTAL, "Committer queue full count");

pub fn committer_full_error() -> ErrorMetadata {
    log_counter(&DATABASE_COMMITTER_FULL_TOTAL, 1);

    ErrorMetadata::overloaded(
        "CommitterFullError",
        "Too many writes in a short period of time. Reduce your writes, spread your writes out \
         over time or remove unused indexes to reduce load and avoid errors.",
    )
}

register_convex_counter!(
    SUBSCRIPTIONS_WORKER_FULL_TOTAL,
    "Count of subscription worker full errors"
);
pub fn subscriptions_worker_full_error() -> ErrorMetadata {
    log_counter(&SUBSCRIPTIONS_WORKER_FULL_TOTAL, 1);
    ErrorMetadata::overloaded(
        "SubscriptionsWorkerFullError",
        "Too many concurrent subscription requests in a short period of time. Reduce the number \
         of subscriptions or throttle them to avoid errors.",
    )
}

register_convex_counter!(
    SHUTDOWN_TOTAL,
    "Count of errors caused due to the database shutting down"
);
pub fn shutdown_error() -> anyhow::Error {
    log_counter(&SHUTDOWN_TOTAL, 1);
    anyhow::anyhow!("Database Shutting Down")
        .context(ErrorMetadata::operational_internal_server_error())
}

register_convex_histogram!(BUMP_REPEATABLE_TS_SECONDS, "Time to bump max_repeatable_ts");
pub fn bump_repeatable_ts_timer() -> Timer<VMHistogram> {
    Timer::new(&BUMP_REPEATABLE_TS_SECONDS)
}

register_convex_histogram!(NEXT_COMMIT_TS_SECONDS, "Time to bump max_repeatable_ts");
pub fn next_commit_ts_seconds() -> Timer<VMHistogram> {
    Timer::new(&NEXT_COMMIT_TS_SECONDS)
}

register_convex_histogram!(
    LATEST_MIN_SNAPSHOT_SECONDS,
    "Time to get latest min_snapshot_ts"
);
pub fn latest_min_snapshot_timer() -> Timer<VMHistogram> {
    Timer::new(&LATEST_MIN_SNAPSHOT_SECONDS)
}

register_convex_histogram!(
    LATEST_MIN_DOCUMENT_SNAPSHOT_SECONDS,
    "Time to get latest min_document_snapshot_ts"
);
pub fn latest_min_document_snapshot_timer() -> Timer<VMHistogram> {
    Timer::new(&LATEST_MIN_DOCUMENT_SNAPSHOT_SECONDS)
}

register_convex_histogram!(
    RETENTION_ADVANCE_TIMER_SECONDS,
    "Time to advance retention min snapshot"
);
pub fn retention_advance_timestamp_timer() -> Timer<VMHistogram> {
    Timer::new(&RETENTION_ADVANCE_TIMER_SECONDS)
}

register_convex_histogram!(
    INDEX_RETENTION_DELETE_SECONDS,
    "Time for index retention to complete deletions"
);
pub fn index_retention_delete_timer() -> Timer<VMHistogram> {
    Timer::new(&INDEX_RETENTION_DELETE_SECONDS)
}

register_convex_histogram!(
    RETENTION_DELETE_DOCUMENTS_SECONDS,
    "Time for retention to complete deletions"
);
pub fn retention_delete_documents_timer() -> Timer<VMHistogram> {
    Timer::new(&RETENTION_DELETE_DOCUMENTS_SECONDS)
}

register_convex_histogram!(
    RETENTION_DELETE_DOCUMENT_CHUNK_SECONDS,
    "Time for document retention to delete one chunk"
);
pub fn retention_delete_document_chunk_timer() -> Timer<VMHistogram> {
    Timer::new(&RETENTION_DELETE_DOCUMENT_CHUNK_SECONDS)
}

register_convex_histogram!(
    INDEX_RETENTION_CURSOR_AGE_SECONDS,
    "Age of the index retention cursor"
);
pub fn log_index_retention_cursor_age(age_secs: f64) {
    log_distribution(&INDEX_RETENTION_CURSOR_AGE_SECONDS, age_secs)
}

register_convex_histogram!(
    INDEX_RETENTION_CURSOR_LAG_SECONDS,
    "Lag between the index retention cursor and the min index snapshot"
);
pub fn log_index_retention_cursor_lag(age_secs: f64) {
    log_distribution(&INDEX_RETENTION_CURSOR_LAG_SECONDS, age_secs)
}

register_convex_histogram!(
    DOCUMENT_RETENTION_CURSOR_AGE_SECONDS,
    "Age of the document retention cursor"
);
pub fn log_document_retention_cursor_age(age_secs: f64) {
    log_distribution(&DOCUMENT_RETENTION_CURSOR_AGE_SECONDS, age_secs)
}

register_convex_histogram!(
    DOCUMENT_RETENTION_CURSOR_LAG_SECONDS,
    "Lag between the retention cursor and the min document snapshot"
);
pub fn log_document_retention_cursor_lag(age_secs: f64) {
    log_distribution(&DOCUMENT_RETENTION_CURSOR_LAG_SECONDS, age_secs)
}

register_convex_counter!(
    INDEX_RETENTION_MISSING_CURSOR_INFO,
    "Index retention has no cursor"
);
pub fn log_index_retention_no_cursor() {
    log_counter(&INDEX_RETENTION_MISSING_CURSOR_INFO, 1)
}

register_convex_counter!(
    DOCUMENT_RETENTION_MISSING_CURSOR_INFO,
    "Document retention has no cursor"
);
pub fn log_document_retention_no_cursor() {
    log_counter(&DOCUMENT_RETENTION_MISSING_CURSOR_INFO, 1)
}

register_convex_counter!(
    DOCUMENT_RETENTION_SCANNED_DOCUMENT_TOTAL,
    "Count of documents scanned by retention",
    &["tombstone", "prev_rev"]
);
pub fn log_document_retention_scanned_document(is_tombstone: bool, has_prev_rev: bool) {
    log_counter_with_labels(
        &DOCUMENT_RETENTION_SCANNED_DOCUMENT_TOTAL,
        1,
        vec![
            StaticMetricLabel::new(
                "tombstone",
                if is_tombstone {
                    "is_tombstone"
                } else {
                    "is_document"
                },
            ),
            StaticMetricLabel::new(
                "prev_rev",
                if has_prev_rev {
                    "has_prev_rev"
                } else {
                    "no_prev_rev"
                },
            ),
        ],
    )
}

register_convex_counter!(
    RETENTION_DOCUMENTS_DELETED_TOTAL,
    "The total number of documents persistence returns as having been actually deleted by \
     retention."
);
pub fn log_retention_documents_deleted(deleted_rows: usize) {
    log_counter(&RETENTION_DOCUMENTS_DELETED_TOTAL, deleted_rows as u64)
}

register_convex_counter!(
    RETENTION_INDEX_ENTRIES_DELETED_TOTAL,
    "The total number of index entries persistence returns as having been actually deleted by \
     retention.",
    &["source"]
);
pub fn log_retention_index_entries_deleted(deleted_rows: usize, source: IndexRetentionSource) {
    log_counter_with_labels(
        &RETENTION_INDEX_ENTRIES_DELETED_TOTAL,
        deleted_rows as u64,
        vec![StaticMetricLabel::new(
            "source",
            <&'static str>::from(source),
        )],
    )
}

register_convex_histogram!(
    RETENTION_UNIQUE_INDEXES_TOTAL,
    "Number of distinct indexes a retention pass found expired entries for",
    &["source"]
);
pub fn log_retention_unique_indexes(unique_indexes: usize, source: IndexRetentionSource) {
    log_distribution_with_labels(
        &RETENTION_UNIQUE_INDEXES_TOTAL,
        unique_indexes as f64,
        vec![StaticMetricLabel::new(
            "source",
            <&'static str>::from(source),
        )],
    )
}

register_convex_histogram!(
    INDEX_BACKFILL_MARKER_CLEANUP_SECONDS,
    "Time for one index backfill marker cleanup pass",
    &STATUS_LABEL
);
pub fn index_backfill_marker_cleanup_timer() -> StatusTimer {
    StatusTimer::new(&INDEX_BACKFILL_MARKER_CLEANUP_SECONDS)
}

register_convex_counter!(
    INDEX_BACKFILL_MARKERS_DELETED_TOTAL,
    "The total number of index backfill markers persistence returns as having been deleted"
);
pub fn log_index_backfill_markers_deleted(deleted_rows: u64) {
    log_counter(&INDEX_BACKFILL_MARKERS_DELETED_TOTAL, deleted_rows)
}

register_convex_counter!(
    RETENTION_DOCUMENTS_DELETED_FROM_DELETED_TABLETS_TOTAL,
    "The total number of documents deleted from tablets deleted outside the document retention \
     window."
);
pub fn log_deleted_tablet_documents_deleted(rows_deleted: usize) {
    log_counter(
        &RETENTION_DOCUMENTS_DELETED_FROM_DELETED_TABLETS_TOTAL,
        rows_deleted as u64,
    )
}

register_convex_counter!(
    RETENTION_TS_ADVANCED_TOTAL,
    "Number of times that min_snapshot timestamp was advanced",
    &["type"]
);
pub fn log_retention_ts_advanced(ty: RetentionType) {
    log_counter_with_labels(
        &RETENTION_TS_ADVANCED_TOTAL,
        1,
        vec![StaticMetricLabel::new(
            "type",
            match ty {
                RetentionType::Document => "document",
                RetentionType::Index => "index",
            },
        )],
    );
}

register_convex_counter!(
    OUTSIDE_RETENTION_TOTAL,
    "Number of snapshots out of retention min_snapshot_ts",
    &["optimistic", "leader"]
);
register_convex_histogram!(
    SNAPSHOT_AGE_SECONDS,
    "Age of snapshot during verification",
    &["optimistic", "leader"]
);
register_convex_histogram!(
    SNAPSHOT_BUFFER_VS_MIN_SNAPSHOT_SECONDS,
    "Time elapsed from snapshot and min_snapshot_ts",
    &["optimistic", "leader"]
);
pub fn log_snapshot_verification_age<RT: Runtime>(
    rt: &RT,
    snapshot: Timestamp,
    min_snapshot_ts: Timestamp,
    optimistic: bool,
    leader: bool,
) {
    let labels = vec![
        StaticMetricLabel::new("optimistic", optimistic.as_label()),
        StaticMetricLabel::new("leader", leader.as_label()),
    ];
    if snapshot < min_snapshot_ts {
        log_counter_with_labels(&OUTSIDE_RETENTION_TOTAL, 1, labels.clone());
    }
    if let Ok(current_timestamp) = rt.generate_timestamp() {
        log_distribution_with_labels(
            &SNAPSHOT_AGE_SECONDS,
            current_timestamp.secs_since_f64(snapshot),
            labels.clone(),
        );
    }
    log_distribution_with_labels(
        &SNAPSHOT_BUFFER_VS_MIN_SNAPSHOT_SECONDS,
        snapshot.secs_since_f64(min_snapshot_ts),
        labels,
    );
}

register_convex_histogram!(
    UDF_QUERY_USED_RESULTS_TOTAL,
    "Number of results used in a UDF index query"
);
register_convex_histogram!(
    UDF_QUERY_UNUSED_RESULTS_TOTAL,
    "Number of results unused in a UDF index query"
);
register_convex_histogram!(
    UDF_QUERY_PAGES_FETCHED_TOTAL,
    "Number of pages fetched in a UDF index query"
);
pub fn log_index_range(returned_results: usize, unused_results: usize, pages_fetched: usize) {
    log_distribution(&UDF_QUERY_USED_RESULTS_TOTAL, returned_results as f64);
    log_distribution(&UDF_QUERY_UNUSED_RESULTS_TOTAL, unused_results as f64);
    log_distribution(&UDF_QUERY_PAGES_FETCHED_TOTAL, pages_fetched as f64);
}

register_convex_counter!(
    DATABASE_READS_REFRESH_MISS_TOTAL,
    "Number of times refreshing reads fails because the write log is stale"
);
pub fn log_reads_refresh_miss() {
    log_counter(&DATABASE_READS_REFRESH_MISS_TOTAL, 1);
}

register_convex_histogram!(
    DATABASE_READS_REFRESH_AGE_SECONDS,
    "How old a given read set is compared to the timestamp of a request that wants to use it",
);
pub fn log_read_set_age(seconds: f64) {
    log_distribution(&DATABASE_READS_REFRESH_AGE_SECONDS, seconds);
}

register_convex_counter!(
    VIRTUAL_TABLE_GET_REQUESTS_TOTAL,
    "Number of times virtual table get path is called"
);
pub fn log_virtual_table_get() {
    log_counter(&VIRTUAL_TABLE_GET_REQUESTS_TOTAL, 1);
}

register_convex_counter!(
    VIRTUAL_TABLE_QUERY_REQUESTS_TOTAL,
    "Number of times virtual table query path is called"
);
pub fn log_virtual_table_query() {
    log_counter(&VIRTUAL_TABLE_QUERY_REQUESTS_TOTAL, 1);
}

register_convex_histogram!(
    SEARCH_AND_VECTOR_BOOTSTRAP_SECONDS,
    "Time taken to bootstrap text and vector indexes",
    &STATUS_LABEL
);
pub fn bootstrap_timer() -> StatusTimer {
    StatusTimer::new(&SEARCH_AND_VECTOR_BOOTSTRAP_SECONDS)
}

register_convex_histogram!(
    SEARCH_AND_VECTOR_BOOTSTRAP_COMMITTER_UPDATE_SECONDS,
    "Time to update text and vector index bootstrap in the committer"
);
pub fn bootstrap_update_timer() -> Timer<VMHistogram> {
    Timer::new(&SEARCH_AND_VECTOR_BOOTSTRAP_COMMITTER_UPDATE_SECONDS)
}
register_convex_counter!(
    SEARCH_AND_VECTOR_BOOTSTRAP_COMMITTER_UPDATE_REVISIONS_TOTAL,
    "Number of revisions loaded during text and vector bootstrap updates in the committer"
);
register_convex_counter!(
    SEARCH_AND_VECTOR_BOOTSTRAP_COMMITTER_UPDATE_REVISIONS_BYTES,
    "Total size of revisions loaded during text and vector bootstrap updates in the committer"
);

pub fn finish_bootstrap_update(num_revisions: usize, bytes: usize) {
    log_counter(
        &SEARCH_AND_VECTOR_BOOTSTRAP_COMMITTER_UPDATE_REVISIONS_TOTAL,
        num_revisions as u64,
    );
    log_counter(
        &SEARCH_AND_VECTOR_BOOTSTRAP_COMMITTER_UPDATE_REVISIONS_BYTES,
        bytes as u64,
    );
}

register_convex_counter!(
    SEARCH_AND_VECTOR_BOOTSTRAP_REVISIONS_TOTAL,
    "Number of revisions loaded during text and vector bootstrap"
);
register_convex_counter!(
    SEARCH_AND_VECTOR_BOOTSTRAP_REVISIONS_BYTES,
    "Total size of revisions loaded during text and vector bootstrap"
);
pub fn finish_bootstrap(num_revisions: usize, bytes: usize, timer: StatusTimer) {
    log_counter(
        &SEARCH_AND_VECTOR_BOOTSTRAP_REVISIONS_TOTAL,
        num_revisions as u64,
    );
    log_counter(&SEARCH_AND_VECTOR_BOOTSTRAP_REVISIONS_BYTES, bytes as u64);
    timer.finish();
}

register_convex_histogram!(
    DATABASE_VECTOR_AND_SEARCH_BOOTSTRAP_SECONDS,
    "Time to bootstrap vector and text indexes",
    &STATUS_LABEL
);
pub fn search_and_vector_bootstrap_timer() -> StatusTimer {
    StatusTimer::new(&DATABASE_VECTOR_AND_SEARCH_BOOTSTRAP_SECONDS)
}

register_convex_histogram!(
    DATABASE_TABLE_SUMMARY_FINISH_BOOTSTRAP_SECONDS,
    "Time to finish table summary bootstrap",
    &STATUS_LABEL
);
pub fn table_summary_finish_bootstrap_timer() -> StatusTimer {
    StatusTimer::new(&DATABASE_TABLE_SUMMARY_FINISH_BOOTSTRAP_SECONDS)
}

register_convex_counter!(
    SEARCH_AND_VECTOR_BOOTSTRAP_DOCUMENTS_SKIPPED_TOTAL,
    "Number of documents skipped during vector and text index bootstrap",
);
pub fn log_document_skipped() {
    log_counter(&SEARCH_AND_VECTOR_BOOTSTRAP_DOCUMENTS_SKIPPED_TOTAL, 1);
}

pub mod search {

    use metrics::{
        register_convex_histogram,
        StatusTimer,
        STATUS_LABEL,
    };

    register_convex_histogram!(
        DATABASE_SEARCH_ITERATOR_NEXT_SECONDS,
        "Time to fetch the next document in a search query iterator",
        &STATUS_LABEL
    );
    pub fn iterator_next_timer() -> StatusTimer {
        StatusTimer::new(&DATABASE_SEARCH_ITERATOR_NEXT_SECONDS)
    }
}

pub mod vector {
    use metrics::{
        register_convex_histogram,
        CancelableTimer,
        StatusTimer,
        STATUS_LABEL,
    };

    register_convex_histogram!(
        DATABASE_VECTOR_SEARCH_QUERY_SECONDS,
        "Time to run a single vector search, not including retries due to bootstrapping",
        &STATUS_LABEL
    );
    pub fn vector_search_timer() -> StatusTimer {
        StatusTimer::new(&DATABASE_VECTOR_SEARCH_QUERY_SECONDS)
    }

    register_convex_histogram!(
        DATABASE_VECTOR_SEARCH_WITH_RETRIES_QUERY_SECONDS,
        "Time to run a vector search, including retries",
        &STATUS_LABEL
    );
    pub fn vector_search_with_retries_timer() -> CancelableTimer {
        CancelableTimer::new(&DATABASE_VECTOR_SEARCH_WITH_RETRIES_QUERY_SECONDS)
    }
}

register_convex_counter!(
    DATABASE_NONEMPTY_COMPONENT_EXPORTS_TOTAL,
    "Nonempty component definition loaded from database"
);
pub fn log_nonempty_component_exports() {
    log_counter(&DATABASE_NONEMPTY_COMPONENT_EXPORTS_TOTAL, 1);
}
register_convex_histogram!(
    DOCUMENT_DELTAS_READ_DOCUMENTS,
    "Total number of rows read in a document_deltas call",
);
pub fn log_document_deltas_read_documents(num: usize) {
    log_distribution(&DOCUMENT_DELTAS_READ_DOCUMENTS, num as f64);
}
register_convex_histogram!(
    DOCUMENT_DELTAS_RETURNED_DOCUMENTS,
    "Total number of documents returned by a document_deltas call",
);
pub fn log_document_deltas_returned_documents(num: usize) {
    log_distribution(&DOCUMENT_DELTAS_RETURNED_DOCUMENTS, num as f64);
}
register_convex_histogram!(
    LIST_SNAPSHOT_PAGE_DOCUMENTS,
    "Total number of documents in a returned SnapshotPage",
);
pub fn log_list_snapshot_page_documents(num_docs: usize) {
    log_distribution(&LIST_SNAPSHOT_PAGE_DOCUMENTS, num_docs as f64);
}

register_convex_histogram!(
    SUBSCRIPTION_INVALIDATION_UPDATES,
    "Number of subscriptions invalidated when advancing the log",
);
pub fn log_subscriptions_invalidated(num: usize) {
    log_distribution(&SUBSCRIPTION_INVALIDATION_UPDATES, num as f64);
}

register_convex_histogram!(
    SUBSCRIPTION_LOG_ITERATE_SECONDS,
    "Time to iterate over the write log when advancing subscriptions",
);
pub fn subscriptions_log_iterate_timer() -> Timer<VMHistogram> {
    Timer::new(&SUBSCRIPTION_LOG_ITERATE_SECONDS)
}

register_convex_histogram!(
    SUBSCRIPTION_LOG_INVALIDATE_SECONDS,
    "Time to invalidate segsstiptions when edvancing rh_ log",
);
pub fn subscriptions_invalidate_timer() -> Timer<VMHistogram> {
    Timer::new(&SUBSCRIPTION_LOG_INVALIDATE_SECONDS)
}

register_convex_histogram!(
    SUBSCRIPTION_LOG_ENFORCE_RETENTION_SECONDS,
    "Time to enforce retention policy when advancing subscriptions",
);
pub fn subscriptions_log_enforce_retention_timer() -> Timer<VMHistogram> {
    Timer::new(&SUBSCRIPTION_LOG_ENFORCE_RETENTION_SECONDS)
}

register_convex_histogram!(
    SUBSCRIPTION_LOG_PROCESSED_UPDATES,
    "Total number of index updates in the write log processed during one advance_log",
);
pub fn log_subscriptions_processed_index_updates(num_writes: usize) {
    log_distribution(&SUBSCRIPTION_LOG_PROCESSED_UPDATES, num_writes as f64);
}

register_convex_counter!(
    INDEX_TOO_LARGE_BLOCKING_WRITES,
    "Number of transactions that failed because search indexes hadn't flushed",
    &[SEARCH_TYPE_LABEL]
);
pub fn log_index_too_large_blocking_writes(index_type: SearchType) {
    log_counter_with_labels(&INDEX_TOO_LARGE_BLOCKING_WRITES, 1, vec![index_type.tag()]);
}

register_convex_histogram!(
    SUBSCRIPTION_QUEUE_LAG_SECONDS,
    "How long subscription requests wait in the subscription worker queue",
);
pub fn log_subscription_queue_lag(seconds: f64) {
    log_distribution(&SUBSCRIPTION_QUEUE_LAG_SECONDS, seconds);
}

register_convex_gauge!(
    SUBSCRIPTION_QUEUE_LENGTH_INFO,
    "The number of items in subscription queues",
);
pub fn log_subscription_queue_length_delta(delta: i64) {
    SUBSCRIPTION_QUEUE_LENGTH_INFO.add(delta as f64);
}

register_convex_counter!(
    WRITE_THROUGHPUT_LIMIT_EXCEEDED_TOTAL,
    "Total number of times mutation execution was rejected due to write throughput limit"
);
pub fn log_write_throughput_limit_exceeded() {
    log_counter(&WRITE_THROUGHPUT_LIMIT_EXCEEDED_TOTAL, 1);
}

register_convex_counter!(
    WRITE_THROUGHPUT_LIMIT_WOULD_BE_EXCEEDED_TOTAL,
    "Total number of times mutation execution would be rejected due to write throughput limit"
);
pub fn log_write_throughput_limit_would_be_exceeded() {
    log_counter(&WRITE_THROUGHPUT_LIMIT_WOULD_BE_EXCEEDED_TOTAL, 1);
}

register_convex_histogram!(
    WRITE_THROUGHPUT_BYTES,
    "Total bytes written in the current window",
);
pub fn log_write_throughput(bytes: u64) {
    log_distribution(&WRITE_THROUGHPUT_BYTES, bytes as f64);
}

register_convex_counter!(
    WRITE_THROUGHPUT_ROWS_LIMIT_EXCEEDED_TOTAL,
    "Total number of times mutation execution was rejected due to the rows written limit"
);
pub fn log_write_throughput_rows_limit_exceeded() {
    log_counter(&WRITE_THROUGHPUT_ROWS_LIMIT_EXCEEDED_TOTAL, 1);
}

register_convex_histogram!(
    WRITE_THROUGHPUT_ROWS,
    "Document and index rows written within the write throughput window"
);
pub fn log_write_throughput_rows(rows: u64) {
    log_distribution(&WRITE_THROUGHPUT_ROWS, rows as f64);
}
