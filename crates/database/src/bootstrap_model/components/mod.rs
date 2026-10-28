pub mod definition;

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        LazyLock,
    },
};

use anyhow::Context;
use common::{
    bootstrap_model::components::{
        definition::{
            ComponentDefinitionMetadata,
            ComponentDefinitionType,
        },
        ComponentMetadata,
        ComponentType,
        EnvBinding,
    },
    components::{
        CanonicalizedComponentFunctionPath,
        CanonicalizedComponentModulePath,
        ComponentDefinitionId,
        ComponentDefinitionPath,
        ComponentId,
        ComponentName,
        ComponentPath,
        Resource,
    },
    document::{
        ParseDocument,
        ParsedDocument,
    },
    runtime::Runtime,
};
use errors::ErrorMetadata;
use value::{
    identifier::Identifier,
    ConvexValue,
    DeveloperDocumentId,
    FieldPath,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

use crate::{
    metrics,
    system_tables::{
        SystemIndex,
        SystemTable,
    },
    ComponentDefinitionsTable,
    Transaction,
    COMPONENT_DEFINITIONS_TABLE,
};

pub const COMPONENTS_TABLE: TableName = TableName::const_new("_components");

pub static COMPONENTS_BY_PARENT_INDEX: LazyLock<SystemIndex<ComponentsTable>> =
    LazyLock::new(|| SystemIndex::new("by_parent_and_name", [&PARENT_FIELD, &NAME_FIELD]).unwrap());
pub static PARENT_FIELD: LazyLock<FieldPath> = LazyLock::new(|| "parent".parse().unwrap());
pub static NAME_FIELD: LazyLock<FieldPath> = LazyLock::new(|| "name".parse().unwrap());

pub struct ComponentsTable;

impl SystemTable for ComponentsTable {
    type Metadata = ComponentMetadata;

    const TABLE_NAME: TableName = COMPONENTS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![COMPONENTS_BY_PARENT_INDEX.clone()]
    }
}

pub struct BootstrapComponentsModel<'a, RT: Runtime> {
    pub tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> BootstrapComponentsModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    pub fn component_in_parent(
        &mut self,
        parent_and_name: Option<(DeveloperDocumentId, ComponentName)>,
    ) -> anyhow::Result<Option<ParsedDocument<ComponentMetadata>>> {
        self.tx
            .component_registry
            .component_in_parent(parent_and_name, &mut self.tx.reads)
    }

    pub fn component_children(
        &mut self,
        parent_id: DeveloperDocumentId,
    ) -> anyhow::Result<Vec<ParsedDocument<ComponentMetadata>>> {
        self.tx
            .component_registry
            .component_children(parent_id, &mut self.tx.reads)
    }

    pub fn root_component(&mut self) -> anyhow::Result<Option<ParsedDocument<ComponentMetadata>>> {
        self.component_in_parent(None)
    }

    pub fn resolve_path(
        &mut self,
        path: &ComponentPath,
    ) -> anyhow::Result<Option<ParsedDocument<ComponentMetadata>>> {
        self.tx
            .component_registry
            .resolve_path(path, &mut self.tx.reads)
    }

    pub fn all_component_paths(&mut self) -> BTreeMap<ComponentId, ComponentPath> {
        self.tx
            .component_registry
            .all_component_paths(&mut self.tx.reads)
    }

    #[fastrace::trace]
    pub async fn load_all_components(
        &mut self,
    ) -> anyhow::Result<Vec<Arc<ParsedDocument<ComponentMetadata>>>> {
        self.tx
            .query_system(
                TableNamespace::Global,
                &SystemIndex::<ComponentsTable>::by_creation_time(),
            )?
            .all()
            .await
    }

    pub fn resolve_component_id(
        &mut self,
        component_id: DeveloperDocumentId,
    ) -> anyhow::Result<ResolvedDocumentId> {
        let component_table = self
            .tx
            .table_mapping()
            .namespace(TableNamespace::Global)
            .id(&COMPONENTS_TABLE)?;
        Ok(ResolvedDocumentId::new(
            component_table.tablet_id,
            component_id,
        ))
    }

    pub fn resolve_component_definition_id(
        &mut self,
        component_definition_id: DeveloperDocumentId,
    ) -> anyhow::Result<ResolvedDocumentId> {
        let component_definitions_table = self
            .tx
            .table_mapping()
            .namespace(TableNamespace::Global)
            .id(&COMPONENT_DEFINITIONS_TABLE)?;
        Ok(ResolvedDocumentId::new(
            component_definitions_table.tablet_id,
            component_definition_id,
        ))
    }

    pub fn get_component_path(&mut self, component_id: ComponentId) -> Option<ComponentPath> {
        self.tx.get_component_path(component_id)
    }

    pub fn must_component_path(
        &mut self,
        component_id: ComponentId,
    ) -> anyhow::Result<ComponentPath> {
        self.get_component_path(component_id)
            .with_context(|| format!("component {component_id:?} missing"))
    }

    pub async fn component_definition(
        &mut self,
        component: ComponentId,
    ) -> anyhow::Result<ComponentDefinitionId> {
        let component_definition = match component {
            ComponentId::Root => ComponentDefinitionId::Root,
            ComponentId::Child(component_id) => {
                let component_doc_id = self.resolve_component_id(component_id)?;
                let component_doc: ParsedDocument<ComponentMetadata> = self
                    .tx
                    .get(component_doc_id)
                    .await?
                    .context("component missing")?
                    .parse()?;
                ComponentDefinitionId::Child(component_doc.definition_id)
            },
        };
        Ok(component_definition)
    }

    pub async fn load_component(
        &mut self,
        id: ComponentId,
    ) -> anyhow::Result<Option<ParsedDocument<ComponentMetadata>>> {
        let result = match id {
            ComponentId::Root => self.root_component()?,
            ComponentId::Child(internal_id) => {
                let component_doc_id = self.resolve_component_id(internal_id)?;
                self.tx
                    .get(component_doc_id)
                    .await?
                    .map(ParseDocument::parse)
                    .transpose()?
            },
        };
        Ok(result)
    }

    pub async fn load_component_type(&mut self, id: ComponentId) -> anyhow::Result<ComponentType> {
        match self.load_component(id).await? {
            None => {
                if id.is_root() {
                    // The root component's metadata document may be missing if the app hasn't been
                    // updated to use components.
                    Ok(ComponentType::App)
                } else {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidReference",
                        format!("Component {id:?} not found"),
                    ))
                }
            },
            Some(component) => Ok(component.into_value().component_type),
        }
    }

    pub async fn load_component_args(
        &mut self,
        id: ComponentId,
    ) -> anyhow::Result<BTreeMap<Identifier, ConvexValue>> {
        let component = self
            .load_component(id)
            .await?
            .context(format!(
                "Component not found for ComponentId {:?}",
                id.serialize_to_string()
            ))?
            .into_value();
        let args = match component.component_type {
            ComponentType::App => anyhow::bail!(ErrorMetadata::bad_request(
                "InvalidComponentType",
                "Can't load component args within the app",
            )),
            ComponentType::ChildComponent { args, .. } => args,
        };
        let mut result = BTreeMap::new();
        for (name, value) in args {
            let Resource::Value(value) = value else {
                anyhow::bail!("Non-value resource within component args");
            };
            result.insert(name, value);
        }
        Ok(result)
    }

    /// Returns component environment variable bindings.
    ///
    /// These will be resolved against the app's actual environment variable
    /// values at runtime.
    pub async fn load_component_env(
        &mut self,
        id: ComponentId,
    ) -> anyhow::Result<BTreeMap<Identifier, EnvBinding>> {
        let component = self
            .load_component(id)
            .await?
            .context(format!(
                "Component not found for ComponentId {:?}",
                id.serialize_to_string()
            ))?
            .into_value();
        let env = match component.component_type {
            ComponentType::App => anyhow::bail!(ErrorMetadata::bad_request(
                "InvalidComponentType",
                "Can't load component env within the app",
            )),
            ComponentType::ChildComponent { env, .. } => env,
        };
        Ok(env)
    }

    pub async fn load_definition(
        &mut self,
        id: ComponentDefinitionId,
    ) -> anyhow::Result<Option<Arc<ParsedDocument<ComponentDefinitionMetadata>>>> {
        let internal_id = match id {
            ComponentDefinitionId::Root => match self.root_component()? {
                Some(root_component) => root_component.definition_id,
                None => return Ok(None),
            },
            ComponentDefinitionId::Child(id) => id,
        };
        let Some(mut doc) = self
            .tx
            .get_system::<ComponentDefinitionsTable>(TableNamespace::Global, internal_id)
            .await?
        else {
            return Ok(None);
        };
        if !doc.exports.is_empty() {
            metrics::log_nonempty_component_exports();
            Arc::make_mut(&mut doc).exports = BTreeMap::new();
        }
        Ok(Some(doc))
    }

    pub async fn load_definition_metadata(
        &mut self,
        id: ComponentDefinitionId,
    ) -> anyhow::Result<ComponentDefinitionMetadata> {
        match self.load_definition(id).await? {
            Some(doc) => Ok(Arc::unwrap_or_clone(doc).into_value()),
            None => {
                if id.is_root() {
                    // The root component's metadata document may be missing if the app hasn't been
                    // updated to use components.
                    Ok(ComponentDefinitionMetadata {
                        path: ComponentDefinitionPath::root(),
                        definition_type: ComponentDefinitionType::App,
                        child_components: Vec::new(),
                        exports: BTreeMap::new(),
                        http_mounts: BTreeMap::new(),
                        http_prefix: None,
                        env_vars: BTreeMap::new(),
                    })
                } else {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidReference",
                        format!("Component definition {id:?} not found"),
                    ))
                }
            },
        }
    }

    #[fastrace::trace]
    pub async fn load_all_definitions(
        &mut self,
    ) -> anyhow::Result<
        BTreeMap<ComponentDefinitionPath, Arc<ParsedDocument<ComponentDefinitionMetadata>>>,
    > {
        let mut definitions = BTreeMap::new();
        for mut definition in self
            .tx
            .query_system(
                TableNamespace::Global,
                &SystemIndex::<ComponentDefinitionsTable>::by_creation_time(),
            )?
            .all()
            .await?
        {
            if !definition.exports.is_empty() {
                metrics::log_nonempty_component_exports();
                Arc::make_mut(&mut definition).exports = BTreeMap::new();
            }
            anyhow::ensure!(definitions
                .insert(definition.path.clone(), definition)
                .is_none());
        }
        Ok(definitions)
    }

    pub fn component_path_to_ids(
        &mut self,
        path: &ComponentPath,
    ) -> anyhow::Result<Option<(ComponentDefinitionId, ComponentId)>> {
        self.tx
            .component_registry
            .component_path_to_ids(path, &mut self.tx.reads)
    }

    pub fn must_component_path_to_ids(
        &mut self,
        path: &ComponentPath,
    ) -> anyhow::Result<(ComponentDefinitionId, ComponentId)> {
        self.component_path_to_ids(path)?
            .context("Component path not found")
    }

    pub fn function_path_to_module(
        &mut self,
        path: &CanonicalizedComponentFunctionPath,
    ) -> anyhow::Result<CanonicalizedComponentModulePath> {
        let (_, component) = self.must_component_path_to_ids(&path.component)?;
        Ok(CanonicalizedComponentModulePath {
            component,
            module_path: path.udf_path.module().clone(),
        })
    }
}
