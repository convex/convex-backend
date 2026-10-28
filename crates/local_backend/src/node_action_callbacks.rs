use std::{
    str::FromStr,
    time::SystemTime,
};

use anyhow::Context;
use axum::{
    extract::FromRequestParts,
    response::IntoResponse,
    RequestPartsExt,
};
use common::{
    components::{
        ComponentFunctionPath,
        ComponentId,
        PublicFunctionPath,
    },
    execution_context::{
        ClientIp,
        ClientUserAgent,
        ExecutionContext,
        ExecutionId,
        RequestMetadata,
    },
    fastrace_helpers::{
        initialize_root_from_parent,
        EncodedSpan,
    },
    http::{
        extract::{
            FromMtState,
            Json,
            MtState,
        },
        ExtractClientVersion,
        HttpResponseError,
    },
    knobs::V8_ACTION_USER_TIMEOUT,
    runtime::UnixTimestamp,
    types::{
        AttributionClaims,
        FunctionCaller,
        QueryInvocation,
        SessionId,
        SessionRequestSeqNumber,
        UdfIdentifier,
        UdfType,
        AI_GATEWAY_URL,
    },
    RequestContext,
    RequestId,
};
use errors::ErrorMetadata;
use fastrace::future::FutureExt;
use http::HeaderMap;
use keybroker::Identity;
use model::session_requests::types::SessionRequestIdentifier;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use sync_types::{
    AuthenticationToken,
    CanonicalizedUdfPath,
};
use udf::{
    helpers::UdfArgsJson,
    ActionCallbacks,
};
use usage_tracking::FunctionUsageTracker;
use value::{
    export::ValueFormat,
    id_v6::DeveloperDocumentId,
};
use vector::{
    VectorSearch,
    VectorSearchRequest,
};

use crate::{
    authentication::ExtractAuthenticationToken,
    public_api::{
        export_value,
        UdfResponse,
    },
    LocalAppState,
};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NodeCallbackUdfPostRequest {
    pub path: Option<String>,
    pub reference: Option<String>,
    pub function_handle: Option<String>,
    pub args: UdfArgsJson,

    pub format: Option<String>,

    /// Identifier used to make a mutation called from an action idempotent
    /// across client-side retries. Only sent for mutation callbacks.
    pub mutation_identifier: Option<MutationIdentifierJson>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MutationIdentifierJson {
    pub session_id: String,
    pub request_id: SessionRequestSeqNumber,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateServiceTokenResponse {
    pub token: String,
}

pub async fn create_service_token(
    MtState(st): MtState<LocalAppState>,
    ExtractActionIdentity {
        identity,
        component_id,
    }: ExtractActionIdentity,
    ExtractActionName(action_name): ExtractActionName,
    ExtractExecutionContext(context): ExtractExecutionContext,
) -> Result<impl IntoResponse, HttpResponseError> {
    let mut tx = st.application.begin(identity.clone()).await?;
    let component_path = tx.must_component_path(component_id)?;
    let attribution = match action_name {
        Some(name) => AttributionClaims {
            component_path: component_path.serialize(),
            function_name: Some(name),
            function_type: Some(UdfType::Action.to_lowercase_string().to_owned()),
        },
        None => AttributionClaims::unknown(),
    };
    let token = st
        .application
        .mint_ai_gateway_jwt(&identity, attribution, &context.request_id)
        .await?;
    Ok(Json(CreateServiceTokenResponse { token }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetServiceUrlResponse {
    pub url: String,
}

pub async fn get_service_url() -> impl IntoResponse {
    Json(GetServiceUrlResponse {
        url: AI_GATEWAY_URL.to_owned(),
    })
}

impl TryFrom<MutationIdentifierJson> for SessionRequestIdentifier {
    type Error = anyhow::Error;

    fn try_from(value: MutationIdentifierJson) -> Result<Self, Self::Error> {
        let session_id: SessionId = value.session_id.parse()?;
        Ok(SessionRequestIdentifier {
            session_id,
            request_id: value.request_id,
        })
    }
}

/// This is like `public_query_post`, except it allows calling internal
/// functions as well. This should not be used for any publicly accessible
/// endpoints, and should only be used to support Convex functions calling into
/// other Convex functions (i.e. actions calling into mutations)
#[fastrace::trace]
pub async fn internal_query_post(
    MtState(st): MtState<LocalAppState>,
    ExtractActionIdentity {
        identity,
        component_id,
    }: ExtractActionIdentity,
    ExtractClientVersion(client_version): ExtractClientVersion,
    ExtractExecutionContext(context): ExtractExecutionContext,
    Json(req): Json<NodeCallbackUdfPostRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let path = st
        .application
        .canonicalized_function_path(
            identity.clone(),
            component_id,
            req.path,
            req.reference,
            req.function_handle,
        )
        .await?;
    let udf_return = st
        .application
        .read_only_udf(
            RequestContext::new(context.request_id, context.request_metadata),
            PublicFunctionPath::Component(path),
            req.args.into_serialized_args()?,
            identity,
            FunctionCaller::Action {
                parent_scheduled_job: context.parent_scheduled_job,
                parent_execution_id: Some(context.execution_id),
            },
            QueryInvocation::Fresh,
        )
        .await?;
    if req.format.is_some() {
        return Err(anyhow::anyhow!("req.format cannot be provided to action callbacks").into());
    }
    let value_format = Some(ValueFormat::ConvexEncodedJSON);
    let response = match udf_return.result {
        Ok(value) => UdfResponse::Success {
            value: export_value(value, value_format, client_version)?,
            log_lines: udf_return.log_lines,
        },
        Err(error) => {
            UdfResponse::nested_error(error, udf_return.log_lines, value_format, client_version)?
        },
    };
    Ok(Json(response))
}

/// This is like `public_mutation_post`, except it allows calling internal
/// functions as well. This should not be used for any publicly accessible
/// endpoints, and should only be used to support Convex functions calling into
/// other Convex functions (i.e. actions calling into mutations)
#[fastrace::trace]
pub async fn internal_mutation_post(
    MtState(st): MtState<LocalAppState>,
    ExtractActionIdentity {
        identity,
        component_id,
    }: ExtractActionIdentity,
    ExtractClientVersion(client_version): ExtractClientVersion,
    ExtractExecutionContext(context): ExtractExecutionContext,
    Json(req): Json<NodeCallbackUdfPostRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let mutation_identifier = req
        .mutation_identifier
        .map(SessionRequestIdentifier::try_from)
        .transpose()?;
    let path = st
        .application
        .canonicalized_function_path(
            identity.clone(),
            component_id,
            req.path,
            req.reference,
            req.function_handle,
        )
        .await?;
    let udf_result = st
        .application
        .mutation_udf(
            RequestContext::new(context.request_id, context.request_metadata),
            PublicFunctionPath::Component(path),
            req.args.into_serialized_args()?,
            identity,
            mutation_identifier,
            FunctionCaller::Action {
                parent_scheduled_job: context.parent_scheduled_job,
                parent_execution_id: Some(context.execution_id),
            },
            None,
        )
        .await?;
    if req.format.is_some() {
        return Err(anyhow::anyhow!("req.format cannot be provided to action callbacks").into());
    }
    let value_format = Some(ValueFormat::ConvexEncodedJSON);
    let response = match udf_result {
        Ok(write_return) => UdfResponse::Success {
            value: export_value(write_return.value, value_format, client_version)?,
            log_lines: write_return.log_lines,
        },
        Err(write_error) => UdfResponse::nested_error(
            write_error.error,
            write_error.log_lines,
            value_format,
            client_version,
        )?,
    };
    Ok(Json(response))
}

/// This is like `public_action_post`, except it allows calling internal
/// functions as well. This should not be used for any publicly accessible
/// endpoints, and should only be used to support Convex functions calling into
/// other Convex functions (i.e. actions calling into actions)
#[fastrace::trace]
pub async fn internal_action_post(
    MtState(st): MtState<LocalAppState>,
    ExtractActionIdentity {
        identity,
        component_id,
    }: ExtractActionIdentity,
    ExtractClientVersion(client_version): ExtractClientVersion,
    ExtractExecutionContext(context): ExtractExecutionContext,
    Json(req): Json<NodeCallbackUdfPostRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let path = st
        .application
        .canonicalized_function_path(
            identity.clone(),
            component_id,
            req.path,
            req.reference,
            req.function_handle,
        )
        .await?;
    let udf_result = st
        .application
        .action_udf(
            RequestContext::new(context.request_id, context.request_metadata),
            PublicFunctionPath::Component(path),
            req.args.into_serialized_args()?,
            identity,
            FunctionCaller::Action {
                parent_scheduled_job: context.parent_scheduled_job,
                parent_execution_id: Some(context.execution_id),
            },
        )
        .await?;
    if req.format.is_some() {
        return Err(anyhow::anyhow!("req.format cannot be provided to action callbacks").into());
    }
    let value_format = Some(ValueFormat::ConvexEncodedJSON);
    let response = match udf_result {
        Ok(action_return) => UdfResponse::Success {
            value: export_value(action_return.value, value_format, client_version)?,
            log_lines: action_return.log_lines,
        },
        Err(action_error) => UdfResponse::nested_error(
            action_error.error,
            action_error.log_lines,
            value_format,
            client_version,
        )?,
    };
    Ok(Json(response))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleJobRequest {
    reference: Option<String>,
    function_handle: Option<String>,
    udf_path: Option<String>,
    udf_args: UdfArgsJson,
    scheduled_ts: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleJobResponse {
    job_id: String,
}

pub async fn schedule_job(
    MtState(st): MtState<LocalAppState>,
    ExtractActionIdentity {
        identity,
        component_id,
    }: ExtractActionIdentity,
    ExtractExecutionContext(context): ExtractExecutionContext,
    Json(req): Json<ScheduleJobRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let scheduled_ts = UnixTimestamp::from_secs_f64(req.scheduled_ts).with_context(|| {
        ErrorMetadata::bad_request("InvalidTimestamp", "Requested scheduled_ts is invalid")
    })?;
    // User might have entered an invalid path, so this is a developer error.
    let path = st
        .application
        .canonicalized_function_path(
            identity.clone(),
            component_id,
            req.udf_path,
            req.reference,
            req.function_handle,
        )
        .await
        .map_err(|e| {
            anyhow::anyhow!(ErrorMetadata::bad_request("InvalidUdfPath", e.to_string()))
        })?;
    let udf_args = req.udf_args.into_serialized_args()?;
    let job_id = st
        .application
        .runner()
        .schedule_job(
            identity,
            component_id,
            path,
            udf_args,
            scheduled_ts,
            context,
        )
        .await?;
    Ok(Json(ScheduleJobResponse {
        job_id: job_id.to_string(),
    }))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelDeveloperJobRequest {
    pub id: String,
}

pub async fn cancel_developer_job(
    MtState(st): MtState<LocalAppState>,
    ExtractActionIdentity {
        identity,
        component_id: _,
    }: ExtractActionIdentity,
    Json(CancelDeveloperJobRequest { id }): Json<CancelDeveloperJobRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let virtual_doc_id = DeveloperDocumentId::from_str(&id).context(ErrorMetadata::bad_request(
        "InvalidArgument",
        "Invalid scheduled function ID",
    ))?;
    st.application
        .runner()
        .cancel_job(identity, virtual_doc_id)
        .await?;
    Ok(Json(json!(null)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFunctionHandleRequest {
    udf_path: Option<String>,
    reference: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFunctionHandleResponse {
    handle: String,
}

pub async fn create_function_handle(
    MtState(st): MtState<LocalAppState>,
    ExtractActionIdentity {
        identity,
        component_id,
    }: ExtractActionIdentity,
    Json(req): Json<CreateFunctionHandleRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let path = st
        .application
        .canonicalized_function_path(
            identity.clone(),
            component_id,
            req.udf_path,
            req.reference,
            None,
        )
        .await
        .map_err(|e| {
            anyhow::anyhow!(ErrorMetadata::bad_request("InvalidUdfPath", e.to_string()))
        })?;
    let handle = st
        .application
        .runner()
        .create_function_handle(identity, path)
        .await?;
    Ok(Json(CreateFunctionHandleResponse {
        handle: String::from(handle),
    }))
}

pub async fn vector_search(
    MtState(st): MtState<LocalAppState>,
    ExtractActionIdentity {
        identity,
        component_id,
    }: ExtractActionIdentity,
    ExtractActionName(action_name): ExtractActionName,
    ExtractExecutionContext(context): ExtractExecutionContext,
    Json(req): Json<VectorSearchRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let VectorSearchRequest { query } = req;
    let query = VectorSearch::try_from(query).map_err(|e| {
        let message = e.to_string();
        e.context(ErrorMetadata::bad_request("InvalidVectorQuery", message))
    })?;
    let (results, usage_stats) = st
        .application
        .vector_search(identity.clone(), query)
        .await?;

    // This is a workaround. The correct way to track usage is to return in the
    // response, and then Node.js should aggregate it and then send it back to
    // the backend alongside the action result, which is how Funrun actions
    // work. Since we don't have that pipeline working in Node.js/TypeScript, we
    // report vector usage directly here.
    if let Some(action_name) = action_name {
        let usage = FunctionUsageTracker::new();
        usage.add(usage_stats);
        let mut tx = st.application.begin(identity).await?;
        let component = tx
            .get_component_path(component_id)
            .context(ErrorMetadata::bad_request(
                "MissingComponent",
                format!("Failed to find a component for id {component_id:?}"),
            ))?;
        let udf_path: CanonicalizedUdfPath = action_name
            .parse()
            .context(format!("Unexpected udf path format, got {action_name}"))?;
        let path = ComponentFunctionPath {
            component,
            udf_path: udf_path.clone().strip(),
        };
        st.application
            .usage_counter()
            .track_function_usage(
                UdfIdentifier::Function(path.canonicalize()),
                // TODO(CX-6045) - have the action send the ExecutionId as a request header
                context.execution_id,
                context.request_id,
                usage.gather_user_stats(),
            )
            .await;
    }

    let results: Vec<_> = results.into_iter().map(JsonValue::from).collect();
    Ok(Json(json!({ "results": results })))
}

pub async fn storage_generate_upload_url(
    MtState(st): MtState<LocalAppState>,
    ExtractActionIdentity {
        identity,
        component_id,
    }: ExtractActionIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    let url = st
        .application
        .storage_generate_upload_url(identity, component_id)
        .await?;
    Ok(Json(json!({ "url": url })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetParams {
    storage_id: String,
}

pub async fn storage_get_url(
    MtState(st): MtState<LocalAppState>,
    ExtractActionIdentity {
        identity,
        component_id,
    }: ExtractActionIdentity,
    Json(req): Json<GetParams>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let storage_id = req.storage_id.parse()?;
    let url = st
        .application
        .runner()
        .storage_get_url(identity, component_id, storage_id)
        .await?;
    Ok(Json(json!({ "url": url })))
}

pub async fn storage_get_metadata(
    MtState(st): MtState<LocalAppState>,
    ExtractActionIdentity {
        identity,
        component_id,
    }: ExtractActionIdentity,
    Json(req): Json<GetParams>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let storage_id = req.storage_id.parse()?;
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct FileMetadataJson {
        storage_id: String,
        sha256: String,
        size: i64,
        content_type: Option<String>,
    }

    let file_metadata = st
        .application
        .runner()
        .storage_get_file_entry(identity, component_id, storage_id)
        .await?
        .map(|(_, entry)| {
            // NB: `storage_key` is an internal field that we shouldn't to Node.
            FileMetadataJson {
                storage_id: entry.storage_id.to_string(),
                // TODO(CX-5533) use base64 for consistency.
                sha256: entry.sha256.as_hex(),
                size: entry.size,
                content_type: entry.content_type,
            }
        });
    Ok(Json(file_metadata))
}

pub async fn storage_delete(
    MtState(st): MtState<LocalAppState>,
    ExtractActionIdentity {
        identity,
        component_id,
    }: ExtractActionIdentity,
    Json(req): Json<GetParams>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let storage_id = req.storage_id.parse()?;
    st.application
        .runner()
        .storage_delete(identity, component_id, storage_id)
        .await?;
    Ok(Json(json!(null)))
}

#[derive(Deserialize)]
pub struct AuditLogParams {
    #[allow(dead_code)]
    body: JsonValue,
}

pub async fn audit_log(
    _: ExtractActionIdentity,
    Json(_): Json<AuditLogParams>,
) -> Result<Json<JsonValue>, HttpResponseError> {
    Err(anyhow::anyhow!(ErrorMetadata::bad_request(
        "AuditLogNotSupportedInAction",
        "Audit logging is not yet supported in actions",
    ))
    .into())
}

pub static CONVEX_ACTIONS_CALLBACK_TOKEN: &str = "Convex-Action-Callback-Token";

async fn check_actions_token(
    st: &LocalAppState,
    headers: &HeaderMap,
) -> anyhow::Result<(SystemTime, ComponentId)> {
    let value = headers
        .get(CONVEX_ACTIONS_CALLBACK_TOKEN)
        .context("Missing callback token - is the call from actions?")?;

    let token = value
        .to_str()
        .context("Callback token must be an ASCII string")?;

    // Tokens are valid for 2x the action timeout, which should be more than enough
    // assuming the timeout measures in tens of seconds.
    let validity = 2 * *V8_ACTION_USER_TIMEOUT;
    st.application
        .key_broker()
        .check_action_token(&token.to_owned(), validity)
}

fn get_encoded_span(headers: &HeaderMap) -> anyhow::Result<EncodedSpan> {
    Ok(EncodedSpan(
        headers
            .get("Convex-Encoded-Parent-Trace")
            .map(|value| value.to_str())
            .transpose()
            .context("Convex-Encoded-Parent-Trace must be a string")?
            .map(|s| s.to_string()),
    ))
}

pub async fn action_callbacks_middleware<S>(
    MtState(st): MtState<LocalAppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<impl IntoResponse, HttpResponseError>
where
    LocalAppState: FromMtState<S>,
{
    // Validate we have an valid token in order to call any methods in this
    // actions_callback router.
    check_actions_token(&st, req.headers()).await?;

    let encoded_parent = get_encoded_span(req.headers())?;
    let root = initialize_root_from_parent(req.uri().path(), encoded_parent);

    let resp = next.run(req).in_span(root).await;
    Ok(resp)
}

// Similar to ExtractIdentity, but validates as of the action token issue time
// instead of the current time.
pub struct ExtractActionIdentity {
    identity: Identity,
    component_id: ComponentId,
}

impl<S> FromRequestParts<S> for ExtractActionIdentity
where
    LocalAppState: FromMtState<S>,
    S: Send + Sync + Clone + 'static,
{
    type Rejection = HttpResponseError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        st: &S,
    ) -> Result<Self, Self::Rejection> {
        let st = LocalAppState::from_request_parts(parts, st).await?;
        let token: AuthenticationToken =
            parts.extract::<ExtractAuthenticationToken>().await?.into();

        // Validate the auth token based on when the action token was issued. This
        // prevents errors due to auth token expiring in the middle of long action.
        let (issue_time, component_id) = check_actions_token(&st, &parts.headers).await?;
        let identity = st.application.authenticate(token, issue_time).await?;
        st.application
            .validate_component_id(identity.clone(), component_id)
            .await?;
        Ok(Self {
            identity,
            component_id,
        })
    }
}

pub struct ExtractActionName(pub Option<String>);

impl<S: Sync> FromRequestParts<S> for ExtractActionName {
    type Rejection = HttpResponseError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _st: &S,
    ) -> Result<Self, Self::Rejection> {
        let action_name = parts
            .headers
            .get("Convex-Action-Function-Name")
            .map(|value| value.to_str())
            .transpose()
            .context("Convex-Action-Function-Name must be a string")?
            .map(|s| s.to_string());

        Ok(Self(action_name))
    }
}

pub struct ExtractExecutionContext(pub ExecutionContext);

impl<T: Sync> FromRequestParts<T> for ExtractExecutionContext {
    type Rejection = HttpResponseError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _st: &T,
    ) -> Result<Self, Self::Rejection> {
        let request_id: RequestId = parts
            .headers
            .get("Convex-Request-Id")
            .map(|v| v.to_str())
            .transpose()
            .context("Request id must be a string")?
            .map(RequestId::from_str)
            .transpose()?
            // Only for backwards compatibility
            .unwrap_or(RequestId::new());
        let execution_id: ExecutionId = parts
            .headers
            .get("Convex-Execution-Id")
            .map(|v| v.to_str())
            .transpose()
            .context("Execution id must be a string")?
            .map(ExecutionId::from_str)
            .transpose()?
            // For backwards compatibility
            .unwrap_or(ExecutionId::new());

        let is_root: bool = match parts.headers.get("Convex-Root-Request") {
            Some(v) => v.to_str().context("Convex-Root-Request must be a string")? == "true",
            None => false,
        };
        let parent_job_id = parts
            .headers
            .get("Convex-Parent-Scheduled-Job")
            .map(|v| v.to_str())
            .transpose()
            .context("Parent scheduled job id must be a string")?
            .map(|s| s.parse())
            .transpose()
            .context("Invalid scheduled job id")?;
        let parent_component_id = ComponentId::deserialize_from_string(
            parts
                .headers
                .get("Convex-Parent-Scheduled-Job-Component-Id")
                .map(|v| v.to_str())
                .transpose()
                .context("Parent scheduled job component id must be a string")?,
        )
        .context("Invalid parent scheduled job component id")?;

        let client_ip: Option<ClientIp> = parts
            .headers
            .get("Convex-Request-Client-Ip")
            .map(|v| v.to_str())
            .transpose()
            .context("Request client IP must be a string")?
            .map(|s| ClientIp::from(s.to_owned()));

        let client_user_agent: Option<ClientUserAgent> = parts
            .headers
            .get("Convex-Request-Client-User-Agent")
            .map(|v| v.to_str())
            .transpose()
            .context("Request User-Agent must be a string")?
            .map(|s| ClientUserAgent::from(s.to_owned()));

        Ok(Self(ExecutionContext::new_from_parts(
            request_id,
            execution_id,
            parent_job_id.map(|id| (parent_component_id, id)),
            is_root,
            RequestMetadata {
                ip: client_ip,
                user_agent: client_user_agent,
            },
        )))
    }
}
