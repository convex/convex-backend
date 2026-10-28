use std::collections::BTreeMap;

use anyhow::Context;
use common::{
    obj,
    types::{
        ObjectKey,
        StorageUuid,
    },
};
use pb::storage::FileStorageEntry as FileStorageEntryProto;
use value::{
    sha256::Sha256Digest,
    ConvexObject,
    ConvexValue,
};

#[derive(Clone, Debug, PartialEq)]
pub struct FileStorageEntry {
    pub storage_id: StorageUuid, /* Used to generate URLs. Used to be the primary storage
                                  * id before convex 1.6. */
    pub storage_key: ObjectKey, // The object key we use in the backing store (S3)
    pub sha256: Sha256Digest,   // Sha256 of contents
    pub size: i64,              // Size of file in storage
    pub content_type: Option<String>, // Optional ContentType header saved with file
}

impl TryFrom<FileStorageEntry> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(
        FileStorageEntry {
            storage_id,
            storage_key,
            sha256,
            size,
            content_type,
        }: FileStorageEntry,
    ) -> Result<Self, Self::Error> {
        let storage_key: String = storage_key.into();
        obj!(
            "storageId" => storage_id.to_string(),
            "storageKey" => storage_key,
            "sha256" => sha256,
            "size" => size,
            "contentType" => match content_type {
                None => ConvexValue::Null,
                Some(ct) => ct.try_into()?,
            },
        )
    }
}

impl TryFrom<ConvexObject> for FileStorageEntry {
    type Error = anyhow::Error;

    fn try_from(value: ConvexObject) -> Result<Self, Self::Error> {
        let mut object_fields: BTreeMap<_, _> = value.into();
        let storage_id = match object_fields.remove("storageId") {
            Some(v) => v.try_into()?,
            _ => anyhow::bail!("Missing 'storageId' in {object_fields:?}"),
        };
        let storage_key = match object_fields.remove("storageKey") {
            Some(ConvexValue::String(key)) => String::from(key).try_into()?,
            _ => anyhow::bail!("Missing 'storageKey' in {object_fields:?}"),
        };
        let sha256 = match object_fields.remove("sha256") {
            Some(sha256) => sha256.try_into()?,
            _ => anyhow::bail!("Missing 'sha256' in {object_fields:?}"),
        };
        let size = match object_fields.remove("size") {
            Some(size) => size.try_into()?,
            _ => anyhow::bail!("Missing 'size' in {object_fields:?}"),
        };
        let content_type = match object_fields.remove("contentType") {
            None | Some(ConvexValue::Null) => None,
            Some(ConvexValue::String(ct)) => Some(String::from(ct)),
            _ => anyhow::bail!("Invalid 'content_type' in {object_fields:?}"),
        };
        Ok(Self {
            storage_id,
            storage_key,
            sha256,
            size,
            content_type,
        })
    }
}

impl TryFrom<FileStorageEntryProto> for FileStorageEntry {
    type Error = anyhow::Error;

    fn try_from(entry: FileStorageEntryProto) -> anyhow::Result<Self> {
        let storage_id = entry
            .storage_id
            .context("Missing `storage_id` field")?
            .parse()?;
        let storage_key = entry
            .storage_key
            .context("Missing `storage_key` field")?
            .try_into()?;
        let sha256 = entry.sha256.context("Missing `sha256` field")?.try_into()?;
        let size = entry.size.context("Missing `size` field")?;
        Ok(FileStorageEntry {
            storage_id,
            storage_key,
            sha256,
            size,
            content_type: entry.content_type,
        })
    }
}

impl From<FileStorageEntry> for FileStorageEntryProto {
    fn from(entry: FileStorageEntry) -> Self {
        Self {
            storage_id: Some(entry.storage_id.to_string()),
            storage_key: Some(entry.storage_key.into()),
            sha256: Some(entry.sha256.to_vec()),
            size: Some(entry.size),
            content_type: entry.content_type,
        }
    }
}
