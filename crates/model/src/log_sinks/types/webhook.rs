use std::{
    collections::BTreeSet,
    fmt,
};

use common::{
    log_streaming::LogTopic,
    runtime::Runtime,
};
use serde::{
    Deserialize,
    Serialize,
};
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq)]
pub struct WebhookConfig {
    pub url: reqwest::Url,
    pub format: WebhookFormat,
    pub hmac_secret: String,
    /// The set of topics this log stream is subscribed to. `None` means the
    /// stream is subscribed to all topics, including ones added in the future.
    pub topics: Option<BTreeSet<LogTopic>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum WebhookFormat {
    Json,
    Jsonl,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedWebhookConfig {
    pub url: String,
    pub format: WebhookFormat,
    pub hmac_secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<String>>,
}

impl From<WebhookConfig> for SerializedWebhookConfig {
    fn from(value: WebhookConfig) -> Self {
        Self {
            url: value.url.to_string(),
            format: value.format,
            hmac_secret: value.hmac_secret,
            topics: super::serialize_topics(value.topics),
        }
    }
}

impl TryFrom<SerializedWebhookConfig> for WebhookConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedWebhookConfig) -> Result<Self, Self::Error> {
        Ok(WebhookConfig {
            url: value.url.parse()?,
            format: value.format,
            hmac_secret: value.hmac_secret,
            topics: super::deserialize_topics(value.topics)?,
        })
    }
}

impl fmt::Display for WebhookConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebhookConfig {{ url: ... }}")
    }
}

pub fn generate_webhook_hmac_secret<RT: Runtime>(rt: RT) -> String {
    rt.new_uuid_v4().as_simple().to_string()
}
