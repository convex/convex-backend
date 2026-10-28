use std::collections::{
    BTreeMap,
    HashSet,
};

use anyhow::Context;
use common::{
    document::{
        CreationTime,
        PackedDocument,
    },
    document_index_keys::{
        SearchIndexKeyValue,
        SearchValueTokens,
    },
    index::IndexKeyBytes,
    query::FilterValue,
    types::{
        SubscriberId,
        TabletIndexName,
        WriteTimestamp,
    },
};
use compact_str::CompactString;
use itertools::Itertools;
use maplit::btreemap;
use tantivy::{
    schema::Field,
    Term,
};
use value::{
    heap_size::{
        HeapSize,
        WithHeapSize,
    },
    ConvexString,
    ConvexValue,
    FieldPath,
    InternalId,
};

use crate::{
    convex_en,
    memory_index::art::ART,
    metrics,
};

/// A search query compiled against a particular `SearchIndexSchema`.
#[derive(Debug, Clone)]
pub struct CompiledQuery {
    pub text_query: Vec<QueryTerm>,
    pub filter_conditions: Vec<CompiledFilterCondition>,
}

impl CompiledQuery {
    /// If true, this query can't match anything
    pub fn is_empty(&self) -> bool {
        self.text_query.is_empty()
    }

    pub fn try_from_text_query_proto(
        value: pb::searchlight::TextQuery,
        search_field: Field,
    ) -> anyhow::Result<CompiledQuery> {
        Ok(Self {
            text_query: value
                .search_terms
                .into_iter()
                .map(|t| QueryTerm::try_from_text_query_term_proto(t, search_field))
                .collect::<anyhow::Result<Vec<_>>>()?,
            filter_conditions: value
                .filter_conditions
                .into_iter()
                // TODO(CX-5481): get rid of this `Term::wrap` call. Need to propagate the Field for these.
                .map(|bytes| CompiledFilterCondition::Must(Term::wrap(bytes)))
                .collect_vec(),
        })
    }
}

impl From<CompiledQuery> for pb::searchlight::TextQuery {
    fn from(value: CompiledQuery) -> Self {
        Self {
            search_terms: value
                .text_query
                .into_iter()
                .map(pb::searchlight::TextQueryTerm::from)
                .collect_vec(),
            filter_conditions: value
                .filter_conditions
                .into_iter()
                .map(|CompiledFilterCondition::Must(term)| term.as_slice().to_vec())
                .collect_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueryTerm {
    term: Term,
    /// If the term is the last in a search query, it can be a prefix match for
    /// typeahead suggestions.
    prefix: bool,
}

impl QueryTerm {
    pub fn new(term: Term, prefix: bool) -> Self {
        QueryTerm { term, prefix }
    }

    pub fn term(&self) -> &Term {
        &self.term
    }

    pub fn into_term(self) -> Term {
        self.term
    }

    pub fn prefix(&self) -> bool {
        self.prefix
    }

    pub fn try_from_text_query_term_proto(
        value: pb::searchlight::TextQueryTerm,
        search_field: Field,
    ) -> anyhow::Result<QueryTerm> {
        let qterm = match value.term_type {
            None => anyhow::bail!("No TermType in QueryTerm"),
            Some(pb::searchlight::text_query_term::TermType::Exact(exact)) => QueryTerm {
                term: Term::from_field_text(search_field, &exact.token),
                prefix: false,
            },
            Some(pb::searchlight::text_query_term::TermType::Prefix(term)) => QueryTerm {
                term: Term::from_field_text(search_field, &term.token),
                prefix: true,
            },
        };
        Ok(qterm)
    }
}

impl TryFrom<QueryTerm> for TextQueryTerm {
    type Error = anyhow::Error;

    fn try_from(value: QueryTerm) -> Result<Self, Self::Error> {
        let term = value
            .term
            .as_str()
            .context("Term was not a string")?
            .to_string();
        let text_query_term = if value.prefix {
            TextQueryTerm::Prefix(term)
        } else {
            TextQueryTerm::Exact(term)
        };
        Ok(text_query_term)
    }
}

impl From<QueryTerm> for pb::searchlight::TextQueryTerm {
    fn from(value: QueryTerm) -> Self {
        let term = value.term();
        let term_str = term.as_str().expect("QueryTerm not a string").to_string();

        let term_type = if value.prefix {
            pb::searchlight::text_query_term::TermType::Prefix(pb::searchlight::PrefixTextTerm {
                token: term_str,
            })
        } else {
            pb::searchlight::text_query_term::TermType::Exact(pb::searchlight::ExactTextTerm {
                token: term_str,
            })
        };
        Self {
            term_type: Some(term_type),
        }
    }
}

#[derive(Debug, Clone)]
pub enum CompiledFilterCondition {
    Must(Term),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateRevision {
    pub score: f32,
    pub id: InternalId,
    pub ts: WriteTimestamp,
    pub creation_time: CreationTime,
}

impl From<CandidateRevision> for pb::searchlight::CandidateRevision {
    fn from(revision: CandidateRevision) -> Self {
        let ts: Option<u64> = match revision.ts {
            WriteTimestamp::Committed(ts) => Some(ts.into()),
            WriteTimestamp::Pending => None,
        };
        let internal_id_bytes = &*revision.id;
        pb::searchlight::CandidateRevision {
            score: revision.score,
            internal_id: internal_id_bytes.to_vec(),
            ts,
            creation_time: revision.creation_time.into(),
        }
    }
}

impl TryFrom<pb::searchlight::CandidateRevision> for CandidateRevision {
    type Error = anyhow::Error;

    fn try_from(proto: pb::searchlight::CandidateRevision) -> Result<Self, Self::Error> {
        let ts = match proto.ts {
            Some(ts) => WriteTimestamp::Committed(ts.try_into()?),
            None => WriteTimestamp::Pending,
        };
        Ok(CandidateRevision {
            score: proto.score,
            id: proto.internal_id.try_into()?,
            ts,
            creation_time: proto.creation_time.try_into()?,
        })
    }
}

pub type RevisionWithKeys = Vec<(CandidateRevision, IndexKeyBytes)>;

pub struct QueryResults {
    pub revisions_with_keys: RevisionWithKeys,
    pub reads: QueryReads,
    /// See `TextSearchResults::filtered_bytes_searched`.
    pub filtered_bytes_searched: u64,
}

impl QueryResults {
    pub fn empty() -> Self {
        Self {
            revisions_with_keys: vec![],
            reads: QueryReads::empty(),
            filtered_bytes_searched: 0,
        }
    }
}

/// A read based on a single token extracted from a text query search.
///
/// A single text query will be split into many parts (tokenized), each part
/// will be combined with the constant metadata (path, prefix etc) into
/// a term, then we track reads based on individual terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextQueryTermRead {
    pub field_path: FieldPath,
    pub term: TextQueryTerm,
}

impl TextQueryTermRead {
    pub fn new(field_path: FieldPath, term: TextQueryTerm) -> Self {
        Self { field_path, term }
    }
}

// For proptest we're using lowercase ascii and a filter to generate tokens so
// that we approximately match what the tokenzier we're using allows. The
// would already have run on these terms prior to this point for production
// code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextQueryTerm {
    Exact(
        String,
    ),
    Prefix(
        String,
    ),
}

impl TextQueryTerm {
    fn token(&self) -> &str {
        match self {
            Self::Exact(token) | Self::Prefix(token) => token,
        }
    }

    fn is_prefix(&self) -> bool {
        matches!(self, Self::Prefix(_))
    }
}

impl HeapSize for TextQueryTerm {
    fn heap_size(&self) -> usize {
        match self {
            TextQueryTerm::Exact(token) | TextQueryTerm::Prefix(token) => token.heap_size(),
        }
    }
}

impl HeapSize for TextQueryTermRead {
    fn heap_size(&self) -> usize {
        self.field_path.heap_size() + self.term.heap_size()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterConditionRead {
    Must(FieldPath, FilterValue),
}

impl HeapSize for FilterConditionRead {
    fn heap_size(&self) -> usize {
        match self {
            FilterConditionRead::Must(p, v) => p.heap_size() + v.heap_size(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueryReads {
    pub text_queries: WithHeapSize<Vec<TextQueryTermRead>>,
    pub filter_conditions: WithHeapSize<Vec<FilterConditionRead>>,

    // State derived from text_queries for more efficient matching with many
    // text subscriptions. Because this is strictly derived, it can always
    // be reconstructed from the simpler text_queries / filter_conditions.
    term_tries: SearchTermTries<()>,
}

impl QueryReads {
    pub fn new(
        text_queries: WithHeapSize<Vec<TextQueryTermRead>>,
        filter_conditions: WithHeapSize<Vec<FilterConditionRead>>,
    ) -> Self {
        let mut term_tries = SearchTermTries::new();
        term_tries.extend((), &text_queries);
        Self {
            text_queries,
            filter_conditions,
            term_tries,
        }
    }
}

impl PartialEq for QueryReads {
    fn eq(&self, other: &Self) -> bool {
        self.text_queries == other.text_queries && self.filter_conditions == other.filter_conditions
    }
}

impl Eq for QueryReads {}

impl HeapSize for QueryReads {
    // TODO(CX-5459): Include term_tries in heap size.
    fn heap_size(&self) -> usize {
        self.text_queries.heap_size() + self.filter_conditions.heap_size()
    }
}

#[derive(Debug, Clone, Default)]
struct SearchTermTries<T: Clone + Ord> {
    terms: BTreeMap<FieldPath, Tries<T>>,
}

impl<T: Clone + Ord> SearchTermTries<T> {
    fn new() -> Self {
        Self {
            terms: BTreeMap::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    #[fastrace::trace]
    fn overlaps_document<'a>(&'a self, document: &'a PackedDocument) -> bool {
        for (path, tries) in self.terms.iter() {
            let Some(ConvexValue::String(document_text)) = document.value().get_path(path) else {
                continue;
            };

            let tokens = tokenize(document_text);
            let mut overlaps = false;
            tries.matching_values(&tokens, &mut |_| overlaps = true);
            if overlaps {
                return true;
            }
        }

        false
    }

    #[fastrace::trace]
    fn overlaps_index_key_value(&self, index_key_value: &SearchIndexKeyValue) -> bool {
        let Some(tokens) = &index_key_value.search_field_value else {
            return false;
        };
        let Some(tries) = self.terms.get(&index_key_value.search_field) else {
            return false;
        };
        let mut overlaps = false;
        tries.matching_values(tokens, &mut |_| overlaps = true);
        overlaps
    }

    fn extend(&mut self, value: T, queries: &WithHeapSize<Vec<TextQueryTermRead>>) {
        for text_query in queries {
            let path = &text_query.field_path;
            let token = text_query.term.token();
            let prefix = text_query.term.is_prefix();
            let art = self
                .terms
                .entry(path.clone())
                .or_insert_with(Tries::new)
                .tries
                .entry(prefix)
                .or_insert_with(ART::new);

            if let Some(value_to_count) = art.get_mut(token) {
                *value_to_count.entry(value.clone()).or_default() += 1
            } else {
                art.insert(token.to_string(), btreemap! { value.clone() => 1});
            }
        }
    }

    fn remove(&mut self, value: T, queries: &WithHeapSize<Vec<TextQueryTermRead>>) {
        for text_query in queries {
            let path = &text_query.field_path;
            let token = text_query.term.token();
            let prefix = text_query.term.is_prefix();
            let value = value.clone();
            let tries = self
                .terms
                .get_mut(path)
                .unwrap_or_else(|| panic!("Missing tries for {path}"));
            let trie = tries
                .tries
                .get_mut(&prefix)
                .unwrap_or_else(|| panic!("Missing trie for prefix={prefix}"));
            let value_to_count = trie
                .get_mut(token)
                .unwrap_or_else(|| panic!("Missing values for token of length {}", token.len()));
            let count = value_to_count
                .entry(value.clone())
                .and_modify(|count| {
                    *count = count
                        .checked_sub(1)
                        .expect("Can't remove more values than were added")
                })
                .or_insert_with(|| panic!("Missing count for value!"));

            if *count == 0 {
                value_to_count.remove(&value);
            }
            if value_to_count.is_empty() {
                trie.remove(token);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Tries<T: Clone> {
    // TODO: Allow ART to store N values:
    // https://github.com/convex/convex/pull/20030/files#r1427222221
    tries: BTreeMap<bool, ART<String, BTreeMap<T, usize>>>,
}

impl<T: Clone> Tries<T> {
    fn new() -> Self {
        Self {
            tries: BTreeMap::new(),
        }
    }
}

impl<T: Clone + Ord> Tries<T> {
    fn matching_values(&self, tokens: &SearchValueTokens, result: &mut impl FnMut(T)) {
        for (prefix, trie) in self.tries.iter() {
            // Prefixing is handled by constructing prefix tokens in ValueTokens (see the
            // notes there), so we can get away with a symmetric search where the dfa's
            // prefix is always set to false.
            tokens.for_each_token(*prefix, |token| {
                if let Some(value) = trie.get(token) {
                    for key in value.keys() {
                        result(key.clone());
                    }
                }
            });
        }
    }
}

impl QueryReads {
    pub fn empty() -> Self {
        QueryReads {
            text_queries: WithHeapSize::default(),
            filter_conditions: WithHeapSize::default(),
            term_tries: SearchTermTries::new(),
        }
    }

    pub fn merge(&mut self, other: Self) {
        self.term_tries.extend((), &other.text_queries);

        self.text_queries.extend(other.text_queries);
        self.filter_conditions.extend(other.filter_conditions);
    }

    #[fastrace::trace]
    pub fn overlaps_document(&self, document: &PackedDocument) -> bool {
        let _timer = metrics::query_reads_overlaps_timer();

        for filter_condition in &self.filter_conditions {
            let FilterConditionRead::Must(field_path, filter_value) = filter_condition;
            let document_value = document.value().get_path(field_path);
            let document_value = FilterValue::from_search_value(document_value.as_ref());
            // If the document doesn't match the filter condition, we can skip checking
            // text query terms
            if document_value != *filter_value {
                metrics::log_query_reads_outcome(false);
                return false;
            }
        }

        // If there are no text queries and all filters match, this counts as an
        // overlap.
        if self.text_queries.is_empty() {
            metrics::log_query_reads_outcome(true);
            return true;
        }
        // If all the filter conditions match and there are text queries, we then check
        // for exact or prefix term matches.
        let is_term_match = self.term_tries.overlaps_document(document);
        metrics::log_query_reads_outcome(is_term_match);
        is_term_match
    }

    #[fastrace::trace]
    pub fn overlaps_search_index_key_value(&self, index_key_value: &SearchIndexKeyValue) -> bool {
        let _timer = metrics::query_reads_overlaps_search_value_timer();

        // Filter out documents that don’t match the filter
        for filter_condition in &self.filter_conditions {
            let FilterConditionRead::Must(field_path, filter_value) = filter_condition;

            let Some(document_value) = index_key_value.filter_values.get(field_path) else {
                // This shouldn’t happen because even if the field doesn’t exist in the
                // document, there is a special `FilterValue` value for
                // undefined. This could happen if the write log entry was created concurrently
                // with index definition changes, but it shouldn’t be a problem.
                metrics::log_missing_filter_value();
                return false;
            };

            if *document_value != *filter_value {
                return false;
            }
        }

        // If there are no text queries and all filters match, this counts as an
        // overlap.
        if self.text_queries.is_empty() {
            metrics::log_query_reads_outcome(true);
            return true;
        }
        // If all the filter conditions match and there are text queries, we then check
        // for exact or prefix term matches.
        let is_term_match = self.term_tries.overlaps_index_key_value(index_key_value);
        metrics::log_query_reads_outcome(is_term_match);
        is_term_match
    }
}

pub struct TextSearchSubscriptions {
    subscriptions: BTreeMap<TabletIndexName, TextSearchSubscription>,
}

#[derive(Default)]
pub struct TextSearchSubscription {
    tries: SearchTermTries<SubscriberId>,
    // TODO: Filter conditions are inefficiently searched, especially in conjunction with text
    // searches. We should eventually optimize this simpler implementation as well.
    filter_conditions: BTreeMap<SubscriberId, Vec<FilterConditionRead>>,
}

impl TextSearchSubscription {
    fn is_empty(&self) -> bool {
        self.tries.is_empty() && self.filter_conditions.is_empty()
    }
}

impl TextSearchSubscriptions {
    pub fn new() -> Self {
        Self {
            subscriptions: BTreeMap::new(),
        }
    }

    pub fn get(&self, index_name: &TabletIndexName) -> Option<&TextSearchSubscription> {
        self.subscriptions.get(index_name)
    }

    pub fn filter_len(&self) -> usize {
        self.subscriptions
            .values()
            .map(|s| s.filter_conditions.len())
            .sum()
    }

    pub fn insert(&mut self, id: SubscriberId, index: &TabletIndexName, reads: &QueryReads) {
        let subscription = self.subscriptions.entry(index.clone()).or_default();
        subscription
            .filter_conditions
            .entry(id)
            .or_default()
            .extend(reads.filter_conditions.to_vec());
        subscription.tries.extend(id, &reads.text_queries);
    }

    pub fn remove(&mut self, id: SubscriberId, index: &TabletIndexName, reads: &QueryReads) {
        let subscription = self
            .subscriptions
            .get_mut(index)
            .unwrap_or_else(|| panic!("Missing subscription for {index}"));
        assert!(subscription.filter_conditions.remove(&id).is_some());
        subscription.tries.remove(id, &reads.text_queries);
        if subscription.is_empty() {
            self.subscriptions.remove(index);
        }
    }

    pub fn add_matches(
        &self,
        subscription: &TextSearchSubscription,
        index_key: &SearchIndexKeyValue,
        notify: &mut impl FnMut(SubscriberId),
    ) {
        self.add_filter_conditions_matches(&subscription.filter_conditions, index_key, notify);
        self.add_term_matches(&subscription.tries, index_key, notify);
    }

    fn add_filter_conditions_matches(
        &self,
        filter_conditions_map: &BTreeMap<SubscriberId, Vec<FilterConditionRead>>,
        index_key: &SearchIndexKeyValue,
        notify: &mut impl FnMut(SubscriberId),
    ) {
        for (subscriber_id, filter_conditions) in filter_conditions_map {
            for FilterConditionRead::Must(field_path, filter_value) in filter_conditions {
                let Some(document_value) = index_key.filter_values.get(field_path) else {
                    metrics::log_missing_filter_value();
                    continue;
                };

                if document_value == filter_value {
                    metrics::log_query_reads_outcome(true);
                    notify(*subscriber_id);
                }
            }
        }
    }

    /// An inverse search where we search document tokens against a trie of read
    /// query terms instead of the more normal trie of the document tokens
    /// against a dfa for each search term.
    ///
    /// This inverse looking search optimizes for cases where the number of
    /// reads/subscriptions is significantly larger than the number of
    /// tokens in the document.
    fn add_term_matches(
        &self,
        tries: &SearchTermTries<SubscriberId>,
        index_key: &SearchIndexKeyValue,
        matches: &mut impl FnMut(SubscriberId),
    ) {
        if let Some(tokens) = &index_key.search_field_value
            && let Some(tries) = tries.terms.get(&index_key.search_field)
        {
            tries.matching_values(tokens, matches);
        };
    }
}

pub fn tokenize(value: ConvexString) -> SearchValueTokens {
    let analyzer = convex_en();

    // Tokenizing the value is expensive, but so is constructing a prefix for
    // every token. So we always keep track of the list of tokens, but we
    // only construct the prefixes for each token if we have at least one search in
    // the read set that uses prefixes.
    let mut token_stream = analyzer.token_stream(&value);
    let mut tokens: HashSet<CompactString> = HashSet::new();
    while token_stream.advance() {
        let text = &token_stream.token().text;
        tokens.insert(text.into());
    }

    SearchValueTokens::from(tokens)
}
