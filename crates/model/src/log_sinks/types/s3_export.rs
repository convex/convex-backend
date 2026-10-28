//! Configuration for mirroring a deployment's data into object storage so it
//! can be queried by analytics engines. These documents live in the log sinks
//! table alongside log streams, but they carry no log sink client.

use std::fmt;

use common::{
    pii::PII,
    types::streaming_export::selection::Selection,
};
use rand::Rng as _;
use serde::{
    Deserialize,
    Serialize,
};
use utoipa::ToSchema;

/// How often the mirror is refreshed from the deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SyncPeriod {
    Continuous,
    Hourly,
    Daily,
}

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
    pub period: SyncPeriod,
}

/// How Porter signs its writes to the bucket.
#[derive(Debug, Clone, PartialEq)]
pub enum S3ExportCredentials {
    /// Long-lived keys of an IAM user the customer created by hand.
    // TODO(ENG-11116): drop this variant and the two stored key fields once
    // existing exports have moved to roles.
    AccessKey {
        access_key_id: PII<String>,
        secret_access_key: PII<String>,
    },
    /// A role in the customer's account that Porter assumes per page.
    /// `role_arn` is empty until the admin pastes it; the worker waits for it.
    Role {
        role_arn: String,
        /// Convex generates this once per export. AWS requires it in the
        /// role's trust policy so only this export can assume the role.
        external_id: String,
    },
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

    /// The admin has finished the AWS side and Porter can write.
    pub fn is_ready(&self) -> bool {
        match self {
            Self::AccessKey { .. } => true,
            Self::Role { role_arn, .. } => !role_arn.is_empty(),
        }
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
            credentials,
            selection: _,
            period: _,
        } = self;
        // `Some("")` and `None` both write at the bucket root.
        let root_as_none = |prefix: &Option<String>| prefix.clone().filter(|p| !p.is_empty());
        // A rotated secret or a replaced role still points at the same
        // destination; a different access key ID is a different IAM user.
        let credentials_changed = match (credentials, &new.credentials) {
            (
                S3ExportCredentials::AccessKey { access_key_id, .. },
                S3ExportCredentials::AccessKey {
                    access_key_id: new_id,
                    ..
                },
            ) => access_key_id != new_id,
            (S3ExportCredentials::Role { .. }, S3ExportCredentials::Role { .. }) => false,
            _ => true,
        };
        *bucket != new.bucket
            || *region != new.region
            || root_as_none(prefix) != root_as_none(&new.prefix)
            || credentials_changed
    }
}

/// Stored shape. The credential fields stay flat, and a role export stores
/// empty strings in the two key fields, so binaries from before and after
/// roles existed both decode every document.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedS3ExportConfig {
    pub bucket: String,
    pub region: String,
    pub prefix: Option<String>,
    pub access_key_id: String,
    pub secret_access_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_arn: Option<String>,
    /// The `sts:ExternalId` the role's trust policy requires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_external_id: Option<String>,
    pub selection: Selection,
    pub period: SyncPeriod,
}

impl From<S3ExportConfig> for SerializedS3ExportConfig {
    fn from(value: S3ExportConfig) -> Self {
        let (access_key_id, secret_access_key, role_arn, role_external_id) = match value.credentials
        {
            S3ExportCredentials::AccessKey {
                access_key_id,
                secret_access_key,
            } => (access_key_id.0, secret_access_key.0, None, None),
            S3ExportCredentials::Role {
                role_arn,
                external_id,
            } => (
                String::new(),
                String::new(),
                Some(role_arn),
                Some(external_id),
            ),
        };
        Self {
            bucket: value.bucket,
            region: value.region,
            prefix: value.prefix,
            access_key_id,
            secret_access_key,
            role_arn,
            role_external_id,
            selection: value.selection,
            period: value.period,
        }
    }
}

impl TryFrom<SerializedS3ExportConfig> for S3ExportConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedS3ExportConfig) -> anyhow::Result<Self> {
        let credentials = match (value.role_arn, value.role_external_id) {
            (None, None) => S3ExportCredentials::AccessKey {
                access_key_id: PII(value.access_key_id),
                secret_access_key: PII(value.secret_access_key),
            },
            (Some(role_arn), Some(external_id)) => S3ExportCredentials::Role {
                role_arn,
                external_id,
            },
            _ => anyhow::bail!("A role export stores both `roleArn` and `roleExternalId`"),
        };
        Ok(Self {
            bucket: value.bucket,
            region: value.region,
            prefix: value.prefix,
            credentials,
            selection: value.selection,
            period: value.period,
        })
    }
}

impl fmt::Display for S3ExportConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "S3ExportConfig {{ bucket: {}, region: {}, period: {:?} }}",
            self.bucket, self.region, self.period
        )
    }
}
