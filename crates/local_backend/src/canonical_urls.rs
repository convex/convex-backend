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
    RequestDestination,
};
use http::StatusCode;
use model::{
    canonical_urls::{
        types::CanonicalUrl,
        CanonicalUrlsModel,
    },
    deployment_audit_log::types::DeploymentAuditLogEvent,
};
use roles::RequireDeploymentOp;
use serde::{
    Deserialize,
    Serialize,
};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;

use crate::{
    admin::must_be_admin,
    authentication::ExtractIdentity,
    LocalAppState,
};

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCanonicalUrlRequest {
    /// Whether to update the canonical URL for convex.cloud or convex.site
    request_destination: RequestDestination,
    /// The new canonical URL. Omit this to reset the canonical URl to the
    /// default value.
    url: Option<String>,
}

/// Update canonical URL
///
/// Set or unset the canonical URL for a deployment's convex.cloud or
/// convex.site domain. This allows you to customize the  CONVEX_SITE_URL and
/// CONVEX_CLOUD_URL environment variables in your deployment.
#[utoipa::path(
    post,
    path = "/update_canonical_url",
    tag = "Canonical URLs",
    request_body = UpdateCanonicalUrlRequest,
    responses((status = 200)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn update_canonical_url(
    State(st): State<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(request): Json<UpdateCanonicalUrlRequest>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteEnvironmentVariables)?;

    let mut tx = st.application.begin(identity).await?;

    let mut audit_log_events = vec![];
    if let Some(url) = request.url.clone() {
        let canonical_url = CanonicalUrl {
            request_destination: request.request_destination,
            url: url.clone(),
        };
        st.application
            .set_canonical_url(&mut tx, canonical_url)
            .await?;
        audit_log_events.push(DeploymentAuditLogEvent::UpdateCanonicalUrl {
            request_destination: request.request_destination,
            url,
        });
    } else {
        st.application
            .unset_canonical_url(&mut tx, request.request_destination)
            .await?;
        audit_log_events.push(DeploymentAuditLogEvent::DeleteCanonicalUrl {
            request_destination: request.request_destination,
        });
    }

    st.application
        .commit_with_audit_log_events(
            tx,
            audit_log_events,
            request_metadata,
            "update_canonical_url",
        )
        .await?;

    Ok(StatusCode::OK)
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetCanonicalUrlsResponse {
    convex_cloud_url: String,
    convex_site_url: String,
}

/// Get canonical URLs
///
/// Get the canonical URLs for a deployment.
#[utoipa::path(
    get,
    path = "/get_canonical_urls",
    tag = "Canonical URLs",
    responses(
        (status = 200, body = GetCanonicalUrlsResponse)
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn get_canonical_urls(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    if !identity.is_system() {
        // Any admin can view canonical URLs as they are
        // not secret & necessary information for deploying
        // via CLI.
        must_be_admin(&identity)?;
    }

    let mut tx = st.application.begin(identity).await?;
    let urls = CanonicalUrlsModel::new(&mut tx)
        .get_canonical_urls()
        .await?;

    let mut convex_cloud_url = None;
    let mut convex_site_url = None;

    for (destination, url) in urls {
        match destination {
            RequestDestination::ConvexCloud => {
                convex_cloud_url = Some(url.into_value().url);
            },
            RequestDestination::ConvexSite => {
                convex_site_url = Some(url.into_value().url);
            },
        }
    }

    // If canonical URLs aren't set, return the default URLs
    let convex_cloud_url = convex_cloud_url.unwrap_or_else(|| st.origin.to_string());
    let convex_site_url = convex_site_url.unwrap_or_else(|| st.site_origin.to_string());

    Ok(Json(GetCanonicalUrlsResponse {
        convex_cloud_url,
        convex_site_url,
    }))
}

pub fn platform_router<S>() -> OpenApiRouter<S>
where
    LocalAppState: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    OpenApiRouter::new().routes(utoipa_axum::routes!(
        update_canonical_url,
        get_canonical_urls
    ))
}
