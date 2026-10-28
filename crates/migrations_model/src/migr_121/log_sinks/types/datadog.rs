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

/// The Datadog deployment locations, used to construct URLs
#[derive(Deserialize, Eq, PartialEq, Debug, Clone, Copy)]
pub enum DatadogSiteLocation {
    US1,
    US3,
    US5,
    EU,
    #[allow(non_camel_case_types)]
    US1_FED,
    AP1,
}

impl DatadogSiteLocation {
    pub fn get_site(&self) -> &str {
        match self {
            Self::US1 => "datadoghq.com",
            Self::US3 => "us3.datadoghq.com",
            Self::US5 => "us5.datadoghq.com",
            Self::EU => "datadoghq.eu",
            Self::US1_FED => "ddog-gov.com",
            Self::AP1 => "ap1.datadoghq.com",
        }
    }

    pub fn get_logging_endpoint(&self) -> anyhow::Result<reqwest::Url> {
        let url = reqwest::Url::parse(
            format!("https://http-intake.logs.{}/api/v2/logs", self.get_site()).as_str(),
        )?;
        Ok(url)
    }
}

impl FromStr for DatadogSiteLocation {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "US1" => Self::US1,
            "US3" => Self::US3,
            "US5" => Self::US5,
            "EU" => Self::EU,
            "US1_FED" => Self::US1_FED,
            "AP1" => Self::AP1,
            _ => anyhow::bail!("Datadog site location is not a valid site location string"),
        })
    }
}

impl fmt::Display for DatadogSiteLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            DatadogSiteLocation::US1 => "US1",
            DatadogSiteLocation::US3 => "US3",
            DatadogSiteLocation::US5 => "US5",
            DatadogSiteLocation::EU => "EU",
            DatadogSiteLocation::US1_FED => "US1_FED",
            DatadogSiteLocation::AP1 => "AP1",
        };
        write!(f, "{s}")
    }
}

/// The main configuration required for Datadog HTTP API
#[derive(Debug, Clone, PartialEq)]
pub struct DatadogConfig {
    pub site_location: DatadogSiteLocation,
    pub dd_api_key: PII<String>,
    pub dd_tags: Vec<String>,
    pub version: LogEventFormatVersion,
    pub service: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedDatadogConfig {
    pub site_location: String,
    pub dd_api_key: String,
    pub dd_tags: Vec<String>,
    pub version: Option<String>,
    pub service: Option<String>,
}

impl From<DatadogConfig> for SerializedDatadogConfig {
    fn from(value: DatadogConfig) -> Self {
        Self {
            site_location: value.site_location.to_string(),
            dd_api_key: value.dd_api_key.0,
            dd_tags: value.dd_tags,
            version: Some(value.version.to_string()),
            service: value.service,
        }
    }
}

impl TryFrom<SerializedDatadogConfig> for DatadogConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedDatadogConfig) -> Result<Self, Self::Error> {
        Ok(DatadogConfig {
            site_location: DatadogSiteLocation::from_str(&value.site_location)?,
            dd_api_key: PII(value.dd_api_key),
            dd_tags: value.dd_tags,
            version: value
                .version
                .map(|v| LogEventFormatVersion::from_str(v.as_str()))
                .transpose()?
                .unwrap_or(LogEventFormatVersion::V1),
            service: value.service,
        })
    }
}

impl fmt::Display for DatadogConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DatadogConfig {{ version: {:?}, ... }}", self.version)
    }
}
