use crate::{
    text_index_worker::text_meta::TextSearchIndex,
    writer::SearchIndexMetadataWriter,
};

pub mod compactor;
pub mod fast_forward;
pub mod flusher;
mod text_meta;

pub type TextIndexMetadataWriter<RT> = SearchIndexMetadataWriter<RT, TextSearchIndex>;
pub use text_meta::BuildTextIndexArgs;
