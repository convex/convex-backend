use async_trait::async_trait;
use common::{
    document::DeveloperDocument,
    index::IndexKeyBytes,
    knobs::TRANSACTION_MAX_READ_SIZE_BYTES,
    query::{
        CursorPosition,
        Search,
        SearchVersion,
    },
    runtime::Runtime,
    types::{
        IndexName,
        StableIndexName,
        TabletIndexName,
        WriteTimestamp,
    },
    version::{
        Version,
        MIN_NPM_VERSION_FOR_PREFIX_SEARCH,
    },
};
use errors::ErrorMetadata;
use indexing::index_registry::index_not_found_error;
use search::{
    CandidateRevision,
    MAX_CANDIDATE_REVISIONS,
};
use tokio::task;
use value::{
    DeveloperDocumentId,
    TableNamespace,
    TableNumber,
};

use super::{
    index_range::{
        soft_data_limit,
        CursorInterval,
    },
    IndexRangeResponse,
    QueryStream,
    QueryStreamNext,
};
use crate::{
    metrics,
    Transaction,
    UserFacingModel,
};

/// A `QueryStream` that begins by querying a search index.
pub struct SearchQuery {
    // The tablet index being searched.
    // Table names in `query` are just for error messages and usage, and may
    // get out of sync with this.
    stable_index_name: StableIndexName,

    query: Search,
    // Results are generated on the first call to SearchQuery::next.
    results: Option<SearchResultIterator>,

    /// The interval defined by the optional start and end cursors.
    /// The start cursor will move as we produce results.
    cursor_interval: CursorInterval,
    version: Option<Version>,
}

impl SearchQuery {
    pub fn new(
        stable_index_name: StableIndexName,
        query: Search,
        cursor_interval: CursorInterval,
        version: Option<Version>,
    ) -> Self {
        Self {
            stable_index_name,
            query,
            results: None,
            cursor_interval,
            version,
        }
    }

    fn get_cli_gated_search_version(&self) -> SearchVersion {
        match &self.version {
            Some(v) if v >= &MIN_NPM_VERSION_FOR_PREFIX_SEARCH => SearchVersion::V2,
            _ => SearchVersion::V1,
        }
    }

    async fn search<RT: Runtime>(
        &self,
        tx: &mut Transaction<RT>,
    ) -> anyhow::Result<SearchResultIterator> {
        let search_version = self.get_cli_gated_search_version();
        let revisions = tx
            .text_search(&self.stable_index_name, self.query.clone(), search_version)
            .await?;
        let revisions_in_range = revisions
            .into_iter()
            .filter(|(_, index_key)| self.cursor_interval.contains(index_key))
            .collect();
        let (namespace, table_number) = match self.stable_index_name.tablet_index_name_or_missing()
        {
            Ok(index_name) => {
                let namespace = tx.table_mapping().tablet_namespace(*index_name.table())?;
                let tablet_number = tx.table_mapping().tablet_number(*index_name.table())?;
                (namespace, tablet_number)
            },
            Err(missing_index_name) => {
                anyhow::bail!(index_not_found_error(missing_index_name));
            },
        };
        Ok(SearchResultIterator::new(
            revisions_in_range,
            namespace,
            table_number,
            self.version.clone(),
        ))
    }

    #[convex_macro::instrument_future]
    async fn _next<RT: Runtime>(
        &mut self,
        tx: &mut Transaction<RT>,
    ) -> anyhow::Result<Option<(DeveloperDocument, WriteTimestamp)>> {
        let iterator = match &mut self.results {
            Some(results) => results,
            None => self.results.get_or_insert(self.search(tx).await?),
        };

        Ok(match iterator.next(tx).await? {
            None => {
                // We're out of results. If we have an end cursor then we must
                // have reached it. Otherwise we're at the end of the entire
                // query.
                self.cursor_interval.curr_exclusive = Some(
                    self.cursor_interval
                        .end_inclusive
                        .clone()
                        .unwrap_or(CursorPosition::End),
                );
                None
            },
            Some((next_document, next_index_key, next_timestamp)) => {
                self.cursor_interval.curr_exclusive = Some(CursorPosition::After(next_index_key));
                Some((next_document, next_timestamp))
            },
        })
    }
}

#[async_trait]
impl QueryStream for SearchQuery {
    fn cursor_position(&self) -> &Option<CursorPosition> {
        &self.cursor_interval.curr_exclusive
    }

    fn split_cursor_position(&self) -> Option<&CursorPosition> {
        // We could try to find a split cursor, but splitting a search query
        // doesn't make it more efficient, so for simplicity we can say splitting
        // isn't allowed.
        None
    }

    fn is_approaching_data_limit(&self) -> bool {
        self.results
            .as_ref()
            .is_some_and(|results| results.is_approaching_data_limit())
    }

    async fn next<RT: Runtime>(
        &mut self,
        tx: &mut Transaction<RT>,
        _prefetch_hint: Option<usize>,
    ) -> anyhow::Result<QueryStreamNext> {
        self._next(tx).await.map(QueryStreamNext::Ready)
    }

    fn feed(&mut self, _index_range_response: IndexRangeResponse) -> anyhow::Result<()> {
        anyhow::bail!("cannot feed an index range response into a search query");
    }

    fn tablet_index_name(&self) -> Option<&TabletIndexName> {
        self.stable_index_name.tablet_index_name()
    }

    fn printable_index_name(&self) -> &IndexName {
        &self.query.index_name
    }
}

#[derive(Clone)]
struct SearchResultIterator {
    namespace: TableNamespace,
    table_number: TableNumber,
    candidates: Vec<(CandidateRevision, IndexKeyBytes)>,
    next_index: usize,
    bytes_read: usize,
    version: Option<Version>,
}

impl SearchResultIterator {
    fn new(
        candidates: Vec<(CandidateRevision, IndexKeyBytes)>,
        namespace: TableNamespace,
        table_number: TableNumber,
        version: Option<Version>,
    ) -> Self {
        Self {
            namespace,
            table_number,
            candidates,
            next_index: 0,
            bytes_read: 0,
            version,
        }
    }

    fn is_approaching_data_limit(&self) -> bool {
        let soft_maximum_rows_read = soft_data_limit(MAX_CANDIDATE_REVISIONS);
        let soft_maximum_bytes_read = soft_data_limit(*TRANSACTION_MAX_READ_SIZE_BYTES);
        self.next_index > soft_maximum_rows_read || self.bytes_read > soft_maximum_bytes_read
    }

    async fn next<RT: Runtime>(
        &mut self,
        tx: &mut Transaction<RT>,
    ) -> anyhow::Result<Option<(DeveloperDocument, IndexKeyBytes, WriteTimestamp)>> {
        let timer = metrics::search::iterator_next_timer();
        task::consume_budget().await;

        if self.next_index == MAX_CANDIDATE_REVISIONS {
            anyhow::bail!(ErrorMetadata::bad_request(
                "SearchQueryScannedTooManyDocumentsError",
                format!(
                    "Search query scanned too many documents (fetched {MAX_CANDIDATE_REVISIONS}). \
                     Consider using a smaller limit, paginating the query, or using a filter \
                     field to limit the number of documents pulled from the search index."
                )
            ))
        }

        let Some((candidate, index_key)) = self.candidates.get(self.next_index) else {
            timer.finish();
            return Ok(None);
        };

        self.next_index += 1;

        let id = DeveloperDocumentId::new(self.table_number, candidate.id);
        let (document, existing_doc_ts) = UserFacingModel::new(tx, self.namespace)
            .get_with_ts(id, self.version.clone())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("Unable to load search result {id}@{:?}", candidate.ts)
            })?;

        self.bytes_read += document.size();

        anyhow::ensure!(
            existing_doc_ts == candidate.ts,
            "Search result has incorrect timestamp. There's a bug in our search logic. id:{id} \
             existing_doc_ts:{existing_doc_ts:?} candidate_ts:{:?}",
            candidate.ts
        );

        timer.finish();
        Ok(Some((document, index_key.clone(), existing_doc_ts)))
    }
}
