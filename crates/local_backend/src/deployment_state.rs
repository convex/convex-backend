use axum::{
    extract::FromRef,
    response::IntoResponse,
};
use common::{
    http::{
        extract::MtState,
        ExtractRequestMetadata,
        HttpResponseError,
    },
    types::{
        SystemStopState,
        UserStopState,
    },
};
use errors::ErrorMetadata;
use http::StatusCode;
use model::backend_state::BackendStateModel;
use roles::RequireDeploymentOp;
use utoipa_axum::router::OpenApiRouter;

use crate::{
    authentication::ExtractIdentity,
    LocalAppState,
};

/// Pause deployment
///
/// Disables a deployment without deleting any data. The deployment will not
/// operate until it is unpaused. While a deployment is paused, new functions
/// calls will return an error, scheduled jobs will queue and run when the
/// deployment is resumed, and cron jobs will be skipped. This means that no
/// function calls or bandwidth usage will be charged while the deployment is
/// paused, but storage costs will still apply.
#[utoipa::path(
    post,
    path = "/pause_deployment",
    tag = "Pause/Unpause",
    responses((status = 200)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn pause_deployment(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::PauseDeployment)?;

    let mut tx = st.application.begin(identity.clone()).await?;
    let current_state = BackendStateModel::new(&mut tx)
        .get_backend_state()
        .await?
        .into_value();
    if current_state.system != SystemStopState::None {
        return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
            "PauseDeploymentFailed",
            "Deployment is currently disabled or suspended by Convex and cannot be paused."
        ))
        .into());
    }

    st.application
        .set_user_stop_state(identity, request_metadata, UserStopState::Paused)
        .await?;

    Ok(StatusCode::OK)
}

/// Unpause deployment
///
/// Reenables a deployment that was previously paused. The deployment will
/// resume normal operation, including any scheduled jobs that were queued while
/// paused.
#[utoipa::path(
    post,
    path = "/unpause_deployment",
    tag = "Pause/Unpause",
    responses((status = 200)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn unpause_deployment(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::UnpauseDeployment)?;

    let mut tx = st.application.begin(identity.clone()).await?;
    let current_state = BackendStateModel::new(&mut tx)
        .get_backend_state()
        .await?
        .into_value();
    if current_state.system != SystemStopState::None {
        return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
            "UnpauseDeploymentFailed",
            "Deployment is currently disabled or suspended by Convex and cannot be unpaused."
        ))
        .into());
    }
    if current_state.user != UserStopState::Paused {
        return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
            "UnpauseDeploymentFailed",
            "Deployment is not currently paused."
        ))
        .into());
    }

    st.application
        .set_user_stop_state(identity, request_metadata, UserStopState::None)
        .await?;

    Ok(StatusCode::OK)
}

pub fn platform_router<S>() -> OpenApiRouter<S>
where
    LocalAppState: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(pause_deployment))
        .routes(utoipa_axum::routes!(unpause_deployment))
}
