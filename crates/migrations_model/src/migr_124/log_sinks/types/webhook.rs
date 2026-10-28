use std::fmt;

use common::runtime::Runtime;
use serde::{
    Deserialize,
    Serialize,
};

#[derive(Debug, Clone, PartialEq)]
pub struct WebhookConfig {
    pub url: reqwest::Url,
    pub format: WebhookFormat,
    pub hmac_secret: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
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
    pub hmac_secret: Option<String>,
}

impl From<WebhookConfig> for SerializedWebhookConfig {
    fn from(value: WebhookConfig) -> Self {
        Self {
            url: value.url.to_string(),
            format: value.format,
            hmac_secret: value.hmac_secret,
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
        })
    }
}

impl fmt::Display for WebhookConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebhookConfig {{ url: ... }}")
    }
}

pub fn generate_webhook_hmac_secret<RT: Runtime>(rt: &RT) -> String {
    rt.new_uuid_v4().as_simple().to_string()
}
