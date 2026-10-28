use std::{
    iter,
    mem,
    ops::{
        AddAssign,
        SubAssign,
    },
    sync::Arc,
};

use sucds::{
    mii_sequences::{
        EliasFano,
        EliasFanoBuilder,
    },
    Serializable,
};
use tantivy::{
    fieldnorm::FieldNormReader,
    Score,
};
use xorf::{
    BinaryFuse16,
    Filter,
};

use super::{
    bitset64::Bitset64,
    PreparedMemoryPostingListQuery,
};
use crate::memory_index::term_table::TermId;

/// Memory-efficient structure for storing the terms in a document, including
/// terms within both search and filter fields. This structure is conceptually a
/// `BTreeMap<TermId, Frequency>`.
///
/// Provides efficient methods for accessing terms and their frequencies within
/// a field. This is useful for calculating BM25 statistics and other ranking
/// statistics for search.
///
/// # Construction
/// In search, a `Document` is conceptually a `BTreeMap<FieldId, Vec<TermId>>`
/// where each `FieldId` corresponds to either a filter field or search field.
/// Since `TermId`s are unique across `FieldId`s (due to uniqueness of how
/// filter field terms are represented), we can flatten this into one
/// `BTreeMap<TermId, Frequency>`.
///
/// To represent this `BTreeMap`, we start by splitting it into two parallel
/// lists:
///
/// terms: btree_map.keys().collect();   // sorted by term_id!
/// freqs: btree_map.values().collect(); // we store this as a sorted list of
/// cumulative frequencies
///
/// Then, we can compress these two lists, assuming that (1) the difference
/// between adjacent term IDs in `terms` is small and (2) the frequencies are
/// generally small themselves. Both of these arrays are monotonically
/// increasing integer arrays, so we use `sucds`'s Elias Fano array datatype.
///
/// # Queries
/// The simplest queries `TermList::iter_*` iterate over the terms or
/// frequencies, fully decompressing the integer streams.
///
/// The real benefit of this sorted approach comes from doing sorted set
/// intersections between the term list and a list of terms in a query. This
/// algorithm is exactly the same as for posting lists in Tantivy but inverted:
/// Instead of querying a posting list of DocIds, we're querying a term list of
/// TermIds.
///
/// # Term filter acceleration
/// When a query or intersection term is sufficiently rare, it's very useful to
/// use a hash-based approximate set data structure to quickly determine that a
/// document doesn't match. To this end, we layer on top a `BinaryFuse16` data
/// structure (more modern version of a Bloom filter).
///
/// Current benchmarks show this as helpful in addition to the sorted integer
/// lists, but it's a bit surprising to me (sujayakar, 3/4/23) that this is an
/// improvement. At the very minimum, I think we'll want to segment our
/// documents by their termlist size and configure the filter differently
/// to keep a constant false positive rate.
#[derive(Clone, Debug)]
pub struct TermList {
    // The `EliasFano` structures are really large (336 bytes on the stack), so
    // box the `NonemptyTermList` - containing two - to avoid stack overflow.
    // https://github.com/kampersanda/sucds/issues/96
    //
    // Furthermore, since term lists are immutable, we use Arc to make cloning
    // cheaper because we store these in `imbl::OrdMap`s.
    inner: Option<Arc<NonemptyTermList>>,
}

#[derive(Debug)]
struct NonemptyTermList {
    term_filter: BinaryFuse16,

    terms: EliasFano,
    cumulative_freqs: EliasFano,
}

impl TermList {
    pub fn new(mut term_ids: Vec<TermId>) -> anyhow::Result<Self> {
        // Step 1: Accumulate parallel lists of the unique sorted term IDs and
        // their frequencies.
        term_ids.sort_unstable();

        let Some(greatest_term_id) = term_ids.last() else {
            return Ok(TermList { inner: None });
        };
        let mut terms_builder =
            EliasFanoBuilder::new(*greatest_term_id as usize + 1, term_ids.len())?;

        let mut cumulative_freqs_builder =
            EliasFanoBuilder::new(term_ids.len() + 1, term_ids.len())?;
        let mut freqs_sum = 0;

        let mut term_u64s = Vec::with_capacity(term_ids.len());

        let mut prev_term = None;

        for term_id in term_ids {
            if let Some((prev_term, ref mut prev_freq)) = prev_term
                && prev_term == term_id
            {
                *prev_freq += 1;
                continue;
            }
            if let Some((prev_term, prev_freq)) = prev_term.take() {
                terms_builder.push(prev_term as usize)?;
                term_u64s.push(prev_term as u64);

                freqs_sum += prev_freq;
                cumulative_freqs_builder.push(freqs_sum as usize)?;
            }
            prev_term = Some((term_id, 1));
        }
        if let Some((term_id, term_freq)) = prev_term {
            terms_builder.push(term_id as usize)?;
            term_u64s.push(term_id as u64);

            freqs_sum += term_freq;
            cumulative_freqs_builder.push(freqs_sum as usize)?;
        }

        let term_filter = BinaryFuse16::try_from(&term_u64s).map_err(anyhow::Error::msg)?;
        let terms = terms_builder.build().enable_rank();
        let cumulative_freqs = cumulative_freqs_builder.build();

        let inner = NonemptyTermList {
            term_filter,
            terms,
            cumulative_freqs,
        };
        Ok(Self {
            inner: Some(Arc::new(inner)),
        })
    }

    pub fn iter_terms(&self) -> impl Iterator<Item = TermId> + '_ {
        iter::from_coroutine(
            #[coroutine]
            move || {
                let Some(ref inner) = self.inner else {
                    return;
                };
                for t in inner.terms.iter(0) {
                    yield t as TermId;
                }
            },
        )
    }

    pub fn iter_freqs(&self) -> impl Iterator<Item = u32> + '_ {
        iter::from_coroutine(
            #[coroutine]
            move || {
                let Some(ref inner) = self.inner else {
                    return;
                };
                let mut current = 0;
                for cumulative_freq in inner.cumulative_freqs.iter(0) {
                    yield (cumulative_freq - current) as u32;
                    current = cumulative_freq;
                }
            },
        )
    }

    pub fn iter_term_freqs(&self) -> impl Iterator<Item = (TermId, u32)> + '_ {
        self.iter_terms().zip(self.iter_freqs())
    }

    pub fn matches(&self, query: &PreparedMemoryPostingListQuery) -> bool {
        let Some(ref inner) = self.inner else {
            return false;
        };
        if !inner.term_filter_matches(query) {
            return false;
        }
        // Build up a bitset of which terms match.
        let mut matching_terms = Bitset64::new();
        for (i, _) in inner.term_matches(&query.sorted_terms) {
            matching_terms.insert(i);
        }

        // Check that all of the intersection bits and any of the union bits are set.
        let all_intersection =
            matching_terms.intersect(query.intersection_terms) == query.intersection_terms;
        let any_union = !matching_terms.intersect(query.union_terms).is_empty();
        all_intersection && any_union
    }

    pub fn matches_with_score(
        &self,
        query: &PreparedMemoryPostingListQuery,
        num_search_tokens: u32,
    ) -> Option<Score> {
        let inner = self.inner.as_ref()?;
        if !inner.term_filter_matches(query) {
            return None;
        }

        let mut score = 0.;
        let fieldnorm_id = FieldNormReader::fieldnorm_to_id(num_search_tokens);

        // Build up a bitset of which terms match.
        let mut matching_terms = Bitset64::new();
        for (i, pos) in inner.term_matches(&query.sorted_terms) {
            matching_terms.insert(i);
            if query.union_terms.contains(i) {
                let term_freq = inner
                    .cumulative_freqs
                    .delta(pos)
                    .expect("term position missing from cumulative_freqs");
                let union_rank = query.union_terms.rank(i);
                let bm25_weight = &query.union_weights[union_rank];
                score += bm25_weight.score(fieldnorm_id, term_freq as u32);
            }
        }
        // Check that all of the intersection bits and any of the union bits are set.
        let all_intersection =
            matching_terms.intersect(query.intersection_terms) == query.intersection_terms;
        let any_union = !matching_terms.intersect(query.union_terms).is_empty();
        (all_intersection && any_union).then_some(score)
    }

    pub fn heap_allocations(&self) -> TermListBytes {
        let Some(ref inner) = self.inner else {
            return TermListBytes::ZERO;
        };
        TermListBytes {
            fingerprints_bytes: inner.term_filter.fingerprints.len() * mem::size_of::<u16>(),
            terms_bytes: inner.terms.size_in_bytes(),
            freqs_bytes: inner.cumulative_freqs.size_in_bytes(),
        }
    }
}

impl NonemptyTermList {
    // Check if the query approximately matches the document set with the
    // possibility of false positives.
    fn term_filter_matches(&self, query: &PreparedMemoryPostingListQuery) -> bool {
        let any_intersection_missing = query
            .intersection_terms()
            .map(|t| t as u64)
            .any(|term_id| !self.term_filter.contains(&term_id));
        if any_intersection_missing {
            return false;
        }
        query
            .union_terms()
            .map(|t| t as u64)
            .any(|term_id| self.term_filter.contains(&term_id))
    }

    // Iterate over all term IDs in a query set that intersect with the document's
    // termlist.
    //
    // Yields: (index into sorted terms, index into term list)
    //
    fn term_matches<'a>(
        &'a self,
        sorted_terms: &'a [TermId],
    ) -> impl Iterator<Item = (usize, usize)> + 'a {
        iter::from_coroutine(
            #[coroutine]
            move || {
                for (i, term) in sorted_terms.iter().cloned().enumerate() {
                    // Find the position of `term` as its "rank": The number of
                    // terms in the sequence that are less than `term`.
                    let Some(rank) = self.terms.rank(term as usize) else {
                        break;
                    };
                    // If `term` is larger than all other terms in the sequence,
                    // it can be `terms.len()` and then return `None`. here.
                    let Some(current_term) = self.terms.select(rank) else {
                        break;
                    };
                    if current_term == term as usize {
                        yield (i, rank);
                    }
                }
            },
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TermListBytes {
    pub fingerprints_bytes: usize,
    pub terms_bytes: usize,
    pub freqs_bytes: usize,
}

impl AddAssign for TermListBytes {
    fn add_assign(&mut self, rhs: Self) {
        self.fingerprints_bytes += rhs.fingerprints_bytes;
        self.terms_bytes += rhs.terms_bytes;
        self.freqs_bytes += rhs.freqs_bytes;
    }
}

impl SubAssign for TermListBytes {
    fn sub_assign(&mut self, rhs: Self) {
        self.fingerprints_bytes -= rhs.fingerprints_bytes;
        self.terms_bytes -= rhs.terms_bytes;
        self.freqs_bytes -= rhs.freqs_bytes;
    }
}

impl TermListBytes {
    pub const ZERO: Self = Self {
        fingerprints_bytes: 0,
        terms_bytes: 0,
        freqs_bytes: 0,
    };

    pub fn bytes(&self) -> usize {
        self.fingerprints_bytes + self.terms_bytes + self.freqs_bytes
    }
}
