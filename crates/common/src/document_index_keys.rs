use std::collections::{
    BTreeMap,
    HashSet,
};

use compact_str::CompactString;
use value::{
    heap_size::{
        HeapSize,
        WithHeapSize,
    },
    FieldPath,
    ResolvedDocumentId,
};

use crate::{
    document::PackedDocument,
    index::IndexKeyBytes,
    query::FilterValue as SearchFilterValue,
    types::TabletIndexName,
};

/// A generic pair of old and new optional index key values.
#[derive(Clone, Debug)]
pub struct Update<K> {
    pub old: Option<K>,
    pub new: Option<K>,
}

impl<K> Update<K> {
    pub fn iter(&self) -> impl Iterator<Item = &K> {
        [self.old.as_ref(), self.new.as_ref()].into_iter().flatten()
    }
}

impl<K: HeapSize> HeapSize for Update<K> {
    fn heap_size(&self) -> usize {
        self.old.as_ref().map_or(0, |k| k.heap_size())
            + self.new.as_ref().map_or(0, |k| k.heap_size())
    }
}

#[derive(Clone, Debug)]
pub enum IndexKeyUpdate {
    Text(Update<SearchIndexKeyValue>),
    Database(Update<IndexKeyBytes>),
}

impl HeapSize for IndexKeyUpdate {
    fn heap_size(&self) -> usize {
        match self {
            IndexKeyUpdate::Text(u) => u.heap_size(),
            IndexKeyUpdate::Database(u) => u.heap_size(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct IndexUpdate {
    pub document_id: ResolvedDocumentId,
    pub update: IndexKeyUpdate,
    pub new_document: Option<PackedDocument>,
}

impl HeapSize for IndexUpdate {
    fn heap_size(&self) -> usize {
        self.update.heap_size() + self.new_document.as_ref().map_or(0, |d| d.heap_size())
    }
}

impl IndexUpdate {
}

#[derive(Clone, Debug)]
pub struct DatabaseIndexWrite {
    pub document_id: ResolvedDocumentId,
    pub update: Update<IndexKeyBytes>,
    /// Used for fast-forwarding the index cache.
    pub new_document: Option<PackedDocument>,
}

impl HeapSize for DatabaseIndexWrite {
    fn heap_size(&self) -> usize {
        self.update.heap_size() + self.new_document.as_ref().map_or(0, |d| d.heap_size())
    }
}

#[derive(Clone, Debug)]
pub struct TextIndexWrite {
    pub document_id: ResolvedDocumentId,
    pub update: Update<SearchIndexKeyValue>,
}

impl HeapSize for TextIndexWrite {
    fn heap_size(&self) -> usize {
        self.update.heap_size()
    }
}

/// For a given document, contains all the index keys for the indexes on the
/// document's table.
#[derive(Debug, Default)]
pub struct DocumentIndexKeys(
    // TODO: Key by IndexId instead of TabletIndexName and make an IndexTableUpdate(TabletId)
    // variant for tracking changes to the `_index` table
    pub BTreeMap<TabletIndexName, IndexUpdate>,
);

impl DocumentIndexKeys {
}

#[derive(Clone, Debug)]
pub struct SearchIndexKeyValue {
    /// These are values for the fields present in the must
    /// clauses of the search index.
    pub filter_values: WithHeapSize<BTreeMap<FieldPath, SearchFilterValue>>,
    pub search_field: FieldPath,
    pub search_field_value: Option<SearchValueTokens>,
}

impl HeapSize for SearchIndexKeyValue {
    fn heap_size(&self) -> usize {
        self.filter_values.heap_size()
            + self.search_field.heap_size()
            + self.search_field_value.heap_size()
    }
}

/// The tokens in some textual value (search field of a full-text search index).
///
/// Tokens are not sorted in any particular order, but must be unique.
/// (Uniqueness is not strictly necessary here, but we'd like to avoid
/// iterating over the same token multiple times.)
#[derive(Clone, Debug)]
pub struct SearchValueTokens(WithHeapSize<Box<[CompactString]>>);

impl From<HashSet<CompactString>> for SearchValueTokens {
    fn from(value: HashSet<CompactString>) -> Self {
        let tokens: Box<[CompactString]> = value.into_iter().collect();
        Self(tokens.into())
    }
}

impl SearchValueTokens {
    pub fn for_each_token<F>(&self, prefix: bool, mut for_each: F)
    where
        F: FnMut(&str),
    {
        if prefix {
            // We're inverting prefix match here by constructing all possible prefixes for
            // each term in the document if at least one prefix search exists in
            // the readset (resulting in this method being called with prefix:
            // true).
            //
            // This lets callers search into tries containing the actual search term with
            // dfa prefixes set to false and still match based on prefix.
            // Searching a trie with the document tokens is bounded by the size
            // of the document, which is expected to be significantly smaller
            // than the total number of subscriptions for busy backends.
            for token in self.calculate_prefixes() {
                for_each(token);
            }
        } else {
            for token in self.0.iter() {
                for_each(token);
            }
        }
    }

    fn calculate_prefixes(&self) -> impl Iterator<Item = &str> + '_ {
        let mut set: HashSet<&str> = HashSet::new();

        for token in self.0.iter() {
            if !set.insert(token) {
                continue;
            }
            for (i, _) in token.char_indices()
                // Skip the first index which is always 0
                .skip(1)
            {
                // After that we get all prefixes except for the complete
                // token (because `..i` always skips the last character
                // bytes).
                set.insert(&token[..i]);
            }
        }
        set.into_iter()
    }
}

impl HeapSize for SearchValueTokens {
    fn heap_size(&self) -> usize {
        self.0.heap_size()
    }
}
