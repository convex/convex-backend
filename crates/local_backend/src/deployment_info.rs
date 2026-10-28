use axum::{
    extract::FromRef,
    response::IntoResponse,
};
use common::{
    http::{
        extract::{
            Json,
            MtState,
        },
        HttpResponseError,
    },
    types::{
        DeploymentId,
        DeploymentType,
        ProjectId,
        TeamId,
    },
};
use model::backend_info::BackendInfoModel;
use roles::RequireDeploymentOp;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;

use crate::{
    authentication::ExtractIdentity,
    LocalAppState,
};

#[derive(Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DeploymentInfoResponse {
    #[serde(rename_all = "camelCase")]
    Cloud {
        team_id: TeamId,
        project_id: ProjectId,
        project_name: Option<String>,
        project_slug: Option<String>,
        id: DeploymentId,
        deployment_type: DeploymentType,
        reference: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    SelfHosted {},
}

/// Get deployment info
///
/// Returns identity information about this deployment.
#[utoipa::path(
    get,
    path = "/deployment_info",
    tag = "Deployment Info",
    operation_id = "get deployment info",
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
    responses(
        (status = 200, body = DeploymentInfoResponse)
    ),
)]
pub async fn get_deployment_info(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ViewData)?;

    let mut tx = st.application.begin(identity).await?;
    let backend_info_doc = BackendInfoModel::new(&mut tx).get().await?;

    let response = match backend_info_doc {
        Some(doc) => {
            let backend_info = &**doc;
            DeploymentInfoResponse::Cloud {
                team_id: backend_info.team,
                project_id: backend_info.project,
                project_name: backend_info.project_name.clone(),
                project_slug: backend_info.project_slug.clone(),
                id: backend_info.deployment,
                deployment_type: backend_info.deployment_type,
                reference: backend_info.deployment_ref.clone(),
            }
        },
        None => DeploymentInfoResponse::SelfHosted {},
    };

    Ok(Json(response))
}

pub fn platform_router<S>() -> OpenApiRouter<S>
where
    LocalAppState: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    OpenApiRouter::new().routes(utoipa_axum::routes!(get_deployment_info))
}
