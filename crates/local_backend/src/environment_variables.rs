use application::EnvVarChange;
use axum::{
    extract::{
        FromRef,
        State,
    },
    response::IntoResponse,
};
use common::http::{
    extract::{
        Json,
        MtState,
    },
    ExtractRequestMetadata,
    HttpResponseError,
};
use http::StatusCode;
use model::environment_variables::{
    types::{
        EnvVarName,
        EnvVarValue,
        EnvironmentVariable,
    },
    EnvironmentVariablesModel,
};
use roles::RequireDeploymentOp;
use serde::{
    Deserialize,
    Serialize,
};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;

use crate::{
    authentication::ExtractIdentity,
    LocalAppState,
};

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEnvVarRequest {
    name: String,
    value: Option<String>, // None → delete existing
}

impl UpdateEnvVarRequest {
    pub async fn into_env_var_changes(self) -> anyhow::Result<Vec<EnvVarChange>> {
        match self {
            UpdateEnvVarRequest {
                name,
                value: Some(value),
            } => {
                let env_var = validate_env_var(&name, &value)?;
                Ok(vec![EnvVarChange::Set(env_var)])
            },
            UpdateEnvVarRequest { name, value: None } => {
                let name = name.parse()?;
                Ok(vec![EnvVarChange::Unset(name)])
            },
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateEnvVarsRequest {
    changes: Vec<UpdateEnvVarRequest>,
}

/// Update environment variables
///
/// Update one or many environment variables in a deployment.
/// This will invalidate all subscriptions, since environment variables
/// are accessible in queries but are not part of the cache key of a query
/// result.
#[utoipa::path(
    post,
    path = "/update_environment_variables",
    tag = "Environment Variables",
    request_body = UpdateEnvVarsRequest,
    responses((status = 200)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn update_environment_variables(
    State(st): State<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(UpdateEnvVarsRequest { changes }): Json<UpdateEnvVarsRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteEnvironmentVariables)?;

    let mut env_var_changes = vec![];
    for change in changes {
        env_var_changes.extend(change.into_env_var_changes().await?);
    }
    env_var_changes.sort();

    let mut tx = st.application.begin(identity).await?;
    let audit_events = st
        .application
        .update_environment_variables(&mut tx, env_var_changes)
        .await?;

    st.application
        .commit_with_audit_log_events(tx, audit_events, request_metadata, "update_env_vars")
        .await?;

    Ok(StatusCode::OK)
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListEnvVarsResponse {
    environment_variables: std::collections::BTreeMap<String, String>,
}

/// List environment variables
///
/// Get all environment variables in a deployment.
/// In the future this might not include "secret" environment
/// variables.
#[utoipa::path(
    get,
    path = "/list_environment_variables",
    tag = "Environment Variables",
    responses(
        (status = 200, body = ListEnvVarsResponse)
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn list_environment_variables(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ViewEnvironmentVariables)?;

    let mut tx = st.application.begin(identity).await?;
    let env_vars = EnvironmentVariablesModel::new(&mut tx).get_all().await?;

    let environment_variables = env_vars
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();

    Ok(Json(ListEnvVarsResponse {
        environment_variables,
    }))
}

fn validate_env_var(name: &String, value: &String) -> anyhow::Result<EnvironmentVariable> {
    let name: EnvVarName = name.parse()?;
    let value: EnvVarValue = value.parse()?;
    Ok(EnvironmentVariable::new(name, value))
}

pub fn platform_router<S>() -> OpenApiRouter<S>
where
    LocalAppState: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    OpenApiRouter::new().routes(utoipa_axum::routes!(
        update_environment_variables,
        list_environment_variables
    ))
}
