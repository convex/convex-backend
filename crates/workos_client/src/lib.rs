// These APIs are informally documented in https://convexdev.slack.com/archives/C071SS99WLA/p1776902623741739?thread_ts=1776900543.948619&cid=C071SS99WLA

use std::{
    collections::{
        HashMap,
        HashSet,
    },
    fmt,
    future::Future,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use chrono::{
    DateTime,
    Utc,
};
use common::types::MemberId;
use errors::ErrorMetadata;
use oauth2::{
    HttpRequest,
    HttpResponse,
};
use parking_lot::RwLock;
use serde::{
    Deserialize,
    Serialize,
};
use tokio::time::timeout;

/// Maps WorkOS identity providers to Auth0-compatible subject formats
pub fn map_workos_identities_to_subjects(
    workos_user_id: &str,
    identities: &[WorkOSIdentity],
) -> anyhow::Result<Vec<String>> {
    match identities.len() {
        // If there are no identities
        0 => Ok(vec![workos_user_id.to_string()]),
        _ => identities
            .iter()
            .map(|identity| -> anyhow::Result<String> {
                let mapped_provider = match identity.provider.as_str() {
                    "MicrosoftOAuth" => "microsoft",
                    "GithubOAuth" => "github",
                    "GoogleOAuth" => "google-oauth2",
                    "VercelOAuth" => "vercel",
                    "VercelMarketplaceOAuth" => "vercel",

                    _ => anyhow::bail!("Unsupported provider: {}", identity.provider),
                };

                // This is the old format of Auth0 subjects for backwards compatability
                let subject = format!("{}|{}", mapped_provider, identity.idp_id);
                Ok(subject)
            })
            .collect::<Result<Vec<String>, _>>(),
    }
}

/// WorkOS provider name for Vercel Marketplace OAuth identities. Their
/// `idp_id` encodes the Vercel account and user as
/// `account:{account_id}:user:{user_id}`.
pub const VERCEL_MARKETPLACE_PROVIDER: &str = "VercelMarketplaceOAuth";

/// Parse a Vercel Marketplace identity `idp_id` of the form
/// `account:{account_id}:user:{user_id}` into `(account_id, user_id)`. The
/// `account_id` matches `vercel_installations.account_id`.
pub fn parse_vercel_marketplace_idp_id(idp_id: &str) -> Option<(&str, &str)> {
    let rest = idp_id.strip_prefix("account:")?;
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.len() >= 3 && parts[1] == "user" {
        Some((parts[0], parts[2]))
    } else {
        None
    }
}

/// Extract the `(account_id, user_id)` pairs of a WorkOS user's Vercel
/// Marketplace identities.
pub fn vercel_marketplace_accounts(identities: &[WorkOSIdentity]) -> Vec<(String, String)> {
    identities
        .iter()
        .filter(|identity| identity.provider == VERCEL_MARKETPLACE_PROVIDER)
        .filter_map(|identity| parse_vercel_marketplace_idp_id(&identity.idp_id))
        .map(|(account_id, user_id)| (account_id.to_string(), user_id.to_string()))
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkOSIdentity {
    pub idp_id: String,
    pub provider: String,
}

#[derive(Debug, Deserialize)]
pub struct WorkOSUser {
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

/// WorkOS list endpoints wrap their results in a `data` array, alongside
/// `list_metadata` used for cursor pagination.
#[derive(Debug, Deserialize)]
struct WorkOSListResponse<T> {
    data: Vec<T>,
    #[serde(default)]
    list_metadata: WorkOSListMetadata,
}

/// Cursor pagination metadata returned by WorkOS list endpoints. `after` is the
/// cursor to pass as the `after` query parameter to fetch the next page; it is
/// absent (or null) once the final page has been returned.
#[derive(Debug, Deserialize, Default)]
struct WorkOSListMetadata {
    #[serde(default)]
    after: Option<String>,
}

/// Minimal projection of a WorkOS user when we only need its id (e.g. when
/// looking a user up by email).
#[derive(Debug, Deserialize)]
struct WorkOSUserId {
    id: String,
}

#[derive(Debug, Deserialize)]
pub struct WorkOSAuthFactor {
    pub id: String,
    #[serde(rename = "type")]
    pub factor_type: String,
}

const APPLICATION_JSON: http::HeaderValue = http::HeaderValue::from_static("application/json");

// Timeout for external WorkOS API calls
// Most calls take less than 1 second, but deletion can take up to 18 seconds.
const WORKOS_API_TIMEOUT: Duration = Duration::from_secs(30);

/// Structured error type for WorkOS API failures
#[derive(Debug, Clone)]
pub struct WorkOSApiError {
    pub operation: String,
    pub status: http::StatusCode,
    pub response_body: String,
}

impl WorkOSApiError {
    fn new(operation: &str, status: http::StatusCode, response_body: &[u8]) -> Self {
        let body_str = String::from_utf8_lossy(response_body);
        let truncated_body = if body_str.len() > 1000 {
            format!("{}...", &body_str[..1000])
        } else {
            body_str.to_string()
        };

        Self {
            operation: operation.to_string(),
            status,
            response_body: truncated_body,
        }
    }

    /// Check if this is an authentication error (401)
    pub fn is_auth_error(&self) -> bool {
        self.status == http::StatusCode::UNAUTHORIZED
    }
}

impl fmt::Display for WorkOSApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WorkOS {} API returned HTTP {} {}: {}",
            self.operation,
            self.status.as_u16(),
            self.status.canonical_reason().unwrap_or("Unknown"),
            self.response_body
        )
    }
}

impl std::error::Error for WorkOSApiError {}

// Helper function to check if an error is an authentication error (401)
fn is_auth_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<WorkOSApiError>()
        .map(|e| e.is_auth_error())
        .unwrap_or(false)
}

/// Check if an error is a WorkOS "team_not_found" error (HTTP 404 with the
/// corresponding error code in the response body). This happens when the
/// WorkOS team has been deleted on WorkOS's side.
pub fn is_team_not_found_error(err: &anyhow::Error) -> bool {
    let Some(api_err) = err.downcast_ref::<WorkOSApiError>() else {
        return false;
    };
    if api_err.status != http::StatusCode::NOT_FOUND {
        return false;
    }
    serde_json::from_str::<WorkOSErrorResponse>(&api_err.response_body)
        .ok()
        .and_then(|r| r.code)
        .as_deref()
        == Some("team_not_found")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum WorkOSProductionState {
    Active,
    Inactive,
    Suspended,
    Deleting,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkOSTeamResponse {
    /// always "team"
    pub object: String,
    /// like "team_01K58C005DSAQCZSX84FFWMT5G"
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub production_state: Option<WorkOSProductionState>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkOSTeamInvitationResponse {
    /// always "workos_team_invitation"
    pub object: String,
    pub email: String,
    pub role_slug: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkOSEnvironmentResponse {
    /// always "environment"
    pub object: String,
    /// like "environment_01K5DJZTWGXWJMFSMHY3HCXK8N"
    pub id: String,
    pub name: String,
    pub client_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkOSAPIKeyResponse {
    /// always "api_key"
    pub object: String,
    /// like "key_01K5DJZVGQ4JM58YS3VC5C5QD3"
    pub id: String,
    pub name: String,
    pub expires_at: Option<String>,
    /// like 'sk_test_a2V5XzAxSzVESlpWR1E0Sk01OFlTM1ZDNUM1UUQzLEIzZkcxNkVxR0swanZVQUZaTXN4VmNWTng'
    pub value: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
/// WorkOS API error response JSON body structure.
/// This is the JSON object returned in the HTTP response body when WorkOS APIs
/// return error status codes (4xx, 5xx).
///
/// Example response body for a 400 Bad Request:
/// ```json
/// {
///   "code": "user_already_exists",  // Sometimes present
///   "error": "Bad Request",          // Sometimes present, mirrors HTTP status
///   "message": "User email@example.com is already a member of this team."
/// }
/// ```
///
/// Example response body for a 500 Internal Server Error:
/// ```json
/// {
///   "message": "Internal Server Error"  // Note: no "error" or "code" fields
/// }
/// ```
pub struct WorkOSErrorResponse {
    /// Error code when available (e.g., "user_already_exists",
    /// "platform_not_authorized")
    pub code: Option<String>,
    /// Generic error category (e.g., "Bad Request").
    /// Not present in 500 Internal Server Error responses.
    pub error: Option<String>,
    /// Human-readable error message that we parse for specific scenarios when
    /// code is absent
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WorkOSOrganizationResponse {
    /// always "organization"
    pub object: String,
    /// like "org_01K5DJZVGQ4JM58YS3VC5QD3"
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub domains: Vec<WorkOSOrganizationDomain>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WorkOSOrganizationDomain {
    pub object: String,
    pub id: String,
    pub domain: String,
    pub state: WorkOSDomainState,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum WorkOSDomainState {
    Verified,
    Pending,
    Failed,
    LegacyVerified,
}

impl WorkOSDomainState {
    pub fn is_verified(&self) -> bool {
        match self {
            WorkOSDomainState::Verified | WorkOSDomainState::LegacyVerified => true,
            WorkOSDomainState::Pending | WorkOSDomainState::Failed => false,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WorkOSOrganizationMembershipResponse {
    /// always "organization_membership"
    pub object: String,
    /// like "om_01E4ZCR3C5A4QZ2Z2JQXGKZJ9E"
    pub id: String,
    pub user_id: String,
    pub organization_id: String,
    pub role: WorkOSOrganizationRole,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WorkOSOrganizationRole {
    pub slug: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkOSPortalLinkResponse {
    /// The portal link URL
    pub link: String,
}

#[derive(Debug, Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum WorkOSPortalIntent {
    Sso,
    DomainVerification,
    CertificateRenewal,
    /// WorkOS serializes the directory sync portal intent as "dsync".
    #[serde(rename = "dsync")]
    DirectorySync,
}

/// A WorkOS SSO Connection.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SSOConnection {
    /// like "conn_01E4ZCR3C56J083X43JQXF3JK"
    pub id: String,
    pub name: String,
    /// e.g. "OktaSAML", "AzureSAML", "GoogleSAML"
    pub connection_type: String,
    /// e.g. "active", "inactive", "draft", "validating"
    pub state: String,
}

impl SSOConnection {
    /// Whether this connection can actually be used to log in. A connection
    /// that is still a draft or validating exists in WorkOS but does not
    /// authenticate anyone yet.
    pub fn is_active(&self) -> bool {
        self.state == "active"
    }
}

/// A WorkOS Directory Sync directory.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Directory {
    /// like "directory_01ECAZ4NV9QMV47GW873HDCX74"
    pub id: String,
    /// e.g. "linked", "unlinked", "invalid_credentials".
    pub state: String,
    #[serde(default)]
    pub name: Option<String>,
    /// The identity provider behind the directory, e.g. "okta scim v2.0",
    /// "azure scim v2.0", "gsuite directory", "generic scim v2.0".
    #[serde(default)]
    pub r#type: Option<String>,
}

impl Directory {
    /// Whether this directory is actively syncing. An unlinked directory or
    /// one with rejected credentials exists but delivers no events.
    pub fn is_linked(&self) -> bool {
        self.state == "linked"
    }
}

/// A group within a WorkOS Directory Sync directory.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DirectoryGroup {
    /// like "directory_group_01E1JG7J09H96KYP8HM9B0G5SJ"
    pub id: String,
    /// The identity provider's own id for this group.
    pub idp_id: String,
    pub name: String,
}

/// A user within a WorkOS Directory Sync directory.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DirectoryUser {
    /// like "directory_user_01E1JG7J09H96KYP8HM9B0G5SJ"
    pub id: String,
    /// The directory user's email address.
    #[serde(default)]
    pub email: Option<String>,
    /// e.g. "active", "inactive", "suspended".
    pub state: String,
}

/// An entry in the WorkOS Events API log. Events are immutable and ordered, so
/// a consumer resumes by passing the last `id` it processed back as the `after`
/// parameter.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WorkOSEvent {
    /// like "event_01E4ZCR3C56J083X43JQXF3JK5"
    pub id: String,
    /// e.g. "dsync.user.created".
    pub event: String,
    pub created_at: DateTime<Utc>,
    /// The event-type-specific body, e.g. a `DirectoryUser` for
    /// `dsync.user.*`.
    pub data: serde_json::Value,
}

#[async_trait]
pub trait WorkOSClient: Send + Sync {
    async fn fetch_identities(&self, user_id: &str) -> anyhow::Result<Vec<WorkOSIdentity>>;
    async fn fetch_user(&self, user_id: &str) -> anyhow::Result<WorkOSUser>;
    /// Whether the WorkOS user with this email has any enrolled MFA factors.
    async fn email_has_enrolled_mfa(&self, email: &str) -> anyhow::Result<bool>;
    /// The id of the WorkOS user with this email, if one exists. Emails are
    /// unique per environment, so at most one user is returned.
    async fn find_user_id_by_email(&self, email: &str) -> anyhow::Result<Option<String>>;
    async fn delete_user(&self, user_id: &str) -> anyhow::Result<()>;
    async fn update_user_metadata(&self, user_id: &str, member_id: MemberId) -> anyhow::Result<()>;
    async fn update_user_email(&self, user_id: &str, email: &str) -> anyhow::Result<()>;

    // Organization methods
    async fn create_organization(
        &self,
        name: &str,
        external_id: &str,
    ) -> anyhow::Result<WorkOSOrganizationResponse>;
    async fn get_organization(
        &self,
        external_id: &str,
    ) -> anyhow::Result<Option<WorkOSOrganizationResponse>>;
    async fn get_organization_by_id(
        &self,
        organization_id: &str,
    ) -> anyhow::Result<Option<WorkOSOrganizationResponse>>;
    async fn update_organization(
        &self,
        organization_id: &str,
        name: Option<&str>,
        domain: Option<&str>,
    ) -> anyhow::Result<WorkOSOrganizationResponse>;
    async fn delete_organization(&self, organization_id: &str) -> anyhow::Result<()>;
    /// Removes one domain from its organization. WorkOS has no list-domains
    /// endpoint; the domains an organization holds come back on
    /// [`WorkOSClient::get_organization_by_id`]. Deleting an already-deleted
    /// domain is a no-op.
    async fn delete_organization_domain(&self, domain_id: &str) -> anyhow::Result<()>;

    // Organization membership methods
    async fn create_membership(
        &self,
        user_id: &str,
        organization_id: &str,
        role_slug: &str,
    ) -> anyhow::Result<WorkOSOrganizationMembershipResponse>;
    async fn list_organization_memberships(
        &self,
        user_id: &str,
    ) -> anyhow::Result<Vec<WorkOSOrganizationMembershipResponse>>;

    // Portal link generation
    async fn generate_portal_link(
        &self,
        organization_id: &str,
        intent: WorkOSPortalIntent,
    ) -> anyhow::Result<WorkOSPortalLinkResponse>;

    // SSO methods
    async fn list_sso_connections(
        &self,
        organization_id: &str,
    ) -> anyhow::Result<Vec<SSOConnection>>;
    /// Deletes an SSO connection. Deleting an already-deleted connection is a
    /// no-op.
    async fn delete_sso_connection(&self, connection_id: &str) -> anyhow::Result<()>;

    // Directory Sync methods
    async fn list_directories(&self, organization_id: &str) -> anyhow::Result<Vec<Directory>>;
    /// Deletes a directory. Deleting an already-deleted directory is a no-op.
    async fn delete_directory(&self, directory_id: &str) -> anyhow::Result<()>;
    async fn list_directory_groups(
        &self,
        directory_id: &str,
    ) -> anyhow::Result<Vec<DirectoryGroup>>;
    async fn list_directory_users(&self, directory_id: &str) -> anyhow::Result<Vec<DirectoryUser>>;
    async fn get_directory_user(
        &self,
        directory_user_id: &str,
    ) -> anyhow::Result<Option<DirectoryUser>>;
    /// Fetches a single directory group by id. Returns `Ok(None)` when the
    /// group no longer exists.
    async fn get_directory_group(
        &self,
        directory_group_id: &str,
    ) -> anyhow::Result<Option<DirectoryGroup>>;
    /// Lists the groups a directory user currently belongs to, following
    /// pagination.
    async fn list_directory_groups_for_user(
        &self,
        directory_user_id: &str,
    ) -> anyhow::Result<Vec<DirectoryGroup>>;

    /// Fetches one page of the Events API, restricted to `event_types` and
    /// resuming after the event id `after`. With no `after`, WorkOS returns
    /// from the start of its event retention window. A page shorter than
    /// `limit` means the caller has caught up. Errors if `event_types` is
    /// empty.
    async fn list_events(
        &self,
        event_types: &[&str],
        after: Option<&str>,
        limit: u32,
    ) -> anyhow::Result<Vec<WorkOSEvent>>;
}

// Separate trait for WorkOS Platform API operations (requires different API
// key)
#[async_trait]
pub trait WorkOSPlatformClient: Send + Sync {
    async fn create_team(
        &self,
        admin_email: &str,
        team_name: &str,
    ) -> anyhow::Result<WorkOSTeamResponse>;
    async fn get_team(&self, workos_team_id: &str) -> anyhow::Result<WorkOSTeamResponse>;
    async fn create_environment(
        &self,
        workos_team_id: &str,
        environment_name: &str,
        is_production: Option<bool>,
    ) -> anyhow::Result<WorkOSEnvironmentResponse>;
    async fn get_environment(
        &self,
        workos_team_id: &str,
        environment_id: &str,
    ) -> anyhow::Result<WorkOSEnvironmentResponse>;
    async fn create_api_key(
        &self,
        workos_team_id: &str,
        environment_id: &str,
        key_name: &str,
    ) -> anyhow::Result<WorkOSAPIKeyResponse>;
    async fn invite_team_member(
        &self,
        workos_team_id: &str,
        email: &str,
        role_slug: &str,
    ) -> anyhow::Result<WorkOSTeamInvitationResponse>;
    async fn delete_environment(
        &self,
        workos_team_id: &str,
        environment_id: &str,
    ) -> anyhow::Result<()>;
}

pub struct RealWorkOSClient<F, E>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    api_key: String,
    http_client: Box<dyn Fn(HttpRequest) -> F + Send + Sync + 'static>,
}

impl<F, E> RealWorkOSClient<F, E>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    pub fn new(
        api_key: String,
        http_client: impl Fn(HttpRequest) -> F + Send + Sync + 'static,
    ) -> Self {
        Self {
            api_key,
            http_client: Box::new(http_client),
        }
    }
}

#[async_trait]
impl<F, E> WorkOSClient for RealWorkOSClient<F, E>
where
    F: Future<Output = Result<HttpResponse, E>> + Send + 'static,
    E: std::error::Error + 'static + Send + Sync,
{
    async fn fetch_identities(&self, user_id: &str) -> anyhow::Result<Vec<WorkOSIdentity>> {
        fetch_workos_identities(&self.api_key, user_id, &*self.http_client).await
    }

    async fn fetch_user(&self, user_id: &str) -> anyhow::Result<WorkOSUser> {
        fetch_workos_user(&self.api_key, user_id, &*self.http_client).await
    }

    async fn email_has_enrolled_mfa(&self, email: &str) -> anyhow::Result<bool> {
        email_has_enrolled_mfa(&self.api_key, email, &*self.http_client).await
    }

    async fn find_user_id_by_email(&self, email: &str) -> anyhow::Result<Option<String>> {
        find_workos_user_id_by_email(&self.api_key, email, &*self.http_client).await
    }

    async fn delete_user(&self, user_id: &str) -> anyhow::Result<()> {
        delete_workos_user(&self.api_key, user_id, &*self.http_client).await
    }

    async fn update_user_metadata(&self, user_id: &str, member_id: MemberId) -> anyhow::Result<()> {
        update_workos_user_metadata(&self.api_key, user_id, member_id, &*self.http_client).await
    }

    async fn update_user_email(&self, user_id: &str, email: &str) -> anyhow::Result<()> {
        update_workos_user_email(&self.api_key, user_id, email, &*self.http_client).await
    }

    async fn create_organization(
        &self,
        name: &str,
        external_id: &str,
    ) -> anyhow::Result<WorkOSOrganizationResponse> {
        create_workos_organization(&self.api_key, name, external_id, &*self.http_client).await
    }

    async fn get_organization(
        &self,
        external_id: &str,
    ) -> anyhow::Result<Option<WorkOSOrganizationResponse>> {
        get_workos_organization_by_external_id(&self.api_key, external_id, &*self.http_client).await
    }

    async fn get_organization_by_id(
        &self,
        organization_id: &str,
    ) -> anyhow::Result<Option<WorkOSOrganizationResponse>> {
        get_workos_organization_by_id(&self.api_key, organization_id, &*self.http_client).await
    }

    async fn update_organization(
        &self,
        organization_id: &str,
        name: Option<&str>,
        domain: Option<&str>,
    ) -> anyhow::Result<WorkOSOrganizationResponse> {
        update_workos_organization(
            &self.api_key,
            organization_id,
            name,
            domain,
            &*self.http_client,
        )
        .await
    }

    async fn delete_organization(&self, organization_id: &str) -> anyhow::Result<()> {
        delete_workos_organization(&self.api_key, organization_id, &*self.http_client).await
    }

    async fn delete_organization_domain(&self, domain_id: &str) -> anyhow::Result<()> {
        delete_workos_resource(
            &self.api_key,
            &format!("https://api.workos.com/organization_domains/{domain_id}"),
            "delete organization domain",
            &*self.http_client,
        )
        .await
    }

    async fn list_organization_memberships(
        &self,
        user_id: &str,
    ) -> anyhow::Result<Vec<WorkOSOrganizationMembershipResponse>> {
        list_workos_organization_memberships(&self.api_key, user_id, &*self.http_client).await
    }

    async fn create_membership(
        &self,
        user_id: &str,
        organization_id: &str,
        role_slug: &str,
    ) -> anyhow::Result<WorkOSOrganizationMembershipResponse> {
        create_workos_membership(
            &self.api_key,
            user_id,
            organization_id,
            role_slug,
            &*self.http_client,
        )
        .await
    }

    async fn generate_portal_link(
        &self,
        organization_id: &str,
        intent: WorkOSPortalIntent,
    ) -> anyhow::Result<WorkOSPortalLinkResponse> {
        generate_workos_portal_link(&self.api_key, organization_id, intent, &*self.http_client)
            .await
    }

    async fn list_sso_connections(
        &self,
        organization_id: &str,
    ) -> anyhow::Result<Vec<SSOConnection>> {
        list_workos_sso_connections(&self.api_key, organization_id, &*self.http_client).await
    }

    async fn delete_sso_connection(&self, connection_id: &str) -> anyhow::Result<()> {
        delete_workos_resource(
            &self.api_key,
            &format!("https://api.workos.com/connections/{connection_id}"),
            "delete SSO connection",
            &*self.http_client,
        )
        .await
    }

    async fn list_directories(&self, organization_id: &str) -> anyhow::Result<Vec<Directory>> {
        list_workos_directories(&self.api_key, organization_id, &*self.http_client).await
    }

    async fn delete_directory(&self, directory_id: &str) -> anyhow::Result<()> {
        delete_workos_resource(
            &self.api_key,
            &format!("https://api.workos.com/directories/{directory_id}"),
            "delete directory",
            &*self.http_client,
        )
        .await
    }

    async fn list_directory_groups(
        &self,
        directory_id: &str,
    ) -> anyhow::Result<Vec<DirectoryGroup>> {
        list_workos_directory_groups(&self.api_key, directory_id, &*self.http_client).await
    }

    async fn list_directory_users(&self, directory_id: &str) -> anyhow::Result<Vec<DirectoryUser>> {
        list_workos_directory_users(&self.api_key, directory_id, &*self.http_client).await
    }

    async fn get_directory_user(
        &self,
        directory_user_id: &str,
    ) -> anyhow::Result<Option<DirectoryUser>> {
        get_workos_directory_user(&self.api_key, directory_user_id, &*self.http_client).await
    }

    async fn get_directory_group(
        &self,
        directory_group_id: &str,
    ) -> anyhow::Result<Option<DirectoryGroup>> {
        get_workos_directory_group(&self.api_key, directory_group_id, &*self.http_client).await
    }

    async fn list_directory_groups_for_user(
        &self,
        directory_user_id: &str,
    ) -> anyhow::Result<Vec<DirectoryGroup>> {
        list_workos_directory_groups_for_user(&self.api_key, directory_user_id, &*self.http_client)
            .await
    }

    async fn list_events(
        &self,
        event_types: &[&str],
        after: Option<&str>,
        limit: u32,
    ) -> anyhow::Result<Vec<WorkOSEvent>> {
        list_workos_events(&self.api_key, event_types, after, limit, &*self.http_client).await
    }
}

/// An in-memory stand-in for the WorkOS API.
///
/// Organizations, SSO connections and directories are tracked independently so
/// tests can build an organization that has SSO but no directory sync, or the
/// reverse. Cloning shares the same state, so a test can keep a handle while
/// the client under test holds an `Arc<dyn WorkOSClient>`.
#[derive(Default, Clone)]
pub struct MockWorkOSClient {
    state: Arc<RwLock<MockWorkOSState>>,
}

#[derive(Default)]
struct MockWorkOSState {
    /// Configured `find_user_id_by_email` responses, keyed by lowercased email.
    email_to_user_id: HashMap<String, String>,
    /// Organizations keyed by WorkOS organization id.
    organizations: HashMap<String, WorkOSOrganizationResponse>,
    /// SSO connections keyed by organization id.
    connections: HashMap<String, Vec<SSOConnection>>,
    /// Directories keyed by organization id.
    directories: HashMap<String, Vec<Directory>>,
    /// Emails `is_mfa_enrolled` reports as enrolled, lowercased.
    mfa_emails: HashSet<String>,
    directory_groups: HashMap<String, Vec<DirectoryGroup>>,
    directory_users: HashMap<String, Vec<DirectoryUser>>,
    /// Users returned by [`MockWorkOSClient::get_directory_user`], keyed by
    /// directory user id.
    directory_users_by_id: HashMap<String, DirectoryUser>,
    /// Groups returned by [`get_directory_group`], keyed by directory group id.
    directory_groups_by_id: HashMap<String, DirectoryGroup>,
    /// Groups returned by [`list_directory_groups_for_user`], keyed by
    /// directory user id.
    directory_groups_by_user_id: HashMap<String, Vec<DirectoryGroup>>,
    /// Domains carried by the organizations [`get_organization_by_id`] returns,
    /// keyed by organization id.
    organization_domains: HashMap<String, Vec<WorkOSOrganizationDomain>>,
    organization_memberships: HashMap<String, Vec<WorkOSOrganizationMembershipResponse>>,
    /// Distinguishes the ids handed out by successive `create_organization`
    /// and `with_organization_domain` calls. Shared between the two so a
    /// domain id names one domain across every organization, as WorkOS's do.
    next_id: u64,
    /// The event log served by [`list_events`], oldest first.
    events: Vec<WorkOSEvent>,
}

impl MockWorkOSClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure `find_user_id_by_email` to return `workos_user_id` for
    /// `email`.
    pub fn with_email_user_id(self, email: &str, workos_user_id: &str) -> Self {
        self.state
            .write()
            .email_to_user_id
            .insert(email.to_lowercase(), workos_user_id.to_string());
        self
    }

    /// Give the organization [`get_organization_by_id`] returns for
    /// `organization_id` one more domain in `state`.
    pub fn with_organization_domain(
        self,
        organization_id: &str,
        domain: &str,
        state: WorkOSDomainState,
    ) -> Self {
        self.add_organization_domain(organization_id, domain, state);
        self
    }

    /// Like [`MockWorkOSClient::with_organization_domain`], for a client that
    /// is already shared, returning the id WorkOS assigned the domain.
    pub fn add_organization_domain(
        &self,
        organization_id: &str,
        domain: &str,
        state: WorkOSDomainState,
    ) -> String {
        let mut guard = self.state.write();
        guard.next_id += 1;
        let id = format!("org_domain_mock{}", guard.next_id);
        guard
            .organization_domains
            .entry(organization_id.to_string())
            .or_default()
            .push(WorkOSOrganizationDomain {
                object: "organization_domain".to_string(),
                id: id.clone(),
                domain: domain.to_string(),
                state,
            });
        id
    }

    /// Report `email` as MFA-enrolled from [`is_mfa_enrolled`].
    pub fn with_mfa_enrolled(self, email: &str) -> Self {
        self.state.write().mfa_emails.insert(email.to_lowercase());
        self
    }

    /// Attach an SSO connection to an organization, as if it had been created
    /// through the WorkOS admin portal.
    pub fn add_sso_connection(&self, organization_id: &str, connection: SSOConnection) {
        self.state
            .write()
            .connections
            .entry(organization_id.to_string())
            .or_default()
            .push(connection);
    }

    pub fn with_organization_membership(
        self,
        user_id: &str,
        organization_id: &str,
        status: &str,
    ) -> Self {
        {
            let mut guard = self.state.write();
            let memberships = guard
                .organization_memberships
                .entry(user_id.to_string())
                .or_default();
            memberships.push(WorkOSOrganizationMembershipResponse {
                object: "organization_membership".to_string(),
                id: format!("om_mock{}", memberships.len()),
                user_id: user_id.to_string(),
                organization_id: organization_id.to_string(),
                role: WorkOSOrganizationRole {
                    slug: "member".to_string(),
                },
                status: status.to_string(),
                created_at: "2024-01-01T00:00:00.000Z".to_string(),
                updated_at: "2024-01-01T00:00:00.000Z".to_string(),
            });
        }
        self
    }

    /// Inject the directories returned by
    /// [`MockWorkOSClient::list_directories`] for an organization.
    pub fn set_directories(&self, organization_id: &str, directories: Vec<Directory>) {
        self.state
            .write()
            .directories
            .insert(organization_id.to_string(), directories);
    }

    /// Inject the groups returned by
    /// [`MockWorkOSClient::list_directory_groups`] for a directory.
    pub fn set_directory_groups(&self, directory_id: &str, groups: Vec<DirectoryGroup>) {
        self.state
            .write()
            .directory_groups
            .insert(directory_id.to_string(), groups);
    }

    /// Inject the users returned by [`MockWorkOSClient::list_directory_users`]
    /// for a directory.
    pub fn set_directory_users(&self, directory_id: &str, users: Vec<DirectoryUser>) {
        self.state
            .write()
            .directory_users
            .insert(directory_id.to_string(), users);
    }

    /// Inject a user returned by [`MockWorkOSClient::get_directory_user`] for
    /// its id.
    pub fn set_directory_user(&self, user: DirectoryUser) {
        self.state
            .write()
            .directory_users_by_id
            .insert(user.id.clone(), user);
    }

    /// Inject a group returned by [`get_directory_group`] for its id.
    pub fn set_directory_group(&self, group: DirectoryGroup) {
        self.state
            .write()
            .directory_groups_by_id
            .insert(group.id.clone(), group);
    }

    /// Inject the groups returned by [`list_directory_groups_for_user`] for a
    /// directory user id.
    pub fn set_directory_groups_for_user(&self, user_id: &str, groups: Vec<DirectoryGroup>) {
        self.state
            .write()
            .directory_groups_by_user_id
            .insert(user_id.to_string(), groups);
    }

    /// Inject the event log served by [`list_events`], oldest first.
    pub fn set_events(&self, events: Vec<WorkOSEvent>) {
        self.state.write().events = events;
    }
}

#[async_trait]
impl WorkOSClient for MockWorkOSClient {
    async fn fetch_identities(&self, user_id: &str) -> anyhow::Result<Vec<WorkOSIdentity>> {
        if let Some(pipe_pos) = user_id.find('|') {
            let provider = &user_id[..pipe_pos];
            let idp_id = &user_id[pipe_pos + 1..];

            Ok(vec![WorkOSIdentity {
                provider: match provider {
                    "google-oauth2" => "GoogleOAuth",
                    "github" => "GithubOAuth",
                    "microsoft" => "MicrosoftOAuth",
                    _ => "Unknown",
                }
                .to_string(),
                idp_id: idp_id.to_string(),
            }])
        } else {
            Ok(vec![])
        }
    }

    async fn fetch_user(&self, _user_id: &str) -> anyhow::Result<WorkOSUser> {
        Ok(WorkOSUser {
            email: "test@example.com".to_string(),
            first_name: Some("Test".to_string()),
            last_name: Some("User".to_string()),
        })
    }

    async fn email_has_enrolled_mfa(&self, email: &str) -> anyhow::Result<bool> {
        Ok(self.state.read().mfa_emails.contains(&email.to_lowercase()))
    }

    async fn find_user_id_by_email(&self, email: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .state
            .read()
            .email_to_user_id
            .get(&email.to_lowercase())
            .cloned())
    }

    async fn delete_user(&self, _user_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn update_user_metadata(
        &self,
        _user_id: &str,
        _member_id: MemberId,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn update_user_email(&self, _user_id: &str, _email: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn create_organization(
        &self,
        name: &str,
        external_id: &str,
    ) -> anyhow::Result<WorkOSOrganizationResponse> {
        let mut state = self.state.write();
        // WorkOS rejects a second organization with the same `external_id`;
        // callers are expected to recover by looking the existing one up.
        if state
            .organizations
            .values()
            .any(|org| org.external_id.as_deref() == Some(external_id))
        {
            anyhow::bail!("an organization with external_id {external_id} already exists");
        }
        state.next_id += 1;
        let org = WorkOSOrganizationResponse {
            object: "organization".to_string(),
            id: format!("org_mock{}", state.next_id),
            name: name.to_string(),
            external_id: Some(external_id.to_string()),
            created_at: "2024-01-01T00:00:00.000Z".to_string(),
            updated_at: "2024-01-01T00:00:00.000Z".to_string(),
            domains: vec![],
        };
        state.organizations.insert(org.id.clone(), org.clone());
        Ok(org)
    }

    async fn get_organization(
        &self,
        external_id: &str,
    ) -> anyhow::Result<Option<WorkOSOrganizationResponse>> {
        Ok(self
            .state
            .read()
            .organizations
            .values()
            .find(|org| org.external_id.as_deref() == Some(external_id))
            .cloned())
    }

    async fn get_organization_by_id(
        &self,
        organization_id: &str,
    ) -> anyhow::Result<Option<WorkOSOrganizationResponse>> {
        let state = self.state.read();
        let domains = state
            .organization_domains
            .get(organization_id)
            .cloned()
            .unwrap_or_default();
        if let Some(org) = state.organizations.get(organization_id) {
            let mut org = org.clone();
            if !domains.is_empty() {
                org.domains = domains;
            }
            return Ok(Some(org));
        }
        // An id that names neither an organization nor a domain fixture is one
        // WorkOS does not have, and saying so is what lets a test cover the
        // callers that fall back when a recorded organization id goes stale.
        if domains.is_empty() {
            return Ok(None);
        }

        // A domain fixture alone conjures its organization, so a test that only
        // cares about domains does not have to create one.
        Ok(Some(WorkOSOrganizationResponse {
            object: "organization".to_string(),
            id: organization_id.to_string(),
            name: format!("Mock Organization {organization_id}"),
            external_id: Some(format!("external_{organization_id}")),
            created_at: "2024-01-01T00:00:00.000Z".to_string(),
            updated_at: "2024-01-01T00:00:00.000Z".to_string(),
            domains,
        }))
    }

    async fn update_organization(
        &self,
        organization_id: &str,
        name: Option<&str>,
        domain: Option<&str>,
    ) -> anyhow::Result<WorkOSOrganizationResponse> {
        let mut state = self.state.write();
        let org = state
            .organizations
            .get_mut(organization_id)
            .with_context(|| format!("no such organization {organization_id}"))?;
        if let Some(name) = name {
            org.name = name.to_string();
        }
        if let Some(domain) = domain {
            org.domains = vec![WorkOSOrganizationDomain {
                object: "organization_domain".to_string(),
                id: "org_domain_mock123".to_string(),
                domain: domain.to_string(),
                state: WorkOSDomainState::Pending,
            }];
        }
        org.updated_at = "2024-01-01T00:01:00.000Z".to_string();
        Ok(org.clone())
    }

    async fn delete_organization(&self, organization_id: &str) -> anyhow::Result<()> {
        let mut state = self.state.write();
        state.organizations.remove(organization_id);
        state.organization_domains.remove(organization_id);
        state.connections.remove(organization_id);
        state.directories.remove(organization_id);
        state.organization_memberships.retain(|_, memberships| {
            memberships.retain(|membership| membership.organization_id != organization_id);
            !memberships.is_empty()
        });
        Ok(())
    }

    async fn delete_organization_domain(&self, domain_id: &str) -> anyhow::Result<()> {
        let mut state = self.state.write();
        for domains in state.organization_domains.values_mut() {
            domains.retain(|domain| domain.id != domain_id);
        }
        // `update_organization` writes domains onto the organization itself,
        // so a domain can live in either place.
        for org in state.organizations.values_mut() {
            org.domains.retain(|domain| domain.id != domain_id);
        }
        Ok(())
    }

    async fn list_organization_memberships(
        &self,
        user_id: &str,
    ) -> anyhow::Result<Vec<WorkOSOrganizationMembershipResponse>> {
        Ok(self
            .state
            .read()
            .organization_memberships
            .get(user_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn create_membership(
        &self,
        user_id: &str,
        organization_id: &str,
        role_slug: &str,
    ) -> anyhow::Result<WorkOSOrganizationMembershipResponse> {
        Ok(WorkOSOrganizationMembershipResponse {
            object: "organization_membership".to_string(),
            id: "om_mock123".to_string(),
            user_id: user_id.to_string(),
            organization_id: organization_id.to_string(),
            role: WorkOSOrganizationRole {
                slug: role_slug.to_string(),
            },
            status: "active".to_string(),
            created_at: "2024-01-01T00:00:00.000Z".to_string(),
            updated_at: "2024-01-01T00:00:00.000Z".to_string(),
        })
    }

    async fn generate_portal_link(
        &self,
        organization_id: &str,
        intent: WorkOSPortalIntent,
    ) -> anyhow::Result<WorkOSPortalLinkResponse> {
        let intent_str = match intent {
            WorkOSPortalIntent::Sso => "sso",
            WorkOSPortalIntent::DomainVerification => "domain_verification",
            WorkOSPortalIntent::CertificateRenewal => "certificate_renewal",
            WorkOSPortalIntent::DirectorySync => "dsync",
        };
        Ok(WorkOSPortalLinkResponse {
            link: format!(
                "https://portal.workos.com/mock-portal-link?organization={organization_id}&intent={intent_str}"
            ),
        })
    }

    async fn list_sso_connections(
        &self,
        organization_id: &str,
    ) -> anyhow::Result<Vec<SSOConnection>> {
        Ok(self
            .state
            .read()
            .connections
            .get(organization_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn delete_sso_connection(&self, connection_id: &str) -> anyhow::Result<()> {
        for connections in self.state.write().connections.values_mut() {
            connections.retain(|c| c.id != connection_id);
        }
        Ok(())
    }

    async fn list_directories(&self, organization_id: &str) -> anyhow::Result<Vec<Directory>> {
        Ok(self
            .state
            .read()
            .directories
            .get(organization_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn delete_directory(&self, directory_id: &str) -> anyhow::Result<()> {
        let mut state = self.state.write();
        for directories in state.directories.values_mut() {
            directories.retain(|d| d.id != directory_id);
        }
        // The by-id lookups are keyed on the group or user rather than the
        // directory, so they outlive the directory unless its own entries are
        // taken out with it.
        let groups = state
            .directory_groups
            .remove(directory_id)
            .unwrap_or_default();
        for group in &groups {
            state.directory_groups_by_id.remove(&group.id);
        }
        let users = state
            .directory_users
            .remove(directory_id)
            .unwrap_or_default();
        for user in &users {
            state.directory_users_by_id.remove(&user.id);
            state.directory_groups_by_user_id.remove(&user.id);
        }
        Ok(())
    }

    async fn list_directory_groups(
        &self,
        directory_id: &str,
    ) -> anyhow::Result<Vec<DirectoryGroup>> {
        Ok(self
            .state
            .read()
            .directory_groups
            .get(directory_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn list_directory_users(&self, directory_id: &str) -> anyhow::Result<Vec<DirectoryUser>> {
        Ok(self
            .state
            .read()
            .directory_users
            .get(directory_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn get_directory_user(
        &self,
        directory_user_id: &str,
    ) -> anyhow::Result<Option<DirectoryUser>> {
        Ok(self
            .state
            .read()
            .directory_users_by_id
            .get(directory_user_id)
            .cloned())
    }

    async fn get_directory_group(
        &self,
        directory_group_id: &str,
    ) -> anyhow::Result<Option<DirectoryGroup>> {
        Ok(self
            .state
            .read()
            .directory_groups_by_id
            .get(directory_group_id)
            .cloned())
    }

    async fn list_directory_groups_for_user(
        &self,
        directory_user_id: &str,
    ) -> anyhow::Result<Vec<DirectoryGroup>> {
        Ok(self
            .state
            .read()
            .directory_groups_by_user_id
            .get(directory_user_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn list_events(
        &self,
        event_types: &[&str],
        after: Option<&str>,
        limit: u32,
    ) -> anyhow::Result<Vec<WorkOSEvent>> {
        // Matches the real client, which rejects this before issuing a request
        // rather than letting an empty filter mean "every type".
        if event_types.is_empty() {
            anyhow::bail!("Refusing to list events with no event types");
        }
        let state = self.state.read();
        let start = match after {
            Some(after) => match state.events.iter().position(|e| e.id == after) {
                Some(index) => index + 1,
                // WorkOS doesn't document what it does with a cursor outside
                // its retention window, so fail loudly rather than pick a
                // behaviour: a test that loses its cursor should say so, not
                // silently replay the whole log.
                None => anyhow::bail!("Unknown event cursor {after}"),
            },
            None => 0,
        };
        Ok(state.events[start..]
            .iter()
            .filter(|e| event_types.contains(&e.event.as_str()))
            .take(limit as usize)
            .cloned()
            .collect())
    }
}

// Separate implementation for WorkOS Platform API
pub struct RealWorkOSPlatformClient<F, E>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    client_id: String,
    client_secret: String,
    access_token: Arc<RwLock<Option<String>>>,
    http_client: Box<dyn Fn(HttpRequest) -> F + Send + Sync + 'static>,
}

impl<F, E> RealWorkOSPlatformClient<F, E>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    pub fn new(
        client_id: String,
        client_secret: String,
        http_client: impl Fn(HttpRequest) -> F + Send + Sync + 'static,
    ) -> Self {
        Self {
            client_id,
            client_secret,
            access_token: Arc::new(RwLock::new(None)),
            http_client: Box::new(http_client),
        }
    }

    async fn update_oauth_token(&self) -> anyhow::Result<String> {
        #[derive(Serialize)]
        struct TokenRequest {
            client_id: String,
            client_secret: String,
            grant_type: String,
        }

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            #[allow(unused)]
            expires_in: u64,
            #[allow(unused)]
            token_type: String,
        }

        let request_body = TokenRequest {
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            grant_type: "client_credentials".to_string(),
        };

        let request = http::Request::builder()
            .uri("https://signin.workos.com/oauth2/token")
            .method(http::Method::POST)
            .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
            .header(http::header::ACCEPT, APPLICATION_JSON)
            .body(serde_json::to_vec(&request_body)?)?;

        let response = timeout(WORKOS_API_TIMEOUT, (self.http_client)(request))
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "WorkOS OAuth token request timed out after {}s",
                    WORKOS_API_TIMEOUT.as_secs()
                )
            })?
            .map_err(|e| anyhow::anyhow!("Could not get WorkOS OAuth token: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let response_body = response.into_body();
            anyhow::bail!(
                "Failed to get WorkOS OAuth token: {} {}",
                status,
                String::from_utf8_lossy(&response_body)
            );
        }

        let response_body = response.into_body();
        let token_response: TokenResponse =
            serde_json::from_slice(&response_body).with_context(|| {
                format!(
                    "Invalid WorkOS OAuth token response: {}",
                    String::from_utf8_lossy(&response_body)
                )
            })?;

        // Overwrite any existing token
        *self.access_token.write() = Some(token_response.access_token.clone());

        Ok(token_response.access_token)
    }

    async fn get_or_refresh_token(&self) -> anyhow::Result<String> {
        if let Some(token) = self.access_token.read().clone() {
            return Ok(token);
        }
        self.update_oauth_token().await
    }
}

#[async_trait]
impl<F, E> WorkOSPlatformClient for RealWorkOSPlatformClient<F, E>
where
    F: Future<Output = Result<HttpResponse, E>> + Send + 'static,
    E: std::error::Error + 'static + Send + Sync,
{
    async fn create_team(
        &self,
        admin_email: &str,
        team_name: &str,
    ) -> anyhow::Result<WorkOSTeamResponse> {
        let token = self.get_or_refresh_token().await?;
        let result = create_workos_team(&token, admin_email, team_name, &*self.http_client).await;

        match result {
            Err(e) if is_auth_error(&e) => {
                // Token expired, refresh and try once more
                let new_token = self.update_oauth_token().await?;
                create_workos_team(&new_token, admin_email, team_name, &*self.http_client).await
            },
            _ => result,
        }
    }

    async fn get_team(&self, workos_team_id: &str) -> anyhow::Result<WorkOSTeamResponse> {
        let token = self.get_or_refresh_token().await?;
        let result = get_workos_team(&token, workos_team_id, &*self.http_client).await;

        match result {
            Err(e) if is_auth_error(&e) => {
                // Token expired, refresh and try once more
                let new_token = self.update_oauth_token().await?;
                get_workos_team(&new_token, workos_team_id, &*self.http_client).await
            },
            _ => result,
        }
    }

    async fn create_environment(
        &self,
        workos_team_id: &str,
        environment_name: &str,
        is_production: Option<bool>,
    ) -> anyhow::Result<WorkOSEnvironmentResponse> {
        let token = self.get_or_refresh_token().await?;
        let result = create_workos_environment(
            &token,
            workos_team_id,
            environment_name,
            is_production,
            &*self.http_client,
        )
        .await;

        match result {
            Err(e) if is_auth_error(&e) => {
                // Token expired, refresh and try once more
                let new_token = self.update_oauth_token().await?;
                create_workos_environment(
                    &new_token,
                    workos_team_id,
                    environment_name,
                    is_production,
                    &*self.http_client,
                )
                .await
            },
            _ => result,
        }
    }

    async fn get_environment(
        &self,
        workos_team_id: &str,
        environment_id: &str,
    ) -> anyhow::Result<WorkOSEnvironmentResponse> {
        let token = self.get_or_refresh_token().await?;
        let result =
            get_workos_environment(&token, workos_team_id, environment_id, &*self.http_client)
                .await;

        match result {
            Err(e) if is_auth_error(&e) => {
                // Token expired, refresh and try once more
                let new_token = self.update_oauth_token().await?;
                get_workos_environment(
                    &new_token,
                    workos_team_id,
                    environment_id,
                    &*self.http_client,
                )
                .await
            },
            _ => result,
        }
    }

    async fn create_api_key(
        &self,
        workos_team_id: &str,
        environment_id: &str,
        key_name: &str,
    ) -> anyhow::Result<WorkOSAPIKeyResponse> {
        let token = self.get_or_refresh_token().await?;
        let result = create_workos_api_key(
            &token,
            workos_team_id,
            environment_id,
            key_name,
            &*self.http_client,
        )
        .await;

        match result {
            Err(e) if is_auth_error(&e) => {
                // Token expired, refresh and try once more
                let new_token = self.update_oauth_token().await?;
                create_workos_api_key(
                    &new_token,
                    workos_team_id,
                    environment_id,
                    key_name,
                    &*self.http_client,
                )
                .await
            },
            _ => result,
        }
    }

    async fn invite_team_member(
        &self,
        workos_team_id: &str,
        email: &str,
        role_slug: &str,
    ) -> anyhow::Result<WorkOSTeamInvitationResponse> {
        let token = self.get_or_refresh_token().await?;
        let result =
            invite_workos_team_member(&token, workos_team_id, email, role_slug, &*self.http_client)
                .await;

        match result {
            Err(e) if is_auth_error(&e) => {
                // Token expired, refresh and try once more
                let new_token = self.update_oauth_token().await?;
                invite_workos_team_member(
                    &new_token,
                    workos_team_id,
                    email,
                    role_slug,
                    &*self.http_client,
                )
                .await
            },
            _ => result,
        }
    }

    async fn delete_environment(
        &self,
        workos_team_id: &str,
        environment_id: &str,
    ) -> anyhow::Result<()> {
        let token = self.get_or_refresh_token().await?;
        let result =
            delete_workos_environment(&token, workos_team_id, environment_id, &*self.http_client)
                .await;

        match result {
            Err(e) if is_auth_error(&e) => {
                // Token expired, refresh and try once more
                let new_token = self.update_oauth_token().await?;
                delete_workos_environment(
                    &new_token,
                    workos_team_id,
                    environment_id,
                    &*self.http_client,
                )
                .await
            },
            _ => result,
        }
    }
}

pub struct MockWorkOSPlatformClient;

impl Default for MockWorkOSPlatformClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockWorkOSPlatformClient {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl WorkOSPlatformClient for MockWorkOSPlatformClient {
    async fn create_team(
        &self,
        _admin_email: &str,
        team_name: &str,
    ) -> anyhow::Result<WorkOSTeamResponse> {
        Ok(WorkOSTeamResponse {
            object: "team".to_string(),
            id: "team_mock123".to_string(),
            name: team_name.to_string(),
            production_state: Some(WorkOSProductionState::Active),
            created_at: "2024-01-01T00:00:00.000Z".to_string(),
            updated_at: "2024-01-01T00:00:00.000Z".to_string(),
        })
    }

    async fn get_team(&self, workos_team_id: &str) -> anyhow::Result<WorkOSTeamResponse> {
        Ok(WorkOSTeamResponse {
            object: "team".to_string(),
            id: workos_team_id.to_string(),
            name: "Mock Team".to_string(),
            production_state: Some(WorkOSProductionState::Active),
            created_at: "2024-01-01T00:00:00.000Z".to_string(),
            updated_at: "2024-01-01T00:00:00.000Z".to_string(),
        })
    }

    async fn create_environment(
        &self,
        _workos_team_id: &str,
        environment_name: &str,
        _is_production: Option<bool>,
    ) -> anyhow::Result<WorkOSEnvironmentResponse> {
        Ok(WorkOSEnvironmentResponse {
            object: "environment".to_string(),
            id: "env_mock123".to_string(),
            name: environment_name.to_string(),
            client_id: "client_mock123".to_string(),
        })
    }

    async fn get_environment(
        &self,
        _workos_team_id: &str,
        environment_id: &str,
    ) -> anyhow::Result<WorkOSEnvironmentResponse> {
        Ok(WorkOSEnvironmentResponse {
            object: "environment".to_string(),
            id: environment_id.to_string(),
            name: "Mock Environment".to_string(),
            client_id: "client_mock123".to_string(),
        })
    }

    async fn create_api_key(
        &self,
        _workos_team_id: &str,
        _environment_id: &str,
        key_name: &str,
    ) -> anyhow::Result<WorkOSAPIKeyResponse> {
        Ok(WorkOSAPIKeyResponse {
            object: "api_key".to_string(),
            id: "key_mock123".to_string(),
            name: key_name.to_string(),
            expires_at: None,
            value: "sk_test_mock_key_value".to_string(),
            created_at: "2024-01-01T00:00:00.000Z".to_string(),
            updated_at: "2024-01-01T00:00:00.000Z".to_string(),
        })
    }

    async fn invite_team_member(
        &self,
        _workos_team_id: &str,
        email: &str,
        role_slug: &str,
    ) -> anyhow::Result<WorkOSTeamInvitationResponse> {
        Ok(WorkOSTeamInvitationResponse {
            object: "workos_team_invitation".to_string(),
            email: email.to_string(),
            role_slug: role_slug.to_string(),
        })
    }

    async fn delete_environment(
        &self,
        _workos_team_id: &str,
        _environment_id: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

pub async fn fetch_workos_identities<F, E>(
    api_key: &str,
    user_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Vec<WorkOSIdentity>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!("https://api.workos.com/user_management/users/{user_id}/identities");

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::GET)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .body(vec![])?;

    let response = http_client(request)
        .await
        .map_err(|e| anyhow::anyhow!("Could not fetch WorkOS identities: {}", e))?;

    if response.status() != http::StatusCode::OK {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "list identities",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let identities: Vec<WorkOSIdentity> =
        serde_json::from_slice(&response_body).with_context(|| {
            format!(
                "Invalid WorkOS identities response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(identities)
}

pub async fn fetch_workos_user<F, E>(
    api_key: &str,
    user_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<WorkOSUser>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!("https://api.workos.com/user_management/users/{user_id}");

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::GET)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .body(vec![])?;

    let response = http_client(request)
        .await
        .map_err(|e| anyhow::anyhow!("Could not fetch WorkOS user: {}", e))?;

    if response.status() != http::StatusCode::OK {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new("get user", status, &response_body));
    }

    let response_body = response.into_body();
    let user: WorkOSUser = serde_json::from_slice(&response_body).with_context(|| {
        format!(
            "Invalid WorkOS user response: {}",
            String::from_utf8_lossy(&response_body)
        )
    })?;

    Ok(user)
}

/// Looks up the WorkOS user id for an email address, if a user with that email
/// exists in the environment. Emails are unique per environment, so at most one
/// user is returned.
async fn find_workos_user_id_by_email<F, E>(
    api_key: &str,
    email: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Option<String>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let mut url = url::Url::parse("https://api.workos.com/user_management/users")
        .context("Invalid WorkOS users URL")?;
    url.query_pairs_mut().append_pair("email", email);

    let request = http::Request::builder()
        .uri(url.as_str())
        .method(http::Method::GET)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .body(vec![])?;

    let response = http_client(request)
        .await
        .map_err(|e| anyhow::anyhow!("Could not list WorkOS users: {}", e))?;

    if response.status() != http::StatusCode::OK {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new("list users", status, &response_body));
    }

    let response_body = response.into_body();
    let list: WorkOSListResponse<WorkOSUserId> = serde_json::from_slice(&response_body)
        .with_context(|| {
            format!(
                "Invalid WorkOS users response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(list.data.into_iter().next().map(|u| u.id))
}

/// Lists the MFA authentication factors enrolled for a WorkOS user.
async fn list_workos_auth_factors<F, E>(
    api_key: &str,
    user_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Vec<WorkOSAuthFactor>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!("https://api.workos.com/user_management/users/{user_id}/auth_factors");

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::GET)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .body(vec![])?;

    let response = http_client(request)
        .await
        .map_err(|e| anyhow::anyhow!("Could not list WorkOS auth factors: {}", e))?;

    if response.status() != http::StatusCode::OK {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "list auth factors",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let list: WorkOSListResponse<WorkOSAuthFactor> = serde_json::from_slice(&response_body)
        .with_context(|| {
            format!(
                "Invalid WorkOS auth factors response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(list.data)
}

/// Whether the WorkOS user with this email has any enrolled MFA factors.
/// Returns `false` when no WorkOS user exists for the email (no user ⟹ no
/// factors could have been enrolled).
async fn email_has_enrolled_mfa<F, E>(
    api_key: &str,
    email: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<bool>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let Some(user_id) = find_workos_user_id_by_email(api_key, email, http_client).await? else {
        return Ok(false);
    };
    let factors = list_workos_auth_factors(api_key, &user_id, http_client).await?;
    Ok(!factors.is_empty())
}

pub async fn delete_workos_user<F, E>(
    api_key: &str,
    user_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<()>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!("https://api.workos.com/user_management/users/{user_id}");

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::DELETE)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(vec![])?;

    let response = http_client(request)
        .await
        .map_err(|e| anyhow::anyhow!("Could not delete WorkOS user: {}", e))?;

    if response.status() != http::StatusCode::OK {
        if response.status() == http::StatusCode::NOT_FOUND {
            return Ok(());
        }
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new("delete user", status, &response_body));
    }

    Ok(())
}

#[derive(Serialize)]
struct WorkOSUserMetadata {
    convex_member_id: String,
}

pub async fn update_workos_user_metadata<F, E>(
    api_key: &str,
    user_id: &str,
    member_id: MemberId,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<()>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!("https://api.workos.com/user_management/users/{user_id}");

    let metadata = WorkOSUserMetadata {
        convex_member_id: member_id.to_string(),
    };

    let mut update_data = HashMap::new();
    update_data.insert("metadata", metadata);

    let request_body = serde_json::to_vec(&update_data)
        .map_err(|e| anyhow::anyhow!("Failed to serialize update data: {}", e))?;

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::PUT)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .body(request_body)?;

    let response = http_client(request)
        .await
        .map_err(|e| anyhow::anyhow!("Could not update WorkOS user metadata: {}", e))?;

    if response.status() != http::StatusCode::OK {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new("update user", status, &response_body));
    }

    Ok(())
}

#[derive(Serialize)]
struct WorkOSUserEmailUpdate<'a> {
    email: &'a str,
    /// WorkOS resets `email_verified` to false whenever the email changes, so
    /// it has to be reasserted here. Callers only reach this after Big Brain
    /// has verified the address through its own code flow.
    email_verified: bool,
}

pub async fn update_workos_user_email<F, E>(
    api_key: &str,
    user_id: &str,
    email: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<()>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!("https://api.workos.com/user_management/users/{user_id}");

    let request_body = serde_json::to_vec(&WorkOSUserEmailUpdate {
        email,
        email_verified: true,
    })
    .context("Failed to serialize WorkOS user email update")?;

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::PUT)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .body(request_body)?;

    let response = http_client(request)
        .await
        .context("Could not update WorkOS user email")?;

    if response.status() != http::StatusCode::OK {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "update user email",
            status,
            &response_body
        ));
    }

    Ok(())
}

pub async fn create_workos_team<F, E>(
    api_key: &str,
    admin_email: &str,
    team_name: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<WorkOSTeamResponse>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    #[derive(Serialize)]
    struct CreateTeamRequest {
        admin_email: String,
        name: String,
    }

    let request_body = CreateTeamRequest {
        admin_email: admin_email.to_string(),
        name: team_name.to_string(),
    };

    let request = http::Request::builder()
        .uri("https://api.workos.com/platform/teams")
        .method(http::Method::POST)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(serde_json::to_vec(&request_body)?)?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not create WorkOS team: {}", e))?;

    if response.status() == http::StatusCode::CONFLICT {
        let response_body = response.into_body();

        if let Ok(error_response) = serde_json::from_slice::<WorkOSErrorResponse>(&response_body)
            && error_response.code == Some("user_already_exists".to_string())
        {
            // This will be special-cased in scripts.
            anyhow::bail!(ErrorMetadata::bad_request(
                "WorkosAccountAlreadyExistsWithThisEmail",
                format!("A WorkOS account already exists with the email: {admin_email}")
            ));
        }

        let status = http::StatusCode::CONFLICT;
        anyhow::bail!(WorkOSApiError::new(
            "create team (conflict)",
            status,
            &response_body
        ));
    }

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new("create team", status, &response_body));
    }

    let response_body = response.into_body();
    let team: WorkOSTeamResponse = serde_json::from_slice(&response_body).with_context(|| {
        format!(
            "Invalid WorkOS team response: {}",
            String::from_utf8_lossy(&response_body)
        )
    })?;

    Ok(team)
}

pub async fn get_workos_team<F, E>(
    api_key: &str,
    workos_team_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<WorkOSTeamResponse>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!("https://api.workos.com/platform/teams/{workos_team_id}");

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::GET)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(vec![])?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not get WorkOS team: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new("get team", status, &response_body));
    }

    let response_body = response.into_body();
    let team: WorkOSTeamResponse = serde_json::from_slice(&response_body).with_context(|| {
        format!(
            "Invalid WorkOS team response: {}",
            String::from_utf8_lossy(&response_body)
        )
    })?;

    Ok(team)
}

pub async fn invite_workos_team_member<F, E>(
    api_key: &str,
    workos_team_id: &str,
    email: &str,
    role_slug: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<WorkOSTeamInvitationResponse>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    #[derive(Serialize)]
    struct InviteTeamMemberRequest {
        email: String,
        role_slug: String,
    }

    let request_body = InviteTeamMemberRequest {
        email: email.to_string(),
        role_slug: role_slug.to_string(),
    };

    let url = format!("https://api.workos.com/platform/teams/{workos_team_id}/invitations");

    // WorkOS Platform API invitation responses as of Jan 2 2025:
    // - Already invited (pending): 500 Internal Server Error with
    //   {"message":"Internal Server Error"}
    // - Already member of another team: 400 Bad Request with {"message":"User
    //   'email' is already a member of another team.","error":"Bad Request"}
    // - Already member of this team: 400 Bad Request with {"message":"User 'email'
    //   is already a member of this team.","error":"Bad Request"}
    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::POST)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(serde_json::to_vec(&request_body)?)?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not invite team member: {}", e))?;

    // Handle 400 Bad Request - WorkOS returns this for "already a member" scenarios
    if response.status() == http::StatusCode::BAD_REQUEST {
        let status = response.status();
        let response_body = response.into_body();

        if let Ok(error_response) = serde_json::from_slice::<WorkOSErrorResponse>(&response_body) {
            let message = &error_response.message;

            // Parse the message to determine the specific scenario
            if message.contains("already a member of this team") {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "WorkosUserAlreadyInThisTeam",
                    format!("The email {email} is already a member of this WorkOS team")
                ));
            }
            if message.contains("already a member of another team") {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "WorkosUserAlreadyInWorkspace",
                    format!("The email {email} is already a member of another WorkOS workspace")
                ));
            }
        }

        // Unknown 400 error - return generic error
        anyhow::bail!(WorkOSApiError::new(
            "invite team member",
            status,
            &response_body
        ));
    }

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.into_body();

        // TODO: WorkOS returns 500 Internal Server Error with no details when
        // a WorkOS users is already invited. We'll update once they fix this.
        if status == http::StatusCode::INTERNAL_SERVER_ERROR {
            let response_str = String::from_utf8_lossy(&response_body);
            if response_str.contains("Internal Server Error") {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "WorkosUserAlreadyInvited",
                    format!("The email {email} may have already been invited to this WorkOS team")
                ));
            }
        }

        anyhow::bail!(WorkOSApiError::new(
            "invite team member",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let invitation: WorkOSTeamInvitationResponse = serde_json::from_slice(&response_body)
        .with_context(|| {
            format!(
                "Invalid WorkOS invitation response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(invitation)
}

pub async fn delete_workos_environment<F, E>(
    api_key: &str,
    workos_team_id: &str,
    environment_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<()>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!(
        "https://api.workos.com/platform/teams/{workos_team_id}/environments/{environment_id}"
    );

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::DELETE)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(vec![])?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not delete environment: {}", e))?;

    // 204 No Content is the success response for delete
    if response.status() == http::StatusCode::NO_CONTENT {
        return Ok(());
    }

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "delete environment",
            status,
            &response_body
        ));
    }

    Ok(())
}

pub async fn create_workos_environment<F, E>(
    api_key: &str,
    workos_team_id: &str,
    environment_name: &str,
    is_production: Option<bool>,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<WorkOSEnvironmentResponse>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    #[derive(Serialize)]
    struct CreateEnvironmentRequest {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        production: Option<bool>,
    }

    let request_body = CreateEnvironmentRequest {
        name: environment_name.to_string(),
        production: is_production,
    };

    let url = format!("https://api.workos.com/platform/teams/{workos_team_id}/environments",);

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::POST)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(serde_json::to_vec(&request_body)?)?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not create WorkOS environment: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.into_body();

        if status == http::StatusCode::FORBIDDEN {
            if let Ok(error_response) =
                serde_json::from_slice::<WorkOSErrorResponse>(&response_body)
                && error_response.code == Some("platform_not_authorized".to_string())
            {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "WorkOSPlatformNotAuthorized",
                    format!("Convex is not authorized to create environments for this WorkOS team, {url}. See https://docs.convex.dev/auth/authkit/troubleshooting#platform-not-authorized for more information.")
                ));
            }

            anyhow::bail!(WorkOSApiError::new(
                "create environment (forbidden) with unexpected error response code",
                status,
                &response_body
            ));
        }

        anyhow::bail!(WorkOSApiError::new(
            "create environment",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let environment: WorkOSEnvironmentResponse = serde_json::from_slice(&response_body)
        .with_context(|| {
            format!(
                "Invalid WorkOS environment response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(environment)
}

pub async fn get_workos_environment<F, E>(
    api_key: &str,
    workos_team_id: &str,
    environment_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<WorkOSEnvironmentResponse>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!(
        "https://api.workos.com/platform/teams/{workos_team_id}/environments/{environment_id}"
    );

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::GET)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(vec![])?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not get WorkOS environment: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "get environment",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let environment: WorkOSEnvironmentResponse = serde_json::from_slice(&response_body)
        .with_context(|| {
            format!(
                "Invalid WorkOS environment response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(environment)
}

pub async fn create_workos_api_key<F, E>(
    api_key: &str,
    workos_team_id: &str,
    environment_id: &str,
    key_name: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<WorkOSAPIKeyResponse>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    #[derive(Serialize)]
    struct CreateAPIKeyRequest {
        name: String,
        expires_at: Option<String>,
    }

    let request_body = CreateAPIKeyRequest {
        name: key_name.to_string(),
        expires_at: None,
    };

    let url = format!(
        "https://api.workos.com/platform/teams/{workos_team_id}/environments/{environment_id}/api_keys",
    );

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::POST)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(serde_json::to_vec(&request_body)?)?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not create WorkOS API key: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "create API key",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let api_key_response: WorkOSAPIKeyResponse = serde_json::from_slice(&response_body)
        .with_context(|| {
            format!(
                "Invalid WorkOS API key response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(api_key_response)
}

pub async fn create_workos_organization<F, E>(
    api_key: &str,
    name: &str,
    external_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<WorkOSOrganizationResponse>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    #[derive(Serialize)]
    struct DomainData {
        domain: String,
        state: String,
    }

    #[derive(Serialize)]
    struct CreateOrganizationRequest {
        name: String,
        external_id: String,
        domain_data: Vec<DomainData>,
    }

    // Always create organization without domains - domains are added through WorkOS
    // portal
    let request_body = CreateOrganizationRequest {
        name: name.to_string(),
        external_id: external_id.to_string(),
        domain_data: vec![],
    };

    let url = "https://api.workos.com/organizations";

    let request = http::Request::builder()
        .uri(url)
        .method(http::Method::POST)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(serde_json::to_vec(&request_body)?)?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not create WorkOS organization: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "create organization",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let organization: WorkOSOrganizationResponse = serde_json::from_slice(&response_body)
        .with_context(|| {
            format!(
                "Invalid WorkOS organization response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(organization)
}

pub async fn get_workos_organization_by_external_id<F, E>(
    api_key: &str,
    external_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Option<WorkOSOrganizationResponse>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!("https://api.workos.com/organizations/external_id/{external_id}");

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::GET)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(vec![])?;

    let response = http_client(request)
        .await
        .map_err(|e| anyhow::anyhow!("Could not fetch WorkOS organization: {}", e))?;

    if response.status() == http::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    if response.status() != http::StatusCode::OK {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "get organization by external_id",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let organization: WorkOSOrganizationResponse = serde_json::from_slice(&response_body)
        .with_context(|| {
            format!(
                "Invalid WorkOS organization response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(Some(organization))
}

pub async fn get_workos_organization_by_id<F, E>(
    api_key: &str,
    organization_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Option<WorkOSOrganizationResponse>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!("https://api.workos.com/organizations/{organization_id}");

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::GET)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(vec![])?;

    let response = http_client(request)
        .await
        .map_err(|e| anyhow::anyhow!("Could not fetch WorkOS organization: {}", e))?;

    if response.status() == http::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    if response.status() != http::StatusCode::OK {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "get organization",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let organization: WorkOSOrganizationResponse = serde_json::from_slice(&response_body)
        .with_context(|| {
            format!(
                "Invalid WorkOS organization response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(Some(organization))
}

pub async fn update_workos_organization<F, E>(
    api_key: &str,
    organization_id: &str,
    name: Option<&str>,
    domain: Option<&str>,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<WorkOSOrganizationResponse>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    #[derive(Serialize)]
    struct DomainData {
        domain: String,
        state: String,
    }

    #[derive(Serialize)]
    struct UpdateOrganizationRequest {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        domain_data: Option<Vec<DomainData>>,
    }

    let request_body = UpdateOrganizationRequest {
        name: name.map(|n| n.to_string()),
        domain_data: domain.map(|d| {
            vec![DomainData {
                domain: d.to_string(),
                state: "pending".to_string(),
            }]
        }),
    };

    let url = format!("https://api.workos.com/organizations/{organization_id}");

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::PUT)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(serde_json::to_vec(&request_body)?)?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not update WorkOS organization: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "update organization",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let organization: WorkOSOrganizationResponse = serde_json::from_slice(&response_body)
        .with_context(|| {
            format!(
                "Invalid WorkOS organization response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(organization)
}

pub async fn delete_workos_organization<F, E>(
    api_key: &str,
    organization_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<()>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    delete_workos_resource(
        api_key,
        &format!("https://api.workos.com/organizations/{organization_id}"),
        "delete organization",
        http_client,
    )
    .await
}

/// `DELETE`s a WorkOS resource, treating a 404 as success so callers are
/// idempotent. `operation` names the call in error messages.
async fn delete_workos_resource<F, E>(
    api_key: &str,
    url: &str,
    operation: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<()>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let request = http::Request::builder()
        .uri(url)
        .method(http::Method::DELETE)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(vec![])?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not {operation}: {e}"))?;

    let status = response.status();
    if status.is_success() || status == http::StatusCode::NOT_FOUND {
        return Ok(());
    }

    let response_body = response.into_body();
    anyhow::bail!(WorkOSApiError::new(operation, status, &response_body));
}

pub async fn create_workos_membership<F, E>(
    api_key: &str,
    user_id: &str,
    organization_id: &str,
    role_slug: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<WorkOSOrganizationMembershipResponse>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    #[derive(Serialize)]
    struct CreateMembershipRequest {
        user_id: String,
        organization_id: String,
        role_slug: String,
    }

    let request_body = CreateMembershipRequest {
        user_id: user_id.to_string(),
        organization_id: organization_id.to_string(),
        role_slug: role_slug.to_string(),
    };

    let url = "https://api.workos.com/user_management/organization_memberships";

    let request = http::Request::builder()
        .uri(url)
        .method(http::Method::POST)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(serde_json::to_vec(&request_body)?)?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not create WorkOS membership: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "create membership",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let membership: WorkOSOrganizationMembershipResponse = serde_json::from_slice(&response_body)
        .with_context(|| {
        format!(
            "Invalid WorkOS membership response: {}",
            String::from_utf8_lossy(&response_body)
        )
    })?;

    Ok(membership)
}

pub async fn generate_workos_portal_link<F, E>(
    api_key: &str,
    organization_id: &str,
    intent: WorkOSPortalIntent,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<WorkOSPortalLinkResponse>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    #[derive(Serialize)]
    struct GeneratePortalLinkRequest {
        intent: WorkOSPortalIntent,
        organization: String,
    }

    let request_body = GeneratePortalLinkRequest {
        intent,
        organization: organization_id.to_string(),
    };

    let url = "https://api.workos.com/portal/generate_link";

    let request = http::Request::builder()
        .uri(url)
        .method(http::Method::POST)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .body(serde_json::to_vec(&request_body)?)?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not generate WorkOS portal link: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.into_body();

        // Handle 400 Bad Request specially to forward the error message
        if status == http::StatusCode::BAD_REQUEST
            && let Ok(error_response) =
                serde_json::from_slice::<WorkOSErrorResponse>(&response_body)
        {
            return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                "WorkOSPortalLinkError",
                error_response.message
            )));
        }

        anyhow::bail!(WorkOSApiError::new(
            "generate portal link",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let portal_link: WorkOSPortalLinkResponse = serde_json::from_slice(&response_body)
        .with_context(|| {
            format!(
                "Invalid WorkOS portal link response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(portal_link)
}

/// Fetches every page of a WorkOS list endpoint, following the
/// `list_metadata.after` cursor until it is exhausted. `query` holds the fixed
/// query parameters; `after` and `limit` are appended per request.
async fn list_workos_paginated<T, F, E>(
    api_key: &str,
    base_url: &str,
    query: &[(&str, &str)],
    operation: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    // WorkOS caps `limit` at 100 for Directory Sync list endpoints.
    const PAGE_LIMIT: &str = "100";

    let mut results = Vec::new();
    let mut after: Option<String> = None;

    loop {
        let mut url =
            url::Url::parse(base_url).with_context(|| format!("Invalid WorkOS URL: {base_url}"))?;
        {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
            pairs.append_pair("limit", PAGE_LIMIT);
            if let Some(after) = &after {
                pairs.append_pair("after", after);
            }
        }

        let request = http::Request::builder()
            .uri(url.as_str())
            .method(http::Method::GET)
            .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
            .header(http::header::ACCEPT, APPLICATION_JSON)
            .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
            .body(vec![])?;

        let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "WorkOS API call timed out after {}s",
                    WORKOS_API_TIMEOUT.as_secs()
                )
            })?
            .map_err(|e| anyhow::anyhow!("Could not {operation}: {e}"))?;

        if response.status() != http::StatusCode::OK {
            let status = response.status();
            let response_body = response.into_body();
            anyhow::bail!(WorkOSApiError::new(operation, status, &response_body));
        }

        let response_body = response.into_body();
        let page: WorkOSListResponse<T> =
            serde_json::from_slice(&response_body).with_context(|| {
                format!(
                    "Invalid WorkOS {operation} response: {}",
                    String::from_utf8_lossy(&response_body)
                )
            })?;
        results.extend(page.data);

        match page.list_metadata.after {
            Some(next) if !next.is_empty() => after = Some(next),
            _ => break,
        }
    }

    Ok(results)
}

/// Lists all SSO connections for an organization, following pagination.
pub async fn list_workos_sso_connections<F, E>(
    api_key: &str,
    organization_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Vec<SSOConnection>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    list_workos_paginated(
        api_key,
        "https://api.workos.com/connections",
        &[("organization_id", organization_id)],
        "list SSO connections",
        http_client,
    )
    .await
}

/// Lists all Directory Sync directories for an organization, following
/// pagination.
pub async fn list_workos_directories<F, E>(
    api_key: &str,
    organization_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Vec<Directory>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    list_workos_paginated(
        api_key,
        "https://api.workos.com/directories",
        &[("organization_id", organization_id)],
        "list directories",
        http_client,
    )
    .await
}

/// Lists all groups within a Directory Sync directory, following pagination.
pub async fn list_workos_directory_groups<F, E>(
    api_key: &str,
    directory_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Vec<DirectoryGroup>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    list_workos_paginated(
        api_key,
        "https://api.workos.com/directory_groups",
        &[("directory", directory_id)],
        "list directory groups",
        http_client,
    )
    .await
}

/// Lists the groups a directory user currently belongs to, following
/// pagination.
pub async fn list_workos_directory_groups_for_user<F, E>(
    api_key: &str,
    directory_user_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Vec<DirectoryGroup>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    list_workos_paginated(
        api_key,
        "https://api.workos.com/directory_groups",
        &[("user", directory_user_id)],
        "list directory groups for user",
        http_client,
    )
    .await
}

pub async fn list_workos_organization_memberships<F, E>(
    api_key: &str,
    user_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Vec<WorkOSOrganizationMembershipResponse>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    list_workos_paginated(
        api_key,
        "https://api.workos.com/user_management/organization_memberships",
        &[("user_id", user_id)],
        "list organization memberships",
        http_client,
    )
    .await
}

/// Lists all users within a Directory Sync directory, following pagination.
pub async fn list_workos_directory_users<F, E>(
    api_key: &str,
    directory_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Vec<DirectoryUser>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    list_workos_paginated(
        api_key,
        "https://api.workos.com/directory_users",
        &[("directory", directory_id)],
        "list directory users",
        http_client,
    )
    .await
}

/// Returns `Ok(None)` when the user no longer exists — deletion between an
/// event and this fetch is normal, not exceptional.
pub async fn get_workos_directory_user<F, E>(
    api_key: &str,
    directory_user_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Option<DirectoryUser>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!("https://api.workos.com/directory_users/{directory_user_id}");

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::GET)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .body(vec![])?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not fetch directory user: {}", e))?;

    if response.status() == http::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    if response.status() != http::StatusCode::OK {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "get directory user",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let user: DirectoryUser = serde_json::from_slice(&response_body).with_context(|| {
        format!(
            "Invalid WorkOS directory user response: {}",
            String::from_utf8_lossy(&response_body)
        )
    })?;

    Ok(Some(user))
}

/// Fetches a single directory group by id. Returns `Ok(None)` when the group
/// no longer exists — deletion between an event and this fetch is normal, not
/// exceptional.
pub async fn get_workos_directory_group<F, E>(
    api_key: &str,
    directory_group_id: &str,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Option<DirectoryGroup>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    let url = format!("https://api.workos.com/directory_groups/{directory_group_id}");

    let request = http::Request::builder()
        .uri(&url)
        .method(http::Method::GET)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .body(vec![])?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not fetch directory group: {}", e))?;

    if response.status() == http::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    if response.status() != http::StatusCode::OK {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new(
            "get directory group",
            status,
            &response_body
        ));
    }

    let response_body = response.into_body();
    let group: DirectoryGroup = serde_json::from_slice(&response_body).with_context(|| {
        format!(
            "Invalid WorkOS directory group response: {}",
            String::from_utf8_lossy(&response_body)
        )
    })?;

    Ok(Some(group))
}

/// Fetches one page of the WorkOS Events API, restricted to `event_types` and
/// resuming after the event id `after`. Errors if `event_types` is empty.
pub async fn list_workos_events<F, E>(
    api_key: &str,
    event_types: &[&str],
    after: Option<&str>,
    limit: u32,
    http_client: &(impl Fn(HttpRequest) -> F + 'static + ?Sized),
) -> anyhow::Result<Vec<WorkOSEvent>>
where
    F: Future<Output = Result<HttpResponse, E>>,
    E: std::error::Error + 'static + Send + Sync,
{
    const EVENTS_URL: &str = "https://api.workos.com/events";

    if event_types.is_empty() {
        anyhow::bail!("Refusing to list events with no event types");
    }

    let mut url = url::Url::parse(EVENTS_URL)?;
    {
        let mut pairs = url.query_pairs_mut();
        // The Events API takes its type filter as a repeated `events`
        // parameter; omitting it entirely would return every event type.
        for event_type in event_types {
            pairs.append_pair("events", event_type);
        }
        pairs.append_pair("limit", &limit.to_string());
        // Oldest first, which is what resuming from `after` assumes. This is
        // already the API's default; setting it keeps the cursor protocol
        // correct if that default ever changes.
        pairs.append_pair("order", "asc");
        if let Some(after) = after {
            pairs.append_pair("after", after);
        }
    }

    let request = http::Request::builder()
        .uri(url.as_str())
        .method(http::Method::GET)
        .header(http::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(http::header::ACCEPT, APPLICATION_JSON)
        .header(http::header::CONTENT_TYPE, APPLICATION_JSON)
        .body(vec![])?;

    let response = timeout(WORKOS_API_TIMEOUT, http_client(request))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "WorkOS API call timed out after {}s",
                WORKOS_API_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Could not list events: {e}"))?;

    if response.status() != http::StatusCode::OK {
        let status = response.status();
        let response_body = response.into_body();
        anyhow::bail!(WorkOSApiError::new("list events", status, &response_body));
    }

    let response_body = response.into_body();
    let page: WorkOSListResponse<WorkOSEvent> = serde_json::from_slice(&response_body)
        .with_context(|| {
            format!(
                "Invalid WorkOS list events response: {}",
                String::from_utf8_lossy(&response_body)
            )
        })?;

    Ok(page.data)
}
