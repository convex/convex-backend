use std::path::PathBuf;

use migrations_model::DatabaseVersion;
use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseGlobals {
    /// Migration version of the database.
    pub version: DatabaseVersion,
    /// Prefix to put on the aws bucket/lambda keys to make it unguessable
    pub aws_prefix_secret: String,
    /// Storage used by this backend. Cannot be changed once initialized
    /// None - means that no storage type was specified yet
    pub storage_type: Option<StorageType>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedDatabaseGlobals {
    version: DatabaseVersion,
    aws_prefix_secret: String,
    storage_type: Option<SerializedStorageType>,
}

impl From<DatabaseGlobals> for SerializedDatabaseGlobals {
    fn from(value: DatabaseGlobals) -> Self {
        Self {
            version: value.version,
            aws_prefix_secret: value.aws_prefix_secret,
            storage_type: value.storage_type.map(|s| s.into()),
        }
    }
}

impl From<SerializedDatabaseGlobals> for DatabaseGlobals {
    fn from(value: SerializedDatabaseGlobals) -> Self {
        Self {
            version: value.version,
            aws_prefix_secret: value.aws_prefix_secret,
            storage_type: value.storage_type.map(|s| s.into()),
        }
    }
}

codegen_convex_serialization!(DatabaseGlobals, SerializedDatabaseGlobals);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageType {
    S3 { s3_prefix: String },
    Local { dir: String },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "tag")]
#[serde(rename_all = "camelCase")]
pub enum SerializedStorageType {
    #[serde(rename_all = "camelCase")]
    S3 { s3_prefix: String },
    #[serde(rename_all = "camelCase")]
    Local { dir: String },
}

impl From<StorageType> for SerializedStorageType {
    fn from(value: StorageType) -> Self {
        match value {
            StorageType::S3 { s3_prefix } => SerializedStorageType::S3 { s3_prefix },
            StorageType::Local { dir } => SerializedStorageType::Local { dir },
        }
    }
}

impl From<SerializedStorageType> for StorageType {
    fn from(value: SerializedStorageType) -> Self {
        match value {
            SerializedStorageType::S3 { s3_prefix } => StorageType::S3 { s3_prefix },
            SerializedStorageType::Local { dir } => StorageType::Local { dir },
        }
    }
}

codegen_convex_serialization!(StorageType, SerializedStorageType);

#[derive(Clone, Debug)]
pub enum StorageTagInitializer {
    S3,
    Local { dir: PathBuf },
}
