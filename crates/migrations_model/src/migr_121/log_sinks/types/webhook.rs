use std::fmt;

use serde::{
    Deserialize,
    Serialize,
};

#[derive(Debug, Clone, PartialEq)]
pub struct WebhookConfig {
    pub url: reqwest::Url,
    pub format: WebhookFormat,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum WebhookFormat {
    #[default]
    Json,
    Jsonl,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedWebhookConfig {
    pub url: String,
    #[serde(default)]
    pub format: WebhookFormat,
}

impl From<WebhookConfig> for SerializedWebhookConfig {
    fn from(value: WebhookConfig) -> Self {
        Self {
            url: value.url.to_string(),
            format: value.format,
        }
    }
}

impl TryFrom<SerializedWebhookConfig> for WebhookConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedWebhookConfig) -> Result<Self, Self::Error> {
        Ok(WebhookConfig {
            url: value.url.parse()?,
            format: value.format,
        })
    }
}

impl fmt::Display for WebhookConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebhookConfig {{ url: ... }}")
    }
}
