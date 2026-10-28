use serde::{
    Deserialize,
    Serialize,
};
use value::{
    codegen_convex_serialization,
    DeveloperDocumentId,
};

/// Schema validation progress is written by the SchemaWorker for each
/// `Pending` attempt in `_schema_validations`. There is at most one progress
/// document per attempt, and every `Pending` attempt has one. Counters are
/// separate from attempts so flushing progress cannot invalidate a document
/// transaction that records a validation failure.
///
/// Documents keyed by `schemaId` instead of `validationId` were the aggregate
/// format written before attempts existed; migration 133 deleted them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaValidationProgress {
    /// The attempt these counters belong to. Should correspond to a document in
    /// the `_schema_validations` table.
    pub validation_id: DeveloperDocumentId,
    /// The number of documents in the attempt's table validated so far.
    pub num_docs_validated: u64,
    /// The number of total documents that need to be validated. Note this is
    /// approximate because there could be changes since the time we wrote this
    /// value from the table summary when the attempt was created. It's
    /// possible for num_docs_validated to exceed total_docs. This field is None
    /// if there is no table summary available.
    pub total_docs: Option<u64>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedSchemaValidationProgress {
    pub validation_id: String,
    pub num_docs_validated: i64,
    pub total_docs: Option<i64>,
}

impl TryFrom<SchemaValidationProgress> for SerializedSchemaValidationProgress {
    type Error = anyhow::Error;

    fn try_from(value: SchemaValidationProgress) -> anyhow::Result<Self> {
        Ok(Self {
            validation_id: value.validation_id.to_string(),
            num_docs_validated: value.num_docs_validated.try_into()?,
            total_docs: value.total_docs.map(|v| v.try_into()).transpose()?,
        })
    }
}

impl TryFrom<SerializedSchemaValidationProgress> for SchemaValidationProgress {
    type Error = anyhow::Error;

    fn try_from(value: SerializedSchemaValidationProgress) -> anyhow::Result<Self> {
        Ok(Self {
            validation_id: value.validation_id.parse()?,
            num_docs_validated: value.num_docs_validated.try_into()?,
            total_docs: value.total_docs.map(|v| v.try_into()).transpose()?,
        })
    }
}
codegen_convex_serialization!(SchemaValidationProgress, SerializedSchemaValidationProgress);
