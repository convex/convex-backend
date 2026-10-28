use std::{
    fmt,
    path::{
        Component,
        Path,
        PathBuf,
    },
    str::FromStr,
};

use crate::path::{
    check_valid_path_component,
    InvalidPathComponentError,
    PathComponent,
};

pub const SYSTEM_UDF_DIR: &str = "_system";
pub const DEPS_DIR: &str = "_deps";
pub const ACTIONS_DIR: &str = "actions";
pub const HTTP_PATH: &str = "http.js";
pub const CRON_PATH: &str = "crons.js";

/// User-specified path to a loaded module.
#[derive(Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ModulePath {
    path: PathBuf,
    is_system: bool,
    is_deps: bool,
    is_http: bool,
    is_cron: bool,
}

impl ModulePath {
    /// NOTE: This constructor should only be used when converting from protos.
    /// Otherwise, prefer parsing the path from a `str` so that it gets
    /// validated.
    pub fn new(
        path: PathBuf,
        is_system: bool,
        is_deps: bool,
        is_http: bool,
        is_cron: bool,
    ) -> Self {
        Self {
            path,
            is_system,
            is_deps,
            is_http,
            is_cron,
        }
    }

    /// View the module path as a `str`.
    pub fn as_str(&self) -> &str {
        self.path
            .to_str()
            .expect("Non-unicode data in module path?")
    }

    pub fn as_path(&self) -> &Path {
        &self.path
    }

    // TODO: it should not be possible for this to return Err,
    // but `"_.js".strip().components()` will do this
    pub fn components(
        &self,
    ) -> impl Iterator<Item = Result<PathComponent, InvalidModulePathError>> + '_ {
        self.path.components().map(|component| match component {
            Component::Normal(c) => c
                .to_str()
                .ok_or_else(|| InvalidModulePathError::InvalidUnicode {
                    module_path: self.path.to_string_lossy().into_owned(),
                })?
                .parse()
                .map_err(|source| InvalidModulePathError::InvalidComponent {
                    module_path: self.path.to_string_lossy().into_owned(),
                    source,
                }),
            c => Err(InvalidModulePathError::InvalidPathComponent {
                module_path: self.path.to_string_lossy().into_owned(),
                component: format!("{c:?}"),
            }),
        })
    }

    /// Does a module live within the `_system/` directory?
    pub fn is_system(&self) -> bool {
        self.is_system
    }

    /// Does a module live within the `_deps/` directory?
    pub fn is_deps(&self) -> bool {
        self.is_deps
    }

    /// Is this module the (single) HTTP router for the deployment?
    pub fn is_http(&self) -> bool {
        self.is_http
    }

    /// Is this module the (single) crons module for the deployment?
    pub fn is_cron(&self) -> bool {
        self.is_cron
    }

    pub fn canonicalize(self) -> CanonicalizedModulePath {
        let Self {
            path,
            is_system,
            is_deps,
            is_http,
            is_cron,
        } = self;
        let path = canonicalize_path_buf(path);
        CanonicalizedModulePath {
            path,
            is_system,
            is_deps,
            is_http,
            is_cron,
        }
    }

    pub fn assume_canonicalized(self) -> Result<CanonicalizedModulePath, CanonicalModulePathError> {
        let Self {
            path,
            is_system,
            is_deps,
            is_http,
            is_cron,
        } = self;
        let ext = path
            .extension()
            .ok_or_else(|| CanonicalModulePathError::MissingExtension { path: path.clone() })?;
        if ext != "js" {
            return Err(CanonicalModulePathError::InvalidExtension { path });
        }
        Ok(CanonicalizedModulePath {
            path,
            is_system,
            is_deps,
            is_http,
            is_cron,
        })
    }
}

fn canonicalize_path_buf(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension("js");
    }
    path
}

#[derive(Debug, thiserror::Error)]
pub enum CanonicalModulePathError {
    #[error("Path {path:?} doesn't have an extension.")]
    MissingExtension { path: PathBuf },
    #[error("Path {path:?} doesn't have a '.js' extension.")]
    InvalidExtension { path: PathBuf },
}

#[derive(Debug, thiserror::Error)]
pub enum InvalidModulePathError {
    #[error("Invalid module path '{module_path}': Module path doesn't have a filename.")]
    MissingFilename { module_path: String },
    #[error("Invalid module path '{module_path}': Module path has an extension that isn't 'js'.")]
    InvalidExtension { module_path: String },
    // TODO: this case is unreachable; stop using PathBuf
    #[error("Invalid module path '{module_path}': Path contains an invalid Unicode character.")]
    InvalidUnicode { module_path: String },
    #[error("Invalid module path '{module_path}': Module paths must be relative.")]
    AbsolutePath { module_path: String },
    #[error("Invalid module path '{module_path}': Invalid path component {component}.")]
    InvalidPathComponent {
        module_path: String,
        component: String,
    },
    #[error("Invalid module path '{module_path}': Module paths must be nonempty.")]
    EmptyPath { module_path: String },
    #[error("Invalid module path '{module_path}': {source}")]
    InvalidComponent {
        module_path: String,
        #[source]
        source: InvalidPathComponentError,
    },
}

impl FromStr for ModulePath {
    type Err = InvalidModulePathError;

    fn from_str(p: &str) -> Result<Self, Self::Err> {
        let path = PathBuf::from(p);
        if path.file_name().is_none() {
            return Err(InvalidModulePathError::MissingFilename {
                module_path: p.to_owned(),
            });
        }
        if let Some(ext) = path.extension() {
            if ext != "js" {
                return Err(InvalidModulePathError::InvalidExtension {
                    module_path: p.to_owned(),
                });
            }
        }

        let components = path
            .components()
            .map(|component| match component {
                Component::Normal(c) => {
                    c.to_str()
                        .ok_or_else(|| InvalidModulePathError::InvalidUnicode {
                            module_path: p.to_owned(),
                        })
                },
                Component::RootDir => Err(InvalidModulePathError::AbsolutePath {
                    module_path: p.to_owned(),
                }),
                c => Err(InvalidModulePathError::InvalidPathComponent {
                    module_path: p.to_owned(),
                    component: format!("{c:?}"),
                }),
            })
            .collect::<Result<Vec<_>, Self::Err>>()?;
        if components.is_empty() {
            return Err(InvalidModulePathError::EmptyPath {
                module_path: p.to_owned(),
            });
        }

        // Determine the module type based on the first components.
        let is_system = matches!(&components[..], &[SYSTEM_UDF_DIR, ..]);
        let is_deps = matches!(
            &components[..],
            &[DEPS_DIR, ..] | &[ACTIONS_DIR, DEPS_DIR, ..],
        );

        // Check all components (canonicalized). Important to re-check first
        // component because canonicalization can change components.
        let canonicalized = canonicalize_path_buf(path.clone());
        for component in canonicalized.components() {
            let Component::Normal(component) = component else {
                return Err(InvalidModulePathError::InvalidPathComponent {
                    module_path: p.to_owned(),
                    component: format!("{component:?}"),
                });
            };
            let component =
                component
                    .to_str()
                    .ok_or_else(|| InvalidModulePathError::InvalidUnicode {
                        module_path: p.to_owned(),
                    })?;
            check_valid_path_component(component).map_err(|source| {
                InvalidModulePathError::InvalidComponent {
                    module_path: p.to_owned(),
                    source,
                }
            })?;
        }

        let canonicalized_string =
            canonicalized
                .to_str()
                .ok_or_else(|| InvalidModulePathError::InvalidUnicode {
                    module_path: p.to_owned(),
                })?;
        let is_http = canonicalized_string == HTTP_PATH;
        let is_cron = canonicalized_string == CRON_PATH;

        Ok(Self {
            path,
            is_system,
            is_deps,
            is_http,
            is_cron,
        })
    }
}

impl From<ModulePath> for String {
    fn from(p: ModulePath) -> Self {
        p.path
            .into_os_string()
            .into_string()
            .expect("ModulePath had invalid Unicode data?")
    }
}

impl From<CanonicalizedModulePath> for ModulePath {
    fn from(p: CanonicalizedModulePath) -> Self {
        let CanonicalizedModulePath {
            path,
            is_system,
            is_deps,
            is_http,
            is_cron,
        } = p;
        Self {
            path,
            is_system,
            is_deps,
            is_http,
            is_cron,
        }
    }
}

impl fmt::Debug for ModulePath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Module paths are allowed to omit the `.js` extension, but the canonical
/// module path stored in the database must have the `.js` extension. This
/// separate type guarantees that the path contains its extension.
#[derive(Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct CanonicalizedModulePath {
    path: PathBuf,
    is_system: bool,
    is_deps: bool,
    is_http: bool,
    is_cron: bool,
}

impl CanonicalizedModulePath {
    /// NOTE: This constructor should only be used when converting from protos.
    /// Otherwise, prefer the [`FromStr`] implementation since it includes
    /// validation.
    pub fn new(
        path: PathBuf,
        is_system: bool,
        is_deps: bool,
        is_http: bool,
        is_cron: bool,
    ) -> Self {
        Self {
            path,
            is_system,
            is_deps,
            is_http,
            is_cron,
        }
    }

    pub fn as_str(&self) -> &str {
        self.path
            .to_str()
            .expect("Non-unicode data in module path?")
    }

    pub fn is_system(&self) -> bool {
        self.is_system
    }

    pub fn is_deps(&self) -> bool {
        self.is_deps
    }

    pub fn is_http(&self) -> bool {
        self.is_http
    }

    pub fn is_cron(&self) -> bool {
        self.is_cron
    }

    pub fn strip(self) -> ModulePath {
        let Self {
            mut path,
            is_system,
            is_deps,
            is_http,
            is_cron,
        } = self;
        if let Some(ext) = path.extension() {
            if ext == "js" {
                path.set_extension("");
            }
        }
        ModulePath {
            path,
            is_system,
            is_deps,
            is_http,
            is_cron,
        }
    }

}

impl FromStr for CanonicalizedModulePath {
    type Err = InvalidModulePathError;

    fn from_str(p: &str) -> Result<Self, Self::Err> {
        let path = ModulePath::from_str(p)?;
        Ok(path.canonicalize())
    }
}

impl From<CanonicalizedModulePath> for String {
    fn from(p: CanonicalizedModulePath) -> Self {
        p.path.into_os_string().into_string().unwrap()
    }
}

impl fmt::Debug for CanonicalizedModulePath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
