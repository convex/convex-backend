use std::sync::Arc;

use common::{
    knobs::{
        MULTI_SEGMENT_FULL_SCAN_THRESHOLD_KB,
        VECTOR_INDEX_SIZE_SOFT_LIMIT,
    },
    persistence::PersistenceReader,
    runtime::Runtime,
};
use database::Database;
use storage::Storage;

use super::vector_meta::BuildVectorIndexArgs;
use crate::{
    search_flusher::{
        SearchFlusher,
        SearchIndexLimits,
    },
    vector_index_worker::vector_meta::VectorSearchIndex,
    writer::SearchIndexMetadataWriter,
    FlusherType,
};

pub type VectorIndexFlusher<RT> = SearchFlusher<RT, VectorSearchIndex>;

pub(crate) fn new_vector_flusher<RT: Runtime>(
    runtime: RT,
    database: Database<RT>,
    reader: Arc<dyn PersistenceReader>,
    storage: Arc<dyn Storage>,
    writer: SearchIndexMetadataWriter<RT, VectorSearchIndex>,
    flusher_type: FlusherType,
) -> VectorIndexFlusher<RT> {
    SearchFlusher::new(
        runtime,
        database,
        reader,
        storage,
        SearchIndexLimits {
            index_size_soft_limit: *VECTOR_INDEX_SIZE_SOFT_LIMIT,
            incremental_multipart_threshold_bytes: *VECTOR_INDEX_SIZE_SOFT_LIMIT,
        },
        writer,
        BuildVectorIndexArgs {
            full_scan_threshold_bytes: *MULTI_SEGMENT_FULL_SCAN_THRESHOLD_KB,
        },
        flusher_type,
    )
}
