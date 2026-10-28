use std::sync::Arc;

use common::{
    knobs::SEARCH_INDEX_SIZE_SOFT_LIMIT,
    persistence::PersistenceReader,
    runtime::Runtime,
};
use database::Database;
use search::searcher::SegmentTermMetadataFetcher;
use storage::Storage;

use crate::{
    search_flusher::{
        SearchFlusher,
        SearchIndexLimits,
    },
    text_index_worker::text_meta::{
        BuildTextIndexArgs,
        TextSearchIndex,
    },
    writer::SearchIndexMetadataWriter,
    FlusherType,
};

pub(crate) struct FlusherBuilder<RT: Runtime> {
    runtime: RT,
    database: Database<RT>,
    reader: Arc<dyn PersistenceReader>,
    storage: Arc<dyn Storage>,
    segment_term_metadata_fetcher: Arc<dyn SegmentTermMetadataFetcher>,
    limits: SearchIndexLimits,
    writer: SearchIndexMetadataWriter<RT, TextSearchIndex>,
    flusher_type: FlusherType,
}

impl<RT: Runtime> FlusherBuilder<RT> {
    pub(crate) fn new(
        runtime: RT,
        database: Database<RT>,
        reader: Arc<dyn PersistenceReader>,
        storage: Arc<dyn Storage>,
        segment_term_metadata_fetcher: Arc<dyn SegmentTermMetadataFetcher>,
        writer: SearchIndexMetadataWriter<RT, TextSearchIndex>,
        flusher_type: FlusherType,
    ) -> Self {
        Self {
            runtime,
            database,
            reader,
            storage,
            segment_term_metadata_fetcher,
            writer,
            limits: SearchIndexLimits {
                index_size_soft_limit: *SEARCH_INDEX_SIZE_SOFT_LIMIT,
                incremental_multipart_threshold_bytes: *SEARCH_INDEX_SIZE_SOFT_LIMIT,
            },
            flusher_type,
        }
    }

    pub(crate) fn build(self) -> TextIndexFlusher<RT> {
        SearchFlusher::new(
            self.runtime,
            self.database,
            self.reader,
            self.storage.clone(),
            self.limits,
            self.writer,
            BuildTextIndexArgs {
                search_storage: self.storage.clone(),
                segment_term_metadata_fetcher: self.segment_term_metadata_fetcher.clone(),
            },
            self.flusher_type,
        )
    }
}

pub type TextIndexFlusher<RT> = SearchFlusher<RT, TextSearchIndex>;

pub(crate) fn new_text_flusher<RT: Runtime>(
    runtime: RT,
    database: Database<RT>,
    reader: Arc<dyn PersistenceReader>,
    storage: Arc<dyn Storage>,
    segment_metadata_fetcher: Arc<dyn SegmentTermMetadataFetcher>,
    writer: SearchIndexMetadataWriter<RT, TextSearchIndex>,
    flusher_type: FlusherType,
) -> TextIndexFlusher<RT> {
    FlusherBuilder::new(
        runtime,
        database,
        reader,
        storage,
        segment_metadata_fetcher,
        writer,
        flusher_type,
    )
    .build()
}
