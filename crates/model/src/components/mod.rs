pub mod auth;
pub mod config;
pub mod file_based_routing;
pub mod handles;
pub mod type_checking;
pub mod types;

use std::collections::BTreeMap;

use anyhow::Context;
use async_recursion::async_recursion;
use common::{
    bootstrap_model::components::{
        definition::ComponentExport,
        ComponentMetadata,
        ComponentType,
    },
    components::{
        CanonicalizedComponentFunctionPath,
        CanonicalizedComponentModulePath,
        ComponentDefinitionId,
        ComponentId,
        ComponentName,
        ComponentPath,
        ExportPath,
        Reference,
        ResolvedComponentFunctionPath,
        Resource,
    },
    document::ParsedDocument,
    runtime::Runtime,
};
use database::{
    BootstrapComponentsModel,
    Transaction,
};
use errors::ErrorMetadata;
use file_based_routing::{
    export_to_udf_path,
    file_based_exports,
};
use sync_types::{
    path::PathComponent,
    CanonicalizedUdfPath,
    UdfPath,
};

use crate::modules::{
    module_versions::Visibility,
    ModuleModel,
};

pub struct ComponentsModel<'a, RT: Runtime> {
    pub tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> ComponentsModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    #[async_recursion]
    #[fastrace::trace]
    pub async fn resolve(
        &mut self,
        component_id: ComponentId,
        current_udf_path: Option<UdfPath>,
        reference: &Reference,
    ) -> anyhow::Result<Resource> {
        let result = match reference {
            Reference::ComponentArgument { attributes } => {
                let attribute = match &attributes[..] {
                    [attribute] => attribute,
                    _ => anyhow::bail!("Nested component argument references unsupported"),
                };
                let component_type = BootstrapComponentsModel::new(self.tx)
                    .load_component_type(component_id)
                    .await?;
                let ComponentType::ChildComponent { ref args, .. } = component_type else {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidReference",
                        "Can't use an argument reference in the app"
                    ))
                };
                let resource = args.get(attribute).ok_or_else(|| {
                    ErrorMetadata::bad_request(
                        "InvalidReference",
                        format!("Component argument '{attribute}' not found"),
                    )
                })?;
                resource.clone()
            },
            Reference::Function(udf_path) => {
                let mut m = BootstrapComponentsModel::new(self.tx);
                let component_path = m.must_component_path(component_id)?;

                let path = CanonicalizedComponentFunctionPath {
                    component: component_path,
                    udf_path: udf_path.clone(),
                };
                Resource::Function(path)
            },
            Reference::ChildComponent {
                component: child_component,
                attributes,
            } => {
                let mut m = BootstrapComponentsModel::new(self.tx);
                let internal_id = match component_id {
                    ComponentId::Root => {
                        let root_component =
                            m.root_component()?.context("Missing root component")?;
                        root_component.id().into()
                    },
                    ComponentId::Child(id) => id,
                };
                let parent = (internal_id, child_component.clone());
                let child = m.component_in_parent(Some(parent))?.ok_or_else(|| {
                    ErrorMetadata::bad_request(
                        "InvalidReference",
                        format!("Child component '{}' not found", **child_component),
                    )
                })?;
                let child_id = ComponentId::Child(child.id().into());
                let Some(resource) = self.resolve_export(child_id, attributes).await? else {
                    let export_path = attributes
                        .iter()
                        .map(|a| &**a)
                        .collect::<Vec<_>>()
                        .join(".");
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidReference",
                        format!(
                            "Child component '{}' does not export '{export_path}'",
                            **child_component
                        ),
                    ));
                };
                return Ok(resource);
            },
            Reference::CurrentSystemUdfInComponent {
                component_id: component_by_id,
            } => {
                if !component_id.is_root() {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidReference",
                        "CurrentSystemUdfInComponent only available in root component"
                    ));
                }
                let Some(current_udf_path) = current_udf_path else {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidReference",
                        "CurrentSystemUdfInComponent must be called from a UDF",
                    ));
                };
                if !current_udf_path.is_system() {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidReference",
                        "CurrentSystemUdfInComponent must be called from a system UDF",
                    ));
                }
                let component_path = BootstrapComponentsModel::new(self.tx)
                    .must_component_path(ComponentId::Child(*component_by_id))?;
                Resource::ResolvedSystemUdf(ResolvedComponentFunctionPath {
                    component: ComponentId::Child(*component_by_id),
                    udf_path: current_udf_path.canonicalize(),
                    component_path,
                })
            },
        };
        Ok(result)
    }

    pub async fn load_component_exports(
        &mut self,
        component_id: ComponentId,
    ) -> anyhow::Result<BTreeMap<PathComponent, ComponentExport>> {
        let mut modules = BTreeMap::new();
        let metadata = ModuleModel::new(self.tx)
            .get_all_metadata(component_id)
            .await?;
        for module in &metadata {
            let Some(analyze_result) = &module.analyze_result else {
                continue;
            };
            modules.insert(&module.path, analyze_result);
        }
        file_based_exports(modules)
    }

    #[async_recursion]
    pub async fn resolve_export(
        &mut self,
        component_id: ComponentId,
        attributes: &[PathComponent],
    ) -> anyhow::Result<Option<Resource>> {
        let udf_path = export_to_udf_path(attributes)?;
        let module_path = CanonicalizedComponentModulePath {
            component: component_id,
            module_path: udf_path.module().clone(),
        };
        let Some(module) = ModuleModel::new(self.tx).get_metadata(module_path).await? else {
            return Ok(None);
        };
        let Some(ref analyze_result) = module.analyze_result else {
            return Ok(None);
        };
        for function in &analyze_result.functions {
            if function.visibility != Some(Visibility::Public) {
                continue;
            }
            if &function.name != udf_path.function_name() {
                continue;
            }
            let resource = self
                .resolve(component_id, None, &Reference::Function(udf_path))
                .await?;
            return Ok(Some(resource));
        }
        Ok(None)
    }

    #[fastrace::trace]
    pub async fn resolve_public_export_path(
        &mut self,
        path: ExportPath,
    ) -> anyhow::Result<CanonicalizedComponentFunctionPath> {
        let root_definition = BootstrapComponentsModel::new(self.tx)
            .load_definition(ComponentDefinitionId::Root)
            .await?;
        // Legacy path: If components aren't enabled, just resolve export paths directly
        // to UDF paths.
        if root_definition.is_none() {
            return Ok(CanonicalizedComponentFunctionPath {
                component: ComponentPath::root(),
                udf_path: path.into(),
            });
        }
        let path_components = path.components()?;
        let resource = self
            .resolve_export(ComponentId::Root, &path_components)
            .await?;
        let Some(resource) = resource else {
            // In certain cases non-exported functions can be called, e.g.
            // system auth can call internal functions.
            return Ok(CanonicalizedComponentFunctionPath {
                component: ComponentPath::root(),
                udf_path: path.into(),
            });
        };
        let Resource::Function(path) = resource else {
            anyhow::bail!("Expected a function");
        };
        Ok(path)
    }

    pub async fn preload_resources(
        &mut self,
        component_id: ComponentId,
    ) -> anyhow::Result<BTreeMap<Reference, Resource>> {
        let mut m = BootstrapComponentsModel::new(self.tx);
        let component_type = m.load_component_type(component_id).await?;
        let component_path = m.must_component_path(component_id)?;

        let mut result = BTreeMap::new();

        if let ComponentType::ChildComponent { ref args, .. } = component_type {
            for (name, resource) in args {
                let reference = Reference::ComponentArgument {
                    attributes: vec![name.clone()],
                };
                result.insert(reference, resource.clone());
            }
        }

        let module_metadata = ModuleModel::new(self.tx)
            .get_application_metadata(component_id)
            .await?;
        for module in module_metadata {
            let Some(ref analyze_result) = module.analyze_result else {
                continue;
            };
            for function in &analyze_result.functions {
                let udf_path =
                    CanonicalizedUdfPath::new(module.path.clone(), function.name.clone());
                let function_path = CanonicalizedComponentFunctionPath {
                    component: component_path.clone(),
                    udf_path: udf_path.clone(),
                };
                result.insert(
                    Reference::Function(udf_path),
                    Resource::Function(function_path),
                );
            }
        }

        for (component_name, child_component_id) in
            self.component_children_ids(component_id).await?
        {
            for (attributes, resource) in
                self.preload_exported_resources(child_component_id).await?
            {
                let reference = Reference::ChildComponent {
                    component: component_name.clone(),
                    attributes,
                };
                result.insert(reference, resource);
            }
        }

        Ok(result)
    }

    pub async fn component_children_ids(
        &mut self,
        component_id: ComponentId,
    ) -> anyhow::Result<BTreeMap<ComponentName, ComponentId>> {
        Ok(self
            .component_children(component_id)
            .await?
            .into_iter()
            .map(|(name, component)| (name, ComponentId::Child(component.id().into())))
            .collect())
    }

    async fn component_children(
        &mut self,
        component_id: ComponentId,
    ) -> anyhow::Result<BTreeMap<ComponentName, ParsedDocument<ComponentMetadata>>> {
        let mut m = BootstrapComponentsModel::new(self.tx);
        let component = m.load_component(component_id).await?;
        let definition_id = m.component_definition(component_id).await?;
        let definition = m.load_definition_metadata(definition_id).await?;

        let mut result = BTreeMap::new();
        if let Some(component) = component {
            for instantiation in definition.child_components {
                let parent = (component.id().into(), instantiation.name.clone());
                let Some(child_component) = m.component_in_parent(Some(parent.clone()))? else {
                    // This shouldn't happen, but it is possible because of a bug where we
                    // weren't deleting child_components when we unmounted the child component.
                    tracing::error!("Missing child component: {:?}", parent);
                    continue;
                };
                result.insert(instantiation.name, child_component);
            }
        }

        Ok(result)
    }

    pub async fn preload_exported_resources(
        &mut self,
        component_id: ComponentId,
    ) -> anyhow::Result<BTreeMap<Vec<PathComponent>, Resource>> {
        let exports = self.load_component_exports(component_id).await?;
        let mut stack = vec![(vec![], &exports)];
        let mut result = BTreeMap::new();
        while let Some((path, internal_node)) = stack.pop() {
            for (name, export) in internal_node {
                match export {
                    ComponentExport::Branch(children) => {
                        let mut new_path = path.clone();
                        new_path.push(name.clone());
                        stack.push((new_path, children));
                    },
                    ComponentExport::Leaf(reference) => {
                        let mut new_path = path.clone();
                        new_path.push(name.clone());
                        let resource = self.resolve(component_id, None, reference).await?;
                        result.insert(new_path, resource);
                    },
                }
            }
        }

        Ok(result)
    }
}
