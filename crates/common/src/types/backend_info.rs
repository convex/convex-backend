use serde::{
    Deserialize,
    Serialize,
};

use crate::types::{
    DeploymentId,
    DeploymentType,
    ProjectId,
    TeamId,
    UdfRuntime,
};

pub const DEFAULT_PROVISION_CONCURRENCY: i32 = 0;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub team_id: TeamId,
    pub project_id: ProjectId,
    pub project_name: Option<String>,
    pub project_slug: Option<String>,

    pub deployment_id: DeploymentId,
    pub deployment_type: DeploymentType,
    pub deployment_ref: Option<String>,

    // None inherits the executor's code default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_udf_runtime: Option<UdfRuntime>,

    // If None, the value is overwritten with the default value.
    pub streaming_export_enabled: Option<bool>,
    pub provision_concurrency: Option<i32>,
    pub log_streaming_enabled: Option<bool>,
    pub audit_log_retention_days: Option<i64>,
    pub send_logs_to_client: Option<bool>,
    pub custom_audit_logs_in_log_streams_config_enabled: Option<bool>,
    pub ai_gateway_disabled: Option<bool>,
}

impl BackendInfo {
    /// Returns the name of the first field that differs between `self` and
    /// `other`, or `None` if they are identical.
    pub fn first_mismatch(&self, other: &BackendInfo) -> Option<&'static str> {
        let BackendInfo {
            team_id,
            project_id,
            project_name,
            project_slug,
            deployment_id,
            deployment_type,
            deployment_ref,
            default_udf_runtime,
            streaming_export_enabled,
            provision_concurrency,
            log_streaming_enabled,
            audit_log_retention_days,
            send_logs_to_client,
            custom_audit_logs_in_log_streams_config_enabled,
            ai_gateway_disabled,
        } = other;
        if self.team_id != *team_id {
            return Some("backend_info.team_id");
        }
        if self.project_id != *project_id {
            return Some("backend_info.project_id");
        }
        if self.project_name != *project_name {
            return Some("backend_info.project_name");
        }
        if self.project_slug != *project_slug {
            return Some("backend_info.project_slug");
        }
        if self.deployment_id != *deployment_id {
            return Some("backend_info.deployment_id");
        }
        if self.deployment_type != *deployment_type {
            return Some("backend_info.deployment_type");
        }
        // TODO: Enable deployment_ref verification once all conductors
        // understand this field.
        let _ = deployment_ref;
        // Verification also runs against Conductors predating runtime preferences.
        let _ = default_udf_runtime;

        if self.streaming_export_enabled != *streaming_export_enabled {
            return Some("backend_info.streaming_export_enabled");
        }
        if self.provision_concurrency != *provision_concurrency {
            return Some("backend_info.provision_concurrency");
        }
        if self.log_streaming_enabled != *log_streaming_enabled {
            return Some("backend_info.log_streaming_enabled");
        }
        if self.audit_log_retention_days != *audit_log_retention_days {
            return Some("backend_info.audit_log_retention_days");
        }
        if self.send_logs_to_client != *send_logs_to_client {
            return Some("backend_info.send_logs_to_client");
        }
        // TODO(reece): enable when all conductors have this field
        let _ = custom_audit_logs_in_log_streams_config_enabled;
        // TODO(andy): enable once all conductors have this field.
        let _ = ai_gateway_disabled;
        None
    }
}
