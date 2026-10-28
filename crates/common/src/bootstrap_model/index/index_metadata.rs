use std::collections::BTreeSet;

use serde::{
    Deserialize,
    Serialize,
};
use sync_types::Timestamp;
use value::{
    codegen_convex_serialization,
    ConvexValue,
    FieldPath,
    TableName,
    TabletId,
};

use super::{
    database_index::{
        DatabaseIndexBackfillState,
        DatabaseIndexSpec,
        DatabaseIndexState,
        IndexedFields,
    },
    index_config::SerializedIndexConfig,
    vector_index::{
        VectorDimensions,
        VectorIndexBackfillState,
        VectorIndexSpec,
        VectorIndexState,
    },
    IndexConfig,
};
use crate::{
    bootstrap_model::index::text_index::{
        TextIndexBackfillState,
        TextIndexSpec,
        TextIndexState,
    },
    document::{
        ParseDocument,
        ParsedDocument,
        ResolvedDocument,
    },
    types::{
        GenericIndexName,
        IndexDescriptor,
        IndexRef,
        IndexTableIdentifier,
        PersistenceIndexId,
    },
};

pub type TabletIndexMetadata = IndexMetadata<TabletId>;
pub type DeveloperIndexMetadata = IndexMetadata<TableName>;

impl TryFrom<&ParsedDocument<TabletIndexMetadata>> for IndexRef {
    type Error = anyhow::Error;

    /// Only database indexes have rows in persistence, so only they have a
    /// ref.
    fn try_from(metadata: &ParsedDocument<TabletIndexMetadata>) -> anyhow::Result<Self> {
        let persistence_index_id = match &metadata.config {
            IndexConfig::Database {
                persistence_index_id,
                ..
            } => *persistence_index_id,
            IndexConfig::Text { .. } => {
                anyhow::bail!(
                    "{} is a text index, which has no persistence rows",
                    metadata.name
                )
            },
            IndexConfig::Vector { .. } => anyhow::bail!(
                "{} is a vector index, which has no persistence rows",
                metadata.name
            ),
        };
        Ok(Self::from_parts(
            metadata.id().internal_id().into(),
            persistence_index_id,
        ))
    }
}

/// In-memory representation of an index's metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexMetadata<T: IndexTableIdentifier> {
    /// Unique name for the index.
    pub name: GenericIndexName<T>,

    /// Configuration that depends on the type of index.
    pub config: IndexConfig,
}

impl<T: IndexTableIdentifier> IndexMetadata<T> {
    pub fn new_backfilling(
        index_created_lower_bound: Timestamp,
        name: GenericIndexName<T>,
        fields: IndexedFields,
    ) -> Self {
        Self::_new_backfilling(index_created_lower_bound, name, fields, false)
    }

    fn _new_backfilling(
        index_created_lower_bound: Timestamp,
        name: GenericIndexName<T>,
        fields: IndexedFields,
        staged: bool,
    ) -> Self {
        Self {
            name,
            config: IndexConfig::Database {
                spec: DatabaseIndexSpec { fields },
                on_disk_state: DatabaseIndexState::Backfilling(DatabaseIndexBackfillState {
                    index_created_lower_bound,
                    retention_started: false,
                    staged,
                }),
                persistence_index_id: None,
            },
        }
    }

    pub fn new_staged_backfilling(
        index_created_lower_bound: Timestamp,
        name: GenericIndexName<T>,
        fields: IndexedFields,
    ) -> Self {
        Self::_new_backfilling(index_created_lower_bound, name, fields, true)
    }

    pub fn new_backfilling_text_index(
        name: GenericIndexName<T>,
        search_field: FieldPath,
        filter_fields: BTreeSet<FieldPath>,
    ) -> Self {
        Self::new_text_index(
            name,
            TextIndexSpec {
                search_field,
                filter_fields,
            },
            TextIndexState::Backfilling(TextIndexBackfillState::new(false)),
        )
    }

    pub fn new_staged_backfilling_text_index(
        name: GenericIndexName<T>,
        search_field: FieldPath,
        filter_fields: BTreeSet<FieldPath>,
    ) -> Self {
        Self::new_text_index(
            name,
            TextIndexSpec {
                search_field,
                filter_fields,
            },
            TextIndexState::Backfilling(TextIndexBackfillState::new(true)),
        )
    }

    pub fn new_backfilling_vector_index(
        name: GenericIndexName<T>,
        vector_field: FieldPath,
        dimensions: VectorDimensions,
        filter_fields: BTreeSet<FieldPath>,
    ) -> Self {
        Self {
            name,
            config: IndexConfig::Vector {
                spec: VectorIndexSpec {
                    dimensions,
                    vector_field,
                    filter_fields,
                },
                on_disk_state: VectorIndexState::Backfilling(VectorIndexBackfillState::new(false)),
            },
        }
    }

    pub fn new_staged_backfilling_vector_index(
        name: GenericIndexName<T>,
        vector_field: FieldPath,
        dimensions: VectorDimensions,
        filter_fields: BTreeSet<FieldPath>,
    ) -> Self {
        Self {
            name,
            config: IndexConfig::Vector {
                spec: VectorIndexSpec {
                    dimensions,
                    vector_field,
                    filter_fields,
                },
                on_disk_state: VectorIndexState::Backfilling(VectorIndexBackfillState::new(true)),
            },
        }
    }

    pub fn new_text_index(
        name: GenericIndexName<T>,
        spec: TextIndexSpec,
        on_disk_state: TextIndexState,
    ) -> Self {
        Self {
            name,
            config: IndexConfig::Text {
                spec,
                on_disk_state,
            },
        }
    }

    pub fn new_enabled(name: GenericIndexName<T>, fields: IndexedFields) -> Self {
        Self {
            name,
            config: IndexConfig::Database {
                spec: DatabaseIndexSpec { fields },
                on_disk_state: DatabaseIndexState::Enabled,
                persistence_index_id: None,
            },
        }
    }

    pub fn assign_persistence_index_id(&mut self, persistence_index_id: PersistenceIndexId) {
        let IndexConfig::Database {
            persistence_index_id: current_id,
            ..
        } = &mut self.config
        else {
            panic!("persistence index IDs only apply to database indexes")
        };
        assert!(
            current_id.replace(persistence_index_id).is_none(),
            "persistence index ID is already assigned"
        );
    }

    pub fn is_database_index(&self) -> bool {
        matches!(self.config, IndexConfig::Database { .. })
    }

    pub fn is_text_index(&self) -> bool {
        matches!(self.config, IndexConfig::Text { .. })
    }

    pub fn is_vector_index(&self) -> bool {
        matches!(self.config, IndexConfig::Vector { .. })
    }

    pub fn map_table<U: IndexTableIdentifier>(
        self,
        f: &impl Fn(T) -> anyhow::Result<U>,
    ) -> anyhow::Result<IndexMetadata<U>> {
        Ok(IndexMetadata {
            name: self.name.map_table(f)?,
            config: self.config,
        })
    }
}

impl TabletIndexMetadata {
    pub fn from_document(document: ResolvedDocument) -> anyhow::Result<ParsedDocument<Self>> {
        document.parse()
    }
}

pub fn index_metadata_serialize_tablet_id(tablet_id: &TabletId) -> anyhow::Result<ConvexValue> {
    ConvexValue::try_from(tablet_id.to_string())
}

#[derive(Serialize, Deserialize)]
pub struct SerializedTabletIndexMetadata {
    table_id: String,
    descriptor: String,
    config: SerializedIndexConfig,
}

impl TryFrom<TabletIndexMetadata> for SerializedTabletIndexMetadata {
    type Error = anyhow::Error;

    fn try_from(m: TabletIndexMetadata) -> anyhow::Result<Self> {
        Ok(Self {
            // New format: write table_id(v5) + descriptor.
            table_id: m.name.table().to_string(),
            descriptor: m.name.descriptor().to_string(),
            config: m.config.try_into()?,
        })
    }
}

impl TryFrom<SerializedTabletIndexMetadata> for TabletIndexMetadata {
    type Error = anyhow::Error;

    fn try_from(s: SerializedTabletIndexMetadata) -> anyhow::Result<Self> {
        let table_id: TabletId = s.table_id.parse()?;
        let descriptor = IndexDescriptor::new(s.descriptor)?;
        let name = if descriptor.is_reserved() {
            GenericIndexName::new_reserved(table_id, descriptor)
        } else {
            GenericIndexName::new(table_id, descriptor)
        }?;
        let config = IndexConfig::try_from(s.config)?;
        Ok(Self { name, config })
    }
}

codegen_convex_serialization!(TabletIndexMetadata, SerializedTabletIndexMetadata);
