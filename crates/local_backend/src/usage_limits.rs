use std::collections::BTreeMap;

use ::usage_limits::{
    SeedStatus,
    UsageMeter,
};
use axum::{
    extract::FromRef,
    response::IntoResponse,
};
use common::{
    execution_context::RequestMetadata,
    http::{
        extract::{
            Json,
            MtState,
            Path,
        },
        ExtractRequestMetadata,
        HttpResponseError,
    },
    runtime::Runtime,
};
use errors::ErrorMetadata;
use http::StatusCode;
use model::{
    deployment_audit_log::types::DeploymentAuditLogEvent,
    usage_limits::{
        types::{
            MetricUnit,
            UsageLimitConfig,
            UsageLimitMetric,
            UsageLimitType,
            UsageLimitWindow,
        },
        UsageLimitsModel,
        USAGE_LIMITS_TABLE,
    },
};
use roles::RequireDeploymentOp;
use serde::{
    Deserialize,
    Serialize,
};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use value::{
    id_v6::DeveloperDocumentId,
    TableNamespace,
};

use crate::{
    authentication::ExtractIdentity,
    parse::parse_document_id,
    LocalAppState,
};

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimitConfigRequest {
    metric: UsageLimitMetric,
    window: UsageLimitWindow,
    limit_type: UsageLimitType,
    #[schema(minimum = 1)]
    limit: u64,
    enabled: bool,
}

impl UsageLimitConfigRequest {
    fn into_usage_limit_config(self) -> anyhow::Result<UsageLimitConfig> {
        let config = UsageLimitConfig {
            metric: self.metric,
            window: self.window,
            limit_type: self.limit_type,
            limit: self.limit,
            enabled: self.enabled,
        };
        config.validate()?;
        Ok(config)
    }
}

#[derive(Serialize, Deserialize, ToSchema, PartialEq, Eq, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimitConfigResponse {
    pub id: String,
    pub metric: UsageLimitMetric,
    pub window: UsageLimitWindow,
    pub limit_type: UsageLimitType,
    #[schema(minimum = 1)]
    pub limit: u64,
    pub enabled: bool,
}

impl From<common::document::ParsedDocument<UsageLimitConfig>> for UsageLimitConfigResponse {
    fn from(doc: common::document::ParsedDocument<UsageLimitConfig>) -> Self {
        let id = String::from(DeveloperDocumentId::from(doc.id()));
        let config = doc.into_value();
        Self {
            id,
            metric: config.metric,
            window: config.window,
            limit_type: config.limit_type,
            limit: config.limit,
            enabled: config.enabled,
        }
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListUsageLimitsResponse {
    usage_limits: Vec<UsageLimitConfigResponse>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimitResponse {
    pub usage_limit: UsageLimitConfigResponse,
}

/// Current-window usage for a single metric.
#[derive(Serialize, Deserialize, ToSchema, PartialEq, Debug, Clone)]
pub struct MetricUsageResponse {
    /// The unit `usage` is expressed in, matching the unit this metric's
    /// configured limits use.
    unit: MetricUnit,
    usage: WindowUsageResponse,
}

/// Usage in each calendar-aligned window currently in progress.
#[derive(Serialize, Deserialize, ToSchema, PartialEq, Debug, Clone)]
pub struct WindowUsageResponse {
    current_day: f64,
    current_month: f64,
}

/// Progress of the historical-usage backfill. Only `complete` guarantees the
/// reported usage reflects the full window. While the status is `pending` or
/// `partial`, history from before this deployment was loaded may not be
/// hydrated yet and the numbers can understate actual usage, so retry later for
/// an accurate total. `failed` means the backfill couldn't hydrate any history.
#[derive(Serialize, Deserialize, ToSchema, PartialEq, Eq, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum SeedStatusResponse {
    Pending,
    Partial,
    Complete,
    Failed,
}

impl From<SeedStatus> for SeedStatusResponse {
    fn from(status: SeedStatus) -> Self {
        match status {
            SeedStatus::Pending => Self::Pending,
            SeedStatus::Partial => Self::Partial,
            SeedStatus::Complete => Self::Complete,
            SeedStatus::Failed => Self::Failed,
        }
    }
}

/// Current usage across every metric, keyed by the same metric name the usage
/// limit config API uses.
#[derive(Serialize, Deserialize, ToSchema, PartialEq, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetCurrentUsageResponse {
    metrics: BTreeMap<String, MetricUsageResponse>,
    seed_status: SeedStatusResponse,
}

/// Get current usage
///
/// Get the values for each usage metric for the current day and month (UTC)
///
/// The reported usage is only guaranteed to reflect the full window when
/// `seedStatus` is `complete`. A `pending` or `partial` status means the
/// backfill is still in progress and the returned usage may understate actual
/// usage, so retry later for an accurate total.
#[utoipa::path(
    get,
    path = "/get_current_usage",
    tag = "Usage Limits",
    tags = ["beta"],
    responses(
        (status = 200, body = GetCurrentUsageResponse)
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn get_current_usage(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ViewUsage)?;

    let meter = st.application.usage_meter();
    let now = st.application.runtime().system_time();
    let metrics = meter
        .usage_snapshot(now)?
        .into_iter()
        .map(|(metric, usage)| {
            (
                metric.to_string(),
                MetricUsageResponse {
                    unit: metric.unit(),
                    usage: WindowUsageResponse {
                        current_day: metric.usage_in_display_units(usage.current_day),
                        current_month: metric.usage_in_display_units(usage.current_month),
                    },
                },
            )
        })
        .collect();

    Ok(Json(GetCurrentUsageResponse {
        metrics,
        seed_status: meter.seed_status().into(),
    }))
}

/// List usage limits
///
/// Get all usage limit configs for a deployment.
#[utoipa::path(
    get,
    path = "/list_usage_limits",
    tag = "Usage Limits",
    responses(
        (status = 200, body = ListUsageLimitsResponse)
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn list_usage_limits(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ViewUsageLimits)?;

    let mut tx = st.application.begin(identity).await?;
    let usage_limits = UsageLimitsModel::new(&mut tx)
        .list()
        .await?
        .into_iter()
        .map(UsageLimitConfigResponse::from)
        .collect();

    Ok(Json(ListUsageLimitsResponse { usage_limits }))
}

/// Create usage limit
///
/// Create a new usage limit config for a deployment.
#[utoipa::path(
    post,
    path = "/create_usage_limit",
    tag = "Usage Limits",
    request_body = UsageLimitConfigRequest,
    responses((status = 200, body = UsageLimitResponse)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn create_usage_limit(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(req): Json<UsageLimitConfigRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let usage_limit = create_usage_limit_handler(st, identity, request_metadata, req).await?;
    Ok(Json(UsageLimitResponse { usage_limit }))
}

pub async fn create_usage_limit_handler(
    st: LocalAppState,
    identity: keybroker::Identity,
    request_metadata: RequestMetadata,
    req: UsageLimitConfigRequest,
) -> Result<UsageLimitConfigResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteUsageLimits)?;

    let mut tx = st.application.begin(identity).await?;
    let config = req.into_usage_limit_config()?;
    validate_limit_above_current_usage(&st, &config)?;
    let id = UsageLimitsModel::new(&mut tx).create(config).await?;
    let created = UsageLimitsModel::new(&mut tx)
        .get(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!(usage_limit_not_found()))?;
    let audit_events = vec![DeploymentAuditLogEvent::CreateUsageLimit {
        id: String::from(DeveloperDocumentId::from(id)),
        config: created.clone().into_value(),
    }];
    let usage_limit = created.into();

    st.application
        .commit_with_audit_log_events(tx, audit_events, request_metadata, "create_usage_limit")
        .await?;

    Ok(usage_limit)
}

/// Update usage limit
///
/// Replace an existing usage limit config for a deployment.
#[utoipa::path(
    post,
    path = "/update_usage_limit/{id}",
    tag = "Usage Limits",
    params(
        ("id" = String, Path, description = "id of the usage limit to update"),
    ),
    request_body = UsageLimitConfigRequest,
    responses((status = 200, body = UsageLimitResponse)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn update_usage_limit(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(id): Path<String>,
    Json(req): Json<UsageLimitConfigRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let usage_limit = update_usage_limit_handler(st, identity, request_metadata, id, req).await?;
    Ok(Json(UsageLimitResponse { usage_limit }))
}

pub async fn update_usage_limit_handler(
    st: LocalAppState,
    identity: keybroker::Identity,
    request_metadata: RequestMetadata,
    id: String,
    req: UsageLimitConfigRequest,
) -> Result<UsageLimitConfigResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteUsageLimits)?;

    let mut tx = st.application.begin(identity).await?;
    let id = parse_document_id(
        &id,
        &tx.table_mapping().namespace(TableNamespace::Global),
        &USAGE_LIMITS_TABLE,
    )?;
    let previous = UsageLimitsModel::new(&mut tx)
        .get(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!(usage_limit_not_found()))?
        .into_value();
    let config = req.into_usage_limit_config()?;
    validate_limit_above_current_usage(&st, &config)?;
    UsageLimitsModel::new(&mut tx).replace(id, config).await?;
    let updated = UsageLimitsModel::new(&mut tx)
        .get(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!(usage_limit_not_found()))?;
    let audit_events: Vec<DeploymentAuditLogEvent> =
        vec![DeploymentAuditLogEvent::UpdateUsageLimit {
            id: String::from(DeveloperDocumentId::from(id)),
            previous,
            current: updated.clone().into_value(),
        }];
    let usage_limit = updated.into();

    st.application
        .commit_with_audit_log_events(tx, audit_events, request_metadata, "update_usage_limit")
        .await?;

    Ok(usage_limit)
}

/// Delete usage limit
///
/// Delete an existing usage limit config for a deployment.
#[utoipa::path(
    post,
    path = "/delete_usage_limit/{id}",
    tag = "Usage Limits",
    params(
        ("id" = String, Path, description = "id of the usage limit to delete"),
    ),
    responses((status = 200)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn delete_usage_limit(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpResponseError> {
    delete_usage_limit_handler(st, identity, request_metadata, id).await?;
    Ok(StatusCode::OK)
}

pub async fn delete_usage_limit_handler(
    st: LocalAppState,
    identity: keybroker::Identity,
    request_metadata: RequestMetadata,
    id: String,
) -> Result<UsageLimitConfigResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteUsageLimits)?;

    let mut tx = st.application.begin(identity).await?;
    let id = parse_document_id(
        &id,
        &tx.table_mapping().namespace(TableNamespace::Global),
        &USAGE_LIMITS_TABLE,
    )?;
    let Some(config) = UsageLimitsModel::new(&mut tx).delete(id).await? else {
        return Err(anyhow::anyhow!(usage_limit_not_found()).into());
    };
    let audit_events = vec![DeploymentAuditLogEvent::DeleteUsageLimit {
        id: String::from(DeveloperDocumentId::from(id)),
        config: config.clone(),
    }];

    st.application
        .commit_with_audit_log_events(tx, audit_events, request_metadata, "delete_usage_limit")
        .await?;

    Ok(UsageLimitConfigResponse {
        id: String::from(DeveloperDocumentId::from(id)),
        metric: config.metric,
        window: config.window,
        limit_type: config.limit_type,
        limit: config.limit,
        enabled: config.enabled,
    })
}

fn usage_limit_not_found() -> ErrorMetadata {
    ErrorMetadata::not_found("UsageLimitNotFound", "The usage limit couldn't be found.")
}

/// Reject an enabled limit set below the usage already accrued in its current
/// window. Such a limit trips the instant it's saved (enforcement is
/// `total >= limit`), warning or disabling the deployment immediately, which
/// is almost never intended. Disabled limits enforce nothing, so they're
/// allowed regardless.
fn validate_limit_above_current_usage(
    st: &LocalAppState,
    config: &UsageLimitConfig,
) -> anyhow::Result<()> {
    if !config.enabled {
        return Ok(());
    }
    let meter: &UsageMeter = st.application.usage_meter();
    let now = st.application.runtime().system_time();
    let current = meter
        .usage_snapshot(now)?
        .into_iter()
        .find(|(metric, _)| *metric == config.metric)
        .map(|(_, usage)| match config.window {
            UsageLimitWindow::Day => usage.current_day,
            UsageLimitWindow::Month => usage.current_month,
        })
        .unwrap_or(0.0);
    if config.metric.limit_in_raw_units(config.limit) < current {
        return Err(ErrorMetadata::bad_request(
            "UsageLimitBelowCurrentUsage",
            format!(
                "Usage limit of {limit} is below the current {window} usage of {current_usage} \
                 for {metric}. Set the limit at or above the current usage.",
                limit = config.limit,
                window = config.window,
                current_usage = config.metric.usage_in_display_units(current),
                metric = config.metric,
            ),
        )
        .into());
    }
    Ok(())
}

pub fn platform_router<S>() -> OpenApiRouter<S>
where
    LocalAppState: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(get_current_usage))
        .routes(utoipa_axum::routes!(list_usage_limits))
        .routes(utoipa_axum::routes!(create_usage_limit))
        .routes(utoipa_axum::routes!(update_usage_limit))
        .routes(utoipa_axum::routes!(delete_usage_limit))
}
