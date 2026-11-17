//! Configuration for mirroring a deployment's data into object storage so it
//! can be queried by analytics engines. These documents live in the
//! integrations table alongside log streams, but they carry no integration
//! client.

use std::fmt;

use common::types::streaming_export::selection::Selection;
use rand::Rng as _;
use serde::{
    Deserialize,
    Serialize,
};

/// A mirror in a customer-owned S3 bucket.
#[derive(Debug, Clone, PartialEq)]
pub struct S3ExportConfig {
    pub bucket: String,
    /// AWS region the bucket lives in, e.g. `us-east-1`.
    pub region: String,
    /// Key prefix within the bucket. `None` writes at the bucket root.
    pub prefix: Option<String>,
    pub credentials: S3ExportCredentials,
    /// The components and tables to export.
    pub selection: Selection,
}

/// A role in the customer's account that Porter assumes per page.
#[derive(Debug, Clone, PartialEq)]
pub struct S3ExportCredentials {
    /// Empty until the admin pastes it; the worker waits for it.
    pub role_arn: String,
    /// Convex generates this once per export. AWS requires it in the role's
    /// trust policy so only this export can assume the role.
    pub external_id: String,
}

impl S3ExportCredentials {
    /// 32 alphanumeric characters, which AWS accepts as an ExternalId and
    /// which need no escaping in a CloudFormation URL.
    pub fn new_external_id() -> String {
        rand::rng()
            .sample_iter(rand::distr::Alphanumeric)
            .take(32)
            .map(char::from)
            .collect()
    }
}

impl S3ExportConfig {
    /// Cursor progress belongs to the destination that received the exported
    /// documents. Keep in sync with the fields `destinationFixed` disables in
    /// `S3ExportConfigurationForm.tsx`.
    pub fn requires_resync(&self, new: &S3ExportConfig) -> bool {
        // Exhaustive destructuring makes new fields require a resync decision.
        let Self {
            bucket,
            region,
            prefix,
            credentials: _,
            selection: _,
        } = self;
        // `Some("")` and `None` both write at the bucket root.
        let root_as_none = |prefix: &Option<String>| prefix.clone().filter(|p| !p.is_empty());
        *bucket != new.bucket
            || *region != new.region
            || root_as_none(prefix) != root_as_none(&new.prefix)
    }
}

/// Stored shape. Documents written by older binaries also have `accessKeyId`
/// and `secretAccessKey`, which serde skips.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedS3ExportConfig {
    pub bucket: String,
    pub region: String,
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_arn: Option<String>,
    /// The `sts:ExternalId` the role's trust policy requires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_external_id: Option<String>,
    pub selection: Selection,
}

impl From<S3ExportConfig> for SerializedS3ExportConfig {
    fn from(value: S3ExportConfig) -> Self {
        Self {
            bucket: value.bucket,
            region: value.region,
            prefix: value.prefix,
            role_arn: Some(value.credentials.role_arn),
            role_external_id: Some(value.credentials.external_id),
            selection: value.selection,
        }
    }
}

impl TryFrom<SerializedS3ExportConfig> for S3ExportConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedS3ExportConfig) -> anyhow::Result<Self> {
        let (Some(role_arn), Some(external_id)) = (value.role_arn, value.role_external_id) else {
            anyhow::bail!("An S3 export stores both `roleArn` and `roleExternalId`");
        };
        Ok(Self {
            bucket: value.bucket,
            region: value.region,
            prefix: value.prefix,
            credentials: S3ExportCredentials {
                role_arn,
                external_id,
            },
            selection: value.selection,
        })
    }
}

impl fmt::Display for S3ExportConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "S3ExportConfig {{ bucket: {}, region: {} }}",
            self.bucket, self.region
        )
    }
}
