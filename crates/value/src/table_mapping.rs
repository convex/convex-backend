use std::collections::BTreeMap;

use anyhow::Context;
use errors::ErrorMetadata;
use imbl::OrdMap;
use serde::Serialize;

use crate::{
    DeveloperDocumentId,
    TableName,
    TableNumber,
    TabletId,
    TabletIdAndTableNumber,
};

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Copy)]
pub enum TableNamespace {
    /// For tables that have a single global namespace, e.g. _tables, _index,
    /// _db.
    /// Also for tables in the root component, including system tables.
    Global,

    /// Some tables are namespaced by component, like user tables,
    /// _file_storage, etc.
    ByComponent(DeveloperDocumentId),
}

impl TableNamespace {
    /// Use this to make it clear that a table pertains to the root component.
    /// It doesn't extend between components like a plain Global.
    /// This is useful for code searching.
    pub const fn root_component() -> Self {
        Self::Global
    }

    /// Namespace that should be replaced with RootComponent or ByComponent,
    /// but for now uses Global. For easy searching.
    #[allow(non_snake_case)]
    pub const fn by_component_TODO() -> Self {
        Self::Global
    }
}

// This TableMapping contains the mapping between TableNames and
// TabletIdAndTableNumber. This only includes active tables and hidden tables
// (i.e. not deleted tables).
// Use is_active to determine if a table is active.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableMapping {
    /// Maps from tablet to number and name exist for all tablets.
    tablet_to_table: OrdMap<TabletId, (TableNamespace, TableNumber, TableName)>,

    /// Maps from number and name only exist for active tablets,
    /// because other tablets might have conflicting numbers/names.
    table_name_to_canonical_tablet: OrdMap<TableNamespace, OrdMap<TableName, TabletId>>,
    table_number_to_canonical_tablet: OrdMap<TableNamespace, OrdMap<TableNumber, TabletId>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamespacedTableMapping {
    namespace: TableNamespace,

    /// Maps from tablet to number and name exist for all tablets.
    tablet_to_table: OrdMap<TabletId, (TableNamespace, TableNumber, TableName)>,

    /// Maps from number and name only exist for active tablets,
    /// because other tablets might have conflicting numbers/names.
    table_name_to_canonical_tablet: OrdMap<TableName, TabletId>,
    table_number_to_canonical_tablet: OrdMap<TableNumber, TabletId>,
}

impl TableMapping {
    pub fn new() -> Self {
        Self {
            tablet_to_table: Default::default(),
            table_name_to_canonical_tablet: Default::default(),
            table_number_to_canonical_tablet: Default::default(),
        }
    }

    pub fn iter(
        &self,
    ) -> impl Iterator<Item = (TabletId, TableNamespace, TableNumber, &TableName)> {
        self.tablet_to_table
            .iter()
            .map(|(table_id, (namespace, table_number, table_name))| {
                (*table_id, *namespace, *table_number, table_name)
            })
    }

    pub fn iter_active_user_tables(
        &self,
    ) -> impl Iterator<Item = (TabletId, TableNamespace, TableNumber, &TableName)> {
        self.tablet_to_table
            .iter()
            .filter(|(tablet_id, (_, _, name))| !name.is_system() && self.is_active(**tablet_id))
            .map(|(table_id, (namespace, table_number, table_name))| {
                (*table_id, *namespace, *table_number, table_name)
            })
    }

    pub fn iter_active_namespaces(
        &self,
    ) -> impl Iterator<Item = (&TableNamespace, &OrdMap<TableName, TabletId>)> {
        self.table_name_to_canonical_tablet.iter()
    }

    pub fn namespaces_for_name(&self, name: &TableName) -> Vec<TableNamespace> {
        self.iter()
            .filter(|(_, _, _, table_name)| *table_name == name)
            .map(|(_, namespace, ..)| namespace)
            .collect()
    }

    pub fn insert(
        &mut self,
        tablet_id: TabletId,
        namespace: TableNamespace,
        table_number: TableNumber,
        name: TableName,
    ) {
        self.insert_tablet(tablet_id, namespace, table_number, name.clone());
        self.table_name_to_canonical_tablet
            .entry(namespace)
            .or_default()
            .insert(name, tablet_id);
        self.table_number_to_canonical_tablet
            .entry(namespace)
            .or_default()
            .insert(table_number, tablet_id);
    }

    /// A tablet has mappings ID -> number, and ID -> name but not necessarily
    /// the reverse mappings.
    pub fn insert_tablet(
        &mut self,
        tablet_id: TabletId,
        namespace: TableNamespace,
        table_number: TableNumber,
        name: TableName,
    ) {
        self.tablet_to_table
            .insert(tablet_id, (namespace, table_number, name));
    }

    /// Removes tablet for active table.
    pub fn remove(&mut self, tablet_id: TabletId) {
        let Some((namespace, number, name)) = self.tablet_to_table.remove(&tablet_id) else {
            panic!("{tablet_id} does not exist");
        };
        if self
            .table_name_to_canonical_tablet
            .get(&namespace)
            .and_then(|m| m.get(&name))
            == Some(&tablet_id)
        {
            self.table_name_to_canonical_tablet
                .entry(namespace)
                .or_default()
                .remove(&name);
        }
        if self
            .table_number_to_canonical_tablet
            .get(&namespace)
            .and_then(|m| m.get(&number))
            == Some(&tablet_id)
        {
            self.table_number_to_canonical_tablet
                .entry(namespace)
                .or_default()
                .remove(&number);
        }
    }

    pub fn id_exists(&self, id: TabletId) -> bool {
        self.tablet_to_table.contains_key(&id)
    }

    pub fn tablet_id_exists(&self, id: TabletId) -> bool {
        self.tablet_to_table.contains_key(&id)
    }

    pub fn tablet_name(&self, id: TabletId) -> anyhow::Result<TableName> {
        self.tablet_to_table
            .get(&id)
            .map(|(_, _, name)| name.clone())
            .with_context(|| format!("cannot find table {id:?}"))
    }

    pub fn tablet_number(&self, id: TabletId) -> anyhow::Result<TableNumber> {
        self.tablet_to_table
            .get(&id)
            .map(|(_, number, ..)| *number)
            .with_context(|| format!("cannot find table {id:?}"))
    }

    pub fn tablet_namespace(&self, id: TabletId) -> anyhow::Result<TableNamespace> {
        self.tablet_to_table
            .get(&id)
            .map(|(namespace, ..)| *namespace)
            .with_context(|| format!("cannot find table {id:?}"))
    }

    pub fn get_table_metadata(
        &self,
        id: TabletId,
    ) -> anyhow::Result<&(TableNamespace, TableNumber, TableName)> {
        self.tablet_to_table
            .get(&id)
            .with_context(|| format!("cannot find table {id:?}"))
    }

    pub fn tablet_to_name(&self) -> impl Fn(TabletId) -> anyhow::Result<TableName> + '_ {
        |id| self.tablet_name(id)
    }

    /// Assuming all system tables are in the table mapping,
    /// does a table id correspond to a system table?
    pub fn is_system_tablet(&self, tablet_id: TabletId) -> bool {
        match self.tablet_to_table.get(&tablet_id) {
            Some((_, _, t)) => t.is_system(),
            None => false,
        }
    }

    pub fn update(&mut self, other: TableMapping) {
        for (table_id, (namespace, table_number, table_name)) in other.tablet_to_table.into_iter() {
            self.insert(table_id, namespace, table_number, table_name);
        }
    }

    pub fn is_active(&self, tablet_id: TabletId) -> bool {
        let Some((namespace, table_number, _)) = self.tablet_to_table.get(&tablet_id) else {
            return false;
        };
        let Some(active_tablet_id) = self
            .table_number_to_canonical_tablet
            .get(namespace)
            .and_then(|m| m.get(table_number))
        else {
            return false;
        };
        tablet_id == *active_tablet_id
    }

    pub fn len(&self) -> usize {
        self.tablet_to_table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tablet_to_table.is_empty()
    }

    pub fn namespace(&self, namespace: TableNamespace) -> NamespacedTableMapping {
        NamespacedTableMapping {
            namespace,
            tablet_to_table: self.tablet_to_table.clone(),
            table_name_to_canonical_tablet: self
                .table_name_to_canonical_tablet
                .get(&namespace)
                .cloned()
                .unwrap_or_default(),
            table_number_to_canonical_tablet: self
                .table_number_to_canonical_tablet
                .get(&namespace)
                .cloned()
                .unwrap_or_default(),
        }
    }
}

impl NamespacedTableMapping {
    pub fn namespace(&self) -> TableNamespace {
        self.namespace
    }

    pub fn iter(&self) -> impl Iterator<Item = (TabletId, TableNumber, &TableName)> {
        self.tablet_to_table
            .iter()
            .filter(|(_, (namespace, ..))| namespace == &self.namespace)
            .map(|(table_id, (_, table_number, table_name))| (*table_id, *table_number, table_name))
    }

    pub fn iter_active_user_tables(
        &self,
    ) -> impl Iterator<Item = (TabletId, TableNumber, &TableName)> {
        self.iter()
            .filter(|(tablet_id, _, name)| !name.is_system() && self.is_active(*tablet_id))
    }

    pub fn id(&self, name: &TableName) -> anyhow::Result<TabletIdAndTableNumber> {
        self.id_and_number_if_exists(name)
            .ok_or_else(|| anyhow::anyhow!("cannot find table {name:?}"))
    }

    pub fn id_if_exists(&self, name: &TableName) -> Option<TabletId> {
        self.table_name_to_canonical_tablet.get(name).cloned()
    }

    pub fn id_and_number_if_exists(&self, name: &TableName) -> Option<TabletIdAndTableNumber> {
        let table_id = self.id_if_exists(name)?;
        let (_, number, _) = self.tablet_to_table.get(&table_id)?;
        Some(TabletIdAndTableNumber {
            tablet_id: table_id,
            table_number: *number,
        })
    }

    pub fn id_exists(&self, id: TabletId) -> bool {
        self.tablet_to_table.contains_key(&id)
    }

    pub fn tablet_id_exists(&self, id: TabletId) -> bool {
        self.tablet_to_table.contains_key(&id)
    }

    pub fn table_number_exists(&self) -> impl Fn(TableNumber) -> bool + '_ {
        |n| self.table_number_to_canonical_tablet.contains_key(&n)
    }

    pub fn name_by_number_if_exists(&self, number: TableNumber) -> Option<&TableName> {
        self.table_number_to_canonical_tablet
            .get(&number)
            .and_then(|id| self.tablet_to_table.get(id))
            .map(|(_, _, name)| name)
    }

    pub fn name_exists(&self, name: &TableName) -> bool {
        self.table_name_to_canonical_tablet.contains_key(name)
    }

    pub fn name_to_id(&self) -> impl Fn(TableName) -> anyhow::Result<TabletIdAndTableNumber> + '_ {
        |name| self.id(&name)
    }

    pub fn name_to_tablet(&self) -> impl Fn(TableName) -> anyhow::Result<TabletId> + '_ {
        |name| self.id(&name).map(|id| id.tablet_id)
    }

    pub fn tablet_name(&self, id: TabletId) -> anyhow::Result<TableName> {
        self.tablet_to_table
            .get(&id)
            .map(|(_, _, name)| name.clone())
            .ok_or_else(|| anyhow::anyhow!("cannot find table {id:?}"))
    }

    pub fn tablet_to_name(&self) -> impl Fn(TabletId) -> anyhow::Result<TableName> + '_ {
        |id| self.tablet_name(id)
    }

    /// When the user inputs a TableName and we don't know whether it exists,
    /// throw a developer error if it doesn't exist.
    pub fn name_to_id_user_input(
        &self,
    ) -> impl Fn(TableName) -> anyhow::Result<TabletIdAndTableNumber> + '_ {
        |name| {
            let Some(id) = self.id_and_number_if_exists(&name) else {
                anyhow::bail!(table_does_not_exist(&name));
            };
            Ok(id)
        }
    }

    /// When the user inputs a TableName and we don't know whether it exists,
    /// throw a developer error if it doesn't exist.
    pub fn name_to_number_user_input(
        &self,
    ) -> impl Fn(TableName) -> anyhow::Result<TableNumber> + '_ {
        |name| {
            let table_id = self.name_to_id_user_input()(name)?;
            Ok(table_id.table_number)
        }
    }

    /// Assuming all system tables are in the table mapping,
    /// does a table id correspond to a system table?
    pub fn is_system_tablet(&self, tablet_id: TabletId) -> bool {
        match self.tablet_to_table.get(&tablet_id) {
            Some((_, _, t)) => t.is_system(),
            None => false,
        }
    }

    pub fn is_active(&self, tablet_id: TabletId) -> bool {
        let Some((_, table_number, _)) = self.tablet_to_table.get(&tablet_id) else {
            return false;
        };
        let Some(active_tablet_id) = self.table_number_to_canonical_tablet.get(table_number) else {
            return false;
        };
        tablet_id == *active_tablet_id
    }

    pub fn tablet_matches_name(&self, tablet_id: TabletId, name: &TableName) -> bool {
        match self.tablet_to_table.get(&tablet_id) {
            Some((_, _, table_name)) => name == table_name,
            None => false,
        }
    }

    pub fn tablet_number(&self, id: TabletId) -> anyhow::Result<TableNumber> {
        self.tablet_to_table
            .get(&id)
            .map(|(_, number, ..)| *number)
            .ok_or_else(|| anyhow::anyhow!("cannot find table {id:?}"))
    }

    pub fn number_to_name(&self) -> impl Fn(TableNumber) -> anyhow::Result<TableName> + '_ {
        |table_number| {
            self.table_number_to_canonical_tablet
                .get(&table_number)
                .and_then(|id| self.tablet_to_table.get(id))
                .map(|(_, _, name)| name.clone())
                .ok_or_else(|| anyhow::anyhow!("cannot find table {table_number:?}"))
        }
    }

    pub fn number_to_tablet(&self) -> impl Fn(TableNumber) -> anyhow::Result<TabletId> + '_ {
        |table_number| {
            self.table_number_to_canonical_tablet
                .get(&table_number)
                .ok_or_else(|| anyhow::anyhow!("cannot find table with id {table_number}"))
                .copied()
        }
    }

    pub fn to_value(self, with_system_tables: bool) -> TableMappingValue {
        TableMappingValue(
            self.iter()
                .filter(|(tablet_id, ..)| self.is_active(*tablet_id))
                .filter(|(_, _, name)| with_system_tables || !name.is_system())
                .map(|(_, number, name)| (number, name.clone()))
                .collect(),
        )
    }
}

fn table_does_not_exist(table: &TableName) -> ErrorMetadata {
    ErrorMetadata::bad_request("TableDoesNotExist", format!("Table '{table}' not found"))
}

/// The table mapping that is sent to the dashboard through the
/// `getTableMapping` operation. It omits system tables.
#[derive(Serialize)]
pub struct TableMappingValue(BTreeMap<TableNumber, TableName>);
