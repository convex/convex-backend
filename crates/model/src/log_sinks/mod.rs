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
    LogSinksRow,
    SinkConfig,
    SinkState,
    SinkType,
    LOG_SINKS_LIMIT,
};

pub const LOG_SINKS_TABLE: TableName = TableName::const_new("_log_sinks");

pub struct LogSinksTable;
impl SystemTable for LogSinksTable {
    type Metadata = LogSinksRow;

    const TABLE_NAME: TableName = LOG_SINKS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub struct LogSinksModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> LogSinksModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    async fn must_get_including_tombstoned(
        &mut self,
        id: DeveloperDocumentId,
    ) -> anyhow::Result<ParsedDocument<LogSinksRow>> {
        Ok(Arc::unwrap_or_clone(
            self.tx
                .get_system::<LogSinksTable>(TableNamespace::Global, id)
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
    ) -> anyhow::Result<ParsedDocument<LogSinksRow>> {
        let doc = self.must_get_including_tombstoned(id).await?;
        anyhow::ensure!(
            doc.status != SinkState::Tombstoned,
            ErrorMetadata::not_found(
                "LogStreamDoesntExist",
                format!("No log stream with the given id {id} exists for this deployment."),
            )
        );
        Ok(doc)
    }

    pub async fn get_by_provider(
        &mut self,
        provider: SinkType,
    ) -> anyhow::Result<Option<ParsedDocument<LogSinksRow>>> {
        let mut result: Vec<_> = self
            .get_by_provider_including_tombstoned(provider.clone())
            .await?
            .into_iter()
            .filter(|doc| doc.status != SinkState::Tombstoned)
            .collect();
        anyhow::ensure!(
            result.len() <= 1,
            "Multiple sinks found of the same type: {:?}",
            provider
        );
        Ok(result.pop())
    }

    async fn get_by_provider_including_tombstoned(
        &mut self,
        provider: SinkType,
    ) -> anyhow::Result<Vec<ParsedDocument<LogSinksRow>>> {
        let result: Vec<_> = self
            .get_all()
            .await?
            .into_iter()
            .filter(|doc| doc.config.sink_type() == provider)
            .collect();
        Ok(result)
    }

    pub async fn get_all(&mut self) -> anyhow::Result<Vec<ParsedDocument<LogSinksRow>>> {
        let result = self
            .tx
            .query_system(
                TableNamespace::Global,
                &SystemIndex::<LogSinksTable>::by_id(),
            )?
            .all()
            .await?
            .into_iter()
            .map(|arc_row| (*arc_row).clone())
            .collect();
        Ok(result)
    }

    pub async fn get_all_non_tombstoned(
        &mut self,
    ) -> anyhow::Result<Vec<ParsedDocument<LogSinksRow>>> {
        let result = self
            .get_all()
            .await?
            .into_iter()
            .filter(|row| row.status != SinkState::Tombstoned)
            .collect();
        Ok(result)
    }

    pub async fn patch_status(
        &mut self,
        id: DeveloperDocumentId,
        status: SinkState,
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
        config: SinkConfig,
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
        self.patch_status(id, SinkState::Tombstoned).await
    }

    pub async fn add_or_update(
        &mut self,
        config: SinkConfig,
    ) -> anyhow::Result<DeveloperDocumentId> {
        let sink_type = config.sink_type();
        let row = LogSinksRow {
            status: SinkState::Pending,
            config,
        };

        let existing = self.get_by_provider(sink_type.clone()).await?;
        // Check within the insert transaction so concurrent creates cannot
        // replace an export and restart it.
        anyhow::ensure!(
            existing.is_none() || sink_type != SinkType::S3Export,
            ErrorMetadata::conflict(
                "LogStreamAlreadyExists",
                "An S3 export already exists. Delete it before creating a new one.",
            )
        );

        // Filter to non-tombstoned log sinks
        let sinks = self
            .get_all()
            .await?
            .into_iter()
            .filter(|row| row.status != SinkState::Tombstoned)
            .collect::<Vec<_>>();
        if sinks.len() >= LOG_SINKS_LIMIT {
            return Err(ErrorMetadata::bad_request(
                "LogSinkQuotaExceeded",
                "Cannot add more LogSinks, the quota for this project has been reached.",
            )
            .into());
        }

        if let Some(row) = existing {
            self.mark_for_removal(row.developer_id()).await?;
        }

        let id = SystemMetadataModel::new_global(self.tx)
            .insert(&LOG_SINKS_TABLE, row.try_into()?)
            .await?;
        Ok(id.developer_id)
    }

    // It's generally not safe to delete an existing sink without marking it
    // Tombstoned first since the LogManager will not know to remove the sink.
    // However, we can do this during startup before the LogManager has started
    // (like when adding a local log sink)
    pub async fn add_on_startup(&mut self, config: SinkConfig) -> anyhow::Result<()> {
        // Search for matching provider
        if let Some(sink) = self.get_by_provider(config.sink_type()).await? {
            SystemMetadataModel::new_global(self.tx)
                .delete(sink.id())
                .await?;
        };
        self.add_or_update(config).await?;
        Ok(())
    }

    pub async fn clear(&mut self) -> anyhow::Result<()> {
        let providers = self.get_all().await?;

        for sink in providers {
            self.mark_for_removal(sink.developer_id()).await?;
        }
        Ok(())
    }
}
