use std::collections::{
    BTreeMap,
    BTreeSet,
};

use common::{
    bootstrap_model::components::definition::{
        ComponentDefinitionMetadata,
        SerializedComponentDefinitionMetadata,
    },
    components::ComponentDefinitionPath,
    schemas::{
        json::DatabaseSchemaJson,
        DatabaseSchema,
    },
    types::NodeDependency,
};
use semver::Version;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as JsonValue;
use sync_types::CanonicalizedModulePath;
use value::ConvexObject;

use crate::{
    config::types::{
        ConfigMetadata,
        ModuleConfig,
        ModuleHashConfig,
    },
    modules::module_versions::{
        AnalyzedModule,
        SerializedAnalyzedModule,
    },
    source_packages::types::NodeVersion,
    udf_config::types::UdfConfig,
};

#[derive(Debug)]
pub struct ProjectConfig {
    pub config: ConfigMetadata,

    pub app_definition: AppDefinitionConfig,
    pub component_definitions: Vec<ComponentDefinitionConfig>,

    // TODO(CX-6483): Add support for components to declare their own external dependencies.
    pub node_dependencies: Vec<NodeDependency>,

    // Version of Node.js to use in the node executor.
    pub node_version: Option<NodeVersion>,

    pub dry_run: bool,

    /// When true, relax typechecks that don't affect the codegen output. The
    /// CLI sets this for standalone component codegen (`convex codegen
    /// --component-dir ...`), where it wraps the target component in a
    /// synthetic root app that can't provide bindings for the child's required
    /// env vars.
    pub for_codegen: bool,
}

#[derive(Debug)]
pub struct AppDefinitionConfig {
    // Bundled `convex.config.js` if present, with dependencies on other components marked external
    // and unresolved. Not available at runtime.
    pub definition: Option<ModuleConfig>,
    // Dependencies on other components discovered at bundling time.
    pub dependencies: BTreeSet<ComponentDefinitionPath>,

    // Optional schema.js. Not available at runtime.
    pub schema: Option<ModuleConfig>,

    // Runtime modules that have changed since the last push.
    // Includes all modules directly available at runtime:
    // - Regular function entry points
    // - http.js
    // - crons.js
    // - Bundler dependency chunks within _deps.
    // Also includes auth.config.js which is empty at runtime.
    pub changed_runtime_modules: Vec<ModuleConfig>,
    // Runtime modules without any changes
    // Files that are neither in `changed_runtime_modules` nor in `unchanged_runtime_module_hashes`
    // are deleted by the push.
    pub unchanged_runtime_module_hashes: Vec<ModuleHashConfig>,

    pub udf_server_version: Version,
}

impl AppDefinitionConfig {
    /// Returns an iterator over all app modules: runtime functions + schema +
    /// definition. Runtime functions need to be passed in since we may need
    /// to retrieve the ModuleConfigs for the unchanged module hashes.
    pub fn all_modules<'a>(
        &'a self,
        app_functions: &'a [ModuleConfig],
    ) -> impl Iterator<Item = &'a ModuleConfig> {
        app_functions
            .iter()
            .chain(self.schema.iter())
            .chain(self.definition.iter())
    }
}

#[derive(Debug)]
pub struct ComponentDefinitionConfig {
    // Relative path from the root `convex/` directory to the component's directory.
    pub definition_path: ComponentDefinitionPath,

    // Bundled component definition at `convex.config.js` with dependencies on other components
    // unresolved.
    pub definition: ModuleConfig,
    // Dependencies on other components discovered at bundling time.
    pub dependencies: BTreeSet<ComponentDefinitionPath>,

    // Optional schema.js. Not available at runtime.
    pub schema: Option<ModuleConfig>,

    // Includes all modules directly available at runtime:
    // - Regular function entry points
    // - http.js
    // - crons.js
    // - Bundler dependency chunks within _deps.
    pub functions: Vec<ModuleConfig>,

    pub udf_server_version: Version,
}

impl ComponentDefinitionConfig {
    pub fn modules(&self) -> impl Iterator<Item = &ModuleConfig> {
        std::iter::once(&self.definition)
            .chain(self.schema.iter())
            .chain(&self.functions)
    }
}

#[derive(Clone, Debug)]
pub struct EvaluatedComponentDefinition {
    pub definition: ComponentDefinitionMetadata,
    pub schema: Option<DatabaseSchema>,
    pub functions: BTreeMap<CanonicalizedModulePath, AnalyzedModule>,
    pub udf_config: UdfConfig,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedEvaluatedComponentDefinition {
    definition: SerializedComponentDefinitionMetadata,
    schema: Option<DatabaseSchemaJson>,
    functions: BTreeMap<String, SerializedAnalyzedModule>,
    udf_config: JsonValue,
}

impl TryFrom<EvaluatedComponentDefinition> for SerializedEvaluatedComponentDefinition {
    type Error = anyhow::Error;

    fn try_from(value: EvaluatedComponentDefinition) -> Result<Self, Self::Error> {
        Ok(SerializedEvaluatedComponentDefinition {
            definition: value.definition.try_into()?,
            schema: value.schema.map(|schema| schema.try_into()).transpose()?,
            functions: value
                .functions
                .into_iter()
                .map(|(k, v)| Ok((String::from(k), v.try_into()?)))
                .collect::<anyhow::Result<_>>()?,
            udf_config: ConvexObject::try_from(value.udf_config)?.into(),
        })
    }
}

impl TryFrom<SerializedEvaluatedComponentDefinition> for EvaluatedComponentDefinition {
    type Error = anyhow::Error;

    fn try_from(value: SerializedEvaluatedComponentDefinition) -> Result<Self, Self::Error> {
        Ok(EvaluatedComponentDefinition {
            definition: value.definition.try_into()?,
            schema: value.schema.map(|schema| schema.try_into()).transpose()?,
            functions: value
                .functions
                .into_iter()
                .map(|(k, v)| Ok((k.parse()?, v.try_into()?)))
                .collect::<anyhow::Result<_>>()?,
            udf_config: UdfConfig::try_from(ConvexObject::try_from(value.udf_config)?)?,
        })
    }
}
