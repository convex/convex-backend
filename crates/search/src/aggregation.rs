use std::{
    cmp::Reverse,
    collections::{
        binary_heap::PeekMut,
        BTreeMap,
        BinaryHeap,
    },
};

use tantivy::Term;

use crate::searcher::{
    PostingListMatch,
    TokenMatch,
};

// Aggregate the top `max_results` posting list matches, sorted by BM25 score,
// creation time, and internal ID in descending order. This is implemented
// using a min-heap so we can efficiently pop the worst match when adding a new
// candidate.
pub struct PostingListMatchAggregator {
    max_results: usize,
    matches: BinaryHeap<Reverse<PostingListMatch>>,
}

impl PostingListMatchAggregator {
    pub fn new(max_results: usize) -> Self {
        Self {
            max_results,
            matches: BinaryHeap::with_capacity(max_results),
        }
    }

    pub fn insert(&mut self, m: PostingListMatch) -> bool {
        let candidate = Reverse(m);
        if self.matches.len() >= self.max_results {
            assert_eq!(self.matches.len(), self.max_results);
            let worst = self.matches.peek_mut().expect("Empty matches?");

            // NB: We reverse the order, so we need to check whether `candidate`
            // is *greater* than the worst candidate.
            if *worst < candidate {
                return false;
            }
            PeekMut::pop(worst);
        }
        self.matches.push(candidate);
        true
    }

    pub fn into_results(self) -> impl Iterator<Item = PostingListMatch> {
        self.matches
            .into_sorted_vec()
            .into_iter()
            .map(|Reverse(m)| m)
    }
}

// Aggregate the top token matches, sorted by prefix (exact matches first),
// term, and token ordinal in *ascending* order. We allow a variable number of
// matches with the invariant of maintaining at most `max_unique_terms` across
// all of the candidate tuples. This uses a max-heap so we can efficiently pop
// the worst candidate.
pub struct TokenMatchAggregator {
    matches: BinaryHeap<TokenMatch>,

    max_unique_terms: usize,

    // Index over `matches` that counts how often each term occurs. The count is always nonzero.
    term_counts: BTreeMap<Term, usize>,
}

impl TokenMatchAggregator {
    pub fn new(max_unique_terms: usize) -> Self {
        Self {
            max_unique_terms,
            matches: BinaryHeap::with_capacity(max_unique_terms),
            term_counts: BTreeMap::new(),
        }
    }

    // Returns whether the match was inserted. If false, the heap was full
    // and the match was worse than the current worse one.
    #[fastrace::trace]
    pub fn insert(&mut self, m: TokenMatch) -> bool {
        let existing_term = self.term_counts.get_mut(&m.term);
        if let Some(count) = existing_term {
            *count += 1;
            self.matches.push(m);
            return true;
        }
        while self.term_counts.len() >= self.max_unique_terms {
            let worst_entry = self
                .matches
                .peek_mut()
                .expect("Empty matches with non-empty term_counts?");

            // If the worst entry is better than us, keep it and tell
            // the caller that their value didn't meet the cutoff.
            if *worst_entry < m {
                return false;
            }
            let worse = PeekMut::pop(worst_entry);
            let existing = self
                .term_counts
                .get_mut(&worse.term)
                .expect("Term missing from term_counts?");
            *existing -= 1;
            if *existing == 0 {
                self.term_counts.remove(&worse.term);
            }
        }
        self.term_counts.insert(m.term.clone(), 1);
        self.matches.push(m);
        true
    }

    pub fn into_results(self) -> Vec<TokenMatch> {
        self.matches.into_sorted_vec()
    }
}
