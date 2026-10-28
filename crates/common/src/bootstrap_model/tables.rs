use serde::{
    Deserialize,
    Serialize,
};
use value::{
    codegen_convex_serialization,
    TableNamespace,
    TableNumber,
};

use crate::types::TableName;

pub const TABLES_TABLE: TableName = TableName::const_new("_tables");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableState {
    /// The table exists. It was created and has not been deleted.
    Active,
    /// This table is in the process of being imported with snapshot import.
    /// New documents may be created in this table by
    /// `ImportFacingModel::insert`. It may have the same name and/or number
    /// as an existing Active table. It appears in only one direction of
    /// TableMapping, so to find its mapping you must look it up by TableId,
    /// not TableNumber or TableName.
    Hidden,
    /// The table has been marked as deleted. Documents in the table may still
    /// exist, but they should be ignored.
    /// No new documents may be created in the table.
    /// A handful of legacy tables were not marked as Deleting but instead their
    /// _tables entry was deleted. Such tables should be treated the same as
    /// Deleting -- for now. Eventually we may want to clean up Deleting tables
    /// and delete the _tables rows or create a new Deleted state.
    Deleting,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TableMetadata {
    pub name: TableName,
    pub number: TableNumber,
    pub state: TableState,
    pub namespace: TableNamespace,
}

impl TableMetadata {
    pub fn is_active(&self) -> bool {
        matches!(self.state, TableState::Active)
    }
}

impl TableMetadata {
    pub fn new(namespace: TableNamespace, name: TableName, number: TableNumber) -> Self {
        Self {
            name,
            number,
            state: TableState::Active,
            namespace,
        }
    }

    pub fn new_with_state(
        namespace: TableNamespace,
        name: TableName,
        number: TableNumber,
        state: TableState,
    ) -> Self {
        Self {
            name,
            number,
            state,
            namespace,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct SerializedTableMetadata {
    name: String,
    number: i64,
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    namespace: Option<SerializedTableNamespace>,
}

impl TryFrom<TableMetadata> for SerializedTableMetadata {
    type Error = anyhow::Error;

    fn try_from(m: TableMetadata) -> anyhow::Result<Self> {
        Ok(Self {
            name: m.name.into(),
            number: u32::from(m.number) as i64,
            state: match m.state {
                TableState::Active => "active".to_owned(),
                TableState::Deleting => "deleting".to_owned(),
                TableState::Hidden => "hidden".to_owned(),
            },
            namespace: table_namespace_to_serialized(m.namespace)?,
        })
    }
}

impl TryFrom<SerializedTableMetadata> for TableMetadata {
    type Error = anyhow::Error;

    fn try_from(m: SerializedTableMetadata) -> anyhow::Result<Self> {
        Ok(Self {
            name: m.name.parse()?,
            number: u32::try_from(m.number)?.try_into()?,
            state: match &m.state[..] {
                "active" => TableState::Active,
                "deleting" => TableState::Deleting,
                "hidden" => TableState::Hidden,
                s => anyhow::bail!("invalid table state {s}"),
            },
            namespace: table_namespace_from_serialized(m.namespace)?,
        })
    }
}

codegen_convex_serialization!(TableMetadata, SerializedTableMetadata);

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SerializedTableNamespace {
    ByComponent { id: String },
}

pub fn table_namespace_from_serialized(
    m: Option<SerializedTableNamespace>,
) -> anyhow::Result<TableNamespace> {
    Ok(match m {
        None => TableNamespace::Global,
        Some(SerializedTableNamespace::ByComponent { id }) => {
            TableNamespace::ByComponent(id.parse()?)
        },
    })
}

pub fn table_namespace_to_serialized(
    m: TableNamespace,
) -> anyhow::Result<Option<SerializedTableNamespace>> {
    match m {
        TableNamespace::Global => Ok(None),
        TableNamespace::ByComponent(id) => Ok(Some(SerializedTableNamespace::ByComponent {
            id: id.to_string(),
        })),
    }
}
