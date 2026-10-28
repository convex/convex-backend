//! Top-level configuration state registered by the user.

use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    path::{
        Component,
        PathBuf,
    },
};

use common::{
    auth::{
        AuthInfo,
        SerializedAuthInfo,
    },
    obj,
    types::{
        IndexDiff,
        ModuleEnvironment,
    },
};
use database::{
    SchemaDiff,
    SerializedSchemaDiff,
};
use serde::{
    Deserialize,
    Serialize,
};
use sync_types::{
    module_path::ACTIONS_DIR,
    CanonicalizedModulePath,
    ModulePath,
};
use value::{
    codegen_convex_serialization,
    remove_string,
    sha256::Sha256Digest,
    ConvexArray,
    ConvexObject,
    ConvexValue,
};

use crate::{
    auth::types::{
        AuthDiff,
        AuthInfoPersisted,
    },
    cron_jobs::types::CronIdentifier,
    modules::module_versions::{
        ModuleSource,
        SourceMap,
    },
};

/// User-specified module definition. See [`ModuleMetadata`] and associated
/// structs for the corresponding module metadata used internally by the system.
#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
pub struct ModuleConfig {
    /// Relative path to the module.
    pub path: ModulePath,
    /// Module source.
    pub source: ModuleSource,
    /// The module's source map (if available).
    pub source_map: Option<SourceMap>,
    /// The environment is bundled to run in.
    pub environment: ModuleEnvironment,
}

/// A module definition that includes a hash instead of the source and
/// source_map, to be used when the module already exists on the server.
#[derive(Debug)]
pub struct ModuleHashConfig {
    /// Relative path to the module.
    pub path: ModulePath,
    /// The environment is bundled to run in.
    pub environment: ModuleEnvironment,
    // This is a hash of source + source_map.
    pub sha256: Sha256Digest,
}

/// This is not safe to use since convex 0.12.0, where we allow defining actions
/// outside of the /actions subfolder. This method should only be used for old
/// cli clients and source packages where environment is not set.
pub fn deprecated_extract_environment_from_path(p: String) -> anyhow::Result<ModuleEnvironment> {
    let path = PathBuf::from(&p);
    let components = path
        .components()
        .map(|component| match component {
            Component::Normal(c) => c
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Path {p} contains an invalid Unicode character")),
            Component::RootDir => {
                anyhow::bail!("Module paths must be relative ({p} is absolute)")
            },
            c => anyhow::bail!("Invalid path component {c:?} in {p}"),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    // This is the old way to indicate a module should execute in Node.js.
    let environment = if matches!(&components[..], &[ACTIONS_DIR, ..]) {
        ModuleEnvironment::Node
    } else {
        ModuleEnvironment::Isolate
    };
    Ok(environment)
}

pub const AUTH_CONFIG_FILE_NAME: &str = "auth.config.js";

/// Representation of Convex config metadata deployed by the client. This
/// metadata isn't written to a table but is instead normalized and represented
/// by state in the other metadata tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigMetadata {
    /// The local directory on the client containing modules.
    pub functions: String,
    /// Authentication info. Empty if this instance has not set up
    /// authentication.
    pub auth_info: Vec<AuthInfo>,
}

impl ConfigMetadata {
    /// Create new empty config metadata for a new instance.
    pub fn new() -> Self {
        Self {
            functions: "convex/".to_string(),
            auth_info: vec![],
        }
    }

    pub fn from_file(file: ConfigFile, auth_info: Vec<AuthInfo>) -> Self {
        Self {
            functions: file.functions,
            auth_info,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
    pub functions: String,
    // Deprecated, moved to AuthConfig.providers
    pub auth_info: Option<Vec<SerializedAuthInfo>>,
}

impl TryFrom<ConfigMetadata> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(m: ConfigMetadata) -> anyhow::Result<Self> {
        let mut config = BTreeMap::new();
        config.insert("functions".parse()?, ConvexValue::try_from(m.functions)?);

        // The auth config was moved from `authInfo` to `auth.config.js` in modules,
        // do not include it in the config response if it is empty.
        if !m.auth_info.is_empty() {
            let auth_info = m
                .auth_info
                .into_iter()
                .map(|v| Ok(ConvexObject::try_from(AuthInfoPersisted(v))?.into()))
                .collect::<anyhow::Result<Vec<ConvexValue>>>()?
                .try_into()?;
            config.insert("authInfo".parse()?, auth_info);
        }
        config.try_into()
    }
}

impl TryFrom<ConfigMetadata> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(value: ConfigMetadata) -> Result<Self, Self::Error> {
        Ok(ConvexObject::try_from(value)?.into())
    }
}

impl TryFrom<ConvexObject> for ConfigMetadata {
    type Error = anyhow::Error;

    fn try_from(o: ConvexObject) -> Result<Self, Self::Error> {
        let mut fields: BTreeMap<_, _> = o.into();
        let functions = match fields.remove("functions") {
            Some(ConvexValue::String(s)) => s.into(),
            _ => anyhow::bail!(
                "Missing or invalid functions field for ConfigMetadata: {:?}",
                fields,
            ),
        };
        let auth_info = match fields.remove("authInfo") {
            Some(v) => ConvexArray::try_from(v)?
                .into_iter()
                .map(|v| {
                    let parsed: AuthInfoPersisted = ConvexObject::try_from(v)?.try_into()?;
                    Ok(parsed.0)
                })
                .collect::<anyhow::Result<Vec<AuthInfo>>>()?,
            _ => vec![],
        };
        Ok(Self {
            functions,
            auth_info,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ConfigDiff {
    pub auth_diff: AuthDiff,
    pub udf_server_version_diff: Option<UdfServerVersionDiff>,
    pub module_diff: ModuleDiff,
    pub cron_diff: CronDiff,
    pub index_diff: ConfigIndexDiff,
    pub schema_diff: Option<SchemaDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedConfigDiff {
    pub auth: AuthDiff,
    // NOTE: not camel-case
    pub server_version: Option<UdfServerVersionDiff>,
    pub modules: ModuleDiff,
    pub crons: Option<CronDiff>,
    pub indexes: Option<ConfigIndexDiff>,
    pub schema: Option<SerializedSchemaDiff>,
}

codegen_convex_serialization!(ConfigDiff, SerializedConfigDiff);

impl TryFrom<ConfigDiff> for SerializedConfigDiff {
    type Error = anyhow::Error;

    fn try_from(value: ConfigDiff) -> Result<Self, Self::Error> {
        Ok(Self {
            auth: value.auth_diff,
            server_version: value.udf_server_version_diff,
            modules: value.module_diff,
            crons: Some(value.cron_diff),
            indexes: Some(value.index_diff),
            schema: value.schema_diff.map(TryFrom::try_from).transpose()?,
        })
    }
}

impl TryFrom<SerializedConfigDiff> for ConfigDiff {
    type Error = anyhow::Error;

    fn try_from(obj: SerializedConfigDiff) -> anyhow::Result<Self> {
        Ok(Self {
            auth_diff: obj.auth,
            udf_server_version_diff: obj.server_version,
            module_diff: obj.modules,
            cron_diff: obj.crons.unwrap_or_default(),
            index_diff: obj.indexes.unwrap_or_default(),
            schema_diff: obj.schema.map(TryFrom::try_from).transpose()?,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigIndexDiff {
    pub added: Vec<String>,
    pub dropped: Vec<String>,
}

impl From<IndexDiff> for ConfigIndexDiff {
    fn from(value: IndexDiff) -> Self {
        Self {
            added: value
                .added
                .into_iter()
                .map(|index_metadata| index_metadata.name.to_string())
                .collect(),
            dropped: value
                .dropped
                .into_iter()
                .map(|index_metadata| index_metadata.name.to_string())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdfServerVersionDiff {
    pub previous_version: String,
    pub next_version: String,
}
impl TryFrom<UdfServerVersionDiff> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(value: UdfServerVersionDiff) -> Result<Self, Self::Error> {
        obj!("previous_version" => value.previous_version, "next_version" => value.next_version)
    }
}

impl TryFrom<ConvexObject> for UdfServerVersionDiff {
    type Error = anyhow::Error;

    fn try_from(obj: ConvexObject) -> anyhow::Result<Self> {
        let mut fields = BTreeMap::from(obj);
        Ok(Self {
            previous_version: remove_string(&mut fields, "previous_version")?,
            next_version: remove_string(&mut fields, "next_version")?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl ModuleDiff {
    pub fn new(
        added_module_paths: BTreeSet<CanonicalizedModulePath>,
        removed_module_paths: BTreeSet<CanonicalizedModulePath>,
    ) -> anyhow::Result<Self> {
        let mut added_functions = Vec::with_capacity(added_module_paths.len());
        for m in added_module_paths {
            if m.is_deps() || m.is_system() {
                continue;
            }
            added_functions.push(m.as_str().to_string());
        }
        let mut removed_functions = Vec::with_capacity(removed_module_paths.len());
        for m in removed_module_paths {
            if m.is_deps() || m.is_system() {
                continue;
            }
            removed_functions.push(m.as_str().to_string());
        }
        Ok(Self {
            added: added_functions,
            removed: removed_functions,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CronDiff {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub deleted: Vec<String>,
}

impl CronDiff {
    pub fn new(
        added_crons: Vec<&CronIdentifier>,
        updated_crons: Vec<&CronIdentifier>,
        deleted_crons: Vec<&CronIdentifier>,
    ) -> Self {
        Self {
            added: added_crons.into_iter().map(|c| c.to_string()).collect(),
            updated: updated_crons.into_iter().map(|c| c.to_string()).collect(),
            deleted: deleted_crons.into_iter().map(|c| c.to_string()).collect(),
        }
    }
}
