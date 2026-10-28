//! Public streaming-export ("data sync") API built on top of the low-level
//! [`table_iteration::data_sync::DataSyncIterator`]. It resolves a table
//! selection to concrete tablets, drives the iterator one page at a time,
//! converts emitted revisions into name-addressed [`SyncEntry`]s, and
//! (de)serializes the opaque cursor.
//!
//! This is the forward-looking replacement for the `list_snapshot` /
//! `document_deltas` APIs (still on `Database`). It lives in its own crate and
//! reads through a [`DatabaseSnapshot`] rather than a live `Database`, so call
//! sites without one (e.g. offline tools, backups) can depend on it directly.
//!
//! See <https://app.notion.com/p/convex-dev/Robust-Streaming-Export-API-36db57ff32ab80c68d97e01c578518d4>

pub mod managed;

use std::collections::{
    BTreeMap,
    BTreeSet,
};

use anyhow::Context as _;
use common::{
    components::{
        ComponentId,
        ComponentPath,
    },
    persistence::DocumentLogEntry,
    runtime::Runtime,
    types::{
        IndexRef,
        Timestamp,
    },
    version::ClientType,
};
use database::{
    streaming_export_selection::{
        StreamingExportDocument,
        StreamingExportSelection,
    },
    unauthorized_error,
    DatabaseSnapshot,
    Snapshot,
};
use errors::ErrorMetadata;
use keybroker::{
    Identity,
    RandomEncryptor,
};
use pb_data_sync::convex_data_sync as pb_ds;
use table_iteration::data_sync::{
    DataSyncCursor,
    DataSyncStatus,
};
use usage_tracking::{
    FunctionUsageStats,
    FunctionUsageTracker,
};
use value::{
    DeveloperDocumentId,
    InternalId,
    TableMapping,
    TableName,
    TabletId,
};

/// A single document-level entry emitted by [`data_sync`], addressed by
/// (component, table) name rather than tablet id. Table truncations are
/// reported separately, in [`SyncResult::truncates`].
#[derive(Debug)]
pub enum SyncEntry {
    /// The latest revision of a document as of its `ts`.
    Document {
        ts: Timestamp,
        component: ComponentPath,
        table: TableName,
        document: StreamingExportDocument,
    },
    /// A document was deleted at `ts`.
    Tombstone {
        ts: Timestamp,
        component: ComponentPath,
        table: TableName,
        id: DeveloperDocumentId,
    },
}

/// A table the consumer must drop everything it previously synced for, because
/// the table is (re)entering the export from scratch — newly selected, replaced
/// wholesale (e.g. by `npx convex import`), or synced for the first time. The
/// [`SyncEntry`]s in the same (and later) pages re-sync it from scratch.
///
/// Truncations logically apply before any [`SyncEntry`]s in the same page.
#[derive(Debug)]
pub struct SyncTruncate {
    pub component: ComponentPath,
    pub table: TableName,
}

/// Integration issuing a data sync, derived from the `Convex-Client` header.
/// A cold start prefixes its `sync_id` with this so `/data/list_active_syncs`
/// can tell integrations apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataSyncClient {
    Fivetran,
    Airbyte,
    Other,
}

impl DataSyncClient {
    /// Prefix prepended to a cold-start `sync_id`. Empty for [`Self::Other`].
    fn sync_id_prefix(self) -> &'static str {
        match self {
            Self::Fivetran => "fivetran-",
            Self::Airbyte => "airbyte-",
            Self::Other => "",
        }
    }
}

impl From<&ClientType> for DataSyncClient {
    fn from(client: &ClientType) -> Self {
        match client {
            ClientType::FivetranExport | ClientType::FivetranImport => Self::Fivetran,
            ClientType::AirbyteExport => Self::Airbyte,
            _ => Self::Other,
        }
    }
}

/// Progress reported while a sync is still [`SyncStatus::Snapshotting`].
#[derive(Debug)]
pub struct SyncProgress {
    pub num_tables_synced: u64,
    pub total_tables: u64,
    /// The component of the table currently being traversed. An in-progress
    /// sync always has a current table: finishing one either starts the next
    /// or completes the sync ([`SyncStatus::Stale`] or
    /// [`SyncStatus::UpToDate`]).
    pub current_component: ComponentPath,
    /// The table currently being traversed.
    pub current_table: TableName,
    /// Documents emitted so far from the current table's `by_id` traversal.
    pub num_documents_in_current_table: u64,
    /// Documents in the current table at a recent snapshot. `None` while
    /// table summaries are still bootstrapping.
    pub total_documents_in_current_table: Option<u64>,
    /// Documents (including tombstones and re-emitted revisions) emitted over
    /// the sync's lifetime, so this can slightly exceed `total_documents`.
    pub num_documents_synced: u64,
    /// Documents across all target tables at a recent snapshot. `None` while
    /// table summaries are still bootstrapping.
    pub total_documents: Option<u64>,
}

/// The consistency state reported alongside a page.
#[derive(Debug)]
pub enum SyncStatus {
    /// More pages are required before the entries form a consistent snapshot;
    /// `progress` describes how far the initial traversal has gotten.
    Snapshotting { progress: SyncProgress },
    /// The entries emitted so far represent a consistent snapshot at `ts`, but
    /// `ts` is behind the latest timestamp. The caller should fetch again
    /// immediately in order to catch up. Note that the next page may be empty.
    Stale { ts: Timestamp },
    /// The entries emitted so far represent a consistent snapshot at `ts` that
    /// has caught up to the latest data; there is nothing more to fetch right
    /// now.
    UpToDate { ts: Timestamp },
}

/// One page of the data sync API.
pub struct SyncResult {
    /// Tables truncated by this page. Logically apply before `entries`.
    pub truncates: Vec<SyncTruncate>,
    pub entries: Vec<SyncEntry>,
    pub cursor: SyncCursor,
    pub status: SyncStatus,
    pub usage: FunctionUsageStats,
}

const DATA_SYNC_CURSOR_VERSION: u8 = 1;

/// An opaque, forward-compatible cursor for the data sync API. It wraps the
/// low-level [`DataSyncCursor`] together with the (component, table) name each
/// captured tablet resolved to. Serialized via protobuf and encrypted (see
/// [`Self::encrypt`]). Clients treat it as an opaque token.
#[derive(Clone, Debug)]
pub struct SyncCursor {
    inner: DataSyncCursor,
    /// Names of every tablet captured by `inner` (its synced tables plus the
    /// in-progress table), as resolved when they were captured. Carried in the
    /// serialized cursor because [`Self::from_proto`] rejects a tablet without
    /// them, so tokens stay decodable across backend versions.
    names: BTreeMap<TabletId, (ComponentPath, TableName)>,
    /// Unique id assigned when the sync started; keys the
    /// `_data_sync_progress` row that tracks this sync's progress.
    sync_id: String,
}

impl SyncCursor {
    /// Serializes and encrypts the cursor into an opaque, tamper-proof token to
    /// hand back to the client.
    pub fn encrypt(&self, encryptor: &RandomEncryptor) -> anyhow::Result<String> {
        Ok(encryptor.encrypt_proto(DATA_SYNC_CURSOR_VERSION, &self.to_proto()?))
    }

    /// Decrypts and deserializes a token previously produced by
    /// [`Self::encrypt`].
    pub fn decrypt(encryptor: &RandomEncryptor, token: &str) -> anyhow::Result<Self> {
        Self::from_proto(encryptor.decrypt_proto(DATA_SYNC_CURSOR_VERSION, token)?)
    }

    /// Serializes the cursor without encryption, for a token that only
    /// servers we run hold.
    pub fn to_bytes(&self) -> anyhow::Result<Vec<u8>> {
        Ok(prost::Message::encode_to_vec(&self.to_proto()?))
    }

    /// Deserializes a token previously produced by [`Self::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        Self::from_proto(prost::Message::decode(bytes)?)
    }

    /// Tables the cursor has captured. At a consistent snapshot
    /// ([`SyncStatus::Stale`] or [`SyncStatus::UpToDate`]) this is every
    /// target table.
    pub fn tables(&self) -> impl Iterator<Item = &(ComponentPath, TableName)> {
        self.names.values()
    }

    /// Unique id of the sync this cursor belongs to.
    pub fn sync_id(&self) -> &str {
        &self.sync_id
    }

    /// Documents (including tombstones and re-emitted revisions) emitted over
    /// the sync's lifetime.
    pub fn num_docs_synced(&self) -> u64 {
        self.inner.num_docs_synced()
    }

    /// Tables whose entire ID space has been traversed. Once the sync has
    /// reached a consistent snapshot ([`SyncStatus::Stale`] or
    /// [`SyncStatus::UpToDate`]) this is every target table.
    pub fn num_synced_tables(&self) -> u64 {
        self.inner.synced_tables().len() as u64
    }

    fn name_of(&self, tablet_id: &TabletId) -> anyhow::Result<(String, String)> {
        let (component, table) = self
            .names
            .get(tablet_id)
            .ok_or_else(|| anyhow::anyhow!("missing name for captured tablet {tablet_id:?}"))?;
        Ok((String::from(component.clone()), table.to_string()))
    }

    fn to_proto(&self) -> anyhow::Result<pb_ds::DataSyncCursor> {
        let synced_tablets = self
            .inner
            .synced_tables()
            .iter()
            .map(|tablet_id| -> anyhow::Result<_> {
                let (component_path, table_name) = self.name_of(tablet_id)?;
                Ok(pb_ds::SyncedTablet {
                    tablet_id: Some(Vec::from(tablet_id.0)),
                    component_path: Some(component_path),
                    table_name: Some(table_name),
                })
            })
            .collect::<anyhow::Result<_>>()?;
        let table_cursor = match self.inner.in_progress_table() {
            None => pb_ds::data_sync_cursor::TableCursor::Synced(()),
            Some((tablet_id, current_id)) => {
                let (component_path, table_name) = self.name_of(&tablet_id)?;
                pb_ds::data_sync_cursor::TableCursor::InProgress(pb_ds::InProgressTablet {
                    tablet_id: Some(Vec::from(tablet_id.0)),
                    component_path: Some(component_path),
                    table_name: Some(table_name),
                    current_id: current_id.map(|id| id.into()),
                    docs_synced: Some(self.inner.current_table_docs_synced()),
                })
            },
        };
        Ok(pb_ds::DataSyncCursor {
            synced_ts: Some(u64::from(self.inner.synced_ts())),
            synced_tablets,
            table_cursor: Some(table_cursor),
            sync_id: Some(self.sync_id.clone()),
            num_docs_synced: Some(self.inner.num_docs_synced()),
        })
    }

    fn from_proto(proto: pb_ds::DataSyncCursor) -> anyhow::Result<Self> {
        let pb_ds::DataSyncCursor {
            synced_ts,
            synced_tablets,
            table_cursor,
            sync_id,
            num_docs_synced,
        } = proto;
        let synced_ts =
            Timestamp::try_from(synced_ts.ok_or_else(|| anyhow::anyhow!("missing synced_ts"))?)?;

        let mut names = BTreeMap::new();
        let mut synced_tables = std::collections::BTreeSet::new();
        for tablet in synced_tablets {
            let (tablet_id, component, table) =
                parse_named_tablet(tablet.tablet_id, tablet.component_path, tablet.table_name)?;
            synced_tables.insert(tablet_id);
            names.insert(tablet_id, (component, table));
        }

        let mut current_table_docs_synced = 0;
        let in_progress =
            match table_cursor.ok_or_else(|| anyhow::anyhow!("missing table_cursor"))? {
                pb_ds::data_sync_cursor::TableCursor::Synced(()) => None,
                pb_ds::data_sync_cursor::TableCursor::InProgress(in_progress) => {
                    let (tablet_id, component, table) = parse_named_tablet(
                        in_progress.tablet_id,
                        in_progress.component_path,
                        in_progress.table_name,
                    )?;
                    let current_id = in_progress
                        .current_id
                        .map(DeveloperDocumentId::try_from)
                        .transpose()?;
                    names.insert(tablet_id, (component, table));
                    current_table_docs_synced = in_progress.docs_synced.unwrap_or(0);
                    Some((tablet_id, current_id))
                },
            };

        Ok(Self {
            inner: DataSyncCursor::from_parts(
                synced_ts,
                synced_tables,
                in_progress,
                current_table_docs_synced,
                num_docs_synced.unwrap_or(0),
            ),
            names,
            sync_id: sync_id.ok_or_else(|| anyhow::anyhow!("missing sync_id"))?,
        })
    }
}

fn parse_named_tablet(
    tablet_id: Option<Vec<u8>>,
    component_path: Option<String>,
    table_name: Option<String>,
) -> anyhow::Result<(TabletId, ComponentPath, TableName)> {
    let tablet_id = TabletId(InternalId::try_from(
        tablet_id.ok_or_else(|| anyhow::anyhow!("missing tablet_id"))?,
    )?);
    let component: ComponentPath = component_path
        .ok_or_else(|| anyhow::anyhow!("missing component_path"))?
        .parse()?;
    let table: TableName = table_name
        .ok_or_else(|| anyhow::anyhow!("missing table_name"))?
        .parse()?;
    Ok((tablet_id, component, table))
}

/// Whether `tablet_id` is included in the streaming export given `selection`.
///
/// Hidden tablets (i.e. tables that are still being imported) are excluded.
/// We'll process them later when they are activated.
fn table_included(
    selection: &StreamingExportSelection,
    tablet_id: TabletId,
    table_mapping: &TableMapping,
    component_paths: &BTreeMap<ComponentId, ComponentPath>,
) -> anyhow::Result<bool> {
    if !table_mapping.id_exists(tablet_id) {
        // Always exclude deleted tablets.
        return Ok(false);
    }
    if table_mapping.is_system_tablet(tablet_id) || !table_mapping.is_active(tablet_id) {
        return Ok(false);
    }
    let (table_namespace, _, table_name) = table_mapping
        .get_table_metadata(tablet_id)
        .with_context(|| format!("Can't find the table entry for the tablet id {tablet_id}"))?;
    let Some(component_path) = component_paths.get(&ComponentId::from(*table_namespace)) else {
        tracing::warn!("Ignoring orphaned table in streaming export: {table_namespace:?}");
        return Ok(false);
    };
    Ok(selection.is_table_included(component_path, table_name))
}

/// The tablets a [`StreamingExportSelection`] selects, resolved against a
/// snapshot to concrete tablet ids. Tablet ids are stable, so this stays valid
/// for the iterator's own (possibly slightly newer) read snapshot.
struct ResolvedStreamingExportSelection {
    /// The selected tablets, each mapped to its `by_id` index.
    target_tables: BTreeMap<TabletId, IndexRef>,
}

impl ResolvedStreamingExportSelection {
    fn new(snapshot: &Snapshot, selection: &StreamingExportSelection) -> anyhow::Result<Self> {
        let table_mapping = snapshot.table_mapping();
        let component_paths = snapshot.component_ids_to_paths();
        let by_id_indexes = snapshot.index_registry.by_id_indexes()?;

        let mut target_tables: BTreeMap<TabletId, IndexRef> = BTreeMap::new();
        for (tablet_id, ..) in table_mapping.iter() {
            if !table_included(selection, tablet_id, table_mapping, &component_paths)? {
                continue;
            }
            let by_id = *by_id_indexes
                .get(&tablet_id)
                .ok_or_else(|| anyhow::anyhow!("by_id index for {tablet_id:?} missing"))?;
            target_tables.insert(tablet_id, by_id);
        }

        Ok(Self { target_tables })
    }
}

/// Resolves a tablet to the (component, table) name it is exported under.
fn resolve_name(
    snapshot: &Snapshot,
    component_paths: &BTreeMap<ComponentId, ComponentPath>,
    tablet_id: TabletId,
) -> anyhow::Result<(ComponentPath, TableName)> {
    let table_mapping = snapshot.table_mapping();
    let table_name = table_mapping.tablet_name(tablet_id)?;
    let component_id = ComponentId::from(table_mapping.tablet_namespace(tablet_id)?);
    let component_path = component_paths
        .get(&component_id)
        .cloned()
        .unwrap_or_else(ComponentPath::root);
    Ok((component_path, table_name))
}

pub fn selected_tables<RT: Runtime>(
    db_snapshot: &DatabaseSnapshot<RT>,
    selection: &StreamingExportSelection,
) -> anyhow::Result<Vec<(ComponentPath, TableName)>> {
    let snapshot = &db_snapshot.snapshot;
    let table_mapping = snapshot.table_mapping();
    let component_paths = snapshot.component_ids_to_paths();
    let mut tables = vec![];
    for (tablet_id, ..) in table_mapping.iter() {
        if table_included(selection, tablet_id, table_mapping, &component_paths)? {
            tables.push(resolve_name(snapshot, &component_paths, tablet_id)?);
        }
    }
    Ok(tables)
}

/// Mint a fresh sync id for a cold start, tagged with the integration that
/// issued it so `/data/list_active_syncs` can tell syncs apart.
fn new_sync_id<RT: Runtime>(runtime: &RT, sync_client: DataSyncClient) -> String {
    format!("{}{}", sync_client.sync_id_prefix(), runtime.new_uuid_v4())
}

/// Produce the next page of a streaming export ("data sync").
///
/// `cursor: None` starts a fresh sync. `selection` picks the components, tables
/// and columns to export; it is compared against the cursor on every call so
/// tables can be added or removed between pages. Any table that enters the
/// export from scratch — newly selected, replaced (e.g. by `npx convex
/// import`), or seen for the first time on a cold start — yields a
/// [`SyncTruncate`] on the page where its fresh traversal begins. A deselected
/// table just stops being exported.
#[fastrace::trace]
pub async fn data_sync<RT: Runtime>(
    db_snapshot: &DatabaseSnapshot<RT>,
    identity: Identity,
    cursor: Option<SyncCursor>,
    selection: StreamingExportSelection,
    // Integration issuing the sync, prepended to the freshly-minted sync id on
    // a cold start so callers can tell apart syncs from different sources.
    sync_client: DataSyncClient,
) -> anyhow::Result<SyncResult> {
    let usage = FunctionUsageTracker::new();
    anyhow::ensure!(
        identity.is_system() || identity.is_admin(),
        unauthorized_error("data_sync")
    );

    // Resolve the selection against `db_snapshot`'s consistent snapshot. Tablet ids
    // are stable, so the mapping stays valid for the iterator's own (possibly
    // slightly newer) read snapshot.
    let snapshot = &db_snapshot.snapshot;
    let component_paths = snapshot.component_ids_to_paths();
    let target_tables = ResolvedStreamingExportSelection::new(snapshot, &selection)?.target_tables;
    let resolve_name = |tablet_id: TabletId| resolve_name(snapshot, &component_paths, tablet_id);

    // The tablets the cursor was tracking (synced plus in-progress) before this
    // page, compared against the tracked set after it to emit truncates.
    let tracked_before: BTreeSet<TabletId> = cursor
        .as_ref()
        .map(|c| {
            c.inner
                .synced_tables()
                .iter()
                .copied()
                .chain(c.inner.in_progress_table().map(|(tablet_id, _)| tablet_id))
                .collect()
        })
        .unwrap_or_default();

    // Adopt the cursor's sync id, assigning a fresh one on cold start.
    let sync_id = cursor
        .as_ref()
        .map(|c| c.sync_id.clone())
        .unwrap_or_else(|| new_sync_id(db_snapshot.runtime(), sync_client));

    let mut entries = Vec::new();
    let iterator = db_snapshot.data_sync_iterator()?;
    let page = iterator
        .next_page(cursor.map(|c| c.inner), &target_tables)
        .await?;

    for DocumentLogEntry { ts, id, value, .. } in page.entries {
        let tablet_id = id.table();
        let (component, table) = resolve_name(tablet_id)?;
        match value {
            Some(doc) => {
                let column_filter = selection.column_filter(&component, &table)?;
                let document = column_filter.filter_document(doc.to_developer())?;
                usage.track_database_egress_v2(
                    component.clone(),
                    &table,
                    document.size() as u64,
                    false,
                );
                usage.track_database_egress_rows(component.clone(), &table, 1, false);
                entries.push(SyncEntry::Document {
                    ts,
                    component,
                    table,
                    document,
                });
            },
            None => {
                let table_number = snapshot.table_mapping().tablet_number(tablet_id)?;
                let developer_id = DeveloperDocumentId::new(table_number, id.internal_id());
                entries.push(SyncEntry::Tombstone {
                    ts,
                    component,
                    table,
                    id: developer_id,
                });
            },
        }
    }

    // Resolve names for every tablet the new cursor captures. After
    // reconciliation these are all live tablets present in `table_mapping`.
    let mut names = BTreeMap::new();
    for tablet_id in page.cursor.synced_tables() {
        names.insert(*tablet_id, resolve_name(*tablet_id)?);
    }
    if let Some((tablet_id, _)) = page.cursor.in_progress_table() {
        names.insert(tablet_id, resolve_name(tablet_id)?);
    }

    // A [`SyncTruncate`] tells the consumer to drop everything it synced for a
    // table before applying this page's values. Emit one for every tablet that
    // entered the tracked set this page — newly selected, the fresh side of an
    // import replacement, or any table on its first page of a cold-start
    // snapshot — since its rows are synced from scratch. Tables truncate only on
    // entry: a deselected table keeps whatever the consumer synced for it until
    // it is re-selected, which truncates it then.
    let truncated: BTreeSet<(ComponentPath, TableName)> = names
        .iter()
        .filter(|(tablet_id, _)| !tracked_before.contains(tablet_id))
        .map(|(_, name)| name.clone())
        .collect();
    let truncates = truncated
        .into_iter()
        .map(|(component, table)| SyncTruncate { component, table })
        .collect();

    let status = match page.status {
        DataSyncStatus::Stale { ts } => SyncStatus::Stale { ts },
        DataSyncStatus::UpToDate { ts } => SyncStatus::UpToDate { ts },
        DataSyncStatus::Snapshotting { progress } => {
            let (current_component, current_table) = resolve_name(progress.current_table)?;
            // Progress denominators from the incrementally-maintained table
            // counts: no table scans, just map lookups.
            let total_documents_in_current_table = snapshot
                .table_counts
                .as_ref()
                .map(|counts| counts.tablet_count(&progress.current_table).num_values());
            let total_documents = snapshot.table_counts.as_ref().map(|counts| {
                target_tables
                    .keys()
                    .map(|tablet_id| counts.tablet_count(tablet_id).num_values())
                    .sum()
            });
            SyncStatus::Snapshotting {
                progress: SyncProgress {
                    num_tables_synced: progress.num_tables_synced,
                    total_tables: progress.total_tables,
                    current_component,
                    current_table,
                    num_documents_in_current_table: progress.num_documents_in_current_table,
                    total_documents_in_current_table,
                    num_documents_synced: progress.num_documents_synced,
                    total_documents,
                },
            }
        },
    };

    Ok(SyncResult {
        truncates,
        entries,
        cursor: SyncCursor {
            inner: page.cursor,
            names,
            sync_id,
        },
        status,
        usage: usage.gather_user_stats(),
    })
}

/// Convert a legacy `document_deltas` cursor to a [`SyncCursor`]
#[fastrace::trace]
pub async fn data_sync_cursor_from_deltas<RT: Runtime>(
    db_snapshot: &DatabaseSnapshot<RT>,
    identity: Identity,
    ts: Timestamp,
    selection: StreamingExportSelection,
    sync_client: DataSyncClient,
) -> anyhow::Result<SyncCursor> {
    anyhow::ensure!(
        identity.is_system() || identity.is_admin(),
        unauthorized_error("data_sync_cursor_from_deltas")
    );

    anyhow::ensure!(
        ts <= *db_snapshot.timestamp(),
        ErrorMetadata::bad_request(
            "InvalidDataSyncCursor",
            "document_deltas cursor is ahead of the deployment's latest timestamp",
        )
    );
    // The first `data_sync` page walks the document log forward from `ts`, so
    // `ts` must still be within document retention. Fail here rather than on the
    // first page so the caller learns the cursor is unusable before it starts.
    db_snapshot
        .retention_validator
        .validate_document_snapshot(ts)
        .await?;

    let snapshot = &db_snapshot.snapshot;
    let component_paths = snapshot.component_ids_to_paths();
    let target_tables = ResolvedStreamingExportSelection::new(snapshot, &selection)?.target_tables;
    let names = target_tables
        .keys()
        .map(|tablet_id| {
            Ok((
                *tablet_id,
                resolve_name(snapshot, &component_paths, *tablet_id)?,
            ))
        })
        .collect::<anyhow::Result<BTreeMap<_, _>>>()?;

    Ok(SyncCursor {
        inner: DataSyncCursor::from_parts(
            ts,
            target_tables.keys().copied().collect(),
            // No in-progress table: the whole ID space is already synced.
            None,
            0,
            0,
        ),
        names,
        sync_id: new_sync_id(db_snapshot.runtime(), sync_client),
    })
}
