use std::collections::{
    BTreeMap,
    BTreeSet,
};

use async_recursion::async_recursion;
use common::{
    bootstrap_model::components::{
        definition::{
            ComponentExport,
            EnvVarValidator,
            HttpMountPath,
        },
        EnvBinding,
    },
    components::{
        CanonicalizedComponentFunctionPath,
        ComponentDefinitionPath,
        ComponentName,
        ComponentPath,
        Reference,
        Resource,
    },
    types::HttpActionRoute,
};
use errors::ErrorMetadata;
use sync_types::path::PathComponent;
use value::identifier::Identifier;

use super::{
    file_based_routing::file_based_exports,
    types::EvaluatedComponentDefinition,
};
use crate::modules::HTTP_MODULE_PATH;

#[derive(Clone, Debug)]
pub struct CheckedComponent {
    pub definition_path: ComponentDefinitionPath,
    pub component_path: ComponentPath,

    /// The HTTP path prefix under which this component's HTTP routes are
    /// served. For the root component, this comes from `http_prefix` on
    /// its definition. For child components, this is the mount path declared
    /// in the parent's `http_mounts`. `None` if the component has no HTTP
    /// prefix.
    pub http_prefix: Option<String>,

    pub env: BTreeMap<Identifier, EnvBinding>,
    pub child_components: BTreeMap<ComponentName, CheckedComponent>,
    pub http_routes: CheckedHttpRoutes,
    pub exports: BTreeMap<PathComponent, ResourceTree>,
}

#[derive(Clone, Debug)]
pub enum ResourceTree {
    Branch(BTreeMap<PathComponent, ResourceTree>),
    Leaf(Resource),
}

pub struct TypecheckContext<'a> {
    evaluated_definitions: &'a BTreeMap<ComponentDefinitionPath, EvaluatedComponentDefinition>,
    validate_env_vars: bool,
}

impl<'a> TypecheckContext<'a> {
    pub fn new(
        definitions: &'a BTreeMap<ComponentDefinitionPath, EvaluatedComponentDefinition>,
    ) -> Self {
        Self {
            evaluated_definitions: definitions,
            validate_env_vars: true,
        }
    }

    pub fn new_for_codegen(
        definitions: &'a BTreeMap<ComponentDefinitionPath, EvaluatedComponentDefinition>,
    ) -> Self {
        Self {
            evaluated_definitions: definitions,
            // When doing isolate codegen for a component, a shell root component is used which
            // doesn't have env vars defined.
            validate_env_vars: false,
        }
    }

    #[fastrace::trace]
    pub async fn instantiate_root(&self) -> anyhow::Result<CheckedComponent> {
        let definition_path = ComponentDefinitionPath::root();
        let component_path = ComponentPath::root();
        let env = BTreeMap::new();
        let evaluated = self
            .evaluated_definitions
            .get(&definition_path)
            .ok_or_else(|| {
                ErrorMetadata::bad_request("TypecheckError", "Root component definition not found")
            })?;
        let http_prefix = evaluated
            .definition
            .http_prefix
            .as_ref()
            .map(|p| p.to_string());
        self.instantiate(definition_path, component_path, env, http_prefix)
            .await
    }

    #[async_recursion]
    pub async fn instantiate(
        &self,
        definition_path: ComponentDefinitionPath,
        component_path: ComponentPath,
        env: BTreeMap<Identifier, EnvBinding>,
        http_prefix: Option<String>,
    ) -> anyhow::Result<CheckedComponent> {
        let evaluated = self
            .evaluated_definitions
            .get(&definition_path)
            .ok_or_else(|| {
                ErrorMetadata::bad_request(
                    "TypecheckError",
                    "Component definition not found: {definition_path:?}",
                )
            })?;

        let mut builder = CheckedComponentBuilder::check_env(
            &definition_path,
            &component_path,
            evaluated,
            env,
            self.validate_env_vars,
        )?;

        // Pre-compute the HTTP mount path for each child component by name.
        // Child mount paths are absolute from the deployment root.
        let http_mount_by_child: BTreeMap<ComponentName, &HttpMountPath> = evaluated
            .definition
            .http_mounts
            .iter()
            .filter_map(|(mount_path, reference)| {
                if let Reference::ChildComponent {
                    component: name,
                    attributes,
                } = reference
                    && attributes.is_empty()
                {
                    Some((name.clone(), mount_path))
                } else {
                    None
                }
            })
            .collect();

        // Instantiate our children in order, since we'd like to support one
        // instantiation depending on another (e.g. passing a function reference
        // from one component as an argument to another).
        for instantiation in &evaluated.definition.child_components {
            let resolved_env: BTreeMap<Identifier, EnvBinding> = instantiation.env.clone();

            let child_component_path = component_path.push(instantiation.name.clone());

            // Validate the child's env bindings against declarations.
            let child_evaluated = self
                .evaluated_definitions
                .get(&instantiation.path)
                .ok_or_else(|| {
                    ErrorMetadata::bad_request(
                        "TypecheckError",
                        format!("Component definition not found: {:?}", instantiation.path),
                    )
                })?;
            validate_component_env(
                &child_component_path,
                &child_evaluated.definition.env_vars,
                &resolved_env,
                Some(&evaluated.definition.env_vars),
            )?;

            // Chain-resolve EnvVar bindings through the component hierarchy.
            let resolved_env: BTreeMap<Identifier, EnvBinding> = resolved_env
                .into_iter()
                .filter_map(|(child_key, binding)| match &binding {
                    EnvBinding::Value(_) => Some((child_key, binding)),
                    EnvBinding::EnvVar(parent_name) => {
                        if component_path.is_root() {
                            return Some((child_key, binding));
                        }
                        let parent_id: Identifier = parent_name
                            .as_ref()
                            .parse()
                            .expect("validate_component_env verified parseability");
                        builder
                            .env
                            .get(&parent_id)
                            .cloned()
                            .map(|resolved| (child_key, resolved))
                    },
                })
                .collect();

            // The child's http_prefix is its absolute mount path (if mounted via HTTP).
            let child_http_prefix =
                http_mount_by_child
                    .get(&instantiation.name)
                    .map(|mount_path| {
                        let mount_str: &str = mount_path;
                        // For non-root components, the http_prefix is the
                        // absolute URL path where the parent is mounted, so we
                        // compose it with the child's mount path. For the root,
                        // http_prefix is the app's httpPrefix (only affects the
                        // root's own routes), and mount paths are already
                        // absolute.
                        if !component_path.is_root()
                            && let Some(parent_prefix) = &http_prefix
                        {
                            return format!("{}{}", parent_prefix.trim_end_matches('/'), mount_str);
                        }
                        mount_str.to_string()
                    });
            let child_component = self
                .instantiate(
                    instantiation.path.clone(),
                    child_component_path,
                    resolved_env,
                    child_http_prefix,
                )
                .await?;
            builder.insert_child_component(instantiation.name.clone(), child_component)?;
        }

        // Check that our HTTP mounts are valid and nonoverlapping.
        for (mount_path, reference) in &evaluated.definition.http_mounts {
            builder.insert_http_mount(self.evaluated_definitions, mount_path, reference)?;
        }

        // Finally, resolve our exports and build the component.
        let component = builder.build_exports(http_prefix)?;

        Ok(component)
    }
}

pub fn validate_component_env(
    component_path: &ComponentPath,
    env_validators: &BTreeMap<Identifier, EnvVarValidator>,
    env: &BTreeMap<Identifier, EnvBinding>,
    parent_env_validators: Option<&BTreeMap<Identifier, EnvVarValidator>>,
) -> anyhow::Result<()> {
    for (env_name, binding) in env {
        let child_validator = env_validators.get(env_name).ok_or_else(|| {
            ErrorMetadata::bad_request(
                "TypecheckError",
                format!("Component {component_path:?} has no env var named {env_name:?}"),
            )
        })?;
        match binding {
            EnvBinding::Value(value) => {
                child_validator.check_provided_value(value).map_err(|_| {
                    ErrorMetadata::bad_request(
                        "TypecheckError",
                        format!(
                            "Component {component_path:?} has an invalid value for env var \
                             {env_name:?}: expected {}",
                            child_validator.validator
                        ),
                    )
                })?;
            },
            EnvBinding::EnvVar(parent_name) => {
                let Some(parent_env_validators) = parent_env_validators else {
                    continue;
                };
                let parent_id: Identifier = parent_name.as_ref().parse().map_err(|e| {
                    ErrorMetadata::bad_request(
                        "TypecheckError",
                        format!(
                            "Component {component_path:?} env var {env_name:?} references invalid \
                             parent env var name {parent_name}: {e}"
                        ),
                    )
                })?;
                let parent_validator = parent_env_validators.get(&parent_id).ok_or_else(|| {
                    ErrorMetadata::bad_request(
                        "TypecheckError",
                        format!(
                            "Component {component_path:?} env var {env_name:?} references \
                             undeclared parent env var {parent_name}"
                        ),
                    )
                })?;
                if parent_validator.optional && !child_validator.optional {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "TypecheckError",
                        format!(
                            "Component {component_path:?} env var {env_name:?} is required, but \
                             parent env var {parent_name} is optional"
                        ),
                    ));
                }
                if !parent_validator
                    .validator
                    .is_subset(&child_validator.validator)
                {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "TypecheckError",
                        format!(
                            "Component {component_path:?} env var {env_name:?} validator is not \
                             satisfied by parent env var {parent_name} validator"
                        ),
                    ));
                }
            },
        }
    }
    Ok(())
}

struct CheckedComponentBuilder<'a> {
    definition_path: &'a ComponentDefinitionPath,
    component_path: &'a ComponentPath,
    evaluated: &'a EvaluatedComponentDefinition,

    // Phase 1: Env bindings are checked immediately at construction time.
    env: BTreeMap<Identifier, EnvBinding>,

    // Phase 2: The layer above adds in child components one at a time, and instantiating a child
    // component may depend on previous child components.
    child_components: BTreeMap<ComponentName, CheckedComponent>,

    // Phase 3: The layer above mounts child component HTTP routes.
    http_routes: CheckedHttpRoutes,
    //
    // Phase 4: The layer above finalizes via `build`, passing in exports, which may depend on any
    // child component.
}

impl<'a> CheckedComponentBuilder<'a> {
    pub fn check_env(
        definition_path: &'a ComponentDefinitionPath,
        component_path: &'a ComponentPath,
        evaluated: &'a EvaluatedComponentDefinition,
        env: BTreeMap<Identifier, EnvBinding>,
        validate_env_vars: bool,
    ) -> anyhow::Result<Self> {
        // Check env bindings against declared env vars.
        let env_validators = &evaluated.definition.env_vars;
        for env_name in env.keys() {
            if !env_validators.contains_key(env_name) {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "TypecheckError",
                    format!("Component {component_path:?} has no env var named {env_name:?}"),
                ));
            }
        }
        if !component_path.is_root() && validate_env_vars {
            for (env_name, validator) in env_validators {
                if !validator.optional && !env.contains_key(env_name) {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "TypecheckError",
                        format!(
                            "Component {component_path:?} is missing required env var {env_name:?}"
                        ),
                    ));
                }
            }
        }
        Ok(Self {
            definition_path,
            component_path,
            evaluated,

            env,
            child_components: BTreeMap::new(),
            http_routes: CheckedHttpRoutes::new(evaluated),
        })
    }

    fn insert_child_component(
        &mut self,
        name: ComponentName,
        component: CheckedComponent,
    ) -> anyhow::Result<()> {
        if self.child_components.contains_key(&name) {
            anyhow::bail!(ErrorMetadata::bad_request(
                "TypecheckError",
                format!(
                    "Component {:?} has multiple child components with the same name: {name:?}",
                    self.definition_path
                ),
            ));
        }
        self.child_components.insert(name, component);
        Ok(())
    }

    fn insert_http_mount(
        &mut self,
        evaluated_definitions: &BTreeMap<ComponentDefinitionPath, EvaluatedComponentDefinition>,
        mount_path: &HttpMountPath,
        reference: &Reference,
    ) -> anyhow::Result<()> {
        let Reference::ChildComponent {
            component,
            attributes,
        } = reference
        else {
            anyhow::bail!(ErrorMetadata::bad_request(
                "TypecheckError",
                "Non-root child component references for HTTP mounts currently unsupported",
            ));
        };
        if !attributes.is_empty() {
            anyhow::bail!(ErrorMetadata::bad_request(
                "TypecheckError",
                "Child component references with attributes currently unsupported",
            ));
        }

        let Some(child_component) = self.child_components.get(component) else {
            anyhow::bail!(ErrorMetadata::bad_request(
                "TypecheckError",
                format!(
                    "HTTP mount {mount_path:?} is invalid: Child component {component:?} not \
                     found."
                ),
            ));
        };
        if !evaluated_definitions.contains_key(&child_component.definition_path) {
            anyhow::bail!(ErrorMetadata::bad_request(
                "TypecheckError",
                format!(
                    "Component definition not found: {:?}",
                    child_component.definition_path
                ),
            ));
        };
        if child_component.http_routes.is_empty() {
            anyhow::bail!(ErrorMetadata::bad_request(
                "TypecheckError",
                format!(
                    "HTTP mount {mount_path:?} is invalid: Child component {component:?} doesn't \
                     have any HTTP routes."
                ),
            ));
        }
        self.http_routes.mount(mount_path.clone())?;
        Ok(())
    }

    fn build_exports(self, http_prefix: Option<String>) -> anyhow::Result<CheckedComponent> {
        let exports = file_based_exports(&self.evaluated.functions)?;
        let exports = self.resolve_exports(&exports)?;
        Ok(CheckedComponent {
            definition_path: self.definition_path.clone(),
            component_path: self.component_path.clone(),
            http_prefix,
            env: self.env,
            http_routes: self.http_routes,
            child_components: self.child_components,
            exports,
        })
    }

    fn resolve_exports(
        &self,
        exports: &BTreeMap<PathComponent, ComponentExport>,
    ) -> anyhow::Result<BTreeMap<PathComponent, ResourceTree>> {
        let mut result = BTreeMap::new();
        for (name, export) in exports {
            let node = match export {
                ComponentExport::Branch(exports) => {
                    ResourceTree::Branch(self.resolve_exports(exports)?)
                },
                ComponentExport::Leaf(reference) => self.resolve(reference)?,
            };
            result.insert(name.clone(), node);
        }
        Ok(result)
    }

    fn resolve(&self, reference: &Reference) -> anyhow::Result<ResourceTree> {
        let unresolved_err = || {
            ErrorMetadata::bad_request(
                "TypecheckError",
                format!(
                    "Component {:?} has an unresolved export: {reference:?}",
                    self.definition_path
                ),
            )
        };
        let result = match reference {
            Reference::Function(path) => {
                let canonicalized = path.clone();
                let module = self
                    .evaluated
                    .functions
                    .get(canonicalized.module())
                    .ok_or_else(unresolved_err)?;
                let _function = module
                    .functions
                    .iter()
                    .find(|f| &f.name == canonicalized.function_name())
                    .ok_or_else(unresolved_err)?;
                let path = CanonicalizedComponentFunctionPath {
                    component: self.component_path.clone(),
                    udf_path: path.clone(),
                };
                ResourceTree::Leaf(Resource::Function(path))
            },
            Reference::ChildComponent {
                component,
                attributes,
            } => {
                let child_component = self
                    .child_components
                    .get(component)
                    .ok_or_else(unresolved_err)?;
                child_component
                    .resolve_export(attributes)?
                    .ok_or_else(unresolved_err)?
            },
            Reference::CurrentSystemUdfInComponent { .. } => {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "TypecheckError",
                    "CurrentSystemUdfInComponent reference currently unsupported",
                ));
            },
        };
        Ok(result)
    }
}

impl CheckedComponent {
    pub fn resolve_export(
        &self,
        attributes: &[PathComponent],
    ) -> anyhow::Result<Option<ResourceTree>> {
        let mut current = &self.exports;
        let mut attribute_iter = attributes.iter();
        while let Some(attribute) = attribute_iter.next() {
            let Some(export) = current.get(attribute) else {
                return Ok(None);
            };
            match export {
                ResourceTree::Branch(next) => {
                    current = next;
                    continue;
                },
                ResourceTree::Leaf(resource) => {
                    if !attribute_iter.as_slice().is_empty() {
                        anyhow::bail!("Unexpected component reference");
                    }
                    return Ok(Some(ResourceTree::Leaf(resource.clone())));
                },
            }
        }
        Ok(Some(ResourceTree::Branch(current.clone())))
    }
}

#[derive(Clone, Debug)]
pub struct CheckedHttpRoutes {
    http_module_routes: Option<Vec<HttpActionRoute>>,
    mounts: BTreeSet<HttpMountPath>,
}

impl CheckedHttpRoutes {
    pub fn new(evaluated: &EvaluatedComponentDefinition) -> Self {
        let http_module_routes = evaluated
            .functions
            .get(&HTTP_MODULE_PATH)
            .and_then(|module| module.http_routes.clone())
            .map(|routes| routes.into_iter().map(|r| r.route).collect());
        Self {
            http_module_routes,
            mounts: BTreeSet::new(),
        }
    }

    pub fn mount(&mut self, mount_path: HttpMountPath) -> anyhow::Result<()> {
        // Check that the mount path does not overlap with any prefix route from our
        // `http.js` or previously mounted route.
        if let Some(http_module_routes) = &self.http_module_routes
            && http_module_routes
                .iter()
                .any(|route| route.overlaps_with_mount(&mount_path))
        {
            anyhow::bail!(ErrorMetadata::bad_request(
                "TypecheckError",
                format!("HTTP mount {mount_path:?} is invalid: Overlap with existing prefix route"),
            ));
        }
        if self.mounts.contains(&mount_path) {
            anyhow::bail!(ErrorMetadata::bad_request(
                "TypecheckError",
                format!(
                    "HTTP mount {mount_path:?} is invalid: Overlap with previously mounted route"
                ),
            ));
        }
        self.mounts.insert(mount_path);
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.http_module_routes
            .as_ref()
            .map(|r| r.is_empty())
            .unwrap_or(true)
            && self.mounts.is_empty()
    }

}

mod json {
    use std::collections::BTreeMap;

    use common::{
        bootstrap_model::components::SerializedEnvBinding,
        components::SerializedResource,
        types::SerializedHttpActionRoute,
    };
    use serde::{
        Deserialize,
        Serialize,
    };

    use super::{
        CheckedComponent,
        CheckedHttpRoutes,
        ResourceTree,
    };

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SerializedCheckedComponent {
        definition_path: String,
        component_path: String,
        http_prefix: Option<String>,

        /// Always empty. CLIs that predate the removal of component arguments
        /// require it in the `start_push` response.
        #[serde(default)]
        args: BTreeMap<String, SerializedResource>,
        #[serde(default)]
        env: BTreeMap<String, SerializedEnvBinding>,
        child_components: BTreeMap<String, SerializedCheckedComponent>,
        http_routes: SerializedCheckedHttpRoutes,
        exports: BTreeMap<String, SerializedResourceTree>,
    }

    impl TryFrom<CheckedComponent> for SerializedCheckedComponent {
        type Error = anyhow::Error;

        fn try_from(value: CheckedComponent) -> Result<Self, Self::Error> {
            Ok(Self {
                definition_path: String::from(value.definition_path),
                component_path: String::from(value.component_path),
                http_prefix: value.http_prefix,
                args: BTreeMap::new(),
                env: value
                    .env
                    .into_iter()
                    .map(|(k, v)| Ok((String::from(k), v.try_into()?)))
                    .collect::<anyhow::Result<_>>()?,
                child_components: value
                    .child_components
                    .into_iter()
                    .map(|(k, v)| Ok((String::from(k), v.try_into()?)))
                    .collect::<anyhow::Result<_>>()?,
                http_routes: value.http_routes.try_into()?,
                exports: value
                    .exports
                    .into_iter()
                    .map(|(k, v)| Ok((String::from(k), v.try_into()?)))
                    .collect::<anyhow::Result<_>>()?,
            })
        }
    }

    impl TryFrom<SerializedCheckedComponent> for CheckedComponent {
        type Error = anyhow::Error;

        fn try_from(value: SerializedCheckedComponent) -> Result<Self, Self::Error> {
            Ok(Self {
                definition_path: value.definition_path.parse()?,
                component_path: value.component_path.parse()?,
                http_prefix: value.http_prefix,
                env: value
                    .env
                    .into_iter()
                    .map(|(k, v)| Ok((k.parse()?, v.try_into()?)))
                    .collect::<anyhow::Result<_>>()?,
                child_components: value
                    .child_components
                    .into_iter()
                    .map(|(k, v)| Ok((k.parse()?, v.try_into()?)))
                    .collect::<anyhow::Result<_>>()?,
                http_routes: value.http_routes.try_into()?,
                exports: value
                    .exports
                    .into_iter()
                    .map(|(k, v)| Ok((k.parse()?, v.try_into()?)))
                    .collect::<anyhow::Result<_>>()?,
            })
        }
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SerializedCheckedHttpRoutes {
        http_module_routes: Option<Vec<SerializedHttpActionRoute>>,
        mounts: Vec<String>,
    }

    impl TryFrom<CheckedHttpRoutes> for SerializedCheckedHttpRoutes {
        type Error = anyhow::Error;

        fn try_from(value: CheckedHttpRoutes) -> Result<Self, Self::Error> {
            let http_module_routes = value
                .http_module_routes
                .map(|routes| routes.into_iter().map(|s| s.try_into()).collect())
                .transpose()?;
            let mounts = value.mounts.into_iter().map(String::from).collect();
            Ok(Self {
                http_module_routes,
                mounts,
            })
        }
    }

    impl TryFrom<SerializedCheckedHttpRoutes> for CheckedHttpRoutes {
        type Error = anyhow::Error;

        fn try_from(value: SerializedCheckedHttpRoutes) -> Result<Self, Self::Error> {
            Ok(Self {
                http_module_routes: value
                    .http_module_routes
                    .map(|routes| {
                        routes
                            .into_iter()
                            .map(|s| s.try_into())
                            .collect::<anyhow::Result<_>>()
                    })
                    .transpose()?,
                mounts: value
                    .mounts
                    .into_iter()
                    .map(|s| s.parse())
                    .collect::<anyhow::Result<_>>()?,
            })
        }
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", tag = "type")]
    pub enum SerializedResourceTree {
        Branch {
            children: BTreeMap<String, SerializedResourceTree>,
        },
        Leaf {
            resource: SerializedResource,
        },
    }

    impl TryFrom<ResourceTree> for SerializedResourceTree {
        type Error = anyhow::Error;

        fn try_from(value: ResourceTree) -> Result<Self, Self::Error> {
            Ok(match value {
                ResourceTree::Branch(branch) => Self::Branch {
                    children: branch
                        .into_iter()
                        .map(|(k, v)| Ok((String::from(k), v.try_into()?)))
                        .collect::<anyhow::Result<_>>()?,
                },
                ResourceTree::Leaf(leaf) => Self::Leaf {
                    resource: leaf.try_into()?,
                },
            })
        }
    }

    impl TryFrom<SerializedResourceTree> for ResourceTree {
        type Error = anyhow::Error;

        fn try_from(value: SerializedResourceTree) -> Result<Self, Self::Error> {
            Ok(match value {
                SerializedResourceTree::Branch { children } => Self::Branch(
                    children
                        .into_iter()
                        .map(|(k, v)| Ok((k.parse()?, v.try_into()?)))
                        .collect::<anyhow::Result<_>>()?,
                ),
                SerializedResourceTree::Leaf { resource } => Self::Leaf(resource.try_into()?),
            })
        }
    }
}
pub use self::json::{
    SerializedCheckedComponent,
    SerializedResourceTree,
};
