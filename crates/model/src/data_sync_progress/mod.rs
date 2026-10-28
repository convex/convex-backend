use std::{
    sync::{
        Arc,
        LazyLock,
    },
    time::Duration,
};

use common::{
    document::{
        ParseDocument as _,
        ParsedDocument,
        CREATION_TIME_FIELD_PATH,
    },
    query::{
        Cursor,
        CursorPosition,
        IndexRange,
        Order,
        Query,
    },
    runtime::Runtime,
};
use database::{
    query::{
        PaginationOptions,
        TableFilter,
    },
    ResolvedQuery,
    SystemMetadataModel,
    Transaction,
};
use value::{
    FieldPath,
    TableName,
    TableNamespace,
};

use crate::{
    SystemIndex,
    SystemTable,
};

pub mod types;
use types::DataSyncProgressMetadata;

pub const DATA_SYNC_PROGRESS_TABLE: TableName = TableName::const_new("_data_sync_progress");

/// A sync is "active" if it completed a page within this window. It matches
/// the document retention window: a sync idle for longer can no longer resume
/// its cursor. Rows of inactive syncs are kept for posterity but excluded
/// from [`DataSyncProgressModel::active_syncs`].
pub const DATA_SYNC_ACTIVE_WINDOW: Duration = Duration::from_secs(3 * 24 * 60 * 60);

pub static DATA_SYNC_PROGRESS_INDEX_BY_SYNC_ID: LazyLock<SystemIndex<DataSyncProgressTable>> =
    LazyLock::new(|| {
        SystemIndex::new("by_sync_id", [&SYNC_ID_FIELD, &CREATION_TIME_FIELD_PATH]).unwrap()
    });
pub static DATA_SYNC_PROGRESS_INDEX_BY_LAST_UPDATED: LazyLock<SystemIndex<DataSyncProgressTable>> =
    LazyLock::new(|| {
        SystemIndex::new(
            "by_last_updated",
            [&LAST_UPDATED_MS_FIELD, &CREATION_TIME_FIELD_PATH],
        )
        .unwrap()
    });
static SYNC_ID_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "syncId".parse().expect("invalid syncId field"));
static LAST_UPDATED_MS_FIELD: LazyLock<FieldPath> = LazyLock::new(|| {
    "lastUpdatedMs"
        .parse()
        .expect("invalid lastUpdatedMs field")
});

pub struct DataSyncProgressTable;
impl SystemTable for DataSyncProgressTable {
    type Metadata = DataSyncProgressMetadata;

    const TABLE_NAME: TableName = DATA_SYNC_PROGRESS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![
            DATA_SYNC_PROGRESS_INDEX_BY_SYNC_ID.clone(),
            DATA_SYNC_PROGRESS_INDEX_BY_LAST_UPDATED.clone(),
        ]
    }
}

pub struct DataSyncProgressModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> DataSyncProgressModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    /// The sync's progress row, if any.
    pub async fn get(
        &mut self,
        sync_id: &str,
    ) -> anyhow::Result<Option<Arc<ParsedDocument<DataSyncProgressMetadata>>>> {
        self.tx
            .query_system(TableNamespace::Global, &DATA_SYNC_PROGRESS_INDEX_BY_SYNC_ID)?
            .eq(&[sync_id])?
            .unique()
            .await
    }

    /// The progress row of the sync with this id, if it completed a page
    /// within [`DATA_SYNC_ACTIVE_WINDOW`]. The single-sync counterpart of
    /// [`Self::active_syncs`].
    pub async fn active_sync(
        &mut self,
        now_ms: u64,
        sync_id: &str,
    ) -> anyhow::Result<Option<DataSyncProgressMetadata>> {
        let cutoff_ms = now_ms.saturating_sub(DATA_SYNC_ACTIVE_WINDOW.as_millis() as u64);
        Ok(self
            .get(sync_id)
            .await?
            .filter(|doc| doc.last_updated_ms >= cutoff_ms)
            .map(|doc| (*doc).clone().into_value()))
    }

    /// Upsert the progress row for `metadata.sync_id`, throttled to at most one
    /// write per `min_write_interval` while the sync is still advancing.
    /// Returns the row's previous metadata; `None` means the sync had no row
    /// yet and this write inserted it. A throttled update returns the
    /// existing metadata without writing, leaving the transaction free of
    /// writes. Pass `Duration::ZERO` to always write.
    ///
    /// `caught_up` marks a page that reached a fully-consistent snapshot with
    /// nothing left to sync (`Synced` with no more to catch up on). Such a
    /// settled state is flushed as soon as its document count changes,
    /// bypassing the throttle, so a sync's final progress is never lost — right
    /// after an import the first `Synced` page can report zero documents (the
    /// writes aren't yet visible at the lagging repeatable snapshot), and the
    /// pages that emit them settle within the throttle window. Intermediate
    /// progress while still catching up is a disposable estimate and may be
    /// dropped. A change in state variant (e.g. the transition out of
    /// `InitialSync`) is likewise always written.
    pub async fn update(
        &mut self,
        metadata: DataSyncProgressMetadata,
        min_write_interval: Duration,
        caught_up: bool,
    ) -> anyhow::Result<Option<DataSyncProgressMetadata>> {
        let existing = self.get(metadata.sync_id.as_str()).await?;
        if let Some(doc) = &existing {
            let elapsed_ms = metadata.last_updated_ms.saturating_sub(doc.last_updated_ms);
            let variant_changed =
                std::mem::discriminant(&doc.state) != std::mem::discriminant(&metadata.state);
            let progressed =
                metadata.state.num_documents_synced() != doc.state.num_documents_synced();
            let should_write = variant_changed
                || (caught_up && progressed)
                || elapsed_ms >= min_write_interval.as_millis() as u64;
            if !should_write {
                return Ok(existing.map(|doc| (*doc).clone().into_value()));
            }
        }
        let mut model = SystemMetadataModel::new_global(self.tx);
        match existing {
            Some(doc) => {
                let old = (*doc).clone().into_value();
                model.replace(doc.id(), metadata.try_into()?).await?;
                Ok(Some(old))
            },
            None => {
                model
                    .insert(&DATA_SYNC_PROGRESS_TABLE, metadata.try_into()?)
                    .await?;
                Ok(None)
            },
        }
    }

    /// One page of the progress rows of active syncs — those that completed a
    /// page within [`DATA_SYNC_ACTIVE_WINDOW`] — most recently updated first.
    /// Returns up to `limit` rows and, if the listing isn't exhausted, a
    /// cursor to resume from.
    pub async fn active_syncs(
        &mut self,
        now_ms: u64,
        cursor: Option<Cursor>,
        limit: usize,
    ) -> anyhow::Result<(
        Vec<ParsedDocument<DataSyncProgressMetadata>>,
        Option<Cursor>,
    )> {
        let cutoff_ms = now_ms.saturating_sub(DATA_SYNC_ACTIVE_WINDOW.as_millis() as u64);
        // The cutoff is applied in code rather than as an index bound: a
        // pagination cursor is only valid for a query with an identical
        // fingerprint, and a bound computed from the wall clock would change
        // between pages and invalidate it.
        let query = Query::index_range(IndexRange {
            index_name: DATA_SYNC_PROGRESS_INDEX_BY_LAST_UPDATED.name(),
            range: vec![],
            order: Order::Desc,
        });
        let mut query_stream = ResolvedQuery::new_bounded(
            self.tx,
            TableNamespace::Global,
            query,
            PaginationOptions::ManualPagination {
                start_cursor: cursor,
                maximum_rows_read: Some(limit),
                maximum_bytes_read: None,
            },
            None,
            TableFilter::IncludePrivateSystemTables,
        )?;

        let mut syncs = Vec::with_capacity(limit);
        let mut reached_stale = false;
        while syncs.len() < limit
            && let Some(document) = query_stream.next(self.tx, None).await?
        {
            let parsed: ParsedDocument<DataSyncProgressMetadata> = document.parse()?;
            if parsed.last_updated_ms < cutoff_ms {
                // Rows are ordered by `lastUpdatedMs` descending, so every
                // row after this one is stale too.
                reached_stale = true;
                break;
            }
            syncs.push(parsed);
        }
        let next_cursor = if reached_stale {
            None
        } else {
            match query_stream.cursor() {
                Some(cursor) if !matches!(cursor.position, CursorPosition::End) => Some(cursor),
                _ => None,
            }
        };
        Ok((syncs, next_cursor))
    }
}
