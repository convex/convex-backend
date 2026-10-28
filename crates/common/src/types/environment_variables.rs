use std::{
    borrow::Borrow,
    collections::BTreeMap,
    str::FromStr,
    sync::LazyLock,
};

use errors::ErrorMetadata;
use regex::Regex;
use serde::{
    Deserialize,
    Serialize,
};

use crate::knobs::{
    ENV_VAR_LIMIT,
    ENV_VAR_TOTAL_SIZE_LIMIT,
};

#[rustfmt::skip]
#[derive(
    Clone, Debug, Eq, PartialEq, PartialOrd, Ord,
    Serialize, Deserialize, derive_more::Display, Hash,
)]
pub struct EnvVarName(String);

impl From<EnvVarName> for String {
    fn from(value: EnvVarName) -> Self {
        value.0
    }
}

impl Borrow<str> for EnvVarName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

#[rustfmt::skip]
#[derive(
    Clone, Debug, Eq, PartialEq, PartialOrd, Ord,
    Serialize, Deserialize, derive_more::Display, Hash,
)]
pub struct EnvVarValue(String);

impl From<EnvVarValue> for String {
    fn from(value: EnvVarValue) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EnvironmentVariable {
    pub name: EnvVarName,
    pub value: EnvVarValue,
}

impl AsRef<str> for EnvVarName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for EnvVarValue {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

static NAME_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z_]+[a-zA-Z0-9_]*$").unwrap());

// NOTE: Make sure to update the doc if you change any of these limits. Also
// don't reduce them since that might break existing projects.

/// Maximum length of the name of an environment variable
pub const MAX_NAME_LENGTH: usize = 256;
/// Maximum length of an environment variable value. 8KiB corresponds to the
/// maximum length of an HTTP header.
pub const MAX_VALUE_LENGTH: usize = 8 * (1 << 10);

impl FromStr for EnvVarName {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            NAME_REGEX.is_match(s),
            ErrorMetadata::bad_request(
                "EnvironmentVariableNameInvalid",
                format!(
                    "The environment variable name {s} is invalid. Environment variable names \
                     must begin with a letter and may only include characters a-z, A-Z, 0-9, and \
                     underscores."
                ),
            )
        );
        anyhow::ensure!(
            s.len() <= MAX_NAME_LENGTH,
            ErrorMetadata::bad_request(
                "EnvironmentVariableNameTooLong",
                format!(
                    "The environment variable name {s} is too long. Environment variable names \
                     must be less than {MAX_NAME_LENGTH}."
                )
            )
        );
        Ok(Self(s.to_owned()))
    }
}

impl FromStr for EnvVarValue {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let len = s.len();
        anyhow::ensure!(
            len <= MAX_VALUE_LENGTH,
            ErrorMetadata::bad_request(
                "EnvironmentVariableValueTooLarge",
                format!(
                    "The environment variable value is {len} bytes, which is too large. (max \
                     size: {MAX_VALUE_LENGTH}"
                ),
            )
        );
        Ok(Self(s.to_owned()))
    }
}

impl EnvironmentVariable {
    pub fn new(name: EnvVarName, value: EnvVarValue) -> Self {
        Self { name, value }
    }

    pub fn name(&self) -> &EnvVarName {
        &self.name
    }

    pub fn value(&self) -> &EnvVarValue {
        &self.value
    }

    pub fn into_value(self) -> EnvVarValue {
        self.value
    }
}

pub fn env_var_limit_met() -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "EnvVarLimitMet",
        format!(
            "The environment variable limit ({}) has been met.",
            *ENV_VAR_LIMIT
        ),
    )
}

pub fn env_var_total_size_limit_met(total_size: usize) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "EnvVarTotalSizeLimitMet",
        format!(
            "The total size of all environment variables ({total_size} bytes) exceeds the limit \
             ({} bytes).",
            *ENV_VAR_TOTAL_SIZE_LIMIT
        ),
    )
}

pub fn env_var_total_size(env_vars: &BTreeMap<EnvVarName, EnvVarValue>) -> usize {
    env_vars
        .iter()
        .map(|(name, value)| name.as_ref().len() + value.as_ref().len())
        .sum()
}

pub fn env_var_name_not_unique(name: Option<&EnvVarName>) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "EnvVarNameNotUnique",
        match name {
            Some(n) => format!("An environment variable with name \"{n}\" already exists"),
            None => "One or more environment variable name is not unique".to_string(),
        },
    )
}

pub fn env_var_name_forbidden(name: &EnvVarName) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "EnvVarNameForbidden",
        format!("Environment variable with name \"{name}\" is built-in and cannot be overridden"),
    )
}
