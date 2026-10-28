use std::{
    cmp,
    collections::BTreeMap,
};

use anyhow::Context;
use common::{
    bootstrap_model::components::ComponentState,
    components::ComponentId,
    document::{
        DeveloperDocument,
        PackedDocument,
        PendingDocument,
    },
    query::CursorPosition,
    runtime::Runtime,
    types::{
        StableIndexName,
        WriteTimestamp,
    },
    version::Version,
};
use errors::ErrorMetadata;
use indexing::index_reader::{
    BatchKey,
    RangeRequest,
};
use itertools::Itertools;
use value::{
    DeveloperDocumentId,
    PendingValue,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

use crate::{
    metrics::{
        log_virtual_table_get,
        log_virtual_table_query,
    },
    query::IndexRangeResponse,
    transaction::{
        IndexRangeRequest,
        MAX_PAGE_SIZE,
    },
    unauthorized_error,
    virtual_tables::VirtualTable,
    BootstrapComponentsModel,
    PatchValue,
    TableModel,
    Transaction,
};

// Low-level model struct that represents a "user facing" data model
// on the database. This view differs from the authoritative system
// state in a few ways:
//
//  1. System tables are only visible for `Identity::Admin` or
//     `Identity::System`.
//  2. We support virtual tables.
//  3. The interface is in `DeveloperDocumentId`s, not `ResolvedDocumentId`.
//  4. We track user size limits for documents, which are more restrictive than
//     the database's limits.
//  5. We support branching on the `convex` NPM package's version.
pub struct UserFacingModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
    namespace: TableNamespace,
}

impl<'a, RT: Runtime> UserFacingModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>, namespace: TableNamespace) -> Self {
        Self { tx, namespace }
    }

    #[fastrace::trace]
    #[convex_macro::instrument_future]
    pub async fn get_with_ts(
        &mut self,
        id: DeveloperDocumentId,
        version: Option<Version>,
    ) -> anyhow::Result<Option<(DeveloperDocument, WriteTimestamp)>> {
        if !self
            .tx
            .table_mapping()
            .namespace(self.namespace)
            .table_number_exists()(id.table())
        {
            return Ok(None);
        }
        let id_ = self.tx.resolve_developer_id(&id, self.namespace)?;
        let physical_table_name = self
            .tx
            .table_mapping()
            .namespace(self.namespace)
            .tablet_name(id_.tablet_id)?;
        if self
            .tx
            .virtual_system_mapping()
            .primary_system_to_virtual_table(&physical_table_name)
            .is_some()
        {
            log_virtual_table_get();
            VirtualTable::new(self.tx)
                .get(self.namespace, id, version)
                .await
        } else {
            let table_name = self.tx.table_mapping().tablet_name(id_.tablet_id)?;
            let result = self.tx.get_inner(id_, table_name).await?;
            Ok(result.map(|(doc, ts)| (doc.to_developer(), ts)))
        }
    }

    /// Returns an error if the component associated with the current namespace
    /// is unmounted. Should be called in all methods that write to user tables.
    async fn require_active_component(&mut self) -> anyhow::Result<()> {
        match self.namespace {
            TableNamespace::Global => {},
            TableNamespace::ByComponent(developer_id) => {
                let component = BootstrapComponentsModel::new(self.tx)
                    .load_component(ComponentId::Child(developer_id))
                    .await?
                    .with_context(|| format!("Component not found for id: {developer_id}"))?;

                match component.state {
                    ComponentState::Active => {},
                    ComponentState::Unmounted => {
                        anyhow::bail!(ErrorMetadata::bad_request(
                            "UnmountedComponent",
                            "Cannot perform write operations in an unmounted component. Make sure \
                             your component is installed in your `convex.config.ts` file.",
                        ));
                    },
                }
            },
        }
        Ok(())
    }

    /// Creates a new document with given value in the specified table.
    #[fastrace::trace]
    #[convex_macro::instrument_future]
    pub async fn insert(
        &mut self,
        table: TableName,
        value: impl Into<PendingValue> + Send,
    ) -> anyhow::Result<DeveloperDocumentId> {
        let value = value.into();
        self.require_active_component().await?;
        if self.tx.virtual_system_mapping().is_virtual_table(&table) {
            anyhow::bail!(ErrorMetadata::bad_request(
                "ReadOnlyTable",
                format!("{table} is a read-only table"),
            ));
        }

        let internal_id = self.tx.id_generator.generate_internal();

        let creation_time = self.tx.next_creation_time.increment()?;

        if table.is_system() {
            anyhow::bail!(ErrorMetadata::bad_request(
                "InvalidTableName",
                format!("Invalid table name {table} starts with metadata prefix '_'")
            ));
        }

        // Note that the index and document store updates within `self.insert_document`
        // below are fallible, and since the layers above still have access to
        // the `Transaction` in that case (we only have `&mut self` here, not a
        // consuming `self`), we need to make sure we leave the transaction in a
        // consistent state on error.
        //
        // It's okay for us to insert the table write here and fail below: At worse the
        // transaction will contain an insertion for an empty table's `_tables`
        // record. On the other hand, it's not okay for us to succeed an
        // insertion into the index/document store and then fail to insert the
        // table metadata. If the user then subsequently commits that transaction,
        // they'll have a record that points to a nonexistent table.
        TableModel::new(self.tx)
            .insert_table_metadata(self.namespace, &table)
            .await?;
        let table_id = self
            .tx
            .table_mapping()
            .namespace(self.namespace)
            .name_to_id_user_input()(table)?;
        let document = PendingDocument::new(
            ResolvedDocumentId::new(
                table_id.tablet_id,
                DeveloperDocumentId::new(table_id.table_number, internal_id),
            ),
            creation_time,
            value,
        )?;
        let document_id = self.tx.insert_pending_document(document).await?;

        Ok(document_id.into())
    }

    /// Merges the existing document with the given object. Will overwrite any
    /// conflicting fields.
    #[fastrace::trace]
    #[convex_macro::instrument_future]
    pub async fn patch(
        &mut self,
        id: DeveloperDocumentId,
        value: PatchValue,
    ) -> anyhow::Result<PendingDocument> {
        if self.tx.is_system(self.namespace, id.table())
            && !(self.tx.identity.is_admin() || self.tx.identity.is_system())
        {
            anyhow::bail!(unauthorized_error("patch"))
        }
        self.require_active_component().await?;

        let id_ = self.tx.resolve_developer_id(&id, self.namespace)?;

        let new_document = self.tx.patch_inner(id_, value).await?;
        Ok(new_document)
    }

    /// Replace the document with the given value.
    #[fastrace::trace]
    #[convex_macro::instrument_future]
    pub async fn replace(
        &mut self,
        id: DeveloperDocumentId,
        value: impl Into<PendingValue> + Send,
    ) -> anyhow::Result<PendingDocument> {
        if self.tx.is_system(self.namespace, id.table())
            && !(self.tx.identity.is_admin() || self.tx.identity.is_system())
        {
            anyhow::bail!(unauthorized_error("replace"))
        }
        self.require_active_component().await?;
        let id_ = self.tx.resolve_developer_id(&id, self.namespace)?;

        let new_document = self.tx.replace_inner(id_, value).await?;
        Ok(new_document)
    }

    /// Delete the document at the given path -- called from user facing APIs
    /// (e.g. syscalls)
    #[fastrace::trace]
    #[convex_macro::instrument_future]
    pub async fn delete(&mut self, id: DeveloperDocumentId) -> anyhow::Result<DeveloperDocument> {
        if self.tx.is_system(self.namespace, id.table())
            && !(self.tx.identity.is_admin() || self.tx.identity.is_system())
        {
            anyhow::bail!(unauthorized_error("delete"))
        }
        self.require_active_component().await?;

        let id_ = self.tx.resolve_developer_id(&id, self.namespace)?;
        let document = self.tx.delete_inner(id_).await?;
        Ok(document.to_developer())
    }

    pub fn record_read_document(
        &mut self,
        document: &PackedDocument,
        table_name: &TableName,
    ) -> anyhow::Result<()> {
        let component_path = self
            .tx
            .must_component_path(ComponentId::from(self.namespace))?;
        self.tx.reads.record_read_document(
            component_path,
            table_name.clone(),
            document.size(),
            &self.tx.usage_tracker,
            &self.tx.virtual_system_mapping,
            &self.tx.limits,
        )
    }
}

fn start_index_range<RT: Runtime>(
    tx: &mut Transaction<RT>,
    request: IndexRangeRequest,
) -> anyhow::Result<Result<(), RangeRequest>> {
    if request.interval.is_empty() {
        return Ok(Ok(()));
    }

    let max_rows = cmp::min(request.max_rows, MAX_PAGE_SIZE);

    match request.stable_index_name {
        StableIndexName::Physical(tablet_index_name) => {
            let index_name = tablet_index_name
                .clone()
                .map_table(&tx.table_mapping().tablet_to_name())?;
            Ok(Err(RangeRequest {
                index_name: tablet_index_name,
                printable_index_name: index_name,
                interval: request.interval.clone(),
                order: request.order,
                max_size: max_rows,
            }))
        },
        StableIndexName::Virtual(index_name, tablet_index_name) => {
            log_virtual_table_query();
            Ok(Err(RangeRequest {
                index_name: tablet_index_name,
                printable_index_name: index_name,
                interval: request.interval.clone(),
                order: request.order,
                max_size: max_rows,
            }))
        },
        StableIndexName::Missing(_) => Ok(Ok(())),
    }
}

/// NOTE: returns a page of results. Callers must call record_read_document +
/// record_indexed_directly for all documents returned from the index stream.
#[fastrace::trace]
#[convex_macro::instrument_future]
pub async fn index_range_batch<RT: Runtime>(
    tx: &mut Transaction<RT>,
    requests: BTreeMap<BatchKey, IndexRangeRequest>,
) -> BTreeMap<BatchKey, anyhow::Result<IndexRangeResponse>> {
    let batch_size = requests.len();
    let mut results = BTreeMap::new();
    let mut fetch_requests = BTreeMap::new();
    for (batch_key, request) in requests {
        match start_index_range(tx, request) {
            Err(e) => {
                results.insert(batch_key, Err(e));
            },
            Ok(Ok(())) => {
                results.insert(
                    batch_key,
                    Ok(IndexRangeResponse {
                        page: vec![],
                        cursor: CursorPosition::End,
                    }),
                );
            },
            Ok(Err(request)) => {
                fetch_requests.insert(batch_key, request);
            },
        }
    }

    let fetch_results = tx
        .index
        .range_batch(&fetch_requests.values().collect_vec())
        .await;

    for (&batch_key, fetch_result) in fetch_requests.keys().zip(fetch_results) {
        results.insert(batch_key, fetch_result);
    }

    assert_eq!(results.len(), batch_size);
    results
}
