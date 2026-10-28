use std::{
    marker::PhantomData,
    num::NonZeroU32,
    ops::Bound,
    sync::Arc,
};

use anyhow::Context;
use common::{
    bootstrap_model::index::{
        IndexMetadata,
        TabletIndexMetadata,
    },
    bounded_thread_pool::BoundedThreadPool,
    document::ParsedDocument,
    knobs::DEFAULT_DOCUMENTS_PAGE_SIZE,
    persistence::TimestampRange,
    query::{
        CursorPosition,
        Order,
    },
    runtime::{
        new_rate_limiter,
        Runtime,
    },
    sync::{
        Mutex,
        MutexGuard,
    },
    types::{
        RepeatableTimestamp,
        TabletIndexName,
    },
};
use database::{
    Database,
    IndexBackfillModel,
    IndexModel,
    SystemMetadataModel,
    Transaction,
};
use governor::Quota;
use itertools::Itertools;
use keybroker::Identity;
use search::metrics::SearchType;
use storage::Storage;
use sync_types::Timestamp;
use value::ResolvedDocumentId;

use crate::{
    index_meta::{
        BackfillState,
        SearchBackfillCursor,
        SearchIndex,
        SearchOnDiskState,
        SearchSnapshot,
        SegmentStatistics,
        SegmentType,
        SnapshotData,
    },
    metrics::{
        finish_search_index_merge_timer,
        search_compaction_merge_commit_timer,
        search_flush_merge_commit_timer,
        search_writer_lock_wait_timer,
        SearchIndexMergeType,
        SearchWriterLockWaiter,
    },
    search_flusher::{
        IndexBuild,
        IndexBuildResult,
    },
    BuildReason,
    MultiSegmentBackfillResult,
};

/// Serializes writes to index metadata from the worker and reconciles any
/// conflicting writes that may have happened due to concurrent modifications in
/// the flusher and compactor.
#[derive(Clone)]
pub struct SearchIndexMetadataWriter<RT: Runtime, T: SearchIndex> {
    inner: Arc<Mutex<Inner<RT, T>>>,
}

pub struct SearchIndexWriteResult<T: SearchIndex> {
    /// Stats summing metrics across all segments in the index.
    pub index_stats: T::Statistics,
    /// Stats for just the new segment if a new segment was built.
    pub new_segment_stats: Option<T::Statistics>,
    /// Stats for all segments in the index, including the new segment if it was
    /// built.
    pub per_segment_stats: Vec<T::Statistics>,
}

impl<RT: Runtime, T: SearchIndex> SearchIndexMetadataWriter<RT, T> {
    pub(crate) fn new(
        runtime: RT,
        database: Database<RT>,
        storage: Arc<dyn Storage>,
        build_index_args: T::BuildIndexArgs,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                runtime: runtime.clone(),
                database,
                storage,
                // Use small limits because we should only ever run one job at a time.
                thread_pool: BoundedThreadPool::new(
                    runtime,
                    2,
                    1,
                    match T::search_type() {
                        SearchType::Vector => "vector_writer",
                        SearchType::Text => "text_writer",
                    },
                ),
                build_index_args,
                _phantom_data: Default::default(),
            })),
        }
    }

    /// Merge results from a compaction with up to N previous writes by the
    /// flusher.
    ///
    /// There are only two writers, the flusher and the compactor. Each run
    /// serially. So we know that the only possibility of contention is from
    /// the flusher because we're writing the result from the compactor.
    ///
    /// The race we're worried about is that the flusher may have written one or
    /// more deletes to the set of segments we just compacted. We need to
    /// ensure those new deletes end up in our newly compacted segment. To
    /// do so, we'll read the document log from the snapshot timestamp
    /// in the index metadata when compaction started and the current snapshot
    /// timestamp. Each time we see a delete, we'll check to see if that
    /// document is in our new segment and if it is, we'll write the delete.
    pub(crate) async fn commit_compaction(
        &self,
        index_id: ResolvedDocumentId,
        index_name: TabletIndexName,
        start_compaction_ts: Timestamp,
        segments_to_compact: Vec<T::Segment>,
        new_segment: Option<T::Segment>,
        rate_limit_pages_per_second: NonZeroU32,
        schema: T::Schema,
    ) -> anyhow::Result<()> {
        self.inner(SearchWriterLockWaiter::Compactor)
            .await
            .commit_compaction(
                index_id,
                index_name,
                start_compaction_ts,
                segments_to_compact,
                new_segment,
                rate_limit_pages_per_second,
                schema,
            )
            .await
    }

    /// Merge results from a flush with up to N previous compactions by the
    /// compactor.
    ///
    /// There are only two writers, the flusher and the compactor. Each run
    /// serially. So we know that the only possibility of contention
    /// is from the compactor because we're writing the result from the flusher.
    ///
    /// The race we're worried about is that we may have just written one or
    /// more deletes to segments that were compacted while we were flushing. We
    /// need to ensure those deletes end up in the newly compacted segment. To
    /// do so, we'll read the document log from the current snapshot timestamp
    /// to the new snapshot time we're about to write. If we find any deletes in
    /// the document log, we'll try and write them to all current segments. Then
    /// we can append our new segment (if present) and write the updated result.
    pub(crate) async fn commit_flush(
        &self,
        job: &IndexBuild<T>,
        result: IndexBuildResult<T>,
    ) -> anyhow::Result<SearchIndexWriteResult<T>> {
        let IndexBuildResult {
            snapshot_ts,
            data,
            total_stats,
            new_segment_stats,
            new_segment_id,
            backfill_result,
        } = result;

        let inner = self.inner(SearchWriterLockWaiter::Flusher).await;
        let segments = data.require_multi_segment()?;
        let per_segment_stats = segments
            .iter()
            .map(|segment| segment.statistics())
            .collect::<anyhow::Result<Vec<_>>>()?;
        if let Some(index_backfill_result) = backfill_result {
            let schema = T::new_schema(&job.index_config.spec);
            inner
                .commit_backfill_flush(job, segments, new_segment_id, index_backfill_result, schema)
                .await?
        } else {
            let schema = T::new_schema(&job.index_config.spec);
            inner
                .commit_snapshot_flush(job, snapshot_ts, segments, new_segment_id, schema)
                .await?
        }

        Ok(SearchIndexWriteResult {
            index_stats: total_stats,
            new_segment_stats,
            per_segment_stats,
        })
    }

    async fn inner(&self, waiter: SearchWriterLockWaiter) -> MutexGuard<'_, Inner<RT, T>> {
        let lock_timer = search_writer_lock_wait_timer(waiter, T::search_type());
        let inner = self.inner.lock().await;
        drop(lock_timer);
        inner
    }
}

struct Inner<RT: Runtime, T: SearchIndex> {
    runtime: RT,
    database: Database<RT>,
    storage: Arc<dyn Storage>,
    thread_pool: BoundedThreadPool<RT>,
    build_index_args: T::BuildIndexArgs,
    _phantom_data: PhantomData<T>,
}

impl<RT: Runtime, T: SearchIndex> Inner<RT, T> {
    async fn require_index_metadata(
        tx: &mut Transaction<RT>,
        index_id: ResolvedDocumentId,
    ) -> anyhow::Result<ParsedDocument<TabletIndexMetadata>> {
        let mut index_model = IndexModel::new(tx);
        index_model.require_index_by_id(index_id).await
    }

    fn is_compaction_merge_required(
        segments_to_compact: &Vec<T::Segment>,
        current_segments: &Vec<T::Segment>,
    ) -> anyhow::Result<bool> {
        for original_segment in segments_to_compact {
            let current_version = current_segments
                .iter()
                .find(|segment| segment.id() == original_segment.id());
            let Some(current_version) = current_version else {
                // Only the compactor should remove segments, so they should never be removed
                // concurrently.
                anyhow::bail!("Segment unexpectedly removed!")
            };
            // For a given segment id, we can only ever increase the number of deletes. The
            // only way to decrease the number of deletes is by compaction,
            // which creates a new segment with a new id. So if the number of deletes has
            // changed, it's due to an increase from a conflicting write by the
            // flusher.
            if current_version.statistics()?.num_deleted_documents()
                != original_segment.statistics()?.num_deleted_documents()
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn commit_compaction(
        &self,
        index_id: ResolvedDocumentId,
        index_name: TabletIndexName,
        start_compaction_ts: Timestamp,
        segments_to_compact: Vec<T::Segment>,
        mut new_segment: Option<T::Segment>,
        rate_limit_pages_per_second: NonZeroU32,
        schema: T::Schema,
    ) -> anyhow::Result<()> {
        let timer = search_compaction_merge_commit_timer(T::search_type());
        let mut tx: Transaction<RT> = self.database.begin(Identity::system()).await?;
        let mut metadata = Self::require_index_metadata(&mut tx, index_id).await?;

        let (spec, state) = T::extract_metadata(metadata)?;
        let snapshot_ts = state.ts().context("Compacted a segment without a ts?")?;
        let snapshot_ts = tx.begin_timestamp().prior_ts(snapshot_ts)?;
        let mut current_segments = state.segments().clone();

        let is_merge_required =
            Self::is_compaction_merge_required(&segments_to_compact, &current_segments)?;
        if is_merge_required {
            // Drop and then restart the transaction, it could take a while to
            // merge deletes.
            drop(tx);
            if let Some(segment) = new_segment {
                let results = self
                    .merge_deletes(
                        vec![segment],
                        start_compaction_ts,
                        snapshot_ts,
                        index_name.clone(),
                        rate_limit_pages_per_second,
                        schema,
                    )
                    .await?;
                anyhow::ensure!(results.len() == 1);
                new_segment = results.into_iter().next();
            }
            tx = self.database.begin(Identity::system()).await?;
            metadata = Self::require_index_metadata(&mut tx, index_id).await?;
            let (_, disk_state) = T::extract_metadata(metadata)?;
            current_segments = disk_state.segments();
        }

        let removed_segment_ids = segments_to_compact
            .into_iter()
            .map(|segment| segment.id().to_string())
            .collect_vec();
        let new_segments = current_segments
            .iter()
            .filter(|segment| !removed_segment_ids.contains(&segment.id().to_string()))
            .cloned()
            .chain(new_segment)
            .collect_vec();

        self.write_metadata(
            tx,
            index_id,
            index_name,
            spec,
            state.with_updated_segments(new_segments)?,
        )
        .await?;

        finish_search_index_merge_timer(
            timer,
            if is_merge_required {
                SearchIndexMergeType::Required
            } else {
                SearchIndexMergeType::NotRequired
            },
        );
        Ok(())
    }

    async fn write_metadata(
        &self,
        mut tx: Transaction<RT>,
        id: ResolvedDocumentId,
        name: TabletIndexName,
        spec: T::Spec,
        state: SearchOnDiskState<T>,
    ) -> anyhow::Result<()> {
        let new_metadata = IndexMetadata {
            name,
            config: T::new_index_config(spec, state),
        };

        SystemMetadataModel::new_global(&mut tx)
            .replace(id, new_metadata.try_into()?)
            .await?;
        self.database
            .commit_with_write_source(
                tx,
                match T::search_type() {
                    SearchType::Vector => "search_index_metadata_writer_write_vector",
                    SearchType::Text => "search_index_metadata_writer_write_text",
                },
            )
            .await?;
        Ok(())
    }

    /// A merge flush is required if a compaction happened.
    fn is_merge_flush_required(
        new_segments: &Vec<T::Segment>,
        current_segments: &Vec<T::Segment>,
        new_segment_id: &Option<String>,
    ) -> bool {
        // TODO(sam): We could be more efficient if we only counted new segments to
        // which our flush actually added at least one delete.
        let current_segment_ids = current_segments
            .iter()
            .map(|segment| segment.id().to_string())
            .collect_vec();
        // If any of the segments other than the one the flush optionally added is
        // missing, then some conflicting compaction must have happened.
        // Compaction is the only way that segments can be removed.
        new_segments
            .iter()
            // Ignore the new segment id, if we created a new segment
            .filter(|segment| {
                new_segment_id
                    .as_ref()
                    .map(|new_segment_id| *new_segment_id != segment.id())
                    .unwrap_or(true)
            })
            // Check to see if any of our other new segments were removed while we flushed.
            .any(|segment| !current_segment_ids.contains(&segment.id().to_string()))
    }

    async fn commit_backfill_flush(
        &self,
        job: &IndexBuild<T>,
        mut new_and_modified_segments: Vec<T::Segment>,
        new_segment_id: Option<String>,
        backfill_result: MultiSegmentBackfillResult,
        schema: T::Schema,
    ) -> anyhow::Result<()> {
        let timer = search_flush_merge_commit_timer(T::search_type());
        let mut tx: Transaction<RT> = self.database.begin(Identity::system()).await?;
        let metadata = Self::require_index_metadata(&mut tx, job.metadata_id).await?;

        anyhow::ensure!(
            metadata.config.is_backfilling()
                || matches!(job.build_reason, BuildReason::VersionMismatch)
        );

        let (spec, current_disk_state) = T::extract_metadata(metadata)?;
        let staged = match &current_disk_state {
            SearchOnDiskState::Backfilling(backfill_state) => backfill_state.staged,
            SearchOnDiskState::Backfilled {
                snapshot: _,
                staged,
            } => *staged,
            SearchOnDiskState::SnapshottedAt(_) => {
                anyhow::bail!("Index is already snapshotted, cannot commit backfill flush")
            },
        };

        let current_segments = current_disk_state.segments();
        let is_merge_required = Self::is_merge_flush_required(
            &new_and_modified_segments,
            &current_segments,
            &new_segment_id,
        );
        let new_segment_num_docs = new_segment_id
            .as_ref()
            .map(|new_segment_id| {
                new_and_modified_segments
                    .iter()
                    .find(|segment| segment.id() == new_segment_id)
                    .context("Missing new segment in segments list!")
            })
            .transpose()?
            .as_ref()
            .map(|segment| anyhow::Ok::<u64>(segment.statistics()?.num_documents()))
            .transpose()?
            .unwrap_or_default();
        if is_merge_required {
            // Drop and restart, merging could take a while.
            drop(tx);
            let start_snapshot_ts = current_disk_state
                .ts()
                .context("Compaction ran before index had a snapshot")?;
            let updated_segments = self
                .merge_deletes(
                    current_segments,
                    start_snapshot_ts,
                    backfill_result.new_ts,
                    job.index_name.clone(),
                    job.build_reason.read_max_pages_per_second(),
                    schema,
                )
                .await?;
            tx = self.database.begin(Identity::system()).await?;
            let new_segment = new_segment_id
                .map(|new_segment_id| {
                    new_and_modified_segments
                        .into_iter()
                        .find(|segment| segment.id() == new_segment_id)
                        .context("Missing new segment in segments list!")
                })
                .transpose()?;
            new_and_modified_segments = updated_segments
                .into_iter()
                .chain(new_segment)
                .collect_vec();
        }
        let mut index_backfill_model = IndexBackfillModel::new(&mut tx);
        let existing_backfill_metadata = index_backfill_model
            .existing_backfill_metadata_for_index(job.index_id)
            .await?
            .with_context(|| format!("Index backfill not found for index {}", job.index_id))?;
        index_backfill_model
            .update_search_index_backfill_progress(
                &existing_backfill_metadata,
                *job.index_name.table(),
                new_segment_num_docs,
            )
            .await?;

        let new_state = match backfill_result.new_cursor.index_key {
            Some(CursorPosition::End) => SearchOnDiskState::Backfilled {
                snapshot: SearchSnapshot {
                    ts: *backfill_result.new_ts,
                    data: SnapshotData::MultiSegment(new_and_modified_segments),
                },
                staged,
            },
            Some(CursorPosition::After(cursor)) => SearchOnDiskState::Backfilling(BackfillState {
                segments: new_and_modified_segments,
                cursor: Some(SearchBackfillCursor {
                    last_segment_ts: *backfill_result.new_ts,
                    table_scan_cursor: cursor.0,
                }),
                staged,
            }),
            None => anyhow::bail!("Tried to commit backfill flush without a cursor"),
        };

        self.write_metadata(tx, job.metadata_id, job.index_name.clone(), spec, new_state)
            .await?;

        finish_search_index_merge_timer(
            timer,
            if is_merge_required {
                SearchIndexMergeType::Required
            } else {
                SearchIndexMergeType::NotRequired
            },
        );
        Ok(())
    }

    async fn commit_snapshot_flush(
        &self,
        job: &IndexBuild<T>,
        new_ts: RepeatableTimestamp,
        mut new_and_modified_segments: Vec<T::Segment>,
        new_segment_id: Option<String>,
        schema: T::Schema,
    ) -> anyhow::Result<()> {
        let timer = search_flush_merge_commit_timer(T::search_type());
        let mut tx: Transaction<RT> = self.database.begin(Identity::system()).await?;
        let metadata = Self::require_index_metadata(&mut tx, job.metadata_id).await?;

        let (spec, current_disk_state) = T::extract_metadata(metadata.clone())?;

        let current_segments = current_disk_state.segments();
        let is_merge_required = Self::is_merge_flush_required(
            &new_and_modified_segments,
            &current_segments,
            &new_segment_id,
        );
        if is_merge_required {
            // Drop and restart, merging could take a while.
            drop(tx);
            // If we were backfilling and had no timestamp, there should have been no
            // segments for compaction to compact. If we do have segments, then
            // we necesssarily must have a snapshot timestamp for when those
            // segments were valid. So it's an error if we think we need to
            // merge with a compaction but have no snapshot timestamp.
            let start_snapshot_ts = current_disk_state
                .ts()
                .context("Compaction ran before index had a snapshot")?;
            let updated_segments = self
                .merge_deletes(
                    current_segments,
                    // We're assuming that nothing else can touch the snapshot other than flushes.
                    // Right now this works because the flusher is already serial and its
                    // the only thing that advances the metadata timestamp. If that were
                    // ever not true, we'd need to pass through a timestamp rather than using the
                    // one in the current metadata.
                    start_snapshot_ts,
                    new_ts,
                    job.index_name.clone(),
                    job.build_reason.read_max_pages_per_second(),
                    schema,
                )
                .await?;
            // If we had a flush that involved only deletes, we may not have a new segment
            // so new_segment / new_segment_id will be None. However if we did
            // have a new segment id, we must find and append the matching
            // segment or something has gone wrong.
            let new_segment = new_segment_id
                .map(|new_segment_id| {
                    new_and_modified_segments
                        .into_iter()
                        .find(|segment| segment.id() == new_segment_id)
                        .context("Missing new segment in segments list!")
                })
                .transpose()?;
            new_and_modified_segments = updated_segments
                .into_iter()
                .chain(new_segment)
                .collect_vec();
            tx = self.database.begin(Identity::system()).await?;
        }

        self.write_metadata(
            tx,
            job.metadata_id,
            job.index_name.clone(),
            spec,
            current_disk_state.with_updated_snapshot(*new_ts, new_and_modified_segments)?,
        )
        .await?;

        finish_search_index_merge_timer(
            timer,
            if is_merge_required {
                SearchIndexMergeType::Required
            } else {
                SearchIndexMergeType::NotRequired
            },
        );
        Ok(())
    }

    async fn merge_deletes(
        &self,
        segments_to_update: Vec<T::Segment>,
        start_ts: Timestamp,
        current_ts: RepeatableTimestamp,
        index_name: TabletIndexName,
        rate_limit_pages_per_second: NonZeroU32,
        schema: T::Schema,
    ) -> anyhow::Result<Vec<T::Segment>> {
        let storage = self.storage.clone();
        let runtime = self.runtime.clone();
        let database = self.database.clone();
        let build_index_args = self.build_index_args.clone();
        self.thread_pool
            .execute_async(move || async move {
                Self::merge_deletes_on_thread(
                    runtime,
                    database,
                    segments_to_update,
                    start_ts,
                    current_ts,
                    index_name,
                    storage,
                    rate_limit_pages_per_second,
                    build_index_args,
                    schema,
                )
                .await
            })
            .await?
    }

    async fn merge_deletes_on_thread(
        runtime: RT,
        database: Database<RT>,
        segments_to_update: Vec<T::Segment>,
        start_ts: Timestamp,
        current_ts: RepeatableTimestamp,
        index_name: TabletIndexName,
        storage: Arc<dyn Storage>,
        rate_limit_pages_per_second: NonZeroU32,
        build_index_args: T::BuildIndexArgs,
        schema: T::Schema,
    ) -> anyhow::Result<Vec<T::Segment>> {
        let row_rate_limiter = new_rate_limiter(
            runtime.clone(),
            Quota::per_second(
                NonZeroU32::new(*DEFAULT_DOCUMENTS_PAGE_SIZE)
                    .and_then(|val| val.checked_mul(rate_limit_pages_per_second))
                    .context("Invalid row rate limit")?,
            ),
        );
        let mut previous_segments =
            T::download_previous_segments(storage.clone(), segments_to_update).await?;
        let range = TimestampRange::new((Bound::Excluded(start_ts), Bound::Included(*current_ts)));
        let documents = T::load_doc_stream(
            &database,
            *index_name.table(),
            range,
            Order::Asc,
            &row_rate_limiter,
        );

        T::merge_deletes(
            &mut previous_segments,
            documents,
            build_index_args,
            schema,
            start_ts,
        )
        .await?;

        T::upload_previous_segments(storage, previous_segments).await
    }
}
