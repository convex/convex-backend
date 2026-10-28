use std::{
    fmt::{
        Debug,
        Display,
    },
    ops::Deref,
    path::PathBuf,
    str::FromStr,
};

use errors::ErrorMetadata;
use itertools::Itertools;
use sync_types::path::check_valid_path_component;
use value::{
    heap_size::{
        HeapSize,
        WithHeapSize,
    },
    identifier::Identifier,
};

// All components under a component have a unique `ComponentName`. For example,
// the root app component may have a waitlist component identified by
// "chatWaitlist".
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ComponentName(Identifier);

impl ComponentName {
    pub fn min() -> Self {
        Self(Identifier::min())
    }
}

impl FromStr for ComponentName {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

impl From<Identifier> for ComponentName {
    fn from(id: Identifier) -> Self {
        Self(id)
    }
}

impl From<ComponentName> for Identifier {
    fn from(name: ComponentName) -> Identifier {
        name.0
    }
}

impl From<ComponentName> for String {
    fn from(name: ComponentName) -> String {
        name.0.to_string()
    }
}

impl Deref for ComponentName {
    type Target = Identifier;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl HeapSize for ComponentName {
    fn heap_size(&self) -> usize {
        self.0.capacity()
    }
}

// Path within the component tree for a particular component. Note that this
// path can potentially change when the component tree changes during a push, so
// we should resolve this path to a `ComponentId` within a transaction
// as soon as possible.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
pub struct ComponentPath {
    path: WithHeapSize<Vec<ComponentName>>,
}

impl ComponentPath {
    pub fn root() -> Self {
        Self {
            path: WithHeapSize::default(),
        }
    }

    pub fn is_root(&self) -> bool {
        self.path.is_empty()
    }

    pub fn parent(&self) -> Option<(Self, ComponentName)> {
        let mut path = self.path.clone();
        match path.pop() {
            None => None,
            Some(name) => Some((Self { path }, name)),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &ComponentName> {
        self.path.iter()
    }

    pub fn join(&self, name: ComponentName) -> Self {
        let mut path = self.path.clone();
        path.push(name);
        Self { path }
    }

    /// Returns a debug or error string to put immediately after something which
    /// should be scoped to this component,
    /// like `format!("'{udf_path}'{}", component_path.in_component_str())`.
    pub fn in_component_str(&self) -> String {
        if self.is_root() {
            "".to_string()
        } else {
            format!(" in '{}'", String::from(self.clone()))
        }
    }

    pub fn deserialize(path: Option<&str>) -> anyhow::Result<Self> {
        match path {
            None => Ok(ComponentPath::root()),
            Some(p) => p.parse().map_err(|e: anyhow::Error| {
                let msg = e.to_string();
                e.context(ErrorMetadata::bad_request("InvalidComponentPath", msg))
            }),
        }
    }

    pub fn serialize(self) -> Option<String> {
        if self.is_root() {
            None
        } else {
            Some(String::from(self))
        }
    }
}

impl Deref for ComponentPath {
    type Target = [ComponentName];

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl From<Vec<ComponentName>> for ComponentPath {
    fn from(path: Vec<ComponentName>) -> Self {
        Self {
            path: path.into_iter().collect(),
        }
    }
}

impl ComponentPath {
    pub fn push(&self, name: ComponentName) -> Self {
        let mut path = self.path.clone();
        path.push(name);
        Self { path }
    }
}

impl From<ComponentPath> for String {
    fn from(path: ComponentPath) -> String {
        path.path.iter().map(|name| &***name).join("/")
    }
}

impl Display for ComponentPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from(self.clone()))
    }
}

impl Debug for ComponentPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", *self.path)
    }
}

impl FromStr for ComponentPath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            path: if s.is_empty() {
                WithHeapSize::default()
            } else {
                s.split('/').map(str::parse).try_collect()?
            },
        })
    }
}

impl HeapSize for ComponentPath {
    fn heap_size(&self) -> usize {
        self.path.heap_size()
    }
}

// Path relative to the `convex` directory for each bundle.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ComponentDefinitionPath(String);

impl FromStr for ComponentDefinitionPath {
    type Err = anyhow::Error;

    fn from_str(p: &str) -> Result<Self, Self::Err> {
        let path = PathBuf::from(p);
        for component in path.components() {
            match component {
                std::path::Component::Normal(c) => {
                    let s = c.to_str().ok_or_else(|| {
                        anyhow::anyhow!("Path {p} contains an invalid Unicode character.")
                    })?;
                    check_valid_path_component(s)?;
                },
                // Component paths are allowed to have `..` (since they're relative from the root
                // component's source directory).
                std::path::Component::ParentDir => (),
                std::path::Component::RootDir => {
                    anyhow::bail!("Component paths must be relative ({p} is absolute).")
                },
                c => anyhow::bail!("Invalid path component {c:?} in {p}."),
            }
        }
        Ok(Self(path.into_os_string().into_string().map_err(|_| {
            anyhow::anyhow!("Path {p} contains an invalid Unicode character.")
        })?))
    }
}

impl From<ComponentDefinitionPath> for String {
    fn from(value: ComponentDefinitionPath) -> Self {
        value.0
    }
}

impl From<ComponentPath> for pb::common::ComponentPath {
    fn from(path: ComponentPath) -> Self {
        Self {
            path: path.iter().map(|name| name.to_string()).collect(),
        }
    }
}

impl TryFrom<pb::common::ComponentPath> for ComponentPath {
    type Error = anyhow::Error;

    fn try_from(value: pb::common::ComponentPath) -> Result<Self, Self::Error> {
        let component_names: Vec<ComponentName> = value
            .path
            .into_iter()
            .map(|name| name.parse::<ComponentName>())
            .try_collect()?;
        Ok(component_names.into())
    }
}
