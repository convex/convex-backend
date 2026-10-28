use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    sync::{
        atomic::{
            AtomicBool,
            AtomicI64,
            Ordering,
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
    errors::{
        database_operational_error,
        lease_lost_error,
    },
    interval::Interval,
    knobs::MYSQL_FALLBACK_PAGE_SIZE,
    persistence::{
        ConflictStrategy,
        DocumentLogEntry,
        DocumentPrevTsQuery,
        DocumentRevisionStream,
        DocumentStream,
        IndexBackfillEntry,
        IndexRetentionProgress,
        IndexRetentionRequest,
        IndexStream,
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
    shutdown::ShutdownSignal,
    types::{
        DeploymentId as BigBrainDeploymentId,
        IndexRef,
        PersistenceIndexId,
        PersistenceVersion,
        Timestamp,
    },
    value::{
        InternalDocumentId,
        InternalId,
        TabletId,
    },
};
use fastrace::prelude::*;
use futures::{
    StreamExt,
    TryStreamExt,
};
use futures_async_stream::try_stream;
use mysql_async::{
    IsolationLevel,
    Row,
    Value,
};
use serde_json::Value as JsonValue;

use super::{
    column,
    documents,
    indexes::{
        self,
        IndexEngine,
    },
    sql::{
        self,
        LogBucketBounds,
    },
    PersistenceDeploymentId,
};
use crate::{
    chunks::{
        fill_chunks,
        smart_chunks,
        ApproxSize,
    },
    connection::{
        is_message_too_large_error,
        MySqlConnection,
        MySqlTransaction,
    },
    document_encoding,
    metrics,
    ConvexMySqlPool,
    MySqlOptions,
    MySqlReaderOptions,
};

pub(crate) struct Persistence<RT: Runtime> {
    inner: Arc<Inner<RT>>,
    lease: Arc<Lease<RT>>,
}

/// The read side of a V6 persistence: the same tenant scope, no lease.
pub(crate) struct Reader<RT: Runtime> {
    inner: Arc<Inner<RT>>,
}

struct Inner<RT: Runtime> {
    pool: Arc<ConvexMySqlPool<RT>>,
    db_name: String,
    deployment_id: PersistenceDeploymentId,
    fresh: AtomicBool,
    published: PublishedBucketBounds,
    engine: IndexEngine,
}

/// Readers and writers share the maximum observed maintenance bounds.
/// A torn read is safe: scans retry a stale ceiling, and a stale floor includes
/// extra buckets (refreshing the bounds if maintenance has dropped one).
#[derive(Default)]
pub(crate) struct PublishedBucketBounds {
    created_through_ts: AtomicI64,
    oldest_kept_ts: AtomicI64,
}

impl PublishedBucketBounds {
    pub(super) fn observe(&self, created_through_ts: i64, oldest_kept_ts: i64) {
        self.created_through_ts
            .fetch_max(created_through_ts, Ordering::Relaxed);
        self.oldest_kept_ts
            .fetch_max(oldest_kept_ts, Ordering::Relaxed);
    }

    pub(super) fn load(&self) -> anyhow::Result<LogBucketBounds> {
        LogBucketBounds::from_state(
            self.created_through_ts.load(Ordering::Relaxed),
            self.oldest_kept_ts.load(Ordering::Relaxed),
        )
    }

    pub(super) async fn refresh<RT: Runtime>(
        &self,
        connection: &mut MySqlConnection<'_, RT>,
    ) -> anyhow::Result<()> {
        let row: Row = connection
            .query_optional(sql::READ_LOG_BUCKET_BOUNDS, vec![])
            .await?
            .context("MySQL V6 log bucket maintenance state is missing")?;
        self.observe(
            row.get_opt(0).context("created_through_ts")??,
            row.get_opt(1).context("oldest_kept_ts")??,
        );
        Ok(())
    }
}

impl<RT: Runtime> Persistence<RT> {
    fn backfill(&self) -> super::backfill::Backfill<'_, RT, Lease<RT>> {
        super::backfill::Backfill {
            pool: &self.inner.pool,
            db_name: &self.inner.db_name,
            deployment_id: self.inner.deployment_id,
            engine: &self.inner.engine,
            lease: &self.lease,
        }
    }

    pub(crate) async fn new(
        pool: Arc<ConvexMySqlPool<RT>>,
        db_name: String,
        options: MySqlOptions,
        lease_lost_shutdown: ShutdownSignal,
    ) -> anyhow::Result<Self> {
        let deployment_id = deployment_id_from_options(
            options.version,
            options.multitenant,
            options.deployment_id,
        )?;
        Self::new_inner(
            pool,
            db_name,
            deployment_id,
            options.allow_read_only,
            lease_lost_shutdown,
        )
        .await
    }

    async fn new_inner(
        pool: Arc<ConvexMySqlPool<RT>>,
        db_name: String,
        deployment_id: PersistenceDeploymentId,
        allow_read_only: bool,
        lease_lost_shutdown: ShutdownSignal,
    ) -> anyhow::Result<Self> {
        Self::initialize_schema(&pool, &db_name).await?;
        let mut connection = pool.acquire("v6_init_deployment", &db_name).await?;
        connection
            .exec_iter(super::INIT_LEASE, vec![deployment_id.into()])
            .await?;
        drop(connection);
        let lease = Arc::new(
            Lease::acquire(
                pool.clone(),
                db_name.clone(),
                deployment_id,
                allow_read_only,
                lease_lost_shutdown,
            )
            .await?,
        );
        // Check after acquiring the lease
        let mut connection = pool.acquire("v6_check_newly_created", &db_name).await?;
        let fresh = connection
            .query_optional(documents::check_newly_created(), vec![deployment_id.into()])
            .await?
            .is_none();
        drop(connection);
        Ok(Self {
            inner: Arc::new(Inner {
                pool,
                db_name,
                deployment_id,
                fresh: AtomicBool::new(fresh),
                published: PublishedBucketBounds::default(),
                engine: IndexEngine::new(deployment_id),
            }),
            lease,
        })
    }

    /// `indexes_latest` is a V6-only sentinel created before tables whose names
    /// overlap V5. Once present, retrying every idempotent DDL statement is
    /// safe; `documents` without the sentinel belongs to an incompatible
    /// persistence layout.
    async fn initialize_schema(pool: &ConvexMySqlPool<RT>, db_name: &str) -> anyhow::Result<()> {
        let mut connection = pool.acquire("v6_init", db_name).await?;
        let has_sentinel = Self::table_exists(&mut connection, db_name, "indexes_latest").await?;
        if !has_sentinel && Self::table_exists(&mut connection, db_name, "documents").await? {
            // Another initializer may have created the sentinel between the
            // two table checks.
            anyhow::ensure!(
                Self::table_exists(&mut connection, db_name, "indexes_latest").await?,
                "refusing to initialize MySQL V6 over a V5 or unversioned persistence database"
            );
        }
        connection.execute_many(super::init_sql()).await?;
        Self::validate_shared_tables(&mut connection, db_name).await
    }

    async fn table_exists(
        connection: &mut MySqlConnection<'_, RT>,
        db_name: &str,
        table_name: &str,
    ) -> anyhow::Result<bool> {
        Ok(connection
            .query_optional(
                super::FIND_TABLE,
                vec![
                    Value::Bytes(db_name.as_bytes().to_vec()),
                    Value::Bytes(table_name.as_bytes().to_vec()),
                ],
            )
            .await?
            .is_some())
    }

    async fn validate_shared_tables(
        connection: &mut MySqlConnection<'_, RT>,
        db_name: &str,
    ) -> anyhow::Result<()> {
        let actual: BTreeSet<String> = connection
            .query_collect(
                super::READ_V6_SCOPED_TABLES,
                vec![Value::Bytes(db_name.as_bytes().to_vec())],
                4,
                |row| row.get::<String, _>(0).context("shared table name"),
            )
            .await?
            .into_iter()
            .collect();
        let expected = BTreeSet::from([
            "documents".to_owned(),
            "indexes_latest".to_owned(),
            "indexes_backfill_deletes".to_owned(),
            "leases".to_owned(),
            "persistence_globals".to_owned(),
            "read_only".to_owned(),
        ]);
        let incompatible: Vec<_> = expected.difference(&actual).cloned().collect();
        anyhow::ensure!(
            incompatible.is_empty(),
            "MySQL V6 shared tables have incompatible schemas: {incompatible:?} lack deployment_id"
        );
        Ok(())
    }

    pub(crate) fn new_reader(
        pool: Arc<ConvexMySqlPool<RT>>,
        db_name: String,
        options: MySqlReaderOptions,
    ) -> anyhow::Result<Reader<RT>> {
        let deployment_id = deployment_id_from_options(
            options.version,
            options.multitenant,
            options.deployment_id,
        )?;
        Ok(Reader {
            inner: Arc::new(Inner {
                pool,
                db_name,
                deployment_id,
                fresh: AtomicBool::new(false),
                published: PublishedBucketBounds::default(),
                engine: IndexEngine::new(deployment_id),
            }),
        })
    }

    pub(crate) async fn set_read_only(
        pool: Arc<ConvexMySqlPool<RT>>,
        db_name: String,
        options: MySqlOptions,
        read_only: bool,
    ) -> anyhow::Result<()> {
        let deployment_id = deployment_id_from_options(
            options.version,
            options.multitenant,
            options.deployment_id,
        )?;
        let mut connection = pool.acquire("v6_set_read_only", &db_name).await?;
        connection
            .exec_iter(
                if read_only {
                    super::SET_READ_ONLY
                } else {
                    super::UNSET_READ_ONLY
                },
                vec![deployment_id.into()],
            )
            .await?;
        Ok(())
    }

    /// Whether `indexes_latest` holds a row for this deployment, as opposed to
    /// history that only survives in the log buckets.
    pub(crate) async fn has_latest_row(&self) -> anyhow::Result<bool> {
        let mut connection = self
            .inner
            .pool
            .acquire("v6_has_index_entries", &self.inner.db_name)
            .await?;
        Ok(connection
            .query_optional(sql::HAS_LATEST_ROW, vec![self.inner.deployment_id.into()])
            .await?
            .is_some())
    }

    /// Publication precedes every displacement into a bucket. The ceiling only
    /// advances, so a cached ceiling remains safe across maintenance rounds.
    async fn ensure_published_bucket(&self, ts: Timestamp) -> anyhow::Result<()> {
        let ts = i64::from(ts);
        let ceiling = || {
            self.inner
                .published
                .created_through_ts
                .load(Ordering::Relaxed)
        };
        if ts < ceiling() {
            return Ok(());
        }
        let mut connection = self
            .inner
            .pool
            .acquire("v6_write_ceiling", &self.inner.db_name)
            .await?;
        self.inner.published.refresh(&mut connection).await?;
        if ts >= ceiling() {
            return Err(database_operational_error(anyhow::anyhow!(
                "MySQL V6 index displacement at {ts} awaits log bucket publication"
            )));
        }
        Ok(())
    }

    fn document_params(&self, update: &DocumentLogEntry) -> anyhow::Result<Vec<Value>> {
        if let Some(document) = &update.value {
            anyhow::ensure!(update.id == document.id_with_table_id());
        }
        let encoded = document_encoding::encode(update.value.as_ref())?;
        anyhow::ensure!(
            document_encoding::decode(&encoded, update.id.table())?.as_ref()
                == update.value.as_ref(),
            "failed to roundtrip document encoding"
        );
        Ok(vec![
            self.inner.deployment_id.into(),
            Value::Bytes(update.id.internal_id().into()),
            Value::Int(i64::from(update.ts)),
            Value::Bytes(update.id.table().0.into()),
            Value::Bytes(encoded),
            Value::from(update.value.is_none()),
            update.prev_ts.map(i64::from).into(),
        ])
    }
}

impl<RT: Runtime> Reader<RT> {
    #[try_stream(ok = RevisionPair, error = anyhow::Error)]
    async fn load_revision_pairs_impl(
        &self,
        tablet_id: Option<TabletId>,
        include_previous_revision: bool,
        range: TimestampRange,
        order: Order,
        mut page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) {
        anyhow::ensure!(page_size > 0);
        let timer = metrics::load_documents_timer(self.inner.pool.cluster_name());
        let mut num_returned = 0;
        let mut last_ts = match order {
            Order::Asc => Timestamp::MIN,
            Order::Desc => Timestamp::MAX,
        };
        let mut last_table_id = match order {
            Order::Asc => InternalId::BEFORE_ALL_BYTES.to_vec(),
            Order::Desc => InternalId::AFTER_ALL_BYTES.to_vec(),
        };
        let mut last_id = last_table_id.clone();
        loop {
            let mut connection = self
                .inner
                .pool
                .acquire("v6_load_documents", &self.inner.db_name)
                .await?;
            let query = documents::load_page(order, tablet_id.is_some(), include_previous_revision);
            let mut params = vec![
                self.inner.deployment_id.into(),
                Value::Int(i64::from(range.min_timestamp_inclusive())),
                Value::Int(i64::from(range.max_timestamp_exclusive())),
                Value::Int(i64::from(last_ts)),
                Value::Int(i64::from(last_ts)),
                Value::Bytes(last_table_id.clone()),
                Value::Bytes(last_table_id.clone()),
                Value::Bytes(last_id.clone()),
            ];
            if let Some(tablet_id) = tablet_id {
                params.push(Value::Bytes(tablet_id.0.into()));
            }
            params.push(Value::Int(i64::from(page_size)));
            let rows = match connection
                .query_collect(&query, params, page_size as usize, Ok)
                .await
            {
                Ok(rows) => rows,
                Err(ref error) if is_message_too_large_error(error).is_some() => {
                    if page_size == 1 {
                        anyhow::bail!(
                            "Failed to load a V6 document within the Vitess message limit"
                        );
                    }
                    page_size = if page_size <= *MYSQL_FALLBACK_PAGE_SIZE {
                        1
                    } else {
                        *MYSQL_FALLBACK_PAGE_SIZE
                    };
                    continue;
                },
                Err(error) => return Err(error),
            };
            drop(connection);
            retention_validator
                .validate_document_snapshot(range.min_timestamp_inclusive())
                .await?;
            let rows_loaded = rows.len();
            for row in rows {
                let entry = row_to_document(&row)?;
                last_ts = entry.ts;
                last_table_id = entry.id.table().0.into();
                last_id = entry.id.internal_id().into();
                let previous_document = if include_previous_revision {
                    column::maybe_bytes(&row, 6)?
                        .map(|bytes| {
                            document_encoding::decode(bytes, entry.id.table())?
                                .context("previous revision is deleted")
                        })
                        .transpose()?
                } else {
                    None
                };
                num_returned += 1;
                yield RevisionPair {
                    id: entry.id,
                    rev: DocumentRevision {
                        ts: entry.ts,
                        document: entry.value,
                    },
                    prev_rev: entry.prev_ts.map(|ts| DocumentRevision {
                        ts,
                        document: previous_document,
                    }),
                };
            }
            if rows_loaded < page_size as usize {
                break;
            }
        }
        metrics::finish_load_documents_timer(timer, num_returned, self.inner.pool.cluster_name());
    }
}

#[async_trait]
impl<RT: Runtime> common::persistence::Persistence for Persistence<RT> {
    fn is_fresh(&self) -> bool {
        self.inner.fresh.load(Ordering::SeqCst)
    }

    fn reader(&self) -> Arc<dyn PersistenceReader> {
        Arc::new(Reader {
            inner: self.inner.clone(),
        })
    }

    #[fastrace::trace]
    async fn write<'a>(
        &self,
        document_updates: &'a [DocumentLogEntry],
        index_updates: &'a [PersistenceIndexEntry],
        conflict_strategy: ConflictStrategy,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(document_updates.len() <= crate::MAX_INSERT_SIZE);
        metrics::log_write_documents(document_updates.len());
        let write_size: usize = document_updates
            .iter()
            .map(|update| update.value.as_ref().map_or(0, |document| document.size()))
            .sum();
        metrics::log_write_bytes(write_size);
        metrics::log_index_write_bytes(index_updates.iter().map(ApproxSize::approx_size).sum());
        LocalSpan::add_properties(|| {
            [
                ("num_documents", document_updates.len().to_string()),
                ("write_size", write_size.to_string()),
            ]
        });
        let batch = self
            .inner
            .engine
            .plan_index_writes(index_updates, conflict_strategy)?;
        batch.validate_documents(document_updates)?;
        if let Some(ts) = batch.replacement_commits().map(|(ts, _)| ts).max() {
            self.ensure_published_bucket(ts).await?;
        }
        let cluster_name = self.inner.pool.cluster_name();
        self.lease
            .transact(async |tx| {
                for chunk in fill_chunks(document_updates) {
                    let query = match conflict_strategy {
                        ConflictStrategy::Error => documents::insert_chunk(chunk.len()),
                        ConflictStrategy::Overwrite => {
                            documents::insert_overwrite_chunk(chunk.len())
                        },
                    };
                    let mut params = Vec::new();
                    for update in chunk {
                        params.extend(self.document_params(update)?);
                    }
                    let chunk_bytes: usize = chunk.iter().map(ApproxSize::approx_size).sum();
                    async {
                        let timer = metrics::insert_document_chunk_timer(cluster_name);
                        tx.query_drop(&query, params).await?;
                        timer.finish();
                        anyhow::Ok(())
                    }
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
                self.inner
                    .engine
                    .write_index_batch(tx, &batch, cluster_name)
                    .await?;
                Ok(())
            })
            .await?;
        if !document_updates.is_empty() || !index_updates.is_empty() {
            self.inner.fresh.store(false, Ordering::SeqCst);
        }
        Ok(())
    }

    #[fastrace::trace]
    async fn write_index_backfill(&self, entries: &[IndexBackfillEntry]) -> anyhow::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        metrics::log_index_write_bytes(entries.iter().map(ApproxSize::approx_size).sum());
        let rows = self.inner.engine.plan_backfill_rows(entries)?;
        let cluster_name = self.inner.pool.cluster_name();
        for chunk in fill_chunks(&rows) {
            self.lease
                .transact(async |tx| {
                    self.inner
                        .engine
                        .write_backfill_chunk(tx, chunk, cluster_name)
                        .await
                })
                .await?;
        }
        LocalSpan::add_properties(|| [("num_entries", entries.len().to_string())]);
        self.inner.fresh.store(false, Ordering::SeqCst);
        Ok(())
    }

    #[fastrace::trace]
    async fn reconcile_index_backfill(&self, index: PersistenceIndexId) -> anyhow::Result<()> {
        self.backfill().reconcile(index).await
    }

    async fn index_backfill_marker_indexes(&self) -> anyhow::Result<Vec<PersistenceIndexId>> {
        self.backfill().marker_indexes().await
    }

    async fn delete_index_backfill_markers_chunk(
        &self,
        index: PersistenceIndexId,
    ) -> anyhow::Result<u64> {
        self.backfill().delete_chunk(index).await
    }

    async fn write_persistence_global(
        &self,
        key: PersistenceGlobalKey,
        value: JsonValue,
    ) -> anyhow::Result<()> {
        let timer = metrics::write_persistence_global_timer(self.inner.pool.cluster_name(), key);
        let params = vec![
            self.inner.deployment_id.into(),
            Value::Bytes(String::from(key).into_bytes()),
            Value::Bytes(serde_json::to_vec(&value)?),
        ];
        self.lease
            .transact(async |tx| {
                tx.exec_drop(super::WRITE_PERSISTENCE_GLOBAL, params)
                    .await?;
                Ok(())
            })
            .await?;
        timer.finish();
        Ok(())
    }

    async fn has_index_entries(&self) -> anyhow::Result<bool> {
        if self.has_latest_row().await? {
            return Ok(true);
        }
        let mut connection = self
            .inner
            .pool
            .acquire("v6_has_index_history", &self.inner.db_name)
            .await?;
        for bucket in indexes::list_log_buckets(&mut connection, &self.inner.db_name).await? {
            if connection
                .query_optional(
                    &sql::has_log_row(bucket),
                    vec![self.inner.deployment_id.into()],
                )
                .await?
                .is_some()
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn reclaim_index_history(
        &self,
        request: IndexRetentionRequest<'_>,
    ) -> anyhow::Result<IndexRetentionProgress> {
        // Index history lives in log tables dropped whole below the retention
        // floor, so a pass reads, expires and deletes nothing; it only says how
        // far the cursor got. Nothing precedes the minimum timestamp.
        let cursor = if *request.min_snapshot_ts == Timestamp::MIN {
            request.cursor
        } else {
            request.min_snapshot_ts.pred()?
        };
        Ok(IndexRetentionProgress {
            cursor,
            expired_entries: 0,
            deleted_rows: 0,
            unique_indexes: 0,
        })
    }

    async fn delete(
        &self,
        mut document_ids: Vec<(Timestamp, InternalDocumentId)>,
    ) -> anyhow::Result<usize> {
        if document_ids.is_empty() {
            return Ok(0);
        }
        document_ids.sort_unstable_by_key(|d| d.1);
        // We implicitly delete all timestamps less than `d.0`, so just keep the
        // highest timestamp for each document id.
        document_ids.dedup_by(|a, b| {
            if a.1 == b.1 {
                // N.B.: returning `true` to dedup_by deletes `a`, so update `b`.
                b.0 = b.0.max(a.0);
                true
            } else {
                false
            }
        });
        self.lease
            .transact(async |tx| {
                let mut deleted = 0;
                for chunk in fill_chunks(&document_ids) {
                    let mut params = vec![self.inner.deployment_id.into()];
                    for (ts, id) in chunk {
                        params.extend([
                            Value::Bytes(id.table().0.into()),
                            Value::Bytes(id.internal_id().into()),
                            Value::Int(i64::from(*ts)),
                        ]);
                    }
                    deleted += tx
                        .query_iter(&documents::delete_chunk(chunk.len()), params)
                        .await? as usize;
                }
                Ok(deleted)
            })
            .await
    }

    async fn delete_tablet_documents(
        &self,
        tablet_id: TabletId,
        chunk_size: usize,
    ) -> anyhow::Result<usize> {
        self.lease
            .transact(async |tx| {
                Ok(tx
                    .exec_iter(
                        documents::delete_tablet_chunk(),
                        vec![
                            self.inner.deployment_id.into(),
                            Value::Bytes(tablet_id.0.into()),
                            Value::UInt(chunk_size as u64),
                        ],
                    )
                    .await? as usize)
            })
            .await
    }
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
        self.load_revision_pairs_impl(None, false, range, order, page_size, retention_validator)
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
        self.load_revision_pairs_impl(
            Some(tablet_id),
            false,
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
        self.load_revision_pairs_impl(
            tablet_id,
            true,
            range,
            order,
            page_size,
            retention_validator,
        )
        .cooperative()
        .boxed()
    }

    async fn previous_revisions(
        &self,
        ids: BTreeSet<(InternalDocumentId, Timestamp)>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<(InternalDocumentId, Timestamp), DocumentLogEntry>> {
        let timer = metrics::prev_revisions_timer(self.inner.pool.cluster_name());
        let min_queried_ts = ids.iter().map(|(_, ts)| *ts).min();
        let ids: Vec<_> = ids.into_iter().collect();
        let mut result = BTreeMap::new();
        let mut remaining: &[(InternalDocumentId, Timestamp)] = &ids;
        let mut fallback_chunk_size = None;
        while !remaining.is_empty() {
            // Acquired per chunk so a long lookup does not pin a connection
            // across yield points.
            let mut connection = self
                .inner
                .pool
                .acquire("v6_previous_revision", &self.inner.db_name)
                .await?;
            let chunk = next_chunk(remaining, fallback_chunk_size);
            let mut params = Vec::with_capacity(chunk.len() * 5);
            for (id, query_ts) in chunk {
                params.extend([
                    Value::Int(i64::from(*query_ts)),
                    self.inner.deployment_id.into(),
                    Value::Bytes(id.table().0.into()),
                    Value::Bytes(id.internal_id().into()),
                    Value::Int(i64::from(*query_ts)),
                ]);
            }
            let rows = match connection
                .query_collect(
                    &documents::previous_revision_chunk(chunk.len()),
                    params,
                    chunk.len(),
                    |row| {
                        let query_ts: i64 = row.get_opt(6).context("row[6]")??;
                        anyhow::Ok((Timestamp::try_from(query_ts)?, row_to_document(&row)?))
                    },
                )
                .await
            {
                Ok(rows) => rows,
                Err(ref error) if is_message_too_large_error(error).is_some() => {
                    fallback_chunk_size =
                        Some(smaller_chunk(fallback_chunk_size.unwrap_or(chunk.len()))?);
                    continue;
                },
                Err(error) => return Err(error),
            };
            for (query_ts, entry) in rows {
                metrics::log_prev_revisions_row_read(self.inner.pool.cluster_name());
                result.insert((entry.id, query_ts), entry);
            }
            remaining = &remaining[chunk.len()..];
        }
        // Validate the snapshots read at, not the revisions found: a previous
        // revision may legitimately predate the retention floor.
        if let Some(min_queried_ts) = min_queried_ts {
            retention_validator
                .validate_document_snapshot(min_queried_ts)
                .await?;
        }
        timer.finish();
        Ok(result)
    }

    async fn previous_revisions_of_documents(
        &self,
        ids: BTreeSet<DocumentPrevTsQuery>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<DocumentPrevTsQuery, DocumentLogEntry>> {
        let timer = metrics::previous_revisions_of_documents_timer(self.inner.pool.cluster_name());
        let min_queried_ts = ids.iter().map(|query| query.ts).min();
        let ids: Vec<_> = ids.into_iter().collect();
        let mut result = BTreeMap::new();
        let mut remaining: &[DocumentPrevTsQuery] = &ids;
        let mut fallback_chunk_size = None;
        while !remaining.is_empty() {
            let mut connection = self
                .inner
                .pool
                .acquire("v6_exact_revision", &self.inner.db_name)
                .await?;
            let chunk = next_chunk(remaining, fallback_chunk_size);
            let mut params = Vec::with_capacity(1 + chunk.len() * 3);
            params.push(self.inner.deployment_id.into());
            // Several queries can ask for the same revision at different
            // snapshots; the statement returns each revision once.
            let mut queries_by_revision: BTreeMap<(InternalDocumentId, Timestamp), Vec<_>> =
                BTreeMap::new();
            for query in chunk {
                params.extend([
                    Value::Bytes(query.id.table().0.into()),
                    Value::Bytes(query.id.internal_id().into()),
                    Value::Int(i64::from(query.prev_ts)),
                ]);
                queries_by_revision
                    .entry((query.id, query.prev_ts))
                    .or_default()
                    .push(*query);
            }
            let rows = match connection
                .query_collect(
                    &documents::exact_revision_chunk(chunk.len()),
                    params,
                    chunk.len(),
                    |row| row_to_document(&row),
                )
                .await
            {
                Ok(rows) => rows,
                Err(ref error) if is_message_too_large_error(error).is_some() => {
                    fallback_chunk_size =
                        Some(smaller_chunk(fallback_chunk_size.unwrap_or(chunk.len()))?);
                    continue;
                },
                Err(error) => return Err(error),
            };
            for entry in rows {
                for query in queries_by_revision
                    .get(&(entry.id, entry.ts))
                    .into_iter()
                    .flatten()
                {
                    result.insert(*query, entry.clone());
                }
            }
            remaining = &remaining[chunk.len()..];
        }
        // As in `previous_revisions`, validate the snapshots read at.
        if let Some(min_queried_ts) = min_queried_ts {
            retention_validator
                .validate_document_snapshot(min_queried_ts)
                .await?;
        }
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
        super::reader::IndexReader {
            documents: super::reader::DocumentScope::DeploymentId,
            published: &self.inner.published,
            pool: &self.inner.pool,
            db_name: &self.inner.db_name,
            deployment_id: self.inner.deployment_id,
        }
        .scan(
            index,
            tablet_id,
            read_timestamp,
            range.clone(),
            order,
            size_hint,
            retention_validator,
        )
        .boxed()
    }

    async fn get_persistence_global(
        &self,
        key: PersistenceGlobalKey,
    ) -> anyhow::Result<Option<JsonValue>> {
        let mut connection = self
            .inner
            .pool
            .acquire("v6_read_global", &self.inner.db_name)
            .await?;
        let row = connection
            .query_optional(
                super::READ_PERSISTENCE_GLOBAL,
                vec![
                    self.inner.deployment_id.into(),
                    Value::Bytes(String::from(key).into_bytes()),
                ],
            )
            .await?;
        row.map(|row| super::decode_persistence_global(&row, key))
            .transpose()
    }

    /// Physical sizes of the shared tables and every log bucket. V6 tables
    /// hold every deployment of the database, so these are database-wide:
    /// summing them per deployment counts each byte once per tenant.
    async fn table_size_stats(&self) -> anyhow::Result<Vec<PersistenceTableSize>> {
        let mut connection = self
            .inner
            .pool
            .acquire("v6_table_size_stats", &self.inner.db_name)
            .await?;
        connection
            .query_collect(
                crate::v5::TABLE_SIZE_QUERY,
                vec![self.inner.db_name.clone().into()],
                8,
                |row| {
                    anyhow::Ok(PersistenceTableSize {
                        table_name: row.get_opt(0).context("row[0]")??,
                        data_bytes: row.get_opt(1).context("row[1]")??,
                        index_bytes: row.get_opt(2).context("row[2]")??,
                        row_count: row.get_opt(3).context("row[3]")??,
                    })
                },
            )
            .await
    }

    fn version(&self) -> PersistenceVersion {
        PersistenceVersion::V6
    }

    fn needs_index_retention_deletes(&self) -> bool {
        false
    }
}

pub(crate) struct Lease<RT: Runtime> {
    pool: Arc<ConvexMySqlPool<RT>>,
    db_name: String,
    deployment_id: PersistenceDeploymentId,
    lease_ts: i64,
    lease_lost_shutdown: ShutdownSignal,
}

impl<RT: Runtime> Lease<RT> {
    async fn acquire(
        pool: Arc<ConvexMySqlPool<RT>>,
        db_name: String,
        deployment_id: PersistenceDeploymentId,
        allow_read_only: bool,
        lease_lost_shutdown: ShutdownSignal,
    ) -> anyhow::Result<Self> {
        let timer = metrics::lease_acquire_timer(pool.cluster_name());
        let mut client = pool.acquire("v6_lease_acquire", &db_name).await?;
        let lease_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is before 1970")
            .as_nanos() as i64;
        let rows_modified = if allow_read_only {
            client
                .exec_iter(
                    super::LEASE_ACQUIRE_ALLOW_READ_ONLY,
                    vec![
                        Value::Int(lease_ts),
                        Value::Int(lease_ts),
                        deployment_id.into(),
                    ],
                )
                .await?
        } else {
            client
                .exec_iter(
                    super::LEASE_ACQUIRE,
                    vec![
                        Value::Int(lease_ts),
                        Value::Int(lease_ts),
                        deployment_id.into(),
                        deployment_id.into(),
                    ],
                )
                .await?
        };
        anyhow::ensure!(
            rows_modified == 1,
            "failed to acquire V6 lease: already acquired with higher timestamp, or read-only"
        );
        timer.finish();
        Ok(Self {
            pool,
            db_name,
            deployment_id,
            lease_ts,
            lease_lost_shutdown,
        })
    }

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
    pub(crate) async fn transact_read_committed<F, T>(&self, f: F) -> anyhow::Result<T>
    where
        F: for<'a> AsyncFnOnce(&'a mut MySqlTransaction<'_>) -> anyhow::Result<T>,
    {
        self.transact_with_isolation(Some(IsolationLevel::ReadCommitted), f)
            .await
    }

    async fn transact_with_isolation<F, T>(
        &self,
        isolation: Option<IsolationLevel>,
        f: F,
    ) -> anyhow::Result<T>
    where
        F: for<'a> AsyncFnOnce(&'a mut MySqlTransaction<'_>) -> anyhow::Result<T>,
    {
        let mut client = self.pool.acquire("v6_transact", &self.db_name).await?;
        let result = async {
            let mut tx = client
                .transaction(self.pool.cluster_name(), isolation)
                .await?;
            let timer = metrics::lease_precond_timer(self.pool.cluster_name());
            let lease: Option<Row> = tx
                .exec_first(
                    super::LEASE_PRECONDITION,
                    vec![Value::Int(self.lease_ts), self.deployment_id.into()],
                )
                .await?;
            if lease.is_none() {
                self.lease_lost_shutdown.signal(lease_lost_error());
                anyhow::bail!(lease_lost_error());
            }
            timer.finish();
            let value = f(&mut tx).await?;
            let timer = metrics::commit_timer(self.pool.cluster_name());
            tx.commit().await?;
            timer.finish();
            Ok(value)
        }
        .await;
        client.handle_errors(result).await
    }
}

/// The tenant a set of connection options addresses. V6 is the multitenant
/// driver's layout and keys every row by the control plane's deployment ID.
fn deployment_id_from_options(
    version: PersistenceVersion,
    multitenant: bool,
    deployment_id: BigBrainDeploymentId,
) -> anyhow::Result<PersistenceDeploymentId> {
    anyhow::ensure!(
        version == PersistenceVersion::V6 && multitenant,
        "MySQL V6 is only supported by the multitenant V6 driver"
    );
    deployment_id.try_into()
}

/// The chunk size to retry with after a chunk exceeded the server's message
/// limit, following the same two steps as the V5 driver.
fn smaller_chunk(current: usize) -> anyhow::Result<usize> {
    anyhow::ensure!(
        current > 1,
        "a single-row chunk exceeds the MySQL message limit"
    );
    let fallback = usize::try_from(*MYSQL_FALLBACK_PAGE_SIZE)?;
    Ok(if current <= fallback { 1 } else { fallback })
}

/// The next chunk of a bulk lookup: `smart_chunks`' first chunk until a
/// message-limit error imposed a fixed size.
fn next_chunk<T: ApproxSize>(remaining: &[T], fallback_chunk_size: Option<usize>) -> &[T] {
    match fallback_chunk_size {
        Some(max) => &remaining[..remaining.len().min(max)],
        None => smart_chunks(remaining)
            .next()
            .expect("smart_chunks yields a chunk for a non-empty slice"),
    }
}

fn row_to_document(row: &Row) -> anyhow::Result<DocumentLogEntry> {
    let id = InternalId::try_from(column::bytes(row, 0)?)?;
    let ts: i64 = row.get_opt(1).context("row[1]")??;
    let table_id = TabletId(InternalId::try_from(column::bytes(row, 2)?)?);
    let deleted: bool = row.get_opt(4).context("row[4]")??;
    let value = if deleted {
        None
    } else {
        Some(
            document_encoding::decode(column::bytes(row, 3)?, table_id)?
                .context("non-deleted document has no value")?,
        )
    };
    let prev_ts: Option<i64> = row.get_opt(5).context("row[5]")??;
    Ok(DocumentLogEntry {
        ts: Timestamp::try_from(ts)?,
        id: InternalDocumentId::new(table_id, id),
        value,
        prev_ts: prev_ts.map(Timestamp::try_from).transpose()?,
    })
}

impl<RT: Runtime> crate::v6::backfill::BackfillLease for Lease<RT> {
    async fn transact_read_committed<F, T>(&self, f: F) -> anyhow::Result<T>
    where
        F: for<'a> AsyncFnOnce(
            &'a mut crate::connection::MySqlTransaction<'_>,
        ) -> anyhow::Result<T>,
    {
        Lease::transact_read_committed(self, f).await
    }
}
