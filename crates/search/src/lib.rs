#![feature(iter_from_coroutine, coroutines)]
#![feature(try_blocks)]
#![feature(try_blocks_heterogeneous)]
#![feature(ptr_metadata)]
#![feature(impl_trait_in_assoc_type)]

mod aggregation;
mod archive;
mod constants;
mod convex_query;
pub mod disk_index;
pub mod fragmented_segment;
mod incremental_index;
mod memory_index;
pub mod metrics;
pub mod query;
pub mod searcher;
mod text_index_manager;

use std::{
    cmp,
    collections::{
        BTreeMap,
        BTreeSet,
    },
    sync::Arc,
};

use aggregation::PostingListMatchAggregator;
use anyhow::Context;
use common::{
    bootstrap_model::index::{
        text_index::{
            FragmentedTextSegment,
            TextIndexSpec,
        },
        IndexConfig,
    },
    document::ResolvedDocument,
    index::IndexKeyBytes,
    query::{
        search_value_to_bytes,
        InternalSearch,
        InternalSearchFilterExpression,
        SearchVersion,
    },
    runtime::{
        block_in_place,
        JoinSet,
    },
    types::{
        IndexName,
        SearchIndexMetricLabels,
        Timestamp,
    },
};
use constants::CONVEX_EN_TOKENIZER;
pub use constants::{
    convex_en,
    MAX_CANDIDATE_REVISIONS,
    MAX_FILTER_CONDITIONS,
    MAX_QUERY_TERMS,
};
use convex_query::OrTerm;
use errors::ErrorMetadata;
use indexing::index_registry::Index;
use itertools::Itertools;
use metrics::log_search_token_limit_exceeded;
pub use query::{
    CandidateRevision,
    FilterConditionRead,
    QueryReads,
    QueryResults,
    TextQueryTermRead,
};
use query::{
    RevisionWithKeys,
    TextQueryTerm,
};
use searcher::FragmentedTextStorageKeys;
use storage::Storage;
pub use tantivy::Document as TantivyDocument;
use tantivy::{
    schema::{
        BytesOptions,
        Field,
        IndexRecordOption,
        Schema,
        TextFieldIndexing,
        TextOptions,
        FAST,
    },
    tokenizer::TextAnalyzer,
    Term,
};
use value::{
    values_to_bytes,
    ConvexValue,
    FieldPath,
};

use self::query::{
    CompiledFilterCondition,
    CompiledQuery,
    QueryTerm,
};
pub use self::{
    incremental_index::{
        build_new_segment,
        fetch_term_ordinals_and_remap_deletes,
        NewTextSegment,
        PreviousTextSegments,
        SegmentStatisticsUpdates,
        TextSegmentPaths,
        UpdatableTextSegment,
    },
    memory_index::MemoryTextIndex,
    searcher::{
        Searcher,
        SegmentTermMetadataFetcher,
    },
    text_index_manager::{
        DiskIndex,
        SnapshotInfo,
        TextIndex,
        TextIndexManager,
        TextIndexManagerState,
    },
};
use crate::{
    aggregation::TokenMatchAggregator,
    constants::MAX_UNIQUE_QUERY_TERMS,
    metrics::{
        log_num_segments_searched_total,
        log_text_search_bytes,
    },
    searcher::{
        Bm25Stats,
        PostingListQuery,
        TokenQuery,
    },
};

/// The field ID of the search field in tantivy. DON'T CHANGE THIS!
const SEARCH_FIELD_ID: u32 = 3;

/// The field name for the internal ID field. DON'T CHANGE THIS!
pub const INTERNAL_ID_FIELD_NAME: &str = "internal_id";

/// The field name for the timestamp field. DON'T CHANGE THIS!
pub const TS_FIELD_NAME: &str = "ts";

/// The field name for the creation time field. DON'T CHANGE THIS!
pub const CREATION_TIME_FIELD_NAME: &str = "creation_time";

#[derive(Debug, Clone)]
pub enum DocumentTerm {
    Search { term: Term },
    Filter { term: Term },
}

impl DocumentTerm {
    pub fn term(&self) -> &Term {
        match self {
            Self::Search { term } => term,
            Self::Filter { term } => term,
        }
    }

    pub fn field_id(&self) -> u32 {
        self.term().field().field_id()
    }
}

impl From<DocumentTerm> for Term {
    fn from(doc_term: DocumentTerm) -> Self {
        match doc_term {
            DocumentTerm::Search { term } => term,
            DocumentTerm::Filter { term } => term,
        }
    }
}

pub type EditDistance = u8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextIndexWriteSize(pub u64);

#[derive(Debug)]
pub struct TextSearchResults {
    pub revisions_with_keys: RevisionWithKeys,
    /// Disk index bytes attributed to this query: each segment's size scaled
    /// by the share of its live documents that match the query's filter
    /// conditions. Documents in the memory index are excluded.
    pub filtered_bytes_searched: u64,
}

/// The share of `segment_size_bytes` attributed to a query, given the segment's
/// BM25 statistics for the query's terms. With several filter conditions the
/// matching set is their intersection, whose size is unknown before the
/// posting lists are read, so the smallest filter term's document frequency
/// bounds it from above.
fn filtered_segment_bytes(
    segment_size_bytes: u64,
    stats: &Bm25Stats,
    filter_terms: &[Term],
) -> u64 {
    if filter_terms.is_empty() {
        return segment_size_bytes;
    }
    if stats.num_documents == 0 {
        return 0;
    }
    let min_doc_frequency = filter_terms
        .iter()
        .map(|term| stats.doc_frequencies.get(term).copied().unwrap_or(0))
        .min()
        .unwrap_or(0);
    let matching_docs = cmp::min(min_doc_frequency, stats.num_documents);
    let scaled = u128::from(segment_size_bytes) * u128::from(matching_docs)
        / u128::from(stats.num_documents);
    u64::try_from(scaled).expect("scaled size is at most segment_size_bytes")
}

#[derive(Clone)]
pub struct TantivySearchIndexSchema {
    analyzer: TextAnalyzer,

    internal_id_field: Field,
    ts_field: Field,
    creation_time_field: Field,

    search_field_path: FieldPath,
    pub search_field: Field,

    pub filter_fields: BTreeMap<FieldPath, Field>,

    pub(crate) schema: Schema,
}

impl From<&TantivySearchIndexSchema> for pb::searchlight::SearchIndexConfig {
    fn from(schema: &TantivySearchIndexSchema) -> Self {
        pb::searchlight::SearchIndexConfig {
            search_field_path: Some(schema.search_field_path.clone().into()),
            filter_fields: schema
                .filter_fields
                .keys()
                .cloned()
                .map(|p| p.into())
                .collect::<Vec<_>>(),
        }
    }
}

impl TantivySearchIndexSchema {
    pub fn new(index_config: &TextIndexSpec) -> Self {
        let analyzer = convex_en();

        let mut schema_builder = Schema::builder();

        let internal_id_field = schema_builder.add_bytes_field(INTERNAL_ID_FIELD_NAME, FAST);
        let ts_field = schema_builder.add_u64_field(TS_FIELD_NAME, FAST);
        let creation_time_field = schema_builder.add_f64_field(CREATION_TIME_FIELD_NAME, FAST);

        let search_field_path = index_config.search_field.clone();
        let index_opts = TextFieldIndexing::default()
            .set_tokenizer(CONVEX_EN_TOKENIZER)
            .set_fieldnorms(true)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions);
        let field_opts = TextOptions::default().set_indexing_options(index_opts);

        let field_name = format!("user/search/{}", String::from(search_field_path.clone()));
        let search_field = schema_builder.add_text_field(&field_name, field_opts);

        // NB: It's important that we iterate over `index_config.filter_fields` in
        // sorted order since tantivy assigns field ids in declaration order.
        let mut filter_fields = BTreeMap::new();
        for field_path in &index_config.filter_fields {
            // We store filter fields as the SHA256 hash of their index key.
            let field_name = format!("user/filter/{}", String::from(field_path.clone()));
            let field_opts = BytesOptions::default().set_indexed();
            let filter_field = schema_builder.add_bytes_field(&field_name, field_opts);
            filter_fields.insert(field_path.clone(), filter_field);
        }
        let schema = schema_builder.build();
        Self {
            analyzer,
            internal_id_field,
            ts_field,
            creation_time_field,

            search_field_path,
            search_field,

            filter_fields,
            schema,
        }
    }

    pub fn new_for_index(
        index: &Index,
        printable_index_name: &IndexName,
    ) -> anyhow::Result<TantivySearchIndexSchema> {
        let IndexConfig::Text { ref spec, .. } = index.metadata().config else {
            anyhow::bail!(ErrorMetadata::bad_request(
                "IndexNotASearchIndexError",
                format!("Index {printable_index_name} is not a search index"),
            ));
        };
        Ok(Self::new(spec))
    }

    fn filter_field_bytes(document: &ResolvedDocument, field_path: &FieldPath) -> Vec<u8> {
        let value = document.value().get_path(field_path);
        search_value_to_bytes(value)
    }

    /// This is a pretty wild over-estimate for documents with lots of shared
    /// terms. But it does at least provide some maximum value we can use
    /// when a super rough estimate is sufficient (e.g. capping the maximum
    /// size of a new segment).
    pub fn estimate_size(&self, document: &ResolvedDocument) -> u64 {
        let document_size = if let Some(ConvexValue::String(s)) =
            document.value().get_path(&self.search_field_path)
        {
            s.len()
        } else {
            0
        };
        let mut filter_field_sizes = 0;
        for field_path in self.filter_fields.keys() {
            let value = TantivySearchIndexSchema::filter_field_bytes(document, field_path);
            filter_field_sizes += value.len();
        }
        (document_size + filter_field_sizes) as u64
    }

    pub fn index_into_terms(
        &self,
        document: &ResolvedDocument,
    ) -> anyhow::Result<Vec<DocumentTerm>> {
        let _timer = metrics::index_into_terms_timer();

        let mut doc_terms = vec![];
        if let Some(ConvexValue::String(s)) = document.value().get_path(&self.search_field_path) {
            let mut token_stream = self.analyzer.token_stream(&s[..]);

            while let Some(token) = token_stream.next() {
                metrics::log_text_term(&token.text);

                doc_terms.push(DocumentTerm::Search {
                    term: Term::from_field_text(self.search_field, &token.text),
                });
            }
        }
        for (field_path, tantivy_field) in &self.filter_fields {
            let value = TantivySearchIndexSchema::filter_field_bytes(document, field_path);
            metrics::log_filter_term(&value);
            doc_terms.push(DocumentTerm::Filter {
                term: Term::from_field_bytes(*tantivy_field, &value),
            });
        }
        Ok(doc_terms)
    }

    pub fn index_into_tantivy_document(
        &self,
        document: &ResolvedDocument,
        ts: Timestamp,
    ) -> TantivyDocument {
        let _timer = metrics::index_into_tantivy_document_timer();
        let mut tantivy_document = TantivyDocument::default();

        let internal_id_bytes = Vec::<u8>::from(document.id().internal_id());
        tantivy_document.add_bytes(self.internal_id_field, internal_id_bytes);

        tantivy_document.add_u64(self.ts_field, ts.into());

        let creation_time = document.creation_time();
        tantivy_document.add_f64(self.creation_time_field, creation_time.into());

        if let Some(ConvexValue::String(s)) = document.value().get_path(&self.search_field_path) {
            tantivy_document.add_text(self.search_field, s);
        }
        for (field_path, tantivy_field) in &self.filter_fields {
            let value = TantivySearchIndexSchema::filter_field_bytes(document, field_path);
            tantivy_document.add_bytes(*tantivy_field, value);
        }

        tantivy_document
    }

    pub fn document_lengths(&self, document: &TantivyDocument) -> DocumentLengths {
        let mut search_field = 0;
        if let Some(tantivy::schema::Value::Str(s)) = document.get_first(self.search_field) {
            search_field += s.len();
        }
        let mut filter_fields = BTreeMap::new();
        for (field_path, tantivy_field) in &self.filter_fields {
            if let Some(tantivy::schema::Value::Bytes(b)) = document.get_first(*tantivy_field) {
                filter_fields.insert(field_path.clone(), b.len());
            }
        }
        DocumentLengths {
            search_field,
            filter_fields,
        }
    }

    #[fastrace::trace]
    pub async fn search(
        &self,
        compiled_query: CompiledQuery,
        memory_index: &MemoryTextIndex,
        search_storage: Arc<dyn Storage>,
        segments: Vec<FragmentedTextSegment>,
        disk_index_ts: Timestamp,
        searcher: Arc<dyn Searcher>,
        labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<TextSearchResults> {
        log_num_segments_searched_total(segments.len());
        let labels = labels.to_owned();
        let total_size_bytes: u64 = segments.iter().map(|s| s.size_bytes_total).sum();

        // Step 1: Map the old `CompiledQuery` struct onto `TokenQuery`s.
        let mut token_queries = vec![];
        let num_text_query_terms = compiled_query.text_query.len() as u32;
        for query_term in compiled_query.text_query {
            let query = TokenQuery {
                prefix: query_term.prefix(),
                term: query_term.into_term(),
            };
            token_queries.push(query);
        }
        let mut filter_terms = vec![];
        for CompiledFilterCondition::Must(term) in compiled_query.filter_conditions {
            let query = TokenQuery {
                term: term.clone(),
                prefix: false,
            };
            token_queries.push(query);
            filter_terms.push(term);
        }
        let exist_filter_conditions = !filter_terms.is_empty();
        let num_expected_filter_conditions = filter_terms.len();
        // Bytes charged when the query ends before per-segment statistics are
        // available: a query without filters reads every document's posting
        // lists, so it searches the whole index, and a filtered query whose
        // filter terms match nowhere touches no documents.
        let short_circuit_bytes_searched = if exist_filter_conditions {
            0
        } else {
            total_size_bytes
        };
        let finish = |revisions_with_keys: RevisionWithKeys, filtered_bytes_searched: u64| {
            log_text_search_bytes(
                total_size_bytes,
                filtered_bytes_searched,
                exist_filter_conditions,
            );
            TextSearchResults {
                revisions_with_keys,
                filtered_bytes_searched,
            }
        };

        // Step 2: Execute the token queries across both the memory and disk indexes,
        // and merge the results to get the top terms. Note that we spawn the calls
        // into the joinset *before* calling `block_in_place` so we can make progress
        // while this thread gets transitioned to being a blocking thread.
        let mut token_query_futures = JoinSet::new();
        for segment in &segments {
            let searcher = searcher.clone();
            let search_storage = search_storage.clone();
            let segment = FragmentedTextStorageKeys::from(segment.clone());
            let token_queries = token_queries.clone();
            let labels = labels.clone();
            token_query_futures.spawn("query_tokens", async move {
                searcher
                    .query_tokens(
                        search_storage,
                        segment,
                        token_queries,
                        MAX_UNIQUE_QUERY_TERMS,
                        labels,
                    )
                    .await
            });
        }
        let mut match_aggregator = TokenMatchAggregator::new(MAX_UNIQUE_QUERY_TERMS);
        block_in_place(|| memory_index.query_tokens(&token_queries, &mut match_aggregator))?;

        while let Some(result) = token_query_futures.join_next().await {
            let segment_token_matches = result??;
            anyhow::ensure!(segment_token_matches.is_sorted());
            for m in segment_token_matches {
                // Since each segment returns results in sorted order, we can stop early once we
                // know that we've already seen `MAX_UNIQUE_QUERY_TERMS` better results.
                if !match_aggregator.insert(m) {
                    break;
                }
            }
        }

        // Deduplicate terms, preferring an exact match over a prefix match for each
        // term.
        let mut results_by_term = BTreeMap::new();
        for token_match in match_aggregator.into_results() {
            let sort_key = (token_match.prefix, token_match.token_ord);
            let existing_key = results_by_term.entry(token_match.term).or_insert(sort_key);

            // NB: Since OR and AND queries are on different fields, we can assume their
            // terms are disjoint. Assert this condition here since we're deduplicating
            // terms and taking their minimum `token_ord`, which could potentially lose
            // intersection conditions otherwise.
            let (_, existing_token_ord) = *existing_key;
            let existing_is_intersection = existing_token_ord >= num_text_query_terms;
            let is_intersection = token_match.token_ord >= num_text_query_terms;
            anyhow::ensure!(existing_is_intersection == is_intersection);

            *existing_key = cmp::min(*existing_key, sort_key);
        }
        let terms = results_by_term.keys().cloned().collect_vec();
        // If there are no matches, short-circuit and return an empty result.
        let not_enough_and_tokens_present = results_by_term
            .iter()
            .filter(|(_, (_, token_ord))| *token_ord >= num_text_query_terms)
            .count()
            < num_expected_filter_conditions;
        let no_filter_matches = exist_filter_conditions && not_enough_and_tokens_present;
        if terms.is_empty() || no_filter_matches {
            return Ok(finish(vec![], short_circuit_bytes_searched));
        }

        // Step 3: Given the terms we decided on, query BM25 statistics across all of
        // the indexes and merge their results.
        let mut bm25_futures = JoinSet::new();
        for segment in &segments {
            let searcher = searcher.clone();
            let search_storage = search_storage.clone();
            let segment_size_bytes = segment.size_bytes_total;
            let segment = FragmentedTextStorageKeys::from(segment.clone());
            let terms = terms.clone();
            let labels = labels.clone();
            bm25_futures.spawn("query_bm25_stats", async move {
                let stats = searcher
                    .query_bm25_stats(search_storage, segment, terms, labels)
                    .await?;
                anyhow::Ok((segment_size_bytes, stats))
            });
        }
        let mut bm25_stats = Bm25Stats::empty();
        let mut filtered_bytes_searched = 0u64;
        while let Some(result) = bm25_futures.join_next().await {
            let (segment_size_bytes, segment_bm25_stats) = result??;
            filtered_bytes_searched +=
                filtered_segment_bytes(segment_size_bytes, &segment_bm25_stats, &filter_terms);
            bm25_stats += segment_bm25_stats;
        }
        let bm25_stats =
            block_in_place(|| memory_index.update_bm25_stats(disk_index_ts, &terms, bm25_stats))?;

        // Step 4: Decide on our posting list queries given the previous results.
        let mut or_terms = vec![];
        let mut and_terms = vec![];
        for (term, (prefix, token_ord)) in results_by_term {
            if token_ord >= num_text_query_terms {
                anyhow::ensure!(!prefix);
                and_terms.push(term);
            } else {
                let doc_frequency = *bm25_stats
                    .doc_frequencies
                    .get(&term)
                    .context("Missing term frequency")?;
                let boost = if prefix { 0.5 } else { 1. };
                let or_term = OrTerm {
                    term,
                    doc_frequency,
                    bm25_boost: boost,
                };
                metrics::log_search_term_match(prefix);
                or_terms.push(or_term);
            }
        }

        // or_terms is the set of text tokens that matches our query. and_terms only
        // filters these terms further. So if we have no or_terms, our result is
        // empty regardless of any matching and_terms.
        if or_terms.is_empty() {
            return Ok(finish(vec![], filtered_bytes_searched));
        }

        // Step 5: Execute the posting list query against the memory index's tombstones
        // to know which `InternalId`s to exclude when querying the disk
        // indexes.
        let (prepared_memory_query, query) = block_in_place(|| {
            let prepared_memory_query =
                memory_index.prepare_posting_list_query(&and_terms, &or_terms, &bm25_stats)?;
            let mut deleted_internal_ids = BTreeSet::new();
            if let Some(ref prepared_query) = prepared_memory_query {
                deleted_internal_ids =
                    memory_index.query_tombstones(disk_index_ts, prepared_query)?;
            }
            let query = PostingListQuery {
                deleted_internal_ids,
                num_terms_by_field: bm25_stats.num_terms_by_field,
                num_documents: bm25_stats.num_documents,
                or_terms,
                and_terms,
                max_results: MAX_CANDIDATE_REVISIONS,
            };
            anyhow::Ok((prepared_memory_query, query))
        })?;

        // Step 6: Query the posting lists across the indexes and take the best
        // results.
        let mut posting_list_futures = JoinSet::new();
        for segment in &segments {
            let searcher = searcher.clone();
            let search_storage = search_storage.clone();
            let segment = FragmentedTextStorageKeys::from(segment.clone());
            let query = query.clone();
            let labels = labels.clone();
            posting_list_futures.spawn("query_posting_lists", async move {
                searcher
                    .query_posting_lists(search_storage, segment, query, labels)
                    .await
            });
        }
        let mut match_aggregator = PostingListMatchAggregator::new(MAX_CANDIDATE_REVISIONS);
        if let Some(ref prepared_query) = prepared_memory_query {
            block_in_place(|| {
                memory_index.query_posting_lists(
                    disk_index_ts,
                    prepared_query,
                    &mut match_aggregator,
                )
            })?;
        }
        while let Some(result) = posting_list_futures.join_next().await {
            let segment_matches = result??;
            for m in segment_matches {
                if !match_aggregator.insert(m) {
                    break;
                }
            }
        }

        // Step 7: Convert the matches into the final result format.
        let result = block_in_place(|| {
            let mut result = vec![];
            for m in match_aggregator.into_results() {
                let candidate = CandidateRevision {
                    score: m.bm25_score,
                    id: m.internal_id,
                    ts: m.ts,
                    creation_time: m.creation_time,
                };
                let index_fields = vec![
                    Some(ConvexValue::Float64(-f64::from(m.bm25_score))),
                    Some(ConvexValue::Float64(-f64::from(m.creation_time))),
                    Some(ConvexValue::Bytes(
                        Vec::<u8>::from(m.internal_id)
                            .try_into()
                            .expect("Could not convert internal ID to value"),
                    )),
                ];
                let bytes = values_to_bytes(&index_fields);
                let index_key_bytes = IndexKeyBytes(bytes);
                result.push((candidate, index_key_bytes));
            }
            result
        });
        Ok(finish(result, filtered_bytes_searched))
    }

    fn compile_tokens_with_prefix_last_term(
        search_field: Field,
        tokens: &Vec<String>,
    ) -> anyhow::Result<Vec<QueryTerm>> {
        let mut res = vec![];

        let mut it = tokens.iter().peekable();
        while let Some(text) = it.next() {
            let term = Term::from_field_text(search_field, text);
            anyhow::ensure!(term.as_str().is_some(), "Term was not valid UTF8");

            let is_prefix = it.peek().is_none();
            res.push(QueryTerm::new(term, is_prefix))
        }
        Ok(res)
    }

    pub fn compile(
        &self,
        query: &InternalSearch,
        version: SearchVersion,
    ) -> anyhow::Result<(CompiledQuery, QueryReads)> {
        let timer = metrics::compile_timer();

        let mut search_text: Option<&str> = None;
        let mut filter_conditions = Vec::new();
        let mut filter_reads = Vec::new();
        for filter in query.filters.iter() {
            match filter {
                InternalSearchFilterExpression::Search(field_path, text_query) => {
                    if *field_path != self.search_field_path {
                        anyhow::bail!(ErrorMetadata::bad_request(
                            "IncorrectSearchField",
                            format!(
                                "Search query against {} contains a search filter against {:?}, \
                                 which doesn't match the indexed `searchField` {:?}.",
                                query.printable_index_name()?,
                                field_path,
                                self.search_field_path,
                            ),
                        ))
                    }
                    if search_text.is_some() {
                        anyhow::bail!(ErrorMetadata::bad_request(
                            "DuplicateSearchFiltersError",
                            format!(
                                "Search query against {} contains multiple search filters against \
                                 {field_path:?}. Only one is allowed.",
                                query.printable_index_name()?,
                            )
                        ))
                    }
                    search_text = Some(text_query)
                },
                InternalSearchFilterExpression::Eq(field_path, value) => {
                    let Some(field) = self.filter_fields.get(field_path) else {
                        anyhow::bail!(ErrorMetadata::bad_request(
                            "IncorrectFilterFieldError",
                            format!(
                                "Search query against {} contains an equality filter on \
                                 {field_path:?} but that field isn't indexed for filtering in \
                                 `filterFields`.",
                                query.printable_index_name()?,
                            )
                        ))
                    };
                    let term = Term::from_field_bytes(*field, value);
                    filter_conditions.push(CompiledFilterCondition::Must(term));
                    filter_reads.push(FilterConditionRead::Must(field_path.clone(), value.clone()));
                },
            }
        }

        let Some(search_text) = search_text else {
            anyhow::bail!(ErrorMetadata::bad_request(
                "MissingSearchFilterError",
                format!(
                    "Search query against {} does not contain any search filters. You must \
                     include a search filter like `q.search(\"{:?}\", searchText)`.",
                    query.printable_index_name()?,
                    self.search_field_path,
                )
            ))
        };

        let mut token_stream = self.analyzer.token_stream(search_text);
        let mut tokens = vec![];
        // TODO(CX-5693): Consider how/if we should surface this to developers.
        while tokens.len() < MAX_QUERY_TERMS
            && let Some(token) = token_stream.next()
        {
            tokens.push(token.text.clone());
        }
        if tokens.len() == MAX_QUERY_TERMS && token_stream.next().is_some() {
            log_search_token_limit_exceeded();
        }

        let text_query = match version {
            SearchVersion::V1 => tokens
                .iter()
                .map(|text| {
                    let term = Term::from_field_text(self.search_field, text);
                    anyhow::ensure!(term.as_str().is_some(), "Term was not valid UTF8");
                    Ok(QueryTerm::new(term, false))
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
            // V2 matches the final query term as a prefix; V1 matches every term exactly.
            SearchVersion::V2 => {
                Self::compile_tokens_with_prefix_last_term(self.search_field, &tokens)?
            },
        };

        let text_reads = text_query
            .clone()
            .into_iter()
            .map(|t| {
                anyhow::Ok(TextQueryTermRead::new(
                    self.search_field_path.clone(),
                    TextQueryTerm::try_from(t)?,
                ))
            })
            .collect::<anyhow::Result<_>>()?;

        if filter_conditions.len() > MAX_FILTER_CONDITIONS {
            anyhow::bail!(ErrorMetadata::bad_request(
                "TooManyFilterConditionsInSearchQueryError",
                format!(
                    "Search query against {} has too many filter conditions. Max: {} Actual: {}",
                    query.printable_index_name()?,
                    MAX_FILTER_CONDITIONS,
                    filter_conditions.len()
                )
            ))
        }
        let query = CompiledQuery {
            text_query,
            filter_conditions,
        };
        let reads = QueryReads::new(text_reads, filter_reads.into());
        metrics::log_compiled_query(&query);

        timer.finish();
        Ok((query, reads))
    }
}

pub struct DocumentLengths {
    pub search_field: usize,
    pub filter_fields: BTreeMap<FieldPath, usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchFileType {
    VectorSegment,
    FragmentedVectorSegment,
    VectorDeletedBitset,
    VectorIdTracker,
    Text,
    TextIdTracker,
    TextAliveBitset,
    TextDeletedTerms,
}
