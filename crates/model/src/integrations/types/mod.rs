use std::{
    collections::BTreeSet,
    fmt,
    str::FromStr,
};

use common::log_streaming::LogTopic;
use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

pub mod axiom;
pub mod datadog;
pub mod mock_integration;
pub mod posthog_error_tracking;
pub mod posthog_logs;
pub mod s3_export;
pub mod sentry;
pub mod webhook;

/// At most one integration per `IntegrationType`, so this bounds the number of
/// distinct integrations a deployment can configure at once.
pub const INTEGRATIONS_LIMIT: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct IntegrationDocument {
    pub status: IntegrationState,
    pub config: IntegrationConfig,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedIntegrationDocument {
    pub status: SerializedIntegrationState,
    pub config: SerializedIntegrationConfig,
}

impl TryFrom<IntegrationDocument> for SerializedIntegrationDocument {
    type Error = anyhow::Error;

    fn try_from(value: IntegrationDocument) -> Result<Self, Self::Error> {
        Ok(Self {
            status: value.status.into(),
            config: value.config.try_into()?,
        })
    }
}

impl TryFrom<SerializedIntegrationDocument> for IntegrationDocument {
    type Error = anyhow::Error;

    fn try_from(value: SerializedIntegrationDocument) -> Result<Self, Self::Error> {
        Ok(Self {
            status: value.status.into(),
            config: value.config.try_into()?,
        })
    }
}

codegen_convex_serialization!(IntegrationDocument, SerializedIntegrationDocument);

/// Serialize a set of subscribed topics for storage. `None` (subscribe to all
/// topics, including ones added in the future) is preserved as `None`.
pub(crate) fn serialize_topics(topics: Option<BTreeSet<LogTopic>>) -> Option<Vec<String>> {
    topics.map(|topics| topics.iter().map(|t| t.to_string()).collect())
}

/// Parse a stored set of subscribed topics back into `LogTopic`s.
pub(crate) fn deserialize_topics(
    topics: Option<Vec<String>>,
) -> anyhow::Result<Option<BTreeSet<LogTopic>>> {
    topics
        .map(|topics| {
            topics
                .iter()
                .map(|t| LogTopic::from_str(t).map_err(anyhow::Error::from))
                .collect::<anyhow::Result<BTreeSet<_>>>()
        })
        .transpose()
}

/// Integration lifecycle, including restart verification and asynchronous
/// removal:
/// ```text
/// +---------+
/// | Pending |
/// +---------+
///      |     |
///      |     v
///      |  +--------+
///      |  | Failed |
///      |  +--------+
///      v
/// +--------+          +------------+
/// | Active | -------> | Tombstoned | ---> Removed
/// +--------+          +------------+
///      |
///      | (on restart)
///      v
/// +------------+
/// | Restarting | -----> Active
/// +------------+
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum IntegrationState {
    Pending,
    Restarting,
    Failed { reason: String },
    Active, // TODO: add health statistics under Active
    Tombstoned,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum SerializedIntegrationState {
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

impl From<IntegrationState> for SerializedIntegrationState {
    fn from(value: IntegrationState) -> Self {
        match value {
            IntegrationState::Pending => SerializedIntegrationState::Pending,
            IntegrationState::Restarting => SerializedIntegrationState::Restarting,
            IntegrationState::Failed { reason } => SerializedIntegrationState::Failed { reason },
            IntegrationState::Active => SerializedIntegrationState::Active,
            IntegrationState::Tombstoned => SerializedIntegrationState::Tombstoned,
        }
    }
}

impl From<SerializedIntegrationState> for IntegrationState {
    fn from(value: SerializedIntegrationState) -> Self {
        match value {
            SerializedIntegrationState::Pending => IntegrationState::Pending,
            SerializedIntegrationState::Restarting => IntegrationState::Restarting,
            SerializedIntegrationState::Failed { reason } => IntegrationState::Failed { reason },
            SerializedIntegrationState::Active => IntegrationState::Active,
            SerializedIntegrationState::Tombstoned => IntegrationState::Tombstoned,
        }
    }
}

codegen_convex_serialization!(IntegrationState, SerializedIntegrationState);

/// The list of logging providers we support
/// This is different from IntegrationConfig in that this is just a C-style
/// enum as opposed to an ADT. This is meant to be used for the unsubscription
/// API and classifying integrations without having to specify a config.
///
/// DatadogV2 + AxiomV2 are the unlaunched versions of
/// https://www.notion.so/convex-dev/Log-streams-round-2-da990dc843e24e13b4a2051f51d0bb9c
/// They will eventually replace `Datadog` and `Axiom`
#[derive(
    Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Serialize, Deserialize, strum::IntoStaticStr,
)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum IntegrationType {
    Local,
    Datadog,
    #[strum(serialize = "datadog")]
    DatadogV2,
    Webhook,
    Axiom,
    #[strum(serialize = "axiom")]
    AxiomV2,
    Sentry,
    PostHogLogs,
    PostHogErrorTracking,
    S3Export,
}

impl IntegrationType {
    pub fn as_str(&self) -> &'static str {
        self.into()
    }
}

/// The configurations associated with each IntegrationType above.
/// Meant to be used for the subscription API.
#[derive(Debug, Clone, PartialEq)]
pub enum IntegrationConfig {
    Local(String),
    Datadog(datadog::DatadogConfig),
    Webhook(webhook::WebhookConfig),
    Axiom(axiom::AxiomConfig),
    Sentry(sentry::SentryConfig),
    PostHogLogs(posthog_logs::PostHogLogsConfig),
    PostHogErrorTracking(posthog_error_tracking::PostHogErrorTrackingConfig),
    S3Export(s3_export::S3ExportConfig),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum SerializedIntegrationConfig {
    Local {
        path: String,
    },
    Datadog(datadog::SerializedDatadogConfig),
    Webhook(webhook::SerializedWebhookConfig),
    Axiom(axiom::SerializedAxiomConfig),
    Sentry(sentry::SerializedSentryConfig),
    PostHogLogs(posthog_logs::SerializedPostHogLogsConfig),
    PostHogErrorTracking(posthog_error_tracking::SerializedPostHogErrorTrackingConfig),
    S3Export(s3_export::SerializedS3ExportConfig),
}

impl TryFrom<SerializedIntegrationConfig> for IntegrationConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedIntegrationConfig) -> Result<Self, Self::Error> {
        match value {
            SerializedIntegrationConfig::Local { path } => Ok(IntegrationConfig::Local(path)),
            SerializedIntegrationConfig::Datadog(config) => Ok(IntegrationConfig::Datadog(
                datadog::DatadogConfig::try_from(config)?,
            )),
            SerializedIntegrationConfig::Webhook(config) => Ok(IntegrationConfig::Webhook(
                webhook::WebhookConfig::try_from(config)?,
            )),
            SerializedIntegrationConfig::Axiom(config) => Ok(IntegrationConfig::Axiom(
                axiom::AxiomConfig::try_from(config)?,
            )),
            SerializedIntegrationConfig::Sentry(config) => Ok(IntegrationConfig::Sentry(
                sentry::SentryConfig::try_from(config)?,
            )),
            SerializedIntegrationConfig::PostHogLogs(config) => {
                Ok(IntegrationConfig::PostHogLogs(config.try_into()?))
            },
            SerializedIntegrationConfig::PostHogErrorTracking(config) => {
                Ok(IntegrationConfig::PostHogErrorTracking(config.into()))
            },
            SerializedIntegrationConfig::S3Export(config) => {
                Ok(IntegrationConfig::S3Export(config.try_into()?))
            },
        }
    }
}

impl TryFrom<IntegrationConfig> for SerializedIntegrationConfig {
    type Error = anyhow::Error;

    fn try_from(value: IntegrationConfig) -> Result<Self, Self::Error> {
        match value {
            IntegrationConfig::Local(path) => Ok(SerializedIntegrationConfig::Local { path }),
            IntegrationConfig::Datadog(config) => Ok(SerializedIntegrationConfig::Datadog(
                datadog::SerializedDatadogConfig::from(config),
            )),
            IntegrationConfig::Webhook(config) => Ok(SerializedIntegrationConfig::Webhook(
                webhook::SerializedWebhookConfig::from(config),
            )),
            IntegrationConfig::Axiom(config) => Ok(SerializedIntegrationConfig::Axiom(
                axiom::SerializedAxiomConfig::from(config),
            )),
            IntegrationConfig::Sentry(config) => Ok(SerializedIntegrationConfig::Sentry(
                sentry::SerializedSentryConfig::try_from(config)?,
            )),
            IntegrationConfig::PostHogLogs(config) => {
                Ok(SerializedIntegrationConfig::PostHogLogs(config.into()))
            },
            IntegrationConfig::PostHogErrorTracking(config) => Ok(
                SerializedIntegrationConfig::PostHogErrorTracking(config.into()),
            ),
            IntegrationConfig::S3Export(config) => {
                Ok(SerializedIntegrationConfig::S3Export(config.into()))
            },
        }
    }
}

codegen_convex_serialization!(IntegrationConfig, SerializedIntegrationConfig);

impl fmt::Display for IntegrationConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(config) => write!(f, "Local({config})"),
            Self::Datadog(config) => write!(f, "Datadog({config})"),
            Self::Webhook(config) => write!(f, "Webhook({config})"),
            Self::Axiom(config) => write!(f, "Axiom({config})"),
            Self::Sentry(config) => write!(f, "Sentry({config})"),
            Self::PostHogLogs(config) => write!(f, "PostHogLogs({config})"),
            Self::PostHogErrorTracking(config) => write!(f, "PostHogErrorTracking({config})"),
            Self::S3Export(config) => write!(f, "S3Export({config})"),
        }
    }
}

impl IntegrationConfig {
    pub fn integration_type(&self) -> IntegrationType {
        match self {
            Self::Local(_) => IntegrationType::Local,
            Self::Datadog(_) => IntegrationType::Datadog,
            Self::Webhook(_) => IntegrationType::Webhook,
            Self::Axiom(_) => IntegrationType::Axiom,
            Self::Sentry(_) => IntegrationType::Sentry,
            Self::PostHogLogs(_) => IntegrationType::PostHogLogs,
            Self::PostHogErrorTracking(_) => IntegrationType::PostHogErrorTracking,
            Self::S3Export(_) => IntegrationType::S3Export,
        }
    }

    /// The set of topics this integration is subscribed to. `Some(None)` for
    /// subscriptions to all topics, and `None` for integration types that do
    /// not support topic subscriptions.
    pub fn topics_mut(&mut self) -> Option<&mut Option<BTreeSet<LogTopic>>> {
        match self {
            Self::Datadog(config) => Some(&mut config.topics),
            Self::Webhook(config) => Some(&mut config.topics),
            Self::Axiom(config) => Some(&mut config.topics),
            Self::PostHogLogs(config) => Some(&mut config.topics),
            Self::Local(_)
            | Self::Sentry(_)
            | Self::PostHogErrorTracking(_)
            | Self::S3Export(_) => None,
        }
    }
}
