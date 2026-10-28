use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

/// SchemaState state machine:
/// ```text
/// +----------+------------------|
/// | Pending  |-+                |
/// +---+------+ |   +--------+   |
///     |         +->| Failed |   |
///     v            +--------+   |
/// +-----------+         ^       |
/// | Validated |---------+       |
/// +---+-------+         |       |
///     |                 |       |
///     v                 v       v
/// +------+            +-----------+
/// |Active|----------->|Overwritten|
/// +------+            +-----------+
/// ```
/// Invariants:
/// 1. At most one schema can be in the `Pending` or `Validated` state at a
///    time.
/// 2. At most one schema can be in the `Active` state at a time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SchemaState {
    Pending,
    Validated,
    Active,
    Failed {
        error: String,
        table_name: Option<String>,
    },
    Overwritten,
}

impl SchemaState {
    /// Indicates a schema should be cached because it can be used for writes,
    /// and it can be cached by state because at most one schema can exist in
    /// the state.
    pub fn is_unique(&self) -> bool {
        matches!(self, Self::Pending | Self::Validated | Self::Active)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SerializedSchemaState {
    Pending,
    Validated,
    Active,
    Failed {
        error: String,
        table_name: Option<String>,
    },
    Overwritten,
}

impl TryFrom<SchemaState> for SerializedSchemaState {
    type Error = anyhow::Error;

    fn try_from(s: SchemaState) -> anyhow::Result<Self> {
        Ok(match s {
            SchemaState::Pending => Self::Pending,
            SchemaState::Validated => Self::Validated,
            SchemaState::Active => Self::Active,
            SchemaState::Failed { error, table_name } => Self::Failed { error, table_name },
            SchemaState::Overwritten => Self::Overwritten,
        })
    }
}

impl TryFrom<SerializedSchemaState> for SchemaState {
    type Error = anyhow::Error;

    fn try_from(s: SerializedSchemaState) -> anyhow::Result<Self> {
        Ok(match s {
            SerializedSchemaState::Pending => Self::Pending,
            SerializedSchemaState::Validated => Self::Validated,
            SerializedSchemaState::Active => Self::Active,
            SerializedSchemaState::Failed { error, table_name } => {
                Self::Failed { error, table_name }
            },
            SerializedSchemaState::Overwritten => Self::Overwritten,
        })
    }
}

codegen_convex_serialization!(SchemaState, SerializedSchemaState);
