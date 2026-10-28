use std::time::Duration;

use anyhow::Context;
use axum::{
    body::Body,
    response::IntoResponse,
};
use axum_extra::{
    headers::{
        CacheControl,
        ContentLength,
    },
    TypedHeader,
};
use common::{
    components::ComponentId,
    http::{
        extract::{
            Json,
            MtState,
            Path,
            Query,
        },
        ExtractRequestMetadata,
        HttpResponseError,
    },
    types::SetExportExpirationRequest,
};
use either::Either;
use errors::ErrorMetadata;
use http::StatusCode;
use model::{
    deployment_audit_log::types::DeploymentAuditLogEvent,
    exports::{
        types::{
            ExportFormat,
            ExportRequestor,
        },
        ExportsModel,
    },
};
use roles::RequireDeploymentOp;
use serde::{
    Deserialize,
    Serialize,
};
use storage::StorageGetStream;
use sync_types::Timestamp;
use value::DeveloperDocumentId;

use crate::{
    authentication::ExtractIdentity,
    custom_headers::ContentDispositionAttachment,
    LocalAppState,
};

// Export GETs are immutable. Browser can cache for a long time.
const MAX_CACHE_AGE: Duration = Duration::from_secs(60 * 60 * 24 * 30);

// Export download tokens are minted right before the download starts, so they
// only need to live long enough to cover the redirect to the download URL.
const EXPORT_DOWNLOAD_TOKEN_VALIDITY: Duration = Duration::from_secs(5 * 60);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestZipExport {
    #[serde(default)]
    pub include_storage: bool,
    pub component: Option<String>,
}

#[fastrace::trace]
pub async fn request_zip_export(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Query(RequestZipExport {
        include_storage,
        component,
    }): Query<RequestZipExport>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let component = ComponentId::deserialize_from_string(component.as_deref())?;
    st.application
        .request_export(
            identity,
            request_metadata,
            ExportFormat::Zip { include_storage },
            component,
            ExportRequestor::SnapshotExport,
            None,
        )
        .await?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct ZipExportRequest {
    // The ID of the snapshot
    id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZipExportTokenResponse {
    token: String,
}

/// Mints a short-lived token authorizing the download of a single snapshot
/// export. This lets the dashboard trigger a browser download (an `<a href>`
/// navigation, which can't carry an `Authorization` header) without putting
/// the long-lived admin key in the URL.
#[fastrace::trace]
pub async fn request_zip_export_token(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Path(ZipExportRequest { id }): Path<ZipExportRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::DownloadBackups)?;
    let actor = keybroker::ExportDownloadActor {
        member_id: identity.member_id(),
        token_id: identity.token_id(),
    };
    let token = st
        .application
        .key_broker()
        .issue_export_download_token(id, actor);
    Ok(Json(ZipExportTokenResponse { token }))
}

#[derive(Deserialize)]
pub struct GetZipExportQueryArgs {
    token: Option<String>,
}

pub async fn get_zip_export(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(ZipExportRequest { id }): Path<ZipExportRequest>,
    Query(GetZipExportQueryArgs { token }): Query<GetZipExportQueryArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let identity = match token {
        Some(token) => {
            let actor = st.application.key_broker().check_export_download_token(
                &token,
                &id,
                EXPORT_DOWNLOAD_TOKEN_VALIDITY,
            )?;
            st.application
                .export_download_identity(actor, token)
                .await?
        },
        None => identity,
    };
    identity.require_operation(keybroker::DeploymentOp::DownloadBackups)?;
    let id: Either<DeveloperDocumentId, Timestamp> = match id.parse() {
        Ok(id) => Either::Left(id),
        Err(_) => Either::Right(id.parse().context(ErrorMetadata::bad_request(
            "BadSnapshotId",
            "Snapshot Id did not parse to an ID.",
        ))?),
    };
    let (
        StorageGetStream {
            content_length,
            stream,
        },
        filename,
    ) = st
        .application
        .get_zip_export(identity, request_metadata, id)
        .await?;
    let content_length = ContentLength(content_length as u64);
    Ok((
        TypedHeader(content_length),
        // `ContentDisposition::attachment()` is not implemented in the headers library yet!
        // so we handroll it:
        TypedHeader(ContentDispositionAttachment(filename)),
        TypedHeader(
            CacheControl::new()
                .with_private()
                .with_max_age(MAX_CACHE_AGE),
        ),
        Body::from_stream(stream),
    ))
}

#[derive(Deserialize)]
pub struct SetExportExpirationPathArgs {
    snapshot_id: String,
}

#[fastrace::trace]
pub async fn set_export_expiration(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(SetExportExpirationPathArgs { snapshot_id }): Path<SetExportExpirationPathArgs>,
    Json(SetExportExpirationRequest { expiration_ts_ns }): Json<SetExportExpirationRequest>,
) -> Result<StatusCode, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::DeleteBackups)?;
    let snapshot_id: DeveloperDocumentId = snapshot_id
        .parse::<DeveloperDocumentId>()
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut tx = st.application.begin(identity).await?;
    ExportsModel::new(&mut tx)
        .set_expiration(snapshot_id, expiration_ts_ns)
        .await?;
    st.application
        .commit_with_audit_log_events(
            tx,
            vec![DeploymentAuditLogEvent::SetExportExpiration {
                id: snapshot_id.encode(),
                expiration_ts_ms: (expiration_ts_ns / 1_000_000) as i64,
            }],
            request_metadata,
            "set_export_expiration",
        )
        .await?;
    Ok(StatusCode::OK)
}

#[fastrace::trace]
pub async fn cancel_export(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(SetExportExpirationPathArgs { snapshot_id }): Path<SetExportExpirationPathArgs>,
) -> Result<StatusCode, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ImportBackups)?;
    let snapshot_id: DeveloperDocumentId = snapshot_id
        .parse::<DeveloperDocumentId>()
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut tx = st.application.begin(identity).await?;
    ExportsModel::new(&mut tx).cancel(snapshot_id).await?;
    st.application
        .commit_with_audit_log_events(
            tx,
            vec![DeploymentAuditLogEvent::CancelExport {
                id: snapshot_id.encode(),
            }],
            request_metadata,
            "cancel_export",
        )
        .await?;
    Ok(StatusCode::OK)
}
