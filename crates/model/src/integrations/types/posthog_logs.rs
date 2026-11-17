use std::{
    collections::BTreeSet,
    fmt,
};

use common::{
    log_streaming::LogTopic,
    pii::PII,
};
use serde::{
    Deserialize,
    Serialize,
};

pub const DEFAULT_POSTHOG_HOST: &str = "https://us.i.posthog.com";

#[derive(Debug, Clone, PartialEq)]
pub struct PostHogLogsConfig {
    pub api_key: PII<String>,
    pub host: Option<String>,
    pub service_name: Option<String>,
    /// The set of topics this log stream is subscribed to. `None` means the
    /// stream is subscribed to all topics, including ones added in the future.
    pub topics: Option<BTreeSet<LogTopic>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedPostHogLogsConfig {
    pub api_key: String,
    pub host: Option<String>,
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<String>>,
}

impl From<PostHogLogsConfig> for SerializedPostHogLogsConfig {
    fn from(value: PostHogLogsConfig) -> Self {
        Self {
            api_key: value.api_key.0,
            host: value.host,
            service_name: value.service_name,
            topics: super::serialize_topics(value.topics),
        }
    }
}

impl TryFrom<SerializedPostHogLogsConfig> for PostHogLogsConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedPostHogLogsConfig) -> Result<Self, Self::Error> {
        Ok(Self {
            api_key: PII(value.api_key),
            host: value.host,
            service_name: value.service_name,
            topics: super::deserialize_topics(value.topics)?,
        })
    }
}

impl fmt::Display for PostHogLogsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PostHogLogsConfig {{ host: {:?} }}",
            self.host.as_deref().unwrap_or(DEFAULT_POSTHOG_HOST)
        )
    }
}
