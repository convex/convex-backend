use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    mem,
    ops::Deref,
    sync::Arc,
};

use anyhow::Context;
use async_trait::async_trait;
use common::{
    bootstrap_model::{
        index::{
            database_index::IndexedFields,
            IndexConfig,
            IndexMetadata,
            TabletIndexMetadata,
        },
        schema::SchemaState,
        tables::{
            TableMetadata,
            TABLES_TABLE,
        },
    },
    components::{
        ComponentId,
        ComponentPath,
    },
    document::{
        CreationTime,
        PendingDocument,
        PendingDocumentUpdate,
        ResolvedDocument,
    },
    identity::InertIdentity,
    index::{
        IndexKey,
        IndexKeyBytes,
    },
    interval::{
        Interval,
        IntervalSet,
    },
    knobs::{
        SEARCH_INDEX_SIZE_SOFT_LIMIT,
        TEXT_INDEX_SIZE_HARD_LIMIT,
        VECTOR_INDEX_SIZE_HARD_LIMIT,
        VECTOR_INDEX_SIZE_SOFT_LIMIT,
    },
    query::{
        CursorPosition,
        Order,
        Search,
        SearchVersion,
    },
    runtime::Runtime,
    schemas::DatabaseSchema,
    types::{
        GenericIndexName,
        IndexId,
        IndexName,
        RepeatableTimestamp,
        StableIndexName,
        TableName,
        TableStats,
        TabletIndexName,
        WriteTimestamp,
    },
    value::{
        id_v6::DeveloperDocumentId,
        ResolvedDocumentId,
        TableMapping,
    },
    virtual_system_mapping::VirtualSystemMapping,
};
use derive_more::{
    Deref,
    DerefMut,
};
use errors::ErrorMetadata;
use indexing::{
    database_index_snapshot::TimestampedIndexCache,
    index_reader::RangeRequest,
};
use keybroker::{
    Identity,
    UserIdentityAttributes,
};
use search::{
    metrics::SearchType,
    CandidateRevision,
    TextSearchResults,
};
use sync_types::{
    AuthenticationToken,
    Timestamp,
};
use tokio::task;
use usage_tracking::FunctionUsageTracker;
use value::{
    sha256::{
        Sha256,
        Sha256Digest,
    },
    PendingValue,
    TableNamespace,
    TableNumber,
    TabletId,
};

use crate::{
    bootstrap_model::{
        defaults::BootstrapTableIds,
        next_persistence_index_id::NextPersistenceIndexIdModel,
        table::{
            NUM_RESERVED_LEGACY_TABLE_NUMBERS,
            NUM_RESERVED_SYSTEM_TABLE_NUMBERS,
        },
    },
    committer::table_dependency_sort_key,
    execution_size::{
        FileStorageReadSize,
        FunctionExecutionSize,
        ScheduledFunctionsSize,
        TransactionLimits,
    },
    file_uploads::{
        FileUploads,
        PendingFileUpload,
        UploadedFileUpload,
    },
    metrics::{
        self,
        log_index_too_large_blocking_writes,
    },
    patch::PatchValue,
    preloaded::PreloadedIndexRange,
    query::{
        IndexRangeResponse,
        TableFilter,
    },
    reads::TransactionReadSet,
    schema_registry::SchemaRegistry,
    search_flusher_wake::SearchFlusherWakeSignals,
    snapshot_manager::Snapshot,
    table_summary::table_summary_bootstrapping_error,
    token::Token,
    transaction_id_generator::TransactionIdGenerator,
    transaction_index::TransactionIndex,
    write_limits::BiggestDocumentWrites,
    writes::{
        NestedWriteToken,
        NestedWrites,
        Writes,
    },
    ComponentRegistry,
    IndexModel,
    ReadSet,
    SchemaModel,
    SystemMetadataModel,
    TableModel,
    TableRegistry,
    TransactionReadSize,
    SCHEMAS_TABLE,
};

/// Safe default number of items to return for each list or filter operation
/// when we're writing internal code and don't know what other value to choose.
pub const DEFAULT_PAGE_SIZE: usize = 512;

pub const MAX_PAGE_SIZE: usize = 1024;
pub struct Transaction<RT: Runtime> {
    pub(crate) identity: Identity,
    pub(crate) id_generator: TransactionIdGenerator,

    pub(crate) next_creation_time: CreationTime,

    // Size of any functions scheduled from this transaction.
    pub scheduled_size: ScheduledFunctionsSize,

    // Size of the file storage accessed from this transaction.
    pub file_storage_read_size: FileStorageReadSize,

    pub(crate) file_uploads: NestedWrites<FileUploads>,

    // Transaction limits (reads, writes, scheduled). Defaults to global limits.
    pub(crate) limits: TransactionLimits,

    pub(crate) reads: TransactionReadSet,
    pub(crate) writes: NestedWrites<Writes>,

    pub(crate) index: NestedWrites<TransactionIndex>,
    pub(crate) metadata: NestedWrites<TableRegistry>,
    pub(crate) schema_registry: NestedWrites<SchemaRegistry>,
    pub(crate) component_registry: NestedWrites<ComponentRegistry>,
    pub(crate) count_snapshot: Arc<dyn TableCountSnapshot>,
    /// The change in the number of documents in table that have had writes in
    /// this transaction. If there is no entry for a table, assume deltas
    /// are zero.
    pub(crate) table_count_deltas: BTreeMap<TabletId, i64>,

    pub(crate) stats: BTreeMap<TabletId, TableStats>,

    pub(crate) runtime: RT,

    pub usage_tracker: FunctionUsageTracker,
    pub(crate) virtual_system_mapping: VirtualSystemMapping,

}

#[async_trait]
pub trait TableCountSnapshot: Send + Sync + 'static {
    /// Returns the number of documents in the table at the timestamp of the
    /// snapshot.
    async fn count(&self, table: TabletId) -> anyhow::Result<Option<u64>>;
}

pub struct SubtransactionToken {
    writes: NestedWriteToken,
    index: NestedWriteToken,
    tables: NestedWriteToken,
    schema_registry: NestedWriteToken,
    component_registry: NestedWriteToken,
    file_uploads: NestedWriteToken,
    /// Parent's transaction limits, restored when the subtransaction
    /// commits or rolls back.
    limits: TransactionLimits,
    /// Parent's table count deltas, restored when the subtransaction rolls
    /// back.
    table_count_deltas: BTreeMap<TabletId, i64>,
}

impl<RT: Runtime> Transaction<RT> {
    pub fn new(
        identity: Identity,
        id_generator: TransactionIdGenerator,
        creation_time: CreationTime,
        index: TransactionIndex,
        metadata: TableRegistry,
        schema_registry: SchemaRegistry,
        component_registry: ComponentRegistry,
        count: Arc<dyn TableCountSnapshot>,
        runtime: RT,
        usage_tracker: FunctionUsageTracker,
        virtual_system_mapping: VirtualSystemMapping,
    ) -> Self {
        Self {
            identity,
            reads: TransactionReadSet::new(),
            writes: NestedWrites::new(Writes::new()),
            id_generator,
            next_creation_time: creation_time,
            scheduled_size: ScheduledFunctionsSize::default(),
            file_storage_read_size: FileStorageReadSize::default(),
            file_uploads: NestedWrites::new(FileUploads::default()),
            limits: TransactionLimits::default(),
            index: NestedWrites::new(index),
            metadata: NestedWrites::new(metadata),
            schema_registry: NestedWrites::new(schema_registry),
            component_registry: NestedWrites::new(component_registry),
            count_snapshot: count,
            table_count_deltas: BTreeMap::new(),
            stats: BTreeMap::new(),
            runtime,
            usage_tracker,
            virtual_system_mapping,
        }
    }

    pub fn table_mapping(&mut self) -> &TableMapping {
        self.take_table_mapping_dep();
        self.metadata.table_mapping()
    }

    pub fn resolve_developer_id(
        &mut self,
        id: &DeveloperDocumentId,
        namespace: TableNamespace,
    ) -> anyhow::Result<ResolvedDocumentId> {
        id.to_resolved(self.table_mapping().namespace(namespace).number_to_tablet())
    }

    pub fn virtual_system_mapping(&self) -> &VirtualSystemMapping {
        &self.virtual_system_mapping
    }

    pub(crate) async fn assign_missing_persistence_index_ids(&mut self) -> anyhow::Result<()> {
        let writes = self.writes.as_flat()?;
        let mut missing_index_id_updates = Vec::new();
        for id in writes.new_index_document_ids() {
            let update = writes
                .get(&id)
                .expect("tracked index metadata insert must have a pending write");
            let Some(new_document) = &update.new_document else {
                unreachable!("tracked index metadata insert must have a new document")
            };
            let PendingDocument::Concrete(document) = new_document else {
                anyhow::bail!("new index metadata must be concrete before commit")
            };
            let metadata = TabletIndexMetadata::from_document(document.clone())?;
            if matches!(
                &metadata.config,
                IndexConfig::Database {
                    persistence_index_id: None,
                    ..
                }
            ) {
                missing_index_id_updates.push((update.id, metadata));
            }
        }
        if missing_index_id_updates.is_empty() {
            return Ok(());
        }
        let persistence_index_ids = NextPersistenceIndexIdModel::new(self)
            .allocate(missing_index_id_updates.len())
            .await
            .context("Failed to allocate persistence index IDs")?;

        for ((id, mut metadata), persistence_index_id) in missing_index_id_updates
            .into_iter()
            .zip(persistence_index_ids)
        {
            metadata.assign_persistence_index_id(persistence_index_id);

            SystemMetadataModel::new_global(self)
                .replace(id, metadata.into_value().try_into()?)
                .await?;
        }
        Ok(())
    }

    /// Checks both virtual tables and tables to get the table number to name
    /// mapping. If table is excluded by `table_filter`, returns error as if
    /// the table doesn't exist.
    pub fn all_tables_number_to_name(
        &mut self,
        namespace: TableNamespace,
        table_filter: TableFilter,
    ) -> impl Fn(TableNumber) -> anyhow::Result<TableName> + '_ {
        let table_mapping = self.table_mapping().namespace(namespace);
        let virtual_system_mapping = self.virtual_system_mapping().clone();
        move |number| {
            let name = table_mapping.number_to_name()(number)?;
            if let Some(virtual_name) =
                virtual_system_mapping.primary_system_to_virtual_table(&name)
            {
                Ok(virtual_name.clone())
            } else {
                match table_filter {
                    TableFilter::IncludePrivateSystemTables => {},
                    TableFilter::ExcludePrivateSystemTables => {
                        anyhow::ensure!(!name.is_system(), "cannot find table {number:?}");
                    },
                }
                Ok(name)
            }
        }
    }

    pub fn bootstrap_tables(&mut self) -> BootstrapTableIds {
        BootstrapTableIds::new(self.table_mapping())
    }

    pub fn resolve_idv6(
        &mut self,
        id: DeveloperDocumentId,
        namespace: TableNamespace,
        table_filter: TableFilter,
    ) -> anyhow::Result<TableName> {
        match self.all_tables_number_to_name(namespace, table_filter)(id.table()) {
            Ok(table_name) => Ok(table_name),
            Err(_) => anyhow::bail!("Table for ID \"{}\" not found", id.encode()),
        }
    }

    pub fn runtime(&self) -> &RT {
        &self.runtime
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn inert_identity(&self) -> InertIdentity {
        self.identity.clone().into()
    }

    pub fn user_identity(&self) -> Option<UserIdentityAttributes> {
        match &self.identity {
            Identity::User(identity) => Some((*identity.attributes).clone()),
            Identity::ActingUser(_, identity) => Some((**identity).clone()),
            _ => None,
        }
    }

    pub fn authentication_token(&self) -> AuthenticationToken {
        self.identity.clone().into()
    }

    pub fn begin_timestamp(&self) -> RepeatableTimestamp {
        self.index.base_snapshot().timestamp()
    }

    pub fn next_creation_time(&self) -> CreationTime {
        self.next_creation_time
    }

    pub fn index_registry(&self) -> &indexing::index_registry::IndexRegistry {
        self.index.index_registry()
    }

    pub fn is_readonly(&self) -> bool {
        self.writes.is_empty()
    }

    pub fn begin_subtransaction(&mut self) -> SubtransactionToken {
        SubtransactionToken {
            writes: self.writes.begin_nested(),
            index: self.index.begin_nested(),
            tables: self.metadata.begin_nested(),
            schema_registry: self.schema_registry.begin_nested(),
            component_registry: self.component_registry.begin_nested(),
            file_uploads: self.file_uploads.begin_nested(),
            limits: self.limits.clone(),
            table_count_deltas: self.table_count_deltas.clone(),
        }
    }

    pub fn commit_subtransaction(&mut self, tokens: SubtransactionToken) -> anyhow::Result<()> {
        self.writes.commit_nested(tokens.writes)?;
        self.index.commit_nested(tokens.index)?;
        self.metadata.commit_nested(tokens.tables)?;
        self.schema_registry.commit_nested(tokens.schema_registry)?;
        self.component_registry
            .commit_nested(tokens.component_registry)?;
        self.file_uploads.commit_nested(tokens.file_uploads)?;
        self.limits = tokens.limits;
        Ok(())
    }

    pub fn rollback_subtransaction(&mut self, tokens: SubtransactionToken) -> anyhow::Result<()> {
        self.writes.rollback_nested(tokens.writes)?;
        self.index.rollback_nested(tokens.index)?;
        self.metadata.rollback_nested(tokens.tables)?;
        self.schema_registry
            .rollback_nested(tokens.schema_registry)?;
        self.component_registry
            .rollback_nested(tokens.component_registry)?;
        self.file_uploads.rollback_nested(tokens.file_uploads)?;
        self.limits = tokens.limits;
        self.table_count_deltas = tokens.table_count_deltas;
        Ok(())
    }

    pub fn require_not_nested(&self) -> anyhow::Result<()> {
        self.writes.require_not_nested()?;
        self.index.require_not_nested()?;
        self.metadata.require_not_nested()?;
        self.schema_registry.require_not_nested()?;
        self.component_registry.require_not_nested()?;
        self.file_uploads.require_not_nested()?;
        Ok(())
    }

    pub fn writes(&self) -> &NestedWrites<Writes> {
        &self.writes
    }

    pub fn into_reads_and_writes(self) -> (TransactionReadSet, NestedWrites<Writes>) {
        (self.reads, self.writes)
    }

    pub fn biggest_document_writes(&self) -> Option<BiggestDocumentWrites> {
        let mut max_size = 0;
        let mut biggest_document_id = None;
        let mut max_nesting = 0;
        let mut most_nested_document_id = None;
        for update in self.writes.coalesced_writes() {
            let document_id = update.id_ref();
            let (size, nesting) = update.new_document_size_and_nesting().unwrap_or_default();
            if size > max_size {
                max_size = size;
                biggest_document_id = Some(document_id);
            }
            if nesting > max_nesting {
                max_nesting = nesting;
                most_nested_document_id = Some(document_id);
            }
        }
        if let Some(biggest_document_id) = biggest_document_id
            && let Some(most_nested_document_id) = most_nested_document_id
        {
            Some(BiggestDocumentWrites {
                max_size: ((*biggest_document_id).into(), max_size),
                max_nesting: ((*most_nested_document_id).into(), max_nesting),
            })
        } else {
            None
        }
    }

    pub fn execution_size(&self) -> FunctionExecutionSize {
        FunctionExecutionSize {
            num_intervals: self.reads.num_intervals(),
            read_size: self.reads.user_tx_size().to_owned(),
            write_size: self.writes.user_size().to_owned(),
            scheduled_size: self.scheduled_size.clone(),
            file_storage_write_size: self.file_uploads.size(),
            file_storage_read_size: self.file_storage_read_size.clone(),
        }
    }

    /// Records the contents of a file whose `_file_storage` row `id` this
    /// transaction just wrote.
    pub fn add_pending_file_upload(
        &mut self,
        id: ResolvedDocumentId,
        upload: PendingFileUpload,
    ) -> anyhow::Result<()> {
        self.file_uploads.add_pending(id, upload)
    }

    /// Forgets the file whose `_file_storage` row `id` this transaction just
    /// deleted, if it was one the transaction stored.
    pub fn remove_pending_file_upload(&mut self, id: &ResolvedDocumentId) -> anyhow::Result<()> {
        self.file_uploads.remove_pending(id)
    }

    pub fn get_pending_file_uploads(
        &self,
    ) -> anyhow::Result<Vec<(ResolvedDocumentId, PendingFileUpload)>> {
        self.file_uploads.pending_files()
    }

    /// The contents of the file whose `_file_storage` document `id` this
    /// transaction stored, if it stored one.
    pub fn get_pending_file_upload(
        &self,
        id: &ResolvedDocumentId,
    ) -> anyhow::Result<Option<PendingFileUpload>> {
        Ok(self.file_uploads.pending_file(id)?.cloned())
    }

    pub fn add_file_storage_read(&mut self, size: usize) {
        self.file_storage_read_size.num_reads += 1;
        self.file_storage_read_size.size += size;
    }

    pub fn mark_files_uploaded(&mut self) -> anyhow::Result<()> {
        self.file_uploads.require_not_nested()?;
        self.file_uploads.mark_uploaded();
        Ok(())
    }

    pub fn discard_pending_file_uploads(&mut self) -> anyhow::Result<()> {
        self.file_uploads.require_not_nested()?;
        self.file_uploads.discard_pending();
        Ok(())
    }

    pub fn get_uploaded_files(
        &self,
    ) -> anyhow::Result<BTreeMap<ResolvedDocumentId, UploadedFileUpload>> {
        self.file_uploads.require_not_nested()?;
        self.file_uploads.uploaded_files()
    }

    /// Returns the transaction limits.
    pub fn transaction_limits(&self) -> &TransactionLimits {
        &self.limits
    }

    /// Apply a per-call budget on top of the transaction's current usage,
    /// clamped by the transaction's existing limits. The argument is
    /// interpreted as "this call may use up to N additional bytes/rows/etc.";
    /// the result is an absolute ceiling that can only tighten the current
    /// limits, never loosen them.
    pub fn set_transaction_limits(&mut self, budget: TransactionLimits) {
        let usage = self.execution_size();
        self.limits = TransactionLimits::from_budget(budget, &usage, &self.limits);
    }

    pub fn user_tx_read_size(&self) -> &TransactionReadSize {
        self.reads.user_tx_size()
    }

    /// Applies the reads and writes from FunctionRunner to the Transaction.
    #[fastrace::trace]
    pub fn apply_function_runner_tx(
        &mut self,
        begin_timestamp: Timestamp,
        reads: ReadSet,
        num_intervals: usize,
        user_tx_size: crate::reads::TransactionReadSize,
        system_tx_size: crate::reads::TransactionReadSize,
        updates: Vec<PendingDocumentUpdate>,
        rows_read_by_tablet: BTreeMap<TabletId, u64>,
        uploaded_files: BTreeMap<ResolvedDocumentId, UploadedFileUpload>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            *self.begin_timestamp() == begin_timestamp,
            "Timestamp mismatch"
        );

        self.reads
            .merge(reads, num_intervals, user_tx_size, system_tx_size);

        self.merge_writes(updates)?;
        self.file_uploads.add_uploaded(uploaded_files)?;

        for (tablet_id, rows_read) in rows_read_by_tablet {
            self.stats.entry(tablet_id).or_default().rows_read += rows_read;
        }

        Ok(())
    }

    // Checks that if this transaction already has some writes, they are included
    // in the given `updates`. This means the passed in `updates` are a superset
    // of the existing `updates` on this transaction, and that the merged-in
    // writes cannot modify documents already written to in this transaction.
    // In most scenarios this transaction will have no writes.
    pub fn merge_writes(
        &mut self,
        updates: impl IntoIterator<Item = PendingDocumentUpdate>,
    ) -> anyhow::Result<()> {
        let existing_updates = self.writes().as_flat()?.clone().into_updates();

        let mut updates = updates.into_iter().collect::<Vec<_>>();
        let bootstrap_tables = self.bootstrap_tables();
        updates.sort_by_cached_key(|update| {
            table_dependency_sort_key(
                bootstrap_tables,
                update.id.into(),
                // Only bootstrap-table (`_tables`/`_index`) metadata affects
                // the sort key, and those documents are system-constructed and
                // always concrete.
                update
                    .new_document
                    .as_ref()
                    .and_then(PendingDocument::as_concrete),
            )
        });

        let mut preserved_update_count = 0;
        for update in updates {
            let id = update.id;
            // Ensure that the existing update matches, and that
            // that the merged-in writes didn't otherwise modify documents
            // already written to in this transaction.
            if let Some(existing_update) = existing_updates.get(&id) {
                anyhow::ensure!(
                    **existing_update == update,
                    "Conflicting updates for document {id}"
                );
                preserved_update_count += 1;
                continue;
            }

            if let Some(ref document) = update.new_document {
                let doc_creation_time = document.creation_time();
                if doc_creation_time >= self.next_creation_time {
                    self.next_creation_time = doc_creation_time;
                    self.next_creation_time.increment()?;
                }
            }
            self.apply_validated_write(
                id,
                update
                    .old_document
                    .map(|(d, ts)| (d, WriteTimestamp::Committed(ts))),
                update.new_document,
            )?;
        }
        assert_eq!(
            preserved_update_count,
            existing_updates.len(),
            "Existing write was not preserved"
        );

        Ok(())
    }

    /// Return the document with the given `id` or None if document doesn't
    /// exist.
    pub async fn get(
        &mut self,
        id: ResolvedDocumentId,
    ) -> anyhow::Result<Option<ResolvedDocument>> {
        Ok(self.get_with_ts(id).await?.map(|(doc, _)| doc))
    }

    #[fastrace::trace]
    #[convex_macro::instrument_future]
    pub async fn get_with_ts(
        &mut self,
        id: ResolvedDocumentId,
    ) -> anyhow::Result<Option<(ResolvedDocument, WriteTimestamp)>> {
        task::consume_budget().await;
        let table_name = match self.table_mapping().tablet_name(id.tablet_id) {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };
        if self.virtual_system_mapping().is_virtual_table(&table_name) {
            anyhow::bail!("Virtual tables should use UserFacingModel::get_with_ts");
        }
        self.get_inner(id, table_name).await
    }

    #[convex_macro::instrument_future]
    pub(crate) async fn patch_inner(
        &mut self,
        id: ResolvedDocumentId,
        value: PatchValue,
    ) -> anyhow::Result<PendingDocument> {
        task::consume_budget().await;

        let table_name = self.table_mapping().tablet_name(id.tablet_id)?;
        let namespace = self.table_mapping().tablet_namespace(id.tablet_id)?;

        let (old_document, old_ts) =
            self.get_inner(id, table_name.clone())
                .await?
                .context(ErrorMetadata::bad_request(
                    "NonexistentDocument",
                    format!("Update on nonexistent document ID {id}"),
                ))?;

        // Merge into the staged pending body when there is one, so unresolved
        // commit timestamps the patch doesn't touch stay unresolved.
        let old_pending = self.old_pending_document(&old_document, old_ts)?;
        let new_body = value.apply(old_pending.clone().into_pending_value())?;
        let new_document = PendingDocument::new(id, old_document.creation_time(), new_body)?;
        if new_document == old_pending {
            return Ok(old_pending);
        }
        let new_document_view = new_document.to_document_with_max_commit_ts()?.into_owned();
        SchemaModel::new(self, namespace)
            .enforce(&new_document_view)
            .await?;

        self.apply_validated_write(id, Some((old_document, old_ts)), Some(new_document.clone()))?;
        Ok(new_document)
    }

    /// The current revision of a document as a [`PendingDocument`]: the staged
    /// write when `old_ts` is pending, the committed document otherwise.
    fn old_pending_document(
        &self,
        old_document: &ResolvedDocument,
        old_ts: WriteTimestamp,
    ) -> anyhow::Result<PendingDocument> {
        match old_ts {
            WriteTimestamp::Pending => self
                .pending_write(&old_document.id())
                .and_then(|update| update.new_document.clone())
                .context("Old document timestamp is Pending, but there is no pending write"),
            WriteTimestamp::Committed(_) => Ok(old_document.clone().into()),
        }
    }

    pub fn is_system(&mut self, namespace: TableNamespace, table_number: TableNumber) -> bool {
        let tablet_id =
            self.table_mapping().namespace(namespace).number_to_tablet()(table_number).ok();
        tablet_id.is_some_and(|id| self.table_mapping().is_system_tablet(id))
    }

    #[convex_macro::instrument_future]
    pub(crate) async fn replace_inner(
        &mut self,
        id: ResolvedDocumentId,
        value: impl Into<PendingValue> + Send,
    ) -> anyhow::Result<PendingDocument> {
        task::consume_budget().await;

        let table_name = self.table_mapping().tablet_name(id.tablet_id)?;
        let namespace = self.table_mapping().tablet_namespace(id.tablet_id)?;
        let (old_document, old_ts) =
            self.get_inner(id, table_name)
                .await?
                .context(ErrorMetadata::bad_request(
                    "NonexistentDocument",
                    format!("Replace on nonexistent document ID {id}"),
                ))?;

        // Replace document.
        let new_document = PendingDocument::new(id, old_document.creation_time(), value.into())?;
        if new_document == self.old_pending_document(&old_document, old_ts)? {
            return Ok(new_document);
        }
        let new_document_view = new_document.to_document_with_max_commit_ts()?.into_owned();
        SchemaModel::new(self, namespace)
            .enforce(&new_document_view)
            .await?;

        self.apply_validated_write(id, Some((old_document, old_ts)), Some(new_document.clone()))?;
        Ok(new_document)
    }

    #[convex_macro::instrument_future]
    pub async fn delete_inner(
        &mut self,
        id: ResolvedDocumentId,
    ) -> anyhow::Result<ResolvedDocument> {
        task::consume_budget().await;

        let table_name = self.table_mapping().tablet_name(id.tablet_id)?;
        let (document, ts) =
            self.get_inner(id, table_name)
                .await?
                .context(ErrorMetadata::bad_request(
                    "NonexistentDocument",
                    format!("Delete on nonexistent document ID {id}"),
                ))?;

        self.apply_validated_write(document.id(), Some((document.clone(), ts)), None)?;
        Ok(document)
    }

    #[fastrace::trace]
    #[convex_macro::instrument_future]
    pub async fn count(
        &mut self,
        namespace: TableNamespace,
        table: &TableName,
    ) -> anyhow::Result<Option<u64>> {
        let virtual_system_mapping = self.virtual_system_mapping().clone();
        let system_table = if virtual_system_mapping.is_virtual_table(table) {
            virtual_system_mapping.virtual_to_system_table(table)?
        } else {
            table
        };
        TableModel::new(self).count(namespace, system_table).await
    }

    #[fastrace::trace]
    #[convex_macro::instrument_future]
    pub async fn must_count(
        &mut self,
        namespace: TableNamespace,
        table: &TableName,
    ) -> anyhow::Result<u64> {
        TableModel::new(self)
            .count(namespace, table)
            .await?
            .ok_or_else(|| {
                table_summary_bootstrapping_error(Some(
                    "Table count unavailable while bootstrapping",
                ))
            })
    }

    pub fn into_token(self) -> anyhow::Result<Token> {
        anyhow::ensure!(self.is_readonly(), "Transaction isn't readonly");
        metrics::log_read_tx(&self);
        let ts = *self.begin_timestamp();
        Ok(Token::new(Arc::new(self.reads.into_read_set()), ts))
    }

    pub fn into_token_and_index_cache(self) -> anyhow::Result<(Token, TimestampedIndexCache)> {
        anyhow::ensure!(self.is_readonly(), "Transaction isn't readonly");
        metrics::log_read_tx(&self);
        let ts = self.begin_timestamp();
        let read_set = self.reads.into_read_set();
        // Reused cache only needs the intervals actually read by the transaction for
        // the next use. Specifically, data that gets copied to the by_id index can be
        // dropped. This is important for bounding the growth of the cache.
        let keep: BTreeMap<IndexId, IntervalSet> = read_set
            .iter_indexed()
            .filter_map(|(index_name, reads)| {
                self.index
                    .index_registry()
                    .get_enabled(index_name)
                    .map(|index| (index.id(), reads.intervals.clone()))
            })
            .collect();
        let mut cache = self.index.into_flat()?.into_cache();
        cache.retain_read_intervals(&keep);
        let token = Token::new(Arc::new(read_set), *ts);
        Ok((token, TimestampedIndexCache { cache, ts }))
    }

    pub fn take_stats(&mut self) -> BTreeMap<TableName, TableStats> {
        let stats = mem::take(&mut self.stats);
        stats
            .into_iter()
            .map(|(tablet, stats)| {
                (
                    match self.table_mapping().tablet_name(tablet) {
                        Ok(name) => name,
                        Err(_) => {
                            // This is unusual, but possible if the tablet was created in a
                            // subtransaction that was rolled back. Such a tablet never gets
                            // created, but might still have usage stats.
                            tracing::warn!("Tablet {tablet} does not exist");
                            // It's fine to return "_unknown" here because nothing requires
                            // these to correspond to an actual table.
                            //
                            // We use "_unknown" to avoid colliding with valid user table names.
                            "_unknown"
                                .parse()
                                .expect("'_unknown' should be a valid table name")
                        },
                    },
                    stats,
                )
            })
            .collect()
    }

    pub fn stats_by_tablet(&self) -> &BTreeMap<TabletId, TableStats> {
        &self.stats
    }

    pub(crate) fn take_table_mapping_dep(&mut self) {
        let tables_by_id = TabletIndexName::by_id(
            self.metadata
                .table_mapping()
                .namespace(TableNamespace::Global)
                .id(&TABLES_TABLE)
                .expect("_tables should exist")
                .tablet_id,
        );
        self.reads
            .record_indexed_derived(tables_by_id, IndexedFields::by_id(), Interval::all());
    }

    /// Reads the schema from the cache, and records a read dependency.
    /// Used by SchemaModel.
    pub(crate) fn get_schema_by_state(
        &mut self,
        namespace: TableNamespace,
        state: SchemaState,
    ) -> anyhow::Result<Option<(ResolvedDocumentId, Arc<DatabaseSchema>)>> {
        if !self
            .table_mapping()
            .namespace(namespace)
            .name_exists(&SCHEMAS_TABLE)
        {
            return Ok(None);
        }
        let schema_tablet = self
            .table_mapping()
            .namespace(namespace)
            .id(&SCHEMAS_TABLE)?
            .tablet_id;
        self.schema_registry
            .get_by_state(namespace, state, schema_tablet, &mut self.reads)
    }

    pub fn get_component_path(&mut self, component_id: ComponentId) -> Option<ComponentPath> {
        self.component_registry
            .get_component_path(component_id, &mut self.reads)
    }

    /// Resolves the path without recording the lookup in this transaction's
    /// read set, for logging and diagnostics only.
    pub fn get_component_path_untracked(&self, component_id: ComponentId) -> Option<ComponentPath> {
        self.component_registry
            .get_component_path(component_id, &mut TransactionReadSet::new())
    }

    pub fn must_component_path(
        &mut self,
        component_id: ComponentId,
    ) -> anyhow::Result<ComponentPath> {
        self.component_registry
            .must_component_path(component_id, &mut self.reads)
    }

    /// Get the component path for a tablet. This might be None when table
    /// namespaces for new components are created in `start_push`,  but
    /// components have not yet been created.
    pub fn component_path_for_tablet_id(
        &mut self,
        id: TabletId,
    ) -> anyhow::Result<Option<ComponentPath>> {
        let table_namespace = self.table_mapping().tablet_namespace(id)?;
        let component_id = ComponentId::from(table_namespace);
        Ok(self
            .component_registry
            .get_component_path(component_id, &mut self.reads))
    }

    // XXX move to table model?
    async fn table_number_for_system_table(
        &mut self,
        namespace: TableNamespace,
        table_name: &TableName,
        default_table_number: Option<TableNumber>,
    ) -> anyhow::Result<TableNumber> {
        Ok(if let Some(default_table_number) = default_table_number {
            let existing_name =
                self.table_mapping().namespace(namespace).number_to_name()(default_table_number)
                    .ok();
            let table_number = if let Some(existing_name) = existing_name {
                if self.virtual_system_mapping.is_virtual_table(table_name)
                    && *self
                        .virtual_system_mapping
                        .virtual_to_system_table(table_name)?
                        == existing_name
                {
                    // Table number is occupied by the system table for the virtual table we're
                    // creating. Allow it.
                    default_table_number
                } else if cfg!(any(test, feature = "testing")) {
                    // In tests, have a hard failure on conflicting default table numbers. In
                    // real system, have a looser fallback where we pick
                    // another table number.
                    anyhow::bail!(
                        "{default_table_number} is used by both {table_name} and {existing_name}"
                    );
                } else {
                    // If the table_number requested is taken, just pick a higher table number.
                    // This might be true for older backends that have lower-numbered system
                    // tables.
                    TableModel::new(self)
                        .next_system_table_number(namespace)
                        .await?
                }
            } else {
                default_table_number
            };
            // TODO(CX-6699) handle system table number exhaustion.
            anyhow::ensure!(
                table_number < TableNumber::try_from(NUM_RESERVED_SYSTEM_TABLE_NUMBERS)?,
                "{table_number} picked for system table {table_name} is reserved for user tables"
            );
            anyhow::ensure!(
                table_number >= TableNumber::try_from(NUM_RESERVED_LEGACY_TABLE_NUMBERS)?,
                "{table_number} picked for system table {table_name} is reserved for legacy tables"
            );
            table_number
        } else {
            TableModel::new(self)
                .next_system_table_number(namespace)
                .await?
        })
    }

    /// Creates a new system table, with _id and _creationTime indexes, returns
    /// false if table already existed
    pub async fn create_system_table(
        &mut self,
        namespace: TableNamespace,
        table_name: &TableName,
        default_table_number: Option<TableNumber>,
    ) -> anyhow::Result<bool> {
        anyhow::ensure!(self.identity().is_system());
        anyhow::ensure!(
            table_name.is_system(),
            "{table_name:?} is not a valid system table name!"
        );

        let is_new = !TableModel::new(self).table_exists(namespace, table_name);
        if is_new {
            let table_number = self
                .table_number_for_system_table(namespace, table_name, default_table_number)
                .await?;
            let metadata = TableMetadata::new(namespace, table_name.clone(), table_number);
            SystemMetadataModel::new_global(self)
                .insert(&TABLES_TABLE, metadata.try_into()?)
                .await?;
            let by_id = GenericIndexName::by_id(table_name.clone());
            let by_id_index = IndexMetadata::new_enabled(by_id, IndexedFields::by_id());
            IndexModel::new(self)
                .add_system_index(namespace, by_id_index)
                .await?;
            let by_creation_time = GenericIndexName::by_creation_time(table_name.clone());
            let metadata =
                IndexMetadata::new_enabled(by_creation_time, IndexedFields::creation_time());
            IndexModel::new(self)
                .add_system_index(namespace, metadata)
                .await?;
            tracing::info!("Created system table: {table_name}");
        } else {
            tracing::debug!("Skipped creating system table {table_name} since it already exists");
        };
        Ok(is_new)
    }

    /// Compute a hashed representation of the documents matching the given
    /// `IntervalSet` in the index `index_name`. The hash is just the sha256 of
    /// the timestamps of all the matching documents, in index order.
    ///
    /// This hash guarantees that if `tx1.hash_index_interval(index_name,
    /// intervals) == tx2.hash_index_interval(index_name, intervals)` (and both
    /// are Some), then all reads within `intervals` on `index_name` agree
    /// between `tx1` and `tx2`.
    ///
    /// The argument is as follows:
    /// - Without loss of generality, assume `tx1.ts < tx2.ts`.
    /// - Suppose `tx1` and `tx2` disagree on some index key `key` within
    ///   `intervals`.
    /// - Consider `tx2.query(index_name, key)`:
    ///   - If it is `Some(rev2)`, then `rev2.ts > tx1.ts` (or else tx1 would
    ///     observe it). In that case, the hash input to
    ///     `tx2.hash_index_interval(..)` contains `rev2.ts`, which is greater
    ///     than any timestamp hashed in `tx1.hash_index_interval(..)`; so the
    ///     two must differ.
    ///   - Otherwise, it is `None`, in which case `tx1.query(index_name, key)`
    ///     must be `Some(rev1)`. Then `tx2.hash_index_interval(..)`'s hash
    ///     input has fewer(*) occurrences of `rev1.ts` compared to
    ///     `tx1.hash_index_interval(..)`, so again the two must differ.
    ///     - (*) because updates can only write _new_ timestamps, not old ones.
    ///
    /// Returns None if the interval includes any pending writes.
    ///
    /// This method doesn't update `self`'s read set.
    pub async fn hash_index_interval_no_deps(
        &mut self,
        index_name: &TabletIndexName,
        // just for error messages
        table_name: &TableName,
        intervals: &IntervalSet,
    ) -> anyhow::Result<Option<Sha256Digest>> {
        let mut index_name = index_name.clone();
        let mut printable_index_name = index_name
            .clone()
            .map_table(&|_| Ok::<_, !>(table_name.clone()))?;
        let mut sha256 = Sha256::new();
        for mut interval in intervals.iter() {
            if self
                .index
                .pending_iter_for_interval(&index_name, &printable_index_name, &interval)?
                .next()
                .is_some()
            {
                // This range contains pending writes (possibly including deletes)
                return Ok(None);
            }
            while !interval.is_empty() {
                let request = RangeRequest {
                    index_name,
                    printable_index_name,
                    order: Order::Asc,
                    interval,
                    max_size: 100,
                };
                let Ok([result]): Result<[_; 1], _> = self
                    .index
                    .base_snapshot_mut()
                    .range_batch(&[&request])
                    .await
                    .try_into()
                else {
                    anyhow::bail!("returned wrong batch size")
                };
                let (docs, cursor) = result?;
                RangeRequest {
                    index_name,
                    printable_index_name,
                    ..
                } = request;
                (_, interval) = request.interval.split(cursor, request.order);
                for (_index_key, ts, _doc) in docs {
                    sha256.update(&u64::to_le_bytes(ts.into()));
                }
            }
        }
        Ok(Some(sha256.finalize()))
    }

    pub fn apply_reads(&mut self, reads: TransactionReadSet) {
        let num_intervals = reads.num_intervals();
        let user_tx_size = reads.user_tx_size().clone();
        let system_tx_size = reads.system_tx_size().clone();
        self.reads.merge(
            reads.into_read_set(),
            num_intervals,
            user_tx_size,
            system_tx_size,
        );
    }

    pub fn has_pending_write(&self, tablet_id: TabletId) -> bool {
        self.index
            .pending_writes_for_by_id_index(tablet_id)
            .is_some_and(|map| !map.is_empty())
    }
}

// Private methods for `Transaction`: Place all authorization checks closer to
// the public interface.
impl<RT: Runtime> Transaction<RT> {
    pub(crate) async fn get_inner(
        &mut self,
        id: ResolvedDocumentId,
        table_name: TableName,
    ) -> anyhow::Result<Option<(ResolvedDocument, WriteTimestamp)>> {
        let index_name = TabletIndexName::by_id(id.tablet_id);
        let printable_index_name = IndexName::by_id(table_name.clone());
        let index_key = IndexKey::new(vec![], id.into());
        let interval = Interval::singleton(index_key.to_bytes().into());
        let range_request = RangeRequest {
            index_name: index_name.clone(),
            printable_index_name,
            interval: interval.clone(),
            order: Order::Asc,
            // Request 2 to best-effort verify uniqueness of by_id index.
            max_size: 2,
        };

        let [result] = self
            .index
            .range_batch(&[&range_request])
            .await
            .try_into()
            .map_err(|_| anyhow::anyhow!("expected result"))?;
        self.reads.record_indexed_directly(
            index_name,
            IndexedFields::by_id(),
            interval,
            &self.limits,
        )?;
        let IndexRangeResponse {
            page: range_results,
            cursor,
        } = result?;
        if range_results.len() > 1 {
            Err(anyhow::anyhow!("Got multiple values for id {id:?}"))?;
        }
        if !matches!(cursor, CursorPosition::End) {
            Err(anyhow::anyhow!(
                "Querying 2 items for a single id didn't exhaust interval for {id:?}"
            ))?;
        }
        let result = match range_results.into_iter().next() {
            Some((_, doc, timestamp)) => {
                let component_path = self
                    .component_path_for_tablet_id(id.tablet_id)?
                    .unwrap_or_default();
                self.reads.record_read_document(
                    component_path,
                    table_name,
                    doc.size(),
                    &self.usage_tracker,
                    &self.virtual_system_mapping,
                    &self.limits,
                )?;

                Some((doc.unpack(), timestamp))
            },
            None => None,
        };
        self.stats.entry(id.tablet_id).or_default().rows_read += 1;
        Ok(result)
    }

    /// Apply a validated write to the [Transaction], updating the
    /// [IndexRegistry] and [TableRegistry]. Validated means the write
    /// has already been checked for schema enforcement.
    pub(crate) fn apply_validated_write(
        &mut self,
        id: ResolvedDocumentId,
        old_document_and_ts: Option<(ResolvedDocument, WriteTimestamp)>,
        new_document: Option<PendingDocument>,
    ) -> anyhow::Result<()> {
        // Implement something like two-phase commit between the index and the document
        // store. We first guarantee that the changes are valid for the index and
        // metadata and then let inserting into writes the commit
        // point so that the Transaction is never in an inconsistent state.
        let is_system_document = self.table_mapping().is_system_tablet(id.tablet_id);
        let new_document_view = new_document
            .as_ref()
            .map(|d| d.to_document_with_max_commit_ts())
            .transpose()?;
        if !is_system_document && let Some(view) = &new_document_view {
            // Size of document with max commit_ts injected should be the same as the size
            // of the document with the correct commit_ts injected since both are i64.
            view.check_user_size()?;
        }
        let bootstrap_tables = self.bootstrap_tables();
        let old_document = old_document_and_ts.as_ref().map(|(doc, _)| doc);
        let index_update = self.index.begin_update(
            old_document_and_ts.clone(),
            new_document_view.as_deref().cloned(),
        )?;
        let schema_update = self.schema_registry.begin_update(
            self.metadata.table_mapping(),
            id,
            old_document,
            new_document_view.as_deref(),
        )?;
        let component_update = self.component_registry.begin_update(
            self.metadata.table_mapping(),
            id,
            old_document,
            new_document_view.as_deref(),
        )?;
        let metadata_update = self.metadata.begin_update(
            index_update.registry(),
            id,
            old_document.map(|d| d.value().deref()),
            new_document_view.as_deref().map(|d| d.value().deref()),
        )?;
        let stats = self.stats.entry(id.tablet_id).or_default();
        let mut delta = 0;
        match (old_document, new_document.as_ref()) {
            (None, None) => {
                stats.rows_deleted += 1;
                stats.rows_created += 1;
            },
            (Some(_previous), None) => {
                // Delete
                stats.rows_deleted += 1;
                delta = -1;
            },
            (old, Some(_new)) => {
                // Insert or replace
                if old.is_none() {
                    // Insert
                    stats.rows_created += 1;
                    delta = 1;
                }
            },
        };
        // NB: Writes::update is fallible, so be sure to call it before applying the
        // index and metadata updates.
        self.writes.update(
            bootstrap_tables,
            is_system_document,
            &mut self.reads,
            id,
            old_document_and_ts,
            new_document,
            &self.limits,
        )?;
        stats.rows_written += 1;

        index_update.apply();
        metadata_update.apply();
        schema_update.apply();
        component_update.apply();

        *self.table_count_deltas.entry(id.tablet_id).or_default() += delta;
        Ok(())
    }

    pub(crate) async fn insert_document(
        &mut self,
        document: ResolvedDocument,
    ) -> anyhow::Result<ResolvedDocumentId> {
        self.insert_pending_document(document.into()).await
    }

    /// Insert a new document whose body may contain unresolved commit
    /// timestamps (see [`PendingDocument`]).
    pub(crate) async fn insert_pending_document(
        &mut self,
        document: PendingDocument,
    ) -> anyhow::Result<ResolvedDocumentId> {
        let document_id = document.id();
        let namespace = self
            .table_mapping()
            .tablet_namespace(document_id.tablet_id)?;
        SchemaModel::new(self, namespace)
            .enforce(document.to_document_with_max_commit_ts()?.as_ref())
            .await?;
        self.apply_validated_write(document_id, None, Some(document))?;
        Ok(document_id)
    }

    pub fn pending_write(&self, id: &ResolvedDocumentId) -> Option<&PendingDocumentUpdate> {
        self.writes.get(id)
    }

    pub async fn text_search(
        &mut self,
        stable_index_name: &StableIndexName,
        search: Search,
        version: SearchVersion,
    ) -> anyhow::Result<Vec<(CandidateRevision, IndexKeyBytes)>> {
        let Some(tablet_index_name) = stable_index_name.tablet_index_name() else {
            return Ok(vec![]);
        };
        // Short-circuit and return no results if the search query is just an empty
        // string. Skip usage tracking.
        if search.is_empty()? {
            return Ok(vec![]);
        }
        let tablet_id = tablet_index_name.table();
        let table_namespace = self.table_mapping().tablet_namespace(*tablet_id)?;
        let component_path = self.must_component_path(table_namespace.into())?;
        let search = search.to_internal(tablet_index_name.clone())?;

        // Get index metadata before performing the search
        let index_name = search.printable_index_name()?;
        let index = self
            .index
            .index_registry()
            .require_enabled(tablet_index_name, &index_name)?;
        let index_size = index.metadata().config.estimate_pricing_size_bytes()?;
        let TextSearchResults {
            revisions_with_keys,
            filtered_bytes_searched,
        } = self
            .index
            .search(&mut self.reads, &search, tablet_index_name.clone(), version)
            .await?;

        self.usage_tracker.track_text_query(
            component_path,
            index_name,
            index_size,
            filtered_bytes_searched,
        );
        Ok(revisions_with_keys)
    }

    // Preload an index range against the transaction, building a snapshot of
    // all its current values for future use. Preloading has a few limitations:
    //
    //   - It doesn't reflect subsequent updates to the index.
    //   - It currently only supports unique database indexes with a single indexed
    //     field.
    //   - It loads the entirety of the index into memory.
    //   - The preloaded snapshot only permits point queries on the index key.
    //
    pub async fn preload_index_range(
        &mut self,
        namespace: TableNamespace,
        index_name: &IndexName,
        interval: &Interval,
    ) -> anyhow::Result<PreloadedIndexRange> {
        let stable_index_name = IndexModel::new(self).stable_index_name(
            namespace,
            index_name,
            TableFilter::IncludePrivateSystemTables,
        )?;
        let StableIndexName::Physical(tablet_index_name) = stable_index_name else {
            anyhow::bail!(
                "Can only preload ranges on physical tables. Failed for index: {index_name} with \
                 {stable_index_name:?}"
            );
        };
        self.index
            .preload_index_range(&mut self.reads, &tablet_index_name, index_name, interval)
            .await
    }

    /// Starts recording reads on this transaction. Returns a
    /// `SnoopedTransaction`; its `finish` method stops recording and return the
    /// recorded read set, as well as the original transaction.
    ///
    /// As a caveat, it is possible to exceed transaction limits since the
    /// transaction read set is temporarily reset. This should only be used to
    /// snoop system reads or reads that are otherwise limited.
    pub fn snoop_reads(self) -> SnoopedTransaction<RT> {
        SnoopedTransaction::new(self)
    }

    pub fn finalize(self) -> anyhow::Result<FinalTransaction> {
        FinalTransaction::new(self)
    }

    /// Clone the transaction to use for a snapshot query.
    pub fn clone_for_snapshot_query(&self) -> Transaction<RT> {
        Transaction {
            identity: self.identity.clone(),
            id_generator: self.id_generator.clone_for_snapshot_query(),
            next_creation_time: self.next_creation_time,
            scheduled_size: self.scheduled_size.clone(),
            file_storage_read_size: self.file_storage_read_size.clone(),
            file_uploads: NestedWrites::new(FileUploads::default()),
            limits: self.limits.clone(),
            // Don't clone the read set because it is expensive and doesn't matter in a snapshot
            // query
            reads: TransactionReadSet::new(),
            // Reset the write set because the query should observe a snapshot of the database from
            // the start of the transaction
            writes: NestedWrites::new(Writes::new()),
            // Reset pending writes
            index: NestedWrites::new(self.index.clone_for_snapshot_query()),
            // TODO(ENG-10621): to be absolutely safe, we should reset changes to metadata,
            // schema_registry, and component_registry. Metadata may contain newly
            // created tables, but the tables will appear empty to snapshot query. It is
            // only correct to clone schema_registry and component_registry if
            // there were no writes to those tables
            metadata: self.metadata.clone(),
            schema_registry: self.schema_registry.clone(),
            component_registry: self.component_registry.clone(),
            count_snapshot: self.count_snapshot.clone(),
            // Also reset table_count_deltas
            table_count_deltas: BTreeMap::new(),
            stats: self.stats.clone(),
            runtime: self.runtime.clone(),
            usage_tracker: self.usage_tracker.clone(),
            virtual_system_mapping: self.virtual_system_mapping.clone(),

        }
    }

}

#[must_use]
#[derive(Deref, DerefMut)]
pub struct SnoopedTransaction<RT: Runtime> {
    saved_read_set: TransactionReadSet,
    #[deref]
    #[deref_mut]
    tx: Transaction<RT>,
}

impl<RT: Runtime> SnoopedTransaction<RT> {
    fn new(mut tx: Transaction<RT>) -> Self {
        Self {
            saved_read_set: mem::replace(&mut tx.reads, TransactionReadSet::new()),
            tx,
        }
    }

    pub fn finish_snoop(mut self) -> (Transaction<RT>, TransactionReadSet) {
        let snooped_reads = mem::replace(&mut self.tx.reads, self.saved_read_set);
        self.tx.apply_reads(snooped_reads.clone());
        (self.tx, snooped_reads)
    }
}

#[derive(Debug)]
pub struct IndexRangeRequest {
    pub stable_index_name: StableIndexName,
    pub interval: Interval,
    pub order: Order,
    pub max_rows: usize,
}

/// FinalTransaction is a finalized Transaction.
/// After all persistence reads have been performed and validated, and all
/// writes have been staged, a FinalTransaction stores the transaction until it
/// has been fully committed.
pub struct FinalTransaction {
    pub(crate) begin_timestamp: RepeatableTimestamp,
    pub(crate) table_mapping: TableMapping,
    pub(crate) component_registry: ComponentRegistry,

    pub(crate) reads: Arc<TransactionReadSet>,
    pub(crate) writes: Writes,

    pub(crate) usage_tracker: FunctionUsageTracker,

}

impl FinalTransaction {
    pub fn is_readonly(&self) -> bool {
        self.writes.is_empty()
    }

    fn new<RT: Runtime>(transaction: Transaction<RT>) -> anyhow::Result<Self> {
        // All subtransactions must have committed or rolled back.
        transaction.require_not_nested()?;
        transaction.file_uploads.require_no_pending()?;

        let begin_timestamp = transaction.begin_timestamp();
        Ok(Self {
            begin_timestamp,
            table_mapping: transaction.metadata.into_flat()?.table_mapping().clone(),
            component_registry: transaction.component_registry.into_flat()?,
            reads: Arc::new(transaction.reads),
            writes: transaction.writes.into_flat()?,
            usage_tracker: transaction.usage_tracker,

        })
    }

    pub(crate) fn validate_memory_index_sizes(
        &self,
        base_snapshot: &Snapshot,
        search_flusher_wake: &SearchFlusherWakeSignals,
    ) -> anyhow::Result<()> {
        #[allow(unused_mut)]
        let mut vector_size_limit = *VECTOR_INDEX_SIZE_HARD_LIMIT;
        #[allow(unused_mut)]
        let mut search_size_limit = *TEXT_INDEX_SIZE_HARD_LIMIT;
        let modified_tables: BTreeSet<_> = self
            .writes
            .coalesced_writes()
            .map(|update| update.id().tablet_id)
            .collect();
        let text_index_sizes = base_snapshot.text_indexes.flushable_in_memory_index_sizes();
        let vector_index_sizes = base_snapshot
            .vector_indexes
            .flushable_in_memory_index_sizes();
        // Wake the flushers before validating so that an index that's already
        // over the hard limit still gets flushed.
        search_flusher_wake.update_index_sizes(
            SearchType::Text,
            text_index_sizes.iter().cloned(),
            *SEARCH_INDEX_SIZE_SOFT_LIMIT,
        );
        search_flusher_wake.update_index_sizes(
            SearchType::Vector,
            vector_index_sizes.iter().cloned(),
            *VECTOR_INDEX_SIZE_SOFT_LIMIT,
        );
        Self::validate_memory_index_size(
            &self.table_mapping,
            base_snapshot,
            &modified_tables,
            text_index_sizes.into_iter(),
            search_size_limit,
            SearchType::Text,
        )?;
        Self::validate_memory_index_size(
            &self.table_mapping,
            base_snapshot,
            &modified_tables,
            vector_index_sizes.into_iter(),
            vector_size_limit,
            SearchType::Vector,
        )?;
        Ok(())
    }

    fn validate_memory_index_size(
        table_mapping: &TableMapping,
        base_snapshot: &Snapshot,
        modified_tables: &BTreeSet<TabletId>,
        iterator: impl Iterator<Item = (IndexId, usize)>,
        hard_limit: usize,
        index_type: SearchType,
    ) -> anyhow::Result<()> {
        for (index_id, size) in iterator {
            if size < hard_limit {
                continue;
            }

            // Note that we are getting an index by name without adding any read dependency.
            // This is fine since we are only decided whether to let the transaction
            // through or not. If we do not, we will throw a non-JS error.
            let index = base_snapshot
                .index_registry
                .enabled_index_by_index_id(&index_id)
                .or_else(|| {
                    base_snapshot
                        .index_registry
                        .pending_index_by_index_id(&index_id)
                })
                .cloned()
                .with_context(|| anyhow::anyhow!("failed to find index id {index_id}"))?
                .name();

            if !modified_tables.contains(index.table()) {
                // NOTE: All operation make the in memory size larger, including
                // deletes and updates with smaller values. So we reject any
                // modification if we are over the limit.
                continue;
            }
            tracing::error!(
                "Index size for index_id {index_id} is {size}, limit for {} is {hard_limit}",
                index_type.as_ref()
            );
            log_index_too_large_blocking_writes(index_type);
            anyhow::bail!(ErrorMetadata::overloaded(
                format!("{}IndexTooLarge", index_type.as_ref()),
                format!(
                    "Too many writes to {}. Spread your writes out over time or throttle them to avoid errors. If you’re importing data into a new application, consider removing the index and adding it again after the import (you can re-add the index as a staged index to avoid blocking your pushes: https://docs.convex.dev/database/reading-data/indexes/#staged-indexes).",
                    index.map_table(&table_mapping.tablet_to_name())?
                )
            ))
        }
        Ok(())
    }
}
