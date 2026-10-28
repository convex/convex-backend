//! Frozen, partial `_index` model as of migration 130.
//!
//! This migration only adds `persistenceIndexId` to database index configs, so
//! it models just the config discriminant (`type`) and that one field. Every
//! other field — both at the document top level and inside `config` — is
//! captured in a `#[serde(flatten)]` catch-all of raw [`ConvexValue`]s, so a
//! patched document round-trips losslessly and adds nothing but the new field.

use std::collections::BTreeMap;

use database::system_tables::{
    SystemIndex,
    SystemTable,
};
use serde::{
    Deserialize,
    Serialize,
};
use value::{
    serde::{
        from_object,
        to_object,
        ConvexSerializable,
    },
    ConvexObject,
    ConvexValue,
    TableName,
};

/// Partial view of an `_index` document. Only `config` is modeled; the rest of
/// the document (e.g. `table_id`, `descriptor`) is preserved opaquely.
#[derive(Clone, Serialize, Deserialize)]
pub struct SerializedIndexMetadata {
    pub config: SerializedIndexConfig,
    #[serde(flatten)]
    rest: BTreeMap<String, ConvexValue>,
}

/// The `config` sub-object, mirroring `SerializedIndexConfig` in
/// `common::bootstrap_model::index::index_config`. Only the `database` variant
/// is modeled; search and vector indexes deserialize to
/// [`SerializedIndexConfig::Other`] and are neither patched nor re-serialized.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SerializedIndexConfig {
    #[serde(rename_all = "camelCase")]
    Database {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        persistence_index_id: Option<i64>,
        #[serde(flatten)]
        rest: BTreeMap<String, ConvexValue>,
    },
    #[serde(other)]
    Other,
}

impl TryFrom<ConvexObject> for SerializedIndexMetadata {
    type Error = anyhow::Error;

    fn try_from(object: ConvexObject) -> anyhow::Result<Self> {
        from_object(object)
    }
}

impl TryFrom<SerializedIndexMetadata> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(metadata: SerializedIndexMetadata) -> anyhow::Result<Self> {
        to_object(metadata)
    }
}

impl ConvexSerializable for SerializedIndexMetadata {
    type Serialized = Self;
}

pub struct IndexTable;

impl SystemTable for IndexTable {
    type Metadata = SerializedIndexMetadata;

    const FOR_MIGRATION: bool = true;
    const TABLE_NAME: TableName = TableName::const_new("_index");

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}
