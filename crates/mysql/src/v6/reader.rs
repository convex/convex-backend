//! Index scans with document reads scoped by deployment ID or instance name.
use std::{
    collections::HashMap,
    ops::Bound,
    sync::Arc,
};

use anyhow::Context;
use common::{
    cover,
    index::{
        IndexKeyBytes,
        MAX_INDEX_KEY_PREFIX_LEN,
    },
    interval::Interval,
    knobs::{
        MYSQL_INDEX_SCAN_CONCURRENCY,
        MYSQL_INDEX_SCAN_MIN_ROWS_PER_WORKER,
        MYSQL_MAX_QUERY_BATCH_SIZE,
        MYSQL_MAX_QUERY_DYNAMIC_BATCH_SIZE,
        MYSQL_MIN_QUERY_BATCH_SIZE,
    },
    persistence::{
        DocumentLogEntry,
        LatestDocument,
        RetentionValidator,
    },
    query::Order,
    runtime::Runtime,
    types::{
        IndexRef,
        Timestamp,
    },
    value::{
        InternalDocumentId,
        InternalId,
        TabletId,
    },
};
use errors::ErrorMetadata;
use futures::{
    stream,
    Stream,
    StreamExt,
    TryStreamExt,
};
use futures_async_stream::try_stream_block;
use mysql_async::Value;

use super::{
    column,
    persistence::{
        fetch_document_revisions,
        PublishedBucketBounds,
    },
    sql::{
        self,
        IndexQueryType,
        LogBucket,
        LogBucketBounds,
    },
    PersistenceDeploymentId,
};
use crate::{
    chunks::smart_chunks,
    connection::is_no_such_table_error,
    document_encoding,
    metrics,
    ConvexMySqlPool,
};

/// A persisted index row refers to a missing or deleted document revision.
#[derive(Debug, thiserror::Error)]
#[error("dangling index reference")]
pub(crate) struct DanglingIndexReference;

#[derive(Clone)]
pub(crate) enum DocumentScope {
    DeploymentId,
    #[allow(
        dead_code,
        reason = "Document layouts are selected by optional persistence adapters."
    )]
    InstanceName(String),
}

impl DocumentScope {
    pub(crate) fn tenant_value(&self, deployment_id: PersistenceDeploymentId) -> Value {
        match self {
            Self::DeploymentId => deployment_id.into(),
            Self::InstanceName(instance) => instance.clone().into(),
        }
    }

    fn has_commit_between_sql(&self) -> &'static str {
        match self {
            Self::DeploymentId => sql::HAS_COMMIT_BETWEEN,
            Self::InstanceName(_) => {
                "SELECT 1 FROM @db_name.documents WHERE instance_name = ? AND ts > ? AND ts < ? \
                 LIMIT 1"
            },
        }
    }
}

pub(crate) struct IndexReader<'a, RT: Runtime> {
    pub(crate) published: &'a PublishedBucketBounds,
    pub(crate) pool: &'a ConvexMySqlPool<RT>,
    pub(crate) db_name: &'a str,
    pub(crate) deployment_id: PersistenceDeploymentId,
    pub(crate) documents: DocumentScope,
}

impl<'a, RT: Runtime> IndexReader<'a, RT> {
    pub(crate) fn scan(
        self,
        index: IndexRef,
        tablet_id: TabletId,
        read_timestamp: Timestamp,
        interval: Interval,
        order: Order,
        size_hint: usize,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> impl Stream<Item = anyhow::Result<(IndexKeyBytes, LatestDocument)>> + 'a {
        try_stream_block!({
            retention_validator.optimistic_validate_snapshot(read_timestamp)?;
            let _timer = metrics::query_index_timer(self.pool.cluster_name());
            let mut stats = metrics::QueryIndexStats::new(self.pool.cluster_name());
            let (mut lower, mut upper) = sql::to_sql_bounds(interval.clone());
            let snapshot_bucket = LogBucket::from_successor_ts(read_timestamp);
            let persistence_index_id = index.persistence_index_id().with_context(|| {
                format!(
                    "MySQL V6 requires a persistence index ID to read index {}",
                    index.id()
                )
            })?;
            // The size hint makes the common case one query. Later pages grow to
            // correct for tombstones, long prefixes and a wrong hint.
            let mut page_size =
                size_hint.clamp(*MYSQL_MIN_QUERY_BATCH_SIZE, *MYSQL_MAX_QUERY_BATCH_SIZE);
            let mut buffered_prefix = None;
            let mut buffered = Vec::new();
            let mut refreshed = false;
            loop {
                let concurrency = index_scan_concurrency(page_size);
                let query_type = if concurrency > 1 {
                    IndexQueryType::IndexOnly
                } else {
                    IndexQueryType::JoinDocuments
                };
                let mut connection = self.pool.acquire("v6_index_scan", self.db_name).await?;
                let queried_bounds = self.published.load()?;
                let buckets = queried_bounds.covering(snapshot_bucket);
                let prepare_timer =
                    metrics::query_index_sql_prepare_timer(self.pool.cluster_name());
                let (query, params) = sql::index_query(
                    self.deployment_id,
                    persistence_index_id,
                    read_timestamp,
                    lower.clone(),
                    upper.clone(),
                    order,
                    page_size,
                    &buckets,
                    &self.documents,
                    query_type,
                );
                prepare_timer.finish();
                let execute_timer =
                    metrics::query_index_sql_execute_timer(self.pool.cluster_name());
                stats.sql_statements += 1;
                let mut rows = match connection
                    .query_collect(&query, params, page_size, Ok)
                    .await
                {
                    Ok(rows) => rows,
                    // A stale cached floor can name a bucket maintenance has since
                    // dropped. Refresh once; a repeated failure propagates to the caller.
                    Err(ref error) if is_no_such_table_error(error) && !refreshed => {
                        stats.sql_statements += 1;
                        self.published.refresh(&mut connection).await?;
                        refreshed = true;
                        continue;
                    },
                    Err(error) => return Err(error),
                };
                execute_timer.finish();
                refreshed = false;
                // The page and bounds share an InnoDB read view. Writers observe
                // publication before displacing an entry, so visible displacements
                // fall within these bounds; earlier views retain the entry in
                // `indexes_latest`. Retry if the query omitted a published bucket.
                let state = rows
                    .first()
                    .context("V6 index page is missing its state row")?;
                let created_through_ts: i64 = state.get_opt(9).context("row[9]")??;
                let oldest_kept_ts: i64 = state.get_opt(10).context("row[10]")??;
                self.published.observe(created_through_ts, oldest_kept_ts);
                let bounds = LogBucketBounds::from_state(created_through_ts, oldest_kept_ts)?;
                // Both bucket ranges are contiguous. Only a newer upper endpoint can
                // add a required bucket.
                if bounds.covering(snapshot_bucket).last() > buckets.last() {
                    continue;
                }
                if column::maybe_bytes(state, 1)?.is_none() {
                    rows.clear();
                }
                // Check after reading the page to include concurrent displacements.
                // Below the floor, any later commit may have displaced a revision
                // into an omitted bucket. `write` requires a document revision for
                // every displacement, so the documents table provides this check
                // even after the bucket is dropped.
                //
                // A lagging replica can expose a floor older than the cached
                // floor used to select this page's buckets.
                let floor = bounds.floor.max(queried_bounds.floor);
                if snapshot_bucket < floor {
                    stats.sql_statements += 1;
                    let displaced_since_snapshot = connection
                        .query_optional(
                            self.documents.has_commit_between_sql(),
                            vec![
                                self.documents.tenant_value(self.deployment_id),
                                Value::Int(i64::from(read_timestamp)),
                                Value::Int(i64::from(floor.start_ts()?)),
                            ],
                        )
                        .await?
                        .is_some();
                    if displaced_since_snapshot {
                        cover!(super::coverage::OUT_OF_RETENTION);
                        return Err(out_of_retention_error(
                            read_timestamp,
                            format!(
                                "a later commit displaced revisions into a log bucket below the \
                                 maintenance floor {}",
                                floor.value()
                            ),
                        ));
                    }
                }
                drop(connection);
                let retention_validate_timer =
                    metrics::retention_validate_timer(self.pool.cluster_name());
                retention_validator
                    .validate_snapshot(read_timestamp)
                    .await?;
                retention_validate_timer.finish();
                let documents = match query_type {
                    IndexQueryType::JoinDocuments => None,
                    IndexQueryType::IndexOnly => {
                        let execute_timer =
                            metrics::query_index_sql_execute_timer(self.pool.cluster_name());
                        let documents =
                            self.fetch_documents(&rows, concurrency, &mut stats).await?;
                        execute_timer.finish();
                        let retention_validate_timer =
                            metrics::retention_validate_timer(self.pool.cluster_name());
                        retention_validator
                            .validate_snapshot(read_timestamp)
                            .await?;
                        retention_validate_timer.finish();
                        Some(documents)
                    },
                };
                let rows_loaded = rows.len();
                let mut cursor = None;
                for row in rows {
                    stats.rows_read += 1;
                    let prefix = column::bytes(&row, 1)?.to_vec();
                    let suffix_hash = column::bytes(&row, 2)?.to_vec();
                    cursor = Some(sql::SqlKey {
                        prefix: prefix.clone(),
                        suffix_hash,
                    });
                    if buffered_prefix.as_ref().is_some_and(|p| p != &prefix) {
                        buffered.sort_by(|a: &(IndexKeyBytes, LatestDocument), b| a.0.cmp(&b.0));
                        if order == Order::Desc {
                            buffered.reverse();
                        }
                        for result in buffered.drain(..) {
                            yield result;
                        }
                    }
                    buffered_prefix = Some(prefix.clone());
                    // A prefix shorter than the limit is the whole key, so no other
                    // row can share it: yield now instead of reading the next page
                    // to find where this prefix's run ends.
                    let complete_key = prefix.len() < MAX_INDEX_KEY_PREFIX_LEN;

                    let mut key = prefix;
                    if let Some(suffix) = column::maybe_bytes(&row, 3)? {
                        cover!(super::coverage::LONG_KEY_SUFFIX);
                        key.extend_from_slice(suffix);
                    }
                    let key = IndexKeyBytes(key);
                    if !interval.contains(&key) {
                        stats.rows_skipped_out_of_range += 1;
                        continue;
                    }
                    let ts: i64 = row.get_opt(4).context("row[4]")??;
                    let ts = Timestamp::try_from(ts)?;
                    let table_id = TabletId(InternalId::try_from(column::bytes(&row, 5)?)?);
                    anyhow::ensure!(table_id == tablet_id);
                    let document = match query_type {
                        IndexQueryType::IndexOnly => {
                            let documents = documents
                                .as_ref()
                                .context("Index-only page requires separately fetched documents")?;
                            let id = InternalDocumentId::new(
                                table_id,
                                InternalId::try_from(column::bytes(&row, 6)?)?,
                            );
                            let entry = documents
                                .get(&(id, ts))
                                .context(DanglingIndexReference)
                                .with_context(|| {
                                    format!("Dangling index reference for {key:?} {ts:?}")
                                })?;
                            let value = entry
                                .value
                                .clone()
                                .context(DanglingIndexReference)
                                .with_context(|| {
                                    format!("Index reference to deleted document {key:?} {ts:?}")
                                })?;
                            LatestDocument {
                                ts,
                                value,
                                prev_ts: entry.prev_ts,
                            }
                        },
                        IndexQueryType::JoinDocuments => {
                            anyhow::ensure!(
                                documents.is_none(),
                                "Unexpected separately fetched documents for joined index query"
                            );
                            let encoded = column::maybe_bytes(&row, 7)?
                                .context(DanglingIndexReference)
                                .with_context(|| {
                                    format!("Dangling index reference for {key:?} {ts:?}")
                                })?;
                            let value = document_encoding::decode(encoded, table_id)?
                                .context(DanglingIndexReference)
                                .with_context(|| {
                                    format!("Index reference to deleted document {key:?} {ts:?}")
                                })?;
                            let prev_ts: Option<i64> = row.get_opt(8).context("row[8]")??;
                            LatestDocument {
                                ts,
                                value,
                                prev_ts: prev_ts.map(Timestamp::try_from).transpose()?,
                            }
                        },
                    };
                    buffered.push((key, document));
                    stats.rows_returned += 1;
                    stats.max_rows_buffered = stats.max_rows_buffered.max(buffered.len());
                    if complete_key {
                        buffered_prefix = None;
                        for result in buffered.drain(..) {
                            yield result;
                        }
                    }
                }
                if rows_loaded < page_size {
                    break;
                }
                let cursor = cursor.context("full V6 index page has no cursor")?;
                cover!(super::coverage::SCAN_RESUMED);
                match order {
                    Order::Asc => lower = Bound::Excluded(cursor),
                    Order::Desc => upper = Bound::Excluded(cursor),
                }
                if page_size < *MYSQL_MAX_QUERY_DYNAMIC_BATCH_SIZE {
                    page_size = (page_size * 2).min(*MYSQL_MAX_QUERY_DYNAMIC_BATCH_SIZE);
                }
            }
            buffered.sort_by(|a: &(IndexKeyBytes, LatestDocument), b| a.0.cmp(&b.0));
            if order == Order::Desc {
                buffered.reverse();
            }
            for result in buffered {
                yield result;
            }
        })
    }

    async fn fetch_documents(
        &self,
        rows: &[mysql_async::Row],
        concurrency: usize,
        stats: &mut metrics::QueryIndexStats<'_>,
    ) -> anyhow::Result<HashMap<(InternalDocumentId, Timestamp), DocumentLogEntry>> {
        let pointers = rows
            .iter()
            .map(|row| {
                let table = TabletId(InternalId::try_from(column::bytes(row, 5)?)?);
                let id = InternalId::try_from(column::bytes(row, 6)?)?;
                let ts = Timestamp::try_from(row.get_opt::<i64, _>(4).context("row[4]")??)?;
                anyhow::Ok((InternalDocumentId::new(table, id), ts))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let queries: Vec<_> = smart_chunks(&pointers)
            .map(|chunk| {
                fetch_document_revisions(
                    self.pool,
                    self.db_name,
                    self.deployment_id,
                    &self.documents,
                    chunk,
                )
            })
            .collect();
        stats.sql_statements += queries.len();
        let results: Vec<_> = stream::iter(queries)
            .buffer_unordered(concurrency)
            .try_collect()
            .await?;
        // Exact-revision SQL can reorder and deduplicate pointers. The captured
        // index page preserves their scan order, even if log buckets are dropped.
        Ok(results
            .into_iter()
            .flatten()
            .map(|entry| ((entry.id, entry.ts), entry))
            .collect())
    }
}

/// Parallel fetching adds a database round trip, so each worker needs enough
/// documents to amortize that latency. Small pages use the JOIN.
fn index_scan_concurrency(page_size: usize) -> usize {
    (*MYSQL_INDEX_SCAN_CONCURRENCY)
        .get()
        .min(page_size / (*MYSQL_INDEX_SCAN_MIN_ROWS_PER_WORKER).get())
        .max(1)
}

fn out_of_retention_error(read_timestamp: Timestamp, reason: String) -> anyhow::Error {
    anyhow::anyhow!(ErrorMetadata::out_of_retention()).context(format!(
        "V6 index snapshot {read_timestamp} is outside the retained log buckets: {reason}"
    ))
}
