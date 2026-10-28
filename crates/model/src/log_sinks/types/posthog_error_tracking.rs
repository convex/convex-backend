use std::fmt;

use common::pii::PII;
use serde::{
    Deserialize,
    Serialize,
};

use super::posthog_logs::DEFAULT_POSTHOG_HOST;

#[derive(Debug, Clone, PartialEq)]
pub struct PostHogErrorTrackingConfig {
    pub api_key: PII<String>,
    pub host: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedPostHogErrorTrackingConfig {
    pub api_key: String,
    pub host: Option<String>,
}

impl From<PostHogErrorTrackingConfig> for SerializedPostHogErrorTrackingConfig {
    fn from(value: PostHogErrorTrackingConfig) -> Self {
        Self {
            api_key: value.api_key.0,
            host: value.host,
        }
    }
}

impl From<SerializedPostHogErrorTrackingConfig> for PostHogErrorTrackingConfig {
    fn from(value: SerializedPostHogErrorTrackingConfig) -> Self {
        Self {
            api_key: PII(value.api_key),
            host: value.host,
        }
    }
}

impl fmt::Display for PostHogErrorTrackingConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PostHogErrorTrackingConfig {{ host: {:?} }}",
            self.host.as_deref().unwrap_or(DEFAULT_POSTHOG_HOST)
        )
    }
}
