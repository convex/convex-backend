use common::types::{
    BackendInfo,
    DeploymentId,
    DeploymentType,
    ProjectId,
    TeamId,
    UdfRuntime,
    DEFAULT_PROVISION_CONCURRENCY,
};
use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

/// Information and configuration about the backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendInfoPersisted {
    pub team: TeamId,
    pub project: ProjectId,
    pub deployment: DeploymentId,
    pub deployment_type: DeploymentType,
    pub deployment_ref: Option<String>,
    pub project_name: Option<String>,
    pub project_slug: Option<String>,
    pub default_udf_runtime: Option<UdfRuntime>,

    // Entitlements
    pub streaming_export_enabled: bool,
    pub provision_concurrency: i32,
    pub log_streaming_enabled: bool,
    pub audit_log_retention_days: i64,
    pub send_logs_to_client: Option<bool>,
    pub custom_audit_logs_in_log_streams_config_enabled: Option<bool>,
    pub ai_gateway_disabled: Option<bool>,
}

impl From<BackendInfoPersisted> for BackendInfo {
    fn from(bi: BackendInfoPersisted) -> BackendInfo {
        Self {
            team_id: bi.team,
            project_id: bi.project,
            deployment_id: bi.deployment,
            deployment_type: bi.deployment_type,
            deployment_ref: bi.deployment_ref,
            project_name: bi.project_name,
            project_slug: bi.project_slug,
            default_udf_runtime: bi.default_udf_runtime,
            streaming_export_enabled: Some(bi.streaming_export_enabled),
            provision_concurrency: Some(bi.provision_concurrency),
            log_streaming_enabled: Some(bi.log_streaming_enabled),
            audit_log_retention_days: Some(bi.audit_log_retention_days),
            send_logs_to_client: bi.send_logs_to_client,
            custom_audit_logs_in_log_streams_config_enabled: bi
                .custom_audit_logs_in_log_streams_config_enabled,
            ai_gateway_disabled: bi.ai_gateway_disabled,
        }
    }
}

impl From<BackendInfo> for BackendInfoPersisted {
    fn from(bi: BackendInfo) -> BackendInfoPersisted {
        Self {
            team: bi.team_id,
            project: bi.project_id,
            deployment: bi.deployment_id,
            project_name: bi.project_name,
            project_slug: bi.project_slug,
            default_udf_runtime: bi.default_udf_runtime,
            deployment_ref: bi.deployment_ref,
            streaming_export_enabled: bi.streaming_export_enabled.unwrap_or_default(),
            deployment_type: bi.deployment_type,
            provision_concurrency: bi
                .provision_concurrency
                .unwrap_or(DEFAULT_PROVISION_CONCURRENCY),
            log_streaming_enabled: bi.log_streaming_enabled.unwrap_or_default(),
            audit_log_retention_days: bi.audit_log_retention_days.unwrap_or_default(),
            send_logs_to_client: bi.send_logs_to_client,
            custom_audit_logs_in_log_streams_config_enabled: bi
                .custom_audit_logs_in_log_streams_config_enabled,
            ai_gateway_disabled: bi.ai_gateway_disabled,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedBackendInfo {
    org: i64,
    project: i64,
    instance: i64,
    deployment_type: String,
    #[serde(default)]
    deployment_ref: Option<String>,
    #[serde(default)]
    streaming_export_enabled: bool,
    #[serde(default)]
    provision_concurrency: Option<i64>,
    #[serde(default)]
    log_streaming_enabled: bool,
    project_name: Option<String>,
    project_slug: Option<String>,
    #[serde(default)]
    audit_log_retention_days: i64,
    #[serde(default)]
    send_logs_to_client: Option<bool>,
    #[serde(default)]
    custom_audit_logs_in_log_streams_config_enabled: Option<bool>,
    #[serde(default)]
    ai_gateway_disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_udf_runtime: Option<String>,
}

impl From<BackendInfoPersisted> for SerializedBackendInfo {
    fn from(b: BackendInfoPersisted) -> Self {
        let team: u64 = b.team.into();
        let project: u64 = b.project.into();
        let deployment: u64 = b.deployment.into();
        let deployment_type: String = b.deployment_type.to_string();

        SerializedBackendInfo {
            org: (team as i64),
            project: (project as i64),
            instance: (deployment as i64),
            deployment_type,
            deployment_ref: b.deployment_ref,
            streaming_export_enabled: b.streaming_export_enabled,
            provision_concurrency: Some(b.provision_concurrency as i64),
            log_streaming_enabled: b.log_streaming_enabled,
            project_name: b.project_name,
            project_slug: b.project_slug,
            audit_log_retention_days: b.audit_log_retention_days,
            send_logs_to_client: b.send_logs_to_client,
            custom_audit_logs_in_log_streams_config_enabled: b
                .custom_audit_logs_in_log_streams_config_enabled,
            ai_gateway_disabled: b.ai_gateway_disabled,
            default_udf_runtime: b.default_udf_runtime.map(|runtime| runtime.to_string()),
        }
    }
}

impl TryFrom<SerializedBackendInfo> for BackendInfoPersisted {
    type Error = anyhow::Error;

    fn try_from(o: SerializedBackendInfo) -> Result<Self, Self::Error> {
        let team = TeamId(o.org as u64);
        let project = ProjectId(o.project as u64);
        let deployment = DeploymentId(o.instance as u64);
        let deployment_type: DeploymentType = o.deployment_type.parse()?;
        let deployment_ref = o.deployment_ref;
        let streaming_export_enabled = o.streaming_export_enabled;
        let provision_concurrency = o
            .provision_concurrency
            .map_or(DEFAULT_PROVISION_CONCURRENCY, |c| c as i32);
        let log_streaming_enabled = o.log_streaming_enabled;
        let project_name = o.project_name;
        let project_slug = o.project_slug;
        let audit_log_retention_days = o.audit_log_retention_days;
        let send_logs_to_client = o.send_logs_to_client;
        let custom_audit_logs_in_log_streams_config_enabled =
            o.custom_audit_logs_in_log_streams_config_enabled;
        let ai_gateway_disabled = o.ai_gateway_disabled;

        Ok(Self {
            team,
            project,
            deployment,
            deployment_type,
            deployment_ref,
            project_name,
            project_slug,
            streaming_export_enabled,
            provision_concurrency,
            log_streaming_enabled,
            audit_log_retention_days,
            send_logs_to_client,
            custom_audit_logs_in_log_streams_config_enabled,
            ai_gateway_disabled,
            default_udf_runtime: o.default_udf_runtime.map(|s| s.parse()).transpose()?,
        })
    }
}

codegen_convex_serialization!(BackendInfoPersisted, SerializedBackendInfo);
