use std::{
    collections::BTreeSet,
    fmt,
    str::FromStr,
};

use common::{
    log_streaming::{
        LogEventFormatVersion,
        LogTopic,
    },
    pii::PII,
};
use serde::{
    Deserialize,
    Serialize,
};
use utoipa::ToSchema;

/// Valid Axiom ingest base URLs for different regions
pub const VALID_AXIOM_INGEST_URLS: &[&str] = &[
    "https://api.axiom.co",
    "https://us-east-1.aws.edge.axiom.co",
    "https://eu-central-1.aws.edge.axiom.co",
];

#[derive(Debug, Clone, PartialEq)]
pub struct AxiomConfig {
    pub api_key: PII<String>,
    pub dataset_name: String,
    pub attributes: Vec<AxiomAttribute>,
    pub version: LogEventFormatVersion,
    pub ingest_url: Option<String>,
    /// The set of topics this log stream is subscribed to. `None` means the
    /// stream is subscribed to all topics, including ones added in the future.
    pub topics: Option<BTreeSet<LogTopic>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedAxiomConfig {
    pub api_key: String,
    pub dataset_name: String,
    pub attributes: Vec<SerializedAxiomAttribute>,
    pub version: Option<String>,
    pub ingest_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<String>>,
}

impl From<AxiomConfig> for SerializedAxiomConfig {
    fn from(value: AxiomConfig) -> Self {
        Self {
            api_key: value.api_key.0,
            dataset_name: value.dataset_name,
            attributes: value
                .attributes
                .into_iter()
                .map(SerializedAxiomAttribute::from)
                .collect(),
            version: Some(value.version.to_string()),
            ingest_url: value.ingest_url,
            topics: super::serialize_topics(value.topics),
        }
    }
}

impl TryFrom<SerializedAxiomConfig> for AxiomConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedAxiomConfig) -> Result<Self, Self::Error> {
        let version = value
            .version
            .map(|v| LogEventFormatVersion::from_str(v.as_str()))
            .transpose()?
            .unwrap_or(LogEventFormatVersion::V1);
        if version == LogEventFormatVersion::V1 {
            tracing::info!("Instance is on log event format version 1 (axiom)")
        }
        Ok(Self {
            api_key: PII(value.api_key),
            dataset_name: value.dataset_name,
            attributes: value
                .attributes
                .into_iter()
                .map(AxiomAttribute::from)
                .collect(),
            version,
            ingest_url: value.ingest_url,
            topics: super::deserialize_topics(value.topics)?,
        })
    }
}

impl fmt::Display for AxiomConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AxiomConfig {{ version:{:?} ... }}", self.version)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, ToSchema)]
pub struct AxiomAttribute {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedAxiomAttribute {
    pub key: String,
    pub value: String,
}

impl From<AxiomAttribute> for SerializedAxiomAttribute {
    fn from(attribute: AxiomAttribute) -> Self {
        Self {
            key: attribute.key,
            value: attribute.value,
        }
    }
}

impl From<SerializedAxiomAttribute> for AxiomAttribute {
    fn from(value: SerializedAxiomAttribute) -> Self {
        Self {
            key: value.key,
            value: value.value,
        }
    }
}
