use std::{
    mem,
    ops::Deref,
    sync::Arc,
};

use imbl_slab::{
    Slab,
    SlabKey,
};
use ref_cast::RefCast;
use tantivy::Term;

use crate::{
    aggregation::TokenMatchAggregator,
    memory_index::{
        art::ART,
        small_slice::SmallSlice,
    },
    searcher::{
        TokenMatch,
        TokenQuery,
    },
};

pub type TermId = SlabKey;

#[derive(Debug, Clone, RefCast)]
#[repr(transparent)]
struct TermRef(Term);

impl AsRef<[u8]> for TermRef {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[derive(Clone, Debug)]
pub struct TermEntry {
    term: SmallSlice,
    refcount: u32,
}

/// Stores filter and search terms. Cheap to Clone via copy-on-write data
/// structures.
#[derive(Clone, Debug)]
pub struct TermTable {
    terms: Slab<TermEntry>,
    index: ART<TermRef, SlabKey>,
    size: usize,
}

impl TermTable {
    pub(crate) fn new() -> Self {
        Self {
            terms: Slab::new(),
            index: ART::new(),
            size: 0,
        }
    }

    pub fn incref(&mut self, term: &Term) -> TermId {
        if let Some(term_id) = self.index.get_mut(TermRef::ref_cast(term)) {
            let entry = self
                .terms
                .get_mut(*term_id)
                .expect("Invalid search term ID");
            entry.refcount += 1;
            return *term_id;
        }
        let term_ref = TermRef(term.clone());
        let term_slice = SmallSlice::from(term_ref.as_ref());
        let entry = TermEntry {
            term: term_slice,
            refcount: 1,
        };

        self.size += entry.term.heap_allocations();
        self.size += mem::size_of::<TermEntry>();
        self.size += mem::size_of::<(SmallSlice, SlabKey)>();

        let term_id = self.terms.alloc(entry);
        self.index.insert(term_ref, term_id);
        term_id
    }

    pub fn decref(&mut self, term_id: TermId, count: u32) {
        let entry = self.terms.get_mut(term_id).expect("Invalid search term ID");
        assert!(entry.refcount >= count);
        entry.refcount -= count;
        if entry.refcount == 0 {
            let entry = self.terms.free(term_id);
            let term_bytes = entry.term.deref();
            let term = Term::wrap(Vec::from(term_bytes));
            assert_eq!(self.index.remove(&TermRef(term)), Some(term_id));

            self.size -= entry.term.heap_allocations();
            self.size -= mem::size_of::<TermEntry>();
            self.size -= mem::size_of::<(Arc<[u8]>, SlabKey)>();
        }
    }

    pub fn get(&self, term: &Term) -> Option<TermId> {
        self.index.get(TermRef::ref_cast(term)).cloned()
    }

    /// Terms that start with `prefix` (including `prefix` itself), in
    /// lexicographic order. Term bytes begin with the field and type, so only
    /// string terms of `prefix`'s field match.
    pub fn get_prefix<'a>(&'a self, prefix: &'a Term) -> impl Iterator<Item = (TermId, Term)> + 'a {
        self.index
            .iter_prefix(prefix.as_slice())
            .map(|(term_id, bytes)| (*term_id, Term::wrap(bytes)))
    }

    /// Visits the exact match for `query.term` and then, for prefix queries,
    /// the terms it prefixes in lexicographic order. This is ascending
    /// `TokenMatch` order, so the first match `results` rejects ends the scan.
    #[fastrace::trace]
    pub fn visit_top_terms_for_query(
        &self,
        token_ord: u32,
        query: &TokenQuery,
        results: &mut TokenMatchAggregator,
    ) -> anyhow::Result<()> {
        if self.get(&query.term).is_some() {
            let m = TokenMatch {
                prefix: false,
                term: query.term.clone(),
                token_ord,
            };
            if !results.insert(m) {
                return Ok(());
            }
        }
        if query.prefix {
            anyhow::ensure!(
                query.term.as_str().is_some(),
                "Prefix query on non-string term"
            );
            for (_, match_term) in self.get_prefix(&query.term) {
                if match_term == query.term {
                    continue;
                }
                let m = TokenMatch {
                    prefix: true,
                    term: match_term,
                    token_ord,
                };
                if !results.insert(m) {
                    break;
                }
            }
        }
        Ok(())
    }

    pub fn refcount(&self, term_id: TermId) -> u32 {
        self.terms.get(term_id).expect("Invalid term ID").refcount
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn consistency_check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.terms.len() == self.index.len());
        let mut expected_size = 0;
        for term_id in self.index.iter_values() {
            let Some(entry) = self.terms.get(*term_id) else {
                anyhow::bail!("Missing term for {term_id}");
            };
            anyhow::ensure!(entry.refcount > 0);
            expected_size += entry.term.heap_allocations();
            expected_size += mem::size_of::<TermEntry>();
            expected_size += mem::size_of::<(Arc<[u8]>, SlabKey)>();
        }
        anyhow::ensure!(self.size == expected_size);
        Ok(())
    }
}
