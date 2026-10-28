use big_brain_private_api_types::{
    DeploymentAuthPreviewArgs,
    DeploymentAuthProdArgs,
    DeploymentAuthResponse,
    DeploymentAuthWithinCurrentProjectArgs,
    MintLocalAiGatewayJwtArgs,
    MintLocalAiGatewayJwtResponse,
    TeamAndProjectForDeploymentResponse,
};
use common::{
    http::HttpError,
    types::{
        AttributionClaims,
        ProjectId,
    },
};
use errors::ErrorMetadata;

pub struct BigBrainClient {
    provision_host: String,
    access_token: String,
}

impl BigBrainClient {
    pub fn new(provision_host: String, access_token: String) -> Self {
        Self {
            provision_host,
            access_token,
        }
    }

    pub async fn get_project_and_team_for_deployment(
        &self,
        deployment_name: String,
    ) -> anyhow::Result<TeamAndProjectForDeploymentResponse> {
        let client = reqwest::Client::new();
        let host = &self.provision_host;
        let url = format!("{host}/api/deployment/{deployment_name}/team_and_project");
        let resp = client
            .get(url)
            .bearer_auth(&self.access_token)
            .send()
            .await?;
        if let Err(e) = resp.error_for_status_ref() {
            anyhow::bail!(anyhow::anyhow!(e)
                .context(format!("delete_project failed: {}", resp.text().await?)));
        }
        Ok(resp.json().await?)
    }

    pub async fn mint_local_ai_gateway_jwt(
        &self,
        deployment_name: String,
        attribution: AttributionClaims,
    ) -> anyhow::Result<MintLocalAiGatewayJwtResponse> {
        let client = reqwest::Client::new();
        let mut url = reqwest::Url::parse(&self.provision_host)?;
        url.path_segments_mut()
            .map_err(|()| anyhow::anyhow!("Control plane URL must support path segments"))?
            .pop_if_empty()
            .extend([
                "v1",
                "deployments",
                deployment_name.as_str(),
                "mint_ai_gateway_jwt",
            ]);
        let resp = client
            .post(url)
            .bearer_auth(&self.access_token)
            .json(&MintLocalAiGatewayJwtArgs { attribution })
            .send()
            .await?;
        if let Err(error) = resp.error_for_status_ref() {
            let status = resp.status();
            let body = resp.bytes().await?;
            if let Ok((code, message)) = HttpError::error_message_from_bytes(&body)
                && let Some(metadata) = ErrorMetadata::from_http_status_code(status, code, message)
            {
                anyhow::bail!(anyhow::anyhow!(metadata).context("mint_local_ai_gateway_jwt failed"));
            }
            anyhow::bail!(anyhow::anyhow!(error).context(format!(
                "mint_local_ai_gateway_jwt failed: {}",
                String::from_utf8_lossy(&body)
            )));
        }
        Ok(resp.json().await?)
    }

    pub async fn delete_project(&self, project_id: ProjectId) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let host = &self.provision_host;
        let url = format!("{host}/api/dashboard/delete_project/{project_id}");
        let resp = client
            .post(url)
            .bearer_auth(&self.access_token)
            .send()
            .await?;
        if let Err(e) = resp.error_for_status_ref() {
            anyhow::bail!(anyhow::anyhow!(e)
                .context(format!("delete_project failed: {}", resp.text().await?)));
        }
        Ok(())
    }

    pub async fn deployment_credentials(
        &self,
        args: DeploymentAuthWithinCurrentProjectArgs,
    ) -> anyhow::Result<DeploymentAuthResponse> {
        let client = reqwest::Client::new();
        let url = format!(
            "{}/api/deployment/authorize_within_current_project",
            self.provision_host
        );
        let resp = client
            .post(url)
            .bearer_auth(&self.access_token)
            .json(&args)
            .send()
            .await?;
        if let Err(e) = resp.error_for_status_ref() {
            anyhow::bail!(anyhow::anyhow!(e).context(format!(
                "deployment_credentials failed: {}",
                resp.text().await?
            )));
        }
        Ok(resp.json::<DeploymentAuthResponse>().await?)
    }

    pub async fn preview_deployment_credentials(
        &self,
        args: DeploymentAuthPreviewArgs,
    ) -> anyhow::Result<DeploymentAuthResponse> {
        let client = reqwest::Client::new();
        let url = format!("{}/api/deployment/authorize_preview", self.provision_host);
        let resp = client
            .post(url)
            .bearer_auth(&self.access_token)
            .json(&args)
            .send()
            .await?;
        if let Err(e) = resp.error_for_status_ref() {
            anyhow::bail!(anyhow::anyhow!(e).context(format!(
                "preview_deployment_credentials failed: {}",
                resp.text().await?
            )));
        }
        Ok(resp.json::<DeploymentAuthResponse>().await?)
    }

    pub async fn prod_deployment_credentials(
        &self,
        args: DeploymentAuthProdArgs,
    ) -> anyhow::Result<DeploymentAuthResponse> {
        let client = reqwest::Client::new();
        let url = format!("{}/api/deployment/authorize_prod", self.provision_host);
        let resp = client
            .post(url)
            .bearer_auth(&self.access_token)
            .json(&args)
            .send()
            .await?;
        if let Err(e) = resp.error_for_status_ref() {
            anyhow::bail!(anyhow::anyhow!(e).context(format!(
                "prod_deployment_credentials failed: {}",
                resp.text().await?
            )));
        }
        Ok(resp.json::<DeploymentAuthResponse>().await?)
    }
}
