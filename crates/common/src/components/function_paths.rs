use std::str::FromStr;

use serde::{
    Deserialize,
    Serialize,
};
use sync_types::{
    path::PathComponent,
    CanonicalizedUdfPath,
    UdfPath,
};
use value::heap_size::HeapSize;

use super::{
    component_definition_path::ComponentDefinitionPath,
    ComponentId,
    ComponentPath,
};

pub struct ComponentDefinitionFunctionPath {
    pub component: ComponentDefinitionPath,
    pub udf_path: UdfPath,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ResolvedComponentFunctionPath {
    pub component: ComponentId,
    pub udf_path: CanonicalizedUdfPath,
    // For error messages and logging.
    pub component_path: ComponentPath,
}

impl ResolvedComponentFunctionPath {
    pub fn for_logging(self) -> CanonicalizedComponentFunctionPath {
        CanonicalizedComponentFunctionPath {
            component: self.component_path,
            udf_path: self.udf_path,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ComponentFunctionPath {
    pub component: ComponentPath,
    pub udf_path: UdfPath,
}

impl ComponentFunctionPath {
    pub fn canonicalize(self) -> CanonicalizedComponentFunctionPath {
        CanonicalizedComponentFunctionPath {
            component: self.component,
            udf_path: self.udf_path.canonicalize(),
        }
    }

    pub fn debug_str(&self) -> String {
        if !self.component.is_root() {
            tracing::warn!("ComponentFunctionPath::debug_str called on non-root path");
        }
        format!("{:?}", self.udf_path)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedComponentFunctionPath {
    pub component: String,
    pub udf_path: String,
}

impl TryFrom<SerializedComponentFunctionPath> for CanonicalizedComponentFunctionPath {
    type Error = anyhow::Error;

    fn try_from(p: SerializedComponentFunctionPath) -> anyhow::Result<Self> {
        Ok(Self {
            component: p.component.parse()?,
            udf_path: p.udf_path.parse()?,
        })
    }
}

impl TryFrom<CanonicalizedComponentFunctionPath> for SerializedComponentFunctionPath {
    type Error = anyhow::Error;

    fn try_from(p: CanonicalizedComponentFunctionPath) -> anyhow::Result<Self> {
        Ok(Self {
            component: String::from(p.component),
            udf_path: p.udf_path.to_string(),
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CanonicalizedComponentFunctionPath {
    pub component: ComponentPath,
    pub udf_path: CanonicalizedUdfPath,
}

impl CanonicalizedComponentFunctionPath {
    pub fn into_root_udf_path(self) -> anyhow::Result<CanonicalizedUdfPath> {
        anyhow::ensure!(self.component.is_root());
        Ok(self.udf_path)
    }

    pub fn debug_str(&self) -> String {
        if !self.component.is_root() {
            tracing::warn!("ComponentFunctionPath::debug_str called on non-root path");
        }
        format!("{:?}", self.udf_path)
    }

    pub fn is_system(&self) -> bool {
        self.udf_path.is_system()
    }

    pub fn into_component_and_udf_path(self) -> (ComponentPath, CanonicalizedUdfPath) {
        (self.component, self.udf_path)
    }
}

impl From<CanonicalizedComponentFunctionPath> for ComponentFunctionPath {
    fn from(p: CanonicalizedComponentFunctionPath) -> Self {
        Self {
            component: p.component,
            udf_path: p.udf_path.into(),
        }
    }
}

impl HeapSize for CanonicalizedComponentFunctionPath {
    fn heap_size(&self) -> usize {
        self.component.heap_size() + self.udf_path.heap_size()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ExportPath {
    path: CanonicalizedUdfPath,
}

impl ExportPath {
    pub fn components(&self) -> anyhow::Result<Vec<PathComponent>> {
        let mut components = vec![];
        let stripped = self.path.clone().strip();
        for c in stripped.module().components() {
            components.push(c?);
        }
        if let Some(name) = stripped.function_name() {
            components.push(name.clone().into())
        } else {
            components.push("default".parse().unwrap());
        }
        Ok(components)
    }

    pub fn is_system(&self) -> bool {
        self.path.is_system()
    }

    pub fn udf_path(&self) -> &CanonicalizedUdfPath {
        &self.path
    }
}

impl From<CanonicalizedUdfPath> for ExportPath {
    fn from(path: CanonicalizedUdfPath) -> Self {
        Self { path }
    }
}

impl From<ExportPath> for CanonicalizedUdfPath {
    fn from(p: ExportPath) -> Self {
        p.path
    }
}

impl FromStr for ExportPath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            path: CanonicalizedUdfPath::from_str(s)?,
        })
    }
}

impl From<ExportPath> for String {
    fn from(p: ExportPath) -> Self {
        p.path.to_string()
    }
}

impl HeapSize for ExportPath {
    fn heap_size(&self) -> usize {
        self.path.heap_size()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum PublicFunctionPath {
    RootExport(ExportPath),
    Component(CanonicalizedComponentFunctionPath),
    ResolvedComponent(ResolvedComponentFunctionPath),
}

impl PublicFunctionPath {
    pub fn is_system(&self) -> bool {
        match self {
            PublicFunctionPath::RootExport(path) => path.is_system(),
            PublicFunctionPath::Component(path) => path.udf_path.is_system(),
            PublicFunctionPath::ResolvedComponent(path) => path.udf_path.is_system(),
        }
    }

    pub fn udf_path(&self) -> &CanonicalizedUdfPath {
        match self {
            PublicFunctionPath::RootExport(path) => path.udf_path(),
            PublicFunctionPath::Component(path) => &path.udf_path,
            PublicFunctionPath::ResolvedComponent(path) => &path.udf_path,
        }
    }

    pub fn debug_into_component_path(self) -> CanonicalizedComponentFunctionPath {
        match self {
            PublicFunctionPath::RootExport(path) => CanonicalizedComponentFunctionPath {
                component: ComponentPath::root(),
                udf_path: path.into(),
            },
            PublicFunctionPath::Component(path) => path,
            PublicFunctionPath::ResolvedComponent(path) => path.for_logging(),
        }
    }
}

impl HeapSize for PublicFunctionPath {
    fn heap_size(&self) -> usize {
        match self {
            PublicFunctionPath::RootExport(path) => path.heap_size(),
            PublicFunctionPath::Component(path) => path.heap_size(),
            PublicFunctionPath::ResolvedComponent(path) => path.udf_path.heap_size(),
        }
    }
}
