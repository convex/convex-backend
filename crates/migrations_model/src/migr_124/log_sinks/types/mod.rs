use std::fmt;

use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

pub mod axiom;
pub mod datadog;
pub mod mock_sink;
pub mod sentry;
pub mod webhook;

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

/// Status of a configured LogSink
/// LogSink SinkState state machine:
/// ```text
/// +---------+          +--------+
/// | Pending | -------> | Active |
/// +---------+          +--------+
///     |                     |
///     v                     v
/// +--------+         +------------+
/// | Failed |         | Tombstoned | ---> Removed
/// +--------+         +------------+
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum SinkState {
    Pending,
    Failed { reason: String },
    Active, // TODO: add health statistics under Active
    Tombstoned,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum SerializedSinkState {
    Pending,
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
        }
    }
}
