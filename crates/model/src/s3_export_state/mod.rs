use std::sync::{
    Arc,
    LazyLock,
};

use common::{
    document::{
        ParsedDocument,
        CREATION_TIME_FIELD_PATH,
    },
    runtime::Runtime,
};
use database::{
    SystemMetadataModel,
    Transaction,
};
use value::{
    DeveloperDocumentId,
    FieldPath,
    TableName,
    TableNamespace,
};

use crate::{
    SystemIndex,
    SystemTable,
};

pub mod types;
use types::{
    S3ExportCheckpoint,
    S3ExportState,
};

pub const S3_EXPORT_STATE_TABLE: TableName = TableName::const_new("_s3_export_state");
pub static S3_EXPORT_STATE_BY_CONFIG_ID: LazyLock<SystemIndex<S3ExportStateTable>> =
    LazyLock::new(|| {
        let config_id: FieldPath = "configId".parse().unwrap();
        SystemIndex::new("by_config_id", [&config_id, &CREATION_TIME_FIELD_PATH]).unwrap()
    });

pub struct S3ExportStateTable;
impl SystemTable for S3ExportStateTable {
    type Metadata = S3ExportState;

    const TABLE_NAME: TableName = S3_EXPORT_STATE_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![S3_EXPORT_STATE_BY_CONFIG_ID.clone()]
    }
}

pub struct S3ExportStateModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> S3ExportStateModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    pub async fn get(
        &mut self,
        config_id: DeveloperDocumentId,
    ) -> anyhow::Result<Option<Arc<ParsedDocument<S3ExportState>>>> {
        self.tx
            .query_system(TableNamespace::Global, &S3_EXPORT_STATE_BY_CONFIG_ID)?
            .eq(&[config_id.to_string().as_str()])?
            .unique()
            .await
    }

    /// Use the transaction that read the configuration before writing to S3 so
    /// OCC rejects checkpoints produced concurrently with configuration
    /// changes.
    pub async fn advance(
        &mut self,
        config_id: DeveloperDocumentId,
        expected_checkpoint: Option<&S3ExportCheckpoint>,
        checkpoint: S3ExportCheckpoint,
    ) -> anyhow::Result<bool> {
        let state = self.get(config_id).await?;
        if state.as_ref().map(|state| &state.checkpoint) != expected_checkpoint {
            return Ok(false);
        }
        if expected_checkpoint != Some(&checkpoint) {
            let updated = S3ExportState {
                config_id,
                checkpoint,
            };
            let mut model = SystemMetadataModel::new_global(self.tx);
            if let Some(state) = state {
                model.replace(state.id(), updated.try_into()?).await?;
            } else {
                model
                    .insert(&S3_EXPORT_STATE_TABLE, updated.try_into()?)
                    .await?;
            }
        }
        Ok(true)
    }
}
