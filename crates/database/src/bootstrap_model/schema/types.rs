use common::{
    json::JsonForm as _,
    schemas::DatabaseSchema,
};
use serde::{
    Deserialize,
    Serialize,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaDiff {
    pub previous_schema: Option<DatabaseSchema>,
    pub next_schema: Option<DatabaseSchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedSchemaDiff {
    // NOTE: not camel-case.
    pub previous_schema: Option<String>,
    pub next_schema: Option<String>,
}

impl TryFrom<SchemaDiff> for SerializedSchemaDiff {
    type Error = anyhow::Error;

    fn try_from(diff: SchemaDiff) -> Result<Self, Self::Error> {
        Ok(Self {
            previous_schema: diff
                .previous_schema
                .map(|schema| schema.json_serialize())
                .transpose()?,
            next_schema: diff
                .next_schema
                .map(|schema| schema.json_serialize())
                .transpose()?,
        })
    }
}

impl TryFrom<SerializedSchemaDiff> for SchemaDiff {
    type Error = anyhow::Error;

    fn try_from(diff: SerializedSchemaDiff) -> Result<Self, Self::Error> {
        Ok(Self {
            previous_schema: diff
                .previous_schema
                .map(|schema| DatabaseSchema::json_deserialize(&schema))
                .transpose()?,
            next_schema: diff
                .next_schema
                .map(|schema| DatabaseSchema::json_deserialize(&schema))
                .transpose()?,
        })
    }
}
