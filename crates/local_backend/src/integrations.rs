use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    net::Ipv4Addr,
};

use anyhow::Context;
use application::{
    integrations::IntegrationWithId,
    Application,
};
use axum::{
    extract::FromRef,
    response::IntoResponse,
};
use common::{
    http::{
        extract::{
            Json,
            MtState,
            Path,
        },
        ExtractRequestMetadata,
        HttpResponseError,
    },
    knobs::AXIOM_MAX_ATTRIBUTES,
    log_streaming::{
        LogEventFormatVersion,
        LogTopic,
    },
    types::streaming_export::selection::{
        ColumnSelection,
        ComponentSelection,
        InclusionDefault,
        Selection,
        TableSelection,
    },
};
use errors::ErrorMetadata;
use http::StatusCode;
use keybroker::Identity;
use model::integrations::types::{
    axiom::{
        AxiomAttribute,
        AxiomConfig,
        VALID_AXIOM_INGEST_URLS,
    },
    datadog::{
        DatadogConfig,
        DatadogSiteLocation,
    },
    posthog_error_tracking::PostHogErrorTrackingConfig,
    posthog_logs::PostHogLogsConfig,
    s3_export::{
        S3ExportConfig,
        S3ExportCredentials,
    },
    sentry::{
        ExceptionFormatVersion,
        SentryConfig,
    },
    webhook::{
        generate_webhook_hmac_secret,
        WebhookConfig,
        WebhookFormat,
    },
    IntegrationConfig,
    IntegrationType,
};
use roles::RequireDeploymentOp;
use runtime::prod::ProdRuntime;
use sentry::types::Dsn;
use serde::{
    Deserialize,
    Serialize,
};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use value::FieldName;

use crate::{
    authentication::ExtractIdentity,
    LocalAppState,
};

/// Status of a log stream
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
#[schema(as = LogStreamStatus)]
pub enum IntegrationStatus {
    Pending,
    Restarting,
    #[serde(rename_all = "camelCase")]
    Failed {
        reason: String,
    },
    Active,
    #[serde(rename = "deleting")]
    Tombstoned,
}

impl From<model::integrations::types::SerializedIntegrationState> for IntegrationStatus {
    fn from(value: model::integrations::types::SerializedIntegrationState) -> Self {
        use model::integrations::types::SerializedIntegrationState;
        match value {
            SerializedIntegrationState::Pending => IntegrationStatus::Pending,
            SerializedIntegrationState::Restarting => IntegrationStatus::Restarting,
            SerializedIntegrationState::Failed { reason } => IntegrationStatus::Failed { reason },
            SerializedIntegrationState::Active => IntegrationStatus::Active,
            SerializedIntegrationState::Tombstoned => IntegrationStatus::Tombstoned,
        }
    }
}

fn validate_axiom_ingest_url(ingest_url: Option<&String>) -> anyhow::Result<()> {
    if let Some(url) = ingest_url
        && !VALID_AXIOM_INGEST_URLS.contains(&url.as_str())
    {
        anyhow::bail!(ErrorMetadata::bad_request(
            "InvalidAxiomIngestUrl",
            format!(
                "Invalid Axiom ingest URL: {url}. Must be one of: {}",
                VALID_AXIOM_INGEST_URLS.join(", ")
            ),
        ));
    }
    Ok(())
}

fn validate_subscribed_topics(
    topics: Option<Vec<LogTopic>>,
) -> anyhow::Result<Option<BTreeSet<LogTopic>>> {
    topics
        .map(|topics| {
            if topics.is_empty() {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "EmptyLogTopics",
                    "A log stream must be subscribed to at least one topic.",
                ));
            }
            topics
                .into_iter()
                .map(|topic| {
                    if topic.is_subscribable() {
                        Ok(topic)
                    } else {
                        Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                            "InvalidLogTopic",
                            format!("Log stream topic `{topic}` cannot be subscribed to"),
                        )))
                    }
                })
                .collect::<anyhow::Result<BTreeSet<LogTopic>>>()
        })
        .transpose()
}

async fn ensure_topic_entitlements(
    application: &Application<ProdRuntime>,
    identity: &keybroker::Identity,
    topics: &Option<BTreeSet<LogTopic>>,
) -> Result<(), HttpResponseError> {
    if topics
        .as_ref()
        .is_some_and(|topics| topics.contains(&LogTopic::CustomAudit))
    {
        application
            .ensure_custom_audit_logs_in_log_streams_allowed(identity.clone())
            .await?;
    }
    Ok(())
}

fn resolve_topics_update(
    // Outer option None means there is no update. Inner None means subscribe to all topics.
    update: Option<Option<Vec<LogTopic>>>,
    existing: Option<BTreeSet<LogTopic>>,
) -> anyhow::Result<Option<BTreeSet<LogTopic>>> {
    match update {
        None => Ok(existing),
        Some(topics) => validate_subscribed_topics(topics),
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = LogStreamType)]
enum ApiIntegrationType {
    Datadog,
    Webhook,
    Axiom,
    Sentry,
    PostHogLogs,
    PostHogErrorTracking,
}

impl From<ApiIntegrationType> for IntegrationType {
    fn from(integration_type: ApiIntegrationType) -> Self {
        match integration_type {
            ApiIntegrationType::Datadog => IntegrationType::Datadog,
            ApiIntegrationType::Webhook => IntegrationType::Webhook,
            ApiIntegrationType::Axiom => IntegrationType::Axiom,
            ApiIntegrationType::Sentry => IntegrationType::Sentry,
            ApiIntegrationType::PostHogLogs => IntegrationType::PostHogLogs,
            ApiIntegrationType::PostHogErrorTracking => IntegrationType::PostHogErrorTracking,
        }
    }
}

/// Analytics export destinations are gated on the streaming export entitlement;
/// every other integration type streams logs and is gated on log streaming.
async fn ensure_integration_allowed(
    application: &Application<ProdRuntime>,
    identity: &Identity,
    integration_type: &IntegrationType,
) -> anyhow::Result<()> {
    match integration_type {
        IntegrationType::S3Export => {
            application
                .ensure_streaming_export_enabled(identity.clone())
                .await
        },
        IntegrationType::Local
        | IntegrationType::Datadog
        | IntegrationType::DatadogV2
        | IntegrationType::Webhook
        | IntegrationType::Axiom
        | IntegrationType::AxiomV2
        | IntegrationType::Sentry
        | IntegrationType::PostHogLogs
        | IntegrationType::PostHogErrorTracking => {
            application
                .ensure_log_streaming_allowed(identity.clone())
                .await
        },
    }
}

/// Delete integration
///
/// Delete the deployment's integration with the given id.
#[utoipa::path(
    delete,
    path = "/integrations/{id}",
    tag = "Integrations",
    responses((status = 200)),
    params(
        ("id" = String, Path, description = "id of the integration to delete"),
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn delete_integration(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteIntegrations)?;
    let integration = st.application.must_get_integration_by_id(&id).await?;
    ensure_integration_allowed(
        &st.application,
        &identity,
        &integration.config.integration_type(),
    )
    .await?;

    st.application
        .remove_integration_by_id(identity, request_metadata, id)
        .await?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = CreateDatadogLogStreamArgs)]
pub struct CreateDatadogIntegrationArgs {
    /// Location of your Datadog deployment.
    site_location: DatadogSiteLocation,
    /// Datadog API key for authentication.
    dd_api_key: String,
    /// Optional comma-separated list of tags. These are sent to Datadog in each
    /// log event via the `ddtags` field.
    dd_tags: Vec<String>,
    /// Service name used as a special tag in Datadog.
    service: Option<String>,
    /// The topics this log stream is subscribed to. Omit to
    /// subscribe to all topics, including ones added in the future.
    #[serde(default)]
    topics: Option<Vec<LogTopic>>,
}

impl TryFrom<CreateDatadogIntegrationArgs> for DatadogConfig {
    type Error = anyhow::Error;

    fn try_from(value: CreateDatadogIntegrationArgs) -> Result<Self, Self::Error> {
        Ok(Self {
            site_location: value.site_location,
            dd_api_key: value.dd_api_key.into(),
            dd_tags: value.dd_tags,
            version: LogEventFormatVersion::V2,
            service: value.service,
            topics: validate_subscribed_topics(value.topics)?,
        })
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = CreateWebhookLogStreamArgs)]
pub struct CreateWebhookIntegrationArgs {
    /// URL to send logs to.
    url: String,
    /// Format for the webhook payload. JSONL sends one object per line of
    /// request, JSON sends one array per request.
    format: WebhookFormat,
    /// The topics this log stream is subscribed to. Omit to
    /// subscribe to all topics, including ones added in the future.
    #[serde(default)]
    topics: Option<Vec<LogTopic>>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = CreateAxiomLogStreamArgs)]
pub struct CreateAxiomIntegrationArgs {
    /// Axiom API key for authentication.
    api_key: String,
    /// Name of the dataset in Axiom. This is where the logs will be sent.
    dataset_name: String,
    /// Optional list of attributes. These are extra fields and values sent to
    /// Axiom in each log event.
    attributes: Vec<AxiomAttribute>,
    /// Optional ingest endpoint for Axiom
    ingest_url: Option<String>,
    /// The topics this log stream is subscribed to. Omit to
    /// subscribe to all topics, including ones added in the future.
    #[serde(default)]
    topics: Option<Vec<LogTopic>>,
}

impl TryFrom<CreateAxiomIntegrationArgs> for AxiomConfig {
    type Error = anyhow::Error;

    fn try_from(value: CreateAxiomIntegrationArgs) -> Result<Self, Self::Error> {
        validate_axiom_ingest_url(value.ingest_url.as_ref())?;

        Ok(Self {
            api_key: value.api_key.into(),
            dataset_name: value.dataset_name,
            attributes: value.attributes,
            version: LogEventFormatVersion::V2,
            ingest_url: value.ingest_url,
            topics: validate_subscribed_topics(value.topics)?,
        })
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = CreateSentryLogStreamArgs)]
pub struct CreateSentryIntegrationArgs {
    /// Sentry Data Source Name (DSN) to route exceptions to.
    dsn: String,
    /// Tags to add to all events routed to Sentry.
    #[schema(value_type = Option<BTreeMap<String, String>>)]
    tags: Option<BTreeMap<FieldName, String>>,
}

impl TryFrom<CreateSentryIntegrationArgs> for SentryConfig {
    type Error = anyhow::Error;

    fn try_from(value: CreateSentryIntegrationArgs) -> Result<Self, Self::Error> {
        Ok(Self {
            dsn: value
                .dsn
                .parse::<Dsn>()
                .context(ErrorMetadata::bad_request(
                    "InvalidSentryDsn",
                    "The Sentry DSN passed was invalid",
                ))?
                .into(),
            tags: value.tags,
            version: ExceptionFormatVersion::V2,
        })
    }
}

fn validate_posthog_host(host: Option<&String>) -> anyhow::Result<()> {
    if let Some(url) = host {
        url.parse::<reqwest::Url>().map_err(|_| {
            anyhow::anyhow!(ErrorMetadata::bad_request(
                "InvalidPostHogHost",
                format!("Invalid PostHog host URL: {url}"),
            ))
        })?;
    }
    Ok(())
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = CreatePostHogLogsLogStreamArgs)]
pub struct CreatePostHogLogsIntegrationArgs {
    /// PostHog project token.
    api_key: String,
    /// PostHog host URL. Defaults to https://us.i.posthog.com.
    host: Option<String>,
    /// OTLP service.name attribute. Defaults to the deployment name.
    service_name: Option<String>,
    /// The topics this log stream is subscribed to. Omit to
    /// subscribe to all topics, including ones added in the future.
    #[serde(default)]
    topics: Option<Vec<LogTopic>>,
}

impl TryFrom<CreatePostHogLogsIntegrationArgs> for PostHogLogsConfig {
    type Error = anyhow::Error;

    fn try_from(value: CreatePostHogLogsIntegrationArgs) -> Result<Self, Self::Error> {
        validate_posthog_host(value.host.as_ref())?;
        Ok(Self {
            api_key: value.api_key.into(),
            host: value.host,
            service_name: value.service_name,
            topics: validate_subscribed_topics(value.topics)?,
        })
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = CreatePostHogErrorTrackingLogStreamArgs)]
pub struct CreatePostHogErrorTrackingIntegrationArgs {
    /// PostHog project token.
    api_key: String,
    /// PostHog host URL. Defaults to https://us.i.posthog.com.
    host: Option<String>,
}

impl TryFrom<CreatePostHogErrorTrackingIntegrationArgs> for PostHogErrorTrackingConfig {
    type Error = anyhow::Error;

    fn try_from(value: CreatePostHogErrorTrackingIntegrationArgs) -> Result<Self, Self::Error> {
        validate_posthog_host(value.host.as_ref())?;
        Ok(Self {
            api_key: value.api_key.into(),
            host: value.host,
        })
    }
}

/// Reserved by AWS for bucket names.
const S3_RESERVED_BUCKET_PREFIXES: [&str; 2] = ["xn--", "sthree-"];
const S3_RESERVED_BUCKET_SUFFIXES: [&str; 2] = ["-s3alias", "--ol-s3"];

// Changing field filters would require re-exporting documents that the cursor
// has already passed. Table selection changes are handled by the iterator.
fn validate_s3_selection(selection: &Selection) -> anyhow::Result<()> {
    let filters_fields = selection
        .components
        .values()
        .any(|component| match component {
            ComponentSelection::Included { tables, .. } => tables.values().any(|table| {
                matches!(table, TableSelection::Included { columns, other_columns }
                if *other_columns == InclusionDefault::Excluded
                    || columns.values().any(|c| *c == ColumnSelection::Excluded))
            }),
            ComponentSelection::Excluded(_) => false,
        });
    anyhow::ensure!(
        !filters_fields,
        ErrorMetadata::bad_request(
            "FieldFiltersUnsupported",
            "S3 exports can't filter fields within a table yet. Select whole tables instead.",
        )
    );
    Ok(())
}

// Resuming a cursor in a different destination would skip all previously
// exported pages. `/` and `None` address the same S3 directory, so an update
// that only respells the prefix keeps the stored one.
fn ensure_same_s3_destination(
    existing: &S3ExportConfig,
    updated: S3ExportConfig,
) -> anyhow::Result<S3ExportConfig> {
    let trimmed = |prefix: &Option<String>| {
        prefix
            .as_deref()
            .unwrap_or_default()
            .trim_matches('/')
            .to_owned()
    };
    let updated = if trimmed(&existing.prefix) == trimmed(&updated.prefix) {
        S3ExportConfig {
            prefix: existing.prefix.clone(),
            ..updated
        }
    } else {
        updated
    };
    anyhow::ensure!(
        !existing.requires_resync(&updated),
        ErrorMetadata::bad_request(
            "S3ExportDestinationChanged",
            "An S3 export's bucket, region, and prefix can't change. Delete the export and create \
             a new one instead.",
        )
    );
    Ok(updated)
}

// The prefix lands in IAM policy resource ARNs, where `*` and `?` are wildcards
// and `$` starts a policy variable.
fn validate_s3_prefix(prefix: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !prefix.contains(['*', '?', '$']),
        ErrorMetadata::bad_request(
            "InvalidS3Prefix",
            "Prefixes may not contain `*`, `?` or `$`",
        )
    );
    Ok(())
}

fn validate_s3_region(region: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        (1..=63).contains(&region.len()),
        ErrorMetadata::bad_request("InvalidS3Region", "Region must be between 1 and 63 bytes")
    );
    Ok(())
}

fn validate_s3_bucket(bucket: &str) -> anyhow::Result<()> {
    // https://docs.aws.amazon.com/AmazonS3/latest/userguide/bucketnamingrules.html
    let is_lower_alnum = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    let valid = (3..=63).contains(&bucket.len())
        && bucket
            .chars()
            .all(|c| is_lower_alnum(c) || c == '-' || c == '.')
        && bucket.starts_with(is_lower_alnum)
        && bucket.ends_with(is_lower_alnum)
        && !bucket.contains("..")
        && bucket.parse::<Ipv4Addr>().is_err()
        && !S3_RESERVED_BUCKET_PREFIXES
            .iter()
            .any(|prefix| bucket.starts_with(prefix))
        && !S3_RESERVED_BUCKET_SUFFIXES
            .iter()
            .any(|suffix| bucket.ends_with(suffix));
    if !valid {
        anyhow::bail!(ErrorMetadata::bad_request(
            "InvalidS3Bucket",
            format!("`{bucket}` is not a valid S3 bucket name"),
        ));
    }
    Ok(())
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schema(as = CreateS3ExportLogStreamArgs)]
pub struct CreateS3ExportIntegrationArgs {
    /// Name of the S3 bucket to mirror into.
    bucket: String,
    /// AWS region the bucket lives in, e.g. `us-east-1`.
    region: String,
    /// Folder in the bucket to put the export under. Convex writes to
    /// `<prefix>/<deployment-name>/data-lake/`, or to
    /// `<deployment-name>/data-lake/` if you omit it.
    #[serde(default)]
    prefix: Option<String>,
    /// ARN of the IAM role Convex assumes to write. The role must be named
    /// `ConvexIntegrations-<roleExternalId>`, with path `/convex/`. Omit it to
    /// create the export first: the response carries the `roleExternalId` the
    /// role's name and trust policy need, and the export waits until an update
    /// sets the ARN.
    #[serde(default)]
    role_arn: String,
    /// The components and tables to export. Defaults to everything. Each table
    /// exports whole documents, so filtering fields within a table is rejected.
    #[serde(default)]
    selection: Option<Selection>,
}

impl TryFrom<CreateS3ExportIntegrationArgs> for S3ExportConfig {
    type Error = anyhow::Error;

    fn try_from(value: CreateS3ExportIntegrationArgs) -> Result<Self, Self::Error> {
        validate_s3_bucket(&value.bucket)?;
        validate_s3_region(&value.region)?;
        validate_s3_prefix(value.prefix.as_deref().unwrap_or_default())?;
        let selection = value.selection.unwrap_or_default();
        validate_s3_selection(&selection)?;
        let credentials = S3ExportCredentials {
            role_arn: value.role_arn,
            external_id: S3ExportCredentials::new_external_id(),
        };
        Ok(Self {
            bucket: value.bucket,
            region: value.region,
            prefix: value.prefix,
            credentials,
            selection,
        })
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "logStreamType")]
#[schema(as = CreateLogStreamArgs)]
pub enum CreateIntegrationArgs {
    #[schema(title = "Datadog")]
    Datadog(CreateDatadogIntegrationArgs),
    #[schema(title = "Webhook")]
    Webhook(CreateWebhookIntegrationArgs),
    #[schema(title = "Axiom")]
    Axiom(CreateAxiomIntegrationArgs),
    #[schema(title = "Sentry")]
    Sentry(CreateSentryIntegrationArgs),
    #[schema(title = "PostHogLogs")]
    PostHogLogs(CreatePostHogLogsIntegrationArgs),
    #[schema(title = "PostHogErrorTracking")]
    PostHogErrorTracking(CreatePostHogErrorTrackingIntegrationArgs),
    #[schema(title = "S3Export")]
    S3Export(CreateS3ExportIntegrationArgs),
}

impl CreateIntegrationArgs {
    fn integration_type(&self) -> IntegrationType {
        match self {
            Self::Datadog(_) => IntegrationType::Datadog,
            Self::Webhook(_) => IntegrationType::Webhook,
            Self::Axiom(_) => IntegrationType::Axiom,
            Self::Sentry(_) => IntegrationType::Sentry,
            Self::PostHogLogs(_) => IntegrationType::PostHogLogs,
            Self::PostHogErrorTracking(_) => IntegrationType::PostHogErrorTracking,
            Self::S3Export(_) => IntegrationType::S3Export,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = CreateWebhookLogStreamResponse)]
pub struct CreateWebhookIntegrationResponse {
    id: String,
    /// Use this secret to verify webhook signatures.
    hmac_secret: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "logStreamType")]
#[schema(as = CreateLogStreamResponse)]
pub enum CreateIntegrationResponse {
    #[schema(title = "Webhook")]
    Webhook(CreateWebhookIntegrationResponse),
    #[schema(title = "Datadog")]
    Datadog { id: String },
    #[schema(title = "Axiom")]
    Axiom { id: String },
    #[schema(title = "Sentry")]
    Sentry { id: String },
    #[schema(title = "PostHogLogs")]
    PostHogLogs { id: String },
    #[schema(title = "PostHogErrorTracking")]
    PostHogErrorTracking { id: String },
    #[schema(title = "S3Export")]
    #[serde(rename_all = "camelCase")]
    S3Export {
        id: String,
        /// The role's trust policy must require it as `sts:ExternalId`.
        role_external_id: String,
    },
}

async fn ensure_integration_does_not_exist(
    application: &Application<ProdRuntime>,
    integration_type: &IntegrationType,
) -> Result<(), HttpResponseError> {
    if application
        .get_integration(integration_type)
        .await?
        .is_some()
    {
        return Err(anyhow::anyhow!(ErrorMetadata::conflict(
            "LogStreamAlreadyExists",
            "This deployment already has this integration. Delete the existing one first."
        ))
        .into());
    }
    Ok(())
}

/// Create integration
///
/// Create a new integration for the deployment. Errors if a integration of the
/// given type already exists.
#[utoipa::path(
    post,
    path = "/integrations",
    tag = "Integrations",
    responses((status = 200, body = CreateIntegrationResponse)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn create_integration(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(args): Json<CreateIntegrationArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteIntegrations)?;
    if matches!(&args, CreateIntegrationArgs::S3Export(_)) {
        identity.require_operation(keybroker::DeploymentOp::ViewData)?;
    }
    ensure_integration_allowed(&st.application, &identity, &args.integration_type()).await?;

    match args {
        CreateIntegrationArgs::Datadog(datadog_integration_post_args) => {
            ensure_integration_does_not_exist(&st.application, &IntegrationType::Datadog).await?;

            let config: DatadogConfig = datadog_integration_post_args.try_into()?;
            ensure_topic_entitlements(&st.application, &identity, &config.topics).await?;
            let id = st
                .application
                .add_integration(
                    identity.clone(),
                    request_metadata.clone(),
                    IntegrationConfig::Datadog(config),
                )
                .await?;
            Ok(Json(CreateIntegrationResponse::Datadog {
                id: id.to_string(),
            }))
        },
        CreateIntegrationArgs::Webhook(webhook_integration_post_args) => {
            ensure_integration_does_not_exist(&st.application, &IntegrationType::Webhook).await?;

            let hmac_secret = generate_webhook_hmac_secret(st.application.runtime());

            let url = webhook_integration_post_args.url.parse().map_err(|_| {
                anyhow::anyhow!(ErrorMetadata::bad_request(
                    "InvalidWebhookUrl",
                    "The URL passed was invalid"
                ))
            })?;

            let topics = validate_subscribed_topics(webhook_integration_post_args.topics)?;
            ensure_topic_entitlements(&st.application, &identity, &topics).await?;
            let config = WebhookConfig {
                url,
                format: webhook_integration_post_args.format,
                hmac_secret: hmac_secret.clone(),
                topics,
            };
            let id = st
                .application
                .add_integration(
                    identity.clone(),
                    request_metadata.clone(),
                    IntegrationConfig::Webhook(config),
                )
                .await?;

            Ok(Json(CreateIntegrationResponse::Webhook(
                CreateWebhookIntegrationResponse {
                    hmac_secret,
                    id: id.to_string(),
                },
            )))
        },
        CreateIntegrationArgs::Axiom(axiom_integration_post_args) => {
            ensure_integration_does_not_exist(&st.application, &IntegrationType::Axiom).await?;

            if axiom_integration_post_args.attributes.len() > *AXIOM_MAX_ATTRIBUTES {
                return Err(anyhow::anyhow!(
                    "Exceeded max number of Axiom attributes. Contact support@convex.dev to \
                     request a limit increase."
                )
                .into());
            }

            let config: AxiomConfig = axiom_integration_post_args.try_into()?;
            ensure_topic_entitlements(&st.application, &identity, &config.topics).await?;
            let id = st
                .application
                .add_integration(
                    identity.clone(),
                    request_metadata.clone(),
                    IntegrationConfig::Axiom(config),
                )
                .await?;

            Ok(Json(CreateIntegrationResponse::Axiom {
                id: id.to_string(),
            }))
        },
        CreateIntegrationArgs::Sentry(sentry_config_args) => {
            ensure_integration_does_not_exist(&st.application, &IntegrationType::Sentry).await?;

            let config = sentry_config_args.try_into()?;
            let id = st
                .application
                .add_integration(
                    identity.clone(),
                    request_metadata.clone(),
                    IntegrationConfig::Sentry(config),
                )
                .await?;
            Ok(Json(CreateIntegrationResponse::Sentry {
                id: id.to_string(),
            }))
        },
        CreateIntegrationArgs::PostHogLogs(args) => {
            ensure_integration_does_not_exist(&st.application, &IntegrationType::PostHogLogs)
                .await?;

            let config: PostHogLogsConfig = args.try_into()?;
            ensure_topic_entitlements(&st.application, &identity, &config.topics).await?;
            let id = st
                .application
                .add_integration(
                    identity.clone(),
                    request_metadata.clone(),
                    IntegrationConfig::PostHogLogs(config),
                )
                .await?;
            Ok(Json(CreateIntegrationResponse::PostHogLogs {
                id: id.to_string(),
            }))
        },
        CreateIntegrationArgs::PostHogErrorTracking(args) => {
            ensure_integration_does_not_exist(
                &st.application,
                &IntegrationType::PostHogErrorTracking,
            )
            .await?;

            let config: PostHogErrorTrackingConfig = args.try_into()?;
            let id = st
                .application
                .add_integration(
                    identity.clone(),
                    request_metadata.clone(),
                    IntegrationConfig::PostHogErrorTracking(config),
                )
                .await?;
            Ok(Json(CreateIntegrationResponse::PostHogErrorTracking {
                id: id.to_string(),
            }))
        },
        CreateIntegrationArgs::S3Export(args) => {
            ensure_integration_does_not_exist(&st.application, &IntegrationType::S3Export).await?;

            let config: S3ExportConfig = args.try_into()?;
            let role_external_id = config.credentials.external_id.clone();
            let id = st
                .application
                .add_integration(
                    identity.clone(),
                    request_metadata.clone(),
                    IntegrationConfig::S3Export(config),
                )
                .await?;
            Ok(Json(CreateIntegrationResponse::S3Export {
                id: id.to_string(),
                role_external_id,
            }))
        },
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "logStreamType")]
#[schema(as = RotateLogStreamSecretArgs)]
pub enum RotateIntegrationSecretArgs {
    Webhook,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "logStreamType")]
#[schema(as = RotateLogStreamSecretResponse)]
pub enum RotateIntegrationSecretResponse {
    #[serde(rename_all = "camelCase")]
    #[schema(title = "Webhook")]
    Webhook { hmac_secret: String },
}

/// Rotate webhook integration secret
///
/// Rotate the secret for the webhook integration.
#[utoipa::path(
    post,
    path = "/integrations/{id}/rotate_webhook_secret",
    tag = "Integrations",
    responses((status = 200, body = RotateIntegrationSecretResponse)),
    params(
        ("id" = String, Path, description = "id of the webhook integration for which to rotate the secret"),
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn rotate_integration_webhook_secret(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteIntegrations)?;
    st.application
        .ensure_log_streaming_allowed(identity.clone())
        .await?;

    let IntegrationWithId {
        config: integration_config,
        ..
    } = st.application.must_get_integration_by_id(&id).await?;

    match integration_config {
        IntegrationConfig::Webhook(existing_webhook_integration) => {
            let hmac_secret = generate_webhook_hmac_secret(st.application.runtime());

            let config = WebhookConfig {
                url: existing_webhook_integration.url,
                format: existing_webhook_integration.format,
                hmac_secret: hmac_secret.clone(),
                topics: existing_webhook_integration.topics,
            };
            st.application
                .patch_integration_config(
                    identity,
                    request_metadata,
                    &id,
                    IntegrationConfig::Webhook(config),
                )
                .await?;

            Ok(Json(RotateIntegrationSecretResponse::Webhook {
                hmac_secret,
            }))
        },
        _ => Err(anyhow::anyhow!(ErrorMetadata::bad_request(
            "NoSecretToRotate",
            "This log stream does not have a secret to rotate."
        ))
        .into()),
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "logStreamType")]
#[schema(as = LogStreamConfig)]
enum IntegrationResponse {
    #[schema(title = "Datadog")]
    Datadog(DatadogIntegrationConfig),
    #[schema(title = "Webhook")]
    Webhook(WebhookIntegrationConfig),
    #[schema(title = "Axiom")]
    Axiom(AxiomIntegrationConfig),
    #[schema(title = "Sentry")]
    Sentry(SentryIntegrationConfig),
    #[schema(title = "PostHogLogs")]
    PostHogLogs(PostHogLogsIntegrationConfig),
    #[schema(title = "PostHogErrorTracking")]
    PostHogErrorTracking(PostHogErrorTrackingIntegrationConfig),
    #[schema(title = "S3Export")]
    S3Export(S3ExportIntegrationConfig),
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(title = "DatadogConfig")]
#[schema(as = DatadogLogStreamConfig)]
pub struct DatadogIntegrationConfig {
    pub id: String,
    /// Status of the log stream
    pub status: IntegrationStatus,
    /// Location of your Datadog deployment.
    pub site_location: DatadogSiteLocation,
    /// Optional comma-separated list of tags. These are sent to Datadog in each
    /// log event via the `ddtags` field.
    pub dd_tags: Vec<String>,
    /// Service name used as a special tag in Datadog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// The topics this log stream is subscribed to. `null` means subscribed to
    /// all topics, including ones added in the future.
    pub topics: Option<Vec<LogTopic>>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(title = "WebhookConfig")]
#[schema(as = WebhookLogStreamConfig)]
pub struct WebhookIntegrationConfig {
    pub id: String,
    /// Status of the log stream
    pub status: IntegrationStatus,
    /// URL to send logs to.
    pub url: String,
    /// Format for the webhook payload. JSONL sends one object per line of
    /// request, JSON sends one array per request.
    pub format: WebhookFormat,
    /// Use this secret to verify webhook signatures.
    pub hmac_secret: String,
    /// The topics this log stream is subscribed to. `null` means subscribed to
    /// all topics, including ones added in the future.
    pub topics: Option<Vec<LogTopic>>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(title = "AxiomConfig")]
#[schema(as = AxiomLogStreamConfig)]
pub struct AxiomIntegrationConfig {
    pub id: String,
    /// Status of the log stream
    pub status: IntegrationStatus,
    /// Name of the dataset in Axiom. This is where the logs will be sent.
    pub dataset_name: String,
    /// Optional list of attributes. These are extra fields and values sent to
    /// Axiom in each log event.
    pub attributes: Vec<AxiomAttribute>,
    /// Optional ingest endpoint for Axiom
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingest_url: Option<String>,
    /// The topics this log stream is subscribed to. `null` means subscribed to
    /// all topics, including ones added in the future.
    pub topics: Option<Vec<LogTopic>>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(title = "SentryConfig")]
#[schema(as = SentryLogStreamConfig)]
pub struct SentryIntegrationConfig {
    pub id: String,
    /// Status of the log stream
    pub status: IntegrationStatus,
    /// Tags to add to all events routed to Sentry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<BTreeMap<String, String>>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(title = "PostHogLogsConfig")]
#[schema(as = PostHogLogsLogStreamConfig)]
pub struct PostHogLogsIntegrationConfig {
    pub id: String,
    /// Status of the log stream
    pub status: IntegrationStatus,
    /// PostHog host URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// OTLP service.name attribute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    /// The topics this log stream is subscribed to. `null` means subscribed to
    /// all topics, including ones added in the future.
    pub topics: Option<Vec<LogTopic>>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(title = "PostHogErrorTrackingConfig")]
#[schema(as = PostHogErrorTrackingLogStreamConfig)]
pub struct PostHogErrorTrackingIntegrationConfig {
    pub id: String,
    /// Status of the log stream
    pub status: IntegrationStatus,
    /// PostHog host URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(title = "S3ExportConfig")]
#[schema(as = S3ExportLogStreamConfig)]
pub struct S3ExportIntegrationConfig {
    pub id: String,
    /// Status of the integration
    pub status: IntegrationStatus,
    /// Name of the S3 bucket being mirrored into.
    pub bucket: String,
    /// AWS region the bucket lives in.
    pub region: String,
    /// Folder in the bucket the export is under, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// ARN of the role Convex assumes to write. Empty if it hasn't been set
    /// yet, and the export doesn't sync while it's empty.
    pub role_arn: String,
    /// Value the role's trust policy must require as `sts:ExternalId`.
    pub role_external_id: String,
    /// The components and tables being exported.
    pub selection: Selection,
}

fn integration_to_api_config(integration: IntegrationWithId) -> Option<IntegrationResponse> {
    let status: IntegrationStatus =
        Into::<model::integrations::types::SerializedIntegrationState>::into(integration.status)
            .into();
    match integration.config {
        IntegrationConfig::Datadog(config) => {
            Some(IntegrationResponse::Datadog(DatadogIntegrationConfig {
                id: integration.id.to_string(),
                status,
                site_location: config.site_location,
                dd_tags: config.dd_tags,
                service: config.service,
                topics: config.topics.map(|topics| topics.into_iter().collect()),
            }))
        },
        IntegrationConfig::Webhook(config) => {
            Some(IntegrationResponse::Webhook(WebhookIntegrationConfig {
                id: integration.id.to_string(),
                status,
                url: config.url.to_string(),
                format: config.format,
                hmac_secret: config.hmac_secret,
                topics: config.topics.map(|topics| topics.into_iter().collect()),
            }))
        },
        IntegrationConfig::Axiom(config) => {
            Some(IntegrationResponse::Axiom(AxiomIntegrationConfig {
                id: integration.id.to_string(),
                status,
                dataset_name: config.dataset_name,
                attributes: config.attributes,
                ingest_url: config.ingest_url,
                topics: config.topics.map(|topics| topics.into_iter().collect()),
            }))
        },
        IntegrationConfig::Sentry(config) => {
            Some(IntegrationResponse::Sentry(SentryIntegrationConfig {
                id: integration.id.to_string(),
                status,
                tags: config
                    .tags
                    .map(|tags| tags.into_iter().map(|(k, v)| (k.into(), v)).collect()),
            }))
        },
        IntegrationConfig::PostHogLogs(config) => Some(IntegrationResponse::PostHogLogs(
            PostHogLogsIntegrationConfig {
                id: integration.id.to_string(),
                status,
                host: config.host,
                service_name: config.service_name,
                topics: config.topics.map(|topics| topics.into_iter().collect()),
            },
        )),
        IntegrationConfig::PostHogErrorTracking(config) => Some(
            IntegrationResponse::PostHogErrorTracking(PostHogErrorTrackingIntegrationConfig {
                id: integration.id.to_string(),
                status,
                host: config.host,
            }),
        ),
        IntegrationConfig::S3Export(config) => {
            Some(IntegrationResponse::S3Export(S3ExportIntegrationConfig {
                id: integration.id.to_string(),
                status,
                bucket: config.bucket,
                region: config.region,
                prefix: config.prefix,
                role_arn: config.credentials.role_arn,
                role_external_id: config.credentials.external_id,
                selection: config.selection,
            }))
        },
        _ => None,
    }
}

/// List integrations
///
/// List configs for all existing integrations in a deployment.
#[utoipa::path(
    get,
    path = "/integrations",
    tag = "Integrations",
    responses((status = 200, body = Vec<IntegrationResponse>)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn list_integrations(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ViewIntegrations)?;

    Ok(Json(
        st.application
            .list_integrations()
            .await?
            .into_iter()
            .filter_map(integration_to_api_config)
            .collect::<Vec<IntegrationResponse>>(),
    ))
}

/// Get integration
///
/// Get the config for a specific integration by id.
#[utoipa::path(
    get,
    path = "/integrations/{id}",
    tag = "Integrations",
    responses((status = 200, body = IntegrationResponse)),
    params(
        ("id" = String, Path, description = "id of the integration to retrieve"),
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn get_integration(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ViewIntegrations)?;

    let integration_with_id = st.application.must_get_integration_by_id(&id).await?;

    let config = integration_to_api_config(integration_with_id).ok_or_else(|| {
        anyhow::anyhow!(ErrorMetadata::bad_request(
            "UnsupportedLogStreamType",
            "This log stream type is not supported",
        ))
    })?;

    Ok(Json(config))
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = UpdateDatadogSinkArgs)]
pub struct UpdateDatadogIntegrationArgs {
    /// Location of your Datadog deployment.
    #[serde(default)]
    site_location: Option<DatadogSiteLocation>,
    /// Datadog API key for authentication.
    #[serde(default)]
    dd_api_key: Option<String>,
    /// Optional comma-separated list of tags. These are sent to Datadog in each
    /// log event via the `ddtags` field.
    #[serde(default)]
    dd_tags: Option<Vec<String>>,
    /// Service name used as a special tag in Datadog.
    #[serde(default, with = "::serde_with::rust::double_option")]
    service: Option<Option<String>>,
    /// The topics this log stream is subscribed to. Omit to keep the current
    /// subscription, or pass `null` to subscribe to all topics (including ones
    /// added in the future).
    #[serde(default, with = "::serde_with::rust::double_option")]
    topics: Option<Option<Vec<LogTopic>>>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = UpdateWebhookSinkArgs)]
pub struct UpdateWebhookIntegrationArgs {
    /// URL to send logs to.
    #[serde(default)]
    url: Option<String>,
    /// Format for the webhook payload. JSONL sends one object per line of
    /// request, JSON sends one array per request.
    #[serde(default)]
    format: Option<WebhookFormat>,
    /// The topics this log stream is subscribed to. Omit to keep the current
    /// subscription, or pass `null` to subscribe to all topics (including ones
    /// added in the future).
    #[serde(default, with = "::serde_with::rust::double_option")]
    topics: Option<Option<Vec<LogTopic>>>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = UpdateAxiomSinkArgs)]
pub struct UpdateAxiomIntegrationArgs {
    /// Axiom API key for authentication.
    #[serde(default)]
    api_key: Option<String>,
    /// Name of the dataset in Axiom. This is where the logs will be sent.
    #[serde(default)]
    dataset_name: Option<String>,
    /// Optional list of attributes. These are extra fields and values sent to
    /// Axiom in each log event.
    #[serde(default)]
    attributes: Option<Vec<AxiomAttribute>>,
    /// Optional ingest endpoint for Axiom
    #[serde(default, with = "::serde_with::rust::double_option")]
    ingest_url: Option<Option<String>>,
    /// The topics this log stream is subscribed to. Omit to keep the current
    /// subscription, or pass `null` to subscribe to all topics (including ones
    /// added in the future).
    #[serde(default, with = "::serde_with::rust::double_option")]
    topics: Option<Option<Vec<LogTopic>>>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = UpdateSentrySinkArgs)]
pub struct UpdateSentryIntegrationArgs {
    /// Sentry Data Source Name (DSN) to route exceptions to.
    #[serde(default)]
    dsn: Option<String>,
    /// Tags to add to all events routed to Sentry.
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[schema(value_type = Option<Option<BTreeMap<String, String>>>)]
    tags: Option<Option<BTreeMap<FieldName, String>>>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = UpdatePostHogLogsSinkArgs)]
pub struct UpdatePostHogLogsIntegrationArgs {
    /// PostHog project token.
    #[serde(default)]
    api_key: Option<String>,
    /// PostHog host URL.
    #[serde(default, with = "::serde_with::rust::double_option")]
    host: Option<Option<String>>,
    /// OTLP service.name attribute.
    #[serde(default, with = "::serde_with::rust::double_option")]
    service_name: Option<Option<String>>,
    /// The topics this log stream is subscribed to. Omit to keep the current
    /// subscription, or pass `null` to subscribe to all topics (including ones
    /// added in the future).
    #[serde(default, with = "::serde_with::rust::double_option")]
    topics: Option<Option<Vec<LogTopic>>>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = UpdatePostHogErrorTrackingSinkArgs)]
pub struct UpdatePostHogErrorTrackingIntegrationArgs {
    /// PostHog project token.
    #[serde(default)]
    api_key: Option<String>,
    /// PostHog host URL.
    #[serde(default, with = "::serde_with::rust::double_option")]
    host: Option<Option<String>>,
}

/// The destination is fixed at creation.
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schema(as = UpdateS3ExportSinkArgs)]
pub struct UpdateS3ExportIntegrationArgs {
    /// ARN of the IAM role Convex assumes to write. The role must be named
    /// `ConvexIntegrations-<roleExternalId>`, with path `/convex/`. Set it to
    /// finish a pending export, or to switch to a role in another account.
    #[serde(default)]
    role_arn: Option<String>,
    /// The components and tables to export. Each table exports whole
    /// documents, so filtering fields within a table is rejected.
    #[serde(default)]
    selection: Option<Selection>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "logStreamType")]
#[schema(as = UpdateLogStreamArgs)]
pub enum UpdateIntegrationArgs {
    #[schema(title = "Datadog")]
    Datadog(UpdateDatadogIntegrationArgs),
    #[schema(title = "Webhook")]
    Webhook(UpdateWebhookIntegrationArgs),
    #[schema(title = "Axiom")]
    Axiom(UpdateAxiomIntegrationArgs),
    #[schema(title = "Sentry")]
    Sentry(UpdateSentryIntegrationArgs),
    #[schema(title = "PostHogLogs")]
    PostHogLogs(UpdatePostHogLogsIntegrationArgs),
    #[schema(title = "PostHogErrorTracking")]
    PostHogErrorTracking(UpdatePostHogErrorTrackingIntegrationArgs),
    #[schema(title = "S3Export")]
    S3Export(UpdateS3ExportIntegrationArgs),
}

/// Update integration
///
/// Update an existing integration for the deployment. Omit a field to keep the
/// existing value, and use `null` to unset a field. For S3 exports, `null`
/// keeps the existing value too.
#[utoipa::path(
    patch,
    path = "/integrations/{id}",
    tag = "Integrations",
    responses((status = 200)),
    params(
        ("id" = String, Path, description = "id of the integration to update"),
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn update_integration(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(id): Path<String>,
    Json(args): Json<UpdateIntegrationArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteIntegrations)?;

    let IntegrationWithId {
        config: integration_config,
        ..
    } = st.application.must_get_integration_by_id(&id).await?;
    if matches!(&integration_config, IntegrationConfig::S3Export(_)) {
        identity.require_operation(keybroker::DeploymentOp::ViewData)?;
    }
    ensure_integration_allowed(
        &st.application,
        &identity,
        &integration_config.integration_type(),
    )
    .await?;

    match integration_config {
        IntegrationConfig::Datadog(existing_config) => {
            let UpdateIntegrationArgs::Datadog(update_args) = args else {
                return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                    "LogStreamTypeMismatch",
                    "Cannot update a Datadog log stream with arguments for a different log stream \
                     type",
                ))
                .into());
            };

            let topics = resolve_topics_update(update_args.topics, existing_config.topics)?;
            ensure_topic_entitlements(&st.application, &identity, &topics).await?;
            let config = DatadogConfig {
                site_location: update_args
                    .site_location
                    .unwrap_or(existing_config.site_location),
                dd_api_key: update_args
                    .dd_api_key
                    .map(|k| k.into())
                    .unwrap_or(existing_config.dd_api_key),
                dd_tags: update_args.dd_tags.unwrap_or(existing_config.dd_tags),
                version: existing_config.version,
                service: update_args.service.unwrap_or(existing_config.service),
                topics,
            };

            st.application
                .patch_integration_config(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    IntegrationConfig::Datadog(config),
                )
                .await?;
        },
        IntegrationConfig::Webhook(existing_config) => {
            let UpdateIntegrationArgs::Webhook(update_args) = args else {
                return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                    "LogStreamTypeMismatch",
                    "Cannot update a Webhook log stream with arguments for a different log stream \
                     type",
                ))
                .into());
            };

            let url = if let Some(url_str) = update_args.url {
                url_str.parse().map_err(|_| {
                    anyhow::anyhow!(ErrorMetadata::bad_request(
                        "InvalidWebhookUrl",
                        "The URL passed was invalid"
                    ))
                })?
            } else {
                existing_config.url
            };

            let topics = resolve_topics_update(update_args.topics, existing_config.topics)?;
            ensure_topic_entitlements(&st.application, &identity, &topics).await?;
            let config = WebhookConfig {
                url,
                format: update_args.format.unwrap_or(existing_config.format),
                hmac_secret: existing_config.hmac_secret,
                topics,
            };

            st.application
                .patch_integration_config(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    IntegrationConfig::Webhook(config),
                )
                .await?;
        },
        IntegrationConfig::Axiom(existing_config) => {
            let UpdateIntegrationArgs::Axiom(update_args) = args else {
                return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                    "LogStreamTypeMismatch",
                    "Cannot update an Axiom log stream with arguments for a different log stream \
                     type",
                ))
                .into());
            };

            let attributes = update_args.attributes.unwrap_or(existing_config.attributes);
            if attributes.len() > *AXIOM_MAX_ATTRIBUTES {
                return Err(anyhow::anyhow!(
                    "Exceeded max number of Axiom attributes. Contact support@convex.dev to \
                     request a limit increase."
                )
                .into());
            }

            let ingest_url = update_args.ingest_url.unwrap_or(existing_config.ingest_url);
            if ingest_url.is_some() {
                validate_axiom_ingest_url(ingest_url.as_ref())?
            }

            let topics = resolve_topics_update(update_args.topics, existing_config.topics)?;
            ensure_topic_entitlements(&st.application, &identity, &topics).await?;
            let config = AxiomConfig {
                api_key: update_args
                    .api_key
                    .map(|k| k.into())
                    .unwrap_or(existing_config.api_key),
                dataset_name: update_args
                    .dataset_name
                    .unwrap_or(existing_config.dataset_name),
                attributes,
                version: existing_config.version,
                ingest_url,
                topics,
            };

            st.application
                .patch_integration_config(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    IntegrationConfig::Axiom(config),
                )
                .await?;
        },
        IntegrationConfig::Sentry(existing_config) => {
            let UpdateIntegrationArgs::Sentry(update_args) = args else {
                return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                    "LogStreamTypeMismatch",
                    "Cannot update a Sentry log stream with arguments for a different log stream \
                     type",
                ))
                .into());
            };

            let dsn = if let Some(dsn_str) = update_args.dsn {
                dsn_str
                    .parse::<Dsn>()
                    .context(ErrorMetadata::bad_request(
                        "InvalidSentryDsn",
                        "The Sentry DSN passed was invalid",
                    ))?
                    .into()
            } else {
                existing_config.dsn
            };

            let config = SentryConfig {
                dsn,
                tags: update_args.tags.unwrap_or(existing_config.tags),
                version: existing_config.version,
            };

            st.application
                .patch_integration_config(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    IntegrationConfig::Sentry(config),
                )
                .await?;
        },
        IntegrationConfig::PostHogLogs(existing_config) => {
            let UpdateIntegrationArgs::PostHogLogs(update_args) = args else {
                return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                    "LogStreamTypeMismatch",
                    "Cannot update a PostHog Logs log stream with arguments for a different log \
                     stream type",
                ))
                .into());
            };

            let host = update_args.host.unwrap_or(existing_config.host);
            if host.is_some() {
                validate_posthog_host(host.as_ref())?;
            }

            let topics = resolve_topics_update(update_args.topics, existing_config.topics)?;
            ensure_topic_entitlements(&st.application, &identity, &topics).await?;
            let config = PostHogLogsConfig {
                api_key: update_args
                    .api_key
                    .map(|k| k.into())
                    .unwrap_or(existing_config.api_key),
                host,
                service_name: update_args
                    .service_name
                    .unwrap_or(existing_config.service_name),
                topics,
            };

            st.application
                .patch_integration_config(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    IntegrationConfig::PostHogLogs(config),
                )
                .await?;
        },
        IntegrationConfig::PostHogErrorTracking(existing_config) => {
            let UpdateIntegrationArgs::PostHogErrorTracking(update_args) = args else {
                return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                    "LogStreamTypeMismatch",
                    "Cannot update a PostHog Error Tracking log stream with arguments for a \
                     different log stream type",
                ))
                .into());
            };

            let host = update_args.host.unwrap_or(existing_config.host);
            if host.is_some() {
                validate_posthog_host(host.as_ref())?;
            }

            let config = PostHogErrorTrackingConfig {
                api_key: update_args
                    .api_key
                    .map(|k| k.into())
                    .unwrap_or(existing_config.api_key),
                host,
            };

            st.application
                .patch_integration_config(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    IntegrationConfig::PostHogErrorTracking(config),
                )
                .await?;
        },
        IntegrationConfig::S3Export(existing_config) => {
            let UpdateIntegrationArgs::S3Export(update_args) = args else {
                return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                    "LogStreamTypeMismatch",
                    "Cannot update an S3 export integration with arguments for a different \
                     integration type",
                ))
                .into());
            };

            if let Some(selection) = &update_args.selection {
                validate_s3_selection(selection)?;
            }
            let config = S3ExportConfig {
                credentials: S3ExportCredentials {
                    role_arn: update_args
                        .role_arn
                        .unwrap_or(existing_config.credentials.role_arn.clone()),
                    external_id: existing_config.credentials.external_id.clone(),
                },
                selection: update_args
                    .selection
                    .unwrap_or(existing_config.selection.clone()),
                ..existing_config.clone()
            };
            let config = ensure_same_s3_destination(&existing_config, config)?;
            // The S3 export worker, not the log manager, sets this status: a
            // failure stays Failed until a page lands. New credentials start
            // over at Pending; other edits keep the status, so a tables-only
            // edit cannot hide a failure.
            let credentials_changed = config.credentials != existing_config.credentials;
            st.application
                .patch_integration_config_and_status(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    IntegrationConfig::S3Export(config),
                    credentials_changed,
                )
                .await?;
            return Ok(StatusCode::OK);
        },
        _ => {
            return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                "UnsupportedLogStreamType",
                "This log stream type does not support updates",
            ))
            .into())
        },
    }

    // Reset the integration status to Pending so that it retries verification
    st.application.reset_integration_to_pending(&id).await?;

    Ok(StatusCode::OK)
}

/// List log streams
///
/// List configs for all existing log streams in a deployment.
///
/// Deprecated: use `GET /integrations` instead.
#[utoipa::path(
    get,
    path = "/list_log_streams",
    tag = "Log Streams",
    responses((status = 200, body = Vec<IntegrationResponse>)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
#[deprecated = "Use `list_integrations`"]
pub async fn list_log_streams(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    list_integrations(MtState(st), ExtractIdentity(identity)).await
}

/// Get log stream
///
/// Get the config for a specific log stream by id.
///
/// Deprecated: use `GET /integrations/{id}` instead.
#[utoipa::path(
    get,
    path = "/get_log_stream/{id}",
    tag = "Log Streams",
    responses((status = 200, body = IntegrationResponse)),
    params(
        ("id" = String, Path, description = "id of the log stream to retrieve"),
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
#[deprecated = "Use `get_integration`"]
pub async fn get_log_stream(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpResponseError> {
    get_integration(MtState(st), ExtractIdentity(identity), Path(id)).await
}

/// Create log stream
///
/// Create a new log stream for the deployment. Errors if a log stream of the
/// given type already exists.
///
/// Deprecated: use `POST /integrations` instead.
#[utoipa::path(
    post,
    path = "/create_log_stream",
    tag = "Log Streams",
    responses((status = 200, body = CreateIntegrationResponse)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
#[deprecated = "Use `create_integration`"]
pub async fn create_log_stream(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(args): Json<CreateIntegrationArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    create_integration(
        MtState(st),
        ExtractIdentity(identity),
        ExtractRequestMetadata(request_metadata),
        Json(args),
    )
    .await
}

/// Update log stream
///
/// Update an existing log stream for the deployment. Omit a field to keep the
/// existing value, and use `null` to unset a field.
///
/// Deprecated: use `PATCH /integrations/{id}` instead.
#[utoipa::path(
    post,
    path = "/update_log_stream/{id}",
    tag = "Log Streams",
    responses((status = 200)),
    params(
        ("id" = String, Path, description = "id of the log stream to update"),
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
#[deprecated = "Use `update_integration`"]
pub async fn update_log_stream(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(id): Path<String>,
    Json(args): Json<UpdateIntegrationArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    update_integration(
        MtState(st),
        ExtractIdentity(identity),
        ExtractRequestMetadata(request_metadata),
        Path(id),
        Json(args),
    )
    .await
}

/// Delete log stream
///
/// Delete the deployment's log stream with the given id.
///
/// Deprecated: use `DELETE /integrations/{id}` instead.
#[utoipa::path(
    post,
    path = "/delete_log_stream/{id}",
    tag = "Log Streams",
    responses((status = 200)),
    params(
        ("id" = String, Path, description = "id of the log stream to delete"),
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
#[deprecated = "Use `delete_integration`"]
pub async fn delete_log_stream(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpResponseError> {
    delete_integration(
        MtState(st),
        ExtractIdentity(identity),
        ExtractRequestMetadata(request_metadata),
        Path(id),
    )
    .await
}

/// Rotate webhook log stream secret
///
/// Rotate the secret for the webhook log stream.
///
/// Deprecated: use `POST /integrations/{id}/rotate_webhook_secret` instead.
#[utoipa::path(
    post,
    path = "/rotate_webhook_secret/{id}",
    tag = "Log Streams",
    responses((status = 200, body = RotateIntegrationSecretResponse)),
    params(
        ("id" = String, Path, description = "id of the webhook log stream for which to rotate the secret"),
    ),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
#[deprecated = "Use `rotate_integration_webhook_secret`"]
pub async fn rotate_webhook_secret(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpResponseError> {
    rotate_integration_webhook_secret(
        MtState(st),
        ExtractIdentity(identity),
        ExtractRequestMetadata(request_metadata),
        Path(id),
    )
    .await
}

// Serves the deprecated log stream routes until callers move to
// `/integrations`.
#[allow(deprecated)]
pub fn platform_router<S>() -> OpenApiRouter<S>
where
    LocalAppState: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(rotate_integration_webhook_secret))
        .routes(utoipa_axum::routes!(delete_integration))
        .routes(utoipa_axum::routes!(update_integration))
        .routes(utoipa_axum::routes!(create_integration))
        .routes(utoipa_axum::routes!(get_integration))
        .routes(utoipa_axum::routes!(list_integrations))
        .routes(utoipa_axum::routes!(list_log_streams))
        .routes(utoipa_axum::routes!(get_log_stream))
        .routes(utoipa_axum::routes!(delete_log_stream))
        .routes(utoipa_axum::routes!(create_log_stream))
        .routes(utoipa_axum::routes!(update_log_stream))
        .routes(utoipa_axum::routes!(rotate_webhook_secret))
}
