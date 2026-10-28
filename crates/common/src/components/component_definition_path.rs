use std::{
    fmt,
    ops::Deref,
    path::{
        Component as PathComponent,
        PathBuf,
    },
    str::FromStr,
};

use anyhow::Context;

// Path relative to a project's `convex/` directory for each component
// definition's folder. This path is project-level and originates from
// a developer's source code.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ComponentDefinitionPath {
    path: PathBuf,
}

impl ComponentDefinitionPath {
    pub fn min() -> Self {
        ComponentDefinitionPath {
            path: PathBuf::new(),
        }
    }

    pub fn root() -> Self {
        ComponentDefinitionPath {
            path: PathBuf::new(),
        }
    }

    pub fn is_root(&self) -> bool {
        self.path.as_os_str().is_empty()
    }
}

// Windows has a maximum path limit of 260 (UTF-16) codepoints, which then is at
// most 260 * 4 = 1040 bytes, which is then at most 1387 bytes encoded as
// base64. Since we encode component definition paths as base64 within our
// esbuild plugin, permit up to 2048 bytes here to be safe.
const MAX_DEFINITION_PATH_COMPONENT_LEN: usize = 2048;

fn check_valid_definition_path_component(s: &str) -> anyhow::Result<()> {
    if s.len() > MAX_DEFINITION_PATH_COMPONENT_LEN {
        anyhow::bail!(
            "Path component is too long ({} > maximum {}): {}...",
            s.len(),
            MAX_DEFINITION_PATH_COMPONENT_LEN,
            &s[..s.len().min(32)]
        );
    }
    if s.is_empty() {
        anyhow::bail!("Path component is empty");
    }
    if !s
        .chars()
        .all(|c| c.is_ascii() && !c.is_ascii_control() && c != '/' && c != '\\')
    {
        anyhow::bail!("Path component {s} can only include non-control ASCII characters.");
    }
    Ok(())
}

impl FromStr for ComponentDefinitionPath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let path = PathBuf::from(s);
        for component in path.components() {
            match component {
                PathComponent::Normal(c) => {
                    let s = c
                        .to_str()
                        .with_context(|| format!("Path {s} has an invalid Unicode character"))?;
                    check_valid_definition_path_component(s)?;
                },
                // Component paths are allowed to have `..` (since they're relative from the root
                // component's source directory).
                PathComponent::ParentDir => (),
                PathComponent::RootDir => {
                    anyhow::bail!("Component paths must be relative ({s} is absolute).")
                },
                c => anyhow::bail!("Invalid path component {c:?} in {s}."),
            }
        }
        path.as_os_str()
            .to_str()
            .with_context(|| format!("Path {s} has an invalid Unicode character"))?;
        Ok(ComponentDefinitionPath { path })
    }
}

impl Deref for ComponentDefinitionPath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.path
            .as_os_str()
            .to_str()
            .expect("Invalid Unicode in ComponentDefinitionPath")
    }
}

impl fmt::Display for ComponentDefinitionPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &**self)
    }
}

impl From<ComponentDefinitionPath> for String {
    fn from(value: ComponentDefinitionPath) -> Self {
        value
            .path
            .into_os_string()
            .into_string()
            .expect("Invalid Unicode in ComponentDefinitionPath?")
    }
}
