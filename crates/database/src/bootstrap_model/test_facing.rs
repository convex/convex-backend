use common::{
    document::{
        PendingDocument,
        ResolvedDocument,
    },
    runtime::Runtime,
};
use value::{
    ConvexObject,
    PendingValue,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

use crate::{
    SystemMetadataModel,
    TableModel,
    Transaction,
};

pub struct TestFacingModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> TestFacingModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    #[convex_macro::instrument_future]
    pub async fn insert(
        &mut self,
        table: &TableName,
        value: ConvexObject,
    ) -> anyhow::Result<ResolvedDocumentId> {
        SystemMetadataModel::new(self.tx, TableNamespace::test_user())
            .insert_metadata(table, value)
            .await
    }

    /// Insert a new document whose body may contain unresolved commit
    /// timestamps (see `value::PendingValue`).
    #[convex_macro::instrument_future]
    pub async fn insert_pending(
        &mut self,
        table: &TableName,
        body: PendingValue,
    ) -> anyhow::Result<ResolvedDocumentId> {
        let namespace = TableNamespace::test_user();
        TableModel::new(self.tx)
            .insert_table_metadata(namespace, table)
            .await?;
        let table_id = self.tx.table_mapping().namespace(namespace).id(table)?;
        let id = self.tx.id_generator.generate_resolved(table_id);
        let creation_time = self.tx.next_creation_time.increment()?;
        let document = PendingDocument::new(id, creation_time, body)?;
        self.tx.insert_pending_document(document).await
    }

    #[convex_macro::instrument_future]
    pub async fn replace(
        &mut self,
        id: ResolvedDocumentId,
        value: ConvexObject,
    ) -> anyhow::Result<ResolvedDocument> {
        SystemMetadataModel::new(self.tx, TableNamespace::test_user())
            .replace(id, value)
            .await
    }

    /// Replace a document by id, bypassing user-facing checks.
    pub async fn replace_inner(
        &mut self,
        id: ResolvedDocumentId,
        value: ConvexObject,
    ) -> anyhow::Result<ResolvedDocument> {
        self.tx.replace_inner(id, value).await?.into_resolved()
    }

    /// Insert a new document and immediately read it. Prefer using `insert`
    /// unless you need to read the creation time.
    #[convex_macro::instrument_future]
    pub async fn insert_and_get(
        &mut self,
        table: TableName,
        value: ConvexObject,
    ) -> anyhow::Result<ResolvedDocument> {
        let id = self.insert(&table, value).await?;
        self.tx
            .get(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Document with id {id} must exist"))
    }
}
