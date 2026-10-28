use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use imbl::OrdMap;
use semver::Version;
use value::{
    DeveloperDocumentId,
    NamespacedTableMapping,
    ResolvedDocumentId,
    TableMapping,
    TableName,
    TableNamespace,
    TableNumber,
};

use crate::{
    document::{
        DeveloperDocument,
        ResolvedDocument,
    },
    types::IndexName,
};

#[async_trait]
pub trait VirtualSystemDocMapper: Send + Sync {
    async fn system_to_virtual_doc(
        &self,
        tx: &mut dyn GetDocument,
        virtual_system_mapping: &VirtualSystemMapping,
        doc: ResolvedDocument,
        table_mapping: &TableMapping,
        version: Version,
    ) -> anyhow::Result<DeveloperDocument>;
}

/// This trait is used for dependency injection, exposing get_document
/// (implemented by `Transaction`) to convert system documents joined across
/// multiple system tables to virtual documents in `VirtualSystemDocMapper`.
#[async_trait]
pub trait GetDocument: Send + Sync {
    async fn get_document(
        &mut self,
        id: ResolvedDocumentId,
    ) -> anyhow::Result<Option<ResolvedDocument>>;
}

/// Captures the relationship between a system table and a virtual table.
///
/// Some virtual tables map 1-1 to system tables (e.g. _file_storage system
/// table has all the fields, indexes, and same table number as _storage virtual
/// table).
///
/// Other virtual tables require joining across different system tables to get a
/// document because the fields are split across different tables.
/// `_scheduled_functions` virtual table shares the same indexes and table
/// number as `_scheduled_jobs` system table, but the arguments field is stored
/// in `_scheduled_job_args` system table. In this taxonomy, the
/// `_scheduled_jobs` system table has `AssociatedVirtualTable::Primary` whereas
/// `_scheduled_job_args` has `AssociatedVirtualTable::Secondary`.
#[derive(Clone)]
pub enum AssociatedVirtualTable {
    /// This virtual table's _id field and indexes are backed by the system
    /// table. This virtual table shares the same TableNumber as the system
    /// table.
    Primary {
        virtual_table_name: TableName,
        virtual_to_system_indexes: OrdMap<IndexName, IndexName>,
        doc_mapper: Arc<dyn VirtualSystemDocMapper>,
    },
    /// This virtual table has some fields backed by the system table.
    Secondary(TableName),
}

impl PartialEq for AssociatedVirtualTable {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Primary {
                    virtual_table_name: l_table_name,
                    virtual_to_system_indexes: l_indexes,
                    doc_mapper: _,
                },
                Self::Primary {
                    virtual_table_name: r_table_name,
                    virtual_to_system_indexes: r_indexes,
                    doc_mapper: _,
                },
            ) => l_table_name == r_table_name && l_indexes == r_indexes,
            (Self::Secondary(l_table_name), Self::Secondary(r_table_name)) => {
                l_table_name == r_table_name
            },
            (Self::Primary { .. }, Self::Secondary(_))
            | (Self::Secondary(_), Self::Primary { .. }) => false,
        }
    }
}

impl AssociatedVirtualTable {
    pub fn virtual_table_name(&self) -> &TableName {
        match &self {
            Self::Primary {
                virtual_table_name, ..
            } => virtual_table_name,
            Self::Secondary(table_name) => table_name,
        }
    }

}

#[derive(Clone, Default)]
pub struct VirtualSystemMapping {
    system_to_associated_virtual_table: OrdMap<TableName, AssociatedVirtualTable>,
    virtual_to_primary_system_table: OrdMap<TableName, TableName>,
}

impl std::fmt::Debug for VirtualSystemMapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualSystemMapping")
            .field("virtual_to_system", &self.virtual_to_primary_system_table)
            .finish()
    }
}

impl PartialEq for VirtualSystemMapping {
    fn eq(&self, other: &Self) -> bool {
        self.virtual_to_primary_system_table == other.virtual_to_primary_system_table
            && self.system_to_associated_virtual_table == other.system_to_associated_virtual_table
    }
}

impl VirtualSystemMapping {
    pub fn add_table(
        &mut self,
        system: TableName,
        associated_virtual_table: AssociatedVirtualTable,
    ) {
        match &associated_virtual_table {
            AssociatedVirtualTable::Primary {
                virtual_table_name, ..
            } => {
                self.virtual_to_primary_system_table
                    .insert(virtual_table_name.clone(), system.clone());
            },
            AssociatedVirtualTable::Secondary(_) => {},
        }
        self.system_to_associated_virtual_table
            .insert(system, associated_virtual_table);
    }

    pub fn is_virtual_table(&self, table_name: &TableName) -> bool {
        self.virtual_to_primary_system_table
            .contains_key(table_name)
    }

    pub fn has_virtual_table(&self, table_name: &TableName) -> bool {
        self.virtual_to_primary_system_table
            .contains_key(table_name)
            || self
                .system_to_associated_virtual_table
                .contains_key(table_name)
    }

    pub fn virtual_to_system_index(
        &self,
        virtual_index_name: &IndexName,
    ) -> anyhow::Result<&IndexName> {
        let index = self
            .virtual_to_primary_system_table
            .get(virtual_index_name.table())
            .and_then(|primary_system_table| {
                self.system_to_associated_virtual_table
                    .get(primary_system_table)
            })
            .and_then(|t| match t {
                AssociatedVirtualTable::Primary {
                    virtual_to_system_indexes,
                    ..
                } => Some(virtual_to_system_indexes),
                AssociatedVirtualTable::Secondary(_) => None,
            })
            .and_then(|t| t.get(virtual_index_name));
        index.context(format!(
            "Could not find system index for virtual index {virtual_index_name}"
        ))
    }

    pub fn virtual_to_system_table(
        &self,
        virtual_table_name: &TableName,
    ) -> anyhow::Result<&TableName> {
        match self.virtual_to_primary_system_table.get(virtual_table_name) {
            Some(system_table) => Ok(system_table),
            None => {
                anyhow::bail!("Could not find system table for virtual table {virtual_table_name}")
            },
        }
    }

    /// Return the virtual table name associated with a system table iff the
    /// system table is the primary table backing the virtual table.
    pub fn primary_system_to_virtual_table(
        &self,
        system_table_name: &TableName,
    ) -> Option<&TableName> {
        self.system_to_associated_virtual_table
            .get(system_table_name)
            .and_then(|t| match t {
                AssociatedVirtualTable::Primary {
                    virtual_table_name, ..
                } => Some(virtual_table_name),
                AssociatedVirtualTable::Secondary(_table_name) => None,
            })
    }

    /// Return the virtual table name if this system table is associated with a
    /// virtual table, either as the primary table backing the virtual table or
    /// as a secondary table with some fields that contribute to the virtual
    /// table.
    pub fn associated_virtual_table_name(
        &self,
        system_table_name: &TableName,
    ) -> Option<&TableName> {
        self.system_to_associated_virtual_table
            .get(system_table_name)
            .map(|t| t.virtual_table_name())
    }

    // Return the doc mapper if the this system table is the primary table backing
    // a virtual table.
    // system_table_name -> (Fn (SystemDoc) -> VirtualDoc)
    pub fn system_to_virtual_doc_mapper(
        &self,
        system_table_name: &TableName,
    ) -> Option<&Arc<dyn VirtualSystemDocMapper>> {
        self.system_to_associated_virtual_table
            .get(system_table_name)
            .and_then(|t| match t {
                AssociatedVirtualTable::Primary { doc_mapper, .. } => Some(doc_mapper),
                AssociatedVirtualTable::Secondary(_) => None,
            })
    }

    // Converts a virtual table DeveloperDocumentId to the system table ResolvedId.
    pub fn virtual_id_v6_to_system_resolved_doc_id(
        &self,
        namespace: TableNamespace,
        virtual_id_v6: &DeveloperDocumentId,
        table_mapping: &TableMapping,
    ) -> anyhow::Result<ResolvedDocumentId> {
        let table_number = virtual_id_v6.table();
        let tablet_id = table_mapping.namespace(namespace).number_to_tablet()(table_number)
            .with_context(|| {
                format!("cannot find table with id {table_number} in {namespace:?}")
            })?;
        Ok(ResolvedDocumentId::new(tablet_id, *virtual_id_v6))
    }

    // Converts a system table ResolvedDocumentId to the equivalent virtual table
    // DeveloperDocumentId by mapping the TableName and using the same InternalId
    pub fn system_resolved_id_to_virtual_developer_id(
        &self,
        system_doc_id: ResolvedDocumentId,
    ) -> anyhow::Result<DeveloperDocumentId> {
        Ok(system_doc_id.developer_id)
    }
}

pub fn all_tables_number_to_name(
    table_mapping: &NamespacedTableMapping,
    virtual_system_mapping: &VirtualSystemMapping,
) -> impl Fn(TableNumber) -> anyhow::Result<TableName> + use<> {
    let table_mapping = table_mapping.clone();
    let virtual_system_mapping = virtual_system_mapping.clone();
    move |number| {
        let physical_name = table_mapping.number_to_name()(number)?;
        if let Some(virtual_name) =
            virtual_system_mapping.primary_system_to_virtual_table(&physical_name)
        {
            Ok(virtual_name.clone())
        } else {
            Ok(physical_name)
        }
    }
}

// Checks both virtual tables and tables to get the table name to number mapping
pub fn all_tables_name_to_number(
    namespace: TableNamespace,
    table_mapping: &TableMapping,
    virtual_system_mapping: &VirtualSystemMapping,
) -> impl Fn(TableName) -> anyhow::Result<TableNumber> + use<> {
    let table_mapping = table_mapping.clone();
    let virtual_system_mapping = virtual_system_mapping.clone();
    move |name| {
        let name = if let Some(physical_table) = virtual_system_mapping
            .virtual_to_primary_system_table
            .get(&name)
        {
            physical_table.clone()
        } else {
            name
        };
        table_mapping
            .namespace(namespace)
            .name_to_number_user_input()(name)
    }
}
