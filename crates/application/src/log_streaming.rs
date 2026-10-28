use common::{
    document::ParsedDocument,
    execution_context::RequestMetadata,
    runtime::Runtime,
};
use database::{
    Database,
    MAX_OCC_FAILURES,
};
use errors::ErrorMetadata;
use futures::FutureExt;
use keybroker::Identity;
use model::{
    backend_info::BackendInfoModel,
    deployment_audit_log::{
        integration_config::{
            audit_config,
            audit_credential_changes,
        },
        types::DeploymentAuditLogEvent,
    },
    log_sinks::{
        types::{
            LogSinksRow,
            SinkConfig,
            SinkState,
            SinkType,
        },
        LogSinksModel,
    },
};
use usage_tracking::FunctionUsageTracker;
use value::{
    DeveloperDocumentId,
    ResolvedDocumentId,
};

use crate::Application;

pub struct LogSinkWithId {
    pub id: ResolvedDocumentId,
    pub config: SinkConfig,
    pub status: SinkState,
}

pub async fn add_local_log_sink_on_startup<RT: Runtime>(
    db: Database<RT>,
    path: String,
) -> anyhow::Result<()> {
    let mut tx = db.begin(Identity::system()).await?;
    let mut log_sink_model = LogSinksModel::new(&mut tx);
    log_sink_model
        .add_on_startup(SinkConfig::Local(path.clone()))
        .await?;
    db.commit_with_write_source(tx, "add_local_log_sink_startup")
        .await?;
    tracing::info!("Local log sink configured at {path}.");
    Ok(())
}

impl<RT: Runtime> Application<RT> {
    pub async fn add_log_sink(
        &self,
        identity: Identity,
        request_metadata: RequestMetadata,
        config: SinkConfig,
    ) -> anyhow::Result<DeveloperDocumentId> {
        let sink_type = config.sink_type().as_str().to_string();
        let id = self
            .execute_with_audit_log_events_and_occ_retries(
                identity,
                request_metadata,
                "add_log_sink",
                |tx| {
                    let config = config.clone();
                    let sink_type = sink_type.clone();
                    async move {
                        let audit_config = audit_config(&config);
                        let id = LogSinksModel::new(tx).add_or_update(config).await?;
                        let id_str = id.encode();
                        Ok((
                            id,
                            vec![DeploymentAuditLogEvent::CreateIntegration {
                                id: id_str,
                                r#type: sink_type,
                                config: audit_config,
                            }],
                        ))
                    }
                    .boxed()
                    .into()
                },
            )
            .await?;
        Ok(id)
    }

    pub async fn patch_log_sink_config(
        &self,
        identity: Identity,
        request_metadata: RequestMetadata,
        id: &String,
        config: SinkConfig,
    ) -> anyhow::Result<()> {
        self.patch_log_sink_config_and_status(identity, request_metadata, id, config, false)
            .await
    }

    /// Like `patch_log_sink_config`, and with `reset_to_pending` also sets the
    /// status to Pending in the same transaction, so no status written for the
    /// old config can land after the new config.
    pub async fn patch_log_sink_config_and_status(
        &self,
        identity: Identity,
        request_metadata: RequestMetadata,
        id: &String,
        config: SinkConfig,
        reset_to_pending: bool,
    ) -> anyhow::Result<()> {
        let sink_type = config.sink_type().as_str().to_string();
        let developer_id = DeveloperDocumentId::decode(id).map_err(|_| {
            anyhow::anyhow!(ErrorMetadata::bad_request(
                "InvalidLogStreamId",
                "The log stream id is invalid"
            ))
        })?;
        self.execute_with_audit_log_events_and_occ_retries(
            identity,
            request_metadata,
            "patch_log_sink_config",
            |tx| {
                let config = config.clone();
                let sink_type = sink_type.clone();
                let id_str = id.clone();
                async move {
                    let mut model = LogSinksModel::new(tx);
                    let previous_config = model.must_get(developer_id).await?.config.clone();
                    let previous = audit_config(&previous_config);
                    let current = audit_config(&config);
                    let credential_changes = audit_credential_changes(&previous_config, &config);
                    model.patch_config(developer_id, config).await?;
                    if reset_to_pending {
                        model.patch_status(developer_id, SinkState::Pending).await?;
                    }
                    Ok((
                        (),
                        vec![DeploymentAuditLogEvent::UpdateIntegration {
                            id: id_str,
                            r#type: sink_type,
                            previous,
                            current,
                            credential_changes,
                        }],
                    ))
                }
                .boxed()
                .into()
            },
        )
        .await?;
        Ok(())
    }

    pub async fn reset_log_sink_to_pending(&self, id: &String) -> anyhow::Result<()> {
        let developer_id = DeveloperDocumentId::decode(id).map_err(|_| {
            anyhow::anyhow!(ErrorMetadata::bad_request(
                "InvalidLogStreamId",
                "The log stream id is invalid"
            ))
        })?;
        self.execute_with_occ_retries(
            Identity::system(),
            FunctionUsageTracker::new(),
            MAX_OCC_FAILURES,
            "reset_log_sink_to_pending",
            |tx| {
                async move {
                    LogSinksModel::new(tx)
                        .patch_status(developer_id, SinkState::Pending)
                        .await
                }
                .boxed()
                .into()
            },
        )
        .await?;
        Ok(())
    }

    pub async fn get_log_sink(&self, sink_type: &SinkType) -> anyhow::Result<Option<SinkConfig>> {
        let mut tx = self.begin(Identity::system()).await?;
        let mut model = LogSinksModel::new(&mut tx);
        let sink = model
            .get_by_provider(sink_type.clone())
            .await?
            .map(|sink| sink.into_value().config);
        Ok(sink)
    }

    pub async fn must_get_log_sink_by_id(&self, id: &str) -> anyhow::Result<LogSinkWithId> {
        let mut tx = self.begin(Identity::system()).await?;

        let id = DeveloperDocumentId::decode(id).map_err(|_| {
            anyhow::anyhow!(ErrorMetadata::bad_request(
                "InvalidLogStreamId",
                "The log stream id is invalid"
            ))
        })?;

        let row = LogSinksModel::new(&mut tx).must_get(id).await?;
        Ok(LogSinkWithId {
            id: row.id(),
            config: row.config.clone(),
            status: row.status.clone(),
        })
    }

    pub async fn list_log_sinks(&self) -> anyhow::Result<Vec<LogSinkWithId>> {
        let mut tx = self.begin(Identity::system()).await?;
        let sinks = LogSinksModel::new(&mut tx)
            .get_all()
            .await?
            .into_iter()
            .map(|sink| {
                let id = sink.id();
                let value = sink.into_value();
                LogSinkWithId {
                    id,
                    config: value.config,
                    status: value.status,
                }
            })
            .collect();
        Ok(sinks)
    }

    pub async fn remove_log_sink_by_id(
        &self,
        identity: Identity,
        request_metadata: RequestMetadata,
        id: String,
    ) -> anyhow::Result<()> {
        let developer_id = DeveloperDocumentId::decode(&id).map_err(|_| {
            anyhow::anyhow!(ErrorMetadata::bad_request(
                "InvalidLogStreamId",
                "The log stream id is invalid"
            ))
        })?;
        self.execute_with_audit_log_events_and_occ_retries(
            identity,
            request_metadata,
            "remove_log_sink",
            |tx| {
                let id = id.clone();
                async move {
                    let row: ParsedDocument<LogSinksRow> =
                        LogSinksModel::new(tx).must_get(developer_id).await?;
                    let sink_type = row.config.sink_type().as_str().to_string();
                    let config = audit_config(&row.config);
                    LogSinksModel::new(tx)
                        .mark_for_removal(developer_id)
                        .await?;
                    Ok((
                        (),
                        vec![DeploymentAuditLogEvent::DeleteIntegration {
                            id,
                            r#type: sink_type,
                            config,
                        }],
                    ))
                }
                .boxed()
                .into()
            },
        )
        .await?;
        Ok(())
    }

    pub async fn ensure_log_streaming_allowed(&self, identity: Identity) -> anyhow::Result<()> {
        let mut tx = self.begin(identity).await?;
        BackendInfoModel::new(&mut tx)
            .ensure_log_streaming_allowed()
            .await
    }

    /// Ensures the deployment is entitled to subscribe a log stream to the
    /// `custom_audit` topic.
    pub async fn ensure_custom_audit_logs_in_log_streams_allowed(
        &self,
        identity: Identity,
    ) -> anyhow::Result<()> {
        let mut tx = self.begin(identity).await?;
        BackendInfoModel::new(&mut tx)
            .ensure_custom_audit_logs_in_log_streams_allowed()
            .await
    }
}
