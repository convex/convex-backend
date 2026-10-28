use std::{
    fmt,
    str::FromStr,
};

use common::{
    log_streaming::LogEventFormatVersion,
    pii::PII,
};
use serde::{
    Deserialize,
    Serialize,
};

#[derive(Debug, Clone, PartialEq)]
pub struct AxiomConfig {
    pub api_key: PII<String>,
    pub dataset_name: String,
    pub attributes: Vec<AxiomAttribute>,
    pub version: LogEventFormatVersion,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedAxiomConfig {
    pub api_key: String,
    pub dataset_name: String,
    pub attributes: Vec<SerializedAxiomAttribute>,
    pub version: Option<String>,
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
        }
    }
}

impl TryFrom<SerializedAxiomConfig> for AxiomConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedAxiomConfig) -> Result<Self, Self::Error> {
        Ok(Self {
            api_key: PII(value.api_key),
            dataset_name: value.dataset_name,
            attributes: value
                .attributes
                .into_iter()
                .map(AxiomAttribute::from)
                .collect(),
            version: value
                .version
                .map(|v| LogEventFormatVersion::from_str(v.as_str()))
                .transpose()?
                .unwrap_or(LogEventFormatVersion::V1),
        })
    }
}

impl fmt::Display for AxiomConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AxiomConfig {{ version:{:?} ... }}", self.version)
    }
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
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
