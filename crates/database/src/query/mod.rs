use std::{
    collections::BTreeMap,
    marker::PhantomData,
    ops::Deref,
};

use anyhow::Context;
use async_trait::async_trait;
use common::{
    bootstrap_model::index::{
        database_index::IndexedFields,
        INDEX_TABLE,
    },
    document::{
        DeveloperDocument,
        PackedDocument,
        ResolvedDocument,
    },
    errors::JsError,
    index::IndexKeyBytes,
    interval::Interval,
    query::{
        Cursor,
        CursorPosition,
        Query,
        QueryFingerprint,
        QueryOperator,
        QuerySource,
    },
    runtime::Runtime,
    try_anyhow,
    types::{
        IndexName,
        TabletIndexName,
        WriteTimestamp,
    },
    version::Version,
};
use errors::ErrorMetadata;
use futures::{
    future::BoxFuture,
    FutureExt,
};
use indexing::index_reader::BatchKey;
use maplit::btreemap;
use value::{
    val,
    TableNamespace,
};

use self::{
    filter::Filter,
    index_range::{
        CursorInterval,
        IndexRange,
    },
    limit::Limit,
    search_query::SearchQuery,
};
use crate::{
    bootstrap_model::user_facing::index_range_batch,
    transaction::IndexRangeRequest,
    IndexModel,
    Transaction,
};

mod filter;
mod index_range;
mod limit;
mod search_query;

pub use index_range::soft_data_limit;

// Even in the presence of large prefetch hints, we should never fetch too much
// data at once.
const MAX_QUERY_FETCH: usize = 1024;

/// The implementation of `interface Query` from the npm package.
#[async_trait]
trait QueryStream: Send {
    /// Return a position for a continuation cursor. A query defines a result
    /// set, independent of pagination, and assuming no concurrent
    /// transactions overlap with this result set, re-executing a query with
    /// this cursor will continue from just after the previous return from
    /// `next()`. The cursor is, in some sense, the "stack" that gets saved
    /// when a query is paused, and the user can decide to "resume" the
    /// query in a subsequent transaction. If there *are* overlapping
    /// transactions, the results of resuming a query are currently
    /// undefined, and we'll eventually want to define them formally and
    /// ensure they're generally useful.
    fn cursor_position(&self) -> &Option<CursorPosition>;

    fn split_cursor_position(&self) -> Option<&CursorPosition>;

    /// Returns true if the query has read so much data that it is in danger
    /// of taking too long and throwing errors. Use this as an indication that
    /// a paginated query should be split in two, or that an unpaginated query
    /// should be paginated, or a query with a filter could use an index
    /// instead.
    fn is_approaching_data_limit(&self) -> bool;

    /// Pull a value out from the query pipeline. The query has completed after
    /// returning `None`, and `.next()` should not be called again. If this
    /// method returns an error, it is safe to retry calling `.next()`, but
    /// the query may not make any progress if the error was, for
    /// example, an `QueryScannedTooManyDocumentsError`.
    /// If `next` needs to fetch an index range, it returns
    /// Ok(WaitingOn(request)) and the response should be fed back into
    /// `feed` before calling `next` again.
    /// TODO(lee) once SearchQuery is no longer in the query pipeline, make
    /// `next` synchronous, with all IO handled by batched index range requests
    /// triggered by WaitingOn(request).
    async fn next<RT: Runtime>(
        &mut self,
        tx: &mut Transaction<RT>,
        prefetch_hint: Option<usize>,
    ) -> anyhow::Result<QueryStreamNext>;
    fn feed(&mut self, index_range_response: IndexRangeResponse) -> anyhow::Result<()>;

    /// All queries walk an index of some kind, as long as the table exists.
    /// This is that index name, tied to a tablet.
    fn tablet_index_name(&self) -> Option<&TabletIndexName>;

    /// For logging. All queries have an index name.
    fn printable_index_name(&self) -> &IndexName;
}

pub struct IndexRangeResponse {
    pub page: Vec<(IndexKeyBytes, PackedDocument, WriteTimestamp)>,
    pub cursor: CursorPosition,
}

#[derive(Debug)]
pub enum QueryStreamNext {
    Ready(Option<(DeveloperDocument, WriteTimestamp)>),
    WaitingOn(IndexRangeRequest),
}

pub struct DeveloperQuery<RT: Runtime> {
    root: QueryNode,
    query_fingerprint: Option<QueryFingerprint>,
    end_cursor: Option<Cursor>,
    _marker: PhantomData<RT>,
}

#[derive(Copy, Clone, Debug)]
pub enum TableFilter {
    IncludePrivateSystemTables,
    ExcludePrivateSystemTables,
}

/// ResolvedQuery is a handy way to query for documents in private system
/// tables. It wraps DeveloperQuery, attaching the tablet id on returned
/// documents, so they can be passed to internal functions.
///
/// You may notice that DeveloperQuery calls Transaction methods that return
/// ResolvedDocuments, so ResolvedQuery is re-attaching a tablet id that
/// was previously discarded. You may think that DeveloperQuery should wrap
/// ResolvedQuery, and convert virtual table documents after querying the
/// documents. However, this doesn't work with Filters on virtual tables, which
/// should execute on the fields of the virtual table.
pub struct ResolvedQuery<RT: Runtime> {
    developer: DeveloperQuery<RT>,
}

impl<RT: Runtime> ResolvedQuery<RT> {
    pub fn new_bounded(
        tx: &mut Transaction<RT>,
        namespace: TableNamespace,
        query: Query,
        pagination_options: PaginationOptions,
        version: Option<Version>,
        table_filter: TableFilter,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            developer: DeveloperQuery::new_bounded(
                tx,
                namespace,
                query,
                pagination_options,
                version,
                table_filter,
            )?,
        })
    }

    pub fn new(
        tx: &mut Transaction<RT>,
        namespace: TableNamespace,
        query: Query,
    ) -> anyhow::Result<Self> {
        Self::new_bounded(
            tx,
            namespace,
            query,
            PaginationOptions::NoPagination,
            None,
            TableFilter::IncludePrivateSystemTables,
        )
    }

    pub fn new_with_version(
        tx: &mut Transaction<RT>,
        namespace: TableNamespace,
        query: Query,
        version: Option<Version>,
    ) -> anyhow::Result<Self> {
        Self::new_bounded(
            tx,
            namespace,
            query,
            PaginationOptions::NoPagination,
            version,
            TableFilter::IncludePrivateSystemTables,
        )
    }
}

impl<RT: Runtime> Deref for ResolvedQuery<RT> {
    type Target = DeveloperQuery<RT>;

    fn deref(&self) -> &Self::Target {
        &self.developer
    }
}

impl<RT: Runtime> AsMut<DeveloperQuery<RT>> for ResolvedQuery<RT> {
    fn as_mut(&mut self) -> &mut DeveloperQuery<RT> {
        &mut self.developer
    }
}

pub enum PaginationOptions {
    /// For one-shot queries that don't need pagination.
    /// e.g. `.collect()`, `.first()`, `.get()`
    /// Such a query does not have a `cursor` so you can't construct a new query
    /// for the next page.
    NoPagination,
    /// For manual pagination, usually internal within workers but could be used
    /// when we know there is no reactivity, like in a oneshot query from the
    /// client or from an action.
    /// Such a query does have a `cursor` so you can fetch the next
    /// page, but it does not have a `split_cursor` and you can't refetch the
    /// query on the same range by passing in an `end_cursor`.
    ManualPagination {
        start_cursor: Option<Cursor>,
        maximum_rows_read: Option<usize>,
        maximum_bytes_read: Option<usize>,
    },
    /// For reactive pagination, when queries call `.paginate()`. Such a query
    /// does have a `cursor` and a `split_cursor`, and you can refetch the query
    /// on the same range by passing in an `end_cursor`.
    ReactivePagination {
        start_cursor: Option<Cursor>,
        end_cursor: Option<Cursor>,
        maximum_rows_read: Option<usize>,
        maximum_bytes_read: Option<usize>,
    },
}

impl<RT: Runtime> DeveloperQuery<RT> {
    pub fn new(
        tx: &mut Transaction<RT>,
        namespace: TableNamespace,
        query: Query,
        table_filter: TableFilter,
    ) -> anyhow::Result<Self> {
        Self::new_bounded(
            tx,
            namespace,
            query,
            PaginationOptions::NoPagination,
            None,
            table_filter,
        )
    }

    pub fn new_with_version(
        tx: &mut Transaction<RT>,
        namespace: TableNamespace,
        query: Query,
        version: Option<Version>,
        table_filter: TableFilter,
    ) -> anyhow::Result<Self> {
        Self::new_bounded(
            tx,
            namespace,
            query,
            PaginationOptions::NoPagination,
            version,
            table_filter,
        )
    }

    pub fn new_bounded(
        tx: &mut Transaction<RT>,
        namespace: TableNamespace,
        query: Query,
        pagination_options: PaginationOptions,
        version: Option<Version>,
        table_filter: TableFilter,
    ) -> anyhow::Result<Self> {
        let index_name = match query.source {
            QuerySource::FullTableScan(ref full_table_scan) => {
                let table_name = full_table_scan.table_name.clone();
                anyhow::ensure!(
                    &table_name != &INDEX_TABLE,
                    "`_index` can't be queried via .collect() since it doesn't have \
                     by_creation_time index. Please query via by_id index."
                );
                IndexName::by_creation_time(table_name)
            },
            QuerySource::IndexRange(ref index_range) => index_range.index_name.clone(),
            QuerySource::Search(ref search) => search.index_name.clone(),
        };
        let stable_index_name =
            IndexModel::new(tx).stable_index_name(namespace, &index_name, table_filter)?;
        let indexed_fields = match query.source {
            QuerySource::FullTableScan(_) => IndexedFields::creation_time(),
            QuerySource::IndexRange(_) => {
                IndexModel::new(tx).indexed_fields(&stable_index_name, &index_name)?
            },
            QuerySource::Search(_) => {
                // Hack! Search indexes don't have any concept of indexed fields.
                // Database queries need the fields for the query fingerprint
                // because the order of the fields changes the query result.
                // Search query results don't depend on the index used so we
                // can just an empty list of fields.
                IndexedFields::try_from(Vec::new())?
            },
        };
        let should_compute_split_cursor = match &pagination_options {
            PaginationOptions::NoPagination => false,
            PaginationOptions::ManualPagination { .. } => false,
            PaginationOptions::ReactivePagination { .. } => true,
        };
        let (maximum_rows_read, maximum_bytes_read) = match &pagination_options {
            PaginationOptions::NoPagination => (None, None),
            PaginationOptions::ManualPagination {
                maximum_bytes_read,
                maximum_rows_read,
                ..
            }
            | PaginationOptions::ReactivePagination {
                maximum_bytes_read,
                maximum_rows_read,
                ..
            } => (*maximum_rows_read, *maximum_bytes_read),
        };
        // Fingerprint makes sure that a cursor is only used with the same
        // query. So you can fetch the next page of a query, but if the query
        // changes, we don't start returning bogus results.
        // e.g.
        // ```
        // const user = await db.get(args.userId);
        // const emails = await db.query("emails")
        //   .withIndex("address", q=>q.eq("address", user.emailAddress))
        //   .paginate(opts);
        // ```
        // If the user changes their email address, we don't want to continue
        // using the same cursors.
        let fingerprint = match &pagination_options {
            PaginationOptions::NoPagination => None,
            PaginationOptions::ManualPagination { .. }
            | PaginationOptions::ReactivePagination { .. } => {
                // Calculating fingerprint is expensive, so only do it if we're
                // paginating.
                Some(query.fingerprint(&indexed_fields)?)
            },
        };
        let end_cursor = match &pagination_options {
            PaginationOptions::NoPagination
            | PaginationOptions::ManualPagination { .. }
            | PaginationOptions::ReactivePagination {
                end_cursor: None, ..
            } => None,
            PaginationOptions::ReactivePagination {
                end_cursor: Some(end_cursor),
                ..
            } => {
                anyhow::ensure!(
                    Some(&end_cursor.query_fingerprint) == fingerprint.as_ref(),
                    invalid_cursor()
                );
                Some(end_cursor.clone())
            },
        };
        let cursor_interval = match pagination_options {
            PaginationOptions::NoPagination => CursorInterval {
                curr_exclusive: None,
                end_inclusive: None,
            },
            PaginationOptions::ManualPagination { start_cursor, .. }
            | PaginationOptions::ReactivePagination { start_cursor, .. } => {
                let start_cursor_position = match start_cursor {
                    Some(cursor) => {
                        anyhow::ensure!(
                            Some(cursor.query_fingerprint) == fingerprint,
                            invalid_cursor()
                        );
                        Some(cursor.position)
                    },
                    None => None,
                };
                CursorInterval {
                    curr_exclusive: start_cursor_position,
                    end_inclusive: end_cursor.as_ref().map(|cursor| cursor.position.clone()),
                }
            },
        };

        let mut cur_node = match query.source {
            QuerySource::FullTableScan(full_table_scan) => QueryNode::IndexRange(IndexRange::new(
                namespace,
                stable_index_name,
                index_name,
                Interval::all(),
                full_table_scan.order,
                indexed_fields,
                cursor_interval,
                maximum_rows_read,
                maximum_bytes_read,
                should_compute_split_cursor,
                version,
            )),
            QuerySource::IndexRange(index_range) => {
                let order = index_range.order;
                let interval = index_range.compile(indexed_fields.clone())?;
                QueryNode::IndexRange(IndexRange::new(
                    namespace,
                    stable_index_name,
                    index_name,
                    interval,
                    order,
                    indexed_fields,
                    cursor_interval,
                    maximum_rows_read,
                    maximum_bytes_read,
                    should_compute_split_cursor,
                    version,
                ))
            },
            QuerySource::Search(search) => QueryNode::Search(SearchQuery::new(
                stable_index_name,
                search,
                cursor_interval,
                version,
            )),
        };
        for operator in query.operators {
            let next_node = match operator {
                QueryOperator::Filter(expr) => {
                    let filter = Filter::new(cur_node, expr);
                    QueryNode::Filter(Box::new(filter))
                },
                QueryOperator::Limit(n) => {
                    let limit = Limit::new(cur_node, n);
                    QueryNode::Limit(Box::new(limit))
                },
            };
            cur_node = next_node;
        }
        Ok(Self {
            root: cur_node,
            query_fingerprint: fingerprint,
            end_cursor,
            _marker: PhantomData,
        })
    }

    /// Get the end_cursor as specified in `new_bounded`.
    pub fn end_cursor(&self) -> Option<Cursor> {
        self.end_cursor.clone()
    }

    /// Get the current cursor for the query.
    ///
    /// Will be `None` if there was no initial cursor and `next` has
    /// never been called,
    /// or if the query was created with PaginationOptions::NoPagination.
    pub fn cursor(&self) -> Option<Cursor> {
        match self.root.cursor_position().clone() {
            Some(position) => Some(Cursor {
                position,
                query_fingerprint: self.query_fingerprint.clone()?,
            }),
            None => None,
        }
    }

    pub fn split_cursor(&self) -> Option<Cursor> {
        match self.root.split_cursor_position().cloned() {
            Some(position) => Some(Cursor {
                position,
                query_fingerprint: self.query_fingerprint.clone()?,
            }),
            None => None,
        }
    }

    pub fn is_approaching_data_limit(&self) -> bool {
        self.root.is_approaching_data_limit()
    }

    pub async fn next(
        &mut self,
        tx: &mut Transaction<RT>,
        prefetch_hint: Option<usize>,
    ) -> anyhow::Result<Option<DeveloperDocument>> {
        match self.next_with_ts(tx, prefetch_hint).await? {
            None => Ok(None),
            Some((document, _)) => Ok(Some(document)),
        }
    }

    #[convex_macro::instrument_future]
    pub async fn next_with_ts(
        &mut self,
        tx: &mut Transaction<RT>,
        prefetch_hint: Option<usize>,
    ) -> anyhow::Result<Option<(DeveloperDocument, WriteTimestamp)>> {
        query_batch_next(btreemap! {0 => (self, prefetch_hint)}, tx)
            .await
            .remove(&0)
            .context("batch_key missing")?
    }

    pub fn printable_index_name(&self) -> &IndexName {
        self.root.printable_index_name()
    }
}

impl<RT: Runtime> ResolvedQuery<RT> {
    pub async fn next(
        &mut self,
        tx: &mut Transaction<RT>,
        prefetch_hint: Option<usize>,
    ) -> anyhow::Result<Option<ResolvedDocument>> {
        match self.next_with_ts(tx, prefetch_hint).await? {
            None => Ok(None),
            Some((document, _)) => Ok(Some(document)),
        }
    }

    #[convex_macro::instrument_future]
    pub async fn next_with_ts(
        &mut self,
        tx: &mut Transaction<RT>,
        prefetch_hint: Option<usize>,
    ) -> anyhow::Result<Option<(ResolvedDocument, WriteTimestamp)>> {
        resolved_query_batch_next(btreemap! {0 => (self, prefetch_hint)}, tx)
            .await
            .remove(&0)
            .context("batch_key missing")?
    }

    pub async fn expect_at_most_one(
        &mut self,
        tx: &mut Transaction<RT>,
    ) -> anyhow::Result<Option<ResolvedDocument>> {
        let v = match self.next(tx, Some(2)).await? {
            Some(v) => v,
            None => return Ok(None),
        };
        if self.next(tx, Some(1)).await?.is_some() {
            anyhow::bail!("Received more than one value for query");
        }
        Ok(Some(v))
    }
}

pub fn query_batch_next<'a, RT: Runtime>(
    batch: BTreeMap<BatchKey, (&'a mut DeveloperQuery<RT>, Option<usize>)>,
    tx: &'a mut Transaction<RT>,
) -> BoxFuture<'a, BTreeMap<BatchKey, anyhow::Result<Option<(DeveloperDocument, WriteTimestamp)>>>>
{
    query_batch_next_(batch, tx).boxed()
}

pub async fn query_batch_next_<RT: Runtime>(
    mut batch: BTreeMap<BatchKey, (&mut DeveloperQuery<RT>, Option<usize>)>,
    tx: &mut Transaction<RT>,
) -> BTreeMap<BatchKey, anyhow::Result<Option<(DeveloperDocument, WriteTimestamp)>>> {
    let batch_size = batch.len();
    // Algorithm overview:
    // Call `next` on every query.
    // Accumulate fetch (IO) requests and perform them all in a batch.
    // Call `feed` on the queries with the responses from the fetch requests.
    // Repeat until all queries have returned Ready from `next`.
    let mut results = BTreeMap::new();
    while !batch.is_empty() {
        let mut batch_to_feed = BTreeMap::new();
        let mut requests = BTreeMap::new();
        for (batch_key, (query, prefetch_hint)) in batch {
            match query.root.next(tx, prefetch_hint).await {
                Err(e) => {
                    results.insert(batch_key, Err(e));
                },
                Ok(QueryStreamNext::WaitingOn(request)) => {
                    requests.insert(batch_key, request);
                    batch_to_feed.insert(batch_key, (query, prefetch_hint));
                },
                Ok(QueryStreamNext::Ready(result)) => {
                    // TODO: This event is reporting too often to Honeycomb
                    // Event::add_to_local_parent("query_batch_next_ready", || {
                    //     let table_name = query.root.printable_index_name().table();
                    //     let table_name = if table_name.is_system() {
                    //         table_name.to_string()
                    //     } else {
                    //         format!("user_table")
                    //     };
                    //     [(Cow::Borrowed("query.table"), Cow::Owned(table_name))]
                    // });

                    results.insert(batch_key, Ok(result));
                },
            }
        }
        let mut responses = if requests.is_empty() {
            BTreeMap::new()
        } else {
            index_range_batch(tx, requests).await
        };
        let mut next_batch = BTreeMap::new();
        for (batch_key, (query, prefetch_hint)) in batch_to_feed {
            let result: anyhow::Result<_> = try_anyhow!({
                let index_range_responses = responses
                    .remove(&batch_key)
                    .context("batch_key missing")??;
                query.root.feed(index_range_responses)?;
            });
            match result {
                Err(e) => {
                    results.insert(batch_key, Err(e));
                },
                Ok(_) => {
                    next_batch.insert(batch_key, (query, prefetch_hint));
                },
            }
        }
        batch = next_batch;
    }
    assert_eq!(results.len(), batch_size);
    results
}

pub async fn resolved_query_batch_next<RT: Runtime>(
    batch: BTreeMap<BatchKey, (&mut ResolvedQuery<RT>, Option<usize>)>,
    tx: &mut Transaction<RT>,
) -> BTreeMap<BatchKey, anyhow::Result<Option<(ResolvedDocument, WriteTimestamp)>>> {
    let tablet_ids: BTreeMap<_, _> = batch
        .iter()
        .map(|(batch_key, (query, _))| {
            (
                *batch_key,
                query
                    .developer
                    .root
                    .tablet_index_name()
                    .map(|index_name| *index_name.table()),
            )
        })
        .collect();
    let results = query_batch_next(
        batch
            .into_iter()
            .map(|(batch_key, (query, prefetch_hint))| (batch_key, (query.as_mut(), prefetch_hint)))
            .collect(),
        tx,
    )
    .await;
    results
        .into_iter()
        .map(|(batch_key, result)| {
            let resolved_result: anyhow::Result<_> = try_anyhow!({
                match result? {
                    Some((document, ts)) => {
                        let tablet_id = tablet_ids
                            .get(&batch_key)
                            .context("tablet_id missing")?
                            .context("document must come from some tablet")?;
                        let document = document.to_resolved(tablet_id);
                        Some((document, ts))
                    },
                    None => None,
                }
            });
            (batch_key, resolved_result)
        })
        .collect()
}

enum QueryNode {
    IndexRange(IndexRange),
    Search(SearchQuery),
    Filter(Box<Filter>),
    Limit(Box<Limit>),
}

#[async_trait]
impl QueryStream for QueryNode {
    fn cursor_position(&self) -> &Option<CursorPosition> {
        match self {
            QueryNode::IndexRange(r) => r.cursor_position(),
            QueryNode::Search(r) => r.cursor_position(),
            QueryNode::Filter(r) => r.cursor_position(),
            QueryNode::Limit(r) => r.cursor_position(),
        }
    }

    fn split_cursor_position(&self) -> Option<&CursorPosition> {
        match self {
            QueryNode::IndexRange(r) => r.split_cursor_position(),
            QueryNode::Search(r) => r.split_cursor_position(),
            QueryNode::Filter(r) => r.split_cursor_position(),
            QueryNode::Limit(r) => r.split_cursor_position(),
        }
    }

    fn is_approaching_data_limit(&self) -> bool {
        match self {
            Self::IndexRange(r) => r.is_approaching_data_limit(),
            Self::Search(r) => r.is_approaching_data_limit(),
            Self::Filter(r) => r.is_approaching_data_limit(),
            Self::Limit(r) => r.is_approaching_data_limit(),
        }
    }

    async fn next<RT: Runtime>(
        &mut self,
        tx: &mut Transaction<RT>,
        prefetch_hint: Option<usize>,
    ) -> anyhow::Result<QueryStreamNext> {
        match self {
            QueryNode::IndexRange(r) => r.next(tx, prefetch_hint).await,
            QueryNode::Search(r) => r.next(tx, prefetch_hint).await,
            QueryNode::Filter(r) => r.next(tx, prefetch_hint).await,
            QueryNode::Limit(r) => r.next(tx, prefetch_hint).await,
        }
    }

    fn feed(&mut self, index_range_response: IndexRangeResponse) -> anyhow::Result<()> {
        match self {
            QueryNode::IndexRange(r) => r.feed(index_range_response),
            QueryNode::Search(r) => r.feed(index_range_response),
            QueryNode::Filter(r) => r.feed(index_range_response),
            QueryNode::Limit(r) => r.feed(index_range_response),
        }
    }

    fn tablet_index_name(&self) -> Option<&TabletIndexName> {
        match self {
            QueryNode::IndexRange(r) => r.tablet_index_name(),
            QueryNode::Search(r) => r.tablet_index_name(),
            QueryNode::Filter(r) => r.tablet_index_name(),
            QueryNode::Limit(r) => r.tablet_index_name(),
        }
    }

    fn printable_index_name(&self) -> &IndexName {
        match self {
            QueryNode::IndexRange(r) => r.printable_index_name(),
            QueryNode::Search(r) => r.printable_index_name(),
            QueryNode::Filter(r) => r.printable_index_name(),
            QueryNode::Limit(r) => r.printable_index_name(),
        }
    }
}

/// Return a system limit for reading too many documents in a query
fn query_scanned_too_many_documents_error(num_documents: usize) -> ErrorMetadata {
    ErrorMetadata::pagination_limit(
        "QueryScannedTooManyDocumentsError",
        format!("Query scanned too many documents (fetched {num_documents})."),
    )
}

/// Return a system limit for reading too much data in a query
fn query_scanned_too_much_data(num_bytes: usize) -> ErrorMetadata {
    ErrorMetadata::pagination_limit(
        "QueryScannedTooMuchDataError",
        format!("Query scanned too much data (fetched {num_bytes} bytes)."),
    )
}

pub fn invalid_cursor() -> anyhow::Error {
    let data: anyhow::Result<_> =
        try { val!({ "isConvexSystemError" => true, "paginationError" => "InvalidCursor"}) };
    let message = "InvalidCursor: Tried to run a query starting from a cursor, but it looks like \
                   this cursor is from a different query.";
    anyhow::anyhow!("InvalidCursor")
        .context(JsError::convex_error(
            message.to_string(),
            data.expect("InvalidCursor data should be a valid Value"),
        ))
        .context(ErrorMetadata::bad_request("InvalidCursor", message))
}
