use std::{
    cmp::Ordering,
    collections::{
        BTreeMap,
        BTreeSet,
    },
    ops::{
        Add,
        AddAssign,
    },
    path::Path,
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use anyhow::Context;
use async_trait::async_trait;
use bytesize::ByteSize;
use common::{
    bootstrap_model::index::text_index::FragmentedTextSegment,
    bounded_thread_pool::BoundedThreadPool,
    document::CreationTime,
    runtime::Runtime,
    try_anyhow,
    types::{
        ObjectKey,
        SearchIndexMetricLabels,
        Timestamp,
        WriteTimestamp,
    },
};
use futures::{
    stream,
    StreamExt,
    TryStreamExt,
};
use itertools::Itertools;
use pb::searchlight::{
    fragmented_text_segment_paths::SegmentMetadata,
    FragmentedTextSegmentPaths,
    FragmentedVectorSegmentPaths,
    MultiSegmentMetadata,
    NumTermsByField,
    PostingListQuery as PostingListQueryProto,
    QueryBm25StatsResponse,
    StorageKey,
};
use storage::Storage;
pub use tantivy::Term;
use tantivy::{
    collector::{
        Collector,
        TopDocs,
    },
    query::{
        Bm25StatisticsProvider,
        EnableScoring,
    },
    schema::Field,
    termdict::TermOrdinal,
    InvertedIndexReader,
    SegmentReader,
    TantivyError,
};
use text_search::tracker::StaticDeletionTracker;
use value::InternalId;
use vector::{
    result_merger::merge_vector_results_stream,
    CompiledVectorSearch,
    QdrantSchema,
    VectorIndexType,
    VectorSearchQueryResult,
    VectorSearcher,
};

use super::{
    metrics::{
        self,
        vector_compaction_prefetch_timer,
    },
    segment_cache::{
        SizedVectorSegment,
        TextSegment,
        TextSegmentCache,
        VectorSegmentCache,
    },
};
use crate::{
    aggregation::TokenMatchAggregator,
    archive::cache::ArchiveCacheManager,
    constants::MAX_UNIQUE_QUERY_TERMS,
    convex_query::{
        AliveDocuments,
        ConvexSearchQuery,
        OrTerm,
    },
    disk_index::index_reader_for_directory,
    fragmented_segment::{
        FragmentedSegmentCompactor,
        FragmentedSegmentFetcher,
        FragmentedSegmentPrefetcher,
        FragmentedSegmentStorageKeys,
    },
    incremental_index::fetch_compact_and_upload_text_segment,
    searcher::{
        metrics::{
            text_compaction_searcher_latency_seconds,
            text_query_bm25_searcher_latency_seconds,
            text_query_posting_lists_searcher_latency_seconds,
            text_query_term_ordinals_searcher_timer,
            text_query_tokens_searcher_latency_seconds,
        },
        searchlight_knobs::{
            MAX_CONCURRENT_SEGMENT_COMPACTIONS,
            MAX_CONCURRENT_SEGMENT_FETCHES,
            MAX_CONCURRENT_TEXT_SEARCHES,
            MAX_CONCURRENT_VECTOR_SEARCHES,
            MAX_CONCURRENT_VECTOR_SEGMENT_PREFETCHES,
            MAX_TEXT_LRU_ENTRIES,
            MAX_VECTOR_LRU_SIZE,
            QUEUE_SIZE_MULTIPLIER,
        },
    },
    SearchFileType,
    CREATION_TIME_FIELD_NAME,
    INTERNAL_ID_FIELD_NAME,
    TS_FIELD_NAME,
};

#[async_trait]
pub trait Searcher: VectorSearcher + Send + Sync + 'static {
    async fn query_tokens(
        &self,
        search_storage: Arc<dyn Storage>,
        storage_keys: FragmentedTextStorageKeys,
        queries: Vec<TokenQuery>,
        max_results: usize,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<Vec<TokenMatch>>;

    async fn query_bm25_stats(
        &self,
        search_storage: Arc<dyn Storage>,
        storage_keys: FragmentedTextStorageKeys,
        terms: Vec<Term>,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<Bm25Stats>;

    async fn query_posting_lists(
        &self,
        search_storage: Arc<dyn Storage>,
        storage_keys: FragmentedTextStorageKeys,
        query: PostingListQuery,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<Vec<PostingListMatch>>;

    async fn execute_text_compaction(
        &self,
        search_storage: Arc<dyn Storage>,
        segments: Vec<FragmentedTextStorageKeys>,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<FragmentedTextSegment>;
}

/// The value of a tantivy `Term`, should only be constructed from
/// `term.value_bytes()` or protos that contain the same bytes.
pub type TermValue = Vec<u8>;

/// Map from field to a map of term values to the number of documents containing
/// that term that have been deleted.
#[derive(Default)]
pub struct TermDeletionsByField(pub BTreeMap<Field, FieldDeletions>);

#[derive(Default)]
pub struct FieldDeletions {
    /// The number of documents that have been deleted for each specific term at
    /// a given field.
    pub term_value_to_deleted_documents: BTreeMap<TermValue, u32>,
    /// The total number of non-unique terms deleted from the segment for a
    /// given field.
    pub num_terms_deleted: u64,
}

impl TermDeletionsByField {
    pub fn increment_num_terms_deleted(&mut self, field: Field) {
        let deletions = self.0.entry(field).or_default();
        deletions.num_terms_deleted += 1;
    }

    pub fn increment_num_docs_deleted_for_term(&mut self, field: Field, term_value: TermValue) {
        self.0
            .entry(field)
            .or_default()
            .term_value_to_deleted_documents
            .entry(term_value)
            .and_modify(|num_documents_deleted_for_term| *num_documents_deleted_for_term += 1)
            .or_insert(1);
    }
}

#[async_trait]
pub trait SegmentTermMetadataFetcher: Send + Sync + 'static {
    async fn fetch_term_ordinals(
        &self,
        search_storage: Arc<dyn Storage>,
        segment: ObjectKey,
        field_to_term_values: BTreeMap<Field, Vec<TermValue>>,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<BTreeMap<Field, Vec<TermOrdinal>>>;
}

pub struct SearcherImpl<RT: Runtime> {
    pub(crate) archive_cache: ArchiveCacheManager<RT>,
    rt: RT,
    vector_segment_cache: VectorSegmentCache<RT>,
    text_segment_cache: TextSegmentCache<RT>,
    // A small thread pool whose size is aimed at capping the maximum memory size
    // from concurrent vector loads.
    vector_search_pool: BoundedThreadPool<RT>,
    // A much larger pool for text search where we assume the memory overhead of each query is
    // quite small.
    text_search_pool: BoundedThreadPool<RT>,
    slow_vector_query_threshold_millis: u64,
    require_exact_vector_search: bool,
    fragmented_segment_fetcher: FragmentedSegmentFetcher<RT>,
    fragmented_segment_compactor: FragmentedSegmentCompactor<RT>,
    fragmented_segment_prefetcher: FragmentedSegmentPrefetcher<RT>,
}

impl<RT: Runtime> SearcherImpl<RT> {
    pub fn new<P: AsRef<Path>>(
        local_storage_path: P,
        max_disk_cache_size: u64,
        slow_vector_query_threshold_millis: u64,
        require_exact_vector_search: bool,
        runtime: RT,
    ) -> anyhow::Result<Self> {
        tracing::info!(
            "Searchlight starting, local_storage_path: {} max_size: {}",
            local_storage_path.as_ref().display(),
            ByteSize(max_disk_cache_size)
        );
        // Tokio uses ~500 threads for blocking tasks
        let blocking_thread_pool = BoundedThreadPool::new(runtime.clone(), 1000, 50, "general");
        let archive_cache = ArchiveCacheManager::new(
            local_storage_path,
            max_disk_cache_size,
            blocking_thread_pool.clone(),
            // This actually sets total concurrency for all files in the pool,
            // but since segments are the largest and most critical to limit, this is a tolerable
            // maximum bound.
            *MAX_CONCURRENT_SEGMENT_FETCHES,
            runtime.clone(),
        )?;
        let vector_search_pool = BoundedThreadPool::new(
            runtime.clone(),
            *MAX_CONCURRENT_VECTOR_SEARCHES * *QUEUE_SIZE_MULTIPLIER,
            *MAX_CONCURRENT_VECTOR_SEARCHES,
            "vector",
        );
        let text_search_pool = BoundedThreadPool::new(
            runtime.clone(),
            *MAX_CONCURRENT_TEXT_SEARCHES * *QUEUE_SIZE_MULTIPLIER,
            *MAX_CONCURRENT_TEXT_SEARCHES,
            "text",
        );
        let fragmented_segment_fetcher = FragmentedSegmentFetcher::new(archive_cache.clone());
        let fragmented_segment_compactor = FragmentedSegmentCompactor::new(
            runtime.clone(),
            fragmented_segment_fetcher.clone(),
            BoundedThreadPool::new(
                runtime.clone(),
                *MAX_CONCURRENT_SEGMENT_COMPACTIONS * *QUEUE_SIZE_MULTIPLIER,
                *MAX_CONCURRENT_SEGMENT_COMPACTIONS,
                "vector_compactor",
            ),
        );
        let fragmented_segment_prefetcher = FragmentedSegmentPrefetcher::new(
            runtime.clone(),
            fragmented_segment_fetcher.clone(),
            *MAX_CONCURRENT_VECTOR_SEGMENT_PREFETCHES,
        );
        Ok(Self {
            rt: runtime.clone(),
            archive_cache: archive_cache.clone(),
            vector_segment_cache: VectorSegmentCache::new(
                runtime.clone(),
                *MAX_VECTOR_LRU_SIZE,
                fragmented_segment_fetcher.clone(),
                vector_search_pool.clone(),
                *MAX_CONCURRENT_VECTOR_SEARCHES,
            ),
            text_segment_cache: TextSegmentCache::new(
                runtime,
                *MAX_TEXT_LRU_ENTRIES,
                archive_cache.clone(),
                text_search_pool,
                *MAX_CONCURRENT_TEXT_SEARCHES,
            ),
            vector_search_pool,
            text_search_pool: blocking_thread_pool,
            slow_vector_query_threshold_millis,
            require_exact_vector_search,
            fragmented_segment_fetcher,
            fragmented_segment_compactor,
            fragmented_segment_prefetcher,
        })
    }

    pub fn queue_prefetch_segments(
        &self,
        search_storage: Arc<dyn Storage>,
        paths: Vec<FragmentedVectorSegmentPaths>,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<()> {
        let paths: Vec<FragmentedSegmentStorageKeys> = paths
            .into_iter()
            .map(|paths| paths.try_into())
            .try_collect()?;
        self.fragmented_segment_prefetcher
            .queue_prefetch(search_storage, paths, labels)
    }

    /// A blocking prefetch for compaction where we explicitly want to stop the
    /// compaction process until the prefetch finishes.
    ///
    /// In contrast queue_prefetch_segments is intended for cases where we
    /// explicitly do not want to block the calling process and instead want the
    /// prefetches to finish asynchronously after the request completes.
    async fn prefetch_segment(
        &self,
        search_storage: Arc<dyn Storage>,
        segment: common::bootstrap_model::index::vector_index::FragmentedVectorSegment,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<()> {
        let timer = vector_compaction_prefetch_timer();
        let paths = FragmentedSegmentStorageKeys {
            segment: segment.segment_key,
            id_tracker: segment.id_tracker_key,
            deleted_bitset: segment.deleted_bitset_key,
        };
        self.fragmented_segment_fetcher
            .stream_fetch_fragmented_segments(search_storage, vec![paths], labels)
            .try_collect::<Vec<_>>()
            .await?;
        timer.finish();
        Ok(())
    }

    async fn load_fragmented_segment(
        &self,
        search_storage: Arc<dyn Storage>,
        fragment: FragmentedVectorSegmentPaths,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<Arc<SizedVectorSegment>> {
        let keys: FragmentedSegmentStorageKeys = fragment.try_into()?;
        self.vector_segment_cache
            .get(search_storage, keys, labels)
            .await
    }

    async fn vector_query_segment(
        &self,
        schema: QdrantSchema,
        query: CompiledVectorSearch,
        overfetch_delta: u32,
        segment: Arc<SizedVectorSegment>,
    ) -> anyhow::Result<Vec<VectorSearchQueryResult>> {
        let slow_query_threshold = self.slow_vector_query_threshold_millis;
        let require_exact = self.require_exact_vector_search;
        let search = move || {
            let timer = metrics::vector_schema_query_timer();
            let start = Instant::now();
            let result = schema.search(
                &segment,
                query,
                overfetch_delta,
                slow_query_threshold,
                require_exact,
            );
            let query_duration = Instant::now().duration_since(start);
            if query_duration > Duration::from_millis(slow_query_threshold) {
                tracing::warn!(
                    "Slow vector query, duration: {}ms, results: {:?} schema: {:?}, \
                     overfetch_delta: {overfetch_delta}",
                    query_duration.as_millis(),
                    result.as_ref().map(|value| value.len()),
                    schema,
                )
            }
            timer.finish();
            result
        };
        self.vector_search_pool.execute(search).await?
    }

    async fn load_text_segment(
        &self,
        storage: Arc<dyn Storage>,
        text_storage_keys: FragmentedTextStorageKeys,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<Arc<TextSegment>> {
        self.text_segment_cache
            .get(storage, text_storage_keys, labels)
            .await
    }
}

#[async_trait]
impl<RT: Runtime> Searcher for SearcherImpl<RT> {
    #[fastrace::trace]
    async fn query_tokens(
        &self,
        search_storage: Arc<dyn Storage>,
        storage_keys: FragmentedTextStorageKeys,
        queries: Vec<TokenQuery>,
        max_results: usize,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<Vec<TokenMatch>> {
        let timer = text_query_tokens_searcher_latency_seconds();
        let text_segment = self
            .load_text_segment(search_storage, storage_keys, labels)
            .await?;
        let query = move || Self::query_tokens_impl(text_segment, queries, max_results);
        let resp = self.text_search_pool.execute(query).await??;
        timer.finish();
        Ok(resp)
    }

    #[fastrace::trace]
    async fn query_bm25_stats(
        &self,
        search_storage: Arc<dyn Storage>,
        storage_keys: FragmentedTextStorageKeys,
        terms: Vec<Term>,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<Bm25Stats> {
        let timer = text_query_bm25_searcher_latency_seconds();
        let text_segment = self
            .load_text_segment(search_storage, storage_keys, labels)
            .await?;
        let query = move || Self::query_bm25_stats_impl(text_segment, terms);
        let resp = self.text_search_pool.execute(query).await??;
        timer.finish();
        Ok(resp)
    }

    #[fastrace::trace]
    async fn query_posting_lists(
        &self,
        search_storage: Arc<dyn Storage>,
        storage_keys: FragmentedTextStorageKeys,
        query: PostingListQuery,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<Vec<PostingListMatch>> {
        let timer = text_query_posting_lists_searcher_latency_seconds();
        let text_segment = self
            .load_text_segment(search_storage, storage_keys, labels)
            .await?;
        let query = move || Self::query_posting_lists_impl(text_segment, query);
        let resp = self.text_search_pool.execute(query).await??;
        timer.finish();
        Ok(resp)
    }

    #[fastrace::trace]
    async fn execute_text_compaction(
        &self,
        search_storage: Arc<dyn Storage>,
        segments: Vec<FragmentedTextStorageKeys>,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<FragmentedTextSegment> {
        let timer = text_compaction_searcher_latency_seconds();
        let result = fetch_compact_and_upload_text_segment(
            &self.rt,
            search_storage,
            self.archive_cache.clone(),
            self.text_search_pool.clone(),
            segments,
            labels,
        )
        .await;
        timer.finish();
        result
    }
}

#[async_trait]
impl<RT: Runtime> SegmentTermMetadataFetcher for SearcherImpl<RT> {
    #[fastrace::trace]
    async fn fetch_term_ordinals(
        &self,
        search_storage: Arc<dyn Storage>,
        segment: ObjectKey,
        field_to_term_values: BTreeMap<Field, Vec<TermValue>>,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<BTreeMap<Field, Vec<TermOrdinal>>> {
        let timer = text_query_term_ordinals_searcher_timer();
        let segment_archive = self
            .archive_cache
            .get(search_storage, &segment, SearchFileType::Text, labels)
            .await?;
        let reader = index_reader_for_directory(&segment_archive).await?;
        let searcher = reader.searcher();

        // Multisegment indexes only write to one segment.
        let num_readers = searcher.segment_readers().len();
        anyhow::ensure!(num_readers == 1, "Expected 1 reader, but got {num_readers}");
        let segment = searcher.segment_reader(0);

        let mut field_to_term_ordinals = BTreeMap::new();
        for (field, term_values) in field_to_term_values {
            let mut term_ordinals = vec![];
            let inverted_index = segment.inverted_index(field)?;
            for term_value in term_values {
                let term_dict = inverted_index.terms();
                let term_ord = term_dict
                    .term_ord(term_value)?
                    .context("Segment must contain term")?;
                term_ordinals.push(term_ord);
            }
            field_to_term_ordinals.insert(field, term_ordinals);
        }
        timer.finish();
        Ok(field_to_term_ordinals)
    }
}

#[async_trait]
impl<RT: Runtime> VectorSearcher for SearcherImpl<RT> {
    async fn execute_multi_segment_vector_query(
        &self,
        search_storage: Arc<dyn Storage>,
        fragments: Vec<FragmentedVectorSegmentPaths>,
        schema: QdrantSchema,
        query: CompiledVectorSearch,
        overfetch_delta: u32,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<Vec<VectorSearchQueryResult>> {
        let timer = metrics::vector_query_timer(VectorIndexType::MultiSegment);
        let results: anyhow::Result<Vec<VectorSearchQueryResult>> = try_anyhow!({
            let total_segments = fragments.len();
            let query_capacity = (query.limit + overfetch_delta) as usize;

            // Use shared result merger to merge results from all segments using
            // a min-heap approach. This avoids storing all intermediate results
            // from parallel segment fetches in-memory.
            let results_stream = stream::iter(fragments)
                .map(|fragment| {
                    self.load_fragmented_segment(search_storage.clone(), fragment, labels.clone())
                })
                // Limit the parallel downloads a bit, we don't want to start and finish all
                // downloads at the same time. We want to be downloading and working with
                // segments concurrently.
                .buffer_unordered(4)
                .and_then(|segment| {
                    self.vector_query_segment(
                        schema.clone(),
                        query.clone(),
                        overfetch_delta,
                        segment,
                    )
                });
            let results: Vec<VectorSearchQueryResult> =
                merge_vector_results_stream(results_stream, query_capacity).await?;
            tracing::debug!(
                "Finished querying {} vectors from {total_segments} segment(s) to get {} limit + \
                 overfetch results",
                results.len(),
                query.limit + overfetch_delta
            );
            results
        });

        timer.finish(results.is_ok());
        results
    }

    async fn execute_vector_compaction(
        &self,
        search_storage: Arc<dyn Storage>,
        segments: Vec<FragmentedVectorSegmentPaths>,
        dimension: usize,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<common::bootstrap_model::index::vector_index::FragmentedVectorSegment> {
        let labels = labels.to_owned();
        let segment = self
            .fragmented_segment_compactor
            .compact(segments, dimension, search_storage.clone(), labels.clone())
            .await?;

        self.prefetch_segment(search_storage, segment.clone(), labels)
            .await?;
        Ok(segment)
    }
}

impl<RT: Runtime> SearcherImpl<RT> {
    #[fastrace::trace]
    fn query_tokens_impl(
        text_segment: Arc<TextSegment>,
        queries: Vec<TokenQuery>,
        max_results: usize,
    ) -> anyhow::Result<Vec<TokenMatch>> {
        match text_segment.as_ref() {
            TextSegment::Empty => Ok(vec![]),
            TextSegment::Segment {
                searcher,
                deletion_tracker,
                id_tracker: _,
                segment_ord,
                ..
            } => {
                let segment = searcher.segment_reader(*segment_ord);
                anyhow::ensure!(max_results <= MAX_UNIQUE_QUERY_TERMS);

                // Deterministically choose the best terms in the segment for the query
                // tokens. Each query token `q_i` totally orders the matching terms by
                // `TokenMatch` order: its exact match first, then (for prefix tokens) the
                // terms it prefixes, ties broken by term contents and then `i`. Logically,
                // we merge these streams and take tuples until we have seen `max_results`
                // unique terms. `TokenMatchAggregator` performs the merge; each token stops
                // producing tuples once the aggregator rejects one, since the rest of its
                // stream sorts after it.
                let mut match_aggregator = TokenMatchAggregator::new(max_results);

                for (token_ord, token_query) in queries.into_iter().enumerate() {
                    let token_ord = token_ord as u32;
                    Self::visit_top_terms_for_query(
                        segment,
                        deletion_tracker,
                        token_ord,
                        &token_query,
                        &mut match_aggregator,
                    )?;
                }
                Ok(match_aggregator.into_results())
            },
        }
    }

    #[fastrace::trace]
    fn visit_top_terms_for_query(
        segment: &SegmentReader,
        deletion_tracker: &StaticDeletionTracker,
        token_ord: u32,
        query: &TokenQuery,
        results: &mut TokenMatchAggregator,
    ) -> anyhow::Result<()> {
        let field = query.term.field();
        let inverted_index = segment.inverted_index(field)?;
        let term_dict = inverted_index.terms();
        let query_bytes = query.term.value_bytes();

        if let Some(term_ord) = term_dict.term_ord(query_bytes)?
            && deletion_tracker.doc_frequency(field, term_dict, term_ord)? > 0
        {
            let m = TokenMatch {
                prefix: false,
                term: query.term.clone(),
                token_ord,
            };
            if !results.insert(m) {
                return Ok(());
            }
        }
        if !query.prefix {
            return Ok(());
        }

        // The term dictionary streams keys in lexicographic order, so the terms
        // `query_bytes` prefixes are a contiguous run starting at `query_bytes`.
        anyhow::ensure!(
            query.term.as_str().is_some(),
            "Prefix query on non-string term"
        );
        let mut term_stream = term_dict.range().gt(query_bytes).into_stream()?;
        while term_stream.advance() {
            let match_term_bytes = term_stream.key();
            if !match_term_bytes.starts_with(query_bytes) {
                break;
            }
            if deletion_tracker.doc_frequency(field, term_dict, term_stream.term_ord())? == 0 {
                continue;
            }
            let match_str = std::str::from_utf8(match_term_bytes)?;
            let m = TokenMatch {
                prefix: true,
                term: Term::from_field_text(field, match_str),
                token_ord,
            };
            if !results.insert(m) {
                break;
            }
        }
        Ok(())
    }

    #[fastrace::trace]
    fn query_bm25_stats_impl(
        text_segment: Arc<TextSegment>,
        terms: Vec<Term>,
    ) -> anyhow::Result<Bm25Stats> {
        match text_segment.as_ref() {
            TextSegment::Empty => Ok(Bm25Stats::empty()),
            TextSegment::Segment {
                searcher,
                deletion_tracker,
                id_tracker: _,
                segment_ord,
                ..
            } => {
                let segment = searcher.segment_reader(*segment_ord);
                let fields: BTreeSet<Field> = terms.iter().map(|t| t.field()).collect();
                let inverted_index_by_field: BTreeMap<_, _> = fields
                    .into_iter()
                    .map(|field| {
                        anyhow::Ok::<(Field, Arc<InvertedIndexReader>)>((
                            field,
                            segment.inverted_index(field)?,
                        ))
                    })
                    .try_collect()?;

                let num_documents = deletion_tracker.num_alive_docs() as u64;
                let mut doc_frequencies = BTreeMap::new();
                for term in terms {
                    let field = term.field();
                    let term_dict = inverted_index_by_field
                        .get(&field)
                        .context("Missing inverted index for field")?
                        .terms();
                    if let Some(term_ord) = term_dict.term_ord(term.value_bytes())? {
                        let doc_freq =
                            deletion_tracker.doc_frequency(field, term_dict, term_ord)?;
                        doc_frequencies.insert(term, doc_freq);
                    } else {
                        // Terms may not exist in one or more segments of a multi segment text
                        // index. Terms skipped here will (should!) be found
                        // in another segment and counted there. We could
                        // probably get away with skipping this entry
                        // entirely, but it seems more explicit to
                        // specifically return the count for every term.
                        doc_frequencies.insert(term, 0);
                    }
                }

                let num_terms_by_field = inverted_index_by_field
                    .into_iter()
                    .map(|(field, inverted_index)| {
                        let total_num_tokens = inverted_index.total_num_tokens();
                        let num_terms_deleted = deletion_tracker.num_terms_deleted(field);
                        let num_terms = total_num_tokens
                            .checked_sub(num_terms_deleted)
                            // Tantivy's total_num_tokens count is only approximate, so we can't guarantee this won't underflow.
                            .unwrap_or_else(|| {
                                tracing::warn!(
                                    "num_terms underflowed for field {field:?} in query_bm_25_stats_impl, subtracted num_terms_deleted: {num_terms_deleted} from \
                                    total_num_tokens: {total_num_tokens}"
                                );
                                0
                            });
                        Ok((field, num_terms))
                    })
                    .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
                let stats = Bm25Stats {
                    num_terms_by_field,
                    num_documents,
                    doc_frequencies,
                };
                if stats.is_empty() {
                    tracing::warn!("Empty BM25 stats");
                }
                Ok(stats)
            },
        }
    }

    #[fastrace::trace]
    fn query_posting_lists_impl(
        text_segment: Arc<TextSegment>,
        query: PostingListQuery,
    ) -> anyhow::Result<Vec<PostingListMatch>> {
        match text_segment.as_ref() {
            TextSegment::Empty => Ok(vec![]),
            TextSegment::Segment {
                searcher,
                deletion_tracker,
                id_tracker,
                segment_ord,
                ..
            } => {
                let stats_provider = StatsProvider {
                    num_terms_by_field: query.num_terms_by_field,
                    num_documents: query.num_documents,
                    doc_frequencies: query
                        .or_terms
                        .iter()
                        .map(|t| (t.term.clone(), t.doc_frequency))
                        .collect(),
                };

                let memory_deleted = query
                    .deleted_internal_ids
                    .iter()
                    .filter_map(|internal_id| id_tracker.lookup(internal_id.0))
                    .collect();
                let alive_documents = AliveDocuments {
                    memory_deleted,
                    segment_alive_bitset: deletion_tracker.alive_bitset().clone(),
                };

                let search_query =
                    ConvexSearchQuery::new(query.or_terms, query.and_terms, alive_documents);
                let enable_scoring =
                    EnableScoring::enabled_from_statistics_provider(&stats_provider, searcher);
                let search_weight = search_query.weight(enable_scoring)?;

                let collector = TopDocs::with_limit(query.max_results);
                let segment = searcher.segment_reader(*segment_ord);
                let segment_results = collector.collect_segment(&*search_weight, 0, segment)?;

                let fast_fields = segment.fast_fields();
                let internal_ids = fast_fields.bytes(INTERNAL_ID_FIELD_NAME)?;
                let timestamps = fast_fields.u64(TS_FIELD_NAME)?;
                let creation_times = fast_fields.f64(CREATION_TIME_FIELD_NAME)?;

                let mut results = Vec::with_capacity(segment_results.len());
                for (bm25_score, doc_address) in segment_results {
                    let internal_id = internal_ids.get_bytes(doc_address.doc_id).try_into()?;

                    let ts = Timestamp::try_from(timestamps.get_val(doc_address.doc_id))?;
                    let creation_time =
                        CreationTime::try_from(creation_times.get_val(doc_address.doc_id))?;
                    let posting_list_match = PostingListMatch {
                        internal_id,
                        ts: WriteTimestamp::Committed(ts),
                        creation_time,
                        bm25_score,
                    };
                    results.push(posting_list_match);
                }

                anyhow::ensure!(results.len() <= query.max_results);

                // TODO: The collector sorts only on score, unlike PostingListMatchAggregator,
                // so we have to resort the results here, sweeping this nondeterminism
                // under the rug.
                results.sort_by(|a, b| a.cmp(b).reverse());

                Ok(results)
            },
        }
    }
}

struct StatsProvider {
    num_terms_by_field: BTreeMap<Field, u64>,
    num_documents: u64,
    doc_frequencies: BTreeMap<Term, u64>,
}

impl Bm25StatisticsProvider for StatsProvider {
    fn total_num_tokens(&self, field: Field) -> tantivy::Result<u64> {
        self.num_terms_by_field
            .get(&field)
            .copied()
            .context("Missing search field")
            .map_err(|e| TantivyError::InvalidArgument(e.to_string()))
    }

    fn total_num_docs(&self) -> tantivy::Result<u64> {
        Ok(self.num_documents)
    }

    fn doc_freq(&self, term: &Term) -> tantivy::Result<u64> {
        self.doc_frequencies.get(term).copied().ok_or_else(|| {
            tantivy::TantivyError::InvalidArgument(format!("Term not found: {term:?}"))
        })
    }
}

#[derive(Clone, Debug)]
pub struct TokenQuery {
    pub term: Term,
    pub prefix: bool,
}

impl TryFrom<pb::searchlight::TokenQuery> for TokenQuery {
    type Error = anyhow::Error;

    fn try_from(value: pb::searchlight::TokenQuery) -> Result<Self, Self::Error> {
        Ok(TokenQuery {
            term: Term::wrap(value.term.context("Missing term")?),
            prefix: value.prefix.context("Missing prefix")?,
        })
    }
}

impl TryFrom<TokenQuery> for pb::searchlight::TokenQuery {
    type Error = anyhow::Error;

    fn try_from(value: TokenQuery) -> Result<Self, Self::Error> {
        Ok(pb::searchlight::TokenQuery {
            term: Some(value.term.as_slice().to_vec()),
            prefix: Some(value.prefix),
        })
    }
}

#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub struct TokenMatch {
    pub prefix: bool,
    pub term: Term,
    pub token_ord: u32,
}

impl TryFrom<pb::searchlight::TokenMatch> for TokenMatch {
    type Error = anyhow::Error;

    fn try_from(value: pb::searchlight::TokenMatch) -> Result<Self, Self::Error> {
        Ok(TokenMatch {
            prefix: value.prefix.context("Missing prefix")?,
            term: Term::wrap(value.tantivy_bytes.context("Missing term")?),
            token_ord: value.token_ord.context("Missing token_ord")?,
        })
    }
}

impl TryFrom<TokenMatch> for pb::searchlight::TokenMatch {
    type Error = anyhow::Error;

    fn try_from(value: TokenMatch) -> Result<Self, Self::Error> {
        Ok(pb::searchlight::TokenMatch {
            prefix: Some(value.prefix),
            tantivy_bytes: Some(value.term.as_slice().to_vec()),
            token_ord: Some(value.token_ord),
        })
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct FragmentedTextStorageKeys {
    pub segment: ObjectKey,
    pub id_tracker: ObjectKey,
    pub deleted_terms_table: ObjectKey,
    pub alive_bitset: ObjectKey,
}

impl TryFrom<FragmentedTextSegmentPaths> for FragmentedTextStorageKeys {
    type Error = anyhow::Error;

    fn try_from(value: FragmentedTextSegmentPaths) -> Result<Self, Self::Error> {
        let from_path = |path: Option<StorageKey>| {
            path.map(|p| p.storage_key)
                .context("Missing path!")?
                .try_into()
        };
        let segment = from_path(value.segment)?;
        let segment_metadata = value.segment_metadata.context("Missing segment metadata")?;
        let SegmentMetadata::MultiSegment(MultiSegmentMetadata {
            id_tracker,
            deleted_terms_table,
            alive_bitset,
        }) = segment_metadata;
        let id_tracker = from_path(id_tracker)?;
        let deleted_terms_table = from_path(deleted_terms_table)?;
        let alive_bitset = from_path(alive_bitset)?;
        Ok(Self {
            segment,
            id_tracker,
            deleted_terms_table,
            alive_bitset,
        })
    }
}

impl From<FragmentedTextStorageKeys> for FragmentedTextSegmentPaths {
    fn from(value: FragmentedTextStorageKeys) -> Self {
        let segment_from = |segment_key: ObjectKey| {
            Some(StorageKey {
                storage_key: segment_key.into(),
            })
        };

        Self {
            segment: segment_from(value.segment),
            segment_metadata: Some(SegmentMetadata::MultiSegment(MultiSegmentMetadata {
                id_tracker: segment_from(value.id_tracker),
                deleted_terms_table: segment_from(value.deleted_terms_table),
                alive_bitset: segment_from(value.alive_bitset),
            })),
        }
    }
}

impl From<FragmentedTextSegment> for FragmentedTextStorageKeys {
    fn from(value: FragmentedTextSegment) -> Self {
        Self {
            segment: value.segment_key,
            id_tracker: value.id_tracker_key,
            deleted_terms_table: value.deleted_terms_table_key,
            alive_bitset: value.alive_bitset_key,
        }
    }
}

impl TryFrom<FragmentedVectorSegmentPaths> for FragmentedSegmentStorageKeys {
    type Error = anyhow::Error;

    fn try_from(value: FragmentedVectorSegmentPaths) -> Result<Self, Self::Error> {
        let from_path = |path: Option<StorageKey>| {
            path.map(|p| p.storage_key)
                .context("Missing path!")?
                .try_into()
        };
        Ok(FragmentedSegmentStorageKeys {
            segment: from_path(value.segment)?,
            id_tracker: from_path(value.id_tracker)?,
            deleted_bitset: from_path(value.deleted_bitset)?,
        })
    }
}

#[derive(Debug)]
pub struct Bm25Stats {
    /// The total number of terms in the inverted index for the field.
    pub num_terms_by_field: BTreeMap<Field, u64>,
    /// The total number of documents in the segment.
    pub num_documents: u64,
    /// The number of documents that contain each term.
    pub doc_frequencies: BTreeMap<Term, u64>,
}

impl Bm25Stats {
    pub fn empty() -> Self {
        Self {
            num_terms_by_field: BTreeMap::new(),
            num_documents: 0,
            doc_frequencies: BTreeMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.num_terms_by_field.is_empty()
            && self.num_documents == 0
            && self.doc_frequencies.is_empty()
    }
}

impl Add for Bm25Stats {
    type Output = Self;

    fn add(mut self, rhs: Self) -> Self::Output {
        self.add_assign(rhs);
        self
    }
}

impl AddAssign for Bm25Stats {
    fn add_assign(&mut self, rhs: Self) {
        for (field, num_terms) in rhs.num_terms_by_field {
            *self.num_terms_by_field.entry(field).or_default() += num_terms;
        }
        self.num_documents += rhs.num_documents;
        for (term, count) in rhs.doc_frequencies {
            *self.doc_frequencies.entry(term).or_default() += count;
        }
    }
}

impl From<Bm25Stats> for QueryBm25StatsResponse {
    fn from(
        Bm25Stats {
            num_terms_by_field,
            num_documents,
            doc_frequencies,
        }: Bm25Stats,
    ) -> Self {
        let num_terms_by_field = num_terms_by_field
            .into_iter()
            .map(|(field, num_terms)| NumTermsByField {
                field: Some(field.field_id()),
                num_terms: Some(num_terms),
            })
            .collect();
        let num_documents = Some(num_documents);
        let doc_frequencies = doc_frequencies
            .into_iter()
            .map(|(term, frequency)| pb::searchlight::DocFrequency {
                term: Some(term.as_slice().to_vec()),
                frequency: Some(frequency),
            })
            .collect();
        QueryBm25StatsResponse {
            num_terms_by_field,
            num_documents,
            doc_frequencies,
        }
    }
}

impl TryFrom<QueryBm25StatsResponse> for Bm25Stats {
    type Error = anyhow::Error;

    fn try_from(
        QueryBm25StatsResponse {
            num_terms_by_field,
            num_documents,
            doc_frequencies,
        }: QueryBm25StatsResponse,
    ) -> Result<Self, Self::Error> {
        let num_terms_by_field = num_terms_by_field
            .into_iter()
            .map(|NumTermsByField { field, num_terms }| {
                anyhow::Ok::<(Field, u64)>((
                    Field::from_field_id(field.context("Missing field")?),
                    num_terms.context("Missing num_terms")?,
                ))
            })
            .try_collect()?;
        let doc_frequencies = doc_frequencies
            .into_iter()
            .map(|df| {
                anyhow::Ok::<(Term, u64)>((
                    Term::wrap(df.term.context("Missing term")?),
                    df.frequency.context("Missing frequency")?,
                ))
            })
            .try_collect()?;
        Ok(Bm25Stats {
            num_terms_by_field,
            num_documents: num_documents.context("Missing num_documents")?,
            doc_frequencies,
        })
    }
}

#[derive(Clone, Debug)]
pub struct PostingListQuery {
    pub deleted_internal_ids: BTreeSet<InternalId>,

    pub num_terms_by_field: BTreeMap<Field, u64>,
    pub num_documents: u64,

    pub or_terms: Vec<OrTerm>,
    pub and_terms: Vec<Term>,

    pub max_results: usize,
}

impl TryFrom<PostingListQueryProto> for PostingListQuery {
    type Error = anyhow::Error;

    fn try_from(
        PostingListQueryProto {
            deleted_internal_ids,
            num_terms_by_field,
            num_documents,
            or_terms,
            and_terms,
            max_results,
        }: PostingListQueryProto,
    ) -> Result<Self, Self::Error> {
        let num_terms_by_field = num_terms_by_field
            .into_iter()
            .map(|NumTermsByField { field, num_terms }| {
                anyhow::Ok::<(Field, u64)>((
                    Field::from_field_id(field.context("Missing field")?),
                    num_terms.context("Missing num_terms")?,
                ))
            })
            .try_collect()?;
        let deleted_internal_ids = deleted_internal_ids
            .into_iter()
            .map(|b| InternalId::try_from(&b[..]))
            .collect::<anyhow::Result<_>>()?;
        let or_terms = or_terms.into_iter().map(|t| t.try_into()).try_collect()?;
        let and_terms = and_terms.into_iter().map(Term::wrap).collect();
        Ok(PostingListQuery {
            deleted_internal_ids,
            num_terms_by_field,
            num_documents: num_documents.context("Missing num_documents")?,
            or_terms,
            and_terms,
            max_results: max_results.context("Missing max_results")? as usize,
        })
    }
}

impl TryFrom<PostingListQuery> for PostingListQueryProto {
    type Error = anyhow::Error;

    fn try_from(
        PostingListQuery {
            deleted_internal_ids,
            num_terms_by_field,
            num_documents,
            or_terms,
            and_terms,
            max_results,
        }: PostingListQuery,
    ) -> Result<Self, Self::Error> {
        let deleted_internal_ids = deleted_internal_ids
            .into_iter()
            .map(|id| id.into())
            .collect();
        let num_terms_by_field = num_terms_by_field
            .into_iter()
            .map(|(field, num_terms)| NumTermsByField {
                field: Some(field.field_id()),
                num_terms: Some(num_terms),
            })
            .collect();
        let or_terms = or_terms.into_iter().map(|t| t.try_into()).try_collect()?;
        let and_terms = and_terms
            .into_iter()
            .map(|t| t.as_slice().to_vec())
            .collect();
        Ok(PostingListQueryProto {
            deleted_internal_ids,
            num_terms_by_field,
            num_documents: Some(num_documents),
            or_terms,
            and_terms,
            max_results: Some(max_results as u32),
        })
    }
}

#[derive(Debug)]
pub struct PostingListMatch {
    pub internal_id: InternalId,
    pub ts: WriteTimestamp,
    pub creation_time: CreationTime,
    pub bm25_score: f32,
}

impl Ord for PostingListMatch {
    fn cmp(&self, other: &Self) -> Ordering {
        self.bm25_score
            .total_cmp(&other.bm25_score)
            .then(self.creation_time.cmp(&other.creation_time))
            .then(self.internal_id.cmp(&other.internal_id))
            .then(self.ts.cmp(&other.ts))
    }
}

impl PartialOrd for PostingListMatch {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for PostingListMatch {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for PostingListMatch {}

impl TryFrom<pb::searchlight::PostingListMatch> for PostingListMatch {
    type Error = anyhow::Error;

    fn try_from(value: pb::searchlight::PostingListMatch) -> Result<Self, Self::Error> {
        Ok(PostingListMatch {
            internal_id: InternalId::try_from(
                &value.internal_id.context("Missing internal id")?[..],
            )?,
            ts: match value.ts {
                Some(pb::searchlight::posting_list_match::Ts::Committed(ts)) => {
                    WriteTimestamp::Committed(ts.try_into()?)
                },
                Some(pb::searchlight::posting_list_match::Ts::Pending(())) => {
                    WriteTimestamp::Pending
                },
                _ => anyhow::bail!("Missing ts field"),
            },
            creation_time: value
                .creation_time
                .context("Missing creation_time")?
                .try_into()?,
            bm25_score: value.bm25_score.context("Missing bm25_score")?,
        })
    }
}

impl TryFrom<PostingListMatch> for pb::searchlight::PostingListMatch {
    type Error = anyhow::Error;

    fn try_from(value: PostingListMatch) -> Result<Self, Self::Error> {
        Ok(pb::searchlight::PostingListMatch {
            internal_id: Some(value.internal_id.into()),
            ts: match value.ts {
                WriteTimestamp::Committed(ts) => Some(
                    pb::searchlight::posting_list_match::Ts::Committed(ts.into()),
                ),
                WriteTimestamp::Pending => {
                    Some(pb::searchlight::posting_list_match::Ts::Pending(()))
                },
            },
            creation_time: Some(value.creation_time.into()),
            bm25_score: Some(value.bm25_score),
        })
    }
}
