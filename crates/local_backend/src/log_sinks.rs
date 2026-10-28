use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    net::Ipv4Addr,
};

use anyhow::Context;
use application::{
    log_streaming::LogSinkWithId,
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
use model::log_sinks::types::{
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
        SyncPeriod,
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
    SinkConfig,
    SinkType,
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
pub enum LogStreamStatus {
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

impl From<model::log_sinks::types::SerializedSinkState> for LogStreamStatus {
    fn from(value: model::log_sinks::types::SerializedSinkState) -> Self {
        use model::log_sinks::types::SerializedSinkState;
        match value {
            SerializedSinkState::Pending => LogStreamStatus::Pending,
            SerializedSinkState::Restarting => LogStreamStatus::Restarting,
            SerializedSinkState::Failed { reason } => LogStreamStatus::Failed { reason },
            SerializedSinkState::Active => LogStreamStatus::Active,
            SerializedSinkState::Tombstoned => LogStreamStatus::Tombstoned,
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
enum LogStreamType {
    Datadog,
    Webhook,
    Axiom,
    Sentry,
    PostHogLogs,
    PostHogErrorTracking,
}

impl From<LogStreamType> for SinkType {
    fn from(log_stream_type: LogStreamType) -> Self {
        match log_stream_type {
            LogStreamType::Datadog => SinkType::Datadog,
            LogStreamType::Webhook => SinkType::Webhook,
            LogStreamType::Axiom => SinkType::Axiom,
            LogStreamType::Sentry => SinkType::Sentry,
            LogStreamType::PostHogLogs => SinkType::PostHogLogs,
            LogStreamType::PostHogErrorTracking => SinkType::PostHogErrorTracking,
        }
    }
}

/// Analytics export destinations are gated on the streaming export entitlement;
/// every other sink type streams logs and is gated on log streaming.
async fn ensure_integration_allowed(
    application: &Application<ProdRuntime>,
    identity: &Identity,
    sink_type: &SinkType,
) -> anyhow::Result<()> {
    match sink_type {
        SinkType::S3Export => {
            application
                .ensure_streaming_export_enabled(identity.clone())
                .await
        },
        SinkType::Local
        | SinkType::Datadog
        | SinkType::DatadogV2
        | SinkType::Webhook
        | SinkType::Axiom
        | SinkType::AxiomV2
        | SinkType::Sentry
        | SinkType::PostHogLogs
        | SinkType::PostHogErrorTracking => {
            application
                .ensure_log_streaming_allowed(identity.clone())
                .await
        },
    }
}

/// Delete log stream
///
/// Delete the deployment's log stream with the given id.
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
pub async fn delete_log_stream(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteIntegrations)?;
    let sink = st.application.must_get_log_sink_by_id(&id).await?;
    ensure_integration_allowed(&st.application, &identity, &sink.config.sink_type()).await?;

    st.application
        .remove_log_sink_by_id(identity, request_metadata, id)
        .await?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDatadogLogStreamArgs {
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

impl TryFrom<CreateDatadogLogStreamArgs> for DatadogConfig {
    type Error = anyhow::Error;

    fn try_from(value: CreateDatadogLogStreamArgs) -> Result<Self, Self::Error> {
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
pub struct CreateWebhookLogStreamArgs {
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
pub struct CreateAxiomLogStreamArgs {
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

impl TryFrom<CreateAxiomLogStreamArgs> for AxiomConfig {
    type Error = anyhow::Error;

    fn try_from(value: CreateAxiomLogStreamArgs) -> Result<Self, Self::Error> {
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
pub struct CreateSentryLogStreamArgs {
    /// Sentry Data Source Name (DSN) to route exceptions to.
    dsn: String,
    /// Tags to add to all events routed to Sentry.
    #[schema(value_type = Option<BTreeMap<String, String>>)]
    tags: Option<BTreeMap<FieldName, String>>,
}

impl TryFrom<CreateSentryLogStreamArgs> for SentryConfig {
    type Error = anyhow::Error;

    fn try_from(value: CreateSentryLogStreamArgs) -> Result<Self, Self::Error> {
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
pub struct CreatePostHogLogsLogStreamArgs {
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

impl TryFrom<CreatePostHogLogsLogStreamArgs> for PostHogLogsConfig {
    type Error = anyhow::Error;

    fn try_from(value: CreatePostHogLogsLogStreamArgs) -> Result<Self, Self::Error> {
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
pub struct CreatePostHogErrorTrackingLogStreamArgs {
    /// PostHog project token.
    api_key: String,
    /// PostHog host URL. Defaults to https://us.i.posthog.com.
    host: Option<String>,
}

impl TryFrom<CreatePostHogErrorTrackingLogStreamArgs> for PostHogErrorTrackingConfig {
    type Error = anyhow::Error;

    fn try_from(value: CreatePostHogErrorTrackingLogStreamArgs) -> Result<Self, Self::Error> {
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
            "An S3 export's bucket, region, prefix, and credential type can't change. Delete the \
             export and create a new one instead.",
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
#[serde(rename_all = "camelCase")]
pub struct CreateS3ExportLogStreamArgs {
    /// Name of the S3 bucket to mirror into.
    bucket: String,
    /// AWS region the bucket lives in, e.g. `us-east-1`.
    region: String,
    /// Key prefix within the bucket. Omit to write at the bucket root.
    #[serde(default)]
    prefix: Option<String>,
    /// Access key of an IAM user that can write to the bucket. Send both key
    /// fields, or `roleArn` instead.
    // TODO(ENG-11116): drop the access key fields once existing exports have
    // moved to roles, so an export authenticates with a role only.
    #[serde(default)]
    access_key_id: Option<String>,
    #[serde(default)]
    secret_access_key: Option<String>,
    /// ARN of a role in the bucket's account that Convex assumes to write.
    /// Send an empty string to create the export first: the response carries
    /// the `roleExternalId` the role's trust policy needs, and the export waits
    /// until an update supplies the ARN.
    #[serde(default)]
    role_arn: Option<String>,
    /// The components and tables to export. Defaults to everything. Each table
    /// exports whole documents, so filtering fields within a table is rejected.
    #[serde(default)]
    selection: Option<Selection>,
    /// How often the mirror is refreshed.
    period: SyncPeriod,
}

impl TryFrom<CreateS3ExportLogStreamArgs> for S3ExportConfig {
    type Error = anyhow::Error;

    fn try_from(value: CreateS3ExportLogStreamArgs) -> Result<Self, Self::Error> {
        validate_s3_bucket(&value.bucket)?;
        validate_s3_prefix(value.prefix.as_deref().unwrap_or_default())?;
        let selection = value.selection.unwrap_or_default();
        validate_s3_selection(&selection)?;
        let credentials = match (value.access_key_id, value.secret_access_key, value.role_arn) {
            (Some(access_key_id), Some(secret_access_key), None) => {
                S3ExportCredentials::AccessKey {
                    access_key_id: access_key_id.into(),
                    secret_access_key: secret_access_key.into(),
                }
            },
            (None, None, Some(role_arn)) => S3ExportCredentials::Role {
                role_arn,
                external_id: S3ExportCredentials::new_external_id(),
            },
            _ => anyhow::bail!(ErrorMetadata::bad_request(
                "S3ExportCredentialsRequired",
                "Send `accessKeyId` with `secretAccessKey`, or `roleArn` on its own.",
            )),
        };
        Ok(Self {
            bucket: value.bucket,
            region: value.region,
            prefix: value.prefix,
            credentials,
            selection,
            period: value.period,
        })
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "logStreamType")]
pub enum CreateLogStreamArgs {
    #[schema(title = "Datadog")]
    Datadog(CreateDatadogLogStreamArgs),
    #[schema(title = "Webhook")]
    Webhook(CreateWebhookLogStreamArgs),
    #[schema(title = "Axiom")]
    Axiom(CreateAxiomLogStreamArgs),
    #[schema(title = "Sentry")]
    Sentry(CreateSentryLogStreamArgs),
    #[schema(title = "PostHogLogs")]
    PostHogLogs(CreatePostHogLogsLogStreamArgs),
    #[schema(title = "PostHogErrorTracking")]
    PostHogErrorTracking(CreatePostHogErrorTrackingLogStreamArgs),
    #[schema(title = "S3Export")]
    S3Export(CreateS3ExportLogStreamArgs),
}

impl CreateLogStreamArgs {
    fn sink_type(&self) -> SinkType {
        match self {
            Self::Datadog(_) => SinkType::Datadog,
            Self::Webhook(_) => SinkType::Webhook,
            Self::Axiom(_) => SinkType::Axiom,
            Self::Sentry(_) => SinkType::Sentry,
            Self::PostHogLogs(_) => SinkType::PostHogLogs,
            Self::PostHogErrorTracking(_) => SinkType::PostHogErrorTracking,
            Self::S3Export(_) => SinkType::S3Export,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookLogStreamResponse {
    id: String,
    /// Use this secret to verify webhook signatures.
    hmac_secret: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "logStreamType")]
pub enum CreateLogStreamResponse {
    #[schema(title = "Webhook")]
    Webhook(CreateWebhookLogStreamResponse),
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
        /// Set for role exports. The role's trust policy must require it as
        /// `sts:ExternalId`.
        #[serde(skip_serializing_if = "Option::is_none")]
        role_external_id: Option<String>,
    },
}

async fn ensure_log_sink_does_not_exist(
    application: &Application<ProdRuntime>,
    sink_type: &SinkType,
) -> Result<(), HttpResponseError> {
    if application.get_log_sink(sink_type).await?.is_some() {
        return Err(anyhow::anyhow!(ErrorMetadata::conflict(
            "LogStreamAlreadyExists",
            format!("{sink_type:?} log stream already exists for this deployment",)
        ))
        .into());
    }
    Ok(())
}

/// Create log stream
///
/// Create a new log stream for the deployment. Errors if a log stream of the
/// given type already exists.
#[utoipa::path(
    post,
    path = "/create_log_stream",
    tag = "Log Streams",
    responses((status = 200, body = CreateLogStreamResponse)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn create_log_stream(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Json(args): Json<CreateLogStreamArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteIntegrations)?;
    if matches!(&args, CreateLogStreamArgs::S3Export(_)) {
        identity.require_operation(keybroker::DeploymentOp::ViewData)?;
    }
    ensure_integration_allowed(&st.application, &identity, &args.sink_type()).await?;

    match args {
        CreateLogStreamArgs::Datadog(datadog_sink_post_args) => {
            ensure_log_sink_does_not_exist(&st.application, &SinkType::Datadog).await?;

            let config: DatadogConfig = datadog_sink_post_args.try_into()?;
            ensure_topic_entitlements(&st.application, &identity, &config.topics).await?;
            let id = st
                .application
                .add_log_sink(
                    identity.clone(),
                    request_metadata.clone(),
                    SinkConfig::Datadog(config),
                )
                .await?;
            Ok(Json(CreateLogStreamResponse::Datadog {
                id: id.to_string(),
            }))
        },
        CreateLogStreamArgs::Webhook(webhook_sink_post_args) => {
            ensure_log_sink_does_not_exist(&st.application, &SinkType::Webhook).await?;

            let hmac_secret = generate_webhook_hmac_secret(st.application.runtime());

            let url = webhook_sink_post_args.url.parse().map_err(|_| {
                anyhow::anyhow!(ErrorMetadata::bad_request(
                    "InvalidWebhookUrl",
                    "The URL passed was invalid"
                ))
            })?;

            let topics = validate_subscribed_topics(webhook_sink_post_args.topics)?;
            ensure_topic_entitlements(&st.application, &identity, &topics).await?;
            let config = WebhookConfig {
                url,
                format: webhook_sink_post_args.format,
                hmac_secret: hmac_secret.clone(),
                topics,
            };
            let id = st
                .application
                .add_log_sink(
                    identity.clone(),
                    request_metadata.clone(),
                    SinkConfig::Webhook(config),
                )
                .await?;

            Ok(Json(CreateLogStreamResponse::Webhook(
                CreateWebhookLogStreamResponse {
                    hmac_secret,
                    id: id.to_string(),
                },
            )))
        },
        CreateLogStreamArgs::Axiom(axiom_sink_post_args) => {
            ensure_log_sink_does_not_exist(&st.application, &SinkType::Axiom).await?;

            if axiom_sink_post_args.attributes.len() > *AXIOM_MAX_ATTRIBUTES {
                return Err(anyhow::anyhow!(
                    "Exceeded max number of Axiom attributes. Contact support@convex.dev to \
                     request a limit increase."
                )
                .into());
            }

            let config: AxiomConfig = axiom_sink_post_args.try_into()?;
            ensure_topic_entitlements(&st.application, &identity, &config.topics).await?;
            let id = st
                .application
                .add_log_sink(
                    identity.clone(),
                    request_metadata.clone(),
                    SinkConfig::Axiom(config),
                )
                .await?;

            Ok(Json(CreateLogStreamResponse::Axiom { id: id.to_string() }))
        },
        CreateLogStreamArgs::Sentry(sentry_config_args) => {
            ensure_log_sink_does_not_exist(&st.application, &SinkType::Sentry).await?;

            let config = sentry_config_args.try_into()?;
            let id = st
                .application
                .add_log_sink(
                    identity.clone(),
                    request_metadata.clone(),
                    SinkConfig::Sentry(config),
                )
                .await?;
            Ok(Json(CreateLogStreamResponse::Sentry { id: id.to_string() }))
        },
        CreateLogStreamArgs::PostHogLogs(args) => {
            ensure_log_sink_does_not_exist(&st.application, &SinkType::PostHogLogs).await?;

            let config: PostHogLogsConfig = args.try_into()?;
            ensure_topic_entitlements(&st.application, &identity, &config.topics).await?;
            let id = st
                .application
                .add_log_sink(
                    identity.clone(),
                    request_metadata.clone(),
                    SinkConfig::PostHogLogs(config),
                )
                .await?;
            Ok(Json(CreateLogStreamResponse::PostHogLogs {
                id: id.to_string(),
            }))
        },
        CreateLogStreamArgs::PostHogErrorTracking(args) => {
            ensure_log_sink_does_not_exist(&st.application, &SinkType::PostHogErrorTracking)
                .await?;

            let config: PostHogErrorTrackingConfig = args.try_into()?;
            let id = st
                .application
                .add_log_sink(
                    identity.clone(),
                    request_metadata.clone(),
                    SinkConfig::PostHogErrorTracking(config),
                )
                .await?;
            Ok(Json(CreateLogStreamResponse::PostHogErrorTracking {
                id: id.to_string(),
            }))
        },
        CreateLogStreamArgs::S3Export(args) => {
            ensure_log_sink_does_not_exist(&st.application, &SinkType::S3Export).await?;

            let config: S3ExportConfig = args.try_into()?;
            let role_external_id = match &config.credentials {
                S3ExportCredentials::Role { external_id, .. } => Some(external_id.clone()),
                S3ExportCredentials::AccessKey { .. } => None,
            };
            let id = st
                .application
                .add_log_sink(
                    identity.clone(),
                    request_metadata.clone(),
                    SinkConfig::S3Export(config),
                )
                .await?;
            Ok(Json(CreateLogStreamResponse::S3Export {
                id: id.to_string(),
                role_external_id,
            }))
        },
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "logStreamType")]
pub enum RotateLogStreamSecretArgs {
    Webhook,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "logStreamType")]
pub enum RotateLogStreamSecretResponse {
    #[serde(rename_all = "camelCase")]
    #[schema(title = "Webhook")]
    Webhook { hmac_secret: String },
}

/// Rotate webhook log stream secret
///
/// Rotate the secret for the webhook log stream.
#[utoipa::path(
    post,
    path = "/rotate_webhook_secret/{id}",
    tag = "Log Streams",
    responses((status = 200, body = RotateLogStreamSecretResponse)),
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
pub async fn rotate_webhook_secret(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteIntegrations)?;
    st.application
        .ensure_log_streaming_allowed(identity.clone())
        .await?;

    let LogSinkWithId {
        config: sink_config,
        ..
    } = st.application.must_get_log_sink_by_id(&id).await?;

    match sink_config {
        SinkConfig::Webhook(existing_webhook_sink) => {
            let hmac_secret = generate_webhook_hmac_secret(st.application.runtime());

            let config = WebhookConfig {
                url: existing_webhook_sink.url,
                format: existing_webhook_sink.format,
                hmac_secret: hmac_secret.clone(),
                topics: existing_webhook_sink.topics,
            };
            st.application
                .patch_log_sink_config(identity, request_metadata, &id, SinkConfig::Webhook(config))
                .await?;

            Ok(Json(RotateLogStreamSecretResponse::Webhook { hmac_secret }))
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
enum LogStreamConfig {
    #[schema(title = "Datadog")]
    Datadog(DatadogLogStreamConfig),
    #[schema(title = "Webhook")]
    Webhook(WebhookLogStreamConfig),
    #[schema(title = "Axiom")]
    Axiom(AxiomLogStreamConfig),
    #[schema(title = "Sentry")]
    Sentry(SentryLogStreamConfig),
    #[schema(title = "PostHogLogs")]
    PostHogLogs(PostHogLogsLogStreamConfig),
    #[schema(title = "PostHogErrorTracking")]
    PostHogErrorTracking(PostHogErrorTrackingLogStreamConfig),
    #[schema(title = "S3Export")]
    S3Export(S3ExportLogStreamConfig),
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(title = "DatadogConfig")]
pub struct DatadogLogStreamConfig {
    pub id: String,
    /// Status of the log stream
    pub status: LogStreamStatus,
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
pub struct WebhookLogStreamConfig {
    pub id: String,
    /// Status of the log stream
    pub status: LogStreamStatus,
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
pub struct AxiomLogStreamConfig {
    pub id: String,
    /// Status of the log stream
    pub status: LogStreamStatus,
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
pub struct SentryLogStreamConfig {
    pub id: String,
    /// Status of the log stream
    pub status: LogStreamStatus,
    /// Tags to add to all events routed to Sentry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<BTreeMap<String, String>>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(title = "PostHogLogsConfig")]
pub struct PostHogLogsLogStreamConfig {
    pub id: String,
    /// Status of the log stream
    pub status: LogStreamStatus,
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
pub struct PostHogErrorTrackingLogStreamConfig {
    pub id: String,
    /// Status of the log stream
    pub status: LogStreamStatus,
    /// PostHog host URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(title = "S3ExportConfig")]
pub struct S3ExportLogStreamConfig {
    pub id: String,
    /// Status of the integration
    pub status: LogStreamStatus,
    /// Name of the S3 bucket being mirrored into.
    pub bucket: String,
    /// AWS region the bucket lives in.
    pub region: String,
    /// Key prefix within the bucket.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// AWS access key ID used to write to the bucket. The matching secret
    /// access key is write-only and is never returned. Absent for role exports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key_id: Option<String>,
    /// ARN of the role Convex assumes to write. Empty until the admin pastes
    /// it. Absent for access-key exports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_arn: Option<String>,
    /// Value the role's trust policy must require as `sts:ExternalId`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_external_id: Option<String>,
    /// The components and tables being exported.
    pub selection: Selection,
    /// How often the mirror is refreshed.
    pub period: SyncPeriod,
}

fn log_sink_to_log_stream_config(sink: LogSinkWithId) -> Option<LogStreamConfig> {
    let status: LogStreamStatus =
        Into::<model::log_sinks::types::SerializedSinkState>::into(sink.status).into();
    match sink.config {
        SinkConfig::Datadog(config) => Some(LogStreamConfig::Datadog(DatadogLogStreamConfig {
            id: sink.id.to_string(),
            status,
            site_location: config.site_location,
            dd_tags: config.dd_tags,
            service: config.service,
            topics: config.topics.map(|topics| topics.into_iter().collect()),
        })),
        SinkConfig::Webhook(config) => Some(LogStreamConfig::Webhook(WebhookLogStreamConfig {
            id: sink.id.to_string(),
            status,
            url: config.url.to_string(),
            format: config.format,
            hmac_secret: config.hmac_secret,
            topics: config.topics.map(|topics| topics.into_iter().collect()),
        })),
        SinkConfig::Axiom(config) => Some(LogStreamConfig::Axiom(AxiomLogStreamConfig {
            id: sink.id.to_string(),
            status,
            dataset_name: config.dataset_name,
            attributes: config.attributes,
            ingest_url: config.ingest_url,
            topics: config.topics.map(|topics| topics.into_iter().collect()),
        })),
        SinkConfig::Sentry(config) => Some(LogStreamConfig::Sentry(SentryLogStreamConfig {
            id: sink.id.to_string(),
            status,
            tags: config
                .tags
                .map(|tags| tags.into_iter().map(|(k, v)| (k.into(), v)).collect()),
        })),
        SinkConfig::PostHogLogs(config) => {
            Some(LogStreamConfig::PostHogLogs(PostHogLogsLogStreamConfig {
                id: sink.id.to_string(),
                status,
                host: config.host,
                service_name: config.service_name,
                topics: config.topics.map(|topics| topics.into_iter().collect()),
            }))
        },
        SinkConfig::PostHogErrorTracking(config) => Some(LogStreamConfig::PostHogErrorTracking(
            PostHogErrorTrackingLogStreamConfig {
                id: sink.id.to_string(),
                status,
                host: config.host,
            },
        )),
        SinkConfig::S3Export(config) => {
            let (access_key_id, role_arn, role_external_id) = match config.credentials {
                S3ExportCredentials::AccessKey { access_key_id, .. } => {
                    (Some(access_key_id.into_value()), None, None)
                },
                S3ExportCredentials::Role {
                    role_arn,
                    external_id,
                } => (None, Some(role_arn), Some(external_id)),
            };
            Some(LogStreamConfig::S3Export(S3ExportLogStreamConfig {
                id: sink.id.to_string(),
                status,
                bucket: config.bucket,
                region: config.region,
                prefix: config.prefix,
                access_key_id,
                role_arn,
                role_external_id,
                selection: config.selection,
                period: config.period,
            }))
        },
        _ => None,
    }
}

/// List log streams
///
/// List configs for all existing log streams in a deployment.
#[utoipa::path(
    get,
    path = "/list_log_streams",
    tag = "Log Streams",
    responses((status = 200, body = Vec<LogStreamConfig>)),
    security(
        ("Deploy Key" = []),
        ("OAuth Team Token" = []),
        ("Team Token" = []),
        ("OAuth Project Token" = []),
    ),
)]
pub async fn list_log_streams(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ViewIntegrations)?;

    Ok(Json(
        st.application
            .list_log_sinks()
            .await?
            .into_iter()
            .filter_map(log_sink_to_log_stream_config)
            .collect::<Vec<LogStreamConfig>>(),
    ))
}

/// Get log stream
///
/// Get the config for a specific log stream by id.
#[utoipa::path(
    get,
    path = "/get_log_stream/{id}",
    tag = "Log Streams",
    responses((status = 200, body = LogStreamConfig)),
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
pub async fn get_log_stream(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::ViewIntegrations)?;

    let log_sink_with_id = st.application.must_get_log_sink_by_id(&id).await?;

    let config = log_sink_to_log_stream_config(log_sink_with_id).ok_or_else(|| {
        anyhow::anyhow!(ErrorMetadata::bad_request(
            "UnsupportedLogStreamType",
            "This log stream type is not supported",
        ))
    })?;

    Ok(Json(config))
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDatadogSinkArgs {
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
pub struct UpdateWebhookSinkArgs {
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
pub struct UpdateAxiomSinkArgs {
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
pub struct UpdateSentrySinkArgs {
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
pub struct UpdatePostHogLogsSinkArgs {
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
pub struct UpdatePostHogErrorTrackingSinkArgs {
    /// PostHog project token.
    #[serde(default)]
    api_key: Option<String>,
    /// PostHog host URL.
    #[serde(default, with = "::serde_with::rust::double_option")]
    host: Option<Option<String>>,
}

/// Destination and AWS access key ID are fixed at creation.
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateS3ExportSinkArgs {
    /// A new secret for the export's access key ID. Access-key exports only.
    #[serde(default)]
    secret_access_key: Option<String>,
    /// The role ARN from the customer's stack. Role exports only; sets or
    /// replaces the ARN, within the same AWS account.
    #[serde(default)]
    role_arn: Option<String>,
    /// The components and tables to export. Each table exports whole
    /// documents, so filtering fields within a table is rejected.
    #[serde(default)]
    selection: Option<Selection>,
    /// How often the mirror is refreshed.
    #[serde(default)]
    period: Option<SyncPeriod>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "logStreamType")]
pub enum UpdateLogStreamArgs {
    #[schema(title = "Datadog")]
    Datadog(UpdateDatadogSinkArgs),
    #[schema(title = "Webhook")]
    Webhook(UpdateWebhookSinkArgs),
    #[schema(title = "Axiom")]
    Axiom(UpdateAxiomSinkArgs),
    #[schema(title = "Sentry")]
    Sentry(UpdateSentrySinkArgs),
    #[schema(title = "PostHogLogs")]
    PostHogLogs(UpdatePostHogLogsSinkArgs),
    #[schema(title = "PostHogErrorTracking")]
    PostHogErrorTracking(UpdatePostHogErrorTrackingSinkArgs),
    #[schema(title = "S3Export")]
    S3Export(UpdateS3ExportSinkArgs),
}

/// Update log stream
///
/// Update an existing log stream for the deployment. Omit a field to keep the
/// existing value, and use `null` to unset a field.
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
pub async fn update_log_stream(
    MtState(st): MtState<LocalAppState>,
    ExtractIdentity(identity): ExtractIdentity,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    Path(id): Path<String>,
    Json(args): Json<UpdateLogStreamArgs>,
) -> Result<impl IntoResponse, HttpResponseError> {
    identity.require_operation(keybroker::DeploymentOp::WriteIntegrations)?;

    let LogSinkWithId {
        config: sink_config,
        ..
    } = st.application.must_get_log_sink_by_id(&id).await?;
    if matches!(&sink_config, SinkConfig::S3Export(_)) {
        identity.require_operation(keybroker::DeploymentOp::ViewData)?;
    }
    ensure_integration_allowed(&st.application, &identity, &sink_config.sink_type()).await?;

    match sink_config {
        SinkConfig::Datadog(existing_config) => {
            let UpdateLogStreamArgs::Datadog(update_args) = args else {
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
                .patch_log_sink_config(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    SinkConfig::Datadog(config),
                )
                .await?;
        },
        SinkConfig::Webhook(existing_config) => {
            let UpdateLogStreamArgs::Webhook(update_args) = args else {
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
                .patch_log_sink_config(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    SinkConfig::Webhook(config),
                )
                .await?;
        },
        SinkConfig::Axiom(existing_config) => {
            let UpdateLogStreamArgs::Axiom(update_args) = args else {
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
                .patch_log_sink_config(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    SinkConfig::Axiom(config),
                )
                .await?;
        },
        SinkConfig::Sentry(existing_config) => {
            let UpdateLogStreamArgs::Sentry(update_args) = args else {
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
                .patch_log_sink_config(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    SinkConfig::Sentry(config),
                )
                .await?;
        },
        SinkConfig::PostHogLogs(existing_config) => {
            let UpdateLogStreamArgs::PostHogLogs(update_args) = args else {
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
                .patch_log_sink_config(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    SinkConfig::PostHogLogs(config),
                )
                .await?;
        },
        SinkConfig::PostHogErrorTracking(existing_config) => {
            let UpdateLogStreamArgs::PostHogErrorTracking(update_args) = args else {
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
                .patch_log_sink_config(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    SinkConfig::PostHogErrorTracking(config),
                )
                .await?;
        },
        SinkConfig::S3Export(existing_config) => {
            let UpdateLogStreamArgs::S3Export(update_args) = args else {
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
            let credentials = match (
                existing_config.credentials.clone(),
                update_args.secret_access_key,
                update_args.role_arn,
            ) {
                (credentials, None, None) => credentials,
                (S3ExportCredentials::AccessKey { access_key_id, .. }, Some(secret), None) => {
                    S3ExportCredentials::AccessKey {
                        access_key_id,
                        secret_access_key: secret.into(),
                    }
                },
                (S3ExportCredentials::Role { external_id, .. }, None, Some(role_arn)) => {
                    S3ExportCredentials::Role {
                        role_arn,
                        external_id,
                    }
                },
                _ => {
                    return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
                        "S3ExportCredentialTypeMismatch",
                        "Send `secretAccessKey` for an access-key export or `roleArn` for a role \
                         export, not the other.",
                    ))
                    .into());
                },
            };
            let config = S3ExportConfig {
                credentials,
                selection: update_args
                    .selection
                    .unwrap_or(existing_config.selection.clone()),
                period: update_args.period.unwrap_or(existing_config.period),
                ..existing_config.clone()
            };
            let config = ensure_same_s3_destination(&existing_config, config)?;
            // The S3 export worker, not the log manager, sets this status: a
            // failure stays Failed until a page lands. New credentials start
            // over at Pending; other edits keep the status, so a tables-only
            // edit cannot hide a failure.
            let credentials_changed = config.credentials != existing_config.credentials;
            st.application
                .patch_log_sink_config_and_status(
                    identity.clone(),
                    request_metadata.clone(),
                    &id,
                    SinkConfig::S3Export(config),
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

    // Reset the log sink status to Pending so that it retries verification
    st.application.reset_log_sink_to_pending(&id).await?;

    Ok(StatusCode::OK)
}

pub fn platform_router<S>() -> OpenApiRouter<S>
where
    LocalAppState: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(list_log_streams))
        .routes(utoipa_axum::routes!(get_log_stream))
        .routes(utoipa_axum::routes!(delete_log_stream))
        .routes(utoipa_axum::routes!(create_log_stream))
        .routes(utoipa_axum::routes!(update_log_stream))
        .routes(utoipa_axum::routes!(rotate_webhook_secret))
}
