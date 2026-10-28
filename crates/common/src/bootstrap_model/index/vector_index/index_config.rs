use std::collections::BTreeSet;

use serde::{
    Deserialize,
    Serialize,
};
use value::{
    codegen_convex_serialization,
    FieldPath,
};

use super::VectorDimensions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorIndexSpec {
    // Dimensions of the vectors
    pub dimensions: VectorDimensions,

    /// The field to index for vector search.
    pub vector_field: FieldPath,

    /// Other fields to index for equality filtering.
    pub filter_fields: BTreeSet<FieldPath>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SerializedVectorIndexSpec {
    // Support legacy alpha users with the old dimension field.
    #[serde(alias = "dimension")]
    dimensions: i64,
    vector_field: String,
    filter_fields: Vec<String>,
}

impl From<VectorIndexSpec> for SerializedVectorIndexSpec {
    fn from(config: VectorIndexSpec) -> Self {
        Self {
            dimensions: u32::from(config.dimensions) as i64,
            vector_field: config.vector_field.into(),
            filter_fields: config.filter_fields.into_iter().map(String::from).collect(),
        }
    }
}

impl TryFrom<SerializedVectorIndexSpec> for VectorIndexSpec {
    type Error = anyhow::Error;

    fn try_from(config: SerializedVectorIndexSpec) -> anyhow::Result<Self> {
        Ok(Self {
            dimensions: VectorDimensions::try_from(u32::try_from(config.dimensions)?)?,
            vector_field: config.vector_field.parse()?,
            filter_fields: config
                .filter_fields
                .into_iter()
                .map(|p| p.parse())
                .collect::<anyhow::Result<BTreeSet<FieldPath>>>()?,
        })
    }
}

codegen_convex_serialization!(VectorIndexSpec, SerializedVectorIndexSpec);

impl TryFrom<pb::searchlight::VectorIndexConfig> for VectorIndexSpec {
    type Error = anyhow::Error;

    fn try_from(proto: pb::searchlight::VectorIndexConfig) -> anyhow::Result<Self> {
        Ok(VectorIndexSpec {
            dimensions: VectorDimensions::try_from(proto.dimension)?,
            vector_field: proto
                .vector_field_path
                .ok_or_else(|| anyhow::format_err!("Missing vector_field_path"))?
                .try_into()?,
            filter_fields: proto
                .filter_fields
                .into_iter()
                .map(|i| i.try_into())
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .collect(),
        })
    }
}

impl From<VectorIndexSpec> for pb::searchlight::VectorIndexConfig {
    fn from(config: VectorIndexSpec) -> Self {
        pb::searchlight::VectorIndexConfig {
            dimension: u32::from(config.dimensions),
            vector_field_path: Some(config.vector_field.into()),
            filter_fields: config
                .filter_fields
                .into_iter()
                .map(|f| f.into())
                .collect::<Vec<_>>(),
        }
    }
}
