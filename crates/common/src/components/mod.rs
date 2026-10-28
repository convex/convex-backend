use std::str::FromStr;

use value::{
    DeveloperDocumentId,
    TableNamespace,
};

mod component_definition_path;
mod component_path;
mod function_paths;
mod module_paths;
mod reference;
mod resource;

pub use self::{
    component_definition_path::ComponentDefinitionPath,
    component_path::{
        ComponentName,
        ComponentPath,
    },
    function_paths::{
        CanonicalizedComponentFunctionPath,
        ComponentDefinitionFunctionPath,
        ComponentFunctionPath,
        ExportPath,
        PublicFunctionPath,
        ResolvedComponentFunctionPath,
    },
    module_paths::CanonicalizedComponentModulePath,
    reference::Reference,
    resource::{
        Resource,
        SerializedResource,
    },
};

// Globally unique system-assigned ID for a component.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ComponentId {
    Root,
    Child(DeveloperDocumentId),
}

impl ComponentId {
    pub fn new(is_root: bool, id: DeveloperDocumentId) -> Self {
        if is_root {
            ComponentId::Root
        } else {
            ComponentId::Child(id)
        }
    }

    pub fn is_root(&self) -> bool {
        matches!(self, ComponentId::Root)
    }

    pub fn serialize_to_string(&self) -> Option<String> {
        match self {
            ComponentId::Root => None,
            ComponentId::Child(id) => Some(id.to_string()),
        }
    }

    pub fn deserialize_from_string(s: Option<&str>) -> anyhow::Result<Self> {
        match s {
            None => Ok(ComponentId::Root),
            Some(s) => Ok(ComponentId::Child(DeveloperDocumentId::from_str(s)?)),
        }
    }
}

impl From<ComponentId> for TableNamespace {
    fn from(value: ComponentId) -> Self {
        match value {
            ComponentId::Root => TableNamespace::root_component(),
            ComponentId::Child(id) => TableNamespace::ByComponent(id),
        }
    }
}

impl From<TableNamespace> for ComponentId {
    fn from(value: TableNamespace) -> Self {
        match value {
            TableNamespace::Global => ComponentId::Root,
            TableNamespace::ByComponent(id) => ComponentId::Child(id),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ComponentDefinitionId {
    Root,
    Child(DeveloperDocumentId),
}

impl ComponentDefinitionId {
    pub fn is_root(&self) -> bool {
        matches!(self, ComponentDefinitionId::Root)
    }
}
