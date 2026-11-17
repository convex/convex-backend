use common::runtime::Runtime;
use keybroker::Identity;
use model::audit_log_config::AuditLogConfigModel;

use crate::Application;

impl<RT: Runtime> Application<RT> {
    pub async fn get_audit_log_firehose_stream(
        &self,
        identity: Identity,
    ) -> anyhow::Result<Option<String>> {
        let mut tx = self.begin(identity).await?;
        let config = AuditLogConfigModel::new(&mut tx).get_or_create().await?;
        Ok(config.firehose_stream_name.clone())
    }

    pub async fn set_audit_log_firehose_stream(
        &self,
        identity: Identity,
        firehose_stream_name: Option<String>,
    ) -> anyhow::Result<()> {
        let mut tx = self.begin(identity).await?;
        AuditLogConfigModel::new(&mut tx)
            .set_firehose_stream_name(&self.deployment_name(), firehose_stream_name)
            .await?;
        self.commit(tx, "set_audit_log_firehose_stream").await?;
        Ok(())
    }
}
