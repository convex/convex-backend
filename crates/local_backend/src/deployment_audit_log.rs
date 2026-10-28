use axum::{
    extract::FromRef,
    response::IntoResponse,
};
use common::{
    http::{
        extract::{
            Json,
            MtState,
            Query,
        },
        HttpResponseError,
        PaginationMetadata,
    },
    types::{
        AccessTokenId,
        MemberId,
    },
};
use model::deployment_audit_log::types::{
    DeploymentAuditLogActor,
    DeploymentAuditLogEntry,
    DeploymentAuditLogEventKind,
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as JsonValue;
use utoipa::{
    IntoParams,
    ToSchema,
};
use utoipa_axum::router::OpenApiRouter;
use value::export::ValueFormat;

use crate::{
    authentication::ExtractIdentity,
    LocalAppState,
};

#[derive(Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListDeploymentAuditLogEventsArgs {
    /// Only return events on or after this time, in milliseconds since epoch.
    /// Pass the same value on every page of a paginated request.
    from: u64,
    /// Maximum number of events to return (defaults to 15, capped at 100).
    limit: Option<usize>,
    /// Cursor from a previous response to fetch the next page.
    cursor: Option<String>,
}

/// The identity that performed an audit log action.
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AuditLogActor {
    /// The internal Convex system.
    #[schema(title = "System")]
    System,
    /// A member acting via the dashboard.
    #[schema(title = "Member")]
    Member {
        /// Member ID
        member_id: MemberId,
    },
    /// A deploy key or access token.
    #[schema(title = "Token")]
    Token {
        /// Member ID the token belongs to, if any.
        member_id: Option<MemberId>,
        /// Token ID. `0` for legacy audit log rows created before token IDs
        /// were recorded.
        token_id: AccessTokenId,
        /// Client ID of the OAuth application, if the token belongs to one.
        client_id: Option<String>,
    },
}

impl From<DeploymentAuditLogActor> for AuditLogActor {
    fn from(actor: DeploymentAuditLogActor) -> Self {
        match actor {
            DeploymentAuditLogActor::System => AuditLogActor::System,
            DeploymentAuditLogActor::Member { member_id } => AuditLogActor::Member { member_id },
            DeploymentAuditLogActor::Token {
                member_id,
                token_id,
                client_id,
            } => AuditLogActor::Token {
                member_id,
                token_id,
                client_id,
            },
        }
    }
}

/// OpenAPI schema for the audit log `action` field, enumerating every possible
/// action string.
fn action_schema() -> utoipa::openapi::schema::Object {
    use utoipa::openapi::schema::{
        ObjectBuilder,
        SchemaType,
        Type,
    };

    let values: Vec<String> = DeploymentAuditLogEventKind::actions()
        .map(String::from)
        .collect();
    ObjectBuilder::new()
        .schema_type(SchemaType::Type(Type::String))
        .enum_values(Some(values))
        .build()
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentAuditLogEventResponse {
    /// The identity that performed the action.
    actor: AuditLogActor,
    /// The audit log action.
    #[schema(schema_with = action_schema)]
    action: String,
    /// Time the event was created, in milliseconds since epoch.
    create_time: i64,
    /// Additional JSON metadata about the audit log event.
    metadata: JsonValue,
    /// IP address of the client that performed the action, if known.
    client_ip: Option<String>,
    /// User agent of the client that performed the action, if known.
    client_user_agent: Option<String>,
}

impl TryFrom<DeploymentAuditLogEntry> for DeploymentAuditLogEventResponse {
    type Error = anyhow::Error;

    fn try_from(entry: DeploymentAuditLogEntry) -> anyhow::Result<Self> {
        let action = entry.action.action().to_string();
        let metadata = entry
            .action
            .metadata()?
            .export(ValueFormat::ConvexCleanJSON);
        Ok(DeploymentAuditLogEventResponse {
            actor: entry.actor.into(),
            action,
            create_time: entry.create_time,
            metadata,
            client_ip: entry.client_ip,
            client_user_agent: entry.client_user_agent,
        })
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListDeploymentAuditLogEventsResponse {
    /// The audit log events for this page, from least to most recent.
    items: Vec<DeploymentAuditLogEventResponse>,
    pagination: PaginationMetadata,
}

/// List audit log events
///
/// List a deployment's audit log events on or after a timestamp, from least to
/// most recent. Pass the returned `cursor` (along with the same `from`) to
/// fetch the next page.
#[utoipa::path(
    get,
    path = "/list_audit_log_events",
    tag = "Audit Log",
    params(ListDeploymentAuditLogEventsArgs),
    responses((status = 200, body = ListDeploymentAuditLogEventsResponse)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn list_audit_log_events(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Query(args): Query<ListDeploymentAuditLogEventsArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let (events, cursor) = st
        .application
        .list_audit_log_events(identity, args.from, args.limit, args.cursor)
        .await?;

    let items = events
        .into_iter()
        .map(DeploymentAuditLogEventResponse::try_from)
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(Json(ListDeploymentAuditLogEventsResponse {
        items,
        pagination: PaginationMetadata {
            has_more: cursor.is_some(),
            next_cursor: cursor,
        },
    }))
}

pub fn platform_router<S>() -> OpenApiRouter<S>
where
    LocalAppState: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    OpenApiRouter::new().routes(utoipa_axum::routes!(list_audit_log_events))
}
