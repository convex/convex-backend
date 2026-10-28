use common::types::PersistenceIndexId;
use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NextPersistenceIndexIdMetadata {
    pub next_id: PersistenceIndexId,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedNextPersistenceIndexIdMetadata {
    next_id: i64,
}

impl From<NextPersistenceIndexIdMetadata> for SerializedNextPersistenceIndexIdMetadata {
    fn from(metadata: NextPersistenceIndexIdMetadata) -> Self {
        Self {
            next_id: i64::from(metadata.next_id.value()),
        }
    }
}

impl TryFrom<SerializedNextPersistenceIndexIdMetadata> for NextPersistenceIndexIdMetadata {
    type Error = anyhow::Error;

    fn try_from(serialized: SerializedNextPersistenceIndexIdMetadata) -> anyhow::Result<Self> {
        Ok(Self {
            next_id: PersistenceIndexId::try_from(u32::try_from(serialized.next_id)?)?,
        })
    }
}

codegen_convex_serialization!(
    NextPersistenceIndexIdMetadata,
    SerializedNextPersistenceIndexIdMetadata
);
