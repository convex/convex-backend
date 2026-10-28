use std::{
    collections::BTreeMap,
    time::Duration,
};

use application::deploy_config::{
    ComponentSchemaPrediction,
    EvaluatePushResponse,
    EvaluateSchemaPredictionResponse,
    FinishPushDiff,
    IndexChangePrediction,
    IndexPrediction,
    SchemaStatusJson,
    StagedValidatorState,
    StartPushRequest,
    StartPushResponse,
    TablePrediction,
};
use axum::{
    debug_handler,
    extract::State,
    response::IntoResponse,
};
use common::{
    auth::{
        AuthInfo,
        SerializedAuthInfo,
    },
    bootstrap_model::components::definition::SerializedComponentDefinitionMetadata,
    execution_context::RequestMetadata,
    http::{
        extract::{
            Json,
            MtState,
        },
        ExtractRequestMetadata,
        HttpResponseError,
    },
    schemas::TableValidationOutcome,
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use model::{
    auth::types::AuthDiff,
    components::{
        config::{
            SerializedComponentDefinitionDiff,
            SerializedComponentDiff,
            SerializedSchemaChange,
        },
        type_checking::SerializedCheckedComponent,
        types::SerializedEvaluatedComponentDefinition,
    },
    deployment_audit_log::{
        developer_index_config::{
            SerializedDeveloperIndexConfig,
            SerializedNamedDeveloperIndexConfig,
        },
        types::PushMessage,
    },
    external_packages::types::ExternalDepsPackageId,
    modules::module_versions::SerializedAnalyzedModule,
    source_packages::types::SourcePackage,
};
use roles::RequireDeploymentOp;
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
};
use serde_json::Value as JsonValue;
use serde_with::{
    base64::{
        Base64,
        UrlSafe,
    },
    formats::Unpadded,
    serde_as,
    DeserializeAs,
};
use value::{
    ConvexObject,
    DeveloperDocumentId,
};

use crate::{
    admin::must_be_admin_from_key,
    LocalAppState,
};

impl TryFrom<StartPushResponse> for SerializedStartPushResponse {
    type Error = anyhow::Error;

    fn try_from(value: StartPushResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            environment_variables: value
                .environment_variables
                .into_iter()
                .map(|(k, v)| Ok((String::from(k), String::from(v))))
                .collect::<anyhow::Result<_>>()?,
            external_deps_id: value
                .external_deps_id
                .map(|id| String::from(DeveloperDocumentId::from(id))),
            component_definition_packages: value
                .component_definition_packages
                .into_iter()
                .map(|(k, v)| Ok((String::from(k), JsonValue::from(ConvexObject::try_from(v)?))))
                .collect::<anyhow::Result<_>>()?,
            app_auth: value
                .app_auth
                .into_iter()
                .map(SerializedAuthInfo::try_from)
                .collect::<anyhow::Result<_>>()?,
            analysis: value
                .analysis
                .into_iter()
                .map(|(k, v)| Ok((String::from(k), v.try_into()?)))
                .collect::<anyhow::Result<_>>()?,
            app: value.app.try_into()?,
            schema_change: value.schema_change.try_into()?,
        })
    }
}

impl TryFrom<SerializedStartPushResponse> for StartPushResponse {
    type Error = anyhow::Error;

    fn try_from(value: SerializedStartPushResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            environment_variables: value
                .environment_variables
                .into_iter()
                .map(|(k, v)| Ok((k.parse()?, v.parse()?)))
                .collect::<anyhow::Result<_>>()?,
            external_deps_id: value
                .external_deps_id
                .map(|id| {
                    anyhow::Ok(ExternalDepsPackageId::from(
                        id.parse::<DeveloperDocumentId>()?,
                    ))
                })
                .transpose()?,
            component_definition_packages: value
                .component_definition_packages
                .into_iter()
                .map(|(k, v)| {
                    Ok((
                        k.parse()?,
                        SourcePackage::try_from(ConvexObject::try_from(v)?)?,
                    ))
                })
                .collect::<anyhow::Result<_>>()?,
            app_auth: value
                .app_auth
                .into_iter()
                .map(AuthInfo::try_from)
                .collect::<anyhow::Result<_>>()?,
            analysis: value
                .analysis
                .into_iter()
                .map(|(k, v)| Ok((k.parse()?, v.try_into()?)))
                .collect::<anyhow::Result<_>>()?,
            app: value.app.try_into()?,
            schema_change: value.schema_change.try_into()?,
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedStartPushResponse {
    environment_variables: BTreeMap<String, String>,

    // Pointers to uploaded code.
    external_deps_id: Option<String>,
    component_definition_packages: BTreeMap<String, JsonValue>,

    // Analysis results.
    app_auth: Vec<SerializedAuthInfo>,
    analysis: BTreeMap<String, SerializedEvaluatedComponentDefinition>,

    // Typechecking results.
    app: SerializedCheckedComponent,

    // Schema changes.
    schema_change: SerializedSchemaChange,
}

impl TryFrom<EvaluatePushResponse> for SerializedEvaluatePushResponse {
    type Error = anyhow::Error;

    fn try_from(value: EvaluatePushResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            schema_change: value.schema_change.try_into()?,
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedEvaluatePushResponse {
    schema_change: SerializedSchemaChange,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedEvaluateSchemaResponse {
    /// Keyed by component path; "" is the root component.
    component_schema_evaluations: BTreeMap<String, SerializedComponentSchemaPrediction>,
    new_component_definitions: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedComponentSchemaPrediction {
    definition_path: String,
    schema_validation: bool,
    tables: Vec<SerializedTablePrediction>,
    indexes: Vec<SerializedIndexPrediction>,
    discarded_staged_validators: Vec<SerializedDiscardedStagedValidator>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedDiscardedStagedValidator {
    table_name: String,
    state: StagedValidatorState,
    replaced: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedTablePrediction {
    name: String,
    outcome: TableValidationOutcome,
    num_docs: u64,
    size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    staged: Option<StagedValidatorState>,
    can_skip_after_staged_validation: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedIndexPrediction {
    #[serde(flatten)]
    index: SerializedNamedDeveloperIndexConfig,
    change: IndexChangePrediction,
    needs_backfill: bool,
    num_docs: u64,
}

impl TryFrom<EvaluateSchemaPredictionResponse> for SerializedEvaluateSchemaResponse {
    type Error = anyhow::Error;

    fn try_from(value: EvaluateSchemaPredictionResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            component_schema_evaluations: value
                .component_schema_evaluations
                .into_iter()
                .map(|(path, prediction)| Ok((String::from(path), prediction.try_into()?)))
                .collect::<anyhow::Result<_>>()?,
            new_component_definitions: value
                .new_component_definitions
                .into_iter()
                .map(String::from)
                .collect(),
        })
    }
}

impl TryFrom<ComponentSchemaPrediction> for SerializedComponentSchemaPrediction {
    type Error = anyhow::Error;

    fn try_from(value: ComponentSchemaPrediction) -> Result<Self, Self::Error> {
        Ok(Self {
            definition_path: String::from(value.definition_path),
            schema_validation: value.schema_validation,
            tables: value.tables.into_iter().map(Into::into).collect(),
            indexes: value.indexes.into_iter().map(Into::into).collect(),
            discarded_staged_validators: value
                .discarded_staged_validators
                .into_iter()
                .map(|discarded| SerializedDiscardedStagedValidator {
                    table_name: discarded.table_name.to_string(),
                    state: discarded.state,
                    replaced: discarded.replaced,
                })
                .collect(),
        })
    }
}

impl From<TablePrediction> for SerializedTablePrediction {
    fn from(value: TablePrediction) -> Self {
        Self {
            name: String::from(value.name),
            outcome: value.outcome,
            num_docs: value.num_docs,
            size_bytes: value.size_bytes,
            staged: value.staged,
            can_skip_after_staged_validation: value.can_skip_after_staged_validation,
        }
    }
}

impl From<IndexPrediction> for SerializedIndexPrediction {
    fn from(value: IndexPrediction) -> Self {
        Self {
            index: SerializedNamedDeveloperIndexConfig {
                name: value.name.to_string(),
                index_config: SerializedDeveloperIndexConfig::from(value.config),
            },
            change: value.change,
            needs_backfill: value.needs_backfill,
            num_docs: value.num_docs,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedComponent {
    definition: SerializedComponentDefinitionMetadata,
    schema: Option<JsonValue>,
    modules: BTreeMap<String, SerializedAnalyzedModule>,
}

#[debug_handler]
pub async fn start_push(
    State(st): State<LocalAppState>,
    Json(req): Json<StartPushRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let _identity = must_be_admin_from_key(
        st.application.app_auth(),
        st.instance_name.clone(),
        req.admin_key.clone(),
    )
    .await?;
    _identity.require_operation(keybroker::DeploymentOp::Deploy)?;
    let config = req.into_project_config().map_err(|e| {
        anyhow::Error::new(ErrorMetadata::bad_request("InvalidConfig", e.to_string()))
    })?;
    let result =
        st.application.start_push(&config).await.map_err(|e| {
            e.wrap_error_message(|msg| format!("Hit an error while pushing:\n{msg}"))
        })?;
    Ok(Json(SerializedStartPushResponse::try_from(
        result.response,
    )?))
}

// This endpoint is similar to `start_push`, but it doesn’t save the schema (so
// it won’t start schema validation/index backfill). It can be used to determine
// what will be the effects of a large push without starting work that can take
// a long time on large instances.
pub async fn evaluate_push(
    MtState(st): MtState<LocalAppState>,
    Json(req): Json<StartPushRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let _identity = must_be_admin_from_key(
        st.application.app_auth(),
        st.instance_name.clone(),
        req.admin_key.clone(),
    )
    .await?;
    _identity.require_operation(keybroker::DeploymentOp::Deploy)?;
    let config = req.into_project_config().map_err(|e| {
        anyhow::Error::new(ErrorMetadata::bad_request("InvalidConfig", e.to_string()))
    })?;
    let resp =
        st.application.evaluate_push(&config).await.map_err(|e| {
            e.wrap_error_message(|msg| format!("Hit an error while pushing:\n{msg}"))
        })?;

    Ok(Json(SerializedEvaluatePushResponse::try_from(resp)?))
}

// Predicts, without side effects, the schema validation and index backfill
// work that pushing this config would trigger: which tables must be walked
// (and why the others can skip), with document counts and sizes, and which
// indexes need backfill. Unlike `evaluate_push`, only the schema bundles are
// evaluated; modules are neither analyzed nor uploaded.
pub async fn evaluate_schema(
    MtState(st): MtState<LocalAppState>,
    Json(req): Json<StartPushRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let identity = must_be_admin_from_key(
        st.application.app_auth(),
        st.instance_name.clone(),
        req.admin_key.clone(),
    )
    .await?;
    identity.require_operation(keybroker::DeploymentOp::Deploy)?;
    let config = req.into_project_config().map_err(|e| {
        anyhow::Error::new(ErrorMetadata::bad_request("InvalidConfig", e.to_string()))
    })?;
    let resp = st.application.evaluate_schema_prediction(&config).await?;
    Ok(Json(SerializedEvaluateSchemaResponse::try_from(resp)?))
}

const DEFAULT_SCHEMA_TIMEOUT_MS: u32 = 10_000;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitForSchemaRequest {
    admin_key: String,
    schema_change: SerializedSchemaChange,
    timeout_ms: Option<u32>,
}

pub async fn wait_for_schema(
    MtState(st): MtState<LocalAppState>,
    Json(req): Json<WaitForSchemaRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let identity = must_be_admin_from_key(
        st.application.app_auth(),
        st.instance_name.clone(),
        req.admin_key,
    )
    .await?;
    identity.require_operation(keybroker::DeploymentOp::Deploy)?;
    let timeout = Duration::from_millis(req.timeout_ms.unwrap_or(DEFAULT_SCHEMA_TIMEOUT_MS) as u64);
    let schema_change = req.schema_change.try_into()?;

    // In dry_run mode, we commit the schema changes in start_push so we can
    // validate the schema against existing data.
    let resp = st
        .application
        .wait_for_schema(identity, schema_change, timeout)
        .await?;
    Ok(Json(SchemaStatusJson::from(resp)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishPushRequest {
    pub admin_key: String,
    start_push: SerializedStartPushResponse,
    pub dry_run: bool,
    pub message: Option<String>,
}

/// Internal version that returns the commit timestamp for use by conductor
pub async fn finish_push_internal(
    st: &LocalAppState,
    request_metadata: RequestMetadata,
    req: FinishPushRequest,
) -> anyhow::Result<(SerializedFinishPushDiff, Option<common::types::Timestamp>)> {
    let identity = must_be_admin_from_key(
        st.application.app_auth(),
        st.instance_name.clone(),
        req.admin_key.clone(),
    )
    .await?;
    identity.require_operation(keybroker::DeploymentOp::Deploy)?;

    let start_push = StartPushResponse::try_from(req.start_push)?;
    let message = req.message.map(PushMessage::try_from).transpose()?;

    // We can't actually run `finish_push` in a dry run, since we rolled back all of
    // our changes during start push.
    if req.dry_run {
        tracing::info!("Skipping finish_push in dry run");
        let empty_diff = FinishPushDiff::default();
        return Ok((SerializedFinishPushDiff::try_from(empty_diff)?, None));
    }

    let (resp, ts) = st
        .application
        .finish_push(identity, request_metadata, start_push, message)
        .await
        .map_err(|e| e.wrap_error_message(|msg| format!("Hit an error while pushing:\n{msg}")))?;
    Ok((SerializedFinishPushDiff::try_from(resp)?, Some(ts)))
}

pub async fn finish_push(
    MtState(st): MtState<LocalAppState>,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(req): Json<FinishPushRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let (diff, _ts) = finish_push_internal(&st, request_metadata, req).await?;
    Ok(Json(diff))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportPushCompletedRequest {
    admin_key: String,
    spans: Vec<SerializedCompletedSpan>,
}

pub async fn report_push_completed(
    st: LocalAppState,
    req: ReportPushCompletedRequest,
) -> anyhow::Result<Vec<SerializedCompletedSpan>> {
    let identity = must_be_admin_from_key(
        st.application.app_auth(),
        st.instance_name.clone(),
        req.admin_key.clone(),
    )
    .await?;
    identity.require_operation(keybroker::DeploymentOp::Deploy)?;
    Ok(req.spans)
}

#[debug_handler]
pub async fn report_push_completed_handler(
    State(st): State<LocalAppState>,
    Json(req): Json<ReportPushCompletedRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let spans = report_push_completed(st, req).await?;
    tracing::debug!("Received spans: {:?}", spans);
    Ok(Json(()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedFinishPushDiff {
    auth_diff: AuthDiff,
    definition_diffs: BTreeMap<String, SerializedComponentDefinitionDiff>,
    component_diffs: BTreeMap<String, SerializedComponentDiff>,
}

impl TryFrom<FinishPushDiff> for SerializedFinishPushDiff {
    type Error = anyhow::Error;

    fn try_from(value: FinishPushDiff) -> Result<Self, Self::Error> {
        Ok(Self {
            auth_diff: value.auth_diff,
            definition_diffs: value
                .definition_diffs
                .into_iter()
                .map(|(k, v)| Ok((String::from(k), v.try_into()?)))
                .collect::<anyhow::Result<_>>()?,
            component_diffs: value
                .component_diffs
                .into_iter()
                .map(|(k, v)| Ok((String::from(k), v.try_into()?)))
                .collect::<anyhow::Result<_>>()?,
        })
    }
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedCompletedSpan {
    #[serde_as(as = "LittleEndian<Base64<UrlSafe, Unpadded>>")]
    pub trace_id: u128,
    #[serde_as(as = "LittleEndian<Base64<UrlSafe, Unpadded>>")]
    pub parent_id: u64,
    #[serde_as(as = "LittleEndian<Base64<UrlSafe, Unpadded>>")]
    pub span_id: u64,
    #[serde_as(as = "LittleEndian<Base64<UrlSafe, Unpadded>>")]
    pub begin_time_unix_ns: u64,
    #[serde_as(as = "LittleEndian<Base64<UrlSafe, Unpadded>>")]
    pub duration_ns: u64,
    pub name: String,
    pub properties: BTreeMap<String, String>,
    pub events: Vec<SerializedEventRecord>,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedEventRecord {
    pub name: String,
    #[serde_as(as = "LittleEndian<Base64<UrlSafe, Unpadded>>")]
    pub timestamp_unix_ns: u64,
    pub properties: BTreeMap<String, String>,
}

struct LittleEndian<A>(std::marker::PhantomData<A>);

impl<'de, A: DeserializeAs<'de, [u8; 16]>> DeserializeAs<'de, u128> for LittleEndian<A> {
    fn deserialize_as<D: Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
        A::deserialize_as(deserializer).map(u128::from_le_bytes)
    }
}

impl<'de, A: DeserializeAs<'de, [u8; 8]>> DeserializeAs<'de, u64> for LittleEndian<A> {
    fn deserialize_as<D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        A::deserialize_as(deserializer).map(u64::from_le_bytes)
    }
}
