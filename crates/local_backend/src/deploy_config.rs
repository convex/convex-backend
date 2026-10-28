use anyhow::Context;
use application::{
    deploy_config::{
        ModuleJson,
        NodeDependencyJson,
        PushAnalytics,
        PushMetrics,
    },
    Application,
};
use axum::{
    debug_handler,
    extract::State,
    response::IntoResponse,
};
use common::{
    components::ComponentId,
    execution_context::RequestMetadata,
    http::{
        extract::{
            Json,
            MtState,
        },
        ExtractRequestMetadata,
        HttpResponseError,
    },
    version::Version,
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use keybroker::Identity;
use model::{
    config::{
        types::{
            ConfigFile,
            ModuleConfig,
        },
        ConfigModel,
    },
    source_packages::{
        types::NodeVersion,
        SourcePackageModel,
    },
};
use roles::RequireDeploymentOp;
use runtime::prod::ProdRuntime;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as JsonValue;
use value::{
    ConvexObject,
    TableNamespace,
};

use crate::{
    admin::must_be_admin_from_key,
    EmptyResponse,
    LocalAppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConfigRequest {
    pub admin_key: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConfigResponse {
    pub config: JsonValue,
    pub modules: Vec<ModuleJson>,
    pub udf_server_version: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConfigHashesResponse {
    pub config: JsonValue,
    pub module_hashes: Vec<ModuleHashJson>,
    pub udf_server_version: Option<String>,
    pub node_version: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClientPushMetrics {
    pub typecheck: f64,
    pub bundle: f64,
    pub schema_push: f64,
    pub code_pull: f64,
    pub total_before_push: f64,
    pub module_diff_stats: Option<ModuleDiffStats>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModuleDiffStats {
    updated: ModuleDiffStat,
    added: ModuleDiffStat,
    identical: ModuleDiffStat,
    num_dropped: usize,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModuleDiffStat {
    count: usize,
    size: usize,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[expect(dead_code)]
pub struct BundledModuleInfoJson {
    name: String,
    platform: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigJson {
    pub config: ConfigFile,
    pub modules: Vec<ModuleJson>,
    pub admin_key: String,
    pub udf_server_version: String,
    // None when there is no schema file.
    pub schema_id: Option<String>,
    pub push_metrics: Option<ClientPushMetrics>,
    // Use for external node dependencies
    pub node_dependencies: Option<Vec<NodeDependencyJson>>,
    // Additional information about the names of the bundled modules.
    // We can use that for stats as well provide better debug messages.
    pub bundled_module_infos: Option<Vec<BundledModuleInfoJson>>,
    // Version of Node.js to use in the node executor.
    pub node_version: Option<String>,
}

pub struct ConfigStats {
    pub num_node_modules: usize,
    pub size_node_modules: usize,
    pub num_v8_modules: usize,
    pub size_v8_modules: usize,
}

static NODE_ENVIRONMENT: &str = "node";
impl ConfigJson {
    pub fn stats(&self) -> ConfigStats {
        let num_node_modules = self
            .modules
            .iter()
            .filter(|module| module.environment.as_deref() == Some(NODE_ENVIRONMENT))
            .count();
        let size_node_modules = self
            .modules
            .iter()
            .filter(|module| module.environment.as_deref() == Some(NODE_ENVIRONMENT))
            .fold(0, |acc, e| {
                acc + e.source.len() + e.source_map.as_ref().map_or(0, |sm| sm.len())
            });
        let size_v8_modules = self
            .modules
            .iter()
            .filter(|module| module.environment.as_deref() != Some(NODE_ENVIRONMENT))
            .fold(0, |acc, e| {
                acc + e.source.len() + e.source_map.as_ref().map_or(0, |sm| sm.len())
            });
        let num_v8_modules = self
            .modules
            .iter()
            .filter(|module| module.environment.as_deref() != Some(NODE_ENVIRONMENT))
            .count();
        ConfigStats {
            num_v8_modules,
            num_node_modules,
            size_v8_modules,
            size_node_modules,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleHashJson {
    path: String,
    hash: String,
    environment: Option<String>,
}

pub async fn get_config(
    MtState(st): MtState<LocalAppState>,
    Json(req): Json<GetConfigRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let identity = must_be_admin_from_key(
        st.application.app_auth(),
        st.instance_name.clone(),
        req.admin_key,
    )
    .await?;
    identity.require_operation(keybroker::DeploymentOp::Deploy)?;

    let mut tx = st.application.begin(identity).await?;
    let component = ComponentId::Root; // This endpoint is only used pre-components.
    let (config, modules, udf_config) = ConfigModel::new(&mut tx, component)
        .get_with_module_source(st.application.modules_storage())
        .await?;
    let config = ConvexObject::try_from(config)?.to_internal_json();

    let modules = modules.into_iter().map(|m| m.into()).collect();
    let udf_server_version = udf_config.map(|config| format!("{}", config.server_version));
    // Should this be committed?
    st.application.commit(tx, "get_config").await?;
    Ok(Json(GetConfigResponse {
        config,
        modules,
        udf_server_version,
    }))
}

pub async fn get_config_hashes(
    MtState(st): MtState<LocalAppState>,
    Json(req): Json<GetConfigRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let identity = must_be_admin_from_key(
        st.application.app_auth(),
        st.instance_name.clone(),
        req.admin_key,
    )
    .await?;
    identity.require_operation(keybroker::DeploymentOp::Deploy)?;

    let mut tx = st.application.begin(identity).await?;
    let component = ComponentId::Root; // This endpoint is not used in components push.
    let (config, modules, udf_config) = ConfigModel::new(&mut tx, component)
        .get_with_module_metadata()
        .await?;
    let module_hashes: Vec<_> = modules
        .into_iter()
        .map(|m| ModuleHashJson {
            path: m.path.clone().into(),
            hash: m.sha256.as_hex(),
            environment: Some(m.environment.to_string()),
        })
        .collect();
    let config = ConvexObject::try_from(config)?;
    let config: JsonValue = config.to_internal_json();

    let node_version = SourcePackageModel::new(&mut tx, TableNamespace::Global)
        .get_latest()
        .await?
        .and_then(|v| v.node_version.map(|v| v.into()));

    let udf_server_version = udf_config.map(|config| format!("{}", config.server_version));
    Ok(Json(GetConfigHashesResponse {
        config,
        module_hashes,
        udf_server_version,
        node_version,
    }))
}

#[debug_handler]
pub async fn push_config(
    State(st): State<LocalAppState>,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(req): Json<ConfigJson>,
) -> Result<impl IntoResponse, HttpResponseError> {
    push_config_handler(&st.application, request_metadata, req)
        .await
        .map_err(|e| e.wrap_error_message(|msg| format!("Hit an error while pushing:\n{msg}")))?;

    Ok(Json(EmptyResponse {}))
}

#[fastrace::trace]
pub async fn push_config_handler(
    application: &Application<ProdRuntime>,
    request_metadata: RequestMetadata,
    config: ConfigJson,
) -> anyhow::Result<(Identity, PushAnalytics, PushMetrics)> {
    let identity = application
        .app_auth()
        .check_key(config.admin_key)
        .await
        .context("bad admin key error")?;

    identity.require_operation(keybroker::DeploymentOp::Deploy)?;

    let modules: Vec<ModuleConfig> = config
        .modules
        .into_iter()
        .map(|m| m.try_into())
        .collect::<anyhow::Result<Vec<_>>>()?;

    let udf_server_version = Version::parse(&config.udf_server_version).context(
        ErrorMetadata::bad_request("InvalidVersion", "The function version is invalid"),
    )?;
    let node_version: Option<NodeVersion> = config
        .node_version
        .clone()
        .map(|v| v.parse())
        .transpose()
        .context(ErrorMetadata::bad_request(
            "InvalidNodeVersion",
            format!(
                "The node version `{}` is invalid",
                config.node_version.unwrap_or_default()
            ),
        ))?;
    if let Some(node_version) = node_version {
        node_version.check_supported_for_push()?;
    }

    let (analytics, metrics) = application
        .push_config_no_components(
            identity.clone(),
            request_metadata,
            config.config,
            modules,
            udf_server_version,
            config.schema_id,
            config.node_dependencies,
            node_version,
        )
        .await?;
    Ok((identity, analytics, metrics))
}
