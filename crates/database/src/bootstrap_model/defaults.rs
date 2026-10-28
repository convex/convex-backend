//! Default state to initialize the database with.

use std::{
    collections::BTreeMap,
    sync::LazyLock,
};

use common::{
    bootstrap_model::{
        index::INDEX_TABLE,
        tables::TABLES_TABLE,
    },
    types::{
        IndexId,
        TableName,
    },
    value::{
        TableMapping,
        TabletIdAndTableNumber,
    },
};
use maplit::btreemap;
use value::{
    DeveloperDocumentId,
    ResolvedDocumentId,
    TableNamespace,
    TableNumber,
    TabletId,
};

use crate::{
    bootstrap_model::{
        components::{
            definition::COMPONENT_DEFINITIONS_TABLE,
            COMPONENTS_TABLE,
        },
        index::IndexTable,
        index_backfills::IndexBackfillTable,
        index_workers::IndexWorkerMetadataTable,
        next_persistence_index_id::NextPersistenceIndexIdTable,
        schema::SchemasTable,
        schema_validation_progress::{
            SchemaValidationProgressTable,
            SCHEMA_VALIDATION_PROGRESS_TABLE,
        },
        table::TablesTable,
    },
    system_tables::ErasedSystemTable,
    ComponentDefinitionsTable,
    ComponentsTable,
    SchemaValidationTable,
    INDEX_BACKFILLS_TABLE,
    INDEX_WORKER_METADATA_TABLE,
    NEXT_PERSISTENCE_INDEX_ID_TABLE,
    NUM_RESERVED_LEGACY_TABLE_NUMBERS,
    SCHEMAS_TABLE,
    SCHEMA_VALIDATIONS_TABLE,
};

pub fn bootstrap_system_tables() -> Vec<&'static dyn ErasedSystemTable> {
    vec![
        &TablesTable,
        &IndexTable,
        &SchemasTable,
        &IndexBackfillTable,
        &IndexWorkerMetadataTable,
        &NextPersistenceIndexIdTable,
        &ComponentDefinitionsTable,
        &ComponentsTable,
        &SchemaValidationProgressTable,
        &SchemaValidationTable,
    ]
}

pub static DEFAULT_BOOTSTRAP_TABLE_NUMBERS: LazyLock<BTreeMap<TableName, TableNumber>> =
    LazyLock::new(|| {
        let tn = |tn| TableNumber::try_from(NUM_RESERVED_LEGACY_TABLE_NUMBERS + tn).unwrap();
        btreemap! {
            TABLES_TABLE.clone() => tn(1),
            INDEX_TABLE.clone() => tn(2),
            SCHEMAS_TABLE.clone() => tn(20),
            INDEX_WORKER_METADATA_TABLE.clone() => tn(30),
            COMPONENT_DEFINITIONS_TABLE.clone() => tn(31),
            COMPONENTS_TABLE.clone() => tn(32),
            INDEX_BACKFILLS_TABLE.clone() => tn(36),
            SCHEMA_VALIDATION_PROGRESS_TABLE.clone() => tn(37),
            NEXT_PERSISTENCE_INDEX_ID_TABLE.clone() => tn(42),
            SCHEMA_VALIDATIONS_TABLE.clone() => tn(43)
            // To add a bootstrap system table, first add to model/src/lib and then
            // replicate that table number to here.
        }
    });

/// Contains the table_id and index_id that never change after initializing the
/// backend database. We prefer to pass this around instead of the full
/// TableMapping so don't worry about passing around a reference to the
/// "authoritative" table mapping.
#[derive(Clone, Copy)]
pub struct BootstrapTableIds {
    pub tables_id: TabletIdAndTableNumber,
    pub index_id: TabletIdAndTableNumber,
}

impl BootstrapTableIds {
    pub fn new(table_mapping: &TableMapping) -> Self {
        let tables_id = table_mapping
            .namespace(TableNamespace::Global)
            .id(&TABLES_TABLE)
            .expect("_tables should exist");
        let index_id = table_mapping
            .namespace(TableNamespace::Global)
            .id(&INDEX_TABLE)
            .expect("_index should exist");
        Self {
            tables_id,
            index_id,
        }
    }

    pub fn table_resolved_doc_id(&self, table_id: TabletId) -> ResolvedDocumentId {
        ResolvedDocumentId::new(
            self.tables_id.tablet_id,
            DeveloperDocumentId::new(self.tables_id.table_number, table_id.0),
        )
    }

    pub fn index_resolved_doc_id(&self, index_id: IndexId) -> ResolvedDocumentId {
        ResolvedDocumentId::new(
            self.index_id.tablet_id,
            DeveloperDocumentId::new(self.index_id.table_number, index_id.0),
        )
    }

    pub fn is_index_table(&self, tablet_id: TabletId) -> bool {
        self.index_id.tablet_id == tablet_id
    }

    pub fn is_tables_table(&self, tablet_id: TabletId) -> bool {
        self.tables_id.tablet_id == tablet_id
    }
}
