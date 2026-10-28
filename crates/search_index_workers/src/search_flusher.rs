use std::{
    collections::{
        BTreeMap,
        Bound,
    },
    iter,
    marker::PhantomData,
    num::NonZeroU32,
    ops::Deref,
    path::PathBuf,
    sync::Arc,
};

use anyhow::Context;
use common::{
    bootstrap_model::index::search_index::SearchBackfillCursor,
    index::{
        IndexKey,
        IndexKeyBytes,
    },
    knobs::{
        DEFAULT_DOCUMENTS_PAGE_SIZE,
        SEARCH_WORKERS_MAX_CHECKPOINT_AGE,
        VECTOR_INDEX_WORKER_PAGE_SIZE,
    },
    persistence::{
        DocumentLogEntry,
        PersistenceReader,
        PersistenceSnapshot,
        RepeatablePersistence,
        TimestampRange,
    },
    query::{
        CursorPosition,
        Order,
    },
    runtime::{
        new_rate_limiter,
        Runtime,
    },
    types::{
        IndexId,
        IndexRef,
        RepeatableTimestamp,
        TabletIndexName,
    },
};
use database::{
    Database,
    IndexBackfillModel,
    IndexModel,
    IndexWorkerMetadataModel,
    SearchFlusherWakeSubscriber,
    TableScanCursor,
    Token,
};
use errors::ErrorMetadataAnyhowExt;
use futures::{
    StreamExt,
    TryStreamExt,
};
use futures_async_stream::try_stream;
use governor::Quota;
use keybroker::Identity;
use search::metrics::SearchType;
use storage::Storage;
use sync_types::Timestamp;
use tempfile::TempDir;
use tokio::{
    sync::oneshot,
    task,
};
use value::{
    ResolvedDocumentId,
    TableNumber,
    TabletId,
};

use crate::{
    index_meta::{
        SearchIndex,
        SearchIndexConfig,
        SearchOnDiskState,
        SearchSnapshot,
        SegmentStatistics,
        SegmentType,
        SnapshotData,
    },
    metrics::{
        build_one_search_index_timer,
        log_documents_per_new_search_segment,
        log_documents_per_search_index,
        log_documents_per_search_segment,
        log_non_deleted_documents_per_search_index,
        log_non_deleted_documents_per_search_segment,
    },
    writer::{
        SearchIndexMetadataWriter,
        SearchIndexWriteResult,
    },
    BuildReason,
    FlusherType,
    MultiSegmentBackfillResult,
};

pub(crate) const FLUSH_RUNNING_LABEL: &str = "flush_running";

pub struct SearchFlusher<RT: Runtime, T: SearchIndex> {
    params: Params<RT, T>,
    writer: SearchIndexMetadataWriter<RT, T>,
    _config: PhantomData<T>,
    flusher_type: FlusherType,
}

impl<RT: Runtime, T: SearchIndex> Deref for SearchFlusher<RT, T> {
    type Target = Params<RT, T>;

    fn deref(&self) -> &Self::Target {
        &self.params
    }
}

#[derive(Clone)]
pub struct Params<RT: Runtime, T: SearchIndex> {
    runtime: RT,
    database: Database<RT>,
    reader: Arc<dyn PersistenceReader>,
    storage: Arc<dyn Storage>,
    limits: SearchIndexLimits,
    build_args: T::BuildIndexArgs,
}

#[derive(Clone)]
pub struct SearchIndexLimits {
    /// The size at which we will start building a new segment. It's a 'soft'
    /// limit because we will allow writes to continue even after this size
    /// is reached. Writes are only blocked after we hit a hard index size
    /// limit not specified here.
    pub index_size_soft_limit: usize,
    /// The number of bytes we'll write to each individual segment when
    /// backfilling a new index.
    ///
    /// For example, if we add an index to a table with 10GiB of documents, we
    /// don't want to build one single 10GiB segment on backend (very costly
    /// in terms of disk/cpu/memory). So instead we build N smaller segments
    /// and allow compaction to merge them (or not) as necessary.
    ///
    /// This number determines the size of each of the N smaller segments based
    /// on an estimate of the size of each document we index.
    pub incremental_multipart_threshold_bytes: usize,
}

impl<RT: Runtime, T: SearchIndex + 'static> SearchFlusher<RT, T> {
    pub(crate) fn new(
        runtime: RT,
        database: Database<RT>,
        reader: Arc<dyn PersistenceReader>,
        storage: Arc<dyn Storage>,
        limits: SearchIndexLimits,
        writer: SearchIndexMetadataWriter<RT, T>,
        build_args: T::BuildIndexArgs,
        flusher_type: FlusherType,
    ) -> Self {
        Self {
            params: Params {
                runtime,
                database,
                reader,
                storage,
                limits,
                build_args,
            },
            writer,
            _config: PhantomData,
            flusher_type,
        }
    }

    /// Live flushers subscribe to be woken as soon as an in-memory index
    /// passes the soft limit, instead of waiting for the next poll. Backfill
    /// flushers don't build in-memory index contents, so they have nothing to
    /// wake up for.
    pub(crate) fn wake_subscriber(&self) -> Option<SearchFlusherWakeSubscriber> {
        match self.flusher_type {
            FlusherType::LiveFlush => Some(
                self.database
                    .subscribe_search_flusher_wake(Self::search_type()),
            ),
            FlusherType::Backfill => None,
        }
    }

    fn index_type_name(&self) -> &'static str {
        match Self::search_type() {
            SearchType::Vector => "vector",
            SearchType::Text => "text",
        }
    }

    /// Run one step of the flusher's main loop.
    ///
    /// Returns a map of IndexName to number of documents indexed for each
    /// index that was built.
    pub async fn step(&self) -> anyhow::Result<(BTreeMap<TabletIndexName, u64>, Token)> {
        let mut metrics = BTreeMap::new();

        let (to_build, token) = self.needs_backfill().await?;
        let num_to_build = to_build.len();
        let index_type = self.index_type_name();
        if num_to_build > 0 {
            tracing::info!(
                "SearchIndexFlusher ({:?}) has {num_to_build} {index_type} indexes to build",
                self.flusher_type
            );
        }

        let pause_client = self.database.runtime().pause_client();
        pause_client.wait(FLUSH_RUNNING_LABEL).await;

        for job in to_build {
            task::consume_budget().await;

            let index_name = job.index_name.clone();
            let num_documents_indexed = self.build_one(job, self.build_args.clone()).await?;
            metrics.insert(index_name, num_documents_indexed);
        }

        if num_to_build > 0 {
            tracing::info!(
                "SearchIndexFlusher ({:?}) built {num_to_build} {index_type} indexes",
                self.flusher_type
            );
        }

        Ok((metrics, token))
    }

    fn search_type() -> SearchType {
        T::search_type()
    }

    pub(crate) async fn build_one(
        &self,
        job: IndexBuild<T>,
        build_args: T::BuildIndexArgs,
    ) -> anyhow::Result<u64> {
        let timer = build_one_search_index_timer(T::search_type());
        match self.build_one_inner(job, build_args).await {
            Ok(num_documents) => {
                timer.finish();
                Ok(num_documents)
            },
            Err(e) => {
                timer.finish_with(e.metric_status_label_value());
                Err(e)
            },
        }
    }

    async fn build_one_inner(
        &self,
        job: IndexBuild<T>,
        build_args: T::BuildIndexArgs,
    ) -> anyhow::Result<u64> {
        let result = self.build_multipart_segment(&job, build_args).await?;
        tracing::debug!(
            "Built a {} segment for: {result:#?}",
            self.index_type_name()
        );

        let SearchIndexWriteResult {
            index_stats,
            new_segment_stats,
            per_segment_stats,
        } = self.writer.commit_flush(&job, result).await?;

        let new_segment_stats = new_segment_stats.unwrap_or_default();
        log_documents_per_new_search_segment(
            new_segment_stats.num_documents(),
            Self::search_type(),
        );

        per_segment_stats.into_iter().for_each(|stats| {
            log_documents_per_search_segment(stats.num_documents(), Self::search_type());
            log_non_deleted_documents_per_search_segment(
                stats.num_non_deleted_documents(),
                Self::search_type(),
            );
        });

        log_documents_per_search_index(index_stats.num_documents(), Self::search_type());
        log_non_deleted_documents_per_search_index(
            index_stats.num_non_deleted_documents(),
            Self::search_type(),
        );

        Ok(new_segment_stats.num_documents())
    }

    /// Compute the set of indexes that need to be backfilled.
    async fn needs_backfill(&self) -> anyhow::Result<(Vec<IndexBuild<T>>, Token)> {
        let mut to_build = vec![];

        let mut tx = self.database.begin(Identity::system()).await?;
        let step_ts = tx.begin_timestamp();

        let snapshot = self.database.snapshot(step_ts)?;

        let ready_index_sizes = T::get_index_sizes(snapshot)?;

        IndexModel::new(&mut tx).take_indexes_dependency()?;
        for index_doc in tx.index_registry().clone().all_indexes() {
            let index_id = index_doc.id();
            let Some(config) = T::get_config(&index_doc.config) else {
                continue;
            };
            let name = &index_doc.name;

            let needs_backfill = match &config.on_disk_state {
                SearchOnDiskState::Backfilling(_) => Some(BuildReason::Backfilling),
                SearchOnDiskState::SnapshottedAt(snapshot)
                | SearchOnDiskState::Backfilled { snapshot, .. }
                    if !T::is_version_current(snapshot) =>
                {
                    Some(BuildReason::VersionMismatch)
                },
                SearchOnDiskState::SnapshottedAt(SearchSnapshot { ts, .. })
                | SearchOnDiskState::Backfilled {
                    snapshot: SearchSnapshot { ts, .. },
                    ..
                } => {
                    let ts = IndexWorkerMetadataModel::new(&mut tx)
                        .get_fast_forward_ts(*ts, index_id.internal_id().into())
                        .await?;

                    let index_size = ready_index_sizes
                        .get(&index_id.internal_id().into())
                        .cloned()
                        .unwrap_or(0);

                    anyhow::ensure!(ts <= *step_ts);

                    let index_age = *step_ts - ts;
                    let too_old = (index_age >= *SEARCH_WORKERS_MAX_CHECKPOINT_AGE
                        && index_size > 0)
                        .then_some(BuildReason::TooOld);
                    if too_old.is_some() {
                        tracing::info!(
                            "Non-empty index is too old, age: {:?}, size: {index_size}",
                            index_age
                        );
                    }
                    tracing::debug!(
                        "Search index {name} (index id {index_id}) index size is {index_size}, \
                         soft limit is {}",
                        self.limits.index_size_soft_limit
                    );
                    let too_large = (index_size > self.limits.index_size_soft_limit)
                        .then_some(BuildReason::TooLarge);
                    // Order matters! Too large is more urgent than too old.
                    too_large.or(too_old)
                },
            };
            if let Some(build_reason) = needs_backfill {
                if FlusherType::from(build_reason) != self.flusher_type {
                    tracing::debug!(
                        "Skipping build for index {name} with id {index_id} and {build_reason:?} \
                         because it is a {:?} flusher",
                        self.flusher_type
                    );
                    continue;
                }
                tracing::info!(
                    "Queueing {} index for rebuild: {name:?} ({build_reason:?})",
                    self.index_type_name()
                );
                let table_id = name.table();
                let by_id_metadata = IndexModel::new(&mut tx)
                    .by_id_index_metadata(*table_id)
                    .await?;
                let job = IndexBuild {
                    index_name: name.clone(),
                    index_id: index_id.internal_id().into(),
                    by_id: IndexRef::try_from(&by_id_metadata)?,
                    index_config: config,
                    metadata_id: index_id,
                    build_reason,
                };
                to_build.push(job);
            } else {
                tracing::debug!(
                    "Search index {name} with id {index_id} does not need segment built"
                );
            }
        }
        Ok((to_build, tx.into_token()?))
    }

    /// Build a segment for a search index, handling both partial updates to
    /// existing indexes and incremental backfill of new indexes.
    ///
    /// # Incremental Backfill Algorithm
    ///
    /// When a new search index is created on a large table, we don't build one
    /// giant segment. Instead we scan the table incrementally by document ID,
    /// building one segment per flusher iteration. The cursor and segments are
    /// persisted in [`BackfillState`] so progress survives restarts.
    ///
    /// ```text
    ///                          doc_id ──►
    ///              1    2    3    4    5    6    7
    ///         ┌──────────────┐
    ///         │  S0          │
    ///         │  table scan  │
    ///  t0 ────├──────────────┼─────────────┐
    ///    │    │  S1          │ S1          │
    ///    │    │  doc log     │ table scan  │
    ///  t1 ────├──────────────┴─────────────┼──────────┐
    ///    │    │  S2                        │ S2       │
    ///    │    │  doc log                   │ table    │
    ///    │    │                            │ scan     │
    ///  t2 ────└────────────────────────────┴──────────┘
    ///    ▼
    ///   ts
    ///
    ///  Each segment has two parts:
    ///    - incremental_table_scan_stream(): Scans the by_id index from
    ///      cursor forward (the squares along the diagonal), streaming
    ///      documents until incremental_multipart_threshold_bytes is
    ///      reached.
    ///    - walk_document_log_for_updates(): Reads the doc
    ///      log for (prev_ts, new_ts], filtered to doc IDs <=
    ///      cursor start (the rectangles below the diagonal).
    ///      This catches updates and deletes to already-scanned
    ///      documents and feeds them to build_disk_index to apply
    ///      deletes to previous segments.
    ///
    ///  build_incremental_doc_stream() chains the table scan then
    ///  the doc log, so build_disk_index sees new documents first,
    ///  then mutations to previous segments.
    /// ```
    ///
    /// Steps per iteration:
    /// 1. [`incremental_table_scan_stream`]: scan by_id from cursor, up to
    ///    threshold
    /// 2. [`build_incremental_doc_stream`]: chain table scan docs with doc log
    ///    updates
    /// 3. [`build_disk_index`]: build new segment, apply deletes to prior ones
    /// 4. Upload segment, persist cursor + segments to [`BackfillState`]
    ///
    /// After the final iteration (cursor reaches End), the index transitions
    /// from Backfilling to SnapshottedAt and becomes ready for queries.
    async fn build_multipart_segment(
        &self,
        job: &IndexBuild<T>,
        build_index_args: T::BuildIndexArgs,
    ) -> anyhow::Result<IndexBuildResult<T>> {
        let index_path = TempDir::new()?;
        let mut tx = self.database.begin(Identity::system()).await?;

        let new_ts = tx.begin_timestamp();
        let (previous_segments, build_type) = match job.index_config.on_disk_state {
            SearchOnDiskState::Backfilling(ref backfill_state) => {
                match &backfill_state.cursor {
                    Some(SearchBackfillCursor {
                        last_segment_ts,
                        table_scan_cursor,
                    }) => (
                        backfill_state.segments.clone(),
                        MultipartBuildType::IncrementalComplete {
                            start_cursor: Some(IndexKeyBytes(table_scan_cursor.to_vec())),
                            last_segment_ts: new_ts.prior_ts(*last_segment_ts)?,
                        },
                    ),
                    None => {
                        // This is the beginning of a backfill!
                        // We need to initialize the backfill with the size of the table at this
                        // snapshot.
                        let tablet = job.index_name.table();
                        let table_name = tx.table_mapping().tablet_name(*tablet)?;
                        let table_namespace = tx.table_mapping().tablet_namespace(*tablet)?;
                        let total_docs = tx.count(table_namespace, &table_name).await?;
                        let mut index_backfill_model = IndexBackfillModel::new(&mut tx);
                        index_backfill_model
                            .initialize_search_index_backfill(job.index_id, total_docs)
                            .await?;
                        (
                            vec![],
                            MultipartBuildType::IncrementalComplete {
                                start_cursor: None,
                                last_segment_ts: new_ts,
                            },
                        )
                    },
                }
            },
            SearchOnDiskState::Backfilled { ref snapshot, .. }
            | SearchOnDiskState::SnapshottedAt(ref snapshot) => {
                match snapshot.data {
                    // We skip rebuilding the index if it is an unknown format because it's very
                    // expensive to compute the whole index in a single segment.
                    SnapshotData::Unknown(_) => {
                        anyhow::bail!("Unknown index format, not rebuilding")
                    },
                    SnapshotData::MultiSegment(ref parts) => {
                        let ts = IndexWorkerMetadataModel::new(&mut tx)
                            .get_fast_forward_ts(snapshot.ts, job.index_id)
                            .await?;
                        (
                            parts.clone(),
                            MultipartBuildType::Partial(new_ts.prior_ts(ts)?),
                        )
                    },
                }
            },
        };
        self.database
            .commit_with_write_source(tx, "search_flusher_initialize_backfill")
            .await?;

        let MultiSegmentBuildResult {
            new_segment,
            updated_previous_segments,
            backfill_result,
        } = self
            .build_multipart_segment_in_dir(
                job,
                &index_path,
                new_ts,
                build_type,
                previous_segments,
                build_index_args,
            )
            .await?;

        let new_segment = if let Some(new_segment) = new_segment {
            Some(self.upload_new_segment(new_segment).await?)
        } else {
            None
        };
        let new_segment_id = new_segment.as_ref().map(|segment| segment.id().to_string());
        let new_segment_stats = new_segment
            .as_ref()
            .map(|segment| segment.statistics())
            .transpose()?;

        let new_and_updated_parts = if let Some(new_segment) = new_segment {
            updated_previous_segments
                .into_iter()
                .chain(iter::once(new_segment))
                .collect()
        } else {
            updated_previous_segments
        };

        let total_stats = new_and_updated_parts
            .iter()
            .map(|segment| segment.statistics())
            .reduce(SegmentStatistics::add)
            .transpose()?
            .unwrap_or_default();
        let data = SnapshotData::MultiSegment(new_and_updated_parts);

        Ok(IndexBuildResult {
            // Backfilled indexes may have a newer timestamp if they're using the new algorithm.
            snapshot_ts: backfill_result
                .as_ref()
                .map(|result| result.new_ts)
                .unwrap_or(new_ts),
            data,
            total_stats,
            new_segment_stats,
            new_segment_id,
            backfill_result,
        })
    }

    async fn build_multipart_segment_in_dir(
        &self,
        job: &IndexBuild<T>,
        index_path: &TempDir,
        snapshot_ts: RepeatableTimestamp,
        build_type: MultipartBuildType,
        previous_segments: Vec<T::Segment>,
        build_index_args: T::BuildIndexArgs,
    ) -> anyhow::Result<MultiSegmentBuildResult<T>> {
        let (tx, rx) = oneshot::channel();
        let index_name = job.index_name.clone();
        let index_path = index_path.path().to_owned();
        let by_id = job.by_id;
        let rate_limit_pages_per_second = job.build_reason.read_max_pages_per_second();
        let spec = job.index_config.spec.clone();
        let params = self.params.clone();
        let handle = self
            .runtime
            .spawn_thread("build_multipart_segment", move || async move {
                let result = Self::build_multipart_segment_on_thread(
                    params,
                    rate_limit_pages_per_second,
                    index_name,
                    by_id,
                    build_type,
                    snapshot_ts,
                    spec,
                    index_path,
                    previous_segments,
                    build_index_args,
                )
                .await;
                let _ = tx.send(result);
            });
        handle.join().await?;
        rx.await?
    }

    async fn build_multipart_segment_on_thread(
        params: Params<RT, T>,
        rate_limit_pages_per_second: NonZeroU32,
        index_name: TabletIndexName,
        by_id: IndexRef,
        build_type: MultipartBuildType,
        snapshot_ts: RepeatableTimestamp,
        spec: T::Spec,
        index_path: PathBuf,
        previous_segments: Vec<T::Segment>,
        build_index_args: T::BuildIndexArgs,
    ) -> anyhow::Result<MultiSegmentBuildResult<T>> {
        let row_rate_limiter = new_rate_limiter(
            params.runtime.clone(),
            Quota::per_second(
                NonZeroU32::new(*DEFAULT_DOCUMENTS_PAGE_SIZE)
                    .and_then(|val| val.checked_mul(rate_limit_pages_per_second))
                    .context("Invalid row rate limit")?,
            ),
        );
        let qdrant_schema = T::new_schema(&spec);

        // Actually create the persistence after new_ts is determined so it covers the
        // full range for incremental doc log walks.
        let persistence;
        // The snapshot the incremental table scan reads from. Declared here so it
        // outlives the lazy document stream that borrows it.
        let table_scan_snapshot;
        // The incremental table scan streams documents lazily rather than
        // materializing them all in memory, so it reports its resume cursor here
        // once it has been fully drained by `build_disk_index` below.
        let mut new_cursor = None;

        let lower_bound_ts: Option<Timestamp>;
        let (documents, incremental_new_ts) = match build_type {
            MultipartBuildType::Partial(last_ts) => {
                lower_bound_ts = Some(*last_ts);
                let range =
                    TimestampRange::new((Bound::Excluded(*last_ts), Bound::Included(*snapshot_ts)));
                let documents = T::load_doc_stream(
                    &params.database,
                    *index_name.table(),
                    range,
                    T::partial_document_order(),
                    &row_rate_limiter,
                );
                (documents, None)
            },
            MultipartBuildType::IncrementalComplete {
                start_cursor,
                last_segment_ts,
            } => {
                let mut tx = params.database.begin_system().await?;
                let tablet_id = *index_name.table();
                let table_number = tx.table_mapping().tablet_number(tablet_id)?;
                let new_ts = tx.begin_timestamp();
                drop(tx);
                persistence = RepeatablePersistence::new(
                    params.reader.clone(),
                    new_ts,
                    params.database.retention_validator(),
                );
                table_scan_snapshot = persistence.read_snapshot(new_ts)?;

                let doc_stream = build_incremental_doc_stream::<T>(
                    &persistence,
                    &table_scan_snapshot,
                    last_segment_ts,
                    new_ts,
                    table_number,
                    tablet_id,
                    by_id,
                    &qdrant_schema,
                    params.limits.incremental_multipart_threshold_bytes,
                    start_cursor,
                    &mut new_cursor,
                );

                lower_bound_ts = Some(*last_segment_ts);
                (doc_stream, Some(new_ts))
            },
        };

        let mut mutable_previous_segments =
            T::download_previous_segments(params.storage.clone(), previous_segments).await?;

        let new_segment = T::build_disk_index(
            &qdrant_schema,
            &index_path,
            documents,
            &mut mutable_previous_segments,
            lower_bound_ts,
            build_index_args,
        )
        .await?;

        let updated_previous_segments =
            T::upload_previous_segments(params.storage, mutable_previous_segments).await?;

        // `build_disk_index` has now fully drained the document stream, so the
        // incremental table scan has recorded where to resume in `new_cursor`.
        let backfill_result = match incremental_new_ts {
            Some(new_ts) => Some(MultiSegmentBackfillResult {
                new_cursor: new_cursor.context("new_cursor not set?")?,
                new_ts,
            }),
            None => None,
        };

        Ok(MultiSegmentBuildResult {
            new_segment,
            updated_previous_segments,
            backfill_result,
        })
    }

    async fn upload_new_segment(&self, new_segment: T::NewSegment) -> anyhow::Result<T::Segment> {
        let (tx, rx) = oneshot::channel();
        let rt = self.runtime.clone();
        let storage = self.storage.clone();
        let handle = self
            .runtime
            .spawn_thread("upload_new_segment", move || async move {
                let result = T::upload_new_segment(&rt, storage, new_segment).await;
                let _ = tx.send(result);
            });
        handle.join().await?;
        rx.await?
    }
}

/// Lazily scan the `by_id` index from `start_cursor` forward, yielding
/// documents until their estimated size reaches `threshold_bytes`.
///
/// Documents are streamed one page at a time rather than collected into a
/// `Vec`, so building a segment only ever holds a single page of documents in
/// memory instead of an entire segment's worth. Once the stream has been fully
/// drained, the cursor to resume the next segment from is written to
/// `new_cursor`.
#[try_stream(ok = DocumentLogEntry, error = anyhow::Error)]
async fn incremental_table_scan_stream<'a, T: SearchIndex>(
    reader: &'a PersistenceSnapshot,
    start_cursor: Option<IndexKeyBytes>,
    by_id: IndexRef,
    tablet_id: TabletId,
    schema: &'a T::Schema,
    threshold_bytes: usize,
    new_cursor: &'a mut Option<TableScanCursor>,
) {
    let page_size = *VECTOR_INDEX_WORKER_PAGE_SIZE;
    let mut total_size = 0u64;
    let mut cursor = TableScanCursor {
        index_key: start_cursor.map(CursorPosition::After),
    };

    'outer: loop {
        let stream = reader.index_scan(by_id, tablet_id, &cursor.interval(), Order::Asc, page_size);
        let page: Vec<_> = stream.take(page_size).try_collect().await?;
        if page.len() < page_size {
            cursor.advance(CursorPosition::End)?;
        } else if let Some((index_key, ..)) = page.last() {
            cursor.advance(CursorPosition::After(index_key.clone()))?;
        }
        if page.is_empty() {
            break;
        }

        let page_len = page.len();
        for (i, (_index_key, latest_doc)) in page.into_iter().enumerate() {
            let developer_doc_id = latest_doc.value.id().developer_id;
            total_size += T::estimate_document_size(schema, &latest_doc.value);

            yield DocumentLogEntry {
                ts: latest_doc.ts,
                id: latest_doc.value.id_with_table_id(),
                value: Some(latest_doc.value),
                prev_ts: latest_doc.prev_ts,
            };

            if total_size >= threshold_bytes as u64 {
                // Reset the cursor to be in the middle of the page we just interrupted
                // processing because we exceeded the size limit for the segment unless we just
                // processed the last document in the page.
                if i != page_len - 1 {
                    cursor.index_key = Some(CursorPosition::After(
                        IndexKey::new(vec![], developer_doc_id).to_bytes(),
                    ));
                }
                break 'outer;
            }
        }
        if matches!(cursor.index_key, Some(CursorPosition::End)) {
            break;
        }
    }

    // Report where to resume now that the scan is exhausted. This runs once the
    // consumer drains the stream past the final yielded document.
    *new_cursor = Some(cursor);
}

/// Build the document stream for incremental backfill: a lazy table scan
/// (bounded by `threshold_bytes`) chained with the doc log for updates to
/// previously-scanned documents if `start_cursor` is present (we're not
/// building the first segment).
///
/// The cursor to resume the table scan from is written to `new_cursor` once
/// the returned stream has been fully consumed.
#[allow(clippy::too_many_arguments)]
fn build_incremental_doc_stream<'a, T: SearchIndex>(
    reader: &'a RepeatablePersistence,
    snapshot: &'a PersistenceSnapshot,
    previous_ts: RepeatableTimestamp,
    new_ts: RepeatableTimestamp,
    table_number: TableNumber,
    tablet_id: TabletId,
    by_id: IndexRef,
    schema: &'a T::Schema,
    threshold_bytes: usize,
    start_cursor: Option<IndexKeyBytes>,
    new_cursor: &'a mut Option<TableScanCursor>,
) -> T::DocStream<'a> {
    let document_stream = incremental_table_scan_stream::<T>(
        snapshot,
        start_cursor.clone(),
        by_id,
        tablet_id,
        schema,
        threshold_bytes,
        new_cursor,
    )
    .boxed();

    let scan_doc_stream = T::table_scan_stream_to_doc_stream(document_stream);

    // If we have a start cursor, walk over the document log to see if
    // there are any updates to documents before and including the cursor that we
    // need to propagate to previous segments.
    if let Some(ref start_index_key) = start_cursor {
        T::walk_document_log_for_updates(
            scan_doc_stream,
            reader,
            tablet_id,
            table_number,
            TimestampRange::new((Bound::Excluded(*previous_ts), Bound::Included(*new_ts))),
            ..=start_index_key.clone(),
        )
    } else {
        scan_doc_stream
    }
}

pub(crate) struct IndexBuild<T: SearchIndex> {
    pub(crate) index_name: TabletIndexName,
    pub(crate) index_id: IndexId,
    pub(crate) by_id: IndexRef,
    pub(crate) metadata_id: ResolvedDocumentId,
    pub(crate) index_config: SearchIndexConfig<T>,
    pub(crate) build_reason: BuildReason,
}

#[derive(Debug)]
pub struct IndexBuildResult<T: SearchIndex> {
    pub snapshot_ts: RepeatableTimestamp,
    pub data: SnapshotData<T::Segment>,
    pub total_stats: T::Statistics,
    pub new_segment_stats: Option<T::Statistics>,
    pub new_segment_id: Option<String>,
    // If this is set, this iteration made progress on backfilling an index
    pub backfill_result: Option<MultiSegmentBackfillResult>,
}

#[derive(Debug)]
pub struct MultiSegmentBuildResult<T: SearchIndex> {
    // This is None only when no new segment was built because all changes were deletes
    new_segment: Option<T::NewSegment>,
    updated_previous_segments: Vec<T::Segment>,
    // This is set only if the build iteration created a segment for a backfilling index
    backfill_result: Option<MultiSegmentBackfillResult>,
}

/// Specifies how documents should be fetched to construct this segment
#[derive(Clone)]
pub enum MultipartBuildType {
    // Build a part
    Partial(RepeatableTimestamp),
    // Build the whole index in parts
    IncrementalComplete {
        /// Index key after which to start the next segment table scan from
        start_cursor: Option<IndexKeyBytes>,
        /// Timestamp from the last segment built during backfilling.
        last_segment_ts: RepeatableTimestamp,
    },
}
