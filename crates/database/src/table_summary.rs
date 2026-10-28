use std::{
    cmp,
    collections::BTreeMap,
    fmt,
    sync::Arc,
};

use anyhow::Context as _;
use common::{
    bootstrap_model::tables::{
        TableMetadata,
        TableState,
    },
    document::{
        ParseDocument,
        ParsedDocument,
    },
    json::JsonForm,
    persistence::{
        new_static_repeatable_recent,
        LatestDocument,
        Persistence,
        PersistenceGlobalKey,
        PersistenceReader,
        RepeatablePersistence,
        RetentionValidator,
        TimestampRange,
    },
    persistence_helpers::{
        DocumentRevision,
        RevisionPair,
    },
    query::Order,
    runtime::Runtime,
    types::{
        IndexRef,
        RepeatableReason,
        RepeatableTimestamp,
        Timestamp,
    },
    value::{
        ConvexObject,
        JsonInteger,
        NamespacedTableMapping,
        Size,
        TableMapping,
        TableName,
        TabletId,
    },
};
use errors::ErrorMetadata;
use futures::{
    Stream,
    TryStreamExt,
};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use shape_inference::{
    CountedShape,
    ProdConfig,
    Shape,
    ShapeEnum,
};

use crate::{
    bootstrap_model::defaults::BootstrapTableIds,
    metrics,
    persistence_helpers::stream_transactions,
    Database,
    DatabaseSnapshot,
    TableIterator,
};

/// Document count and byte size of the documents in a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableCount {
    num_values: u64,
    total_size: u64,
}

impl TableCount {
    pub fn empty() -> Self {
        Self {
            num_values: 0,
            total_size: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.num_values == 0 && self.total_size == 0
    }

    pub fn num_values(&self) -> u64 {
        self.num_values
    }

    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    pub fn insert(&mut self, object: &ConvexObject) {
        self.num_values += 1;
        self.total_size += object.size() as u64;
    }

    pub fn remove(&mut self, object: &ConvexObject) -> anyhow::Result<()> {
        self.num_values = self
            .num_values
            .checked_sub(1)
            .context("num_values went negative?")?;
        self.total_size = self
            .total_size
            .checked_sub(object.size() as u64)
            .context("total_size went negative?")?;
        Ok(())
    }
}

/// Inferred shape of the documents in a table.
/// TODO: Add `ts` that the shape is valid at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableShape {
    inferred_type: CountedShape<ProdConfig>,
}

impl fmt::Display for TableShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inferred_type)
    }
}

impl TableShape {
    pub fn empty() -> Self {
        Self {
            inferred_type: Shape::empty(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.inferred_type.is_empty()
    }

    pub fn inferred_type(&self) -> &CountedShape<ProdConfig> {
        &self.inferred_type
    }

    pub fn insert(&mut self, object: &ConvexObject) {
        self.inferred_type = self.inferred_type.insert(object);
    }

    pub fn remove(&mut self, object: &ConvexObject) -> anyhow::Result<()> {
        self.inferred_type = self.inferred_type.remove(object)?;
        Ok(())
    }

    /// Drop the tracked shape while preserving the document count, replacing it
    /// with an `Unknown` shape of the given size.
    pub fn reset(&mut self, num_values: u64) {
        self.inferred_type = CountedShape::new(ShapeEnum::Unknown, num_values);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSummary {
    count: TableCount,
    shape: TableShape,
}

impl fmt::Display for TableSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TableSummary {{ inferred_type: {}, total_size: {} }}",
            self.shape.inferred_type, self.count.total_size
        )
    }
}

impl TableSummary {
    pub fn empty() -> Self {
        Self {
            count: TableCount::empty(),
            shape: TableShape::empty(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.count.is_empty() && self.shape.is_empty()
    }

    pub fn count(&self) -> &TableCount {
        &self.count
    }

    pub fn shape(&self) -> &TableShape {
        &self.shape
    }

    pub fn into_shape(self) -> TableShape {
        self.shape
    }

    pub fn total_size(&self) -> u64 {
        self.count.total_size()
    }

    pub fn num_values(&self) -> u64 {
        self.count.num_values()
    }

    pub fn inferred_type(&self) -> &CountedShape<ProdConfig> {
        self.shape.inferred_type()
    }

    pub fn insert(&mut self, object: &ConvexObject) {
        self.count.insert(object);
        self.shape.insert(object);
    }

    pub fn remove(&mut self, object: &ConvexObject) -> anyhow::Result<()> {
        self.shape.remove(object)?;
        self.count.remove(object)?;
        Ok(())
    }

    pub fn reset_shape(&mut self) {
        self.shape.reset(self.count.num_values());
    }

    pub fn persistence_key() -> PersistenceGlobalKey {
        PersistenceGlobalKey::TableSummary
    }
}

impl From<&TableSummary> for JsonValue {
    fn from(summary: &TableSummary) -> Self {
        let mut object = serde_json::Map::new();
        object.insert(
            "totalSize".into(),
            JsonInteger(summary.count.total_size as i64).into(),
        );
        object.insert(
            "inferredTypeWithOptionalFields".into(),
            JsonValue::from(&summary.shape.inferred_type),
        );
        object.into()
    }
}

impl TryFrom<JsonValue> for TableSummary {
    type Error = anyhow::Error;

    fn try_from(json_value: JsonValue) -> anyhow::Result<Self> {
        match json_value {
            JsonValue::Object(mut v) => {
                let total_size = match v.remove("totalSize") {
                    Some(JsonValue::String(s)) => JsonInteger::decode(s)? as u64,
                    _ => anyhow::bail!("Invalid totalSize"),
                };
                anyhow::ensure!(total_size >= 0);
                let inferred_type = match v.remove("inferredTypeWithOptionalFields") {
                    Some(v) => CountedShape::<ProdConfig>::json_deserialize_value(v)?,
                    None => anyhow::bail!("Missing field inferredTypeWithOptionalFields"),
                };
                // The document count is derived from the shape's own counter on
                // load; from here on it is maintained authoritatively in
                // `TableCount`.
                let num_values = *inferred_type.num_values();
                Ok(TableSummary {
                    count: TableCount {
                        num_values,
                        total_size,
                    },
                    shape: TableShape { inferred_type },
                })
            },
            _ => anyhow::bail!("Wrong type of json value for TableSummaryJson"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSummarySnapshot {
    pub tables: BTreeMap<TabletId, TableSummary>,
    pub ts: Timestamp,
}

/// The inferred shapes of every table, as of a timestamp.
#[derive(Debug, Clone)]
pub struct TableShapes {
    pub tables: BTreeMap<TabletId, TableShape>,
    /// Timestamp of the snapshot these shapes are accurate for
    pub ts: Timestamp,
}

impl TableShapes {
    pub fn tablet_shape(&self, tablet: &TabletId) -> Option<&TableShape> {
        self.tables.get(tablet)
    }

    /// Shape for a table resolved against the given (possibly newer) table
    /// mapping. `None` when the table doesn't resolve or was created after
    /// `ts`, i.e. no shape has been computed for it yet.
    pub fn table_shape(
        &self,
        mapping: &NamespacedTableMapping,
        table: &TableName,
    ) -> Option<&TableShape> {
        let table_id = mapping.id(table).ok()?;
        self.tables.get(&table_id.tablet_id)
    }
}

impl From<TableSummarySnapshot> for TableShapes {
    fn from(snapshot: TableSummarySnapshot) -> Self {
        Self {
            tables: snapshot
                .tables
                .into_iter()
                .map(|(tablet, summary)| (tablet, summary.into_shape()))
                .collect(),
            ts: snapshot.ts,
        }
    }
}

/// A [`TableSummarySnapshot`] filtered to tables that exist: counts and shapes
/// together, exact at a single timestamp.
///
/// This is for offline tools (`db-info`, `db-verifier`) that load summaries
/// via `DatabaseSnapshot::load_table_summaries`. The live `Snapshot` holds
/// only counts (`TableCounts`); live shape readers use the asynchronously
/// published [`TableShapes`] instead.
#[derive(Clone)]
pub struct TableSummaries {
    pub tables: BTreeMap<TabletId, TableSummary>,
}

impl TableSummaries {
    pub fn new(
        TableSummarySnapshot { tables, ts: _ }: TableSummarySnapshot,
        table_mapping: &TableMapping,
    ) -> Self {
        Self {
            tables: tables
                .into_iter()
                .filter(|(table_id, _table_summary)| table_mapping.tablet_id_exists(*table_id))
                .collect(),
        }
    }

    pub fn tablet_summary(&self, table: &TabletId) -> TableSummary {
        self.tables
            .get(table)
            .cloned()
            .unwrap_or_else(TableSummary::empty)
    }
}

impl TableSummarySnapshot {
    pub async fn load(
        reader: &dyn PersistenceReader,
    ) -> anyhow::Result<Option<(Self, RepeatableTimestamp)>> {
        let Some(value) = reader
            .get_persistence_global(TableSummary::persistence_key())
            .await?
        else {
            return Ok(None);
        };

        let summary_snapshot = Self::try_from(value)?;
        let ts = RepeatableTimestamp::new_validated(
            summary_snapshot.ts,
            RepeatableReason::TableSummarySnapshot,
        );
        Ok(Some((summary_snapshot, ts)))
    }
}

impl From<&TableSummarySnapshot> for JsonValue {
    fn from(snapshot: &TableSummarySnapshot) -> Self {
        let mut object = serde_json::Map::new();
        object.insert(
            "tables".into(),
            snapshot
                .tables
                .iter()
                .map(|(k, v)| (k.to_string(), JsonValue::from(v)))
                .collect::<serde_json::Map<String, JsonValue>>()
                .into(),
        );
        object.insert("ts".into(), JsonInteger(snapshot.ts.into()).into());
        object.into()
    }
}

impl TryFrom<JsonValue> for TableSummarySnapshot {
    type Error = anyhow::Error;

    fn try_from(json_value: JsonValue) -> anyhow::Result<Self> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct TableSummarySnapshotJson {
            tables: serde_json::Map<String, JsonValue>,
            ts: String,
        }
        let snapshot: TableSummarySnapshotJson = serde_json::from_value(json_value)?;
        Ok(TableSummarySnapshot {
            tables: snapshot
                .tables
                .into_iter()
                .map(|(k, v)| {
                    let table_name = k.parse()?;
                    let summary = TableSummary::try_from(v)?;
                    Ok((table_name, summary))
                })
                .collect::<anyhow::Result<_>>()?,
            ts: JsonInteger::decode(snapshot.ts)?.try_into()?,
        })
    }
}

pub struct TableSummaryWriter<RT: Runtime> {
    persistence: Arc<dyn Persistence>,
    database: Database<RT>,
    retention_validator: Arc<dyn RetentionValidator>,
}

impl<RT: Runtime> TableSummaryWriter<RT> {
    pub fn new(
        runtime: RT,
        persistence: Arc<dyn Persistence>,
        database: Database<RT>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> Self {
        Self::new_with_config(runtime, persistence, database, retention_validator)
    }

    pub fn new_with_config(
        _runtime: RT,
        persistence: Arc<dyn Persistence>,
        database: Database<RT>,
        retention_validator: Arc<dyn RetentionValidator>,
    ) -> Self {
        Self {
            persistence,
            database,
            retention_validator,
        }
    }

    pub async fn collect_snapshot(
        // table_iterator, table_mapping, and by_id_indexes should all be
        // computed at the same snapshot.
        snapshot_ts: Timestamp,
        table_iterator: impl Fn() -> TableIterator<RT>,
        table_mapping: &TableMapping,
        by_id_indexes: &BTreeMap<TabletId, IndexRef>,
    ) -> anyhow::Result<TableSummarySnapshot> {
        let mut snapshot = BTreeMap::new();
        for (tablet_id, ..) in table_mapping.iter() {
            let by_id_index = by_id_indexes.get(&tablet_id).expect("by_id should exist");
            // table_iterator, table_mapping, and by_id_indexes should all be
            // computed at the same snapshot.
            let revision_stream =
                table_iterator().stream_documents_in_table(tablet_id, *by_id_index, None);
            let summary = Self::collect_table_revisions(revision_stream).await?;
            snapshot.insert(tablet_id, summary);
        }
        Ok(TableSummarySnapshot {
            tables: snapshot,
            ts: snapshot_ts,
        })
    }

    pub async fn collect_table_revisions(
        revision_stream: impl Stream<Item = anyhow::Result<LatestDocument>>,
    ) -> anyhow::Result<TableSummary> {
        futures::pin_mut!(revision_stream);
        let mut summary = TableSummary::empty();
        while let Some(rev) = revision_stream.try_next().await? {
            summary.insert(rev.value.value());
            let num_values = summary.num_values();
            if num_values % 10000 == 0 {
                tracing::info!("Collecting table summary with {num_values} documents")
            }
        }
        Ok(summary)
    }

    /// Compute a fresh table summary from the last persisted checkpoint,
    /// persist it as the new checkpoint, and publish its shapes to the
    /// [`Database`]'s in-memory store. Returns the checkpoint's timestamp.
    pub async fn checkpoint(&self) -> anyhow::Result<Timestamp> {
        let snapshot = self.compute_from_last_checkpoint().await?;
        let ts = snapshot.ts;
        tracing::info!("Writing table summary checkpoint at ts {ts}");
        write_snapshot(self.persistence.as_ref(), &snapshot).await?;
        self.database.publish_table_shapes(snapshot.into());
        Ok(ts)
    }

    pub async fn compute_from_last_checkpoint(&self) -> anyhow::Result<TableSummarySnapshot> {
        self.compute(BootstrapKind::FromCheckpoint).await
    }

    pub async fn compute_from_scratch(&self) -> anyhow::Result<TableSummarySnapshot> {
        self.compute(BootstrapKind::FromScratch).await
    }

    async fn compute(&self, bootstrap_kind: BootstrapKind) -> anyhow::Result<TableSummarySnapshot> {
        let reader = self.persistence.reader();
        let upper_bound = self.database.now_ts_for_reads();
        let (new_snapshot, _) = bootstrap(
            self.database.runtime().clone(),
            reader,
            self.retention_validator.clone(),
            upper_bound,
            bootstrap_kind,
        )
        .await?;
        Ok(new_snapshot)
    }
}

/// Persist a table summary checkpoint. On a live deployment, use
/// [`TableSummaryWriter::checkpoint`] instead so the checkpoint's shapes are
/// also published to the in-memory store; this free function exists for
/// tooling (e.g. cluster migration) that writes to a persistence with no
/// running `Database`.
pub async fn write_snapshot(
    persistence: &dyn Persistence,
    snapshot: &TableSummarySnapshot,
) -> anyhow::Result<()> {
    persistence
        .write_persistence_global(TableSummary::persistence_key(), JsonValue::from(snapshot))
        .await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapKind {
    FromScratch,
    FromCheckpoint,
}

pub fn table_summary_bootstrapping_error(msg: Option<&'static str>) -> anyhow::Error {
    anyhow::anyhow!(ErrorMetadata::feature_temporarily_unavailable(
        "TableSummariesUnavailable",
        msg.unwrap_or("Table summary unavailable (still bootstrapping)")
    ))
}

/// Compute a `TableSummarySnapshot` at a given timestamp.
/// If there is no stored snapshot or `from_scratch` is true, we recompute
/// by walking by_id indexes using TableIterator.
/// If the snapshot is <target_ts, we walk the documents log forwards and add
/// the documents to the snapshot.
/// If the snapshot is >target_ts, we walk the documents log backwards and
/// remove the documents from the snapshot.
///
/// Returns:
/// * The new table summary snapshot
/// * The number of log entries processed
pub async fn bootstrap<RT: Runtime>(
    runtime: RT,
    persistence: Arc<dyn PersistenceReader>,
    retention_validator: Arc<dyn RetentionValidator>,
    target_ts: RepeatableTimestamp,
    bootstrap_kind: BootstrapKind,
) -> anyhow::Result<(TableSummarySnapshot, usize)> {
    let _timer = metrics::bootstrap_table_summaries_timer();
    let stored_snapshot = match bootstrap_kind {
        BootstrapKind::FromScratch => None,
        BootstrapKind::FromCheckpoint => TableSummarySnapshot::load(persistence.as_ref()).await?,
    };
    let recent_ts = new_static_repeatable_recent(persistence.as_ref()).await?;
    let (table_mapping, _, index_registry, ..) =
        DatabaseSnapshot::<RT>::load_table_and_index_metadata(
            &RepeatablePersistence::new(
                persistence.clone(),
                recent_ts,
                retention_validator.clone(),
            )
            .read_snapshot(recent_ts)?,
        )
        .await?;
    let (base_snapshot, base_snapshot_ts) = match stored_snapshot {
        Some(base) => base,
        None => {
            let by_id_indexes = index_registry.by_id_indexes()?;
            let base_snapshot = TableSummaryWriter::<RT>::collect_snapshot(
                *recent_ts,
                || {
                    TableIterator::new(
                        runtime.clone(),
                        recent_ts,
                        persistence.clone(),
                        retention_validator.clone(),
                        1000,
                    )
                },
                &table_mapping,
                &by_id_indexes,
            )
            .await?;
            (base_snapshot, recent_ts)
        },
    };
    let bootstrap_tables = BootstrapTableIds::new(&table_mapping);
    let (range, order) = match base_snapshot_ts.cmp(&target_ts) {
        std::cmp::Ordering::Less => (
            TimestampRange::new(base_snapshot_ts.succ()?..=*target_ts),
            Order::Asc,
        ),
        std::cmp::Ordering::Equal => return Ok((base_snapshot, 0)),
        std::cmp::Ordering::Greater => (
            TimestampRange::new(target_ts.succ()?..=*base_snapshot_ts),
            Order::Desc,
        ),
    };
    let mut tables = base_snapshot.tables;
    let repeatable_persistence = RepeatablePersistence::new(
        persistence.clone(),
        cmp::max(base_snapshot_ts, target_ts),
        retention_validator.clone(),
    );
    let transaction_stream =
        stream_transactions(bootstrap_tables, &repeatable_persistence, range, order);
    futures::pin_mut!(transaction_stream);

    let mut num_added = 0;
    while let Some(transaction) = transaction_stream.try_next().await? {
        for revision_pair in transaction.revision_pairs {
            let revision_pair = match order {
                Order::Asc => revision_pair,
                Order::Desc => time_reverse_revision_pair(revision_pair),
            };
            add_revision(bootstrap_tables, &mut tables, &revision_pair)?;
            num_added += 1;
        }
    }
    let snapshot = TableSummarySnapshot {
        tables,
        ts: *target_ts,
    };
    Ok((snapshot, num_added))
}

fn time_reverse_revision_pair(revision_pair: RevisionPair) -> RevisionPair {
    let RevisionPair {
        id,
        rev: DocumentRevision { ts, document },
        prev_rev,
    } = revision_pair;
    RevisionPair {
        id,
        rev: DocumentRevision {
            ts,
            document: prev_rev.and_then(|rev| rev.document),
        },
        prev_rev: document.map(|doc| DocumentRevision {
            ts: Timestamp::MAX, // we don't know when the current revision was/will be changed
            document: Some(doc),
        }),
    }
}

fn add_revision(
    table_mapping: BootstrapTableIds,
    tables: &mut BTreeMap<TabletId, TableSummary>,
    revision_pair: &RevisionPair,
) -> anyhow::Result<()> {
    // First, create tables for all new tables within the transaction.
    // And delete tables dropped within the transaction.
    // Since our table metadata is fixed at `start_ts`, we know that all
    // subsequent table creations aren't in `snapshot` and must be
    // included.
    let tablet_id = TabletId(revision_pair.id.internal_id());
    if table_mapping.is_tables_table(revision_pair.id.table()) {
        match (revision_pair.prev_document(), revision_pair.document()) {
            (None, Some(_)) => {
                // Table creation creates a TableSummary::empty, if none exists.
                // In historical instances, some _tables rows were created after the records for
                // that table had been inserted.
                tables.entry(tablet_id).or_insert_with(TableSummary::empty);
            },
            (Some(_), None) => {
                // Table deletion removes table summary.
                tables.remove(&tablet_id);
            },
            _ => {},
        }
        if let Some(new_doc) = revision_pair.document() {
            let table_metadata: ParsedDocument<TableMetadata> = new_doc.parse()?;
            if table_metadata.state == TableState::Deleting {
                // Hax alert! Remove shape tracking from soft-deleted tables'
                // summaries, to prevent old shapes from filling up the overall
                // table summary object.
                // It's not correct to remove the summary entry entirely because
                // we still want to be able to rewind through this revision.
                if let Some(summary) = tables.get_mut(&tablet_id) {
                    summary.reset_shape();
                }
            }
        }
    }
    let id = &revision_pair.id;
    let summary = tables.entry(id.table()).or_insert_with(
        // In historical instances, some rows were created before their corresponding
        // `_table` row.
        TableSummary::empty,
    );
    if let Some(old_document) = revision_pair.prev_document() {
        summary.remove(old_document.value())?;
    }
    if let Some(new_document) = revision_pair.document() {
        summary.insert(new_document.value());
    }
    Ok(())
}
