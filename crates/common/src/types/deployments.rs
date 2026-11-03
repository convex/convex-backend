use std::collections::BTreeSet;

use enum_iterator::Sequence;
use serde::{
    Deserialize,
    Serialize,
};
use utoipa::ToSchema;

use crate::types::RegionName;

#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    strum::EnumString,
    strum::Display,
    strum::VariantArray,
    strum::IntoStaticStr,
    Serialize,
    Deserialize,
    ToSchema,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeploymentClass {
    S16,
    S256,
    D1024,
    D2048,
    /// S256 limits on a db cluster and conductor reserved for one team, which
    /// may hold many of that team's deployments.
    P256,
}

impl DeploymentClass {
    pub fn is_big(&self) -> bool {
        match self {
            DeploymentClass::D1024 | DeploymentClass::D2048 => true,
            DeploymentClass::S16 | DeploymentClass::S256 | DeploymentClass::P256 => false,
        }
    }

    /// One deployment per db cluster.
    pub fn is_dedicated(&self) -> bool {
        match self {
            DeploymentClass::D1024 | DeploymentClass::D2048 => true,
            DeploymentClass::S16 | DeploymentClass::S256 | DeploymentClass::P256 => false,
        }
    }

    /// Many deployments on a db cluster reserved for one team.
    pub fn is_pooled(&self) -> bool {
        matches!(self, DeploymentClass::P256)
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    Deserialize,
    PartialEq,
    Eq,
    Serialize,
    PartialOrd,
    Ord,
    Sequence,
    strum::EnumString,
    strum::Display,
    ToSchema,
)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum DeploymentType {
    Dev,
    Prod,
    Preview,
    Custom,
}

impl DeploymentType {
    pub fn all_types() -> BTreeSet<Self> {
        enum_iterator::all().collect()
    }

    pub fn as_sentry_tag(&self) -> String {
        self.to_string()
    }

    pub fn default_send_logs_to_client(&self) -> bool {
        matches!(self, DeploymentType::Dev | DeploymentType::Preview)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeploymentMetadata {
    pub name: String,
    pub region: Option<RegionName>,
    pub class: DeploymentClass,
}

impl DeploymentMetadata {
}
