pub mod types;

use std::sync::{
    Arc,
    LazyLock,
};

use anyhow::Context as _;
use common::{
    document::{
        ParsedDocument,
        CREATION_TIME_FIELD_PATH,
    },
    runtime::Runtime,
};
use value::{
    FieldPath,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

use crate::{
    system_tables::{
        SystemIndex,
        SystemTable,
    },
    SchemaValidationProgress,
    SystemMetadataModel,
    Transaction,
};

pub const SCHEMA_VALIDATION_PROGRESS_TABLE: TableName =
    TableName::const_new("_schema_validation_progress");
pub static SCHEMA_VALIDATION_PROGRESS_BY_VALIDATION_ID: LazyLock<
    SystemIndex<SchemaValidationProgressTable>,
> = LazyLock::new(|| {
    SystemIndex::new(
        "by_validation_id",
        [&VALIDATION_ID_FIELD, &CREATION_TIME_FIELD_PATH],
    )
    .unwrap()
});
static VALIDATION_ID_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "validationId".parse().expect("invalid validationId field"));

pub struct SchemaValidationProgressTable;
impl SystemTable for SchemaValidationProgressTable {
    type Metadata = SchemaValidationProgress;

    const TABLE_NAME: TableName = SCHEMA_VALIDATION_PROGRESS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![SCHEMA_VALIDATION_PROGRESS_BY_VALIDATION_ID.clone()]
    }
}

pub struct SchemaValidationProgressModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
    namespace: TableNamespace,
}
impl<'a, RT: Runtime> SchemaValidationProgressModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>, namespace: TableNamespace) -> Self {
        Self { tx, namespace }
    }

    pub async fn get(
        &mut self,
        validation_id: ResolvedDocumentId,
    ) -> anyhow::Result<Option<ParsedDocument<SchemaValidationProgress>>> {
        Ok(self
            .tx
            .query_system(
                self.namespace,
                &*SCHEMA_VALIDATION_PROGRESS_BY_VALIDATION_ID,
            )?
            .eq(&[validation_id
                .developer_id
                .encode_into(&mut Default::default())])?
            .unique()
            .await?
            .map(Arc::unwrap_or_clone))
    }

    pub async fn must_get(
        &mut self,
        validation_id: ResolvedDocumentId,
    ) -> anyhow::Result<ParsedDocument<SchemaValidationProgress>> {
        self.get(validation_id)
            .await?
            .context("Validation attempt is missing its progress")
    }

    pub(crate) async fn create(
        &mut self,
        validation_id: ResolvedDocumentId,
        num_docs_validated: u64,
        total_docs: Option<u64>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.get(validation_id).await?.is_none(),
            "Validation attempt {validation_id} already has progress"
        );
        let metadata = SchemaValidationProgress {
            validation_id: validation_id.developer_id,
            num_docs_validated,
            total_docs,
        };
        SystemMetadataModel::new(self.tx, self.namespace)
            .insert(&SCHEMA_VALIDATION_PROGRESS_TABLE, metadata.try_into()?)
            .await?;
        Ok(())
    }

    pub(crate) async fn reset(
        &mut self,
        validation_id: ResolvedDocumentId,
        total_docs: Option<u64>,
    ) -> anyhow::Result<()> {
        let doc = self.must_get(validation_id).await?;
        let (id, mut metadata) = doc.into_id_and_value();
        metadata.num_docs_validated = 0;
        metadata.total_docs = total_docs;
        SystemMetadataModel::new(self.tx, self.namespace)
            .replace(id, metadata.try_into()?)
            .await?;
        Ok(())
    }

    pub(crate) async fn record(
        &mut self,
        validation_id: ResolvedDocumentId,
        count: u64,
        total_docs: Option<u64>,
    ) -> anyhow::Result<()> {
        let doc = self.must_get(validation_id).await?;
        let (id, mut metadata) = doc.into_id_and_value();
        metadata.num_docs_validated += count;
        metadata.total_docs = metadata.total_docs.or(total_docs);
        SystemMetadataModel::new(self.tx, self.namespace)
            .replace(id, metadata.try_into()?)
            .await?;
        Ok(())
    }

    pub(crate) async fn delete(&mut self, validation_id: ResolvedDocumentId) -> anyhow::Result<()> {
        if let Some(doc) = self.get(validation_id).await? {
            SystemMetadataModel::new(self.tx, self.namespace)
                .delete(doc.id())
                .await?;
        }
        Ok(())
    }
}
