use std::collections::BTreeMap;

use anyhow::Context;
use common::{
    bootstrap_model::{
        components::{
            ComponentMetadata,
            ComponentType,
        },
        index::database_index::IndexedFields,
    },
    components::{
        ComponentDefinitionId,
        ComponentId,
        ComponentName,
        ComponentPath,
    },
    document::{
        ParseDocument,
        ParsedDocument,
        ResolvedDocument,
    },
    index::IndexKey,
    interval::Interval,
    types::TabletIndexName,
    value::ResolvedDocumentId,
};
use imbl::OrdMap;
use value::{
    val,
    values_to_bytes,
    DeveloperDocumentId,
    TableMapping,
    TableNamespace,
    TabletId,
};

use crate::{
    bootstrap_model::components::{
        NAME_FIELD,
        PARENT_FIELD,
    },
    TransactionReadSet,
    COMPONENTS_BY_PARENT_INDEX,
    COMPONENTS_TABLE,
};

/// This structure is an index over the `_components` tables.
/// TODO: Make the data structures more efficient. For now we just care about
/// correctness, since the main gain is keeping the parsed metadata in memory.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentRegistry {
    components_tablet: TabletId,
    components: OrdMap<DeveloperDocumentId, ParsedDocument<ComponentMetadata>>,
}

impl ComponentRegistry {
    #[fastrace::trace]
    pub fn bootstrap(
        table_mapping: &TableMapping,
        component_docs: Vec<ParsedDocument<ComponentMetadata>>,
    ) -> anyhow::Result<Self> {
        let components_tablet = table_mapping
            .namespace(TableNamespace::Global)
            .id(&COMPONENTS_TABLE)?
            .tablet_id;
        let components: OrdMap<_, _> = component_docs
            .into_iter()
            .map(|component| (component.developer_id(), component))
            .collect();
        Ok(Self {
            components_tablet,
            components,
        })
    }

    pub(crate) fn update(
        &mut self,
        table_mapping: &TableMapping,
        id: ResolvedDocumentId,
        old_doc: Option<&ResolvedDocument>,
        new_doc: Option<&ResolvedDocument>,
    ) -> anyhow::Result<()> {
        self.begin_update(table_mapping, id, old_doc, new_doc)?
            .apply();
        Ok(())
    }

    pub(crate) fn begin_update<'a>(
        &'a mut self,
        table_mapping: &TableMapping,
        id: ResolvedDocumentId,
        old_doc: Option<&ResolvedDocument>,
        new_doc: Option<&ResolvedDocument>,
    ) -> anyhow::Result<Update<'a>> {
        let mut component_update = None;
        if table_mapping
            .namespace(TableNamespace::Global)
            .tablet_matches_name(id.tablet_id, &COMPONENTS_TABLE)
        {
            let old_component = match old_doc {
                None => None,
                Some(old_doc) => Some(old_doc.parse()?),
            };
            anyhow::ensure!(self.components.get(&id.developer_id) == old_component.as_ref());
            let new_component = match new_doc {
                None => None,
                Some(new_doc) => Some(new_doc.parse()?),
            };
            component_update = Some(ComponentUpdate {
                old_component,
                new_component,
            });
        }
        Ok(Update {
            registry: self,
            update: component_update,
        })
    }

    pub fn get_component_path(
        &self,
        mut component_id: ComponentId,
        reads: &mut TransactionReadSet,
    ) -> Option<ComponentPath> {
        let mut path = Vec::new();
        while let ComponentId::Child(internal_id) = component_id {
            let component_doc = self.get_component(internal_id, reads)?;
            // TODO: consider returning None if unmounted.
            component_id = match &component_doc.component_type {
                ComponentType::App => ComponentId::Root,
                ComponentType::ChildComponent { parent, name, .. } => {
                    path.push(name.clone());
                    ComponentId::Child(*parent)
                },
            };
        }
        path.reverse();
        Some(ComponentPath::from(path))
    }

    pub fn must_component_path(
        &self,
        component_id: ComponentId,
        reads: &mut TransactionReadSet,
    ) -> anyhow::Result<ComponentPath> {
        self.get_component_path(component_id, reads)
            .with_context(|| format!("Component {component_id:?} not found"))
    }

    pub fn all_component_paths(
        &self,
        reads: &mut TransactionReadSet,
    ) -> BTreeMap<ComponentId, ComponentPath> {
        reads.record_indexed_derived(
            TabletIndexName::by_id(self.components_tablet),
            IndexedFields::by_id(),
            Interval::all(),
        );
        let mut paths = BTreeMap::new();
        for id in self.components.keys() {
            let path = self.get_component_path(ComponentId::Child(*id), reads);
            if let Some(path) = path {
                if path.is_root() {
                    paths.insert(ComponentId::Root, path);
                } else {
                    paths.insert(ComponentId::Child(*id), path);
                }
            }
        }
        // In case the component doesn't exist, we still want to return the root path.
        paths.insert(ComponentId::Root, ComponentPath::root());
        paths
    }

    pub fn component_path_to_ids(
        &self,
        path: &ComponentPath,
        reads: &mut TransactionReadSet,
    ) -> anyhow::Result<Option<(ComponentDefinitionId, ComponentId)>> {
        if path.is_root() {
            Ok(Some((ComponentDefinitionId::Root, ComponentId::Root)))
        } else {
            let Some(component_metadata) = self.resolve_path(path, reads)? else {
                return Ok(None);
            };
            Ok(Some((
                ComponentDefinitionId::Child(component_metadata.definition_id),
                ComponentId::Child(component_metadata.id().into()),
            )))
        }
    }

    pub fn resolve_path(
        &self,
        path: &ComponentPath,
        reads: &mut TransactionReadSet,
    ) -> anyhow::Result<Option<ParsedDocument<ComponentMetadata>>> {
        let mut component_doc = match self.root_component(reads)? {
            Some(doc) => doc,
            None => return Ok(None),
        };
        for name in path.iter() {
            component_doc = match self
                .component_in_parent(Some((component_doc.id().into(), name.clone())), reads)?
            {
                Some(doc) => doc,
                None => return Ok(None),
            };
        }
        Ok(Some(component_doc))
    }

    pub fn root_component(
        &self,
        reads: &mut TransactionReadSet,
    ) -> anyhow::Result<Option<ParsedDocument<ComponentMetadata>>> {
        self.component_in_parent(None, reads)
    }

    pub fn component_children(
        &self,
        parent_id: DeveloperDocumentId,
        reads: &mut TransactionReadSet,
    ) -> anyhow::Result<Vec<ParsedDocument<ComponentMetadata>>> {
        let interval =
            Interval::prefix(values_to_bytes(&[Some(val!(parent_id.to_string()))]).into());
        reads.record_indexed_derived(
            TabletIndexName::new(
                self.components_tablet,
                COMPONENTS_BY_PARENT_INDEX.descriptor().clone(),
            )?,
            vec![PARENT_FIELD.clone(), NAME_FIELD.clone()].try_into()?,
            interval,
        );
        let child_ids = self
            .components
            .iter()
            .filter_map(|(_, doc)| {
                let (component_parent_id, _) = doc.parent_and_name()?;
                if component_parent_id == parent_id {
                    Some(doc.clone())
                } else {
                    None
                }
            })
            .collect();
        Ok(child_ids)
    }

    pub fn component_in_parent(
        &self,
        parent_and_name: Option<(DeveloperDocumentId, ComponentName)>,
        reads: &mut TransactionReadSet,
    ) -> anyhow::Result<Option<ParsedDocument<ComponentMetadata>>> {
        let interval = Interval::prefix(
            values_to_bytes(&match &parent_and_name {
                Some((parent, name)) => {
                    vec![Some(val!(parent.to_string())), Some(val!(name.to_string()))]
                },
                None => vec![Some(val!(null))],
            })
            .into(),
        );
        reads.record_indexed_derived(
            TabletIndexName::new(
                self.components_tablet,
                COMPONENTS_BY_PARENT_INDEX.descriptor().clone(),
            )?,
            vec![PARENT_FIELD.clone(), NAME_FIELD.clone()].try_into()?,
            interval,
        );
        let component = self
            .components
            .iter()
            .find(|(_, doc)| match (&parent_and_name, &doc.component_type) {
                (Some((p, n)), ComponentType::ChildComponent { parent, name, .. })
                    if p == parent && n == name =>
                {
                    true
                },
                (None, ComponentType::App) => true,
                _ => false,
            })
            .map(|(_, doc)| doc.clone());
        Ok(component)
    }

    fn get_component(
        &self,
        id: DeveloperDocumentId,
        reads: &mut TransactionReadSet,
    ) -> Option<&ParsedDocument<ComponentMetadata>> {
        let index_key = IndexKey::new(vec![], id).to_bytes().into();
        reads.record_indexed_derived(
            TabletIndexName::by_id(self.components_tablet),
            IndexedFields::by_id(),
            Interval::prefix(index_key),
        );
        self.components.get(&id)
    }
}

pub(crate) struct ComponentUpdate {
    pub old_component: Option<ParsedDocument<ComponentMetadata>>,
    pub new_component: Option<ParsedDocument<ComponentMetadata>>,
}

pub(crate) struct Update<'a> {
    registry: &'a mut ComponentRegistry,
    update: Option<ComponentUpdate>,
}

impl Update<'_> {
    pub(crate) fn apply(self) {
        if let Some(update) = self.update {
            let components = &mut self.registry.components;
            if let Some(old_component) = update.old_component {
                components.remove(&old_component.developer_id());
            }
            if let Some(new_component) = update.new_component {
                components.insert(new_component.developer_id(), new_component);
            }
        }
    }
}
