use async_trait::async_trait;
use common::{
    execution_context::RequestId,
    types::{
        AttributionClaims,
        DeploymentMetadata,
        ProjectId,
        TeamId,
    },
};

/// Mints short-lived JWTs accepted by the AI gateway.
#[async_trait]
pub trait AiGatewayJwtMinter: Send + Sync {
    /// All arguments must come from trusted backend
    /// metadata or execution state rather than function input.
    async fn mint(
        &self,
        deployment: &DeploymentMetadata,
        team_id: Option<TeamId>,
        project_id: Option<ProjectId>,
        attribution: AttributionClaims,
        request_id: &RequestId,
    ) -> anyhow::Result<String>;
}
