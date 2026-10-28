use std::{
    collections::BTreeMap,
    sync::Arc,
};

use anyhow::Context;
use common::{
    bootstrap_model::{
        components::{
            definition::{
                ComponentDefinitionMetadata,
                ComponentDefinitionType,
            },
            ComponentMetadata,
            ComponentState,
            ComponentType,
        },
        schema::SchemaState,
    },
    components::{
        ComponentDefinitionPath,
        ComponentId,
        ComponentName,
        ComponentPath,
    },
    document::ParsedDocument,
    runtime::Runtime,
};
use database::{
    BootstrapComponentsModel,
    IndexModel,
    SchemaDiff,
    SchemaModel,
    SchemaValidationProgressTable,
    SchemaValidationTable,
    SchemasTable,
    SerializedSchemaDiff,
    SystemMetadataModel,
    TableModel,
    Transaction,
    COMPONENTS_TABLE,
    COMPONENT_DEFINITIONS_TABLE,
    SCHEMAS_TABLE,
};
use errors::ErrorMetadata;
use serde::{
    Deserialize,
    Serialize,
};
use strum::AsRefStr;
use sync_types::CanonicalizedModulePath;
use value::{
    DeveloperDocumentId,
    InternalDocumentId,
    ResolvedDocumentId,
    TableNamespace,
};

use super::{
    handles::FunctionHandlesModel,
    type_checking::CheckedComponent,
    types::EvaluatedComponentDefinition,
};
use crate::{
    component_system_tables,
    config::types::{
        CronDiff,
        ModuleConfig,
        ModuleDiff,
        UdfServerVersionDiff,
    },
    cron_jobs::CronModel,
    deployment_audit_log::types::{
        AuditLogIndexDiff,
        SerializedIndexDiff,
    },
    initialize_application_system_table,
    modules::{
        module_versions::AnalyzedModule,
        ModuleModel,
    },
    source_packages::{
        types::SourcePackage,
        SourcePackageModel,
    },
    udf_config::{
        types::UdfConfig,
        UdfConfigModel,
    },
    DEFAULT_TABLE_NUMBERS,
};

pub struct ComponentDefinitionConfigModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> ComponentDefinitionConfigModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    #[fastrace::trace]
    pub async fn apply_component_definitions_diff(
        &mut self,
        new_definitions: &BTreeMap<ComponentDefinitionPath, EvaluatedComponentDefinition>,
        source_packages: &BTreeMap<ComponentDefinitionPath, SourcePackage>,
        downloaded_source_packages: &BTreeMap<
            ComponentDefinitionPath,
            BTreeMap<CanonicalizedModulePath, ModuleConfig>,
        >,
    ) -> anyhow::Result<(
        BTreeMap<ComponentDefinitionPath, ComponentDefinitionDiff>,
        BTreeMap<DeveloperDocumentId, NewModules>,
        BTreeMap<DeveloperDocumentId, UdfConfig>,
    )> {
        let mut definition_diffs = BTreeMap::new();

        let existing_definitions = BootstrapComponentsModel::new(self.tx)
            .load_all_definitions()
            .await?;

        // Delete all definitions that aren't in the new set.
        for (definition_path, existing_definition) in &existing_definitions {
            if new_definitions.contains_key(definition_path) {
                continue;
            }
            let diff = self
                .delete_component_definition(existing_definition)
                .await?;
            definition_diffs.insert(definition_path.clone(), diff);
        }
        let mut modules_by_definition = BTreeMap::new();
        let mut udf_config_by_definition = BTreeMap::new();

        for (definition_path, new_definition) in new_definitions {
            let source_package = source_packages.get(definition_path).ok_or_else(|| {
                ErrorMetadata::bad_request(
                    "MissingSourcePackage",
                    "Missing source package for component",
                )
            })?;

            let downloaded_source_package = downloaded_source_packages
                .get(definition_path)
                .context("Missing downloaded source package for component")?;

            let mut functions = new_definition.functions.clone();
            let mut new_modules = vec![];

            for (module_path, module) in downloaded_source_package {
                // NB: The source package here may contain more modules (e.g. `_deps/*`) that
                // aren't in `new_definition.functions`.
                if !module_path.is_deps() && !functions.contains_key(module_path) {
                    // TODO: It's a bit kludgy that we're filling in a default value here rather
                    // than earlier in the push pipeline.
                    tracing::warn!("Module not in functions: {:?}", module_path);
                    functions.insert(module_path.clone(), AnalyzedModule::default());
                }
                new_modules.push(module.clone());
            }

            let (id, diff) = match existing_definitions.get(definition_path) {
                Some(existing_definition) => (
                    existing_definition.id().into(),
                    self.modify_component_definition(
                        existing_definition,
                        new_definition.definition.clone(),
                    )
                    .await?,
                ),
                None => {
                    self.create_component_definition(new_definition.definition.clone())
                        .await?
                },
            };
            definition_diffs.insert(definition_path.clone(), diff);

            modules_by_definition.insert(
                id,
                NewModules {
                    modules: new_modules,
                    source_package: source_package.clone(),
                    analyze_results: functions,
                },
            );
            udf_config_by_definition.insert(id, new_definition.udf_config.clone());
        }

        Ok((
            definition_diffs,
            modules_by_definition,
            udf_config_by_definition,
        ))
    }

    #[fastrace::trace]
    pub async fn create_component_definition(
        &mut self,
        definition: ComponentDefinitionMetadata,
    ) -> anyhow::Result<(DeveloperDocumentId, ComponentDefinitionDiff)> {
        let id = SystemMetadataModel::new_global(self.tx)
            .insert(&COMPONENT_DEFINITIONS_TABLE, definition.clone().try_into()?)
            .await?;

        let diff = ComponentDefinitionDiff {};
        Ok((id.into(), diff))
    }

    #[fastrace::trace]
    pub async fn modify_component_definition(
        &mut self,
        existing: &ParsedDocument<ComponentDefinitionMetadata>,
        new_definition: ComponentDefinitionMetadata,
    ) -> anyhow::Result<ComponentDefinitionDiff> {
        SystemMetadataModel::new_global(self.tx)
            .replace(existing.id(), new_definition.clone().try_into()?)
            .await?;
        let diff = ComponentDefinitionDiff {};
        Ok(diff)
    }

    #[fastrace::trace]
    pub async fn delete_component_definition(
        &mut self,
        existing: &ParsedDocument<ComponentDefinitionMetadata>,
    ) -> anyhow::Result<ComponentDefinitionDiff> {
        SystemMetadataModel::new_global(self.tx)
            .delete(existing.id())
            .await?;
        // TODO: Delete the module system tables.
        Ok(ComponentDefinitionDiff {})
    }
}

#[derive(Debug)]
pub struct ComponentDefinitionDiff {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedComponentDefinitionDiff {}

impl TryFrom<ComponentDefinitionDiff> for SerializedComponentDefinitionDiff {
    type Error = anyhow::Error;

    fn try_from(_: ComponentDefinitionDiff) -> Result<Self, Self::Error> {
        Ok(Self {})
    }
}

pub struct ComponentConfigModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

pub struct NewModules {
    modules: Vec<ModuleConfig>,
    source_package: SourcePackage,
    analyze_results: BTreeMap<CanonicalizedModulePath, AnalyzedModule>,
}

impl<'a, RT: Runtime> ComponentConfigModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    #[fastrace::trace]
    pub async fn start_component_schema_changes(
        &mut self,
        app: &CheckedComponent,
        new_definitions: &BTreeMap<ComponentDefinitionPath, EvaluatedComponentDefinition>,
        skip_index_diff: bool,
    ) -> anyhow::Result<SchemaChange> {
        let existing_components_by_parent = BootstrapComponentsModel::new(self.tx)
            .load_all_components()
            .await?
            .into_iter()
            .map(|c| (c.parent_and_name(), c))
            .collect::<BTreeMap<_, _>>();

        let mut allocated_component_ids = BTreeMap::new();
        let mut schema_ids = BTreeMap::new();
        let mut index_diffs = BTreeMap::new();

        let existing_root = existing_components_by_parent.get(&None);
        let mut stack = vec![(ComponentPath::root(), existing_root, Some(app))];
        while let Some((path, existing_node, new_node)) = stack.pop() {
            // First, diff the schemas of the existing and new nodes.
            let internal_id = match (existing_node, new_node) {
                // Creating a new component. We need to allocate a component ID
                // here for the table namespace.
                (None, Some(..)) => {
                    let id = self.initialize_component_namespace(path.is_root()).await?;
                    allocated_component_ids.insert(path.clone(), id);
                    id
                },
                // Updating an existing component.
                (Some(node), Some(..)) => node.id().into(),

                // Deleting an existing component.
                (Some(node), None) => node.id().into(),

                (None, None) => anyhow::bail!("Unexpected None/None in stack"),
            };
            let component_id = if path.is_root() {
                ComponentId::Root
            } else {
                ComponentId::Child(internal_id)
            };
            if let Some(new_node) = new_node {
                let namespace = TableNamespace::from(component_id);
                let definition = new_definitions
                    .get(&new_node.definition_path)
                    .context("Missing definition for component")?;
                let schema_id = if let Some(ref schema) = definition.schema {
                    if !skip_index_diff {
                        let index_diff = IndexModel::new(self.tx)
                            .prepare_new_and_mutated_indexes(namespace, schema)
                            .await?;
                        index_diffs.insert(path.clone(), index_diff.into());
                    }

                    let (schema_id, schema_state) = SchemaModel::new(self.tx, namespace)
                        .submit_pending(schema.clone())
                        .await?;
                    match schema_state {
                        SchemaState::Pending | SchemaState::Validated | SchemaState::Active => (),
                        SchemaState::Failed { .. } | SchemaState::Overwritten => {
                            anyhow::bail!(
                                "Unexpected state for newly written schema: {schema_state:?}"
                            );
                        },
                    };
                    Some(schema_id.into())
                } else {
                    // Properly populate the index diff when there is no schema in the new push. We
                    // treat this as no indexes being defined
                    if !skip_index_diff {
                        let index_diff = IndexModel::new(self.tx)
                            .get_index_diff(namespace, &BTreeMap::new())
                            .await?;
                        index_diffs.insert(path.clone(), index_diff.into());
                    }
                    None
                };
                schema_ids.insert(path.clone(), schema_id);
            } else {
                tracing::warn!(
                    "Leaving existing schema and tables in place for deleted component: {path:?}"
                );
            }
            // Second, push children to traverse onto the stack.
            for child in tree_diff_children(&existing_components_by_parent, new_node, internal_id) {
                stack.push((path.join(child.name.clone()), child.existing, child.new));
            }
        }

        Ok(SchemaChange {
            allocated_component_ids,
            schema_ids,
            index_diffs,
        })
    }

    #[fastrace::trace]
    pub async fn initialize_component_namespace(
        &mut self,
        is_root: bool,
    ) -> anyhow::Result<DeveloperDocumentId> {
        let internal_id = SystemMetadataModel::new_global(self.tx).allocate_internal_id()?;
        let table_id = self
            .tx
            .table_mapping()
            .namespace(TableNamespace::Global)
            .name_to_id()(COMPONENTS_TABLE.clone())?;
        let id = DeveloperDocumentId::new(table_id.table_number, internal_id);
        let component_id = ComponentId::new(is_root, id);

        if matches!(component_id, ComponentId::Root) {
            tracing::info!(
                "No-op initializing component tables in global namespace, because they already \
                 exist."
            );
            return Ok(id);
        }
        initialize_application_system_table(
            self.tx,
            &SchemasTable,
            component_id.into(),
            &DEFAULT_TABLE_NUMBERS,
        )
        .await?;
        initialize_application_system_table(
            self.tx,
            &SchemaValidationProgressTable,
            component_id.into(),
            &DEFAULT_TABLE_NUMBERS,
        )
        .await?;
        initialize_application_system_table(
            self.tx,
            &SchemaValidationTable,
            component_id.into(),
            &DEFAULT_TABLE_NUMBERS,
        )
        .await?;
        for table in component_system_tables() {
            initialize_application_system_table(
                self.tx,
                table,
                component_id.into(),
                &DEFAULT_TABLE_NUMBERS,
            )
            .await?;
        }
        Ok(id)
    }

    fn schema_id_from_schema_change(
        &mut self,
        schema_change: &SchemaChange,
        path: &ComponentPath,
    ) -> anyhow::Result<Option<ResolvedDocumentId>> {
        schema_change
            .schema_ids
            .get(path)
            .context("Missing schema ID")?
            .map(|id| {
                let table_number = self.tx.table_mapping().tablet_number(id.table())?;
                anyhow::Ok(ResolvedDocumentId::new(
                    id.table(),
                    DeveloperDocumentId::new(table_number, id.internal_id()),
                ))
            })
            .transpose()
    }

    #[fastrace::trace]
    pub async fn apply_component_tree_diff(
        &mut self,
        app: &CheckedComponent,
        udf_config_by_definition: BTreeMap<DeveloperDocumentId, UdfConfig>,
        schema_change: &SchemaChange,
        modules_by_definition: BTreeMap<DeveloperDocumentId, NewModules>,
    ) -> anyhow::Result<BTreeMap<ComponentPath, ComponentDiff>> {
        let definition_id_by_path = BootstrapComponentsModel::new(self.tx)
            .load_all_definitions()
            .await?
            .into_iter()
            .map(|(path, d)| (path, d.id().into()))
            .collect::<BTreeMap<_, _>>();

        let existing_components_by_parent = BootstrapComponentsModel::new(self.tx)
            .load_all_components()
            .await?
            .into_iter()
            .map(|c| (c.parent_and_name(), c))
            .collect::<BTreeMap<_, _>>();

        let existing_root = existing_components_by_parent.get(&None);
        let mut stack = vec![(ComponentPath::root(), None, existing_root, Some(app))];
        let mut diffs = BTreeMap::new();
        while let Some((path, parent_and_name, existing_node, new_node)) = stack.pop() {
            let new_metadata = new_node
                .map(|new_node| {
                    let definition_id = *definition_id_by_path
                        .get(&new_node.definition_path)
                        .context("Missing definition ID for component")?;
                    let component_type = match parent_and_name {
                        None => {
                            anyhow::ensure!(new_node.args.is_empty());
                            anyhow::ensure!(new_node.env.is_empty());
                            ComponentType::App
                        },
                        Some((parent, name)) => ComponentType::ChildComponent {
                            parent,
                            name,
                            args: new_node.args.clone(),
                            env: new_node.env.clone(),
                        },
                    };
                    Ok(ComponentMetadata {
                        definition_id,
                        component_type,
                        state: ComponentState::Active,
                        http_prefix: new_node.http_prefix.clone(),
                    })
                })
                .transpose()?;

            // Diff the node itself.
            let (internal_id, diff) = match (existing_node, new_metadata) {
                // Create a new node.
                (None, Some(new_metadata)) => {
                    let internal_id = *schema_change
                        .allocated_component_ids
                        .get(&path)
                        .context("Missing allocated component ID")?;
                    let schema_id = self.schema_id_from_schema_change(schema_change, &path)?;
                    self.create_component(
                        internal_id,
                        new_metadata,
                        &modules_by_definition,
                        &udf_config_by_definition,
                        schema_id,
                    )
                    .await?
                },
                // Update a node.
                (Some(existing_node), Some(new_metadata)) => {
                    // A concurrent push created this component with a different namespace
                    // than our `start_push` allocated.
                    if let Some(allocated_id) = schema_change.allocated_component_ids.get(&path)
                        && *allocated_id != existing_node.developer_id()
                    {
                        anyhow::bail!(ErrorMetadata::conflict(
                            "RaceDetected",
                            format!("Component {path} was created by a concurrent push"),
                        ));
                    }
                    let schema_id = self.schema_id_from_schema_change(schema_change, &path)?;
                    self.modify_component(
                        existing_node,
                        new_metadata,
                        &modules_by_definition,
                        &udf_config_by_definition,
                        schema_id,
                    )
                    .await?
                },
                // Unmount an existing node.
                (Some(existing_node), None) => {
                    // Don't recurse into unmounted nodes.
                    if existing_node.state == ComponentState::Unmounted {
                        continue;
                    }
                    self.unmount_component(existing_node).await?
                },
                (None, None) => anyhow::bail!("Unexpected None/None in stack"),
            };
            diffs.insert(path.clone(), diff);

            // After diffing the current node, push children to traverse onto the stack.
            for child in tree_diff_children(&existing_components_by_parent, new_node, internal_id) {
                stack.push((
                    path.join(child.name.clone()),
                    Some((internal_id, child.name)),
                    child.existing,
                    child.new,
                ));
            }
        }
        Ok(diffs)
    }

    #[fastrace::trace]
    pub async fn create_component(
        &mut self,
        id: DeveloperDocumentId,
        metadata: ComponentMetadata,
        modules_by_definition: &BTreeMap<DeveloperDocumentId, NewModules>,
        udf_config_by_definition: &BTreeMap<DeveloperDocumentId, UdfConfig>,
        schema_id: Option<ResolvedDocumentId>,
    ) -> anyhow::Result<(DeveloperDocumentId, ComponentDiff)> {
        let modules = modules_by_definition
            .get(&metadata.definition_id)
            .context("Missing modules for component definition")?;
        let udf_config = udf_config_by_definition
            .get(&metadata.definition_id)
            .context("Missing UDF config for component definition")?;
        let is_root = metadata.component_type.is_root();
        let document_id = SystemMetadataModel::new_global(self.tx)
            .insert_with_internal_id(&COMPONENTS_TABLE, id.internal_id(), metadata.try_into()?)
            .await?;
        anyhow::ensure!(DeveloperDocumentId::from(document_id) == id);
        let component_id = ComponentId::new(is_root, id);
        let udf_config_diff = UdfConfigModel::new(self.tx, component_id.into())
            .set(udf_config.clone())
            .await?;
        let source_package_id = SourcePackageModel::new(self.tx, component_id.into())
            .put(modules.source_package.clone())
            .await?;

        let module_diff = ModuleModel::new(self.tx)
            .apply(
                component_id,
                modules.modules.clone(),
                Some(source_package_id),
                modules.analyze_results.clone(),
            )
            .await?;
        let cron_diff = CronModel::new(self.tx, component_id)
            .apply(&modules.analyze_results)
            .await?;
        FunctionHandlesModel::new(self.tx)
            .apply_config_diff(component_id, Some(&modules.analyze_results))
            .await?;
        let (schema_diff, next_schema) = SchemaModel::new(self.tx, component_id.into())
            .apply(schema_id)
            .await?;

        let index_diff = IndexModel::new(self.tx)
            .get_full_index_diff(component_id.into(), &next_schema)
            .await?
            .into();
        IndexModel::new(self.tx)
            .apply(component_id.into(), &next_schema)
            .await?;
        Ok((
            id,
            ComponentDiff {
                diff_type: ComponentDiffType::Create,
                module_diff,
                udf_config_diff,
                cron_diff,
                index_diff,
                schema_diff,
            },
        ))
    }

    #[fastrace::trace]
    async fn modify_component(
        &mut self,
        existing: &ParsedDocument<ComponentMetadata>,
        new_metadata: ComponentMetadata,
        modules_by_definition: &BTreeMap<DeveloperDocumentId, NewModules>,
        udf_config_by_definition: &BTreeMap<DeveloperDocumentId, UdfConfig>,
        schema_id: Option<ResolvedDocumentId>,
    ) -> anyhow::Result<(DeveloperDocumentId, ComponentDiff)> {
        let component_id = if existing.parent_and_name().is_none() {
            ComponentId::Root
        } else {
            ComponentId::Child(existing.id().into())
        };
        let modules = modules_by_definition
            .get(&new_metadata.definition_id)
            .context("Missing modules for component definition")?;
        let udf_config = udf_config_by_definition
            .get(&new_metadata.definition_id)
            .context("Missing UDF config for component definition")?;
        SystemMetadataModel::new_global(self.tx)
            .replace(existing.id(), new_metadata.try_into()?)
            .await?;
        let source_package_id = SourcePackageModel::new(self.tx, component_id.into())
            .put(modules.source_package.clone())
            .await?;
        let udf_config_diff = UdfConfigModel::new(self.tx, component_id.into())
            .set(udf_config.clone())
            .await?;
        let module_diff = ModuleModel::new(self.tx)
            .apply(
                component_id,
                modules.modules.clone(),
                Some(source_package_id),
                modules.analyze_results.clone(),
            )
            .await?;
        let cron_diff = CronModel::new(self.tx, component_id)
            .apply(&modules.analyze_results)
            .await?;
        FunctionHandlesModel::new(self.tx)
            .apply_config_diff(component_id, Some(&modules.analyze_results))
            .await?;
        let (schema_diff, next_schema) = SchemaModel::new(self.tx, component_id.into())
            .apply(schema_id)
            .await?;

        let index_diff = IndexModel::new(self.tx)
            .get_full_index_diff(component_id.into(), &next_schema)
            .await?
            .into();
        IndexModel::new(self.tx)
            .apply(component_id.into(), &next_schema)
            .await?;

        let diff_type = if existing.state == ComponentState::Unmounted {
            ComponentDiffType::Remount
        } else {
            ComponentDiffType::Modify
        };
        Ok((
            existing.id().into(),
            ComponentDiff {
                diff_type,
                module_diff,
                udf_config_diff,
                cron_diff,
                index_diff,
                schema_diff,
            },
        ))
    }

    #[fastrace::trace]
    async fn unmount_component(
        &mut self,
        existing: &ParsedDocument<ComponentMetadata>,
    ) -> anyhow::Result<(DeveloperDocumentId, ComponentDiff)> {
        let component_id = if existing.parent_and_name().is_none() {
            ComponentId::Root
        } else {
            ComponentId::Child(existing.id().into())
        };
        let mut unmounted_metadata = existing.clone().into_value();
        unmounted_metadata.state = ComponentState::Unmounted;
        SystemMetadataModel::new_global(self.tx)
            .replace(existing.id(), unmounted_metadata.try_into()?)
            .await?;
        let module_diff = ModuleModel::new(self.tx)
            .apply(component_id, vec![], None, BTreeMap::new())
            .await?;
        let cron_diff = CronModel::new(self.tx, component_id)
            .apply(&BTreeMap::new())
            .await?;
        FunctionHandlesModel::new(self.tx)
            .apply_config_diff(component_id, None)
            .await?;
        Ok((
            existing.id().into(),
            ComponentDiff {
                diff_type: ComponentDiffType::Unmount,
                module_diff,
                udf_config_diff: None,
                cron_diff,
                index_diff: AuditLogIndexDiff::default(),
                schema_diff: None,
            },
        ))
    }

    #[fastrace::trace]
    pub async fn delete_component(&mut self, component_id: ComponentId) -> anyhow::Result<()> {
        if component_id.is_root() {
            anyhow::bail!("Cannot delete root component");
        }

        // First, walk the component tree and validate that it's okay to delete each
        // component.
        let component = BootstrapComponentsModel::new(self.tx)
            .load_component(component_id)
            .await?;
        let Some(component) = component else {
            anyhow::bail!(ErrorMetadata::not_found(
                "ComponentNotFound",
                format!("Component with ID {component_id:?} not found")
            ));
        };
        let mut stack = vec![component];
        let mut all_ids = vec![];
        while let Some(component) = stack.pop() {
            if component.state != ComponentState::Unmounted {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "ComponentMustBeUnmounted",
                    "Component must be unmounted before deletion"
                ));
            }
            let children =
                BootstrapComponentsModel::new(self.tx).component_children(component.id().into())?;
            stack.extend(children);
            all_ids.push(component.id());
        }

        // Delete the components we found.
        for component_id in all_ids {
            SystemMetadataModel::new_global(self.tx)
                .delete(component_id)
                .await?;

            let namespace = TableNamespace::from(ComponentId::Child(component_id.into()));
            // delete the schema table first
            // tables defined in the schema cannot be deleted, so we delete the _schemas
            // table first to remove that restriction
            TableModel::new(self.tx)
                .delete_active_table(namespace, SCHEMAS_TABLE.clone())
                .await?;

            // then delete all tables, including system tables and hidden tables
            let namespaced_table_mapping = self.tx.table_mapping().namespace(namespace);
            for (tablet_id, ..) in namespaced_table_mapping.iter() {
                TableModel::new(self.tx)
                    .delete_table_by_id_bypassing_schema_enforcement(tablet_id)
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn disable_components(&mut self) -> anyhow::Result<()> {
        let components = BootstrapComponentsModel::new(self.tx)
            .load_all_components()
            .await?;
        for component in components {
            if component.component_type.is_root() {
                continue;
            }
            if component.state == ComponentState::Unmounted {
                continue;
            }
            tracing::warn!("Unmounting component: {:?}", &*component);
            self.unmount_component(&component).await?;
        }
        let existing_definitions = BootstrapComponentsModel::new(self.tx)
            .load_all_definitions()
            .await?;
        for (definition_path, definition) in existing_definitions {
            if definition_path.is_root() {
                ComponentDefinitionConfigModel::new(self.tx)
                    .modify_component_definition(
                        &definition,
                        ComponentDefinitionMetadata {
                            path: definition_path,
                            definition_type: ComponentDefinitionType::App,
                            child_components: Vec::new(),
                            http_mounts: BTreeMap::new(),
                            http_prefix: None,
                            exports: BTreeMap::new(),
                            env_vars: BTreeMap::new(),
                        },
                    )
                    .await?;
                continue;
            }
            ComponentDefinitionConfigModel::new(self.tx)
                .delete_component_definition(&definition)
                .await?;
        }

        Ok(())
    }
}

fn tree_diff_children<'a>(
    existing_components_by_parent: &'a BTreeMap<
        Option<(DeveloperDocumentId, ComponentName)>,
        Arc<ParsedDocument<ComponentMetadata>>,
    >,
    new_node: Option<&'a CheckedComponent>,
    internal_id: DeveloperDocumentId,
) -> impl Iterator<Item = TreeDiffChild<'a>> {
    std::iter::from_coroutine(
        #[coroutine]
        move || {
            // First, visit children of the existing node.
            for (parent_and_name, existing_child) in
                existing_components_by_parent.range(Some((internal_id, ComponentName::min()))..)
            {
                let Some((parent, name)) = parent_and_name else {
                    break;
                };
                if parent != &internal_id {
                    break;
                }
                let new_node = new_node.and_then(|new_node| new_node.child_components.get(name));
                yield TreeDiffChild {
                    name: name.clone(),
                    existing: Some(existing_child),
                    new: new_node,
                };
            }
            // Next, visit children of the new node that aren't in the existing node.
            if let Some(new_node) = new_node {
                for (name, new_child) in &new_node.child_components {
                    if existing_components_by_parent
                        .contains_key(&Some((internal_id, name.clone())))
                    {
                        continue;
                    }
                    yield TreeDiffChild {
                        name: name.clone(),
                        existing: None,
                        new: Some(new_child),
                    };
                }
            }
        },
    )
}

struct TreeDiffChild<'a> {
    name: ComponentName,
    existing: Option<&'a Arc<ParsedDocument<ComponentMetadata>>>,
    new: Option<&'a CheckedComponent>,
}

#[derive(Debug, Clone, AsRefStr)]
pub enum ComponentDiffType {
    Create,
    Modify,
    Unmount,
    Remount,
}

#[derive(Debug, Clone)]
pub struct ComponentDiff {
    pub diff_type: ComponentDiffType,
    pub module_diff: ModuleDiff,
    pub udf_config_diff: Option<UdfServerVersionDiff>,
    pub cron_diff: CronDiff,
    pub index_diff: AuditLogIndexDiff,
    pub schema_diff: Option<SchemaDiff>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SerializedComponentDiffType {
    Create,
    Modify,
    Unmount,
    Remount,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedComponentDiff {
    diff_type: SerializedComponentDiffType,
    module_diff: ModuleDiff,
    udf_config_diff: Option<UdfServerVersionDiff>,
    cron_diff: CronDiff,
    index_diff: Option<SerializedIndexDiff>,
    schema_diff: Option<SerializedSchemaDiff>,
}

impl TryFrom<ComponentDiffType> for SerializedComponentDiffType {
    type Error = anyhow::Error;

    fn try_from(value: ComponentDiffType) -> Result<Self, Self::Error> {
        Ok(match value {
            ComponentDiffType::Create => Self::Create,
            ComponentDiffType::Modify => Self::Modify,
            ComponentDiffType::Unmount => Self::Unmount,
            ComponentDiffType::Remount => Self::Remount,
        })
    }
}

impl TryFrom<SerializedComponentDiffType> for ComponentDiffType {
    type Error = anyhow::Error;

    fn try_from(value: SerializedComponentDiffType) -> Result<Self, Self::Error> {
        Ok(match value {
            SerializedComponentDiffType::Create => Self::Create,
            SerializedComponentDiffType::Modify => Self::Modify,
            SerializedComponentDiffType::Unmount => Self::Unmount,
            SerializedComponentDiffType::Remount => Self::Remount,
        })
    }
}

impl TryFrom<ComponentDiff> for SerializedComponentDiff {
    type Error = anyhow::Error;

    fn try_from(value: ComponentDiff) -> Result<Self, Self::Error> {
        Ok(Self {
            diff_type: value.diff_type.try_into()?,
            module_diff: value.module_diff,
            udf_config_diff: value.udf_config_diff,
            cron_diff: value.cron_diff,
            index_diff: Some(value.index_diff.into()),
            schema_diff: value.schema_diff.map(|diff| diff.try_into()).transpose()?,
        })
    }
}

impl TryFrom<SerializedComponentDiff> for ComponentDiff {
    type Error = anyhow::Error;

    fn try_from(value: SerializedComponentDiff) -> Result<Self, Self::Error> {
        Ok(Self {
            diff_type: value.diff_type.try_into()?,
            module_diff: value.module_diff,
            udf_config_diff: value.udf_config_diff,
            cron_diff: value.cron_diff,
            index_diff: match value.index_diff {
                Some(index_diff) => index_diff.try_into()?,
                None => AuditLogIndexDiff::default(),
            },
            schema_diff: value.schema_diff.map(|diff| diff.try_into()).transpose()?,
        })
    }
}

#[derive(Clone, Debug)]
pub struct SchemaChange {
    pub allocated_component_ids: BTreeMap<ComponentPath, DeveloperDocumentId>,
    pub schema_ids: BTreeMap<ComponentPath, Option<InternalDocumentId>>,
    pub index_diffs: BTreeMap<ComponentPath, AuditLogIndexDiff>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedSchemaChange {
    allocated_component_ids: BTreeMap<String, String>,
    schema_ids: BTreeMap<String, Option<String>>,
    #[serde(default)]
    index_diffs: BTreeMap<String, SerializedIndexDiff>,
}

impl TryFrom<SchemaChange> for SerializedSchemaChange {
    type Error = anyhow::Error;

    fn try_from(value: SchemaChange) -> Result<Self, Self::Error> {
        Ok(Self {
            allocated_component_ids: value
                .allocated_component_ids
                .into_iter()
                .map(|(k, v)| (String::from(k), String::from(v)))
                .collect(),
            schema_ids: value
                .schema_ids
                .into_iter()
                .map(|(k, v)| (String::from(k), v.map(String::from)))
                .collect(),
            index_diffs: value
                .index_diffs
                .into_iter()
                .map(|(k, v)| (String::from(k), v.into()))
                .collect(),
        })
    }
}

impl TryFrom<SerializedSchemaChange> for SchemaChange {
    type Error = anyhow::Error;

    fn try_from(value: SerializedSchemaChange) -> Result<Self, Self::Error> {
        Ok(Self {
            allocated_component_ids: value
                .allocated_component_ids
                .into_iter()
                .map(|(k, v)| Ok((k.parse()?, v.parse()?)))
                .collect::<anyhow::Result<_>>()?,
            schema_ids: value
                .schema_ids
                .into_iter()
                .map(|(k, v)| Ok((k.parse()?, v.map(|v| v.parse()).transpose()?)))
                .collect::<anyhow::Result<_>>()?,
            index_diffs: value
                .index_diffs
                .into_iter()
                .map(|(k, v)| Ok((k.parse()?, v.try_into()?)))
                .collect::<anyhow::Result<_>>()?,
        })
    }
}
