use std::{
    ops::Deref,
    sync::Arc,
};

use async_lru::async_lru::{
    AsyncLru,
    SizedValue,
};
use common::{
    bounded_thread_pool::BoundedThreadPool,
    id_tracker::StaticIdTracker,
    runtime::Runtime,
    types::SearchIndexMetricLabels,
};
use qdrant_segment::segment::Segment;
use storage::Storage;
use tantivy::Searcher;
use text_search::tracker::{
    load_alive_bitset,
    StaticDeletionTracker,
};
use vector::qdrant_segments::load_disk_segment;

use crate::{
    archive::cache::{
        ArchiveCacheManager,
        CachedArchive,
    },
    disk_index::index_reader_for_directory,
    fragmented_segment::{
        FetchedVectorSegment,
        FragmentedSegmentFetcher,
        FragmentedSegmentStorageKeys,
    },
    searcher::FragmentedTextStorageKeys,
    SearchFileType,
};

#[derive(Clone)]
struct VectorSegmentGenerator<RT: Runtime> {
    fetcher: FragmentedSegmentFetcher<RT>,
    thread_pool: BoundedThreadPool<RT>,
}

/// A loaded qdrant segment together with the archive handles that keep its
/// files on disk for as long as the segment is open.
pub struct SizedVectorSegment {
    segment: Segment,
    _archives: Vec<CachedArchive>,
}

impl SizedVectorSegment {
    pub fn new(segment: Segment, archives: Vec<CachedArchive>) -> Self {
        Self {
            segment,
            _archives: archives,
        }
    }
}

impl SizedValue for SizedVectorSegment {
    fn size(&self) -> u64 {
        1
    }
}

impl Deref for SizedVectorSegment {
    type Target = Segment;

    fn deref(&self) -> &Self::Target {
        &self.segment
    }
}

impl<RT: Runtime> VectorSegmentGenerator<RT> {
    async fn generate_value(
        self,
        storage: Arc<dyn Storage>,
        keys: FragmentedSegmentStorageKeys,
        labels: SearchIndexMetricLabels<'static>,
    ) -> anyhow::Result<SizedVectorSegment> {
        let FetchedVectorSegment { paths, archives } = self
            .fetcher
            .fetch_fragmented_segment(storage, keys, labels)
            .await?;
        let segment = self
            .thread_pool
            .execute(move || load_disk_segment(paths))
            .await??;
        Ok(SizedVectorSegment::new(segment, archives))
    }
}

/// Open qdrant segments, keyed by the storage keys that identify the segment
/// so that re-fetching an evicted archive never creates a second entry for the
/// same segment.
pub(crate) struct VectorSegmentCache<RT: Runtime> {
    lru: AsyncLru<RT, FragmentedSegmentStorageKeys, SizedVectorSegment>,
    segment_generator: VectorSegmentGenerator<RT>,
}

impl<RT: Runtime> VectorSegmentCache<RT> {
    pub fn new(
        rt: RT,
        size: u64,
        fetcher: FragmentedSegmentFetcher<RT>,
        thread_pool: BoundedThreadPool<RT>,
        max_concurrent_searches: usize,
    ) -> Self {
        Self {
            lru: AsyncLru::new(
                rt,
                size,
                max_concurrent_searches,
                200,
                "vector_segment_cache",
            ),
            segment_generator: VectorSegmentGenerator {
                fetcher,
                thread_pool,
            },
        }
    }

    pub async fn get(
        &self,
        storage: Arc<dyn Storage>,
        keys: FragmentedSegmentStorageKeys,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<Arc<SizedVectorSegment>> {
        let labels = labels.to_owned();
        self.lru
            .get(&keys, || {
                self.segment_generator
                    .clone()
                    .generate_value(storage, keys.clone(), labels)
            })
            .await
    }
}

pub enum TextSegment {
    Empty,
    Segment {
        searcher: Searcher,
        deletion_tracker: StaticDeletionTracker,
        id_tracker: StaticIdTracker,
        segment_ord: u32,
        /// Keeps the files behind `searcher` and the trackers on disk.
        _archives: Vec<CachedArchive>,
    },
}

impl SizedValue for TextSegment {
    fn size(&self) -> u64 {
        1
    }
}

#[derive(Clone)]
pub struct TextSegmentGenerator<RT: Runtime> {
    archive_cache: ArchiveCacheManager<RT>,
    thread_pool: BoundedThreadPool<RT>,
}

impl<RT: Runtime> TextSegmentGenerator<RT> {
    async fn generate_value(
        self,
        storage: Arc<dyn Storage>,
        FragmentedTextStorageKeys {
            segment,
            id_tracker,
            deleted_terms_table,
            alive_bitset,
        }: FragmentedTextStorageKeys,
        labels: SearchIndexMetricLabels<'static>,
    ) -> anyhow::Result<TextSegment> {
        let archives = futures::try_join!(
            self.archive_cache.get(
                storage.clone(),
                &segment,
                SearchFileType::Text,
                labels.clone(),
            ),
            self.archive_cache.get_single_file(
                storage.clone(),
                &alive_bitset,
                SearchFileType::TextAliveBitset,
                labels.clone(),
            ),
            self.archive_cache.get_single_file(
                storage.clone(),
                &deleted_terms_table,
                SearchFileType::TextDeletedTerms,
                labels.clone(),
            ),
            self.archive_cache.get_single_file(
                storage,
                &id_tracker,
                SearchFileType::TextIdTracker,
                labels,
            )
        )?;
        let (index_archive, alive_bitset_archive, deleted_terms_archive, id_tracker_archive) =
            archives;
        let load_text_segment = move || async move {
            let reader = index_reader_for_directory(&index_archive).await?;
            let searcher = reader.searcher();
            if searcher.segment_readers().is_empty() {
                return Ok(TextSegment::Empty);
            }
            anyhow::ensure!(searcher.segment_readers().len() == 1);
            let alive_bitset = load_alive_bitset(&alive_bitset_archive)?;
            let deletion_tracker =
                StaticDeletionTracker::load(alive_bitset, &deleted_terms_archive)?;
            let id_tracker = StaticIdTracker::load_from_path(&id_tracker_archive)?;
            let text_segment_reader = TextSegment::Segment {
                searcher,
                deletion_tracker,
                id_tracker,
                segment_ord: 0,
                _archives: vec![
                    index_archive,
                    alive_bitset_archive,
                    deleted_terms_archive,
                    id_tracker_archive,
                ],
            };
            anyhow::Ok(text_segment_reader)
        };
        self.thread_pool.execute_async(load_text_segment).await?
    }
}

/// Open tantivy segments, keyed by the storage keys that identify the segment
/// so that re-fetching an evicted archive never creates a second entry for the
/// same segment.
pub(crate) struct TextSegmentCache<RT: Runtime> {
    lru: AsyncLru<RT, FragmentedTextStorageKeys, TextSegment>,
    text_segment_generator: TextSegmentGenerator<RT>,
}

impl<RT: Runtime> TextSegmentCache<RT> {
    pub fn new(
        rt: RT,
        size: u64,
        archive_cache: ArchiveCacheManager<RT>,
        thread_pool: BoundedThreadPool<RT>,
        max_concurrent_searches: usize,
    ) -> Self {
        Self {
            lru: AsyncLru::new(rt, size, max_concurrent_searches, 200, "text_segment_cache"),
            text_segment_generator: TextSegmentGenerator {
                archive_cache,
                thread_pool,
            },
        }
    }

    pub async fn get(
        &self,
        storage: Arc<dyn Storage>,
        keys: FragmentedTextStorageKeys,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<Arc<TextSegment>> {
        let labels = labels.to_owned();
        self.lru
            .get(&keys, || {
                self.text_segment_generator
                    .clone()
                    .generate_value(storage, keys.clone(), labels)
            })
            .await
    }
}
