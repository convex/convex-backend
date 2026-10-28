// API types for the streaming export HTTP APIs. These are consumed by the
// Fivetran and Airbyte source connectors, as well as any other client of the
// streaming export API.

pub mod selection;

use std::collections::BTreeMap;

use selection::Selection;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as JsonValue;
use tuple_struct::tuple_struct_string;
use utoipa::ToSchema;

use crate::http::PaginationMetadata;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDeltasArgs {
    /// Exclusive timestamp. Initially pass `ListSnapshotResponse.snapshot` for
    /// the first page. Then pass DocumentDeltasResponse.cursor for
    /// subsequent pages.
    pub cursor: Option<i64>,

    /// The components, tables, and columns to export.
    #[serde(flatten)]
    pub selection: SelectionArg,

    /// Export format
    pub format: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDeltasResponse {
    /// Document deltas, in timestamp order.
    pub values: Vec<DocumentDeltasValue>,
    /// Exclusive timestamp for passing in as `cursor` to subsequent API calls.
    pub cursor: i64,
    /// Continue calling the API while has_more is true.
    pub has_more: bool,
}

/// Identical to `ListSnapshotValue`, but with a `deleted` field
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DocumentDeltasValue {
    /// The path of the component this document is from.
    #[serde(rename = "_component")]
    pub component: String,

    /// The name of the table this document is from.
    #[serde(rename = "_table")]
    pub table: String,

    /// _ts is the field used for ordering documents with the same
    /// _id, and determining which version is latest.
    #[serde(rename = "_ts")]
    pub ts: i64,

    /// Indicates whether the document was deleted. Will always be `false` in
    /// the list snapshot API
    #[serde(rename = "_deleted")]
    pub deleted: bool,

    /// The fields of the document. Connectors must ignore fields prefixed by
    /// `_` (except `_id` and `_creationTime`) since they could be used by
    /// future versions of the API for new fields.
    #[serde(flatten)]
    pub fields: BTreeMap<String, JsonValue>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSnapshotArgs {
    /// Timestamp snapshot. Initially pass None, then pass
    /// ListSnapshotResponse.snapshot for subsequent pages.
    pub snapshot: Option<i64>,

    /// Exclusive internal identifier. Initially pass None, then pass
    /// ListSnapshotResponse.cursor for subsequent pages.
    pub cursor: Option<String>,

    /// The components, tables, and columns to export.
    #[serde(flatten)]
    pub selection: SelectionArg,

    /// Export format
    pub format: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSnapshotResponse {
    /// Documents, in (id, ts) order.
    pub values: Vec<ListSnapshotValue>,
    /// Timestamp snapshot. Pass this in as `snapshot` to subsequent API calls.
    pub snapshot: i64,
    /// Exclusive document id for passing in as `cursor` to subsequent API
    /// calls.
    pub cursor: Option<String>,
    /// Continue calling the API while has_more is true.
    /// When this becomes false, the `ListSnapshotResponse.snapshot` can be used
    /// as `DocumentDeltasArgs.cursor` to get deltas after the snapshot.
    pub has_more: bool,
}

/// A value returned by the list snapshot API.
/// This corresponds to a Convex document with some special fields added.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ListSnapshotValue {
    /// The path of the component this document is from.
    #[serde(rename = "_component")]
    pub component: String,

    /// The name of the table this document is from.
    #[serde(rename = "_table")]
    pub table: String,

    /// _ts is the field used for ordering documents with the same
    /// _id, and determining which version is latest.
    #[serde(rename = "_ts")]
    pub ts: i64,

    /// The fields of the document. Connectors must ignore fields prefixed by
    /// `_` (except `_id` and `_creationTime`) since they could be used by
    /// future versions of the API for new fields.
    #[serde(flatten)]
    pub fields: BTreeMap<String, JsonValue>,
}

tuple_struct_string!(
    /// Unique id of a data sync, assigned by `/api/v1/data/sync` on the sync's
    /// first page and stable across its lifetime.
    SyncId
);

/// Arguments to the data sync (streaming export) API (`/api/v1/data/sync`).
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataSyncArgs {
    /// Opaque cursor returned by a previous call. Omit to start from scratch.
    pub cursor: Option<String>,

    /// When set, only sync the selected subset of the data.
    ///
    /// Selects the components, tables, and columns to export. Each key is a
    /// component path (`""` for the root component), mapped to the selection
    /// for that component.
    ///
    /// The selection may change between calls of the same sync: newly selected
    /// tables are synced from scratch, possibly moving the sync into
    /// `snapshotting` state if necessary, and emit a truncate on the first page
    /// they appear so the consumer starts them from a clean slate. Deselected
    /// tables stop being synced.
    #[serde(default)]
    pub selection: Selection,
}

/// One page returned by the data sync API.
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataSyncResponse {
    /// The status of the sync after this page.
    pub status: DataSyncStatus,
    /// Tables truncated by this page. The consumer should drop everything it
    /// previously synced for each table, then apply `values` (which re-sync
    /// them from scratch). Logically applies before `values`.
    ///
    /// A table is truncated whenever it (re)enters the export from scratch —
    /// the first page it is synced (including on a cold start), when it is
    /// newly selected, or when it is replaced by a bulk operation such as
    /// `npx convex import`.
    pub truncates: Vec<DataSyncTruncate>,
    /// Documents created, updated, or deleted in this page.
    pub values: Vec<DataSyncValue>,
    /// Unique id of the sync, assigned on the first page and stable across
    /// the sync's lifetime. Identifies this sync in `/data/sync/{syncId}` and
    /// `/data/list_active_syncs`.
    pub sync_id: SyncId,
    /// Pagination information. The data sync endpoint is an infinite streaming
    /// endpoint, so `nextCursor` is always present and `hasMore` is always
    /// `true` — another page can always be fetched with the cursor. Use
    /// `status` to pace calls: back off significantly to wait for more writes
    /// once it reports `upToDate`.
    pub pagination: PaginationMetadata,
}

/// An entry indicating that the table should be truncated. Emitted when a table
/// is newly syncing or replaced wholesale (e.g. by `npx convex import`).
/// Reported separately from `values` since it carries none of the per-document
/// fields.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DataSyncTruncate {
    /// The path of the component the table is in.
    pub component: String,

    /// The name of the truncated table.
    pub table: String,
}

/// A single document-level entry emitted by the data sync API: a Convex
/// document (or a tombstone, for a deletion) nested under `value`, with
/// metadata fields alongside it.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DataSyncValue {
    /// The path of the component this entry is from.
    pub component: String,

    /// The name of the table this entry is from.
    pub table: String,

    /// The timestamp at which this revision was written.
    pub ts: i64,

    /// Whether the document was deleted (a tombstone).
    pub deleted: bool,

    /// The fields of the document, including the built-in `_id` and
    /// `_creationTime`. For `deleted` documents, only `_id` is present.
    #[schema(value_type = Object)]
    pub value: BTreeMap<String, JsonValue>,
}

/// The literal string `snapshotting`, discriminating "snapshotting" status
/// objects.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, ToSchema)]
pub enum SnapshottingTag {
    #[serde(rename = "snapshotting")]
    Snapshotting,
}

/// The literal string `stale`, discriminating "stale" status objects.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, ToSchema)]
pub enum StaleTag {
    #[serde(rename = "stale")]
    Stale,
}

/// The literal string `upToDate`, discriminating "up to date" status objects.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, ToSchema)]
pub enum UpToDateTag {
    #[serde(rename = "upToDate")]
    UpToDate,
}

/// The consistency state reported alongside a data sync page, discriminated
/// by `type`.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
#[serde(untagged)]
#[schema(discriminator(
    property_name = "type",
    mapping(
        ("snapshotting" = "#/components/schemas/DataSyncSnapshotting"),
        ("stale" = "#/components/schemas/DataSyncStale"),
        ("upToDate" = "#/components/schemas/DataSyncUpToDate"),
    )
))]
pub enum DataSyncStatus {
    Snapshotting(DataSyncSnapshotting),
    Stale(DataSyncStale),
    UpToDate(DataSyncUpToDate),
}

/// The sync has not yet reached a consistent snapshot. The entries emitted
/// so far are an incomplete initial traversal of the selected tables.
/// Syncs begin in this state. The sync's
/// progress can be monitored via `/data/sync/{syncId}` or
/// `/data/list_active_syncs`, keyed by the response's `syncId`. Syncs may
/// return to this state if the table selection has changes that requires large
/// data sync.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataSyncSnapshotting {
    /// Always `snapshotting`.
    #[serde(rename = "type")]
    #[schema(inline)]
    pub status_type: SnapshottingTag,
}

/// The entries emitted so far represent a consistent snapshot at
/// a stale `snapshotTs`.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataSyncStale {
    /// Always `stale`.
    #[serde(rename = "type")]
    #[schema(inline)]
    pub status_type: StaleTag,
    /// The database timestamp at which the synced data is consistent.
    pub snapshot_ts: i64,
}

/// The sync is up to date and represents a latest consistent snapshot.
/// For a streaming export in this state, it is recommended to backoff for
/// some time, wait for more data, and then continue the streaming sync.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataSyncUpToDate {
    /// Always `upToDate`.
    #[serde(rename = "type")]
    #[schema(inline)]
    pub status_type: UpToDateTag,
    /// The database timestamp at which the synced data is consistent.
    pub snapshot_ts: i64,
}

/// Arguments to the legacy-cursor conversion API
/// (`/api/data_sync_cursor_from_deltas`). Deliberately undocumented and outside
/// the platform OpenAPI spec: it exists so integrations built on
/// `document_deltas` can move to `/api/v1/data/sync` without a full resync.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSyncCursorFromDeltasArgs {
    /// The exclusive timestamp cursor last returned by `document_deltas`.
    pub cursor: i64,

    /// The selection the caller was passing to `document_deltas`. Tables
    /// outside it are synced from scratch by the resulting data sync.
    #[serde(default)]
    pub selection: Selection,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSyncCursorFromDeltasResponse {
    /// Opaque cursor to pass to `/api/v1/data/sync` as `cursor`.
    pub cursor: String,
}

/// Response of the active-syncs listing API
/// (`/api/v1/data/list_active_syncs`).
#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListActiveSyncsResponse {
    /// Page of active data syncs, most recently updated first. A sync is
    /// active if it fetched a page from `/api/v1/data/sync` within the past 3
    /// days.
    pub syncs: Vec<ActiveDataSync>,
    pub pagination: PaginationMetadata,
}

/// The status of one active data sync, as of its most recent page. Returned
/// by `/api/v1/data/sync/{syncId}` and for each sync listed by
/// `/api/v1/data/list_active_syncs`.
#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActiveDataSync {
    /// Unique id of the sync, assigned when it started (i.e. when
    /// `/api/v1/data/sync` was called without a cursor) and stable across its
    /// pages.
    pub sync_id: SyncId,
    /// Wall-clock time of the last `/data/sync` call made by this sync, as a
    /// unix timestamp in milliseconds.
    pub last_updated: i64,
    /// The sync's progress as of its most recently recorded page.
    pub status: ActiveDataSyncStatus,
}

/// The progress of an active data sync, discriminated by `type`.
// See `DataSyncStatus` for why this is untagged with per-variant structs.
#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
#[serde(untagged)]
#[schema(discriminator(
    property_name = "type",
    mapping(
        ("snapshotting" = "#/components/schemas/ActiveDataSyncSnapshotting"),
        ("stale" = "#/components/schemas/ActiveDataSyncStale"),
        ("upToDate" = "#/components/schemas/ActiveDataSyncUpToDate"),
    )
))]
pub enum ActiveDataSyncStatus {
    /// The sync has not yet reached a consistent snapshot. The entries emitted
    /// so far are an incomplete initial traversal of the selected tables.
    /// Syncs begin in this state, and may reenter this state if tables are
    /// added/replaced
    Snapshotting(ActiveDataSyncSnapshotting),
    /// The entries emitted so far represent a consistent snapshot at
    /// a stale `snapshotTs`. The sync is "catching up".
    Stale(ActiveDataSyncStale),
    /// The sync is up to date and represents a latest consistent snapshot.
    UpToDate(ActiveDataSyncUpToDate),
}

/// The sync is still traversing its selected tables; the data returned so
/// far is not yet a consistent snapshot.
#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActiveDataSyncSnapshotting {
    /// Always `snapshotting`.
    #[serde(rename = "type")]
    #[schema(inline)]
    pub status_type: SnapshottingTag,
    /// Tables whose initial traversal has completed.
    pub num_tables_synced: u64,
    /// Total tables selected for the sync.
    pub total_tables: u64,
    /// The component of the table currently being traversed (the empty
    /// string for the root component).
    pub current_component: String,
    /// The table currently being traversed.
    pub current_table: String,
    /// Documents synced so far from the current table.
    pub num_documents_in_current_table: u64,
    /// Total documents in the current table, as of a recent snapshot.
    pub total_documents_in_current_table: u64,
    /// Documents synced over the sync's lifetime, including deletions and
    /// re-synced revisions of documents that changed mid-sync — so this can
    /// slightly exceed `totalDocuments`.
    pub num_documents_synced: u64,
    /// Total documents across all selected tables, as of a recent snapshot.
    pub total_documents: u64,
}

/// The sync reached a consistent snapshot at `syncedTs`, but its timestamp is
/// stale; it is streaming later changes (CDC). Newer data may be available.
#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActiveDataSyncStale {
    /// Always `stale`.
    #[serde(rename = "type")]
    #[schema(inline)]
    pub status_type: StaleTag,
    /// Total tables selected for the sync.
    pub total_tables: u64,
    /// Documents synced over the sync's lifetime, including deletions and
    /// re-synced revisions.
    pub num_documents_synced: u64,
    /// The database timestamp at which the synced data is consistent.
    pub synced_ts: i64,
}

/// The sync reached a consistent snapshot at `syncedTs` and has caught up to
/// the latest data.
#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActiveDataSyncUpToDate {
    /// Always `upToDate`.
    #[serde(rename = "type")]
    #[schema(inline)]
    pub status_type: UpToDateTag,
    /// Total tables selected for the sync.
    pub total_tables: u64,
    /// Documents synced over the sync's lifetime, including deletions and
    /// re-synced revisions.
    pub num_documents_synced: u64,
    /// The database timestamp at which the synced data is consistent.
    pub synced_ts: i64,
}

/// Since [ListSnapshotArgs] and [DocumentDeltasArgs] need to support the older
/// selection formats, this wraps the newer selection format ([Selection]) while
/// providing a way to deserialize the older formats.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum SelectionArg {
    /// Newer selection format, allows to select specific tables, components,
    /// and columns.
    Exact { selection: Selection },

    /// If only the table name is provided, assumes it’s in the root component.
    SingleTable {
        #[serde(alias = "tableName")]
        table_name: String,

        /// The component path of the table. If not provided, the table is
        /// assumed to be in the root component.
        component: Option<String>,
    },

    /// The user can also provide a component name to export all tables in that
    /// component.
    SingleComponent { component: String },

    /// If no selection parameter is provided, return all components, tables and
    /// columns.
    Everything {},
}

impl Default for SelectionArg {
    fn default() -> Self {
        SelectionArg::Everything {}
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTableColumnNamesResponse {
    pub by_component: BTreeMap<String, Vec<GetTableColumnNameTable>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTableColumnNameTable {
    pub name: String,
    pub columns: Vec<String>,
}
