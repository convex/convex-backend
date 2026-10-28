use serde::{
    Deserialize,
    Serialize,
};

use super::indexed_fields::IndexedFields;
use crate::paths::FieldPath;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseIndexSpec {
    /// Ordered field(s) to index. The "unindexed" primary key ordering of
    /// documents by [`DocumentId`] is represented by an empty vector.
    pub fields: IndexedFields,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SerializedDatabaseIndexSpec {
    fields: Vec<String>,
}

impl From<DatabaseIndexSpec> for SerializedDatabaseIndexSpec {
    fn from(config: DatabaseIndexSpec) -> Self {
        Self {
            fields: Vec::<FieldPath>::from(config.fields)
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }
}

impl TryFrom<SerializedDatabaseIndexSpec> for DatabaseIndexSpec {
    type Error = anyhow::Error;

    fn try_from(config: SerializedDatabaseIndexSpec) -> anyhow::Result<Self> {
        Ok(Self {
            fields: config
                .fields
                .into_iter()
                .map(|p| p.parse())
                .collect::<anyhow::Result<Vec<FieldPath>>>()?
                .try_into()?,
        })
    }
}
