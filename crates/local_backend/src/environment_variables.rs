use application::EnvVarChange;
use axum::{
    extract::{
        FromRef,
        State,
    },
    response::IntoResponse,
    routing::get,
};
use common::{
    http::{
        extract::{
            Json,
            MtState,
        },
        ExtractRequestMetadata,
        HttpResponseError,
    },
    types::MemberId,
};
use errors::ErrorMetadata;
use http::StatusCode;
use model::environment_variables::{
    types::{
        EnvVarName,
        EnvVarValue,
        EnvVarVisibility,
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
    // Only the system identity uses the member override, so it should be
    // excluded from the public API schema.
    #[serde(default)]
    #[schema(ignore)]
    member_id: Option<MemberId>,
}

fn is_secret(visibility: EnvVarVisibility) -> bool {
    match visibility {
        EnvVarVisibility::Secret => true,
        EnvVarVisibility::Config => false,
    }
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
    Json(UpdateEnvVarsRequest { changes, member_id }): Json<UpdateEnvVarsRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteEnvironmentVariables)?;
    if member_id.is_some() && !identity.is_system() {
        return Err(anyhow::anyhow!(ErrorMetadata::forbidden(
            "MemberIdOverrideRequiresSystemIdentity",
            "Only system identities can override the audit log member ID.",
        ))
        .into());
    }

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
        .commit_with_audit_log_events_with_member_override(
            tx,
            audit_events,
            member_id,
            request_metadata,
            "update_env_vars",
        )
        .await?;

    Ok(StatusCode::OK)
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListEnvVarsResponse {
    /// The value of every environment variable that isn't a secret, by
    /// name.
    environment_variables: std::collections::BTreeMap<String, String>,
}

/// List environment variables
///
/// Get the environment variables in a deployment that aren't secrets.
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
    let env_vars = EnvironmentVariablesModel::new(&mut tx)
        .get_all_non_secret()
        .await?;

    let environment_variables = env_vars
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();

    Ok(Json(ListEnvVarsResponse {
        environment_variables,
    }))
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvVarResponse {
    name: String,
    /// Null for a secret: its value can't be read.
    value: Option<String>,
    /// Whether the variable is a secret: functions can read it, but the
    /// dashboard, the CLI and this API can only overwrite or delete it.
    /// Null while that is undecided; the value is readable until then.
    secret: Option<bool>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListAllEnvVarsResponse {
    /// Every environment variable in the deployment, sorted by name.
    items: Vec<EnvVarResponse>,
}

/// List environment variables
///
/// Get all environment variables in a deployment, secrets included.
/// Secrets are listed without their value.
#[utoipa::path(
    get,
    path = "/environment_variables",
    tag = "Environment Variables",
    responses(
        (status = 200, body = ListAllEnvVarsResponse)
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn list_all_environment_variables(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ViewEnvironmentVariables)?;

    let mut tx = st.application.begin(identity).await?;
    let items = EnvironmentVariablesModel::new(&mut tx)
        .get_all_persisted()
        .await?
        .into_iter()
        .map(|(name, env_var)| EnvVarResponse {
            name: name.to_string(),
            value: (!env_var.is_secret()).then(|| env_var.variable.value().to_string()),
            secret: env_var.visibility.map(is_secret),
        })
        .collect();

    Ok(Json(ListAllEnvVarsResponse { items }))
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
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(
            update_environment_variables,
            list_environment_variables
        ))
        // Registered as a plain route, which keeps it out of the OpenAPI
        // spec, and so out of the docs and the generated clients, while its
        // name is being settled.
        .route(
            "/environment_variables",
            get(list_all_environment_variables),
        )
}
