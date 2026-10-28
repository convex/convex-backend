use std::{
    fmt::Formatter,
    str::FromStr,
};

use common::types::ObjectKey;
use errors::ErrorMetadata;
use humansize::{
    FormatSize,
    BINARY,
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_bytes::ByteBuf;
use value::{
    codegen_convex_serialization,
    heap_size::HeapSize,
    id_v6::DeveloperDocumentId,
    sha256::Sha256Digest,
};

use crate::external_packages::types::ExternalDepsPackageId;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum NodeVersion {
    /// Node 18 is deprecated in AWS, so customers with it set will
    /// no longer be able to update their static lambdas. This is okay because
    /// users can update their Node version to unblock themselves. New pushes
    /// with Node 18 are rejected by [`NodeVersion::check_supported_for_push`].
    V18x,
    /// Node 20 is end-of-life. Existing source packages keep running on it,
    /// but new pushes with Node 20 are rejected by
    /// [`NodeVersion::check_supported_for_push`].
    V20x,
    V22x,
    V24x,
}

impl NodeVersion {
    /// Rejects end-of-life versions so that new code can't be pushed with
    /// them. Versions stay parseable so existing source packages still load.
    pub fn check_supported_for_push(self) -> anyhow::Result<()> {
        match self {
            NodeVersion::V18x | NodeVersion::V20x => {
                let version = String::from(self);
                anyhow::bail!(ErrorMetadata::bad_request(
                    "NodeVersionNotSupported",
                    format!(
                        "Node {version} is no longer supported. Upgrade to a newer Node version \
                         (https://docs.convex.dev/config/convex.json#configuring-the-nodejs-version)."
                    )
                ));
            },
            NodeVersion::V22x | NodeVersion::V24x => Ok(()),
        }
    }
}

impl FromStr for NodeVersion {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "18" => Ok(NodeVersion::V18x),
            "20" => Ok(NodeVersion::V20x),
            "22" => Ok(NodeVersion::V22x),
            "24" => Ok(NodeVersion::V24x),
            _ => anyhow::bail!("Invalid node version: {value}"),
        }
    }
}

impl From<NodeVersion> for String {
    fn from(value: NodeVersion) -> String {
        match value {
            NodeVersion::V18x => "18".to_string(),
            NodeVersion::V20x => "20".to_string(),
            NodeVersion::V22x => "22".to_string(),
            NodeVersion::V24x => "24".to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeVersionDiff {
    pub previous_version: Option<NodeVersion>,
    pub next_version: Option<NodeVersion>,
}

#[derive(Serialize, Deserialize)]
pub struct SerializedNodeVersionDiff {
    pub previous_version: Option<String>,
    pub next_version: Option<String>,
}

impl TryFrom<NodeVersionDiff> for SerializedNodeVersionDiff {
    type Error = anyhow::Error;

    fn try_from(value: NodeVersionDiff) -> anyhow::Result<Self> {
        Ok(SerializedNodeVersionDiff {
            previous_version: value.previous_version.map(String::from),
            next_version: value.next_version.map(String::from),
        })
    }
}

impl TryFrom<SerializedNodeVersionDiff> for NodeVersionDiff {
    type Error = anyhow::Error;

    fn try_from(value: SerializedNodeVersionDiff) -> anyhow::Result<Self> {
        Ok(NodeVersionDiff {
            previous_version: value.previous_version.map(|s| s.parse()).transpose()?,
            next_version: value.next_version.map(|s| s.parse()).transpose()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Contains the metadata for a source package. Multiple [`SourcePackage`]
/// documents may be referenced in the modules table. [`ModuleMetadata`] that
/// reference old versions of [`SourcePackage`] are able to be read at all
/// subsequent versions of [`SourcePackage`].
pub struct SourcePackage {
    pub storage_key: ObjectKey,
    pub sha256: Sha256Digest,
    pub external_deps_package_id: Option<ExternalDepsPackageId>,
    pub package_size: PackageSize,
    pub node_version: Option<NodeVersion>,
}

impl SourcePackage {
    pub fn metadata_matches(&self, other: &SourcePackage) -> bool {
        // We explicitly do not compare the sha256 because it also includes creation
        // time in the hash
        self.node_version == other.node_version
            && self.external_deps_package_id == other.external_deps_package_id
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub struct PackageSize {
    pub zipped_size_bytes: usize,
    pub unzipped_size_bytes: usize,
}

impl std::ops::Add for PackageSize {
    type Output = PackageSize;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            zipped_size_bytes: self.zipped_size_bytes + rhs.zipped_size_bytes,
            unzipped_size_bytes: self.unzipped_size_bytes + rhs.unzipped_size_bytes,
        }
    }
}

impl std::ops::AddAssign for PackageSize {
    fn add_assign(&mut self, rhs: Self) {
        self.zipped_size_bytes += rhs.zipped_size_bytes;
        self.unzipped_size_bytes += rhs.unzipped_size_bytes;
    }
}

impl std::fmt::Display for PackageSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "(Zipped: {}, Unzipped, {})",
            self.zipped_size_bytes, self.unzipped_size_bytes
        )
    }
}

const MAX_UNZIPPED_PACKAGES_SIZE: usize = 230_000_000; // 230 MB - Lambda gives us 250 MB

impl PackageSize {
    pub fn verify_size(&self) -> anyhow::Result<()> {
        if self.unzipped_size_bytes >= MAX_UNZIPPED_PACKAGES_SIZE {
            anyhow::bail!(ErrorMetadata::bad_request(
                "ModulesTooLarge",
                format!(
                    "Total module size exceeded the unzipped maximum ({} > maximum size {})",
                    self.unzipped_size_bytes.format_size(BINARY),
                    MAX_UNZIPPED_PACKAGES_SIZE.format_size(BINARY)
                ),
            ),);
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedPackageSize {
    zipped_size_bytes: i64,
    unzipped_size_bytes: i64,
}

impl TryFrom<SerializedPackageSize> for PackageSize {
    type Error = anyhow::Error;

    fn try_from(value: SerializedPackageSize) -> Result<Self, Self::Error> {
        let zipped_size_bytes: usize = value.zipped_size_bytes.try_into()?;
        let unzipped_size_bytes: usize = value.unzipped_size_bytes.try_into()?;
        Ok(PackageSize {
            zipped_size_bytes,
            unzipped_size_bytes,
        })
    }
}

impl TryFrom<PackageSize> for SerializedPackageSize {
    type Error = anyhow::Error;

    fn try_from(value: PackageSize) -> Result<Self, Self::Error> {
        Ok(SerializedPackageSize {
            zipped_size_bytes: value.zipped_size_bytes.try_into()?,
            unzipped_size_bytes: value.unzipped_size_bytes.try_into()?,
        })
    }
}

codegen_convex_serialization!(PackageSize, SerializedPackageSize);

#[derive(Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash)]
pub struct SourcePackageId(DeveloperDocumentId);

impl HeapSize for SourcePackageId {
    fn heap_size(&self) -> usize {
        self.0.heap_size()
    }
}

impl From<DeveloperDocumentId> for SourcePackageId {
    fn from(id: DeveloperDocumentId) -> Self {
        Self(id)
    }
}

impl From<SourcePackageId> for String {
    fn from(value: SourcePackageId) -> Self {
        let id: DeveloperDocumentId = value.into();
        id.into()
    }
}
impl TryFrom<String> for SourcePackageId {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let id = DeveloperDocumentId::decode(&value)?;
        Ok(SourcePackageId(id))
    }
}

impl From<SourcePackageId> for DeveloperDocumentId {
    fn from(id: SourcePackageId) -> DeveloperDocumentId {
        id.0
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedSourcePackage {
    storage_key: String,
    sha256: ByteBuf,
    external_package_id: Option<String>,
    package_size: Option<SerializedPackageSize>,
    node_version: Option<String>,
}

impl TryFrom<SourcePackage> for SerializedSourcePackage {
    type Error = anyhow::Error;

    fn try_from(value: SourcePackage) -> anyhow::Result<Self> {
        Ok(SerializedSourcePackage {
            storage_key: value.storage_key.into(),
            sha256: ByteBuf::from(value.sha256.to_vec()),
            external_package_id: value
                .external_deps_package_id
                .map(|id| DeveloperDocumentId::from(id).encode()),
            package_size: Some(value.package_size.try_into()?),
            node_version: value.node_version.map(String::from),
        })
    }
}
impl TryFrom<SerializedSourcePackage> for SourcePackage {
    type Error = anyhow::Error;

    fn try_from(value: SerializedSourcePackage) -> Result<Self, Self::Error> {
        let storage_key = value.storage_key.try_into()?;
        let sha256 = value.sha256.into_vec().try_into()?;
        let external_package_id = match value.external_package_id {
            None => None,
            Some(s) => Some(DeveloperDocumentId::decode(&s)?.into()),
        };
        let package_size: PackageSize = match value.package_size {
            Some(o) => o.try_into()?,
            // Just use default for old source packages
            None => PackageSize::default(),
        };
        let node_version = match value.node_version {
            None => None,
            Some(s) => Some(s.parse()?),
        };
        Ok(Self {
            storage_key,
            sha256,
            external_deps_package_id: external_package_id,
            package_size,
            node_version,
        })
    }
}

codegen_convex_serialization!(SourcePackage, SerializedSourcePackage);
