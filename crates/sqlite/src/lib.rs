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
use futures::StreamExt;
use futures_async_stream::try_stream;
use parking_lot::Mutex;
use rusqlite::{
    params,
    types::Null,
    Connection,
    OptionalExtension as _,
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

    /// Reads at most `batch_size` keys of an index scan, resuming strictly
    /// after `cursor`. Tombstoned keys are returned as `(key, None)` so they
    /// advance the cursor without being yielded.
    fn _index_scan_page(
        &self,
        index_id: IndexId,
        tablet_id: TabletId,
        read_timestamp: Timestamp,
        interval: &Interval,
        order: Order,
        batch_size: usize,
        cursor: Option<&IndexKeyBytes>,
    ) -> anyhow::Result<Vec<(IndexKeyBytes, Option<LatestDocument>)>> {
        let interval = interval.clone();
        let index_id = &index_id.0[..];
        let read_timestamp: i64 = read_timestamp.into();

        // Placeholders are numbered (`?N`) because `?2` is referenced after
        // the key bounds, and SQLite numbers `$name` placeholders by first
        // appearance.
        let mut params = params![index_id, read_timestamp].to_vec();

        // The cursor replaces the interval's bound on its side: with both,
        // SQLite may seek by the interval's bound and re-read every earlier
        // page.
        let cursor_bytes = cursor.map(|key| &key.0[..]);

        let StartIncluded(ref start) = interval.start;
        let (lower_bytes, lower_op) = match (order, cursor_bytes) {
            (Order::Asc, Some(cursor_bytes)) => (cursor_bytes, ">"),
            _ => (&start[..], ">="),
        };

        params.push(&lower_bytes);
        let lower = format!(" AND B.key {lower_op} ?{}", params.len());

        let upper_bytes = match (order, cursor_bytes, &interval.end) {
            (Order::Desc, Some(cursor_bytes), _) => Some(cursor_bytes),
            (_, _, End::Excluded(t)) => Some(&t[..]),
            (_, _, End::Unbounded) => None,
        };
        let upper = match upper_bytes {
            Some(ref t) => {
                params.push(t);
                format!(" AND B.key < ?{}", params.len())
            },
            None => "".to_owned(),
        };

        let order = match order {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        };
        // Walks the `(index_id, key, ts)` primary key in scan order, keeping
        // each key's newest version at or before the read timestamp, so
        // `LIMIT` stops the walk without a sort. Tombstones are selected, not
        // filtered, so they count toward `LIMIT` and a short page means the
        // interval is exhausted.
        let query = format!(
            r#"
SELECT B.key, B.ts, B.document_id, C.table_id, C.json_value, C.prev_ts, B.deleted
FROM indexes B
LEFT JOIN documents C
ON B.ts = C.ts
AND B.table_id = C.table_id
AND B.document_id = C.id
WHERE B.index_id = ?1{lower}{upper}
AND B.ts = (
    SELECT MAX(X.ts)
    FROM indexes X
    WHERE X.index_id = B.index_id AND X.key = B.key AND X.ts <= ?2
)
ORDER BY B.key {order}
LIMIT {batch_size}
"#,
        );

        let connection = &self.inner.lock().connection;
        let mut stmt = connection.prepare(&query)?;
        let row_iter = stmt.query_map(&params[..], |row| {
            let key = IndexKeyBytes(row.get::<_, Vec<u8>>(0)?);
            let ts = Timestamp::try_from(row.get::<_, i64>(1)?).expect("timestamp out of bounds");
            let document_id: Option<Vec<u8>> = row.get(2)?;
            let table: Option<Vec<u8>> = row.get(3)?;
            let json_value: Option<String> = row.get(4)?;
            let prev_ts: Option<Timestamp> = row
                .get::<_, Option<i64>>(5)?
                .map(|ts| Timestamp::try_from(ts).expect("prev_ts out of bounds"));
            let deleted = row.get::<_, u32>(6)? != 0;

            Ok((key, ts, document_id, table, json_value, prev_ts, deleted))
        })?;
        let mut page = vec![];
        for row in row_iter {
            let (key, ts, document_id, table, json_value, prev_ts, deleted) = row?;
            if deleted {
                page.push((key, None));
                continue;
            }
            let table = table.ok_or_else(|| {
                anyhow::anyhow!("Dangling index reference for {:?} {:?}", key, ts)
            })?;
            let table = TabletId(table.try_into()?);
            let document_id = document_id.ok_or_else(|| {
                anyhow::anyhow!("Dangling index reference for {:?} {:?}", key, ts)
            })?;
            let _document_id = InternalDocumentId::new(table, InternalId::try_from(document_id)?);
            let json_value = json_value.ok_or_else(|| {
                anyhow::anyhow!("Index reference to deleted document {:?} {:?}", key, ts)
            })?;
            let json_value: serde_json::Value = serde_json::from_str(&json_value)?;
            let value: ConvexValue = json_value.try_into()?;
            let document = ResolvedDocument::from_database(tablet_id, value)?;
            page.push((
                key,
                Some(LatestDocument {
                    ts,
                    value: document,
                    prev_ts,
                }),
            ));
        }
        Ok(page)
    }

    /// Streams an index scan one page at a time, validating retention after
    /// each page is read and before any of its rows are yielded, as the
    /// Postgres reader does.
    #[try_stream(ok = (IndexKeyBytes, LatestDocument), error = anyhow::Error)]
    async fn _index_scan_paginated(
        &self,
        index_id: IndexId,
        tablet_id: TabletId,
        read_timestamp: Timestamp,
        interval: Interval,
        order: Order,
        batch_size: usize,
        retention_validator: Arc<dyn RetentionValidator>,
    ) {
        let mut cursor: Option<IndexKeyBytes> = None;
        loop {
            let page = self._index_scan_page(
                index_id,
                tablet_id,
                read_timestamp,
                &interval,
                order,
                batch_size,
                cursor.as_ref(),
            )?;
            let page_len = page.len();
            retention_validator
                .validate_snapshot(read_timestamp)
                .await?;
            // The next page resumes after the last key read, even a tombstone
            // that yields nothing.
            cursor = page.last().map(|(key, _)| key.clone());
            for (key, doc) in page {
                if let Some(doc) = doc {
                    yield (key, doc);
                }
            }
            if page_len < batch_size {
                break;
            }
        }
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

/// The last row of a document-log page. The next page starts strictly after
/// it in `(ts, table_id, id)` order.
struct DocumentLogCursor {
    ts: Timestamp,
    id: InternalDocumentId,
}

impl SqlitePersistence {
    /// Read bounded pages in primary-key order without retaining the history.
    /// Release the connection before validation or yielding: validation can
    /// read persistence itself, and consumers can pause the stream between
    /// entries.
    #[try_stream(ok = DocumentLogEntry, error = anyhow::Error)]
    async fn stream_document_log(
        &self,
        tablet_id: Option<TabletId>,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) {
        let page_size = page_size.clamp(1, 1000);
        let mut cursor: Option<DocumentLogCursor> = None;
        loop {
            let entries = {
                let inner = self.inner.lock();
                let query = load_docs(
                    range,
                    order,
                    tablet_id.is_some(),
                    cursor.as_ref(),
                    page_size,
                );
                let mut stmt = inner.connection.prepare_cached(&query)?;
                let tablet_bytes = tablet_id.map(|tablet| tablet.0);
                let tablet_slice = tablet_bytes.as_ref().map(|tablet| &tablet[..]);
                let cursor_ts = cursor.as_ref().map(|cursor| i64::from(cursor.ts));
                let cursor_table = cursor.as_ref().map(|cursor| cursor.id.table().0);
                let cursor_table = cursor_table.as_ref().map(|table| &table[..]);
                let cursor_id = cursor.as_ref().map(|cursor| cursor.id.internal_id());
                let cursor_id = cursor_id.as_ref().map(|id| &id[..]);
                let mut parameters: Vec<&dyn ToSql> = vec![];
                if let Some(ref tablet) = tablet_slice {
                    parameters.push(tablet);
                }
                if cursor.is_some() {
                    parameters.push(&cursor_ts);
                    if tablet_id.is_none() {
                        parameters.push(&cursor_table);
                    }
                    parameters.push(&cursor_id);
                }
                let mut entries = Vec::new();
                for row in stmt.query_map(parameters.as_slice(), load_document_row)? {
                    let (id, ts, value, prev_ts) = row_to_document(row)?;
                    entries.push(DocumentLogEntry {
                        ts,
                        id,
                        value,
                        prev_ts,
                    });
                }
                entries
            };
            retention_validator
                .validate_document_snapshot(range.min_timestamp_inclusive())
                .await?;
            let done = entries.len() < page_size as usize;
            if let Some(last) = entries.last() {
                cursor = Some(DocumentLogCursor {
                    ts: last.ts,
                    id: last.id,
                });
            }
            for entry in entries {
                yield entry;
            }
            if done {
                break;
            }
        }
    }
}

#[async_trait]
impl PersistenceReader for SqlitePersistence {
    async fn max_ts(&self) -> anyhow::Result<Option<Timestamp>> {
        // A timestamp lookup does not need to deserialize a document payload.
        // Use a covering primary-key seek and preserve the repeatable timestamp.
        let max_committed_ts = {
            let inner = self.inner.lock();
            inner
                .connection
                .query_row(
                    "SELECT ts FROM documents ORDER BY ts DESC LIMIT 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
        }
        .map(Timestamp::try_from)
        .transpose()?;
        let max_repeatable_ts = self
            .get_persistence_global(PersistenceGlobalKey::MaxRepeatableTimestamp)
            .await?
            .map(Timestamp::try_from)
            .transpose()?;
        Ok(cmp::max(max_committed_ts, max_repeatable_ts))
    }

    fn load_documents(
        &self,
        range: TimestampRange,
        order: Order,
        page_size: u32,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> DocumentStream<'_> {
        self.stream_document_log(None, range, order, page_size, retention_validator)
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
        self.stream_document_log(
            Some(tablet_id),
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
        size_hint: usize,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> IndexStream<'_> {
        let batch_size = size_hint.clamp(1, 5000);
        self._index_scan_paginated(
            index.id(),
            tablet_id,
            read_timestamp,
            interval.clone(),
            order,
            batch_size,
            retention_validator,
        )
        .boxed()
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
-- Table-scoped log pages need the same timestamp ordering as the global log.
CREATE INDEX IF NOT EXISTS documents_by_table_and_ts ON documents (table_id, ts, id);
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

fn load_docs(
    range: TimestampRange,
    order: Order,
    tablet_filter: bool,
    cursor: Option<&DocumentLogCursor>,
    page_size: u32,
) -> String {
    let columns = if tablet_filter {
        "ts, id"
    } else {
        "ts, table_id, id"
    };
    // SQLite assigns $name parameters by appearance; ?N preserves bind positions.
    let parameters = if tablet_filter {
        "?2, ?3"
    } else {
        "?1, ?2, ?3"
    };
    let lower = range.min_timestamp_inclusive();
    let upper = range.max_timestamp_exclusive();
    let bounds = match (cursor, order) {
        (Some(_), Order::Asc) => format!("({columns}) > ({parameters}) AND ts < {upper}"),
        (Some(_), Order::Desc) => format!("ts >= {lower} AND ({columns}) < ({parameters})"),
        (None, _) => format!("ts >= {lower} AND ts < {upper}"),
    };
    let direction = match order {
        Order::Asc => "ASC",
        Order::Desc => "DESC",
    };
    let ordering = if tablet_filter {
        format!("ts {direction}, id {direction}")
    } else {
        format!("ts {direction}, table_id {direction}, id {direction}")
    };
    let filter = if tablet_filter {
        "AND table_id = ?1"
    } else {
        ""
    };
    format!(
        "SELECT id, ts, table_id, json_value, deleted, prev_ts FROM documents WHERE {bounds} \
         {filter} ORDER BY {ordering} LIMIT {page_size}"
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
