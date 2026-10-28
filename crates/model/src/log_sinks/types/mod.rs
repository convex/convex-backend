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
pub mod mock_sink;
pub mod posthog_error_tracking;
pub mod posthog_logs;
pub mod s3_export;
pub mod sentry;
pub mod webhook;

/// Constants/Limits
/// At most one sink per `SinkType`, so this bounds the number of distinct
/// integrations a deployment can configure at once.
pub const LOG_SINKS_LIMIT: usize = 8;

/// Data model for an entry in the LOG_SINKS_TABLE
#[derive(Debug, Clone, PartialEq)]
pub struct LogSinksRow {
    pub status: SinkState,
    pub config: SinkConfig,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedLogSinksRow {
    pub status: SerializedSinkState,
    pub config: SerializedSinkConfig,
}

impl TryFrom<LogSinksRow> for SerializedLogSinksRow {
    type Error = anyhow::Error;

    fn try_from(value: LogSinksRow) -> Result<Self, Self::Error> {
        Ok(Self {
            status: value.status.into(),
            config: value.config.try_into()?,
        })
    }
}

impl TryFrom<SerializedLogSinksRow> for LogSinksRow {
    type Error = anyhow::Error;

    fn try_from(value: SerializedLogSinksRow) -> Result<Self, Self::Error> {
        Ok(Self {
            status: value.status.into(),
            config: value.config.try_into()?,
        })
    }
}

codegen_convex_serialization!(LogSinksRow, SerializedLogSinksRow);

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

/// Status of a configured LogSink
/// LogSink SinkState state machine:
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
pub enum SinkState {
    Pending,
    Restarting,
    Failed { reason: String },
    Active, // TODO: add health statistics under Active
    Tombstoned,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum SerializedSinkState {
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

impl From<SinkState> for SerializedSinkState {
    fn from(value: SinkState) -> Self {
        match value {
            SinkState::Pending => SerializedSinkState::Pending,
            SinkState::Restarting => SerializedSinkState::Restarting,
            SinkState::Failed { reason } => SerializedSinkState::Failed { reason },
            SinkState::Active => SerializedSinkState::Active,
            SinkState::Tombstoned => SerializedSinkState::Tombstoned,
        }
    }
}

impl From<SerializedSinkState> for SinkState {
    fn from(value: SerializedSinkState) -> Self {
        match value {
            SerializedSinkState::Pending => SinkState::Pending,
            SerializedSinkState::Restarting => SinkState::Restarting,
            SerializedSinkState::Failed { reason } => SinkState::Failed { reason },
            SerializedSinkState::Active => SinkState::Active,
            SerializedSinkState::Tombstoned => SinkState::Tombstoned,
        }
    }
}

codegen_convex_serialization!(SinkState, SerializedSinkState);

/// The list of logging providers we support
/// This is different from LogSinkConfig in that this is just a C-style
/// enum as opposed to an ADT. This is meant to be used for the unsubscription
/// API and classifying sinks without having to specify a config.
///
/// DatadogV2 + AxiomV2 are the unlaunched versions of
/// https://www.notion.so/convex-dev/Log-streams-round-2-da990dc843e24e13b4a2051f51d0bb9c
/// They will eventually replace `Datadog` and `Axiom`
#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SinkType {
    Local,
    Datadog,
    DatadogV2,
    Webhook,
    Axiom,
    AxiomV2,
    Sentry,
    PostHogLogs,
    PostHogErrorTracking,
    S3Export,
}

impl SinkType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SinkType::Local => "local",
            SinkType::Datadog => "datadog",
            SinkType::DatadogV2 => "datadog",
            SinkType::Webhook => "webhook",
            SinkType::Axiom => "axiom",
            SinkType::AxiomV2 => "axiom",
            SinkType::Sentry => "sentry",
            SinkType::PostHogLogs => "postHogLogs",
            SinkType::PostHogErrorTracking => "postHogErrorTracking",
            SinkType::S3Export => "s3Export",
        }
    }
}

/// The configurations associated with each LogSinkType above.
/// Meant to be used for the subscription API.
#[derive(Debug, Clone, PartialEq)]
pub enum SinkConfig {
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
pub enum SerializedSinkConfig {
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

impl TryFrom<SerializedSinkConfig> for SinkConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedSinkConfig) -> Result<Self, Self::Error> {
        match value {
            SerializedSinkConfig::Local { path } => Ok(SinkConfig::Local(path)),
            SerializedSinkConfig::Datadog(config) => Ok(SinkConfig::Datadog(
                datadog::DatadogConfig::try_from(config)?,
            )),
            SerializedSinkConfig::Webhook(config) => Ok(SinkConfig::Webhook(
                webhook::WebhookConfig::try_from(config)?,
            )),
            SerializedSinkConfig::Axiom(config) => {
                Ok(SinkConfig::Axiom(axiom::AxiomConfig::try_from(config)?))
            },
            SerializedSinkConfig::Sentry(config) => {
                Ok(SinkConfig::Sentry(sentry::SentryConfig::try_from(config)?))
            },
            SerializedSinkConfig::PostHogLogs(config) => {
                Ok(SinkConfig::PostHogLogs(config.try_into()?))
            },
            SerializedSinkConfig::PostHogErrorTracking(config) => {
                Ok(SinkConfig::PostHogErrorTracking(config.into()))
            },
            SerializedSinkConfig::S3Export(config) => Ok(SinkConfig::S3Export(config.try_into()?)),
        }
    }
}

impl TryFrom<SinkConfig> for SerializedSinkConfig {
    type Error = anyhow::Error;

    fn try_from(value: SinkConfig) -> Result<Self, Self::Error> {
        match value {
            SinkConfig::Local(path) => Ok(SerializedSinkConfig::Local { path }),
            SinkConfig::Datadog(config) => Ok(SerializedSinkConfig::Datadog(
                datadog::SerializedDatadogConfig::from(config),
            )),
            SinkConfig::Webhook(config) => Ok(SerializedSinkConfig::Webhook(
                webhook::SerializedWebhookConfig::from(config),
            )),
            SinkConfig::Axiom(config) => Ok(SerializedSinkConfig::Axiom(
                axiom::SerializedAxiomConfig::from(config),
            )),
            SinkConfig::Sentry(config) => Ok(SerializedSinkConfig::Sentry(
                sentry::SerializedSentryConfig::try_from(config)?,
            )),
            SinkConfig::PostHogLogs(config) => Ok(SerializedSinkConfig::PostHogLogs(config.into())),
            SinkConfig::PostHogErrorTracking(config) => {
                Ok(SerializedSinkConfig::PostHogErrorTracking(config.into()))
            },
            SinkConfig::S3Export(config) => Ok(SerializedSinkConfig::S3Export(config.into())),
        }
    }
}

codegen_convex_serialization!(SinkConfig, SerializedSinkConfig);

impl fmt::Display for SinkConfig {
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

impl SinkConfig {
    pub fn sink_type(&self) -> SinkType {
        match self {
            Self::Local(_) => SinkType::Local,
            Self::Datadog(_) => SinkType::Datadog,
            Self::Webhook(_) => SinkType::Webhook,
            Self::Axiom(_) => SinkType::Axiom,
            Self::Sentry(_) => SinkType::Sentry,
            Self::PostHogLogs(_) => SinkType::PostHogLogs,
            Self::PostHogErrorTracking(_) => SinkType::PostHogErrorTracking,
            Self::S3Export(_) => SinkType::S3Export,
        }
    }

    /// The set of topics this sink is subscribed to. `Some(None)` for
    /// subscriptions to all topics, and `None` for sink types that do not
    /// support topic subscriptions.
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
