#![feature(coroutines)]
#![feature(proc_macro_hygiene)]
#![feature(type_alias_impl_trait)]
mod connection;
mod metrics;
mod sql;

use std::{
    array,
    cmp,
    collections::{
        BTreeMap,
        BTreeSet,
    },
    env,
    error::Error,
    fs,
    ops::{
        Bound,
        Deref,
    },
    path::Path,
    sync::{
        atomic::{
            AtomicBool,
            Ordering::SeqCst,
        },
        Arc,
        LazyLock,
    },
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use anyhow::Context;
use async_trait::async_trait;
use bytes::BytesMut;
use cmd_util::env::env_config;
use common::{
    document::{
        InternalId,
        ResolvedDocument,
    },
    errors::lease_lost_error,
    index::{
        IndexEntry,
        IndexKeyBytes,
        SplitKey,
        MAX_INDEX_KEY_PREFIX_LEN,
    },
    interval::{
        End,
        Interval,
        StartIncluded,
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
        Persistence,
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
        assert_send,
        CoopStreamExt as _,
    },
    sha256::Sha256,
    shutdown::ShutdownSignal,
    types::{
        IndexId,
        IndexRef,
        PersistenceVersion,
        Timestamp,
    },
    value::{
        ConvexValue,
        InternalDocumentId,
        TabletId,
    },
};
use fastrace::{
    func_path,
    future::FutureExt,
    local::LocalSpan,
    Span,
};
use futures::{
    future::{
        self,
    },
    pin_mut,
    stream::{
        self,
        BoxStream,
        StreamExt,
        TryStreamExt,
    },
    try_join,
};
use futures_async_stream::try_stream;
use itertools::Itertools;
use postgres_protocol::escape::escape_literal;
use rustls::{
    ClientConfig,
    KeyLogFile,
    RootCertStore,
};
use rustls_pki_types::{
    pem::PemObject,
    CertificateDer,
};
use serde::Deserialize as _;
use serde_json::Value as JsonValue;
use tokio::sync::{
    mpsc::{
        self,
    },
    oneshot,
};
use tokio_postgres::{
    binary_copy::BinaryCopyInWriter,
    config::TargetSessionAttrs,
    types::{
        to_sql_checked,
        IsNull,
        ToSql,
        Type,
    },
    Row,
};
use tokio_postgres_rustls::MakeRustlsConnect;
use tokio_util::task::AbortOnDropHandle;

pub use crate::connection::ConvexPgPool;
use crate::{
    connection::{
        PostgresConnection,
        PostgresTransaction,
        SchemaName,
    },
    metrics::{
        log_import_batch_rows,
        QueryIndexStats,
    },
};

const ROWS_PER_COPY_BATCH: usize = 1_000_000;
const CHUNK_SIZE: usize = 8;
const NUM_DOCUMENT_PARAMS: usize = 6;
const NUM_INDEX_PARAMS: usize = 8;
// Maximum number of writes within a single transaction. This is the sum of
// TRANSACTION_MAX_SYSTEM_NUM_WRITES and TRANSACTION_MAX_NUM_USER_WRITES.
const MAX_INSERT_SIZE: usize = 56000;
static PIPELINE_QUERIES: LazyLock<usize> = LazyLock::new(|| env_config("PIPELINE_QUERIES", 16));

pub struct PostgresPersistence {
    newly_created: AtomicBool,
    lease: Lease,

    // Used by the reader.
    read_pool: Arc<ConvexPgPool>,
    version: PersistenceVersion,
    schema: SchemaName,
    instance_name: PgInstanceName,
    multitenant: bool,
}

/// Instance name that has been escaped for use as a Postgres literal
#[derive(Clone, Debug)]
pub struct PgInstanceName {
    raw: String,
    escaped: String,
}

impl Deref for PgInstanceName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

impl<T: ToString> From<T> for PgInstanceName {
    fn from(raw: T) -> Self {
        Self::new(raw.to_string())
    }
}

impl PgInstanceName {
    pub fn new(raw: String) -> Self {
        Self {
            escaped: escape_literal(&raw),
            raw,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ConnectError {
    #[error("persistence is read-only, data migration in progress")]
    ReadOnly,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Clone)]
pub struct PostgresOptions {
    pub allow_read_only: bool,
    pub version: PersistenceVersion,
    pub schema: Option<String>,
    pub instance_name: PgInstanceName,
    pub multitenant: bool,
    /// If true, the schema is only partially initialized - enough to call
    /// `write`, but read queries will be slow or broken.
    ///
    /// Indexes will be created the next time the persistence is initialized
    /// with `skip_index_creation: false`.
    pub skip_index_creation: bool,
}

pub struct PostgresReaderOptions {
    pub version: PersistenceVersion,
    pub schema: Option<String>,
    pub instance_name: PgInstanceName,
    pub multitenant: bool,
}

async fn get_current_schema(pool: &ConvexPgPool) -> anyhow::Result<String> {
    let instance_name = PgInstanceName::new("".to_string());
    let mut client = pool
        .get_connection(
            "get_current_schema",
            // This is invalid but we don't use `@db_name`
            const { &SchemaName::EMPTY },
            // This is invalid but we don't use '@instance_name'
            &instance_name,
        )
        .await?;
    let row = client
        .with_retry(async |client| client.query_opt("SELECT current_schema()", &[]).await)
        .await?
        .context("current_schema() returned nothing?")?;
    row.try_get::<_, Option<String>>(0)?
        .context("PostgresOptions::schema not provided and database has no current_schema()?")
}

impl PostgresPersistence {
    pub async fn new(
        url: &str,
        options: PostgresOptions,
        lease_lost_shutdown: ShutdownSignal,
    ) -> Result<Self, ConnectError> {
        let mut config: tokio_postgres::Config =
            url.parse().context("invalid postgres connection url")?;
        config.target_session_attrs(TargetSessionAttrs::ReadWrite);
        let pool = Self::create_pool(config)?;
        Self::with_pool(pool, options, lease_lost_shutdown).await
    }

    pub async fn with_pool(
        pool: Arc<ConvexPgPool>,
        options: PostgresOptions,
        lease_lost_shutdown: ShutdownSignal,
    ) -> Result<Self, ConnectError> {
        if !pool.is_leader_only() {
            return Err(anyhow::anyhow!(
                "PostgresPersistence must be configured with target_session_attrs=read-write"
            )
            .into());
        }

        let schema = if let Some(s) = &options.schema {
            SchemaName::new(s)?
        } else {
            SchemaName::new(&get_current_schema(&pool).await?)?
        };
        {
            let mut client = pool
                .get_connection("init_sql", &schema, &options.instance_name)
                .await?;
            // Only create a new schema if one was specified and it's not
            // already present. This avoids requiring extra permissions to run
            // `CREATE SCHEMA IF NOT EXISTS` if it's already been created.
            if let Some(raw_schema) = options.schema
                && client
                    .with_retry(async move |client| {
                        client
                            .query_opt(sql::CHECK_SCHEMA_SQL, &[&raw_schema])
                            .await
                    })
                    .await?
                    .is_none()
            {
                client.batch_execute(sql::CREATE_SCHEMA_SQL).await?;
            }
            let skip_index_creation = options.skip_index_creation;
            client
                .with_retry(async move |client| {
                    for &(stmt, is_create_index) in sql::init_sql(options.multitenant) {
                        if is_create_index && skip_index_creation {
                            continue;
                        }
                        client.batch_execute(stmt).await?;
                    }
                    Ok(())
                })
                .await?;
        };

        let lease = Lease::acquire(
            pool.clone(),
            &schema,
            options.instance_name.clone(),
            options.multitenant,
            options.allow_read_only,
            lease_lost_shutdown,
        )
        .await?;
        let newly_created = {
            let client = pool
                .get_connection("check_newly_created", &schema, &options.instance_name)
                .await?;
            Self::check_newly_created(&client, options.multitenant, &options.instance_name).await?
        };
        Ok(Self {
            newly_created: newly_created.into(),
            lease,
            read_pool: pool,
            version: options.version,
            schema,
            instance_name: options.instance_name,
            multitenant: options.multitenant,
        })
    }

    pub async fn set_read_only(
        pool: Arc<ConvexPgPool>,
        options: PostgresOptions,
        read_only: bool,
    ) -> anyhow::Result<()> {
        let schema = if let Some(s) = &options.schema {
            SchemaName::new(s)?
        } else {
            SchemaName::new(&get_current_schema(&pool).await?)?
        };
        let multitenant = options.multitenant;
        let mut conn = pool
            .get_connection("set_read_only", &schema, &options.instance_name)
            .await?;
        let instance_name = options.instance_name.clone();
        conn.with_retry(async move |conn| {
            let statement = if read_only {
                sql::set_read_only(multitenant)
            } else {
                sql::unset_read_only(multitenant)
            };
            let statement = conn.prepare_cached(statement).await?;
            let mut params = vec![];
            if multitenant {
                params.push(&instance_name.raw as &(dyn ToSql + Sync));
            }
            conn.execute(&statement, &params).await?;
            Ok(())
        })
        .await?;

        Ok(())
    }

    pub async fn new_reader(
        pool: Arc<ConvexPgPool>,
        options: PostgresReaderOptions,
    ) -> anyhow::Result<PostgresReader> {
        let schema = match options.schema {
            Some(s) => s,
            None => get_current_schema(&pool).await?,
        };
        Ok(PostgresReader {
            read_pool: pool,
            version: options.version,
            schema: SchemaName::new(&schema)?,
            instance_name: options.instance_name,
            multitenant: options.multitenant,
        })
    }

    pub fn create_pool(pg_config: tokio_postgres::Config) -> anyhow::Result<Arc<ConvexPgPool>> {
        let mut roots = RootCertStore::empty();
        let native_certs = rustls_native_certs::load_native_certs();
        anyhow::ensure!(
            native_certs.errors.is_empty(),
            "failed to load native certs: {:?}",
            native_certs.errors
        );
        for cert in native_certs.certs {
            roots.add(cert)?;
        }
        if let Some(ca_file_path) = env::var_os("PG_CA_FILE")
            && !ca_file_path.is_empty()
        {
            let ca_file_path = Path::new(&ca_file_path);
            let ca_file_content = fs::read(ca_file_path)
                .with_context(|| format!("Failed to read CA file: {}", ca_file_path.display()))?;
            for ca_cert in CertificateDer::pem_slice_iter(&ca_file_content) {
                roots.add(ca_cert.with_context(|| {
                    format!("Failed to parse CA file as PEM: {}", ca_file_path.display())
                })?)?;
            }
        }
        let mut config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        if let Ok(path) = env::var("SSLKEYLOGFILE") {
            tracing::warn!("SSLKEYLOGFILE is set, TLS secrets will be logged to {path}");
            config.key_log = Arc::new(KeyLogFile::new());
        }
        let connector = MakeRustlsConnect::new(config);

        Ok(ConvexPgPool::new(pg_config, connector))
    }

    async fn check_newly_created(
        client: &PostgresConnection<'_>,
        multitenant: bool,
        instance_name: &PgInstanceName,
    ) -> anyhow::Result<bool> {
        let mut params = vec![];
        if multitenant {
            params.push(&instance_name.raw as &(dyn ToSql + Sync));
        }
        Ok(client
            .query_opt(sql::check_newly_created(multitenant), &params)
            .await?
            .is_none())
    }
}

#[async_trait]
impl IndexRowPersistence for PostgresPersistence {
    async fn delete_index_rows(&self, expired_entries: Vec<IndexEntry>) -> anyhow::Result<usize> {
        let multitenant = self.multitenant;
        let instance_name = self.instance_name.clone();
        self.lease
            .transact(async move |tx| {
                let mut deleted_count = 0;
                let mut expired_chunks = expired_entries.chunks_exact(CHUNK_SIZE);
                for chunk in &mut expired_chunks {
                    let delete_chunk = tx
                        .prepare_cached(sql::delete_index_chunk(multitenant))
                        .await?;
                    let mut params = chunk
                        .iter()
                        .map(|index_entry| PostgresReader::_index_cursor_params(Some(index_entry)))
                        .flatten_ok()
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    if multitenant {
                        params.push(Param::Text(instance_name.to_string()));
                    }
                    deleted_count += tx.execute_raw(&delete_chunk, params).await?;
                }
                for index_entry in expired_chunks.remainder() {
                    let delete_index = tx.prepare_cached(sql::delete_index(multitenant)).await?;
                    let mut params =
                        PostgresReader::_index_cursor_params(Some(index_entry))?.to_vec();
                    if multitenant {
                        params.push(Param::Text(instance_name.to_string()));
                    }
                    deleted_count += tx.execute_raw(&delete_index, params).await?;
                }
                Ok(deleted_count as usize)
            })
            .await
    }
}

#[async_trait]
impl Persistence for PostgresPersistence {
    fn is_fresh(&self) -> bool {
        self.newly_created.load(SeqCst)
    }

    fn reader(&self) -> Arc<dyn PersistenceReader> {
        Arc::new(PostgresReader {
            read_pool: self.read_pool.clone(),
            version: self.version,
            schema: self.schema.clone(),
            instance_name: self.instance_name.clone(),
            multitenant: self.multitenant,
        })
    }

    #[fastrace::trace]
    async fn write<'a>(
        &self,
        documents: &'a [DocumentLogEntry],
        indexes: &'a [PersistenceIndexEntry],
        conflict_strategy: ConflictStrategy,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(documents.len() <= MAX_INSERT_SIZE);
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
        metrics::log_write_documents(documents.len());
        LocalSpan::add_properties(|| {
            [
                ("num_documents", documents.len().to_string()),
                ("write_size", write_size.to_string()),
            ]
        });

        // True, the below might end up failing and not changing anything.
        self.newly_created.store(false, SeqCst);
        let multitenant = self.multitenant;
        let instance_name = self.instance_name.clone();
        self.lease
            .transact(async move |tx| {
                let (insert_documents, insert_indexes) = try_join!(
                    match conflict_strategy {
                        ConflictStrategy::Error =>
                            tx.prepare_cached(sql::insert_document(multitenant)),
                        ConflictStrategy::Overwrite =>
                            tx.prepare_cached(sql::insert_overwrite_document(multitenant)),
                    },
                    match conflict_strategy {
                        ConflictStrategy::Error =>
                            tx.prepare_cached(sql::insert_index(multitenant)),
                        ConflictStrategy::Overwrite =>
                            tx.prepare_cached(sql::insert_overwrite_index(multitenant)),
                    },
                )?;

                // Split up statements to avoid hitting timeouts.
                const INSERTS_PER_STATEMENT: usize = 1024;

                let insert_docs = async {
                    for chunk in documents.chunks(INSERTS_PER_STATEMENT) {
                        let mut doc_params: [Vec<Param>; NUM_DOCUMENT_PARAMS] =
                            array::from_fn(|_| Vec::with_capacity(chunk.len()));
                        for update in chunk {
                            for (vec, param) in doc_params.iter_mut().zip(document_params(
                                update.ts,
                                update.id,
                                &update.value,
                                update.prev_ts,
                            )?) {
                                vec.push(param);
                            }
                        }
                        let mut doc_params = doc_params
                            .iter()
                            .map(|v| v as &(dyn ToSql + Sync))
                            .collect::<Vec<_>>();
                        if multitenant {
                            doc_params.push(&instance_name.raw as &(dyn ToSql + Sync));
                        }
                        tx.execute_raw(&insert_documents, doc_params).await?;
                    }
                    anyhow::Ok(())
                };

                let insert_idxs = async {
                    for chunk in indexes.chunks(INSERTS_PER_STATEMENT) {
                        let mut idx_params: [Vec<Param>; NUM_INDEX_PARAMS] =
                            array::from_fn(|_| Vec::with_capacity(chunk.len()));
                        for update in chunk {
                            for (vec, param) in idx_params.iter_mut().zip(index_params(update)) {
                                vec.push(param);
                            }
                        }
                        let mut idx_params = idx_params
                            .iter()
                            .map(|v| v as &(dyn ToSql + Sync))
                            .collect::<Vec<_>>();
                        if multitenant {
                            idx_params.push(&instance_name.raw as &(dyn ToSql + Sync));
                        }
                        tx.execute_raw(&insert_indexes, idx_params).await?;
                    }
                    Ok(())
                };

                let timer = metrics::insert_timer();
                try_join!(insert_docs, insert_idxs)?;
                timer.finish();

                Ok(())
            })
            .await
    }

    async fn write_persistence_global(
        &self,
        key: PersistenceGlobalKey,
        value: JsonValue,
    ) -> anyhow::Result<()> {
        let multitenant = self.multitenant;
        let instance_name = self.instance_name.clone();
        self.lease
            .transact(async move |tx| {
                let stmt = tx
                    .prepare_cached(sql::write_persistence_global(multitenant))
                    .await?;
                let mut params = [
                    Param::PersistenceGlobalKey(key),
                    Param::JsonValue(value.to_string()),
                ]
                .to_vec();
                if multitenant {
                    params.push(Param::Text(instance_name.to_string()));
                }
                tx.execute_raw(&stmt, params).await?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    async fn has_index_entries(&self) -> anyhow::Result<bool> {
        let mut client = self
            .read_pool
            .get_connection("has_index_entries", &self.schema, &self.instance_name)
            .await?;
        let mut params = vec![];
        if self.multitenant {
            params.push(Param::Text(self.instance_name.to_string()));
        }
        let multitenant = self.multitenant;
        let row_stream = client
            .with_retry(async move |client| {
                let stmt = client
                    .prepare_cached(sql::has_index_entries(multitenant))
                    .await?;
                client.query_raw(&stmt, &params).await
            })
            .await?;
        futures::pin_mut!(row_stream);
        Ok(row_stream.try_next().await?.is_some())
    }

    async fn reclaim_index_history(
        &self,
        request: IndexRetentionRequest<'_>,
    ) -> anyhow::Result<IndexRetentionProgress> {
        delete_expired_entries(self, request).await
    }

    async fn delete(
        &self,
        documents: Vec<(Timestamp, InternalDocumentId)>,
    ) -> anyhow::Result<usize> {
        let multitenant = self.multitenant;
        let instance_name = self.instance_name.clone();
        self.lease
            .transact(async move |tx| {
                let mut deleted_count = 0;
                let mut expired_chunks = documents.chunks_exact(CHUNK_SIZE);
                for chunk in &mut expired_chunks {
                    let delete_chunk = tx
                        .prepare_cached(sql::delete_document_chunk(multitenant))
                        .await?;
                    let mut params = chunk
                        .iter()
                        .map(PostgresReader::_document_cursor_params)
                        .flatten_ok()
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    if multitenant {
                        params.push(Param::Text(instance_name.to_string()));
                    }
                    deleted_count += tx.execute_raw(&delete_chunk, params).await?;
                }
                for document in expired_chunks.remainder() {
                    let delete_doc = tx.prepare_cached(sql::delete_document(multitenant)).await?;
                    let mut params = PostgresReader::_document_cursor_params(document)?.to_vec();
                    if multitenant {
                        params.push(Param::Text(instance_name.to_string()));
                    }
                    deleted_count += tx.execute_raw(&delete_doc, params).await?;
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
        let instance_name = self.instance_name.clone();
        self.lease
            .transact(async move |tx| {
                let delete_doc = tx
                    .prepare_cached(sql::delete_tablet_chunk(multitenant))
                    .await?;
                let mut params = vec![Param::Bytes(tablet_id.0.into())];
                if multitenant {
                    params.push(Param::Text(instance_name.to_string()));
                }
                params.push(Param::Limit(chunk_size as i64));
                let deleted_count = tx.execute_raw(&delete_doc, params).await?;
                Ok(deleted_count as usize)
            })
            .await
    }

    async fn import_documents_batch(
        &self,
        mut documents: BoxStream<'_, Vec<DocumentLogEntry>>,
    ) -> anyhow::Result<()> {
        let conn = self
            .lease
            .pool
            .get_connection("import_documents_batch", &self.schema, &self.instance_name)
            .await?;
        let stmt = conn
            .prepare_cached(sql::import_documents_batch(self.multitenant))
            .await?;

        'outer: loop {
            let sink = conn.copy_in(&stmt).await?;
            let types = [
                Type::TEXT,
                Type::BYTEA,
                Type::INT8,
                Type::BYTEA,
                Type::BYTEA,
                Type::BOOL,
                Type::INT8,
            ];
            let writer = BinaryCopyInWriter::new(
                sink,
                if self.multitenant {
                    &types
                } else {
                    &types[1..]
                },
            );
            pin_mut!(writer);

            let mut batch_count = 0;

            let mut params: Vec<Param> =
                Vec::with_capacity(NUM_DOCUMENT_PARAMS + self.multitenant as usize);
            if self.multitenant {
                params.push(Param::Text(self.instance_name.to_string()));
            }
            while let Some(chunk) = documents.next().await {
                let rows = chunk.len();
                for document in chunk {
                    params.extend(document_params(
                        document.ts,
                        document.id,
                        &document.value,
                        document.prev_ts,
                    )?);
                    writer.as_mut().write_raw(&params).await?;
                    params.truncate(self.multitenant as usize);
                }
                log_import_batch_rows(rows, "documents");
                batch_count += rows;

                if batch_count >= ROWS_PER_COPY_BATCH {
                    writer.finish().await?;
                    continue 'outer;
                }
            }

            writer.finish().await?;
            break;
        }

        Ok(())
    }

    async fn import_indexes_batch(
        &self,
        mut indexes: BoxStream<'_, Vec<PersistenceIndexEntry>>,
    ) -> anyhow::Result<()> {
        let conn = self
            .lease
            .pool
            .get_connection("import_indexes_batch", &self.schema, &self.instance_name)
            .await?;
        let stmt = conn
            .prepare_cached(sql::import_indexes_batch(self.multitenant))
            .await?;

        'outer: loop {
            let sink = conn.copy_in(&stmt).await?;
            let types = [
                Type::TEXT,
                Type::BYTEA,
                Type::INT8,
                Type::BYTEA,
                Type::BYTEA,
                Type::BYTEA,
                Type::BOOL,
                Type::BYTEA,
                Type::BYTEA,
            ];
            let writer = BinaryCopyInWriter::new(
                sink,
                if self.multitenant {
                    &types
                } else {
                    &types[1..]
                },
            );
            pin_mut!(writer);

            let mut batch_count = 0;

            let mut params: Vec<Param> =
                Vec::with_capacity(NUM_INDEX_PARAMS + self.multitenant as usize);
            if self.multitenant {
                params.push(Param::Text(self.instance_name.to_string()));
            }
            while let Some(chunk) = indexes.next().await {
                let rows = chunk.len();
                for index in chunk {
                    params.extend(index_params(&index));
                    writer.as_mut().write_raw(&params).await?;
                    params.truncate(self.multitenant as usize);
                }
                log_import_batch_rows(rows, "indexes");
                batch_count += rows;

                if batch_count >= ROWS_PER_COPY_BATCH {
                    writer.finish().await?;
                    continue 'outer;
                }
            }

            writer.finish().await?;
            break;
        }

        Ok(())
    }

    /// Runs CREATE INDEX statements that were skipped by `skip_index_creation`.
    async fn finish_loading(&self) -> anyhow::Result<()> {
        let mut client = self
            .lease
            .pool
            .get_connection("finish_loading", &self.schema, &self.instance_name)
            .await?;
        for &(stmt, is_create_index) in sql::init_sql(self.multitenant) {
            if is_create_index {
                tracing::info!("Running: {stmt}");
                assert_send(client.with_retry(async move |client| {
                    client.batch_execute_no_timeout(stmt).await?;
                    Ok(())
                }))
                .await?;
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct PostgresReader {
    read_pool: Arc<ConvexPgPool>,
    version: PersistenceVersion,
    schema: SchemaName,
    instance_name: PgInstanceName,
    multitenant: bool,
}

impl PostgresReader {
    fn row_to_document(
        &self,
        row: Row,
    ) -> anyhow::Result<(
        Timestamp,
        InternalDocumentId,
        Option<ResolvedDocument>,
        Option<Timestamp>,
    )> {
        let bytes: Vec<u8> = row.get(0);
        let internal_id = InternalId::try_from(bytes)?;
        let ts: i64 = row.get(1);
        let ts = Timestamp::try_from(ts)?;
        let tablet_id_bytes: Vec<u8> = row.get(2);
        let binary_value: Vec<u8> = row.get(3);
        let json_value: JsonValue = serde_json::from_slice(&binary_value)
            .context("Failed to deserialize database value")?;

        let deleted: bool = row.get(4);
        let table = TabletId(
            InternalId::try_from(tablet_id_bytes).context("Invalid ID stored in the database")?,
        );
        let document_id = InternalDocumentId::new(table, internal_id);
        let document = if !deleted {
            let value: ConvexValue = json_value.try_into()?;
            Some(ResolvedDocument::from_database(table, value)?)
        } else {
            None
        };
        let prev_ts: Option<i64> = row.get(5);
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
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) {
        let timer = metrics::load_documents_timer();
        let mut num_returned = 0;
        // Construct the initial cursor on (ts, table_id, id);
        // note that this is always an exclusive bound
        let (mut last_ts_param, mut last_tablet_id_param, mut last_id_param) = match order {
            Order::Asc => (
                // `ts >= $1` <==> `(ts, table_id, id) > ($1 - 1, AFTER_ALL_BYTES,
                // AFTER_ALL_BYTES)`
                // N.B.: subtracting 1 never overflows since
                // i64::from(Timestamp::MIN) >= 0
                Param::Ts(i64::from(range.min_timestamp_inclusive()) - 1),
                Param::Bytes(InternalId::AFTER_ALL_BYTES.to_vec()),
                Param::Bytes(InternalId::AFTER_ALL_BYTES.to_vec()),
            ),
            Order::Desc => (
                // `ts < $1` <==> `(ts, table_id, id) < ($1, BEFORE_ALL_BYTES, BEFORE_ALL_BYTES)`
                Param::Ts(i64::from(range.max_timestamp_exclusive())),
                Param::Bytes(InternalId::BEFORE_ALL_BYTES.to_vec()),
                Param::Bytes(InternalId::BEFORE_ALL_BYTES.to_vec()),
            ),
        };
        loop {
            let mut client = self
                .read_pool
                .get_connection("load_documents", &self.schema, &self.instance_name)
                .await?;
            let mut rows_loaded = 0;

            let (query, params) = match order {
                Order::Asc => (
                    sql::load_docs_by_ts_page_asc(
                        self.multitenant,
                        tablet_id.is_some(),
                        include_prev_rev,
                    ),
                    [
                        last_ts_param.clone(),
                        last_tablet_id_param.clone(),
                        last_id_param.clone(),
                        Param::Ts(i64::from(range.max_timestamp_exclusive())),
                        Param::Limit(page_size as i64),
                    ],
                ),
                Order::Desc => (
                    sql::load_docs_by_ts_page_desc(
                        self.multitenant,
                        tablet_id.is_some(),
                        include_prev_rev,
                    ),
                    [
                        Param::Ts(i64::from(range.min_timestamp_inclusive())),
                        last_ts_param.clone(),
                        last_tablet_id_param.clone(),
                        last_id_param.clone(),
                        Param::Limit(page_size as i64),
                    ],
                ),
            };
            let mut params = params.to_vec();
            if let Some(tablet_id) = tablet_id {
                params.push(Param::Bytes(tablet_id.0.to_vec()));
            }
            if self.multitenant {
                params.push(Param::Text(self.instance_name.to_string()));
            }
            let row_stream = assert_send(client.with_retry(async move |client| {
                let stmt = client.prepare_cached(query).await?;
                client.query_raw(&stmt, &params).await
            }))
            .await?;

            futures::pin_mut!(row_stream);

            let mut batch = vec![];
            while let Some(row) = row_stream.try_next().await? {
                let prev_rev_value: Option<Vec<u8>> = if include_prev_rev {
                    row.try_get(6)?
                } else {
                    None
                };
                let (ts, document_id, document, prev_ts) = self.row_to_document(row)?;
                let prev_rev_document: Option<ResolvedDocument> = prev_rev_value
                    .map(|v| {
                        let json_value: JsonValue = serde_json::from_slice(&v)
                            .context("Failed to deserialize database value")?;
                        // N.B.: previous revisions should never be deleted, so we don't check that.
                        let value: ConvexValue = json_value.try_into()?;
                        ResolvedDocument::from_database(document_id.table(), value)
                    })
                    .transpose()?;
                rows_loaded += 1;
                last_ts_param = Param::Ts(i64::from(ts));
                last_tablet_id_param = Param::TableId(document_id.table());
                last_id_param = internal_doc_id_param(document_id);
                num_returned += 1;
                batch.push(RevisionPair {
                    id: document_id,
                    rev: DocumentRevision { ts, document },
                    prev_rev: prev_ts.map(|prev_ts| DocumentRevision {
                        ts: prev_ts,
                        document: prev_rev_document,
                    }),
                });
            }
            // Return the connection to the pool as soon as possible.
            drop(client);

            // N.B.: `retention_validator` can itself talk back to this
            // PersistenceReader (to call get_persistence_global). This uses a
            // separate connection.
            // TODO: ideally we should be using the same connection.
            // If we ever run against a replica DB this should also make sure
            // that the data read & validation read run against the same
            // replica - otherwise we may not be validating the right thing!
            retention_validator
                .validate_document_snapshot(range.min_timestamp_inclusive())
                .await?;

            for row in batch {
                yield row;
            }
            if rows_loaded < page_size {
                break;
            }
        }

        metrics::finish_load_documents_timer(timer, num_returned);
    }

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
        // We use the size_hint to determine the batch size. This means in the
        // common case we should do a single query. Exceptions are if the size_hint
        // is wrong or if we truncate it or if we observe too many deletes.
        let batch_size = size_hint.clamp(1, 5000);
        let (tx, mut rx) = mpsc::channel(batch_size);
        let task = AbortOnDropHandle::new(common::runtime::tokio_spawn(
            func_path!(),
            self.clone()
                ._index_scan_inner(
                    index_id,
                    read_timestamp,
                    interval,
                    order,
                    batch_size,
                    retention_validator,
                    tx,
                )
                .in_span(Span::enter_with_local_parent(
                    // For some reason #[fastrace::trace] on _index_scan_inner
                    // causes the compiler to blow up in memory usage, so do
                    // this here instead
                    "postgres::PostgresReader::_index_scan_inner",
                )),
        ));
        while let Some(result) = rx.recv().await {
            match result {
                IndexScanResult::Row {
                    key,
                    ts,
                    json,
                    prev_ts,
                } => {
                    let json_value: JsonValue = serde_json::from_slice(&json)
                        .context("Failed to deserialize database value")?;
                    anyhow::ensure!(
                        json_value != JsonValue::Null,
                        "Index reference to deleted document {:?} {:?}",
                        key,
                        ts
                    );
                    let value: ConvexValue = json_value.try_into()?;
                    let document = ResolvedDocument::from_database(tablet_id, value)?;
                    yield (
                        key,
                        LatestDocument {
                            ts,
                            value: document,
                            prev_ts,
                        },
                    );
                },
                IndexScanResult::PageBoundary(sender) => {
                    _ = sender.send(());
                },
            }
        }
        task.await??;
    }

    async fn _index_scan_inner(
        self,
        index_id: IndexId,
        read_timestamp: Timestamp,
        interval: Interval,
        order: Order,
        batch_size: usize,
        retention_validator: Arc<dyn RetentionValidator>,
        tx: mpsc::Sender<IndexScanResult>,
    ) -> anyhow::Result<()> {
        let _timer = metrics::query_index_timer();
        let multitenant = self.multitenant;
        let instance_name = self.instance_name.clone();
        let (mut lower, mut upper) = to_sql_bounds(interval.clone());

        let mut stats = QueryIndexStats::new();

        // We iterate results in (key_prefix, key_sha256) order while we actually
        // need them in (key_prefix, key_suffix order). key_suffix is not part of the
        // primary key so we do the sort here. If see any record with maximum length
        // prefix, we should buffer it until we reach a different prefix.
        let mut result_buffer: Vec<(IndexKeyBytes, Timestamp, Vec<u8>, Option<Timestamp>)> =
            Vec::new();
        loop {
            let mut client = self
                .read_pool
                .get_connection("index_scan", &self.schema, &self.instance_name)
                .await?;
            stats.sql_statements += 1;
            let (query, params) = index_query(
                index_id,
                read_timestamp,
                lower.clone(),
                upper.clone(),
                order,
                batch_size,
                multitenant,
                &instance_name,
            );

            let row_stream = assert_send(client.with_retry(async move |client| {
                let prepare_timer = metrics::query_index_sql_prepare_timer();
                let stmt = client.prepare_cached(query).await?;
                prepare_timer.finish();
                let execute_timer = metrics::query_index_sql_execute_timer();
                let row_stream = client.query_raw(&stmt, &params).await?;
                execute_timer.finish();
                Ok(row_stream)
            }))
            .await?;

            futures::pin_mut!(row_stream);

            let mut batch_rows = 0;
            let mut batch = vec![];
            while let Some(row) = row_stream.try_next().await? {
                batch_rows += 1;

                // Fetch
                let internal_row = parse_row(&row)?;

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
                            batch.push((key, ts, doc, prev_ts));
                        } else {
                            stats.rows_skipped_out_of_range += 1;
                        }
                    }
                }

                // Update the bounds for future queries.
                let bound = Bound::Excluded(SqlKey {
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
                let table: Option<Vec<u8>> = row.get(7);
                table.ok_or_else(|| {
                    anyhow::anyhow!("Dangling index reference for {:?} {:?}", key, ts)
                })?;
                let binary_value: Vec<u8> = row.get(8);

                let prev_ts: Option<i64> = row.get(9);
                let prev_ts = prev_ts.map(Timestamp::try_from).transpose()?;

                if key.len() < MAX_INDEX_KEY_PREFIX_LEN {
                    assert!(result_buffer.is_empty());
                    if interval.contains(&key) {
                        stats.rows_returned += 1;
                        batch.push((IndexKeyBytes(key), ts, binary_value, prev_ts));
                    } else {
                        stats.rows_skipped_out_of_range += 1;
                    }
                } else {
                    // There might be other records with the same key_prefix that
                    // are ordered before this result. Buffer it.
                    result_buffer.push((IndexKeyBytes(key), ts, binary_value, prev_ts));
                    stats.max_rows_buffered =
                        cmp::max(result_buffer.len(), stats.max_rows_buffered);
                }
            }

            // Return the connection to the pool as soon as possible.
            drop(client);

            // N.B.: `retention_validator` can itself talk back to this
            // PersistenceReader (to call get_persistence_global). This uses a
            // separate connection.
            // TODO: ideally we should be using the same connection.
            // If we ever run against a replica DB this should also make sure
            // that the data read & validation read run against the same
            // replica - otherwise we may not be validating the right thing!
            let retention_validate_timer = metrics::retention_validate_timer();
            retention_validator
                .validate_snapshot(read_timestamp)
                .await?;
            retention_validate_timer.finish();

            for (key, ts, json, prev_ts) in batch {
                // this could block arbitrarily long if the caller of
                // `index_scan` stops polling
                tx.send(IndexScanResult::Row {
                    key,
                    ts,
                    json,
                    prev_ts,
                })
                .await?;
            }

            if batch_rows < batch_size {
                break;
            }

            // After each page, wait until the caller of `index_scan()` next
            // polls the stream before beginning the next read. This hurts
            // latency if the caller wants to read everything, but avoids
            // prefetching an extra page in the common case where the caller
            // only reads the first `batch_size` rows.
            let (page_tx, page_rx) = oneshot::channel();
            tx.send(IndexScanResult::PageBoundary(page_tx)).await?;
            if page_rx.await.is_err() {
                // caller dropped
                return Ok(());
            }
        }

        // Yield any remaining values.
        result_buffer.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, ts, json, prev_ts) in order.apply(result_buffer.drain(..)) {
            if interval.contains(&key) {
                stats.rows_returned += 1;
                tx.send(IndexScanResult::Row {
                    key,
                    ts,
                    json,
                    prev_ts,
                })
                .await?;
            } else {
                stats.rows_skipped_out_of_range += 1;
            }
        }

        Ok(())
    }

    fn _index_cursor_params(cursor: Option<&IndexEntry>) -> anyhow::Result<Vec<Param>> {
        match cursor {
            Some(cursor) => {
                let last_id_param = Param::Bytes(cursor.index_id.0.into());
                let last_key_prefix = Param::Bytes(cursor.key_prefix.clone());
                let last_sha256 = Param::Bytes(cursor.key_sha256.clone());
                let last_ts = Param::Ts(cursor.ts.into());
                Ok(vec![last_id_param, last_key_prefix, last_sha256, last_ts])
            },
            None => {
                let last_id_param = Param::Bytes(InternalId::BEFORE_ALL_BYTES.to_vec());
                let last_key_prefix = Param::Bytes(vec![]);
                let last_sha256 = Param::Bytes(vec![]);
                let last_ts = Param::Ts(0);
                Ok(vec![last_id_param, last_key_prefix, last_sha256, last_ts])
            },
        }
    }

    fn _document_cursor_params(
        (ts, internal_id): &(Timestamp, InternalDocumentId),
    ) -> anyhow::Result<Vec<Param>> {
        let tablet_id = Param::Bytes(internal_id.table().0.into());
        let id = Param::Bytes(internal_id.internal_id().into());
        let ts = Param::Ts((*ts).into());
        Ok(vec![tablet_id, id, ts])
    }
}

enum IndexScanResult {
    Row {
        key: IndexKeyBytes,
        ts: Timestamp,
        json: Vec<u8>,
        prev_ts: Option<Timestamp>,
    },
    // After a page the index scan will wait until a message is sent on this
    // channel
    PageBoundary(oneshot::Sender<()>),
}

fn parse_row(row: &Row) -> anyhow::Result<IndexEntry> {
    let bytes: Vec<u8> = row.get(0);
    let index_id = InternalId::try_from(bytes)
        .map_err(|_| anyhow::anyhow!("index_id wrong size"))?
        .into();

    let key_prefix: Vec<u8> = row.get(1);
    let key_sha256: Vec<u8> = row.get(2);
    let key_suffix: Option<Vec<u8>> = row.get(3);
    let ts: i64 = row.get(4);
    let ts = Timestamp::try_from(ts)?;
    let deleted: bool = row.get(5);
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
impl PersistenceReader for PostgresReader {
    fn load_documents(
        &self,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentStream<'_> {
        self._load_documents(None, false, range, order, page_size, retention_validator)
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

    async fn previous_revisions(
        &self,
        ids: BTreeSet<(InternalDocumentId, Timestamp)>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<(InternalDocumentId, Timestamp), DocumentLogEntry>> {
        let timer = metrics::prev_revisions_timer();
        let multitenant = self.multitenant;
        let instance_name = self.instance_name.clone();

        let mut client = self
            .read_pool
            .get_connection("previous_revisions", &self.schema, &self.instance_name)
            .await?;
        let (prev_rev_chunk, prev_rev) = client
            .with_retry(async move |client| {
                try_join!(
                    client.prepare_cached(sql::prev_rev_chunk(multitenant)),
                    client.prepare_cached(sql::prev_rev(multitenant))
                )
            })
            .await?;
        let ids: Vec<_> = ids.into_iter().collect();

        let mut result = BTreeMap::new();

        let mut chunks = ids.chunks_exact(CHUNK_SIZE);
        // NOTE we rely on `client.query_raw` not executing anything until it is
        // awaited, i.e. we rely on it being an async fn and not just returning an
        // `impl Future`. This guarantees only `PIPELINE_QUERIES` queries are in
        // the pipeline at once.
        let mut result_futures = vec![];

        let mut min_ts = Timestamp::MAX;
        for chunk in chunks.by_ref() {
            assert_eq!(chunk.len(), 8);
            let mut params = Vec::with_capacity(24);
            for (id, ts) in chunk {
                params.push(Param::TableId(id.table()));
                params.push(internal_doc_id_param(*id));
                params.push(Param::Ts(i64::from(*ts)));
                min_ts = cmp::min(*ts, min_ts);
            }
            if multitenant {
                params.push(Param::Text(instance_name.to_string()));
            }
            result_futures.push(client.query_raw(&prev_rev_chunk, params));
        }
        for (id, ts) in chunks.remainder() {
            let mut params = vec![
                Param::TableId(id.table()),
                internal_doc_id_param(*id),
                Param::Ts(i64::from(*ts)),
            ];
            if multitenant {
                params.push(Param::Text(instance_name.to_string()));
            }
            min_ts = cmp::min(*ts, min_ts);
            result_futures.push(client.query_raw(&prev_rev, params));
        }
        let mut result_stream = stream::iter(result_futures).buffered(*PIPELINE_QUERIES);
        while let Some(row_stream) = result_stream.try_next().await? {
            futures::pin_mut!(row_stream);
            while let Some(row) = row_stream.try_next().await? {
                let ts: i64 = row.get(6);
                let ts = Timestamp::try_from(ts)?;
                let (prev_ts, id, maybe_doc, prev_prev_ts) = self.row_to_document(row)?;
                min_ts = cmp::min(ts, min_ts);
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
            }
        }

        retention_validator
            .validate_document_snapshot(min_ts)
            .await?;
        timer.finish();
        Ok(result)
    }

    async fn previous_revisions_of_documents(
        &self,
        ids: BTreeSet<DocumentPrevTsQuery>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<DocumentPrevTsQuery, DocumentLogEntry>> {
        let timer = metrics::previous_revisions_of_documents_timer();
        let multitenant = self.multitenant;
        let instance_name = self.instance_name.clone();

        let mut client = self
            .read_pool
            .get_connection(
                "previous_revisions_of_documents",
                &self.schema,
                &self.instance_name,
            )
            .await?;
        let (exact_rev_chunk, exact_rev) = client
            .with_retry(async move |client| {
                try_join!(
                    client.prepare_cached(sql::exact_rev_chunk(multitenant)),
                    client.prepare_cached(sql::exact_rev(multitenant))
                )
            })
            .await?;
        let ids: Vec<_> = ids.into_iter().collect();

        let mut result = BTreeMap::new();

        let mut chunks = ids.chunks_exact(CHUNK_SIZE);
        let mut result_futures = vec![];

        for chunk in chunks.by_ref() {
            assert_eq!(chunk.len(), 8);
            let mut params = Vec::with_capacity(24);
            for DocumentPrevTsQuery { id, ts, prev_ts } in chunk {
                params.push(Param::TableId(id.table()));
                params.push(internal_doc_id_param(*id));
                params.push(Param::Ts(i64::from(*prev_ts)));
                params.push(Param::Ts(i64::from(*ts)));
            }
            if multitenant {
                params.push(Param::Text(instance_name.to_string()));
            }
            result_futures.push(client.query_raw(&exact_rev_chunk, params));
        }
        for DocumentPrevTsQuery { id, ts, prev_ts } in chunks.remainder() {
            let mut params = vec![
                Param::TableId(id.table()),
                internal_doc_id_param(*id),
                Param::Ts(i64::from(*prev_ts)),
                Param::Ts(i64::from(*ts)),
            ];
            if multitenant {
                params.push(Param::Text(instance_name.to_string()));
            }
            result_futures.push(client.query_raw(&exact_rev, params));
        }
        let mut result_stream = stream::iter(result_futures).buffered(*PIPELINE_QUERIES);
        while let Some(row_stream) = result_stream.try_next().await? {
            futures::pin_mut!(row_stream);
            while let Some(row) = row_stream.try_next().await? {
                let ts: i64 = row.get(6);
                let ts = Timestamp::try_from(ts)?;
                let (prev_ts, id, maybe_doc, prev_prev_ts) = self.row_to_document(row)?;
                anyhow::ensure!(result
                    .insert(
                        DocumentPrevTsQuery { id, ts, prev_ts },
                        DocumentLogEntry {
                            ts: prev_ts,
                            id,
                            value: maybe_doc,
                            prev_ts: prev_prev_ts,
                        }
                    )
                    .is_none());
            }
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

    async fn get_persistence_global(
        &self,
        key: PersistenceGlobalKey,
    ) -> anyhow::Result<Option<JsonValue>> {
        let mut client = self
            .read_pool
            .get_connection("get_persistence_global", &self.schema, &self.instance_name)
            .await?;
        let mut params = vec![Param::PersistenceGlobalKey(key)];
        if self.multitenant {
            params.push(Param::Text(self.instance_name.to_string()));
        }
        let multitenant = self.multitenant;
        let row_stream = client
            .with_retry(async move |client| {
                let stmt = client
                    .prepare_cached(sql::get_persistence_global(multitenant))
                    .await?;
                client.query_raw(&stmt, &params).await
            })
            .await?;
        futures::pin_mut!(row_stream);

        let row = row_stream.try_next().await?;
        let value = row.map(|r| -> anyhow::Result<JsonValue> {
            let binary_value: Vec<u8> = r.get(0);
            let mut json_deserializer = serde_json::Deserializer::from_slice(&binary_value);
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
        self.version
    }

    async fn table_size_stats(&self) -> anyhow::Result<Vec<PersistenceTableSize>> {
        let mut client = self
            .read_pool
            .get_connection("table_size_stats", &self.schema, &self.instance_name)
            .await?;
        let mut stats = vec![];
        for &table in sql::TABLES {
            let full_name = format!("{}.{table}", self.schema.escaped);
            let row = client
                .with_retry(async move |client| {
                    client.query_opt(sql::TABLE_SIZE_QUERY, &[&full_name]).await
                })
                .await?
                .context("nothing returned from table size query?")?;
            stats.push(PersistenceTableSize {
                table_name: table.to_owned(),
                data_bytes: row.try_get::<_, i64>(0)? as u64,
                index_bytes: row.try_get::<_, i64>(1)? as u64,
                row_count: row.try_get::<_, i64>(2)?.try_into().ok(),
            });
        }
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
/// transaction, and otherwise return a `LeaseLostError`.
struct Lease {
    pool: Arc<ConvexPgPool>,
    lease_ts: i64,
    schema: SchemaName,
    instance_name: PgInstanceName,
    multitenant: bool,
    lease_lost_shutdown: ShutdownSignal,
}

impl Lease {
    /// Acquire a lease. Blocks as long as there is another lease holder.
    /// Returns any transient errors encountered.
    async fn acquire(
        pool: Arc<ConvexPgPool>,
        schema: &SchemaName,
        instance_name: PgInstanceName,
        multitenant: bool,
        allow_read_only: bool,
        lease_lost_shutdown: ShutdownSignal,
    ) -> anyhow::Result<Self> {
        let timer = metrics::lease_acquire_timer();
        let mut client = pool
            .get_connection("lease_acquire", schema, &instance_name)
            .await?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("before 1970")
            .as_nanos() as i64;

        tracing::info!("attempting to acquire lease");
        let stmt = client
            .with_retry(async move |client| {
                client
                    .prepare_cached(sql::lease_acquire(multitenant, allow_read_only))
                    .await
            })
            .await?;
        let mut params = vec![&ts as &(dyn ToSql + Sync)];
        if multitenant {
            params.push(&instance_name.raw as &(dyn ToSql + Sync));
        }
        let rows_modified = client.execute(&stmt, params.as_slice()).await?;
        drop(client);
        anyhow::ensure!(
            rows_modified == 1,
            "failed to acquire lease: already acquired with higher timestamp, or read-only"
        );
        tracing::info!("lease acquired with ts {}", ts);

        timer.finish();
        Ok(Self {
            pool,
            lease_ts: ts,
            schema: schema.clone(),
            instance_name,
            multitenant,
            lease_lost_shutdown,
        })
    }

    /// Execute the transaction function f atomically ensuring that the lease is
    /// still held, otherwise return `LeaseLostError`.
    ///
    /// Once `transact` returns `LeaseLostError`, no future transactions using
    /// it will succeed. Instead, a new `Lease` must be made with `acquire`,
    /// and any in-memory state then resynced because of any changes that
    /// might've been made to the database state while the lease was not
    /// held.
    async fn transact<F, T>(&self, f: F) -> anyhow::Result<T>
    where
        F: for<'a> AsyncFnOnce(&'a PostgresTransaction) -> anyhow::Result<T>,
    {
        let mut client = self
            .pool
            .get_connection("transact", &self.schema, &self.instance_name)
            .await?;
        // Retry once to open a transaction. We can't use `with_retry` for
        // awkward borrow checker reasons.
        let mut retried_err = None;
        let mut reconnected = false;
        let tx = 'retry: loop {
            if let Some(e) = retried_err {
                if reconnected || !client.reconnect_if_poisoned().await? {
                    return Err(e);
                }
                reconnected = true;
            }
            match client.transaction().await {
                Ok(tx) => break 'retry tx,
                Err(e) => {
                    retried_err = Some(e);
                    continue 'retry;
                },
            }
        };
        let lease_ts = self.lease_ts;

        let advisory_lease_check = async {
            let timer = metrics::lease_check_timer();
            let stmt = tx
                .prepare_cached(sql::advisory_lease_check(self.multitenant))
                .await?;
            let mut params = vec![&lease_ts as &(dyn ToSql + Sync)];
            if self.multitenant {
                params.push(&self.instance_name.raw as &(dyn ToSql + Sync));
            }
            let rows = tx.query(&stmt, params.as_slice()).await?;
            if rows.len() != 1 {
                self.lease_lost_shutdown.signal(lease_lost_error());
                return Err(lease_lost_error());
            }
            timer.finish();
            Ok(())
        };

        let ((), result) = future::try_join(advisory_lease_check, f(&tx)).await?;

        // We don't run SELECT FOR UPDATE until the *end* of the transaction
        // to minimize the time spent holding the row lock, and therefore allow
        // the lease to be stolen as much as possible.
        let timer = metrics::lease_precond_timer();
        let stmt = tx
            .prepare_cached(sql::lease_precond(self.multitenant))
            .await?;
        let mut params = vec![&lease_ts as &(dyn ToSql + Sync)];
        if self.multitenant {
            params.push(&self.instance_name.raw as &(dyn ToSql + Sync));
        }
        let rows = tx.query(&stmt, params.as_slice()).await?;
        if rows.len() != 1 {
            self.lease_lost_shutdown.signal(lease_lost_error());
            return Err(lease_lost_error());
        }
        timer.finish();

        let timer = metrics::commit_timer();
        tx.commit().await?;
        timer.finish();

        Ok(result)
    }
}

fn document_params(
    ts: Timestamp,
    id: InternalDocumentId,
    maybe_document: &Option<ResolvedDocument>,
    prev_ts: Option<Timestamp>,
) -> anyhow::Result<[Param; NUM_DOCUMENT_PARAMS]> {
    let (json_value, deleted) = match maybe_document {
        Some(doc) => (doc.value().json_serialize()?, false),
        None => (JsonValue::Null.to_string(), true),
    };

    Ok([
        internal_doc_id_param(id),
        Param::Ts(i64::from(ts)),
        Param::TableId(id.table()),
        Param::JsonValue(json_value),
        Param::Deleted(deleted),
        match prev_ts {
            Some(prev_ts) => Param::Ts(i64::from(prev_ts)),
            None => Param::None,
        },
    ])
}

fn internal_id_param(id: InternalId) -> Param {
    Param::Bytes(id.into())
}

fn internal_doc_id_param(id: InternalDocumentId) -> Param {
    internal_id_param(id.internal_id())
}

fn index_params(update: &PersistenceIndexEntry) -> [Param; NUM_INDEX_PARAMS] {
    let key: Vec<u8> = update.key.to_vec();
    let key_sha256 = Sha256::hash(&key);
    let key = SplitKey::new(key);

    let (deleted, tablet_id, doc_id) = match &update.value {
        None => (Param::Deleted(true), Param::None, Param::None),
        Some(doc_id) => (
            Param::Deleted(false),
            Param::TableId(doc_id.table()),
            internal_doc_id_param(*doc_id),
        ),
    };
    [
        internal_id_param(update.index.id().0),
        Param::Ts(i64::from(update.ts)),
        Param::Bytes(key.prefix),
        match key.suffix {
            Some(key_suffix) => Param::Bytes(key_suffix),
            None => Param::None,
        },
        Param::Bytes(key_sha256.to_vec()),
        deleted,
        tablet_id,
        doc_id,
    ]
}

#[derive(Clone, Debug)]
enum Param {
    None,
    Ts(i64),
    Limit(i64),
    TableId(TabletId),
    JsonValue(String),
    Deleted(bool),
    Bytes(Vec<u8>),
    PersistenceGlobalKey(PersistenceGlobalKey),
    Text(String),
}

impl ToSql for Param {
    to_sql_checked!();

    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Send + Sync + 'static>> {
        match self {
            Param::None => Ok(IsNull::Yes),
            Param::Ts(ts) => ts.to_sql(ty, out),
            Param::TableId(s) => s.0 .0.to_vec().to_sql(ty, out),
            Param::JsonValue(v) => v.as_bytes().to_sql(ty, out),
            Param::Deleted(d) => d.to_sql(ty, out),
            Param::Bytes(v) => v.to_sql(ty, out),
            Param::Limit(v) => v.to_sql(ty, out),
            Param::PersistenceGlobalKey(key) => String::from(*key).to_sql(ty, out),
            Param::Text(v) => v.to_sql(ty, out),
        }
    }

    // TODO(presley): This does not seem right. Perhaps we should simply
    // use native types?
    fn accepts(ty: &Type) -> bool {
        i64::accepts(ty)
            || String::accepts(ty)
            || JsonValue::accepts(ty)
            || bool::accepts(ty)
            || Vec::<u8>::accepts(ty)
    }
}

static MIN_SHA256: LazyLock<Vec<u8>> = LazyLock::new(|| vec![0; 32]);
static MAX_SHA256: LazyLock<Vec<u8>> = LazyLock::new(|| vec![255; 32]);

// The key we use to paginate in SQL, note that we can't use key_prefix since
// it is not part of the primary key. We use key_sha256 instead.
#[derive(Clone)]
struct SqlKey {
    prefix: Vec<u8>,
    sha256: Vec<u8>,
}

impl SqlKey {
    // Returns the maximum possible
    fn min_with_same_prefix(key: Vec<u8>) -> Self {
        let key = SplitKey::new(key);
        Self {
            prefix: key.prefix,
            sha256: MIN_SHA256.clone(),
        }
    }

    fn max_with_same_prefix(key: Vec<u8>) -> Self {
        let key = SplitKey::new(key);
        Self {
            prefix: key.prefix,
            sha256: MAX_SHA256.clone(),
        }
    }
}

// Translates a range to a SqlKey bounds we can use to get records in that
// range. Note that because the SqlKey does not sort the same way as IndexKey
// for very long keys, the returned range might contain extra keys that needs to
// be filtered application side.
fn to_sql_bounds(interval: Interval) -> (Bound<SqlKey>, Bound<SqlKey>) {
    let lower = match interval.start {
        StartIncluded(key) => {
            // This can potentially include more results than needed.
            Bound::Included(SqlKey::min_with_same_prefix(key.into()))
        },
    };
    let upper = match interval.end {
        End::Excluded(key) => {
            if key.len() < MAX_INDEX_KEY_PREFIX_LEN {
                Bound::Excluded(SqlKey::min_with_same_prefix(key.into()))
            } else {
                // We can't exclude the bound without potentially excluding other
                // keys that fall within the range.
                Bound::Included(SqlKey::max_with_same_prefix(key.into()))
            }
        },
        End::Unbounded => Bound::Unbounded,
    };
    (lower, upper)
}

fn index_query(
    index_id: IndexId,
    read_timestamp: Timestamp,
    lower: Bound<SqlKey>,
    upper: Bound<SqlKey>,
    order: Order,
    batch_size: usize,
    multitenant: bool,
    instance_name: &PgInstanceName,
) -> (&'static str, Vec<Param>) {
    let mut params: Vec<Param> = vec![
        internal_id_param(index_id.0),
        Param::Ts(read_timestamp.into()),
    ];

    let mut map_bound = |b: Bound<SqlKey>| -> sql::BoundType {
        match b {
            Bound::Unbounded => sql::BoundType::Unbounded,
            Bound::Excluded(sql_key) => {
                params.push(Param::Bytes(sql_key.prefix));
                params.push(Param::Bytes(sql_key.sha256));
                sql::BoundType::Excluded
            },
            Bound::Included(sql_key) => {
                params.push(Param::Bytes(sql_key.prefix));
                params.push(Param::Bytes(sql_key.sha256));
                sql::BoundType::Included
            },
        }
    };

    let lt = map_bound(lower);
    let ut = map_bound(upper);
    params.push(Param::Limit(batch_size as i64));

    // Add instance_name parameter if multitenant
    if multitenant {
        params.push(Param::Text(instance_name.to_string()));
    }

    let query = sql::index_queries(multitenant)
        .get(&(lt, ut, order))
        .unwrap();
    (query, params)
}
