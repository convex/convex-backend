use std::fmt::Debug;

use anyhow::Ok;
use common::bootstrap_model::index::{
    database_index::{
        DatabaseIndexSpec,
        SerializedDatabaseIndexSpec,
    },
    text_index::{
        SerializedTextIndexSpec,
        TextIndexSpec,
    },
    vector_index::{
        SerializedVectorIndexSpec,
        VectorIndexSpec,
    },
    IndexConfig,
};
use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

// Index config that's specified by the developer - including spec + staged
// state
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeveloperIndexConfig {
    spec: DeveloperIndexSpec,
    pub staged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeveloperIndexSpec {
    /// Standard database index.
    Database(DatabaseIndexSpec),
    /// Full text search index.
    Search(TextIndexSpec),
    /// Vector search index.
    Vector(VectorIndexSpec),
}

impl From<IndexConfig> for DeveloperIndexConfig {
    fn from(value: IndexConfig) -> Self {
        let staged = value.is_staged();
        let spec = match value {
            IndexConfig::Database { spec, .. } => DeveloperIndexSpec::Database(spec),
            IndexConfig::Text { spec, .. } => DeveloperIndexSpec::Search(spec),
            IndexConfig::Vector { spec, .. } => DeveloperIndexSpec::Vector(spec),
        };
        Self { spec, staged }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SerializedNamedDeveloperIndexConfig {
    pub name: String,
    #[serde(flatten)]
    pub index_config: SerializedDeveloperIndexConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub struct SerializedDeveloperIndexConfig {
    #[serde(flatten)]
    spec: SerializedDeveloperIndexSpec,
    #[serde(default)]
    staged: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SerializedDeveloperIndexSpec {
    Database(SerializedDatabaseIndexSpec),
    Search(SerializedTextIndexSpec),
    Vector(SerializedVectorIndexSpec),
}

impl From<DeveloperIndexConfig> for SerializedDeveloperIndexConfig {
    fn from(index_config: DeveloperIndexConfig) -> Self {
        let spec = match index_config.spec {
            DeveloperIndexSpec::Database(spec) => {
                SerializedDeveloperIndexSpec::Database(spec.into())
            },
            DeveloperIndexSpec::Search(spec) => SerializedDeveloperIndexSpec::Search(spec.into()),
            DeveloperIndexSpec::Vector(spec) => SerializedDeveloperIndexSpec::Vector(spec.into()),
        };
        Self {
            spec,
            staged: index_config.staged,
        }
    }
}

impl TryFrom<SerializedDeveloperIndexConfig> for DeveloperIndexConfig {
    type Error = anyhow::Error;

    fn try_from(index_config: SerializedDeveloperIndexConfig) -> anyhow::Result<Self> {
        let spec = match index_config.spec {
            SerializedDeveloperIndexSpec::Database(spec) => {
                DeveloperIndexSpec::Database(spec.try_into()?)
            },
            SerializedDeveloperIndexSpec::Search(spec) => {
                DeveloperIndexSpec::Search(spec.try_into()?)
            },
            SerializedDeveloperIndexSpec::Vector(spec) => {
                DeveloperIndexSpec::Vector(spec.try_into()?)
            },
        };
        Ok(Self {
            spec,
            staged: index_config.staged,
        })
    }
}

codegen_convex_serialization!(DeveloperIndexConfig, SerializedDeveloperIndexConfig);
