use tantivy::tokenizer::{
    LowerCaser,
    RemoveLongFilter,
    SimpleTokenizer,
    TextAnalyzer,
};

/// How many words (after stemming) can be in a text query?
pub const MAX_QUERY_TERMS: usize = 16;

/// What is the maximum length of a single text term? We will silently drop
/// terms that exceed this length.
///
/// TODO: Or should we truncate these to a prefix?
pub const MAX_TEXT_TERM_LENGTH: usize = 32;

/// What is the maximum number candidate revisions will we load into memory?
pub const MAX_CANDIDATE_REVISIONS: usize = 1024;

/// How many filter conditions can be on a query?
pub const MAX_FILTER_CONDITIONS: usize = 8;

/// Name of the Convex English tokenizer passed to Tantivy.
pub const CONVEX_EN_TOKENIZER: &str = "convex_en";

/// The maximum terms we'll return from QueryTokens. This corresponds to the
/// maximum number of posting lists we'll want to consider in a single query.
pub const MAX_UNIQUE_QUERY_TERMS: usize = 64;

pub fn convex_en() -> TextAnalyzer {
    TextAnalyzer::from(SimpleTokenizer)
        .filter(RemoveLongFilter::limit(MAX_TEXT_TERM_LENGTH))
        .filter(LowerCaser)
}
