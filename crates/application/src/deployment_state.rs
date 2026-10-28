use common::{
    execution_context::RequestMetadata,
    runtime::Runtime,
    types::{
        BackendState,
        SystemStopState,
        UsageLimitStopState,
        UserStopState,
    },
};
use database::Transaction;
use keybroker::Identity;
use model::{
    backend_state::BackendStateModel,
    deployment_audit_log::types::DeploymentAuditLogEvent,
};

use crate::Application;

impl<RT: Runtime> Application<RT> {
    pub async fn set_user_stop_state(
        &self,
        identity: Identity,
        request_metadata: RequestMetadata,
        new_user_state: UserStopState,
    ) -> anyhow::Result<()> {
        let mut tx = self.begin(identity).await?;
        let mut model = BackendStateModel::new(&mut tx);
        if model.set_user_stop_state(new_user_state).await?.is_none() {
            return Ok(());
        }
        let deployment_audit_log_event = match new_user_state {
            UserStopState::Paused => DeploymentAuditLogEvent::PauseDeployment,
            UserStopState::None => DeploymentAuditLogEvent::UnpauseDeployment,
        };
        self.commit_with_audit_log_events(
            tx,
            vec![deployment_audit_log_event],
            request_metadata,
            "set_user_stop_state",
        )
        .await?;
        Ok(())
    }

    pub async fn set_system_stop_state(
        &self,
        identity: Identity,
        request_metadata: RequestMetadata,
        new_system_state: SystemStopState,
    ) -> anyhow::Result<()> {
        let mut tx = self.begin(identity).await?;
        let mut model = BackendStateModel::new(&mut tx);
        let Some(old_state) = model.set_system_stop_state(new_system_state).await? else {
            return Ok(());
        };
        let deployment_audit_log_event = DeploymentAuditLogEvent::ChangeSystemStopState {
            old_state: old_state.system,
            new_state: new_system_state,
        };
        self.commit_with_audit_log_events(
            tx,
            vec![deployment_audit_log_event],
            request_metadata,
            "set_system_stop_state",
        )
        .await?;
        Ok(())
    }

    pub async fn set_usage_limit_stop_state(
        &self,
        transaction: &mut Transaction<RT>,
        new_usage_limit_state: UsageLimitStopState,
    ) -> anyhow::Result<Option<BackendState>> {
        BackendStateModel::new(transaction)
            .set_usage_limit_stop_state(new_usage_limit_state)
            .await
    }
}
