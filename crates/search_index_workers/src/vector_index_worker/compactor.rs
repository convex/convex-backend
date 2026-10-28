use std::sync::Arc;

use common::runtime::Runtime;
use database::Database;
use search::searcher::Searcher;
use storage::Storage;

use crate::{
    search_compactor::{
        CompactionConfig,
        SearchIndexCompactor,
    },
    vector_index_worker::vector_meta::VectorSearchIndex,
    writer::SearchIndexMetadataWriter,
};

pub type VectorIndexCompactor<RT> = SearchIndexCompactor<RT, VectorSearchIndex>;

pub(crate) fn new_vector_compactor<RT: Runtime>(
    database: Database<RT>,
    searcher: Arc<dyn Searcher>,
    search_storage: Arc<dyn Storage>,
    config: CompactionConfig,
    writer: SearchIndexMetadataWriter<RT, VectorSearchIndex>,
) -> VectorIndexCompactor<RT> {
    VectorIndexCompactor::new(database, searcher, search_storage, config, writer)
}
