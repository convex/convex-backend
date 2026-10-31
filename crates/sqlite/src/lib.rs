#![feature(try_blocks_heterogeneous)]
#![feature(coroutines)]

use std::{
    cmp,
    collections::{
        BTreeMap,
        BTreeSet,
    },
    path::Path,
    sync::Arc,
};

use anyhow::Context as _;
use async_trait::async_trait;
use common::{
    document::{
        InternalId,
        ResolvedDocument,
    },
    index::{
        IndexEntry,
        IndexKeyBytes,
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
        DocumentStream,
        IndexRetentionProgress,
        IndexRetentionRequest,
        IndexStream,
        LatestDocument,
        Persistence,
        PersistenceGlobalKey,
        PersistenceIndexEntry,
        PersistenceReader,
        RetentionValidator,
        TimestampRange,
    },
    query::Order,
    runtime::CoopStreamExt as _,
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
use futures::{
    stream,
    StreamExt,
};
use futures_async_stream::try_stream;
use parking_lot::Mutex;
use rusqlite::{
    params,
    types::Null,
    Connection,
    Row,
    ToSql,
};
use serde::Deserialize as _;
use serde_json::Value as JsonValue;

// We only have a single Sqlite connection which does not allow async calls, so
// we can't really make queries concurrent.
pub struct SqlitePersistence {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    newly_created: bool,
    connection: Connection,
}

impl SqlitePersistence {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        let newly_created = !Path::new(path).exists();
        let connection = Connection::open(path)?;
        // Execute create tables unconditionally since they are idempotent.
        connection.execute_batch(DOCUMENTS_INIT)?;
        connection.execute_batch(INDEXES_INIT)?;
        connection.execute_batch(PERSISTENCE_GLOBALS_INIT)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                newly_created,
                connection,
            })),
        })
    }

    #[allow(clippy::needless_lifetimes)]
    #[try_stream(ok = T, error = anyhow::Error)]
    async fn validate_snapshot<T: 'static>(
        &self,
        ts: Timestamp,
        retention_validator: Arc<dyn RetentionValidator>,
    ) {
        retention_validator.validate_snapshot(ts).await?;
    }

    #[allow(clippy::needless_lifetimes)]
    #[try_stream(ok = T, error = anyhow::Error)]
    async fn validate_document_snapshot<T: 'static>(
        &self,
        ts: Timestamp,
        retention_validator: Arc<dyn RetentionValidator>,
    ) {
        retention_validator.validate_document_snapshot(ts).await?;
    }

    fn _index_scan_inner(
        &self,
        index_id: IndexId,
        tablet_id: TabletId,
        read_timestamp: Timestamp,
        interval: &Interval,
        order: Order,
    ) -> anyhow::Result<Vec<anyhow::Result<(IndexKeyBytes, LatestDocument)>>> {
        let interval = interval.clone();
        let index_id = &index_id.0[..];
        let read_timestamp: i64 = read_timestamp.into();

        let mut params = params![index_id, read_timestamp].to_vec();

        let StartIncluded(ref start) = interval.start;
        let start_bytes = &start[..];

        params.push(&start_bytes);
        let lower = format!(" AND key >= ${}", params.len());

        let end_bytes = match interval.end {
            End::Excluded(ref t) => Some(&t[..]),
            End::Unbounded => None,
        };
        let upper = match end_bytes {
            Some(ref t) => {
                params.push(t);
                format!(" AND key < ${}", params.len())
            },
            None => "".to_owned(),
        };

        let order = match order {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        };
        let query = format!(
            r#"
SELECT B.key, B.ts, B.document_id, C.table_id, C.json_value, C.prev_ts
FROM (
    SELECT index_id, key, MAX(ts) as max_ts
    FROM indexes
    WHERE index_id = $1 AND ts <= $2{lower}{upper}
    GROUP BY index_id, key
) A
JOIN indexes B
ON B.deleted is FALSE
AND A.index_id = B.index_id
AND A.key = B.key
AND A.max_ts = B.ts
LEFT JOIN documents C
ON B.ts = C.ts
AND B.table_id = c.table_id
AND B.document_id = C.id
ORDER BY B.key {order}
"#,
        );

        let connection = &self.inner.lock().connection;
        let mut stmt = connection.prepare(&query)?;
        let row_iter = stmt.query_map(&params[..], |row| {
            let key = IndexKeyBytes(row.get::<_, Vec<u8>>(0)?);
            let ts = Timestamp::try_from(row.get::<_, i64>(1)?).expect("timestamp out of bounds");
            let document_id = row.get::<_, Vec<u8>>(2)?;
            let table: Option<Vec<u8>> = row.get(3)?;
            let json_value: Option<String> = row.get(4)?;
            let prev_ts: Option<Timestamp> = row
                .get::<_, Option<i64>>(5)?
                .map(|ts| Timestamp::try_from(ts).expect("prev_ts out of bounds"));

            Ok((key, ts, document_id, table, json_value, prev_ts))
        })?;
        let mut triples = vec![];
        for row in row_iter {
            let (key, ts, document_id, table, json_value, prev_ts) = row?;
            let table = table.ok_or_else(|| {
                anyhow::anyhow!("Dangling index reference for {:?} {:?}", key, ts)
            })?;
            let table = TabletId(table.try_into()?);
            let _document_id = InternalDocumentId::new(table, InternalId::try_from(document_id)?);
            let json_value = json_value.ok_or_else(|| {
                anyhow::anyhow!("Index reference to deleted document {:?} {:?}", key, ts)
            })?;
            let json_value: serde_json::Value = serde_json::from_str(&json_value)?;
            let value: ConvexValue = json_value.try_into()?;
            let document = ResolvedDocument::from_database(tablet_id, value)?;
            triples.push(Ok((
                key,
                LatestDocument {
                    ts,
                    value: document,
                    prev_ts,
                },
            )));
        }
        Ok(triples)
    }

    fn _get_persistence_global(
        &self,
        key: PersistenceGlobalKey,
    ) -> anyhow::Result<Option<JsonValue>> {
        let connection = &self.inner.lock().connection;
        let mut stmt = connection.prepare(GET_PERSISTENCE_GLOBAL)?;
        let key = String::from(key);
        let params: Vec<&dyn ToSql> = vec![&key];
        let mut row_iter = stmt.query_map(&params[..], |row| {
            let json_value_str: String = row.get(0)?;
            Ok(json_value_str)
        })?;
        row_iter
            .next()
            .map(|json_value_str| {
                let json_value_str = json_value_str?;
                let mut json_deserializer = serde_json::Deserializer::from_str(&json_value_str);
                // XXX: this is bad, but shapes can get much more nested than convex values
                json_deserializer.disable_recursion_limit();
                let json_value = JsonValue::deserialize(&mut json_deserializer)
                    .with_context(|| format!("Invalid JSON at persistence key {key:?}"))?;
                json_deserializer.end()?;
                Ok(json_value)
            })
            .transpose()
    }
}

#[async_trait]
impl IndexRowPersistence for SqlitePersistence {
    async fn delete_index_rows(&self, expired_rows: Vec<IndexEntry>) -> anyhow::Result<usize> {
        let mut inner = self.inner.lock();
        let tx = inner.connection.transaction()?;
        let mut delete_index_query = tx.prepare_cached(DELETE_INDEX)?;
        let mut count_deleted = 0;

        for IndexEntry {
            index_id,
            key_prefix,
            ts,
            ..
        } in expired_rows
        {
            count_deleted +=
                delete_index_query
                    .execute(params![&index_id.0[..], &i64::from(ts), key_prefix,])?;
        }
        drop(delete_index_query);
        tx.commit()?;
        Ok(count_deleted)
    }
}

#[async_trait]
impl Persistence for SqlitePersistence {
    fn is_fresh(&self) -> bool {
        self.inner.lock().newly_created
    }

    fn reader(&self) -> Arc<dyn PersistenceReader> {
        Arc::new(Self {
            inner: self.inner.clone(),
        })
    }

    async fn write<'a>(
        &self,
        documents: &'a [DocumentLogEntry],
        indexes: &'a [PersistenceIndexEntry],
        conflict_strategy: ConflictStrategy,
    ) -> anyhow::Result<()> {
        let mut inner = self.inner.lock();
        let tx = inner.connection.transaction()?;
        let mut insert_document_query = match conflict_strategy {
            ConflictStrategy::Error => tx.prepare_cached(INSERT_DOCUMENT)?,
            ConflictStrategy::Overwrite => tx.prepare_cached(INSERT_OVERWRITE_DOCUMENT)?,
        };

        for update in documents {
            let (json_value, deleted) = if let Some(document) = &update.value {
                assert_eq!(update.id, document.id_with_table_id());
                let json_value = document.value().json_serialize()?;
                (Some(json_value), 0)
            } else {
                (None, 1)
            };
            insert_document_query.execute(params![
                &update.id.internal_id()[..],
                &i64::from(update.ts),
                &update.id.table().0[..],
                &json_value,
                &deleted,
                &update.prev_ts.map(i64::from),
            ])?;
        }
        drop(insert_document_query);

        let mut insert_index_query = if conflict_strategy == ConflictStrategy::Overwrite {
            tx.prepare_cached(INSERT_OVERWRITE_INDEX)?
        } else {
            tx.prepare_cached(INSERT_INDEX)?
        };
        for update in indexes {
            let index_id = update.index.id();
            let key: &[u8] = &update.key.0;
            match update.value {
                None => {
                    insert_index_query.execute(params![
                        &index_id.0[..],
                        &i64::from(update.ts),
                        key,
                        &1,
                        &Null,
                        &Null,
                    ])?;
                },
                Some(doc_id) => {
                    insert_index_query.execute(params![
                        &index_id.0[..],
                        &i64::from(update.ts),
                        key,
                        &0,
                        &doc_id.table().0[..],
                        &doc_id.internal_id()[..],
                    ])?;
                },
            };
        }
        drop(insert_index_query);

        tx.commit()?;
        Ok(())
    }

    async fn write_persistence_global(
        &self,
        key: PersistenceGlobalKey,
        value: JsonValue,
    ) -> anyhow::Result<()> {
        let mut inner = self.inner.lock();
        let tx = inner.connection.transaction()?;
        let mut write_query = tx.prepare_cached(WRITE_PERSISTENCE_GLOBAL)?;
        let json_value = serde_json::to_string(&value)?;
        write_query.execute(params![&String::from(key), &json_value])?;
        drop(write_query);
        tx.commit()?;
        Ok(())
    }

    async fn has_index_entries(&self) -> anyhow::Result<bool> {
        let connection = &self.inner.lock().connection;
        Ok(connection.prepare(HAS_INDEX_ENTRIES)?.exists([])?)
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
        let mut inner = self.inner.lock();
        let tx = inner.connection.transaction()?;
        let mut delete_document_query = tx.prepare_cached(DELETE_DOCUMENT)?;
        let mut count_deleted = 0;

        for (ts, internal_id) in documents {
            let tablet_id: TabletId = internal_id.table();
            let id = internal_id.internal_id();
            count_deleted += delete_document_query.execute(params![
                &tablet_id.0[..],
                &id[..],
                &i64::from(ts),
            ])?;
        }
        drop(delete_document_query);
        tx.commit()?;
        Ok(count_deleted)
    }

    async fn delete_tablet_documents(
        &self,
        tablet_id: TabletId,
        chunk_size: usize,
    ) -> anyhow::Result<usize> {
        let mut inner = self.inner.lock();
        let tx = inner.connection.transaction()?;
        let mut delete_table_documents_query = tx.prepare_cached(DELETE_TABLE_DOCUMENTS)?;
        let count_deleted = delete_table_documents_query.execute(params![
            &tablet_id.0[..],
            &tablet_id.0[..],
            i64::try_from(chunk_size)?,
        ])?;
        drop(delete_table_documents_query);
        tx.commit()?;
        Ok(count_deleted)
    }
}

impl SqlitePersistence {
    /// Read the document log in `range`, restricted to `tablet_id` if given.
    ///
    /// Rows are fetched in pages of `page_size`, using keyset pagination over
    /// the (ts, table_id, id) primary key, so a single page is held in memory
    /// at a time and the connection lock is released between pages.
    #[allow(clippy::needless_lifetimes)]
    #[try_stream(ok = DocumentLogEntry, error = anyhow::Error)]
    async fn stream_document_log_pages(
        &self,
        tablet_id: Option<TabletId>,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) {
        // A zero page size would make the termination check below never
        // trigger while SQL still returns rows under LIMIT 0.
        let page_size = page_size.max(1) as usize;
        let min_ts = range.min_timestamp_inclusive();
        let mut cursor: Option<DocumentLogCursor> = None;
        loop {
            let page = {
                let connection = &self.inner.lock().connection;
                let query = load_docs(
                    range,
                    order,
                    tablet_id.is_some(),
                    cursor.is_some(),
                    page_size,
                );
                let mut stmt = connection.prepare_cached(&query)?;

                let mut params: Vec<Box<dyn ToSql>> = vec![];
                if let Some(tablet_id) = tablet_id {
                    params.push(Box::new(tablet_id.0.to_vec()));
                }
                if let Some(cursor) = &cursor {
                    params.push(Box::new(cursor.ts));
                    params.push(Box::new(cursor.table_id.clone()));
                    params.push(Box::new(cursor.id.clone()));
                }

                let mut page = vec![];
                for row in
                    stmt.query_map(rusqlite::params_from_iter(params.iter()), load_document_row)?
                {
                    page.push(row_to_document(row)?);
                }
                page
            };

            // The connection was released and re-acquired between pages, so
            // retention may have advanced; re-validate the snapshot to report
            // an out-of-retention error rather than silently omitting rows.
            retention_validator
                .validate_document_snapshot(min_ts)
                .await?;

            let page_len = page.len();
            cursor = page.last().map(|(document_id, ts, ..)| DocumentLogCursor {
                ts: i64::from(*ts),
                table_id: document_id.table().0.to_vec(),
                id: document_id.internal_id().to_vec(),
            });
            for (document_id, ts, document, prev_ts) in page {
                yield DocumentLogEntry {
                    ts,
                    id: document_id,
                    value: document,
                    prev_ts,
                };
            }
            if page_len < page_size {
                break;
            }
        }
    }

    fn stream_document_log(
        &self,
        tablet_id: Option<TabletId>,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentStream<'_> {
        // The caller isn't async so we have to validate snapshot as part of the
        // stream.
        let validate = self.validate_document_snapshot(
            range.min_timestamp_inclusive(),
            retention_validator.clone(),
        );
        validate
            .chain(
                self.stream_document_log_pages(
                    tablet_id,
                    range,
                    order,
                    page_size,
                    retention_validator,
                )
                .cooperative(),
            )
            .boxed()
    }
}

#[async_trait]
impl PersistenceReader for SqlitePersistence {
    fn load_documents(
        &self,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentStream<'_> {
        self.stream_document_log(None, range, order, page_size, retention_validator)
    }

    fn load_documents_from_table(
        &self,
        tablet_id: TabletId,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentStream<'_> {
        self.stream_document_log(
            Some(tablet_id),
            range,
            order,
            page_size,
            retention_validator,
        )
    }

    async fn previous_revisions(
        &self,
        ids: BTreeSet<(InternalDocumentId, Timestamp)>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<(InternalDocumentId, Timestamp), DocumentLogEntry>> {
        let mut out = BTreeMap::new();
        let mut min_ts = Timestamp::MAX;
        {
            let inner = self.inner.lock();
            for (id, ts) in ids {
                min_ts = cmp::min(ts, min_ts);
                let mut stmt = inner.connection.prepare(PREV_REV_QUERY)?;
                let internal_id = id.internal_id();
                let params = params![&id.table().0[..], &internal_id[..], &i64::from(ts)];
                let mut row_iter = stmt.query_map(params, load_document_row)?;
                if let Some(row) = row_iter.next() {
                    let (document_id, prev_ts, document, prev_prev_ts) = row_to_document(row)?;
                    out.insert(
                        (document_id, ts),
                        DocumentLogEntry {
                            ts: prev_ts,
                            id: document_id,
                            value: document,
                            prev_ts: prev_prev_ts,
                        },
                    );
                }
            }
        }
        retention_validator
            .validate_document_snapshot(min_ts)
            .await?;
        Ok(out)
    }

    async fn previous_revisions_of_documents(
        &self,
        ids: BTreeSet<DocumentPrevTsQuery>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> anyhow::Result<BTreeMap<DocumentPrevTsQuery, DocumentLogEntry>> {
        // Validate retention for all queried timestamps first
        let min_ts = ids.iter().map(|DocumentPrevTsQuery { ts, .. }| *ts).min();

        let mut out = BTreeMap::new();
        {
            let inner = self.inner.lock();
            for DocumentPrevTsQuery { id, ts, prev_ts } in ids {
                let mut stmt = inner.connection.prepare(EXACT_REV_QUERY)?;
                let internal_id = id.internal_id();
                let params = params![&id.table().0[..], &internal_id[..], &i64::from(prev_ts)];
                let mut row_iter = stmt.query_map(params, load_document_row)?;
                if let Some(row) = row_iter.next() {
                    let (document_id, prev_ts, document, prev_prev_ts) = row_to_document(row)?;
                    out.insert(
                        DocumentPrevTsQuery {
                            id: document_id,
                            ts,
                            prev_ts,
                        },
                        DocumentLogEntry {
                            ts: prev_ts,
                            id: document_id,
                            value: document,
                            prev_ts: prev_prev_ts,
                        },
                    );
                }
            }
        }
        if let Some(min_ts) = min_ts {
            retention_validator
                .validate_document_snapshot(min_ts)
                .await?;
        }
        Ok(out)
    }

    fn index_scan(
        &self,
        index: IndexRef,
        tablet_id: TabletId,
        read_timestamp: Timestamp,
        interval: &Interval,
        order: Order,
        _size_hint: usize,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> IndexStream<'_> {
        let triples =
            self._index_scan_inner(index.id(), tablet_id, read_timestamp, interval, order);
        // index_scan isn't async so we have to validate snapshot as part of the stream.
        let validate = self.validate_snapshot(read_timestamp, retention_validator);
        match triples {
            Ok(s) => (validate.chain(stream::iter(s))).boxed(),
            Err(e) => stream::once(async { Err(e) }).boxed(),
        }
    }

    async fn get_persistence_global(
        &self,
        key: PersistenceGlobalKey,
    ) -> anyhow::Result<Option<JsonValue>> {
        self._get_persistence_global(key)
    }

    fn version(&self) -> PersistenceVersion {
        PersistenceVersion::V5
    }
}

const DOCUMENTS_INIT: &str = r#"
CREATE TABLE IF NOT EXISTS documents (
    id BLOB NOT NULL,
    ts INTEGER NOT NULL,

    table_id BLOB NOT NULL,

    json_value TEXT NULL,
    deleted INTEGER NOT NULL,

    prev_ts INTEGER,

    PRIMARY KEY (ts, table_id, id)
);
CREATE INDEX IF NOT EXISTS documents_by_table_and_id ON documents (table_id, id, ts);
"#;

const INDEXES_INIT: &str = r#"
CREATE TABLE IF NOT EXISTS indexes (
    index_id BLOB NOT NULL,
    ts INTEGER NOT NULL,

    key BLOB NOT NULL,

    deleted INTEGER NOT NULL,

    table_id BLOB NULL,
    document_id BLOB NULL,

    PRIMARY KEY (index_id, key, ts)
);
"#;

const PERSISTENCE_GLOBALS_INIT: &str = r#"
CREATE TABLE IF NOT EXISTS persistence_globals (
    key TEXT NOT NULL,
    json_value TEXT NOT NULL,

    PRIMARY KEY (key)
);
"#;

fn row_to_document(
    row: rusqlite::Result<(Vec<u8>, i64, Vec<u8>, Option<String>, bool, Option<i64>)>,
) -> anyhow::Result<(
    InternalDocumentId,
    Timestamp,
    Option<ResolvedDocument>,
    Option<Timestamp>,
)> {
    let (id, prev_ts, table, json_value, deleted, prev_prev_ts) = row?;
    let id = InternalId::try_from(id)?;
    let prev_ts = Timestamp::try_from(prev_ts)?;
    let table = TabletId(table.try_into()?);
    let document_id = InternalDocumentId::new(table, id);
    let document = if !deleted {
        let json_value = json_value
            .ok_or_else(|| anyhow::anyhow!("Unexpected NULL json_value at {} {}", id, prev_ts))?;
        let json_value: serde_json::Value = serde_json::from_str(&json_value)?;
        let value: ConvexValue = json_value.try_into()?;
        Some(ResolvedDocument::from_database(table, value)?)
    } else {
        None
    };
    let prev_prev_ts = prev_prev_ts.map(Timestamp::try_from).transpose()?;
    Ok((document_id, prev_ts, document, prev_prev_ts))
}

/// Position in the `(ts, table_id, id)` ordering to resume the next page from.
struct DocumentLogCursor {
    ts: i64,
    table_id: Vec<u8>,
    id: Vec<u8>,
}

fn load_docs(
    range: TimestampRange,
    order: Order,
    tablet_filter: bool,
    has_cursor: bool,
    page_size: usize,
) -> String {
    let direction = match order {
        Order::Asc => "ASC",
        Order::Desc => "DESC",
    };
    let mut where_clause = String::new();
    // With a cursor, the (ts, table_id, id) comparison bounds the scan on the
    // ordering side already. Keeping the redundant range bound on that side
    // would make SQLite re-scan all preceding rows on every page.
    if !(has_cursor && order == Order::Asc) {
        where_clause += &format!("AND ts >= {} ", range.min_timestamp_inclusive());
    }
    if !(has_cursor && order == Order::Desc) {
        where_clause += &format!("AND ts < {} ", range.max_timestamp_exclusive());
    }
    if tablet_filter {
        // With a cursor, force the primary-key index so the keyset comparison
        // seeks directly; the (table_id, id, ts) index would otherwise sort the
        // whole table on every page.
        where_clause += if has_cursor {
            "AND +table_id = ?1 "
        } else {
            "AND table_id = ?1 "
        };
    }
    if has_cursor {
        // Params ?1..?3 (or ?2..?4 with a tablet filter) hold the previous
        // page's last (ts, table_id, id).
        let first = if tablet_filter { 2 } else { 1 };
        let cmp = match order {
            Order::Asc => ">",
            Order::Desc => "<",
        };
        where_clause += &format!(
            "AND (ts, table_id, id) {cmp} (?{first}, ?{second}, ?{third}) ",
            second = first + 1,
            third = first + 2,
        );
    }
    format!(
        r#"
SELECT id, ts, table_id, json_value, deleted, prev_ts
FROM documents
WHERE TRUE
{}
ORDER BY ts {}, table_id {}, id {}
LIMIT {}
"#,
        where_clause, direction, direction, direction, page_size,
    )
}

fn load_document_row(
    row: &Row<'_>,
) -> rusqlite::Result<(Vec<u8>, i64, Vec<u8>, Option<String>, bool, Option<i64>)> {
    let id = row.get::<_, Vec<u8>>(0)?;
    let ts = row.get::<_, i64>(1)?;
    let table: Vec<u8> = row.get(2)?;
    let json_value: Option<String> = row.get(3)?;
    let deleted = row.get::<_, u32>(4)? != 0;
    let prev_ts: Option<i64> = row.get(5)?;
    Ok((id, ts, table, json_value, deleted, prev_ts))
}

const GET_PERSISTENCE_GLOBAL: &str = "SELECT json_value FROM persistence_globals WHERE key = ?";

const INSERT_DOCUMENT: &str = "INSERT INTO documents (id, ts, table_id, json_value, deleted, \
                               prev_ts) VALUES (?, ?, ?, ?, ?, ?)";
const INSERT_OVERWRITE_DOCUMENT: &str = "INSERT OR REPLACE INTO documents (id, ts, table_id, \
                                         json_value, deleted, prev_ts) VALUES (?, ?, ?, ?, ?, ?)";
const INSERT_INDEX: &str = "INSERT INTO indexes VALUES (?, ?, ?, ?, ?, ?)";
const INSERT_OVERWRITE_INDEX: &str = "INSERT OR REPLACE INTO indexes VALUES (?, ?, ?, ?, ?, ?)";
const WRITE_PERSISTENCE_GLOBAL: &str = "INSERT OR REPLACE INTO persistence_globals VALUES (?, ?)";

const HAS_INDEX_ENTRIES: &str = "SELECT 1 FROM indexes LIMIT 1";

const DELETE_INDEX: &str = "DELETE FROM indexes WHERE index_id = ? AND ts <= ? AND key = ?";

const DELETE_DOCUMENT: &str = "DELETE FROM documents WHERE table_id = ? AND id = ? AND ts <= ?";

const DELETE_TABLE_DOCUMENTS: &str = "DELETE FROM documents WHERE table_id = ? AND id IN (SELECT \
                                      id FROM documents WHERE table_id = ? LIMIT ?)";

const PREV_REV_QUERY: &str = r#"
SELECT id, ts, table_id, json_value, deleted, prev_ts
FROM documents
WHERE
    table_id = $1 AND
    id = $2 AND
    ts < $3
ORDER BY ts desc
LIMIT 1
"#;

const EXACT_REV_QUERY: &str = r#"
SELECT id, ts, table_id, json_value, deleted, prev_ts
FROM documents
WHERE
    table_id = $1 AND
    id = $2 AND
    ts = $3
ORDER BY ts ASC, table_id ASC, id ASC
"#;
