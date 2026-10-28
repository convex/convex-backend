use std::{
    collections::BTreeMap,
    sync::LazyLock,
};

use async_trait::async_trait;
use common::{
    document::{
        DeveloperDocument,
        ParseDocument,
        ParsedDocument,
        ResolvedDocument,
        CREATION_TIME_FIELD,
        ID_FIELD,
    },
    virtual_system_mapping::{
        GetDocument,
        VirtualSystemDocMapper,
        VirtualSystemMapping,
    },
};
use semver::Version;
use value::{
    val,
    ConvexObject,
    ConvexValue,
    FieldName,
    TableMapping,
};

use super::{
    types::FileStorageEntry,
    FILE_STORAGE_TABLE,
};

// First release of virtual tables
static MIN_NPM_VERSION_FILE_STORAGE_V1: LazyLock<Version> =
    LazyLock::new(|| Version::parse("1.6.1").unwrap());

// sha256 of the file now uses base64 instead of hex for consistency
static MIN_NPM_VERSION_FILE_STORAGE_V2: LazyLock<Version> =
    LazyLock::new(|| Version::parse("1.9.0").unwrap());

pub struct FileStorageDocMapper;

#[async_trait]
impl VirtualSystemDocMapper for FileStorageDocMapper {
    async fn system_to_virtual_doc(
        &self,
        _tx: &mut dyn GetDocument,
        virtual_system_mapping: &VirtualSystemMapping,
        doc: ResolvedDocument,
        table_mapping: &TableMapping,
        version: Version,
    ) -> anyhow::Result<DeveloperDocument> {
        // Note: in the future we may support different versions of our virtual table
        // APIs, which we determine based on the NPM client version
        let system_table_name = table_mapping.tablet_name(doc.id().tablet_id)?;
        if system_table_name == FILE_STORAGE_TABLE.clone()
            && version < *MIN_NPM_VERSION_FILE_STORAGE_V1
        {
            anyhow::bail!("System document cannot be converted to a virtual document")
        }
        let metadata: ParsedDocument<FileStorageEntry> = doc.clone().parse()?;
        let metadata: FileStorageEntry = metadata.into_value();
        let sha256 = if version >= *MIN_NPM_VERSION_FILE_STORAGE_V2 {
            metadata.sha256.as_base64()
        } else {
            metadata.sha256.as_hex()
        };
        let public_metadata = PublicFileMetadata {
            sha256,
            size: metadata.size as f64,
            content_type: metadata.content_type,
        };
        let mut public_metadata_resolved: ConvexObject = public_metadata.try_into()?;

        let virtual_developer_id =
            virtual_system_mapping.system_resolved_id_to_virtual_developer_id(doc.id())?;

        let mut fields: BTreeMap<_, _> = public_metadata_resolved.into();
        fields.insert(ID_FIELD.to_owned().into(), virtual_developer_id.into());
        fields.insert(
            CREATION_TIME_FIELD.to_owned().into(),
            ConvexValue::from(f64::from(doc.creation_time())),
        );
        public_metadata_resolved = fields.try_into()?;

        let public_doc = DeveloperDocument::new(
            virtual_developer_id,
            doc.creation_time(),
            public_metadata_resolved,
        );
        Ok(public_doc)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PublicFileMetadata {
    sha256: String,               // Hex-encoded Sha256 of contents
    size: f64,                    // Size of file in storage
    content_type: Option<String>, // Optional ContentType header saved with file
}

impl TryFrom<PublicFileMetadata> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(
        PublicFileMetadata {
            sha256,
            size,
            content_type,
        }: PublicFileMetadata,
    ) -> Result<Self, Self::Error> {
        let mut obj: BTreeMap<FieldName, ConvexValue> = BTreeMap::new();
        obj.insert("sha256".parse()?, sha256.try_into()?);
        obj.insert("size".parse()?, ConvexValue::Float64(size));
        obj.insert(
            "contentType".parse()?,
            match content_type {
                None => val!(null),
                Some(ct) => val!(ct),
            },
        );
        ConvexObject::try_from(obj)
    }
}
