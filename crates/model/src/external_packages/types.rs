use common::types::{
    NodeDependency,
    ObjectKey,
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_bytes::ByteBuf;
use value::{
    codegen_convex_serialization,
    id_v6::DeveloperDocumentId,
    sha256::Sha256Digest,
    ConvexObject,
};

use crate::source_packages::types::{
    PackageSize,
    SerializedPackageSize,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalDepsPackage {
    pub storage_key: ObjectKey,
    pub sha256: Sha256Digest,
    pub deps: Vec<NodeDependency>,
    pub package_size: PackageSize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalDepsPackageId(DeveloperDocumentId);

impl From<DeveloperDocumentId> for ExternalDepsPackageId {
    fn from(id: DeveloperDocumentId) -> Self {
        Self(id)
    }
}

impl From<ExternalDepsPackageId> for DeveloperDocumentId {
    fn from(value: ExternalDepsPackageId) -> Self {
        value.0
    }
}

impl From<ExternalDepsPackageId> for String {
    fn from(value: ExternalDepsPackageId) -> Self {
        value.0.into()
    }
}

impl TryFrom<String> for ExternalDepsPackageId {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let id = DeveloperDocumentId::decode(&value)?;
        Ok(Self(id))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedExternalDepsPackage {
    storage_key: String,
    sha256: ByteBuf,
    deps: Vec<ConvexObject>,
    #[serde(default)]
    package_size: Option<SerializedPackageSize>,
}

impl TryFrom<SerializedExternalDepsPackage> for ExternalDepsPackage {
    type Error = anyhow::Error;

    fn try_from(value: SerializedExternalDepsPackage) -> Result<Self, Self::Error> {
        Ok(Self {
            storage_key: value.storage_key.try_into()?,
            sha256: value.sha256.into_vec().try_into()?,
            deps: value
                .deps
                .into_iter()
                .map(NodeDependency::try_from)
                .collect::<anyhow::Result<_>>()?,
            package_size: value
                .package_size
                .map(TryInto::try_into)
                .transpose()?
                .unwrap_or_default(),
        })
    }
}

impl TryFrom<ExternalDepsPackage> for SerializedExternalDepsPackage {
    type Error = anyhow::Error;

    fn try_from(value: ExternalDepsPackage) -> Result<Self, Self::Error> {
        Ok(Self {
            storage_key: value.storage_key.into(),
            sha256: ByteBuf::from(value.sha256.to_vec().as_slice()),
            deps: value
                .deps
                .into_iter()
                .map(ConvexObject::try_from)
                .collect::<anyhow::Result<_>>()?,
            package_size: Some(value.package_size.try_into()?),
        })
    }
}

codegen_convex_serialization!(ExternalDepsPackage, SerializedExternalDepsPackage);
