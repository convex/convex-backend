use database::system_tables::{
    SystemIndex,
    SystemTable,
};
use serde::{
    Deserialize,
    Serialize,
};
use value::{
    codegen_convex_serialization,
    TableName,
};

pub struct ProgressTable;
impl SystemTable for ProgressTable {
    type Metadata = Progress;

    const FOR_MIGRATION: bool = true;
    const TABLE_NAME: TableName = TableName::const_new("_schema_validation_progress");

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}
/// The one field this migration reads. Stored documents also carry the owner
/// (`schemaId` or `validationId`) and the document counters; serde skips the
/// fields not declared here, and the migration only needs to tell the two
/// formats apart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    #[serde(default)]
    pub validation_id: Option<String>,
}
codegen_convex_serialization!(Progress, Progress);
