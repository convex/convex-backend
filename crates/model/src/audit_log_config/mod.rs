use anyhow::Context;
use common::{
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
    TableName,
    TableNamespace,
};

use crate::{
    SystemIndex,
    SystemTable,
};

pub mod types;
use types::AuditLogConfig;

pub const AUDIT_LOG_CONFIG_TABLE: TableName = TableName::const_new("_audit_log_config");

pub struct AuditLogConfigTable;
impl SystemTable for AuditLogConfigTable {
    type Metadata = AuditLogConfig;

    const TABLE_NAME: TableName = AUDIT_LOG_CONFIG_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub fn validate_audit_log_firehose_stream_name(
    firehose_stream_name: &str,
    deployment_name: &str,
) -> anyhow::Result<()> {
    let prefix = format!("customer-audit-logs-{deployment_name}");
    anyhow::ensure!(
        firehose_stream_name.starts_with(&prefix),
        ErrorMetadata::bad_request(
            "InvalidAuditLogFirehoseStreamName",
            format!(
                "Expected audit log firehose stream name to start with \"{prefix}\" but got \
                 {firehose_stream_name}"
            ),
        ),
    );
    Ok(())
}

pub struct AuditLogConfigModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> AuditLogConfigModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    /// Get the current audit log config, if one exists.
    async fn get(&mut self) -> anyhow::Result<Option<ParsedDocument<AuditLogConfig>>> {
        let result = self
            .tx
            .query_system(
                TableNamespace::Global,
                &SystemIndex::<AuditLogConfigTable>::by_id(),
            )?
            .unique()
            .await?
            .map(|arc_row| (*arc_row).clone());
        Ok(result)
    }

    /// Get the existing config row ID, or create a new row with defaults.
    pub async fn get_or_create(&mut self) -> anyhow::Result<ParsedDocument<AuditLogConfig>> {
        if let Some(existing) = self.get().await? {
            Ok(existing)
        } else {
            let config = AuditLogConfig {
                firehose_stream_name: None,
            };
            let _ = SystemMetadataModel::new_global(self.tx)
                .insert(&AUDIT_LOG_CONFIG_TABLE, config.try_into()?)
                .await?;
            let doc = self
                .get()
                .await?
                .context("Expected audit log config to exist")?;
            Ok(doc)
        }
    }

    /// Set or unset the firehose stream name, creating the config row if
    /// needed.
    pub async fn set_firehose_stream_name(
        &mut self,
        deployment_name: &str,
        firehose_stream_name: Option<String>,
    ) -> anyhow::Result<()> {
        if let Some(name) = &firehose_stream_name {
            validate_audit_log_firehose_stream_name(name, deployment_name)?;
        }
        let config = self.get_or_create().await?;
        let value = firehose_stream_name
            .map(ConvexValue::try_from)
            .transpose()?;
        SystemMetadataModel::new_global(self.tx)
            .patch(config.id(), patch_value!("firehoseStreamName" => value)?)
            .await?;
        Ok(())
    }
}
