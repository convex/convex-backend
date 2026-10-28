use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    time::{
        Duration,
        Instant,
    },
};

use anyhow::Context;
use common::types::streaming_export::{
    selection::Selection,
    ActiveDataSyncStatus,
    DataSyncStatus,
    SyncId,
};
use derive_more::{
    Display,
    From,
    Into,
};
use fivetran_common::fivetran_sdk::{
    self,
    update_response,
    value_type,
    Record,
    RecordType,
    UpdateResponse as FivetranUpdateResponse,
    ValueType,
};
use futures::{
    stream::BoxStream,
    StreamExt,
};
use futures_async_stream::try_stream;
use serde::{
    Deserialize,
    Serialize,
};
use value_type::Inner as FivetranValue;

use crate::{
    convert::to_fivetran_row,
    convex_api::{
        fivetran_schema_name,
        Source,
    },
    log::log,
};

/// The value currently used for the `version` field of [`State`].
const CURSOR_VERSION: i64 = 3;

/// Stores the current synchronization state of a destination. A state will be
/// send (as JSON) to Fivetran every time we perform a checkpoint, and will be
/// returned to us every time Fivetran calls the `update` method of the
/// connector.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct State {
    /// The version of the connector that emitted this checkpoint. Could be used
    /// in the future to support backward compatibility with older state
    /// formats.
    pub version: i64,

    pub checkpoint: Checkpoint,

    /// The set of tables a version-2 connector had already seen, used to decide
    /// when to issue a truncate. The data sync API reports truncates itself, so
    /// this is only still declared to accept checkpoints written before the
    /// migration.
    ///
    /// The format of this string is `{table_name}` for the root component,
    /// or `{component_path}/{table_name}` for tables in other components.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tables_seen: Option<BTreeSet<String>>,
}

impl State {
    pub fn create(checkpoint: Checkpoint) -> Self {
        Self {
            version: CURSOR_VERSION,
            checkpoint,
            tables_seen: None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub enum Checkpoint {
    /// A checkpoint emitted by a version-2 connector during its initial
    /// synchronization, using the `list_snapshot` API.
    InitialSync {
        snapshot: i64,
        cursor: ListSnapshotCursor,
    },
    /// A checkpoint emitted by a version-2 connector after an initial
    /// synchronization completed, using the `document_deltas` API.
    DeltaUpdates { cursor: DocumentDeltasCursor },
    /// An opaque cursor from the data sync API.
    DataSync { cursor: String },
}

/// A cursor for the legacy `list_snapshot` API. Only ever read, from
/// checkpoints written before the data sync migration.
#[derive(Display, Serialize, Deserialize, Debug, PartialEq, Eq, Clone, From, Into)]
pub struct ListSnapshotCursor(pub String);

/// A cursor for the legacy `document_deltas` API: an exclusive timestamp. Only
/// ever read, from checkpoints written before the data sync migration.
#[derive(Display, Serialize, Deserialize, Debug, PartialEq, Eq, Clone, From, Into, Copy)]
pub struct DocumentDeltasCursor(pub i64);

/// A simplification of the messages sent to Fivetran in the `update` endpoint.
#[derive(Debug)]
pub enum UpdateMessage {
    Update {
        schema_name: Option<String>,
        table_name: String,
        op_type: RecordType,
        row: BTreeMap<String, FivetranValue>,
    },
    Checkpoint(State),
}

/// Conversion of the simplified update message type to the actual gRPC type.
impl From<UpdateMessage> for FivetranUpdateResponse {
    fn from(value: UpdateMessage) -> Self {
        FivetranUpdateResponse {
            operation: Some(match value {
                UpdateMessage::Update {
                    schema_name,
                    table_name,
                    op_type,
                    row,
                } => update_response::Operation::Record(Record {
                    schema_name,
                    table_name,
                    r#type: op_type as i32,
                    data: row
                        .into_iter()
                        .map(|(field_name, field_value)| {
                            (
                                field_name,
                                ValueType {
                                    inner: Some(field_value),
                                },
                            )
                        })
                        .collect(),
                }),
                UpdateMessage::Checkpoint(checkpoint) => {
                    let state_json = serde_json::to_string(&checkpoint)
                        .expect("Couldn’t serialize a checkpoint");
                    update_response::Operation::Checkpoint(fivetran_sdk::Checkpoint { state_json })
                },
            }),
        }
    }
}

/// Returns the stream that the `update` endpoint emits.
pub fn sync(
    source: impl Source + 'static,
    state: Option<State>,
    selection: Selection,
) -> BoxStream<'static, anyhow::Result<UpdateMessage>> {
    let cursor = state.map(|state| state.checkpoint);
    data_sync(source, cursor, selection).boxed()
}

/// Resolves the checkpoint we were handed into a data sync cursor, or `None` to
/// start a sync from scratch.
///
/// A `DeltaUpdates` checkpoint is converted server-side into an equivalent data
/// sync cursor, so connections created before the migration keep their data
/// instead of resyncing. There is no equivalent for a mid-`InitialSync`
/// checkpoint, but starting over is harmless: the data sync API truncates each
/// table before syncing it, so the partial data is replaced rather than
/// duplicated.
async fn resolve_cursor(
    source: &impl Source,
    checkpoint: Option<Checkpoint>,
    selection: &Selection,
) -> anyhow::Result<Option<String>> {
    let Some(checkpoint) = checkpoint else {
        log(&format!("Starting a data sync from {source}"));
        return Ok(None);
    };
    match checkpoint {
        Checkpoint::DataSync { cursor } => {
            // The cursor itself is an opaque, unbounded token: not worth logging.
            log(&format!("Resuming a data sync from {source}"));
            Ok(Some(cursor))
        },
        Checkpoint::InitialSync { snapshot, .. } => {
            log(&format!(
                "Restarting an initial sync from {source} that was interrupted at {snapshot} \
                 before the data sync migration"
            ));
            Ok(None)
        },
        Checkpoint::DeltaUpdates { cursor } => {
            log(&format!(
                "Migrating {source} from document deltas at {cursor} to a data sync"
            ));
            // A cursor the deployment can't carry over (e.g. one that fell out
            // of the retention window) fails the sync, surfacing Convex's
            // message to the customer as a Fivetran task. That matches what the
            // `document_deltas` API already did for such cursors.
            Ok(Some(
                source
                    .data_sync_cursor_from_deltas(cursor.into(), selection.clone())
                    .await?,
            ))
        },
    }
}

/// Streams pages from the data sync API until the sync has caught up to the
/// latest data.
#[try_stream(ok = UpdateMessage, error = anyhow::Error)]
async fn data_sync(source: impl Source, checkpoint: Option<Checkpoint>, selection: Selection) {
    let mut cursor = resolve_cursor(&source, checkpoint, &selection).await?;
    let mut last_progress_log: Option<Instant> = None;

    loop {
        let page = source.data_sync(cursor.clone(), selection.clone()).await?;

        // Truncates logically apply before the values in the same page.
        for truncate in page.truncates {
            yield UpdateMessage::Update {
                schema_name: Some(fivetran_schema_name(&truncate.component)),
                table_name: truncate.table,
                op_type: RecordType::Truncate,
                row: BTreeMap::new(),
            };
        }

        for value in page.values {
            yield UpdateMessage::Update {
                schema_name: Some(fivetran_schema_name(&value.component)),
                table_name: value.table,
                op_type: if value.deleted {
                    RecordType::Delete
                } else {
                    RecordType::Upsert
                },
                row: to_fivetran_row(value.value)?,
            };
        }

        cursor = Some(
            page.pagination
                .next_cursor
                .context("Data sync response is missing a cursor")?,
        );

        // Fivetran applies the records above and this checkpoint together, which
        // is what the data sync API requires: a page and the cursor that follows
        // it must be persisted atomically.
        yield UpdateMessage::Checkpoint(State::create(Checkpoint::DataSync {
            cursor: cursor.clone().expect("just set"),
        }));

        // `pagination.has_more` is always true for a data sync — the stream has
        // no end — so the status is what tells us we've caught up.
        let caught_up = matches!(page.status, DataSyncStatus::UpToDate(_));
        // A large initial sync runs for many pages, so report progress on a
        // timer rather than per page. The final page always reports, since
        // that's the line saying the sync finished.
        let due = last_progress_log.is_none_or(|at| at.elapsed() >= PROGRESS_LOG_INTERVAL);
        if caught_up || due {
            last_progress_log = Some(Instant::now());
            log_progress(&source, &page.sync_id).await;
        }
        if caught_up {
            break;
        }
    }
}

/// How often the connector reports a sync's progress while it runs.
const PROGRESS_LOG_INTERVAL: Duration = Duration::from_secs(15);

/// Logs the deployment's own view of how far `sync_id` has gotten.
///
/// Progress is informational, so a failure to read it is logged and the sync
/// carries on.
async fn log_progress(source: &impl Source, sync_id: &SyncId) {
    let sync = match source.get_active_sync(sync_id).await {
        Ok(sync) => sync,
        Err(e) => {
            log(&format!(
                "Could not read the progress of data sync {sync_id} from {source}: {e}"
            ));
            return;
        },
    };
    let progress = match sync.status {
        ActiveDataSyncStatus::Snapshotting(snapshotting) => format!(
            "taking an initial snapshot: {} of {} documents ({}), on table {} ({} of {} tables)",
            snapshotting.num_documents_synced,
            snapshotting.total_documents,
            percentage(
                snapshotting.num_documents_synced,
                snapshotting.total_documents
            ),
            snapshotting.current_table,
            snapshotting.num_tables_synced,
            snapshotting.total_tables,
        ),
        ActiveDataSyncStatus::Stale(stale) => format!(
            "caught up to a consistent snapshot at {} after {} documents, still behind the latest \
             data",
            stale.synced_ts, stale.num_documents_synced,
        ),
        ActiveDataSyncStatus::UpToDate(up_to_date) => format!(
            "up to date at {} after {} documents across {} tables",
            up_to_date.synced_ts, up_to_date.num_documents_synced, up_to_date.total_tables,
        ),
    };
    log(&format!("Data sync {sync_id} from {source} is {progress}"));
}

/// `done` out of `total` as a percentage, or `?%` when the deployment hasn't
/// finished counting the target tables yet.
fn percentage(done: u64, total: u64) -> String {
    if total == 0 {
        return "?%".to_string();
    }
    format!("{}%", (done * 100).saturating_div(total).min(100))
}
