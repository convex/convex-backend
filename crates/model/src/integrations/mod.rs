use std::sync::Arc;

use anyhow::Context as _;
use common::{
    self,
    document::ParsedDocument,
    runtime::Runtime,
};
use database::{
    patch_value,
    SystemMetadataModel,
    Transaction,
};
use errors::ErrorMetadata;
use value::{
    ConvexValue,
    DeveloperDocumentId,
    TableName,
    TableNamespace,
};

use crate::{
    SystemIndex,
    SystemTable,
};

pub mod types;
use types::{
    IntegrationConfig,
    IntegrationDocument,
    IntegrationState,
    IntegrationType,
    INTEGRATIONS_LIMIT,
};

pub const INTEGRATIONS_TABLE: TableName = TableName::const_new("_log_sinks");

pub struct IntegrationsTable;
impl SystemTable for IntegrationsTable {
    type Metadata = IntegrationDocument;

    const TABLE_NAME: TableName = INTEGRATIONS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub struct IntegrationsModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> IntegrationsModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    async fn must_get_including_tombstoned(
        &mut self,
        id: DeveloperDocumentId,
    ) -> anyhow::Result<ParsedDocument<IntegrationDocument>> {
        Ok(Arc::unwrap_or_clone(
            self.tx
                .get_system::<IntegrationsTable>(TableNamespace::Global, id)
                .await?
                .with_context(|| {
                    ErrorMetadata::not_found(
                        "LogStreamDoesntExist",
                        format!("No log stream with the given id {id} exists for this deployment."),
                    )
                })?,
        ))
    }

    pub async fn must_get(
        &mut self,
        id: DeveloperDocumentId,
    ) -> anyhow::Result<ParsedDocument<IntegrationDocument>> {
        let doc = self.must_get_including_tombstoned(id).await?;
        anyhow::ensure!(
            doc.status != IntegrationState::Tombstoned,
            ErrorMetadata::not_found(
                "LogStreamDoesntExist",
                format!("No log stream with the given id {id} exists for this deployment."),
            )
        );
        Ok(doc)
    }

    pub async fn get_by_provider(
        &mut self,
        provider: IntegrationType,
    ) -> anyhow::Result<Option<ParsedDocument<IntegrationDocument>>> {
        let mut result: Vec<_> = self
            .get_by_provider_including_tombstoned(provider.clone())
            .await?
            .into_iter()
            .filter(|doc| doc.status != IntegrationState::Tombstoned)
            .collect();
        anyhow::ensure!(
            result.len() <= 1,
            "Multiple integrations found of the same type: {:?}",
            provider
        );
        Ok(result.pop())
    }

    async fn get_by_provider_including_tombstoned(
        &mut self,
        provider: IntegrationType,
    ) -> anyhow::Result<Vec<ParsedDocument<IntegrationDocument>>> {
        let result: Vec<_> = self
            .get_all()
            .await?
            .into_iter()
            .filter(|doc| doc.config.integration_type() == provider)
            .collect();
        Ok(result)
    }

    pub async fn get_all(&mut self) -> anyhow::Result<Vec<ParsedDocument<IntegrationDocument>>> {
        let result = self
            .tx
            .query_system(
                TableNamespace::Global,
                &SystemIndex::<IntegrationsTable>::by_id(),
            )?
            .all()
            .await?
            .into_iter()
            .map(|arc_document| (*arc_document).clone())
            .collect();
        Ok(result)
    }

    pub async fn get_all_non_tombstoned(
        &mut self,
    ) -> anyhow::Result<Vec<ParsedDocument<IntegrationDocument>>> {
        let result = self
            .get_all()
            .await?
            .into_iter()
            .filter(|document| document.status != IntegrationState::Tombstoned)
            .collect();
        Ok(result)
    }

    pub async fn patch_status(
        &mut self,
        id: DeveloperDocumentId,
        status: IntegrationState,
    ) -> anyhow::Result<()> {
        let doc = self.must_get_including_tombstoned(id).await?;
        SystemMetadataModel::new_global(self.tx)
            .patch(
                doc.id(),
                patch_value!("status" => Some(ConvexValue::Object(status.try_into()?)))?,
            )
            .await?;
        Ok(())
    }

    pub async fn patch_config(
        &mut self,
        id: DeveloperDocumentId,
        config: IntegrationConfig,
    ) -> anyhow::Result<()> {
        let doc = self.must_get(id).await?;
        SystemMetadataModel::new_global(self.tx)
            .patch(
                doc.id(),
                patch_value!("config" => Some(ConvexValue::Object(config.try_into()?)))?,
            )
            .await?;
        Ok(())
    }

    pub async fn mark_for_removal(&mut self, id: DeveloperDocumentId) -> anyhow::Result<()> {
        self.patch_status(id, IntegrationState::Tombstoned).await
    }

    pub async fn add_or_update(
        &mut self,
        config: IntegrationConfig,
    ) -> anyhow::Result<DeveloperDocumentId> {
        let integration_type = config.integration_type();
        let document = IntegrationDocument {
            status: IntegrationState::Pending,
            config,
        };

        let existing = self.get_by_provider(integration_type.clone()).await?;
        // Check within the insert transaction so concurrent creates cannot
        // replace an export and restart it.
        anyhow::ensure!(
            existing.is_none() || integration_type != IntegrationType::S3Export,
            ErrorMetadata::conflict(
                "LogStreamAlreadyExists",
                "An S3 export already exists. Delete it before creating a new one.",
            )
        );

        let integrations = self
            .get_all()
            .await?
            .into_iter()
            .filter(|document| document.status != IntegrationState::Tombstoned)
            .collect::<Vec<_>>();
        if integrations.len() >= INTEGRATIONS_LIMIT {
            return Err(ErrorMetadata::bad_request(
                "LogSinkQuotaExceeded",
                "Cannot add more integrations, the quota for this project has been reached.",
            )
            .into());
        }

        if let Some(document) = existing {
            self.mark_for_removal(document.developer_id()).await?;
        }

        let id = SystemMetadataModel::new_global(self.tx)
            .insert(&INTEGRATIONS_TABLE, document.try_into()?)
            .await?;
        Ok(id.developer_id)
    }

    pub async fn clear(&mut self) -> anyhow::Result<()> {
        let providers = self.get_all().await?;

        for integration in providers {
            self.mark_for_removal(integration.developer_id()).await?;
        }
        Ok(())
    }
}
