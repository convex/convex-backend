use common::bootstrap_model::components::definition::ComponentDefinitionMetadata;
use value::TableName;

use crate::system_tables::{
    SystemIndex,
    SystemTable,
};

pub const COMPONENT_DEFINITIONS_TABLE: TableName = TableName::const_new("_component_definitions");

pub struct ComponentDefinitionsTable;

impl SystemTable for ComponentDefinitionsTable {
    type Metadata = ComponentDefinitionMetadata;

    const TABLE_NAME: TableName = COMPONENT_DEFINITIONS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        Vec::new()
    }
}
