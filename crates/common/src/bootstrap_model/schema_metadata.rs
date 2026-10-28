use json_trait::JsonForm as _;
use packed_value::StringBuffer;
use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

use super::schema_state::{
    SchemaState,
    SerializedSchemaState,
};
use crate::schemas::DatabaseSchema;

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaMetadata {
    pub state: SchemaState,
    pub raw_schema: StringBuffer,
}

impl SchemaMetadata {
    pub fn database_schema(&self) -> anyhow::Result<DatabaseSchema> {
        DatabaseSchema::json_deserialize(&self.raw_schema)
    }

    pub fn new(state: SchemaState, schema: DatabaseSchema) -> anyhow::Result<Self> {
        let raw_schema = schema.json_serialize()?;
        Ok(Self {
            state,
            raw_schema: StringBuffer::new(raw_schema),
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct SerializedSchemaMetadata {
    state: SerializedSchemaState,
    schema: StringBuffer,
}

impl TryFrom<SchemaMetadata> for SerializedSchemaMetadata {
    type Error = anyhow::Error;

    fn try_from(s: SchemaMetadata) -> anyhow::Result<Self> {
        Ok(Self {
            state: s.state.try_into()?,
            schema: s.raw_schema,
        })
    }
}

impl TryFrom<SerializedSchemaMetadata> for SchemaMetadata {
    type Error = anyhow::Error;

    fn try_from(s: SerializedSchemaMetadata) -> anyhow::Result<Self> {
        Ok(Self {
            state: s.state.try_into()?,
            raw_schema: s.schema,
        })
    }
}

codegen_convex_serialization!(SchemaMetadata, SerializedSchemaMetadata);
