use std::{
    collections::BTreeMap,
    fmt::{
        self,
        Display,
    },
    str::FromStr,
};

use common::pii::PII;
use sentry::types::Dsn;
use serde::{
    Deserialize,
    Serialize,
};
use utoipa::ToSchema;
use value::FieldName;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentryConfig {
    pub dsn: PII<Dsn>,
    pub tags: Option<BTreeMap<FieldName, String>>,
    pub version: ExceptionFormatVersion,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedSentryConfig {
    pub dsn: String,
    pub tags: Option<BTreeMap<FieldName, String>>,
    pub version: Option<String>,
}

impl TryFrom<SentryConfig> for SerializedSentryConfig {
    type Error = anyhow::Error;

    fn try_from(value: SentryConfig) -> Result<Self, Self::Error> {
        Ok(Self {
            dsn: value.dsn.into_value().to_string(),
            tags: value.tags,
            version: Some(value.version.to_string()),
        })
    }
}

impl TryFrom<SerializedSentryConfig> for SentryConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedSentryConfig) -> Result<Self, Self::Error> {
        let version = match value.version {
            Some(v) => ExceptionFormatVersion::from_str(&v)?,
            // Treat missing version as V1
            None => ExceptionFormatVersion::V1,
        };
        if version == ExceptionFormatVersion::V1 {
            tracing::info!("Instance is on exception format version 1 (sentry)")
        }
        Ok(Self {
            dsn: value.dsn.parse::<Dsn>()?.into(),
            tags: value.tags,
            version,
        })
    }
}

impl fmt::Display for SentryConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SentryConfig {{ dsn: ..., version: {} }}", self.version)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum ExceptionFormatVersion {
    V1,
    V2,
}

impl FromStr for ExceptionFormatVersion {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "1" => Ok(Self::V1),
            "2" => Ok(Self::V2),
            v => anyhow::bail!("Invalid ExceptionFormatVersion: {v}"),
        }
    }
}

impl Display for ExceptionFormatVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V1 => write!(f, "1"),
            Self::V2 => write!(f, "2"),
        }
    }
}
