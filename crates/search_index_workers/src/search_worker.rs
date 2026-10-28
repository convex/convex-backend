use std::{
    collections::BTreeMap,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use common::{
    knobs::{
        DATABASE_WORKERS_POLL_INTERVAL,
        INDEX_WORKERS_INITIAL_BACKOFF,
        MULTI_SEGMENT_FULL_SCAN_THRESHOLD_KB,
        SEARCH_COMPACTOR_INITIAL_BACKOFF,
        SEARCH_COMPACTOR_MAX_BACKOFF,
        SEARCH_INDEX_FLUSHER_MAX_BACKOFF,
    },
    persistence::PersistenceReader,
    runtime::{
        Runtime,
        SpawnHandle,
    },
    types::TabletIndexName,
};
use database::{
    Database,
    SearchFlusherWakeSubscriber,
    Token,
};
use futures::{
    future::BoxFuture,
    pin_mut,
    select_biased,
    FutureExt,
};
use search::{
    searcher::SegmentTermMetadataFetcher,
    Searcher,
};
use storage::Storage;
use sync_types::backoff::Backoff;

use crate::{
    retriable_worker::{
        retry_loop_expect_occs_and_overloaded,
        RetriableWorker,
    },
    search_compactor::CompactionConfig,
    text_index_worker::{
        compactor::{
            new_text_compactor,
            TextIndexCompactor,
        },
        flusher::{
            new_text_flusher,
            TextIndexFlusher,
        },
        BuildTextIndexArgs,
        TextIndexMetadataWriter,
    },
    timeout_with_jitter,
    vector_index_worker::{
        compactor::{
            new_vector_compactor,
            VectorIndexCompactor,
        },
        flusher::{
            new_vector_flusher,
            VectorIndexFlusher,
        },
        BuildVectorIndexArgs,
    },
    writer::SearchIndexMetadataWriter,
    FlusherType,
};

/// Builds and compacts text/vector search indexes.
pub struct SearchIndexWorkers {
    handles: Vec<Box<dyn SpawnHandle>>,
}

enum SearchIndexWorker<RT: Runtime> {
    VectorFlusher(VectorIndexFlusher<RT>),
    VectorCompactor(VectorIndexCompactor<RT>),
    TextFlusher(TextIndexFlusher<RT>),
    TextCompactor(TextIndexCompactor<RT>),
}

#[async_trait]
impl<RT: Runtime> RetriableWorker<RT> for SearchIndexWorker<RT> {
    async fn work_loop(
        &mut self,
        name: &'static str,
        rt: &RT,
        db: &Database<RT>,
        backoff: &mut Backoff,
    ) -> anyhow::Result<()> {
        self.work_and_wait_for_changes(name, rt, db, backoff).await
    }
}

impl SearchIndexWorkers {
    pub fn create_and_start<RT: Runtime>(
        runtime: RT,
        database: Database<RT>,
        reader: Arc<dyn PersistenceReader>,
        search_storage: Arc<dyn Storage>,
        searcher: Arc<dyn Searcher>,
        segment_term_metadata_fetcher: Arc<dyn SegmentTermMetadataFetcher>,
    ) -> Self {
        let vector_index_metadata_writer = SearchIndexMetadataWriter::new(
            runtime.clone(),
            database.clone(),
            search_storage.clone(),
            BuildVectorIndexArgs {
                full_scan_threshold_bytes: *MULTI_SEGMENT_FULL_SCAN_THRESHOLD_KB,
            },
        );
        let text_index_metadata_writer = TextIndexMetadataWriter::new(
            runtime.clone(),
            database.clone(),
            search_storage.clone(),
            BuildTextIndexArgs {
                search_storage: search_storage.clone(),
                segment_term_metadata_fetcher: segment_term_metadata_fetcher.clone(),
            },
        );
        let vector_live_flush = retry_loop_expect_occs_and_overloaded(
            "VectorLiveFlusher",
            runtime.clone(),
            database.clone(),
            // Wait a bit since vector needs time to bootstrap. Makes startup logs a bit cleaner.
            Duration::from_secs(5),
            *INDEX_WORKERS_INITIAL_BACKOFF,
            *SEARCH_INDEX_FLUSHER_MAX_BACKOFF,
            SearchIndexWorker::VectorFlusher(new_vector_flusher(
                runtime.clone(),
                database.clone(),
                reader.clone(),
                search_storage.clone(),
                vector_index_metadata_writer.clone(),
                FlusherType::LiveFlush,
            )),
        );
        let vector_backfill_flush = retry_loop_expect_occs_and_overloaded(
            "VectorBackfillFlusher",
            runtime.clone(),
            database.clone(),
            // Wait a bit since vector needs time to bootstrap. Makes startup logs a bit cleaner.
            Duration::from_secs(5),
            *INDEX_WORKERS_INITIAL_BACKOFF,
            *SEARCH_INDEX_FLUSHER_MAX_BACKOFF,
            SearchIndexWorker::VectorFlusher(new_vector_flusher(
                runtime.clone(),
                database.clone(),
                reader.clone(),
                search_storage.clone(),
                vector_index_metadata_writer.clone(),
                FlusherType::Backfill,
            )),
        );
        let vector_compact = retry_loop_expect_occs_and_overloaded(
            "VectorCompactor",
            runtime.clone(),
            database.clone(),
            Duration::ZERO,
            *SEARCH_COMPACTOR_INITIAL_BACKOFF,
            *SEARCH_COMPACTOR_MAX_BACKOFF,
            SearchIndexWorker::VectorCompactor(new_vector_compactor(
                database.clone(),
                searcher.clone(),
                search_storage.clone(),
                CompactionConfig::default(),
                vector_index_metadata_writer,
            )),
        );
        let text_live_flusher = SearchIndexWorker::TextFlusher(new_text_flusher(
            runtime.clone(),
            database.clone(),
            reader.clone(),
            search_storage.clone(),
            segment_term_metadata_fetcher.clone(),
            text_index_metadata_writer.clone(),
            FlusherType::LiveFlush,
        ));
        let text_live_flush = retry_loop_expect_occs_and_overloaded(
            "TextLiveFlusher",
            runtime.clone(),
            database.clone(),
            Duration::ZERO,
            *INDEX_WORKERS_INITIAL_BACKOFF,
            *SEARCH_INDEX_FLUSHER_MAX_BACKOFF,
            text_live_flusher,
        );
        let text_backfill_flusher = SearchIndexWorker::TextFlusher(new_text_flusher(
            runtime.clone(),
            database.clone(),
            reader,
            search_storage.clone(),
            segment_term_metadata_fetcher,
            text_index_metadata_writer.clone(),
            FlusherType::Backfill,
        ));
        let text_backfill_flush = retry_loop_expect_occs_and_overloaded(
            "TextBackfillFlusher",
            runtime.clone(),
            database.clone(),
            Duration::ZERO,
            *INDEX_WORKERS_INITIAL_BACKOFF,
            *SEARCH_INDEX_FLUSHER_MAX_BACKOFF,
            text_backfill_flusher,
        );

        let text_compact = retry_loop_expect_occs_and_overloaded(
            "TextCompactor",
            runtime.clone(),
            database.clone(),
            Duration::ZERO,
            *SEARCH_COMPACTOR_INITIAL_BACKOFF,
            *SEARCH_COMPACTOR_MAX_BACKOFF,
            SearchIndexWorker::TextCompactor(new_text_compactor(
                database,
                searcher,
                search_storage,
                CompactionConfig::default(),
                text_index_metadata_writer.clone(),
            )),
        );

        let vector_backfill_flush_handle =
            runtime.spawn("vector_backfill_flush", vector_backfill_flush);
        let vector_live_flush_handle = runtime.spawn("vector_live_flush", vector_live_flush);
        let vector_compact_handle = runtime.spawn("vector_compact", vector_compact);
        let text_live_flush_handle = runtime.spawn("text_live_flush", text_live_flush);
        let text_backfill_flush_handle = runtime.spawn("text_backfill_flush", text_backfill_flush);
        let text_compact_handle = runtime.spawn("text_compact", text_compact);
        Self {
            handles: vec![
                vector_backfill_flush_handle,
                vector_live_flush_handle,
                vector_compact_handle,
                text_live_flush_handle,
                text_backfill_flush_handle,
                text_compact_handle,
            ],
        }
    }

    pub fn shutdown(&mut self) {
        self.handles.iter_mut().for_each(|handle| handle.shutdown())
    }
}

impl<RT: Runtime> SearchIndexWorker<RT> {
    /// A subscription that fires when an in-memory index passes its soft size
    /// limit, for the workers that flush those indexes.
    fn wake_subscriber(&self) -> Option<SearchFlusherWakeSubscriber> {
        match self {
            Self::VectorFlusher(flusher) => flusher.wake_subscriber(),
            Self::TextFlusher(flusher) => flusher.wake_subscriber(),
            Self::VectorCompactor(_) | Self::TextCompactor(_) => None,
        }
    }

    // The actual work future is fairly large, so box it to avoid consuming
    // steady-state memory.
    fn step(&mut self) -> BoxFuture<'_, anyhow::Result<(BTreeMap<TabletIndexName, u64>, Token)>> {
        match self {
            Self::VectorFlusher(flusher) => flusher.step().boxed(),
            Self::VectorCompactor(compactor) => compactor.step().boxed(),
            Self::TextFlusher(flusher) => flusher.step().boxed(),
            Self::TextCompactor(compactor) => compactor.step().boxed(),
        }
    }

    async fn work_and_wait_for_changes(
        &mut self,
        name: &'static str,
        rt: &RT,
        db: &Database<RT>,
        backoff: &mut Backoff,
    ) -> anyhow::Result<()> {
        let mut wake_subscriber = self.wake_subscriber();
        loop {
            let (metrics, token) = self.step().await?;

            if !metrics.is_empty() {
                // We did some useful work this loop iteration that we expect is committed.
                // There's no point in subscribing, as we'd immediately be woken by our own
                // changes.
                backoff.reset();
                continue;
            }

            // We need to wake up for two reasons:
            // 1. A new or updated index needs to be built - Implement via subscription on
            //    indexes
            // 2. Our soft index size is exceeded so we need to flush to disk - Implement
            //    via a signal from the committer, and via polling as a backstop in case no
            //    commits arrive to trigger the signal.
            let poll = timeout_with_jitter(rt, *DATABASE_WORKERS_POLL_INTERVAL);
            pin_mut!(poll);
            let subscription_fut = db.subscribe_and_wait_for_invalidation(token);
            pin_mut!(subscription_fut);
            let wake_fut = async {
                match &mut wake_subscriber {
                    Some(wake_subscriber) => wake_subscriber.wait_for_wake().await,
                    None => std::future::pending().await,
                }
            };
            pin_mut!(wake_fut);
            select_biased! {
                _ = subscription_fut.fuse() => {
                    tracing::debug!(
                        "{name} resuming after index subscription notification"
                    );
                }
                _ = wake_fut.fuse() => {
                    tracing::debug!(
                        "{name} resuming after an index exceeded the soft size limit"
                    );
                }
                _ = poll.fuse() => {
                    tracing::debug!("{name} starting background checks");
                }
            }
            backoff.reset();
        }
    }
}
