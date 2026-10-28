use std::{
    cmp,
    collections::{
        BTreeMap,
        BTreeSet,
        HashMap,
    },
    iter,
    ops::Bound,
    sync::{
        atomic::{
            AtomicBool,
            Ordering::SeqCst,
        },
        Arc,
    },
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use anyhow::Context;
use async_trait::async_trait;
use common::{
    document::{
        InternalId,
        ResolvedDocument,
    },
    errors::lease_lost_error,
    index::{
        IndexEntry,
        IndexKeyBytes,
        MAX_INDEX_KEY_PREFIX_LEN,
    },
    interval::{
        BinaryKey,
        Interval,
    },
    knobs::{
        MYSQL_FALLBACK_PAGE_SIZE,
        MYSQL_MAX_QUERY_BATCH_SIZE,
        MYSQL_MAX_QUERY_DYNAMIC_BATCH_SIZE,
        MYSQL_MIN_QUERY_BATCH_SIZE,
    },
    persistence::{
        row_index_retention::{
            delete_expired_entries,
            IndexRowPersistence,
        },
        ConflictStrategy,
        DocumentLogEntry,
        DocumentPrevTsQuery,
        DocumentRevisionStream,
        DocumentStream,
        IndexRetentionProgress,
        IndexRetentionRequest,
        IndexStream,
        LatestDocument,
        Persistence as PersistenceTrait,
        PersistenceGlobalKey,
        PersistenceIndexEntry,
        PersistenceReader,
        PersistenceTableSize,
        RetentionValidator,
        TimestampRange,
    },
    persistence_helpers::{
        DocumentRevision,
        RevisionPair,
    },
    query::Order,
    runtime::{
        CoopStreamExt as _,
        Runtime,
    },
    sha256::Sha256,
    shutdown::ShutdownSignal,
    try_anyhow,
    types::{
        IndexId,
        IndexRef,
        PersistenceVersion,
        Timestamp,
    },
    value::{
        InternalDocumentId,
        TabletId,
    },
};
use fastrace::prelude::*;
use futures::{
    pin_mut,
    stream::{
        StreamExt,
        TryStreamExt,
    },
};
use futures_async_stream::try_stream;
use mysql_async::{
    IsolationLevel,
    Row,
    Value,
};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use smallvec::SmallVec;

use crate::{
    chunks::{
        smart_chunks,
        ApproxSize,
    },
    connection::{
        is_message_too_large_error,
        MySqlConnection,
        MySqlTransaction,
    },
    document_encoding,
    metrics::{
        self,
        log_prev_revisions_row_read,
        write_persistence_global_timer,
        QueryIndexStats,
    },
    ConvexMySqlPool,
    MySqlInstanceName,
    MySqlOptions,
    MySqlReaderOptions,
};

pub struct Persistence<RT: Runtime> {
    newly_created: AtomicBool,
    pub(crate) lease: Lease<RT>,

    // Used by the reader.
    pub(super) read_pool: Arc<ConvexMySqlPool<RT>>,
    pub(super) db_name: String,
    pub(super) instance_name: MySqlInstanceName,
    pub(super) multitenant: bool,
}

impl<RT: Runtime> Persistence<RT> {
    pub async fn new(
        pool: Arc<ConvexMySqlPool<RT>>,
        db_name: String,
        options: MySqlOptions,
        lease_lost_shutdown: ShutdownSignal,
    ) -> anyhow::Result<Self> {
        if options.version != PersistenceVersion::V5 {
            return Err(anyhow::anyhow!(
                "V5 persistence cannot open a non-V5 database"
            ));
        }
        let expected_table_count = super::EXPECTED_TABLE_COUNT;
        let init_sql = super::init_sql(options.multitenant);
        let init_lease = super::init_lease(options.multitenant);
        {
            let mut client = pool.acquire("init_sql", &db_name).await?;
            let table_count: usize = client
                .query_optional(super::GET_TABLE_COUNT, vec![(&db_name).into()])
                .await?
                .context("GET_TABLE_COUNT query returned no rows?")?
                .get(0)
                .context("GET_TABLE_COUNT query returned zero columns?")?;
            // Only run INIT_SQL if we have less tables than we expect. We suspect
            // CREATE TABLE IF EXISTS is creating lock contention due to acquiring
            // an exclusive lock https://bugs.mysql.com/bug.php?id=63144.
            if table_count < expected_table_count {
                tracing::info!("Initializing MySQL Persistence...");
                client.execute_many(init_sql).await?;
            } else {
                tracing::info!("MySQL Persistence already initialized");
            }
            client
                .exec_iter(
                    init_lease,
                    if options.multitenant {
                        vec![(&options.instance_name.raw).into()]
                    } else {
                        vec![]
                    },
                )
                .await?;
        };

        let lease = Lease::acquire(
            pool.clone(),
            db_name.clone(),
            options.instance_name.clone(),
            options.multitenant,
            options.allow_read_only,
            lease_lost_shutdown,
        )
        .await?;
        let newly_created = {
            let mut client = pool.acquire("check_newly_created", &db_name).await?;
            Self::check_newly_created(&mut client, options.multitenant, &options.instance_name)
                .await?
        };
        Ok(Self {
            newly_created: newly_created.into(),
            lease,
            read_pool: pool,
            db_name,
            instance_name: options.instance_name,
            multitenant: options.multitenant,
        })
    }

    pub async fn set_read_only(
        pool: Arc<ConvexMySqlPool<RT>>,
        db_name: String,
        options: MySqlOptions,
        read_only: bool,
    ) -> anyhow::Result<()> {
        let multitenant = options.multitenant;
        let instance_name = mysql_async::Value::from(&options.instance_name.raw);
        let params = if multitenant {
            vec![instance_name]
        } else {
            vec![]
        };
        let mut conn = pool.acquire("set_read_only", &db_name).await?;
        anyhow::ensure!(
            options.version == PersistenceVersion::V5,
            "V5 persistence cannot change read-only state for a non-V5 database"
        );
        let statement = if read_only {
            super::set_read_only(multitenant)
        } else {
            super::unset_read_only(multitenant)
        };
        conn.exec_iter(statement, params).await?;
        Ok(())
    }

    pub fn new_reader(
        pool: Arc<ConvexMySqlPool<RT>>,
        db_name: String,
        options: MySqlReaderOptions,
    ) -> Reader<RT> {
        Reader {
            db_name,
            read_pool: pool,
            instance_name: options.instance_name,
            multitenant: options.multitenant,
        }
    }

    async fn check_newly_created(
        client: &mut MySqlConnection<'_, RT>,
        multitenant: bool,
        instance_name: &MySqlInstanceName,
    ) -> anyhow::Result<bool> {
        let mut params = vec![];
        if multitenant {
            params.push((&instance_name.raw).into());
        }
        Ok(client
            .query_optional(super::documents::check_newly_created(multitenant), params)
            .await?
            .is_none())
    }

    /// Runs `f` in the transaction that writes the V5 rows, so an error from
    /// either rolls back the other's writes as well.
    #[fastrace::trace]
    pub(crate) async fn write_in_transaction<'a, F>(
        &self,
        documents: &'a [DocumentLogEntry],
        indexes: &'a [PersistenceIndexEntry],
        conflict_strategy: ConflictStrategy,
        f: F,
    ) -> anyhow::Result<()>
    where
        F: for<'tx> AsyncFnOnce(&'tx mut MySqlTransaction<'_>) -> anyhow::Result<()>,
    {
        anyhow::ensure!(documents.len() <= crate::MAX_INSERT_SIZE);
        let mut write_size = 0;
        for update in documents {
            match &update.value {
                Some(doc) => {
                    anyhow::ensure!(update.id == doc.id_with_table_id());
                    write_size += doc.size();
                },
                None => {},
            }
        }
        metrics::log_write_bytes(write_size);
        let index_write_size = indexes.iter().map(|entry| entry.approx_size()).sum();
        metrics::log_index_write_bytes(index_write_size);
        metrics::log_write_documents(documents.len());
        LocalSpan::add_properties(|| {
            [
                ("num_documents", documents.len().to_string()),
                ("write_size", write_size.to_string()),
            ]
        });

        // True, the below might end up failing and not changing anything.
        self.newly_created.store(false, SeqCst);
        let cluster_name = self.read_pool.cluster_name().to_owned();
        let multitenant = self.multitenant;
        let instance_name = mysql_async::Value::from(&self.instance_name.raw);
        self.lease
            .transact(async move |tx| {
                // First, process all of the full document chunks.
                let mut document_chunks = smart_chunks(documents);
                for chunk in &mut document_chunks {
                    let chunk_bytes: usize = chunk.iter().map(|item| item.approx_size()).sum();
                    let insert_chunk_query = match conflict_strategy {
                        ConflictStrategy::Error => {
                            super::documents::insert_document_chunk(chunk.len(), multitenant)
                        },
                        ConflictStrategy::Overwrite => {
                            super::documents::insert_overwrite_document_chunk(
                                chunk.len(),
                                multitenant,
                            )
                        },
                    };
                    let mut insert_document_chunk = Vec::with_capacity(
                        chunk.len()
                            * (super::documents::INSERT_DOCUMENT_COLUMN_COUNT
                                + (multitenant as usize)),
                    );
                    for update in chunk {
                        if multitenant {
                            insert_document_chunk.push(instance_name.clone());
                        }
                        insert_document_chunk = document_params(
                            insert_document_chunk,
                            update.ts,
                            update.id,
                            update.value.as_ref(),
                            update.prev_ts,
                        )?;
                    }
                    let future = async {
                        let timer = metrics::insert_document_chunk_timer(cluster_name.as_str());
                        tx.exec_drop(insert_chunk_query, insert_document_chunk)
                            .await?;
                        timer.finish();
                        Ok::<_, anyhow::Error>(())
                    };
                    future
                        .in_span(
                            Span::enter_with_local_parent(format!(
                                "{}::document_chunk_write",
                                func_path!()
                            ))
                            .with_properties(|| {
                                [
                                    ("chunk_length", chunk.len().to_string()),
                                    ("chunk_bytes", chunk_bytes.to_string()),
                                ]
                            }),
                        )
                        .await?;
                }

                let mut index_chunks = smart_chunks(indexes);
                for chunk in &mut index_chunks {
                    let chunk_bytes: usize = chunk.iter().map(|item| item.approx_size()).sum();
                    let future = async {
                        let timer = metrics::insert_index_chunk_timer(cluster_name.as_str());
                        super::indexes::write_index_chunk(
                            tx,
                            chunk,
                            conflict_strategy,
                            multitenant,
                            &instance_name,
                        )
                        .await?;
                        timer.finish();
                        Ok::<_, anyhow::Error>(())
                    };
                    future
                        .in_span(
                            Span::enter_with_local_parent(format!(
                                "{}::index_chunk_write",
                                func_path!()
                            ))
                            .with_properties(|| {
                                [
                                    ("chunk_length", chunk.len().to_string()),
                                    ("chunk_bytes", chunk_bytes.to_string()),
                                ]
                            }),
                        )
                        .await?;
                }
                f(tx).await
            })
            .await
    }
}

#[async_trait]
impl<RT: Runtime> IndexRowPersistence for Persistence<RT> {
    async fn delete_index_rows(&self, expired_entries: Vec<IndexEntry>) -> anyhow::Result<usize> {
        super::indexes::delete_index_rows(self, expired_entries).await
    }
}

#[async_trait]
impl<RT: Runtime> PersistenceTrait for Persistence<RT> {
    fn is_fresh(&self) -> bool {
        self.newly_created.load(SeqCst)
    }

    fn reader(&self) -> Arc<dyn PersistenceReader> {
        Arc::new(Reader {
            db_name: self.db_name.clone(),
            read_pool: self.read_pool.clone(),
            instance_name: self.instance_name.clone(),
            multitenant: self.multitenant,
        })
    }

    async fn write<'a>(
        &self,
        documents: &'a [DocumentLogEntry],
        indexes: &'a [PersistenceIndexEntry],
        conflict_strategy: ConflictStrategy,
    ) -> anyhow::Result<()> {
        self.write_in_transaction(documents, indexes, conflict_strategy, async |_| Ok(()))
            .await
    }

    async fn write_persistence_global(
        &self,
        key: PersistenceGlobalKey,
        value: JsonValue,
    ) -> anyhow::Result<()> {
        let timer = write_persistence_global_timer(self.read_pool.cluster_name(), key);
        let multitenant = self.multitenant;
        let instance_name = mysql_async::Value::from(&self.instance_name.raw);
        self.lease
            .transact(async move |tx| {
                let stmt = super::write_persistence_global(multitenant);
                let mut params = if multitenant {
                    vec![instance_name]
                } else {
                    vec![]
                };
                params.extend([String::from(key).into(), value.into()]);
                tx.exec_drop(stmt, params).await?;
                Ok(())
            })
            .await?;
        timer.finish();
        Ok(())
    }

    async fn has_index_entries(&self) -> anyhow::Result<bool> {
        super::indexes::has_index_entries(self).await
    }

    async fn reclaim_index_history(
        &self,
        request: IndexRetentionRequest<'_>,
    ) -> anyhow::Result<IndexRetentionProgress> {
        delete_expired_entries(self, request).await
    }

    async fn delete(
        &self,
        mut documents: Vec<(Timestamp, InternalDocumentId)>,
    ) -> anyhow::Result<usize> {
        let multitenant = self.multitenant;
        let instance_name = mysql_async::Value::from(&self.instance_name.raw);
        documents.sort_unstable_by_key(|d| d.1);
        // We implicitly delete all timestamps less than `d.0`, so just keep the
        // highest timestamp for each document id.
        documents.dedup_by(|a, b| {
            if a.1 == b.1 {
                // N.B.: returning `true` to dedup_by deletes `a`, so update `b`.
                b.0 = b.0.max(a.0);
                true
            } else {
                false
            }
        });
        self.lease
            .transact(async move |tx| {
                let mut deleted_count = 0;
                for chunk in smart_chunks(&documents) {
                    let mut params = Vec::with_capacity(
                        chunk.len()
                            * (super::documents::DELETE_DOCUMENT_COLUMN_COUNT
                                + (multitenant as usize)),
                    );
                    for doc in chunk.iter() {
                        Reader::<RT>::_document_delete_params(&mut params, doc);
                        if multitenant {
                            params.push(instance_name.clone());
                        }
                    }
                    deleted_count += tx
                        .exec_iter(
                            super::documents::delete_document_chunk(chunk.len(), multitenant),
                            params,
                        )
                        .await?;
                }
                Ok(deleted_count as usize)
            })
            .await
    }

    async fn delete_tablet_documents(
        &self,
        tablet_id: TabletId,
        chunk_size: usize,
    ) -> anyhow::Result<usize> {
        let multitenant = self.multitenant;
        let instance_name = mysql_async::Value::from(&self.instance_name.raw);
        self.lease
            .transact(async move |tx| {
                let mut deleted_count = 0;
                let mut params = Vec::with_capacity(
                    super::documents::DELETE_TABLE_COLUMN_COUNT + (multitenant as usize),
                );
                let tablet_id: Vec<u8> = tablet_id.0.into();
                params.push(tablet_id.into());
                if multitenant {
                    params.push(instance_name.clone());
                }
                params.push(chunk_size.into());
                deleted_count += tx
                    .exec_iter(super::documents::delete_tablet_chunk(multitenant), params)
                    .await?;
                Ok(deleted_count as usize)
            })
            .await
    }
}

#[derive(Clone)]
pub struct Reader<RT: Runtime> {
    read_pool: Arc<ConvexMySqlPool<RT>>,
    db_name: String,
    instance_name: MySqlInstanceName,
    multitenant: bool,
}

fn maybe_bytes_col(row: &Row, col: usize) -> anyhow::Result<Option<&[u8]>> {
    match row.as_ref(col) {
        Some(Value::Bytes(b)) => Ok(Some(b)),
        Some(Value::NULL) => Ok(None),
        _ => anyhow::bail!("row[{col}] must be Bytes or NULL"),
    }
}
fn bytes_col(row: &Row, col: usize) -> anyhow::Result<&[u8]> {
    match row.as_ref(col) {
        Some(Value::Bytes(b)) => Ok(b),
        _ => anyhow::bail!("row[{col}] must be Bytes"),
    }
}

impl<RT: Runtime> Reader<RT> {
    fn initial_id_param(order: Order) -> Vec<u8> {
        match order {
            Order::Asc => InternalId::BEFORE_ALL_BYTES.to_vec(),
            Order::Desc => InternalId::AFTER_ALL_BYTES.to_vec(),
        }
    }

    fn row_to_document(
        row: &Row,
    ) -> anyhow::Result<(
        Timestamp,
        InternalDocumentId,
        Option<ResolvedDocument>,
        Option<Timestamp>,
    )> {
        let internal_id = InternalId::try_from(bytes_col(row, 0)?)?;
        let ts: i64 = row.get_opt(1).context("row[1]")??;
        let ts = Timestamp::try_from(ts)?;
        let table_b = bytes_col(row, 2)?;
        let encoded_value = bytes_col(row, 3)?;
        let deleted: bool = row.get_opt(4).context("row[4]")??;
        let table = TabletId(table_b[..].try_into()?);
        let document_id = InternalDocumentId::new(table, internal_id);
        let document = if !deleted {
            Some(
                document_encoding::decode(encoded_value, table)?
                    .context("deleted=false but value is empty")?,
            )
        } else {
            None
        };
        let prev_ts: Option<i64> = row.get_opt(5).context("row[5]")??;
        let prev_ts = prev_ts.map(Timestamp::try_from).transpose()?;
        Ok((ts, document_id, document, prev_ts))
    }

    // If `include_prev_rev` is false then the returned
    // RevisionPair.prev_rev.document will always be None (but prev_rev.ts will
    // still be correct)
    #[try_stream(
        ok = RevisionPair,
        error = anyhow::Error,
    )]
    async fn _load_documents(
        &self,
        tablet_id: Option<TabletId>,
        include_prev_rev: bool,
        range: TimestampRange,
        order: Order,
        mut page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) {
        anyhow::ensure!(page_size > 0); // 0 size pages loop forever.
        let timer = metrics::load_documents_timer(self.read_pool.cluster_name());
        let mut num_returned = 0;
        let mut last_ts = match order {
            Order::Asc => Timestamp::MIN,
            Order::Desc => Timestamp::MAX,
        };
        let mut last_tablet_id_param = Self::initial_id_param(order);
        let mut last_id_param = Self::initial_id_param(order);
        loop {
            // Avoid holding connections across yield points, to limit lifetime
            // and improve fairness.
            let mut client = self
                .read_pool
                .acquire("load_documents", &self.db_name)
                .await?;

            let query = match order {
                Order::Asc => super::documents::load_docs_by_ts_page_asc(
                    self.multitenant,
                    tablet_id.is_some(),
                    include_prev_rev,
                ),
                Order::Desc => super::documents::load_docs_by_ts_page_desc(
                    self.multitenant,
                    tablet_id.is_some(),
                    include_prev_rev,
                ),
            };
            let mut params = vec![
                i64::from(range.min_timestamp_inclusive()).into(),
                i64::from(range.max_timestamp_exclusive()).into(),
                i64::from(last_ts).into(),
                i64::from(last_ts).into(),
                last_tablet_id_param.clone().into(),
                last_tablet_id_param.clone().into(),
                last_id_param.clone().into(),
            ];
            if let Some(tablet_id) = tablet_id {
                params.push(tablet_id.0 .0.into());
            }
            if self.multitenant {
                params.push(self.instance_name.to_string().into());
            }
            params.push((page_size as i64).into());
            let rows = match client
                .query_collect(query, params, page_size as usize, Ok)
                .await
            {
                Ok(rows) => Ok(rows),
                Err(ref e) if let Some(db_err) = is_message_too_large_error(e) => {
                    if page_size == 1 {
                        anyhow::bail!(
                            "Failed to load documents with minimum page size `1`: {}",
                            db_err.message,
                        );
                    }
                    if page_size == *MYSQL_FALLBACK_PAGE_SIZE {
                        tracing::warn!(
                            "Falling back to page size `1` due to repeated server error: {}",
                            db_err.message
                        );
                        page_size = 1;
                    } else {
                        tracing::warn!(
                            "Falling back to page size `{}` due to server error: {}",
                            *MYSQL_FALLBACK_PAGE_SIZE,
                            db_err.message
                        );
                        page_size = *MYSQL_FALLBACK_PAGE_SIZE;
                    }
                    continue;
                },
                Err(e) => Err(e),
            }?;
            drop(client);

            retention_validator
                .validate_document_snapshot(range.min_timestamp_inclusive())
                .await?;

            let rows_loaded = rows.len();
            for row in rows {
                let (ts, document_id, document, prev_ts) = Self::row_to_document(&row)?;
                let prev_rev_document: Option<ResolvedDocument> = if include_prev_rev {
                    maybe_bytes_col(&row, 6)?
                        .map(|v| {
                            // N.B.: previous revisions should never be deleted, so we don't check
                            // that.
                            anyhow::Ok(
                                document_encoding::decode(v, document_id.table())?
                                    .context("previous revisions should never be deleted")?,
                            )
                        })
                        .transpose()?
                } else {
                    None
                };
                last_ts = ts;
                last_tablet_id_param = internal_id_param(document_id.table().0);
                last_id_param = internal_doc_id_param(document_id);
                num_returned += 1;
                yield RevisionPair {
                    id: document_id,
                    rev: DocumentRevision { ts, document },
                    prev_rev: prev_ts.map(|prev_ts| DocumentRevision {
                        ts: prev_ts,
                        document: prev_rev_document,
                    }),
                }
            }
            if rows_loaded < page_size as usize {
                break;
            }
        }

        metrics::finish_load_documents_timer(timer, num_returned, self.read_pool.cluster_name());
    }

    #[allow(clippy::needless_lifetimes)]
    #[try_stream(ok = (IndexKeyBytes, LatestDocument), error = anyhow::Error)]
    async fn _index_scan(
        &self,
        index_id: IndexId,
        tablet_id: TabletId,
        read_timestamp: Timestamp,
        interval: Interval,
        order: Order,
        size_hint: usize,
        retention_validator: Arc<dyn RetentionValidator>,
    ) {
        if let Some(key) = interval.is_singleton()
            && key.len() <= MAX_INDEX_KEY_PREFIX_LEN
        {
            // Fast path for looking up a single value
            if let Some(doc) = self
                .index_point_query(index_id, key, read_timestamp, retention_validator)
                .await?
            {
                anyhow::ensure!(doc.value.id().tablet_id == tablet_id);
                yield (IndexKeyBytes(key.to_vec()), doc);
            }
            return Ok(());
        }
        let scan = self._index_scan_inner(
            index_id,
            read_timestamp,
            interval,
            order,
            size_hint,
            retention_validator,
        );
        pin_mut!(scan);
        while let Some((key, ts, document, prev_ts)) = scan.try_next().await? {
            anyhow::ensure!(document.id().tablet_id == tablet_id);
            yield (
                key,
                LatestDocument {
                    ts,
                    value: document,
                    prev_ts,
                },
            );
        }
    }

    #[allow(clippy::needless_lifetimes)]
    #[try_stream(
        ok = (IndexKeyBytes, Timestamp, ResolvedDocument, Option<Timestamp>),
        error = anyhow::Error
    )]
    async fn _index_scan_inner(
        &self,
        index_id: IndexId,
        read_timestamp: Timestamp,
        interval: Interval,
        order: Order,
        size_hint: usize,
        retention_validator: Arc<dyn RetentionValidator>,
    ) {
        let _timer = metrics::query_index_timer(self.read_pool.cluster_name());
        let (mut lower, mut upper) = super::indexes::to_sql_bounds(interval.clone());

        let mut stats = QueryIndexStats::new(self.read_pool.cluster_name());

        // We use the size_hint to determine the batch size. This means in the
        // common case we should do a single query. Exceptions are if the size_hint
        // is wrong or if we truncate it or if we observe too many deletes.
        let mut batch_size =
            size_hint.clamp(*MYSQL_MIN_QUERY_BATCH_SIZE, *MYSQL_MAX_QUERY_BATCH_SIZE) as u32;

        // We iterate results in (key_prefix, key_sha256) order while we actually
        // need them in (key_prefix, key_suffix order). key_suffix is not part of the
        // primary key so we do the sort here. If see any record with maximum length
        // prefix, we should buffer it until we reach a different prefix.
        let mut result_buffer: Vec<(
            IndexKeyBytes,
            Timestamp,
            ResolvedDocument,
            Option<Timestamp>,
        )> = Vec::new();
        let mut has_more = true;
        let mut fallback = false;
        while has_more {
            // Avoid holding connections across yield points, to limit lifetime
            // and improve fairness.
            let mut client = self.read_pool.acquire("index_scan", &self.db_name).await?;
            stats.sql_statements += 1;
            let (query, params) = super::indexes::index_query(
                index_id,
                read_timestamp,
                lower.clone(),
                upper.clone(),
                order,
                batch_size as usize,
                self.multitenant,
                &self.instance_name,
            );

            let prepare_timer =
                metrics::query_index_sql_prepare_timer(self.read_pool.cluster_name());
            prepare_timer.finish();

            let execute_timer =
                metrics::query_index_sql_execute_timer(self.read_pool.cluster_name());
            let rows = match client
                .query_collect(query, params, batch_size as usize, Ok)
                .await
            {
                Ok(rows) => Ok(rows),
                Err(ref e) if let Some(db_err) = is_message_too_large_error(e) => {
                    if batch_size == 1 {
                        anyhow::bail!(
                            "Failed to load index rows with minimum page size `1`: {}",
                            db_err.message
                        );
                    }
                    if batch_size == *MYSQL_FALLBACK_PAGE_SIZE {
                        tracing::warn!(
                            "Falling back to page size `1` due to repeated server error: {}",
                            db_err.message
                        );
                        batch_size = 1;
                    } else {
                        tracing::warn!(
                            "Falling back to page size `{}` due to server error: {}",
                            *MYSQL_FALLBACK_PAGE_SIZE,
                            db_err.message
                        );
                        batch_size = *MYSQL_FALLBACK_PAGE_SIZE;
                    }
                    fallback = true;
                    continue;
                },
                Err(e) => Err(e),
            }?;
            execute_timer.finish();
            drop(client);

            let retention_validate_timer =
                metrics::retention_validate_timer(self.read_pool.cluster_name());
            retention_validator
                .validate_snapshot(read_timestamp)
                .await?;
            retention_validate_timer.finish();

            let batch_rows = rows.len();
            for mut row in rows {
                stats.rows_read += 1;

                // Fetch
                let internal_row = parse_row(&mut row)?;

                // Yield buffered results if applicable.
                if let Some((buffer_key, ..)) = result_buffer.first()
                    && buffer_key[..MAX_INDEX_KEY_PREFIX_LEN] != internal_row.key_prefix
                {
                    // We have exhausted all results that share the same key prefix
                    // we can sort and yield the buffered results.
                    result_buffer.sort_by(|a, b| a.0.cmp(&b.0));
                    for (key, ts, doc, prev_ts) in order.apply(result_buffer.drain(..)) {
                        if interval.contains(&key) {
                            stats.rows_returned += 1;
                            yield (key, ts, doc, prev_ts);
                        } else {
                            stats.rows_skipped_out_of_range += 1;
                        }
                    }
                }

                // Update the bounds for future queries.
                let bound = Bound::Excluded(super::indexes::SqlKey {
                    prefix: internal_row.key_prefix.clone(),
                    sha256: internal_row.key_sha256.clone(),
                });
                match order {
                    Order::Asc => lower = bound,
                    Order::Desc => upper = bound,
                }

                // Filter if needed.
                if internal_row.deleted {
                    stats.rows_skipped_deleted += 1;
                    continue;
                }

                // Construct key.
                let mut key = internal_row.key_prefix;
                if let Some(key_suffix) = internal_row.key_suffix {
                    key.extend(key_suffix);
                };
                let ts = internal_row.ts;

                // Fetch the remaining columns and construct the document
                let table_b = maybe_bytes_col(&row, 7)?;
                let table = TabletId(
                    table_b
                        .ok_or_else(|| {
                            anyhow::anyhow!("Dangling index reference for {:?} {:?}", key, ts)
                        })?
                        .try_into()?,
                );
                let doc =
                    document_encoding::decode(bytes_col(&row, 8)?, table)?.with_context(|| {
                        format!("Index reference to deleted document {key:?} {ts:?}")
                    })?;

                let prev_ts: Option<i64> = row.get_opt(9).context("row[9]")??;
                let prev_ts = prev_ts.map(Timestamp::try_from).transpose()?;

                if key.len() < MAX_INDEX_KEY_PREFIX_LEN {
                    assert!(result_buffer.is_empty());
                    if interval.contains(&key) {
                        stats.rows_returned += 1;
                        yield (IndexKeyBytes(key), ts, doc, prev_ts);
                    } else {
                        stats.rows_skipped_out_of_range += 1;
                    }
                } else {
                    // There might be other records with the same key_prefix that
                    // are ordered before this result. Buffer it.
                    result_buffer.push((IndexKeyBytes(key), ts, doc, prev_ts));
                    stats.max_rows_buffered =
                        cmp::max(result_buffer.len(), stats.max_rows_buffered);
                }
            }

            if batch_rows < batch_size as usize {
                // Yield any remaining values.
                result_buffer.sort_by(|a, b| a.0.cmp(&b.0));
                for (key, ts, doc, prev_ts) in order.apply(result_buffer.drain(..)) {
                    if interval.contains(&key) {
                        stats.rows_returned += 1;
                        yield (key, ts, doc, prev_ts);
                    } else {
                        stats.rows_skipped_out_of_range += 1;
                    }
                }
                has_more = false;
            }

            // Double the batch size every iteration until we max dynamic batch size. This
            // helps correct for tombstones, long prefixes and wrong client
            // size estimates.
            // If we've had to fall back to the fallback page size, stay there without
            // doubling. TODO: Take size into consideration and increase the max
            // dynamic batch size.
            if batch_size < *MYSQL_MAX_QUERY_DYNAMIC_BATCH_SIZE as u32 && !fallback {
                batch_size = (batch_size * 2).min(*MYSQL_MAX_QUERY_DYNAMIC_BATCH_SIZE as u32);
            }
        }
    }

    async fn index_point_query(
        &self,
        index_id: IndexId,
        key: &BinaryKey,
        read_timestamp: Timestamp,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<Option<LatestDocument>> {
        let mut client = self
            .read_pool
            .acquire("index_lookup", &self.db_name)
            .await?;
        let key_prefix = key.to_vec();
        anyhow::ensure!(key_prefix.len() <= MAX_INDEX_KEY_PREFIX_LEN);
        let key_sha256 = Sha256::hash(key);
        let execute_timer =
            metrics::query_index_point_sql_execute_timer(self.read_pool.cluster_name());
        let mut params: Vec<mysql_async::Value> = vec![
            internal_id_param(index_id.0).into(),
            key_prefix.into(),
            key_sha256.to_vec().into(),
            i64::from(read_timestamp).into(),
        ];
        if self.multitenant {
            params.push(self.instance_name.to_string().into());
        }
        let maybe_row = client
            .query_optional(super::indexes::index_point_query(self.multitenant), params)
            .await?;
        execute_timer.finish();

        let retention_validate_timer =
            metrics::retention_validate_timer(self.read_pool.cluster_name());
        retention_validator
            .validate_snapshot(read_timestamp)
            .await?;
        retention_validate_timer.finish();

        let Some(row) = maybe_row else {
            return Ok(None);
        };
        let ts = Timestamp::try_from(row.get_opt::<i64, _>(0).context("row[0]")??)?;
        let tablet_id = TabletId(bytes_col(&row, 1)?.try_into()?);
        let value = maybe_bytes_col(&row, 2)?
            .ok_or_else(|| anyhow::anyhow!("Dangling index reference for {:?} {:?}", key, ts))?;
        let prev_ts: Option<i64> = row.get_opt(3).context("row[3]")??;
        let prev_ts = prev_ts.map(Timestamp::try_from).transpose()?;
        let value = document_encoding::decode(value, tablet_id)?
            .with_context(|| format!("Index reference to deleted document {key:?} {ts:?}"))?;
        Ok(Some(LatestDocument { ts, value, prev_ts }))
    }

    pub(super) fn _index_delete_params(query: &mut Vec<mysql_async::Value>, entry: &IndexEntry) {
        let last_id_param: Vec<u8> = entry.index_id.0.into();
        let last_key_prefix: Vec<u8> = entry.key_prefix.clone();
        let last_sha256: Vec<u8> = entry.key_sha256.clone();
        let last_ts: u64 = entry.ts.into();
        query.push(last_id_param.into());
        query.push(last_key_prefix.into());
        query.push(last_sha256.into());
        query.push(last_ts.into());
    }

    fn _document_delete_params(
        query: &mut Vec<mysql_async::Value>,
        (ts, internal_id): &(Timestamp, InternalDocumentId),
    ) {
        let tablet_id: Vec<u8> = internal_id.table().0.into();
        let id: Vec<u8> = internal_id.internal_id().to_vec();
        let ts: u64 = (*ts).into();
        query.push(tablet_id.into());
        query.push(id.into());
        query.push(ts.into());
    }
}

/// Takes the key columns out of the `Row`
pub(crate) fn parse_row(row: &mut Row) -> anyhow::Result<IndexEntry> {
    let index_id = InternalId::try_from(bytes_col(row, 0)?)
        .context("index_id wrong size")?
        .into();

    let key_prefix: Vec<u8> = row.take_opt(1).context("row[1]")??;
    let key_sha256: Vec<u8> = row.take_opt(2).context("row[2]")??;
    let key_suffix: Option<Vec<u8>> = row.take_opt(3).context("row[3]")??;
    let ts: i64 = row.get_opt(4).context("row[4]")??;
    let ts = Timestamp::try_from(ts)?;
    let deleted: bool = row.get_opt(5).context("row[5]")??;
    Ok(IndexEntry {
        index_id,
        key_prefix,
        key_suffix,
        key_sha256,
        ts,
        deleted,
    })
}

#[async_trait]
impl<RT: Runtime> PersistenceReader for Reader<RT> {
    fn load_documents(
        &self,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentStream<'_> {
        self._load_documents(
            None,  /* tablet_id */
            false, /* include_prev_rev */
            range,
            order,
            page_size,
            retention_validator,
        )
        .map_ok(RevisionPair::into_log_entry)
        .cooperative()
        .boxed()
    }

    fn load_documents_from_table(
        &self,
        tablet_id: TabletId,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentStream<'_> {
        self._load_documents(
            Some(tablet_id),
            false, /* include_prev_rev */
            range,
            order,
            page_size,
            retention_validator,
        )
        .map_ok(RevisionPair::into_log_entry)
        .cooperative()
        .boxed()
    }

    fn load_revision_pairs(
        &self,
        tablet_id: Option<TabletId>,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentRevisionStream<'_> {
        self._load_documents(
            tablet_id,
            true, /* include_prev_rev */
            range,
            order,
            page_size,
            retention_validator,
        )
        .cooperative()
        .boxed()
    }

    async fn previous_revisions_of_documents(
        &self,
        ids: BTreeSet<DocumentPrevTsQuery>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<DocumentPrevTsQuery, DocumentLogEntry>> {
        let timer = metrics::previous_revisions_of_documents_timer(self.read_pool.cluster_name());

        let ids: Vec<_> = ids.into_iter().collect();

        let mut result = BTreeMap::new();

        let multitenant = self.multitenant;
        let instance_name: mysql_async::Value = (&self.instance_name.raw).into();

        // Track remaining items to process and fallback chunk size
        let mut remaining: &[DocumentPrevTsQuery] = &ids;
        let mut fallback_chunk_size: Option<usize> = None;

        while !remaining.is_empty() {
            // Avoid holding connections across yield points, to limit lifetime
            // and improve fairness.
            let mut client = self
                .read_pool
                .acquire("previous_revisions_of_documents", &self.db_name)
                .await?;

            // Determine chunk - either use smart_chunks or fallback size
            let chunk = if let Some(max_size) = fallback_chunk_size {
                let len = remaining.len().min(max_size);
                &remaining[..len]
            } else {
                // Use first chunk from smart_chunks
                smart_chunks(remaining).next().unwrap()
            };

            let mut params = Vec::with_capacity(
                chunk.len() * (super::documents::EXACT_REV_CHUNK_PARAMS + multitenant as usize),
            );
            let mut id_ts_to_query: HashMap<
                (InternalDocumentId, Timestamp),
                SmallVec<[DocumentPrevTsQuery; 1]>,
            > = HashMap::with_capacity(chunk.len());
            for q @ &DocumentPrevTsQuery { id, ts: _, prev_ts } in chunk {
                params.push(internal_id_param(id.table().0).into());
                params.push(internal_doc_id_param(id).into());
                params.push(i64::from(prev_ts).into());
                if multitenant {
                    params.push(instance_name.clone());
                }
                // the underlying query does not care about `ts` and will
                // deduplicate, so create a map from DB results back to queries
                id_ts_to_query.entry((id, prev_ts)).or_default().push(*q);
            }
            let results = match client
                .query_collect(
                    super::documents::exact_rev_chunk(chunk.len(), multitenant),
                    params,
                    chunk.len(),
                    |row| Self::row_to_document(&row),
                )
                .await
            {
                Ok(r) => Ok(r),
                Err(ref e) if let Some(db_err) = is_message_too_large_error(e) => {
                    let current_size = fallback_chunk_size.unwrap_or(chunk.len());
                    if current_size == 1 {
                        anyhow::bail!(
                            "Failed to load previous revisions of documents with minimum chunk \
                             size `1`: {}",
                            db_err.message,
                        );
                    }
                    if current_size <= *MYSQL_FALLBACK_PAGE_SIZE as usize {
                        tracing::warn!(
                            "Falling back to chunk size `1` due to repeated server error: {}",
                            db_err.message
                        );
                        fallback_chunk_size = Some(1);
                    } else {
                        tracing::warn!(
                            "Falling back to chunk size `{}` due to server error: {}",
                            *MYSQL_FALLBACK_PAGE_SIZE,
                            db_err.message
                        );
                        fallback_chunk_size = Some(*MYSQL_FALLBACK_PAGE_SIZE as usize);
                    }
                    continue;
                },
                Err(e) => Err(e),
            }?;
            for (prev_ts, id, maybe_doc, prev_prev_ts) in results {
                let entry = DocumentLogEntry {
                    ts: prev_ts,
                    id,
                    value: maybe_doc,
                    prev_ts: prev_prev_ts,
                };
                let original_queries = id_ts_to_query
                    .get(&(id, prev_ts))
                    .context("exact_rev_chunk query returned an unasked row")?;
                for (entry, &q) in
                    iter::repeat_n(entry, original_queries.len()).zip(original_queries)
                {
                    anyhow::ensure!(result.insert(q, entry).is_none());
                }
            }

            // Advance past the processed chunk
            remaining = &remaining[chunk.len()..];
        }

        if let Some(min_ts) = ids.iter().map(|DocumentPrevTsQuery { ts, .. }| *ts).min() {
            // Validate retention after finding documents
            retention_validator
                .validate_document_snapshot(min_ts)
                .await?;
        }

        timer.finish();
        Ok(result)
    }

    async fn previous_revisions(
        &self,
        ids: BTreeSet<(InternalDocumentId, Timestamp)>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<(InternalDocumentId, Timestamp), DocumentLogEntry>> {
        let timer = metrics::prev_revisions_timer(self.read_pool.cluster_name());

        let ids: Vec<_> = ids.into_iter().collect();

        let mut result = BTreeMap::new();

        let mut results = vec![];

        let mut min_ts = Timestamp::MAX;
        let multitenant = self.multitenant;
        let instance_name: mysql_async::Value = (&self.instance_name.raw).into();

        // Track remaining items to process and fallback chunk size
        let mut remaining: &[(InternalDocumentId, Timestamp)] = &ids;
        let mut fallback_chunk_size: Option<usize> = None;

        while !remaining.is_empty() {
            // Avoid holding connections across yield points, to limit lifetime
            // and improve fairness.
            let mut client = self
                .read_pool
                .acquire("previous_revisions", &self.db_name)
                .await?;

            // Determine chunk - either use smart_chunks or fallback size
            let chunk = if let Some(max_size) = fallback_chunk_size {
                let len = remaining.len().min(max_size);
                &remaining[..len]
            } else {
                // Use first chunk from smart_chunks
                smart_chunks(remaining).next().unwrap()
            };

            let mut params = Vec::with_capacity(
                chunk.len() * (super::documents::PREV_REV_CHUNK_PARAMS + multitenant as usize),
            );
            for (id, ts) in chunk {
                params.push(i64::from(*ts).into());
                params.push(internal_id_param(id.table().0).into());
                params.push(internal_doc_id_param(*id).into());
                params.push(i64::from(*ts).into());
                if multitenant {
                    params.push(instance_name.clone());
                }
                min_ts = cmp::min(*ts, min_ts);
            }
            let rows = match client
                .query_collect(
                    super::documents::prev_rev_chunk(chunk.len(), multitenant),
                    params,
                    chunk.len(),
                    Ok,
                )
                .await
            {
                Ok(r) => Ok(r),
                Err(ref e) if let Some(db_err) = is_message_too_large_error(e) => {
                    let current_size = fallback_chunk_size.unwrap_or(chunk.len());
                    if current_size == 1 {
                        anyhow::bail!(
                            "Failed to load previous revisions with minimum chunk size `1`: {}",
                            db_err.message,
                        );
                    }
                    if current_size <= *MYSQL_FALLBACK_PAGE_SIZE as usize {
                        tracing::warn!(
                            "Falling back to chunk size `1` due to repeated server error: {}",
                            db_err.message
                        );
                        fallback_chunk_size = Some(1);
                    } else {
                        tracing::warn!(
                            "Falling back to chunk size `{}` due to server error: {}",
                            *MYSQL_FALLBACK_PAGE_SIZE,
                            db_err.message
                        );
                        fallback_chunk_size = Some(*MYSQL_FALLBACK_PAGE_SIZE as usize);
                    }
                    continue;
                },
                Err(e) => Err(e),
            }?;

            results.extend(rows);

            // Advance past the processed chunk
            remaining = &remaining[chunk.len()..];
        }
        for row in results.into_iter() {
            let ts: i64 = row.get_opt(6).context("row[6]")??;
            let ts = Timestamp::try_from(ts)?;
            let (prev_ts, id, maybe_doc, prev_prev_ts) = Self::row_to_document(&row)?;
            anyhow::ensure!(result
                .insert(
                    (id, ts),
                    DocumentLogEntry {
                        ts: prev_ts,
                        id,
                        value: maybe_doc,
                        prev_ts: prev_prev_ts,
                    }
                )
                .is_none());
            log_prev_revisions_row_read(self.read_pool.cluster_name());
        }

        retention_validator
            .validate_document_snapshot(min_ts)
            .await?;
        timer.finish();
        Ok(result)
    }

    fn index_scan(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        read_timestamp: Timestamp,
        range: &Interval,
        order: Order,
        size_hint: usize,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> IndexStream<'_> {
        self._index_scan(
            index.id(),
            tablet_id,
            read_timestamp,
            range.clone(),
            order,
            size_hint,
            retention_validator,
        )
        .boxed()
    }

    #[fastrace::trace(properties = {"key": "{key:?}"})]
    async fn get_persistence_global(
        &self,
        key: PersistenceGlobalKey,
    ) -> anyhow::Result<Option<JsonValue>> {
        let mut client = self
            .read_pool
            .acquire("get_persistence_global", &self.db_name)
            .await?;
        let mut params = vec![String::from(key).into()];
        if self.multitenant {
            params.push(self.instance_name.to_string().into());
        }
        let rows = client
            .query_collect(
                super::get_persistence_global(self.multitenant),
                params,
                1,
                Ok,
            )
            .await?;
        let row = rows.into_iter().next();
        let value = row.map(|r| -> anyhow::Result<JsonValue> {
            let binary_value = bytes_col(&r, 0)?;
            let mut json_deserializer = serde_json::Deserializer::from_slice(binary_value);
            // XXX: this is bad, but shapes can get much more nested than convex values
            json_deserializer.disable_recursion_limit();
            let json_value = JsonValue::deserialize(&mut json_deserializer)
                .with_context(|| format!("Invalid JSON at persistence key {key:?}"))?;
            json_deserializer.end()?;
            Ok(json_value)
        });
        value.transpose()
    }

    fn version(&self) -> PersistenceVersion {
        PersistenceVersion::V5
    }

    async fn table_size_stats(&self) -> anyhow::Result<Vec<PersistenceTableSize>> {
        let mut client = self
            .read_pool
            .acquire("table_size_stats", &self.db_name)
            .await?;
        let stats = client
            .query_collect(
                super::TABLE_SIZE_QUERY,
                vec![self.db_name.clone().into()],
                5,
                |row| {
                    anyhow::Ok(PersistenceTableSize {
                        table_name: row.get_opt(0).context("row[0]")??,
                        data_bytes: row.get_opt(1).context("row[1]")??,
                        index_bytes: row.get_opt(2).context("row[2]")??,
                        row_count: row.get_opt(3).context("row[3]")??,
                    })
                },
            )
            .await?;
        Ok(stats)
    }
}

/// A `Lease` is unique for an instance across all of the processes in the
/// system. Its purpose is to make it safe to have multiple processes running
/// for the same instance at once, since we cannot truly guarantee that it will
/// not happen (e.g. processes unreachable by coordinator but still active, or
/// late-delivered packets from an already dead process) and we want to
/// purposefully run multiple so that one can coordinate all writes and the
/// others can serve stale reads, and smoothly swap between them during
/// deployment and node failure.
///
/// The only thing a `Lease` can do is execute a transaction against the
/// database and atomically ensure that the lease was still held during the
/// transaction, and otherwise return a lease lost.
pub(crate) struct Lease<RT: Runtime> {
    pool: Arc<ConvexMySqlPool<RT>>,
    db_name: String,
    instance_name: MySqlInstanceName,
    multitenant: bool,
    lease_ts: i64,
    lease_lost_shutdown: ShutdownSignal,
}

impl<RT: Runtime> Lease<RT> {
    #[allow(
        dead_code,
        reason = "Consumed by the opt-in shadow implementation in the follow-up PR"
    )]
    pub(crate) fn pool(&self) -> &Arc<ConvexMySqlPool<RT>> {
        &self.pool
    }

    #[allow(
        dead_code,
        reason = "Consumed by the opt-in shadow implementation in the follow-up PR"
    )]
    pub(crate) fn db_name(&self) -> &str {
        &self.db_name
    }

    #[allow(
        dead_code,
        reason = "Consumed by the opt-in shadow implementation in the follow-up PR"
    )]
    pub(crate) fn multitenant(&self) -> bool {
        self.multitenant
    }

    #[allow(
        dead_code,
        reason = "Consumed by the opt-in shadow implementation in the follow-up PR"
    )]
    pub(crate) fn timestamp(&self) -> i64 {
        self.lease_ts
    }

    #[allow(
        dead_code,
        reason = "Consumed by the opt-in shadow implementation in the follow-up PR"
    )]
    pub(crate) fn instance_name(&self) -> &str {
        &self.instance_name.raw
    }

    /// Acquire a lease. Makes other lease-holders get `LeaseLostError` when
    /// they commit.
    async fn acquire(
        pool: Arc<ConvexMySqlPool<RT>>,
        db_name: String,
        instance_name: MySqlInstanceName,
        multitenant: bool,
        allow_read_only: bool,
        lease_lost_shutdown: ShutdownSignal,
    ) -> anyhow::Result<Self> {
        let timer = metrics::lease_acquire_timer(pool.cluster_name());
        let mut client = pool.acquire("lease_acquire", &db_name).await?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("before 1970")
            .as_nanos() as i64;

        tracing::info!("attempting to acquire lease");
        let mut params = vec![ts.into(), ts.into()];
        if multitenant {
            params.push((&instance_name.raw).into());
            if !allow_read_only {
                params.push((&instance_name.raw).into());
            }
        }
        let rows_modified = client
            .exec_iter(super::lease_acquire(multitenant, allow_read_only), params)
            .await?;
        anyhow::ensure!(
            rows_modified == 1,
            "failed to acquire lease: already acquired with higher timestamp, or read-only"
        );
        tracing::info!("lease acquired with ts {}", ts);

        timer.finish();
        Ok(Self {
            db_name,
            pool,
            lease_ts: ts,
            lease_lost_shutdown,
            instance_name,
            multitenant,
        })
    }

    /// Execute the transaction function f atomically ensuring that the lease is
    /// still held, otherwise return lease lost.
    ///
    /// Once `transact` returns lease lost, no future transactions using
    /// it will succeed. Instead, a new `Lease` must be made with `acquire`,
    /// and any in-memory state then resynced because of any changes that
    /// might've been made to the database state while the lease was not
    /// held.
    #[fastrace::trace]
    pub(crate) async fn transact<F, T>(&self, f: F) -> anyhow::Result<T>
    where
        F: for<'a> AsyncFnOnce(&'a mut MySqlTransaction<'_>) -> anyhow::Result<T>,
    {
        self.transact_with_isolation(None, f).await
    }

    /// Like [`Self::transact`], but runs at READ COMMITTED instead of the
    /// InnoDB session default (REPEATABLE READ). Index retention deletes use
    /// this so their DELETEs take only record locks, sparing concurrent index
    /// writers the next-key/gap locks REPEATABLE READ adds.
    #[fastrace::trace]
    pub(crate) async fn transact_read_committed<F, T>(&self, f: F) -> anyhow::Result<T>
    where
        F: for<'a> AsyncFnOnce(&'a mut MySqlTransaction<'_>) -> anyhow::Result<T>,
    {
        self.transact_with_isolation(Some(IsolationLevel::ReadCommitted), f)
            .await
    }

    #[fastrace::trace]
    async fn transact_with_isolation<F, T>(
        &self,
        isolation: Option<IsolationLevel>,
        f: F,
    ) -> anyhow::Result<T>
    where
        F: for<'a> AsyncFnOnce(&'a mut MySqlTransaction<'_>) -> anyhow::Result<T>,
    {
        let mut client = self.pool.acquire("transact", &self.db_name).await?;
        let r = try_anyhow!({
            let mut tx = client
                .transaction(self.pool.cluster_name(), isolation)
                .await?;

            let timer = metrics::lease_precond_timer(self.pool.cluster_name());
            let mut params = vec![mysql_async::Value::Int(self.lease_ts)];
            if self.multitenant {
                params.push((&self.instance_name.raw).into());
            }
            let rows: Option<Row> = tx
                .exec_first(super::lease_precond(self.multitenant), params)
                .in_span(Span::enter_with_local_parent(format!(
                    "{}::lease_precondition",
                    func_path!()
                )))
                .await?;
            if rows.is_none() {
                self.lease_lost_shutdown.signal(lease_lost_error());
                anyhow::bail!(lease_lost_error());
            }
            timer.finish();

            let result = f(&mut tx)
                .in_span(Span::enter_with_local_parent(format!(
                    "{}::execute_function",
                    func_path!()
                )))
                .await?;

            let timer = metrics::commit_timer(self.pool.cluster_name());
            tx.commit().await?;
            timer.finish();

            result
        });
        client.handle_errors(r).await
    }
}

fn document_params(
    mut query: Vec<mysql_async::Value>,
    ts: Timestamp,
    id: InternalDocumentId,
    maybe_doc: Option<&ResolvedDocument>,
    prev_ts: Option<Timestamp>,
) -> anyhow::Result<Vec<mysql_async::Value>> {
    let deleted = maybe_doc.is_none();
    let encoded_doc = document_encoding::encode(maybe_doc)?;
    anyhow::ensure!(
        document_encoding::decode(&encoded_doc, id.table())?.as_ref() == maybe_doc,
        "failed to roundtrip document encoding"
    );

    query.push(internal_doc_id_param(id).into());
    query.push(i64::from(ts).into());
    query.push(internal_id_param(id.table().0).into());
    query.push(mysql_async::Value::Bytes(encoded_doc));
    query.push(deleted.into());
    query.push(prev_ts.map(i64::from).into());
    Ok(query)
}

pub(crate) fn internal_id_param(id: InternalId) -> Vec<u8> {
    id.into()
}

pub(crate) fn internal_doc_id_param(id: InternalDocumentId) -> Vec<u8> {
    internal_id_param(id.internal_id())
}
