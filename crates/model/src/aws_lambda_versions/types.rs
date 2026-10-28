use std::{
    collections::BTreeMap,
    fmt::Formatter,
};

use common::document::ParsedDocument;
use serde::{
    Deserialize,
    Serialize,
};
use value::{
    codegen_convex_serialization,
    id_v6::DeveloperDocumentId,
    sha256::Sha256Digest,
    FieldName,
};

use crate::{
    external_packages::types::ExternalDepsPackageId,
    source_packages::types::{
        SourcePackage,
        SourcePackageId,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AwsLambdaConfig {
    pub env: BTreeMap<FieldName, String>, // Env to pass to node
    pub runtime: String,                  // Eg NodeJs16
    pub handler: String,                  // function to call within the source
    pub memory_size_mb: i32,
    pub disk_size_mb: i32,
    pub timeout_sec: i32,
    pub vpc_subnet_ids: Vec<String>,
    pub vpc_security_group_ids: Vec<String>,
    pub ipv6_enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AwsLambdaCodeStorage {
    #[serde(rename = "s3Bucket")]
    pub s3_bucket: String,
    #[serde(rename = "s3Key")]
    pub s3_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AwsLambdaVersion {
    pub lambda_name: String,
    pub lambda_version: String, // Returned from CreateFunction as a string
    pub node_executor_sha256: Sha256Digest, // hash of our node-executor code
    pub lambda_config: AwsLambdaConfig,
    pub package_desc: AwsLambdaPackageDesc,
    /// Static Lambdas store the S3 bucket/key of the code object they reference
    /// (Lambda REFERENCE mode). Dynamic Lambdas execute inline code and keep
    /// this unset, as do static Lambdas deployed before this field existed.
    pub code_storage: Option<AwsLambdaCodeStorage>,
}

/// Stores the configuration information for the Lambda relevant to the type
/// of Lambda this enum identifies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AwsLambdaPackageDesc {
    Static {
        source_package_id: Option<SourcePackageId>,
    },
    Dynamic {
        external_deps_package_id: Option<ExternalDepsPackageId>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AwsLambdaType {
    Static,
    Dynamic,
}

impl AwsLambdaType {
    /// Given a source package of code to deploy and an optional fallback
    /// Lambda version, builds an AwsLambdaPackageDesc
    pub fn get_package_desc(
        &self,
        deployed_code: Option<&ParsedDocument<SourcePackage>>,
    ) -> anyhow::Result<AwsLambdaPackageDesc> {
        match self {
            Self::Static => {
                let source_package_id = deployed_code
                    .map(|source_package| DeveloperDocumentId::from(source_package.id()).into());
                Ok(AwsLambdaPackageDesc::Static { source_package_id })
            },
            Self::Dynamic => {
                // Dynamic lambdas always download packages at invocation time,
                // so keep this descriptor constant to avoid unnecessary redeploys.
                Ok(AwsLambdaPackageDesc::Dynamic {
                    external_deps_package_id: None,
                })
            },
        }
    }
}

impl AwsLambdaVersion {
    pub fn static_code_storage(&self) -> Option<&AwsLambdaCodeStorage> {
        match self.package_desc {
            AwsLambdaPackageDesc::Static { .. } => self.code_storage.as_ref(),
            AwsLambdaPackageDesc::Dynamic { .. } => None,
        }
    }
}

impl std::fmt::Display for AwsLambdaType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Static => write!(f, "Static"),
            Self::Dynamic => write!(f, "Dynamic"),
        }
    }
}

impl AwsLambdaPackageDesc {
    pub fn as_static(&self) -> anyhow::Result<&Option<SourcePackageId>> {
        let AwsLambdaPackageDesc::Static { source_package_id } = self else {
            anyhow::bail!("Expected AwsLambdaType::Static, but got {:?}", self);
        };
        Ok(source_package_id)
    }

    pub fn as_dynamic(&self) -> anyhow::Result<&Option<ExternalDepsPackageId>> {
        let AwsLambdaPackageDesc::Dynamic {
            external_deps_package_id,
        } = self
        else {
            anyhow::bail!("Expected AwsLambdaType::Static, but got {:?}", self);
        };
        Ok(external_deps_package_id)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SerializedAwsLambdaPackageDesc {
    pub r#type: String,
    #[serde(rename = "sourcePackageId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub source_package_id: Option<Option<String>>,
    #[serde(rename = "externalDepsPackageId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub external_deps_package_id: Option<Option<String>>,
}

impl From<AwsLambdaPackageDesc> for SerializedAwsLambdaPackageDesc {
    fn from(value: AwsLambdaPackageDesc) -> Self {
        match value {
            AwsLambdaPackageDesc::Static { source_package_id } => SerializedAwsLambdaPackageDesc {
                r#type: "static".into(),
                source_package_id: Some(match source_package_id {
                    None => None,
                    Some(source_package_id) => Some(source_package_id.into()),
                }),
                external_deps_package_id: None,
            },
            AwsLambdaPackageDesc::Dynamic {
                external_deps_package_id,
            } => SerializedAwsLambdaPackageDesc {
                r#type: "dynamic".into(),
                external_deps_package_id: Some(match external_deps_package_id {
                    None => None,
                    Some(external_deps_package_id) => Some(external_deps_package_id.into()),
                }),
                source_package_id: None,
            },
        }
    }
}

impl TryFrom<SerializedAwsLambdaPackageDesc> for AwsLambdaPackageDesc {
    type Error = anyhow::Error;

    fn try_from(obj: SerializedAwsLambdaPackageDesc) -> Result<Self, Self::Error> {
        match obj.r#type.as_str() {
            "static" => {
                let source_package_id: Option<SourcePackageId> = match obj.source_package_id {
                    Some(None) => None,
                    Some(Some(value)) => Some(value.try_into()?),
                    None => anyhow::bail!("Missing 'sourcePackageId' in {obj:?}"),
                };
                Ok(AwsLambdaPackageDesc::Static { source_package_id })
            },
            "dynamic" => {
                let external_deps_package_id: Option<ExternalDepsPackageId> =
                    match obj.external_deps_package_id {
                        Some(None) => None,
                        Some(Some(value)) => Some(value.try_into()?),
                        None => anyhow::bail!("Missing 'externalDepsPackageId' in {obj:?}"),
                    };
                Ok(AwsLambdaPackageDesc::Dynamic {
                    external_deps_package_id,
                })
            },
            lambda_type => anyhow::bail!("Unknown AwsLambdaType {lambda_type:}: {obj:?}"),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct SerializedAwsLambdaConfig {
    pub env: BTreeMap<String, String>,
    pub runtime: String,
    pub handler: String,
    #[serde(rename = "memorySizeMb")]
    pub memory_size_mb: i64,
    #[serde(rename = "diskSizeMb")]
    pub disk_size_mb: Option<i64>,
    #[serde(rename = "timeoutSec")]
    pub timeout_sec: i64,
    #[serde(rename = "subnetIds")]
    pub vpc_subnet_ids: Option<Vec<String>>,
    #[serde(rename = "securityGroupIds")]
    pub vpc_security_group_ids: Option<Vec<String>>,
    #[serde(rename = "ipv6Enabled")]
    pub ipv6_enabled: Option<bool>,
}

impl TryFrom<AwsLambdaConfig> for SerializedAwsLambdaConfig {
    type Error = anyhow::Error;

    fn try_from(
        AwsLambdaConfig {
            env,
            runtime,
            handler,
            memory_size_mb,
            disk_size_mb,
            timeout_sec,
            vpc_subnet_ids,
            vpc_security_group_ids,
            ipv6_enabled,
        }: AwsLambdaConfig,
    ) -> Result<Self, Self::Error> {
        let env = env.into_iter().map(|(k, v)| (k.into(), v)).collect();
        Ok(SerializedAwsLambdaConfig {
            env,
            runtime,
            handler,
            memory_size_mb: memory_size_mb as i64,
            disk_size_mb: Some(disk_size_mb as i64),
            timeout_sec: timeout_sec as i64,
            vpc_subnet_ids: Some(vpc_subnet_ids),
            vpc_security_group_ids: Some(vpc_security_group_ids),
            ipv6_enabled: Some(ipv6_enabled),
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct SerializedAwsLambdaVersion {
    #[serde(rename = "lambdaName")]
    pub lambda_name: String,
    #[serde(rename = "lambdaVersion")]
    pub lambda_version: String,
    #[serde(rename = "sourcePackageId")]
    pub source_package_id: Option<String>,
    #[serde(rename = "nodeExecutorSha256")]
    pub node_executor_sha256: serde_bytes::ByteBuf,
    #[serde(rename = "lambdaConfig")]
    pub lambda_config: SerializedAwsLambdaConfig,
    #[serde(rename = "typeConfig")]
    pub package_desc: Option<SerializedAwsLambdaPackageDesc>,
    #[serde(rename = "codeStorage")]
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_storage: Option<AwsLambdaCodeStorage>,
}

impl TryFrom<AwsLambdaVersion> for SerializedAwsLambdaVersion {
    type Error = anyhow::Error;

    fn try_from(
        AwsLambdaVersion {
            lambda_name,
            lambda_version,
            node_executor_sha256,
            lambda_config,
            package_desc,
            code_storage,
        }: AwsLambdaVersion,
    ) -> Result<Self, Self::Error> {
        Ok(SerializedAwsLambdaVersion {
            lambda_name,
            lambda_version,
            // Double-write this field for backwards compatability
            source_package_id: match &package_desc {
                AwsLambdaPackageDesc::Static {
                    source_package_id: Some(id),
                } => Some((*id).into()),
                _ => None,
            },
            node_executor_sha256: serde_bytes::ByteBuf::from(node_executor_sha256.to_vec()),
            lambda_config: lambda_config.try_into()?,
            package_desc: Some(package_desc.into()),
            code_storage,
        })
    }
}

impl TryFrom<SerializedAwsLambdaConfig> for AwsLambdaConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedAwsLambdaConfig) -> Result<Self, Self::Error> {
        let env = value
            .env
            .into_iter()
            .map(|(k, v)| Ok((k.try_into()?, v)))
            .collect::<anyhow::Result<_>>()?;
        let runtime = value.runtime;
        let handler = value.handler;
        let memory_size_mb = value.memory_size_mb.try_into()?;
        let disk_size_mb = match value.disk_size_mb {
            Some(disk_size_mb) => disk_size_mb.try_into()?,
            // Old AwsLambdaConfigs do not have this so just use a sensible default of 512 MB
            None => 512i32,
        };
        let timeout_sec = value.timeout_sec.try_into()?;
        let vpc_subnet_ids = value.vpc_subnet_ids.unwrap_or_default();
        let vpc_security_group_ids = value.vpc_security_group_ids.unwrap_or_default();
        let ipv6_enabled = value.ipv6_enabled.unwrap_or(false);
        Ok(Self {
            env,
            runtime,
            handler,
            memory_size_mb,
            disk_size_mb,
            timeout_sec,
            vpc_subnet_ids,
            vpc_security_group_ids,
            ipv6_enabled,
        })
    }
}

impl TryFrom<SerializedAwsLambdaVersion> for AwsLambdaVersion {
    type Error = anyhow::Error;

    fn try_from(value: SerializedAwsLambdaVersion) -> Result<Self, Self::Error> {
        let lambda_name = value.lambda_name;
        let lambda_version = value.lambda_version;
        let node_executor_sha256 = value.node_executor_sha256.into_vec().try_into()?;
        let lambda_config = value.lambda_config.try_into()?;
        // Deprecated, reading this field for now to populate package_desc on old lambda
        // versions
        let source_package_id = match value.source_package_id {
            None => None,
            Some(value) => Some(value.try_into()?),
        };
        let package_desc = match value.package_desc {
            Some(package_desc) => package_desc.try_into()?,
            // This must be an old-style Lambda so this must be a static lambda and we should read
            // the sourcePackageId field above.
            None => AwsLambdaPackageDesc::Static { source_package_id },
        };
        let code_storage = value.code_storage;

        Ok(Self {
            lambda_name,
            lambda_version,
            node_executor_sha256,
            lambda_config,
            package_desc,
            code_storage,
        })
    }
}

codegen_convex_serialization!(AwsLambdaVersion, SerializedAwsLambdaVersion);
