use application::{
    deploy_config::ModuleJson,
    valid_identifier::ValidIdentifier,
};
use axum::{
    debug_handler,
    extract::State,
    response::IntoResponse,
};
use common::{
    components::ComponentId,
    http::{
        extract::{
            FromMtState,
            Json,
            MtState,
            Query,
        },
        ExtractClientVersion,
        ExtractRequestId,
        ExtractRequestMetadata,
        HttpResponseError,
    },
    runtime::try_join,
    shapes::{
        dashboard_shape_json,
        reduced::ReducedShape,
    },
    types::FunctionCaller,
    RequestContext,
};
use database::IndexModel;
use http::StatusCode;
use model::{
    config::types::ModuleConfig,
    virtual_system_mapping,
};
use roles::RequireDeploymentOp;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::json;
use udf::helpers::UdfArgsJson;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use value::{
    TableName,
    TableNamespace,
};

use crate::{
    admin::must_be_admin_from_key,
    authentication::ExtractIdentity,
    public_api::{
        export_value,
        UdfResponse,
    },
    scheduling::{
        __path_delete_scheduled_functions_table,
        delete_scheduled_functions_table,
    },
    schema::IndexMetadataResponse,
    LocalAppState,
};

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTableArgs {
    table_names: Vec<String>,
    component_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteComponentArgs {
    component_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShapesArgs {
    component: Option<String>,
}

/// Get table shapes
///
/// Returns the schema shapes for all tables in the specified component.
#[utoipa::path(
    get,
    path = "/shapes2",
    params(
        ("component" = Option<String>, Query, description = "Component ID to get shapes for")
    ),
    responses((status = 200, body = serde_json::Value)),
)]
pub async fn shapes2(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(ShapesArgs { component }): Query<ShapesArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ViewData)?;
    let component = ComponentId::deserialize_from_string(component.as_deref())?;
    let snapshot = st.application.latest_snapshot()?;
    let table_shapes = st.application.table_shapes();
    let mapping = snapshot.table_mapping().namespace(component.into());

    // This can block the CPU for a long time so as a stopgap, spawn
    // it onto its own task
    let out = try_join("shapes2", async move {
        let mut out = serde_json::Map::new();
        for (namespace, table_name) in snapshot.table_registry.user_table_names() {
            if TableNamespace::from(component) != namespace {
                continue;
            }
            let table_shape = table_shapes
                .as_ref()
                .and_then(|shapes| shapes.table_shape(&mapping, table_name));

            let shape = match table_shape {
                Some(table_shape) => ReducedShape::from_type(
                    table_shape.inferred_type(),
                    &mapping.table_number_exists(),
                ),
                // Table shapes haven't been published yet, or the table was
                // created after the last shape checkpoint; use `Unknown` in the
                // meantime.
                None => ReducedShape::Unknown,
            };
            let json = dashboard_shape_json(&shape, &mapping, virtual_system_mapping())?;
            out.insert(String::from(table_name.clone()), json);
        }
        Ok(out)
    })
    .await?;
    Ok(Json(out))
}

/// Delete database tables
///
/// Deletes the specified tables from the database.
#[utoipa::path(
    post,
    path = "/delete_tables",
    request_body = DeleteTableArgs,
    responses((status = 200)),
)]
pub async fn delete_tables(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(DeleteTableArgs {
        table_names,
        component_id,
    }): Json<DeleteTableArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteData)?;
    let table_names = table_names
        .into_iter()
        .map(|t| Ok(t.parse::<ValidIdentifier<TableName>>()?.0))
        .collect::<anyhow::Result<_>>()?;
    let component_id = ComponentId::deserialize_from_string(component_id.as_deref())?;
    st.application
        .delete_tables(&identity, request_metadata, table_names, component_id)
        .await?;
    Ok(StatusCode::OK)
}

/// Delete component
///
/// Deletes the specified component and all its associated data.
#[utoipa::path(
    post,
    path = "/delete_component",
    request_body = DeleteComponentArgs,
    responses((status = 200)),
)]
pub async fn delete_component(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(DeleteComponentArgs { component_id }): Json<DeleteComponentArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteData)?;
    let component_id = ComponentId::deserialize_from_string(component_id.as_deref())?;
    st.application
        .delete_component(&identity, request_metadata, component_id)
        .await?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetIndexesArgs {
    component_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct GetIndexesResponse {
    #[schema(value_type = Vec<Object>)]
    indexes: Vec<IndexMetadataResponse>,
}

/// Get database indexes
///
/// Returns metadata about database indexes for the specified component.
#[utoipa::path(
    get,
    path = "/get_indexes",
    params(
        ("component_id" = Option<String>, Query, description = "Component ID to get indexes for")
    ),
    responses((status = 200, body = GetIndexesResponse)),
)]
pub async fn get_indexes(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(GetIndexesArgs { component_id }): Query<GetIndexesArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ViewData)?;
    let component_id = ComponentId::deserialize_from_string(component_id.as_deref())?;
    let mut tx = st.application.begin(identity.clone()).await?;
    let indexes = IndexModel::new(&mut tx)
        .get_application_indexes(TableNamespace::from(component_id))
        .await?;
    Ok(Json(GetIndexesResponse {
        indexes: indexes
            .into_iter()
            .map(|idx| idx.into_value().try_into())
            .collect::<anyhow::Result<_>>()?,
    }))
}

/// Check admin key validity
///
/// This endpoint checks if the admin key included in the header is valid for
/// this instance. Returns the allowed operations and read-only status for the
/// key so the dashboard can show appropriate disabled states.
#[utoipa::path(
    get,
    path = "/check_admin_key",
    responses((status = 200, body = serde_json::Value)),
    tag = "public_api"
)]
pub async fn check_admin_key(
    MtState(_st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    let admin = match &identity {
        keybroker::Identity::DeploymentAdmin(admin) | keybroker::Identity::ActingUser(admin, _) => {
            admin
        },
        _ => {
            return Err(
                anyhow::anyhow!(keybroker::bad_admin_key_error(identity.instance_name())).into(),
            );
        },
    };
    let allowed_ops = admin
        .allowed_ops()
        .map_err(|e| anyhow::anyhow!(e))?
        .to_vec();
    let is_read_only = admin.is_read_only();
    let serialized_ops: Vec<serde_json::Value> = allowed_ops
        .iter()
        .map(|op| serde_json::to_value(op).unwrap())
        .collect();
    Ok(Json(json!({
        "success": true,
        "allowedOps": serialized_ops,
        "isReadOnly": is_read_only,
    })))
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunTestFunctionArgs {
    admin_key: String,
    #[schema(value_type = Object)]
    bundle: ModuleJson,
    #[schema(value_type = Object)]
    args: UdfArgsJson,
    format: String,
    component_id: Option<String>,
}

/// Run test function
///
/// Executes a test function with the provided arguments and bundle.
#[utoipa::path(
    post,
    path = "/run_test_function",
    request_body = RunTestFunctionArgs,
    responses((status = 200, body = serde_json::Value)),
)]
#[debug_handler]
pub async fn run_test_function(
    State(st): State<LocalAppState>,
    ExtractRequestId(request_id): ExtractRequestId,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    ExtractClientVersion(client_version): ExtractClientVersion,
    Json(req): Json<RunTestFunctionArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let identity = must_be_admin_from_key(
        st.application.app_auth(),
        st.instance_name.clone(),
        req.admin_key.clone(),
    )
    .await?;
    identity.require_operation(keybroker::DeploymentOp::RunTestQuery)?;
    let args = req.args.into_serialized_args()?;
    let module: ModuleConfig = req.bundle.try_into()?;
    let component_id = ComponentId::deserialize_from_string(req.component_id.as_deref())?;
    let request_context = RequestContext::new(request_id, request_metadata);
    let udf_return = st
        .application
        .execute_standalone_module(
            request_context,
            module,
            args,
            identity,
            FunctionCaller::Tester(client_version.clone()),
            component_id,
        )
        .await?;
    let value_format = Some(req.format.parse()?);
    let response = match udf_return {
        Ok(result) => UdfResponse::Success {
            value: export_value(result.value, value_format, client_version)?,
            log_lines: result.log_lines,
        },
        Err(error) => {
            UdfResponse::error(error.error, error.log_lines, value_format, client_version)?
        },
    };
    Ok(Json(response))
}

pub fn local_only_dashboard_router() -> OpenApiRouter<crate::LocalAppState> {
    OpenApiRouter::new()
}

// Routes with the same handlers for the local backend + closed source backend
pub fn common_dashboard_api_router<S>() -> OpenApiRouter<S>
where
    LocalAppState: FromMtState<S>,
    S: Clone + Send + Sync + 'static,
{
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(check_admin_key))
        .routes(utoipa_axum::routes!(shapes2))
        .routes(utoipa_axum::routes!(get_indexes))
        .routes(utoipa_axum::routes!(delete_tables))
        .routes(utoipa_axum::routes!(delete_component))
        .routes(utoipa_axum::routes!(delete_scheduled_functions_table))
}
