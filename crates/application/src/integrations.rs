use common::{
    document::ParsedDocument,
    execution_context::RequestMetadata,
    runtime::Runtime,
};
use database::MAX_OCC_FAILURES;
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
    integrations::{
        types::{
            IntegrationConfig,
            IntegrationDocument,
            IntegrationState,
            IntegrationType,
        },
        IntegrationsModel,
    },
};
use usage_tracking::FunctionUsageTracker;
use value::{
    DeveloperDocumentId,
    ResolvedDocumentId,
};

use crate::Application;

/// The statuses a config change can move an integration to.
#[derive(Clone, Copy, Debug)]
pub enum IntegrationStartState {
    Pending,
    Restarting,
}

impl From<IntegrationStartState> for IntegrationState {
    fn from(status: IntegrationStartState) -> Self {
        match status {
            IntegrationStartState::Pending => IntegrationState::Pending,
            IntegrationStartState::Restarting => IntegrationState::Restarting,
        }
    }
}

pub struct IntegrationWithId {
    pub id: ResolvedDocumentId,
    pub config: IntegrationConfig,
    pub status: IntegrationState,
}

impl<RT: Runtime> Application<RT> {
    pub async fn add_integration(
        &self,
        identity: Identity,
        request_metadata: RequestMetadata,
        config: IntegrationConfig,
    ) -> anyhow::Result<DeveloperDocumentId> {
        let integration_type = config.integration_type().as_str().to_string();
        let id = self
            .execute_with_audit_log_events_and_occ_retries(
                identity,
                request_metadata,
                "add_integration",
                |tx| {
                    let config = config.clone();
                    let integration_type = integration_type.clone();
                    async move {
                        let audit_config = audit_config(&config);
                        let id = IntegrationsModel::new(tx).add_or_update(config).await?;
                        let id_str = id.encode();
                        Ok((
                            id,
                            vec![DeploymentAuditLogEvent::CreateIntegration {
                                id: id_str,
                                r#type: integration_type,
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

    pub async fn patch_integration_config(
        &self,
        identity: Identity,
        request_metadata: RequestMetadata,
        id: &String,
        config: IntegrationConfig,
    ) -> anyhow::Result<()> {
        self.patch_integration_config_and_status(identity, request_metadata, id, config, None)
            .await
    }

    /// Like `patch_integration_config`, and with `status` also sets the status
    /// in the same transaction, so no status written for the old config can
    /// land after the new config.
    pub async fn patch_integration_config_and_status(
        &self,
        identity: Identity,
        request_metadata: RequestMetadata,
        id: &String,
        config: IntegrationConfig,
        status: Option<IntegrationStartState>,
    ) -> anyhow::Result<()> {
        let integration_type = config.integration_type().as_str().to_string();
        let developer_id = DeveloperDocumentId::decode(id).map_err(|_| {
            anyhow::anyhow!(ErrorMetadata::bad_request(
                "InvalidLogStreamId",
                "The log stream id is invalid"
            ))
        })?;
        self.execute_with_audit_log_events_and_occ_retries(
            identity,
            request_metadata,
            "patch_integration_config",
            |tx| {
                let config = config.clone();
                let integration_type = integration_type.clone();
                let id_str = id.clone();
                async move {
                    let mut model = IntegrationsModel::new(tx);
                    let previous_config = model.must_get(developer_id).await?.config.clone();
                    let previous = audit_config(&previous_config);
                    let current = audit_config(&config);
                    let credential_changes = audit_credential_changes(&previous_config, &config);
                    model.patch_config(developer_id, config).await?;
                    if let Some(status) = status {
                        model.patch_status(developer_id, status.into()).await?;
                    }
                    Ok((
                        (),
                        vec![DeploymentAuditLogEvent::UpdateIntegration {
                            id: id_str,
                            r#type: integration_type,
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

    pub async fn reset_integration_to_pending(&self, id: &String) -> anyhow::Result<()> {
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
            "reset_integration_to_pending",
            |tx| {
                async move {
                    IntegrationsModel::new(tx)
                        .patch_status(developer_id, IntegrationState::Pending)
                        .await
                }
                .boxed()
                .into()
            },
        )
        .await?;
        Ok(())
    }

    pub async fn get_integration(
        &self,
        integration_type: &IntegrationType,
    ) -> anyhow::Result<Option<IntegrationConfig>> {
        let mut tx = self.begin(Identity::system()).await?;
        let mut model = IntegrationsModel::new(&mut tx);
        let integration = model
            .get_by_provider(integration_type.clone())
            .await?
            .map(|integration| integration.into_value().config);
        Ok(integration)
    }

    pub async fn must_get_integration_by_id(&self, id: &str) -> anyhow::Result<IntegrationWithId> {
        let mut tx = self.begin(Identity::system()).await?;

        let id = DeveloperDocumentId::decode(id).map_err(|_| {
            anyhow::anyhow!(ErrorMetadata::bad_request(
                "InvalidLogStreamId",
                "The log stream id is invalid"
            ))
        })?;

        let document = IntegrationsModel::new(&mut tx).must_get(id).await?;
        Ok(IntegrationWithId {
            id: document.id(),
            config: document.config.clone(),
            status: document.status.clone(),
        })
    }

    pub async fn list_integrations(&self) -> anyhow::Result<Vec<IntegrationWithId>> {
        let mut tx = self.begin(Identity::system()).await?;
        let integrations = IntegrationsModel::new(&mut tx)
            .get_all()
            .await?
            .into_iter()
            .map(|integration| {
                let id = integration.id();
                let value = integration.into_value();
                IntegrationWithId {
                    id,
                    config: value.config,
                    status: value.status,
                }
            })
            .collect();
        Ok(integrations)
    }

    pub async fn remove_integration_by_id(
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
            "remove_integration",
            |tx| {
                let id = id.clone();
                async move {
                    let document: ParsedDocument<IntegrationDocument> =
                        IntegrationsModel::new(tx).must_get(developer_id).await?;
                    let integration_type = document.config.integration_type().as_str().to_string();
                    let config = audit_config(&document.config);
                    IntegrationsModel::new(tx)
                        .mark_for_removal(developer_id)
                        .await?;
                    Ok((
                        (),
                        vec![DeploymentAuditLogEvent::DeleteIntegration {
                            id,
                            r#type: integration_type,
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
