use anyhow::Context as _;
use common::{
    document::ParsedDocument,
    errors::report_error,
    execution_context::RequestMetadata,
    knobs::{
        DATA_SYNC_PROGRESS_WRITE_INTERVAL,
        DOCUMENT_DELTAS_LIMIT,
        SNAPSHOT_LIST_LIMIT,
    },
    runtime::Runtime,
};
use database::{
    streaming_export_selection::StreamingExportSelection,
    DocumentDeltas,
    SnapshotPage,
};
use errors::ErrorMetadata;
use keybroker::Identity;
use model::{
    backend_info::BackendInfoModel,
    data_sync_progress::{
        types::{
            DataSyncProgressMetadata,
            DataSyncState,
        },
        DataSyncProgressModel,
    },
    deployment_audit_log::types::DeploymentAuditLogEvent,
};
use streaming_export::{
    DataSyncClient,
    SyncCursor,
    SyncResult,
    SyncStatus,
};
use sync_types::Timestamp;
use value::ResolvedDocumentId;

use crate::Application;

const DEFAULT_LIST_ACTIVE_SYNCS_LIMIT: usize = 50;
const MAX_LIST_ACTIVE_SYNCS_LIMIT: usize = 100;

impl<RT: Runtime> Application<RT> {
    pub async fn ensure_streaming_export_enabled(&self, identity: Identity) -> anyhow::Result<()> {
        let mut tx = self.begin(identity).await?;
        BackendInfoModel::new(&mut tx)
            .ensure_streaming_export_enabled()
            .await
    }

    #[fastrace::trace]
    pub async fn document_deltas(
        &self,
        identity: Identity,
        cursor: Timestamp,
        selection: StreamingExportSelection,
    ) -> anyhow::Result<DocumentDeltas> {
        self.database
            .document_deltas(
                identity,
                Some(cursor),
                selection,
                *DOCUMENT_DELTAS_LIMIT,
                *DOCUMENT_DELTAS_LIMIT,
            )
            .await
    }

    #[fastrace::trace]
    pub async fn data_sync(
        &self,
        identity: Identity,
        cursor: Option<SyncCursor>,
        selection: StreamingExportSelection,
        sync_client: DataSyncClient,
        request_metadata: RequestMetadata,
    ) -> anyhow::Result<SyncResult> {
        let result = streaming_export::data_sync(
            &self.database.latest_database_snapshot()?,
            identity.clone(),
            cursor,
            selection,
            sync_client,
        )
        .await?;
        self.record_data_sync_progress(&result, identity, &request_metadata)
            .await?;
        Ok(result)
    }

    /// Mint a data sync cursor equivalent to a legacy `document_deltas` cursor,
    /// letting a consumer switch protocols without re-reading its data.
    #[fastrace::trace]
    pub async fn data_sync_cursor_from_deltas(
        &self,
        identity: Identity,
        cursor: Timestamp,
        selection: StreamingExportSelection,
        sync_client: DataSyncClient,
    ) -> anyhow::Result<SyncCursor> {
        streaming_export::data_sync_cursor_from_deltas(
            &self.database.latest_database_snapshot()?,
            identity,
            cursor,
            selection,
            sync_client,
        )
        .await
    }

    /// One page of the progress documents of active data syncs — those that
    /// fetched a page within the active window — most recently updated first.
    /// The returned cursor, if any, fetches the next page.
    pub async fn active_data_syncs(
        &self,
        identity: Identity,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> anyhow::Result<(
        Vec<ParsedDocument<DataSyncProgressMetadata>>,
        Option<String>,
    )> {
        let limit = limit.unwrap_or(DEFAULT_LIST_ACTIVE_SYNCS_LIMIT);
        if limit == 0 || limit > MAX_LIST_ACTIVE_SYNCS_LIMIT {
            anyhow::bail!(ErrorMetadata::bad_request(
                "LimitOutOfRange",
                format!(
                    "The limit for listing active syncs must be between 1 and \
                     {MAX_LIST_ACTIVE_SYNCS_LIMIT}"
                ),
            ));
        }
        let cursor = cursor
            .map(|cursor| self.key_broker().decrypt_cursor(cursor))
            .transpose()?;
        let now_ms = self.runtime.unix_timestamp().as_ms_since_epoch()?;
        let mut tx = self.begin(identity).await?;
        let (syncs, next_cursor) = DataSyncProgressModel::new(&mut tx)
            .active_syncs(now_ms, cursor, limit)
            .await?;
        let next_cursor = next_cursor.map(|cursor| self.key_broker().encrypt_cursor(&cursor));
        Ok((syncs, next_cursor))
    }

    /// The progress document of a single active data sync — one that fetched a
    /// page within the active window — or `None` if no such sync exists.
    pub async fn active_data_sync(
        &self,
        identity: Identity,
        sync_id: &str,
    ) -> anyhow::Result<Option<DataSyncProgressMetadata>> {
        let now_ms = self.runtime.unix_timestamp().as_ms_since_epoch()?;
        let mut tx = self.begin(identity).await?;
        DataSyncProgressModel::new(&mut tx)
            .active_sync(now_ms, sync_id)
            .await
    }

    /// Upsert this sync's `_data_sync_progress` document from the page's
    /// outcome.
    ///
    /// If the sync has no document yet, this page records its creation: the
    /// insert and its audit log entry are committed together and any
    /// failure fails the page, so a client can never advance past a
    /// creation that wasn't audit logged. Once the document exists,
    /// refreshes are best-effort: a failure (e.g. an OCC with a concurrent
    /// page of the same sync, or table summaries still bootstrapping) is
    /// reported without failing the page.
    async fn record_data_sync_progress(
        &self,
        result: &SyncResult,
        identity: Identity,
        request_metadata: &RequestMetadata,
    ) -> anyhow::Result<()> {
        let mut tx = self.begin(identity).await?;
        let existing = DataSyncProgressModel::new(&mut tx)
            .get(result.cursor.sync_id())
            .await?;
        // The async block scopes the upsert's `?`s so `existing` can pick the
        // error policy below.
        let recorded = async {
            let state = match &result.status {
                SyncStatus::Snapshotting { progress } => DataSyncState::Snapshotting {
                    num_tables_synced: progress.num_tables_synced,
                    total_tables: progress.total_tables,
                    current_component: progress.current_component.clone(),
                    current_table: progress.current_table.clone(),
                    num_documents_synced_in_current_table: progress.num_documents_in_current_table,
                    total_documents_in_current_table: progress
                        .total_documents_in_current_table
                        .context("table summaries are still bootstrapping")?,
                    num_documents_synced: progress.num_documents_synced,
                    total_documents: progress
                        .total_documents
                        .context("table summaries are still bootstrapping")?,
                },
                SyncStatus::Stale { ts } => DataSyncState::Stale {
                    total_tables: result.cursor.num_synced_tables(),
                    num_documents_synced: result.cursor.num_docs_synced(),
                    synced_ts: i64::from(*ts),
                },
                SyncStatus::UpToDate { ts } => DataSyncState::UpToDate {
                    total_tables: result.cursor.num_synced_tables(),
                    num_documents_synced: result.cursor.num_docs_synced(),
                    synced_ts: i64::from(*ts),
                },
            };
            let metadata = DataSyncProgressMetadata {
                sync_id: result.cursor.sync_id().to_string(),
                last_updated_ms: self.runtime.unix_timestamp().as_ms_since_epoch()?,
                state,
            };
            // A fully caught-up snapshot is the sync's settled progress; flush
            // it past the throttle so its final document count is recorded
            // even if a page wrote moments earlier.
            let caught_up = matches!(&result.status, SyncStatus::UpToDate { .. });
            let old = DataSyncProgressModel::new(&mut tx)
                .update(metadata, *DATA_SYNC_PROGRESS_WRITE_INTERVAL, caught_up)
                .await?;
            match old {
                None => {
                    self.commit_with_audit_log_events(
                        tx,
                        vec![DeploymentAuditLogEvent::CreateDataSync {
                            sync_id: result.cursor.sync_id().to_string(),
                        }],
                        request_metadata.clone(),
                        "data_sync_progress",
                    )
                    .await?;
                },
                Some(_) => {
                    // A throttled update leaves the transaction empty; skip
                    // the commit to avoid loading the DB with a no-op write.
                    if !tx.is_readonly() {
                        self.database
                            .commit_with_write_source(tx, "data_sync_progress")
                            .await?;
                    }
                },
            }
            anyhow::Ok(())
        }
        .await;
        match recorded {
            Ok(()) => Ok(()),
            // The sync's creation and its audit log entry must land; fail the
            // page so the client retries it.
            Err(e) if existing.is_none() => Err(e),
            Err(mut e) => {
                report_error(&mut e).await;
                Ok(())
            },
        }
    }

    #[fastrace::trace]
    pub async fn list_snapshot(
        &self,
        identity: Identity,
        snapshot: Option<Timestamp>,
        cursor: Option<ResolvedDocumentId>,
        selection: StreamingExportSelection,
    ) -> anyhow::Result<SnapshotPage> {
        self.database
            .list_snapshot(
                identity,
                snapshot,
                cursor,
                selection,
                *SNAPSHOT_LIST_LIMIT,
                *SNAPSHOT_LIST_LIMIT,
            )
            .await
    }
}
