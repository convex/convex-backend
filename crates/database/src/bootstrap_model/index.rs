use std::collections::{
    BTreeMap,
    BTreeSet,
    HashMap,
};

use anyhow::Context;
use common::{
    bootstrap_model::index::{
        database_index::{
            DatabaseIndexSpec,
            DatabaseIndexState,
            IndexedFields,
        },
        index_validation_error,
        text_index::{
            TextIndexSpec,
            TextIndexState,
        },
        vector_index::{
            VectorIndexSpec,
            VectorIndexState,
        },
        DeveloperIndexMetadata,
        IndexConfig,
        IndexMetadata,
        TabletIndexMetadata,
        INDEX_TABLE,
    },
    document::ParsedDocument,
    interval::Interval,
    runtime::Runtime,
    schemas::{
        DatabaseSchema,
        TableDefinition,
        MAX_INDEXES_PER_TABLE,
    },
    types::{
        IndexDescriptor,
        IndexDiff,
        IndexName,
        IndexRef,
        StableIndexName,
        TableName,
        TabletIndexName,
    },
};
use errors::ErrorMetadata;
use indexing::{
    database_index_snapshot::index_not_a_database_index_error,
    index_registry::{
        index_not_found_error,
        Index,
    },
};
use value::{
    ResolvedDocumentId,
    TableMapping,
    TableNamespace,
    TabletId,
};

use crate::{
    patch_value,
    query::TableFilter,
    reads::TransactionReadSet,
    system_tables::{
        SystemIndex,
        SystemTable,
    },
    table_summary::table_summary_bootstrapping_error,
    transaction_index::TransactionIndex,
    unauthorized_error,
    SystemMetadataModel,
    TableModel,
    Transaction,
};

pub struct IndexTable;
impl SystemTable for IndexTable {
    type Metadata = TabletIndexMetadata;

    const TABLE_NAME: TableName = INDEX_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub struct IndexModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> IndexModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    /// Register given index.
    /// Indexes won't be backfilled and available for queries until after the
    /// transaction has committed.
    pub async fn add_application_index(
        &mut self,
        namespace: TableNamespace,
        index: IndexMetadata<TableName>,
    ) -> anyhow::Result<ResolvedDocumentId> {
        anyhow::ensure!(
            self.tx.identity().is_admin() || self.tx.identity().is_system(),
            unauthorized_error("add_index")
        );
        anyhow::ensure!(!index.name.is_system_owned(), "Can't change system indexes");
        let application_indexes = self.get_application_indexes(namespace).await?;
        // Indexes may exist in both a pending and an enabled state. If we're at or over
        // the index limit, we should still be able to add a new pending copy of
        // an enabled index with the expectation that the pending index will
        // replace the enabled index eventually. We must have other checks to
        // ensure we don't add multiple pending or enabled indexes, so
        // here we just verify we're not increasing the total number of indexes.
        let index_names_in_table = application_indexes
            .into_iter()
            .filter(|application_index| application_index.name.table() == index.name.table())
            .map(|index| index.into_value().name)
            .collect::<BTreeSet<_>>();
        anyhow::ensure!(
            index_names_in_table.contains(&index.name)
                || index_names_in_table.len() < MAX_INDEXES_PER_TABLE,
            index_validation_error::too_many_indexes(index.name.table(), MAX_INDEXES_PER_TABLE)
        );
        self._add_index(namespace, index).await
    }

    /// Add system index.
    /// Indexes won't be backfilled and available for queries until
    /// after the transaction has committed.
    pub async fn add_system_index(
        &mut self,
        namespace: TableNamespace,
        index: IndexMetadata<TableName>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.tx.identity().is_admin() || self.tx.identity().is_system(),
            unauthorized_error("add_system_index")
        );
        self._add_index(namespace, index).await?;
        Ok(())
    }

    async fn _add_index(
        &mut self,
        namespace: TableNamespace,
        index: IndexMetadata<TableName>,
    ) -> anyhow::Result<ResolvedDocumentId> {
        // Make sure the table exists before creating the index.
        TableModel::new(self.tx)
            .insert_table_metadata(namespace, index.name.table())
            .await?;
        let index: TabletIndexMetadata = index.map_table(
            &self
                .tx
                .table_mapping()
                .namespace(namespace)
                .name_to_tablet(),
        )?;
        SystemMetadataModel::new_global(self.tx)
            .insert_metadata(&INDEX_TABLE, index.try_into()?)
            .await
    }

    async fn enable_index(&mut self, backfilled_index: &TabletIndexMetadata) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.tx.identity().is_admin() || self.tx.identity().is_system(),
            unauthorized_error("enable_index")
        );
        anyhow::ensure!(
            !backfilled_index.name.is_by_id_or_creation_time(),
            "Can't change system indexes"
        );

        let mut doc: ParsedDocument<TabletIndexMetadata> = self
            .pending_resolved_index_metadata(&backfilled_index.name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Missing pending data for index: {:?}",
                    backfilled_index.name
                )
            })?;
        match doc.config {
            IndexConfig::Database {
                ref mut on_disk_state,
                ..
            } => match on_disk_state {
                DatabaseIndexState::Backfilling(_) | DatabaseIndexState::Enabled => {
                    anyhow::bail!(
                        "Expected backfilled index, but found: {:?} for {:?}",
                        on_disk_state,
                        backfilled_index.name.descriptor()
                    )
                },
                DatabaseIndexState::Backfilled { .. } => {
                    *on_disk_state = DatabaseIndexState::Enabled;
                },
            },
            IndexConfig::Text {
                ref mut on_disk_state,
                ..
            } => match on_disk_state {
                TextIndexState::Backfilled { snapshot, .. } => {
                    *on_disk_state = TextIndexState::SnapshottedAt(snapshot.clone());
                },
                TextIndexState::Backfilling { .. } | TextIndexState::SnapshottedAt(_) => {
                    anyhow::bail!(
                        "Expected backfilled index, but found: {on_disk_state:?} for {:?}",
                        backfilled_index.name.descriptor()
                    )
                },
            },
            IndexConfig::Vector {
                ref mut on_disk_state,
                ..
            } => match on_disk_state {
                VectorIndexState::Backfilled { snapshot, .. } => {
                    *on_disk_state = VectorIndexState::SnapshottedAt(snapshot.clone());
                },
                VectorIndexState::Backfilling(_) | VectorIndexState::SnapshottedAt(_) => {
                    anyhow::bail!(
                        "Expected backfilled index, but found: {on_disk_state:?} for {:?}",
                        backfilled_index.name.descriptor()
                    )
                },
            },
        };

        let id = doc.id();
        let table_id_metadata: TabletIndexMetadata = doc.into_value();
        SystemMetadataModel::new_global(self.tx)
            .replace(id, table_id_metadata.try_into()?)
            .await?;

        Ok(())
    }

    /// The old push flow split the index diff between indexes being added in
    /// the `prepare_schema` phase and indexes being deleted in the
    /// `apply_config` phase.
    ///
    /// This method merges the two halves to give a "full" index diff when the
    /// index changes have been prepared but not committed.
    #[fastrace::trace]
    pub async fn get_full_index_diff(
        &mut self,
        namespace: TableNamespace,
        next_schema: &Option<DatabaseSchema>,
    ) -> anyhow::Result<IndexDiff> {
        let empty = BTreeMap::new();
        let tables_in_schema = next_schema
            .as_ref()
            .map(|schema| &schema.tables)
            .unwrap_or(&empty);
        let mut index_diff: IndexDiff = self.get_index_diff(namespace, tables_in_schema).await?;
        anyhow::ensure!(index_diff.added.is_empty(), "Expected no new indexes");

        // Find all indexes that are being replaced by their pending variant to count as
        // added.
        index_diff.added = index_diff
            .identical
            .iter()
            .filter(|index| !index.config.is_enabled())
            .map(|doc| doc.clone().into_value())
            .collect();

        index_diff
            .identical
            .retain(|index| index.config.is_enabled());

        Ok(index_diff)
    }

    // This method assumes it's being called in apply_config, or at least after
    // indexes have been added and backfilled.
    #[fastrace::trace]
    pub async fn apply(
        &mut self,
        namespace: TableNamespace,
        next_schema: &Option<DatabaseSchema>,
    ) -> anyhow::Result<IndexDiff> {
        let empty = BTreeMap::new();
        let tables_in_schema = next_schema
            .as_ref()
            .map(|schema| &schema.tables)
            .unwrap_or(&empty);

        // Without a schema id, we cannot accurately determine the status of
        // indexes. So for legacy CLIs, we do nothing here and instead rely
        // on build_indexes / legacy_get_indexes to commit index changes.
        let index_diff = self
            .commit_indexes_for_schema(namespace, tables_in_schema)
            .await?;

        tracing::info!(
            "Committed indexes for {namespace:?}: (added {}. dropped {})",
            index_diff.added.len(),
            index_diff.dropped.len(),
        );
        Ok(index_diff)
    }

    pub async fn commit_indexes_for_schema(
        &mut self,
        namespace: TableNamespace,
        tables_in_schema: &BTreeMap<TableName, TableDefinition>,
    ) -> anyhow::Result<IndexDiff> {
        let index_diff: IndexDiff = self.get_index_diff(namespace, tables_in_schema).await?;

        let IndexDiff {
            added,
            identical,
            dropped,
            enabled,
            disabled,
        } = &index_diff;

        // New indexes should all have been added in prepare_schema, something has gone
        // wrong if we're trying to commit and realize there's a new index.
        anyhow::ensure!(
            added.is_empty(),
            "Trying to add new indexes when committing"
        );

        for index in dropped {
            self.drop_index(index.id()).await?;
        }

        // Added indexes should have backfilled via build_indexes
        let mut table_model = TableModel::new(self.tx);
        let indexes_to_enable = identical
            .iter()
            .filter(|index| !index.config.is_enabled() && !index.config.is_staged())
            .chain(enabled.iter())
            .cloned()
            .map(|doc| table_model.doc_table_name_to_id(namespace, doc))
            .collect::<anyhow::Result<Vec<_>>>()?;
        self.enable_backfilled_indexes(indexes_to_enable).await?;

        for index in disabled {
            self.disable_index(index.clone()).await?;
        }

        Ok(index_diff)
    }

    // Enables the given set of indexes if they're backfilled.
    pub async fn enable_backfilled_indexes(
        &mut self,
        indexes: Vec<ParsedDocument<TabletIndexMetadata>>,
    ) -> anyhow::Result<()> {
        for index in indexes {
            self.enable_index(&index).await?;
        }
        Ok(())
    }

    async fn disable_index(
        &mut self,
        doc: ParsedDocument<DeveloperIndexMetadata>,
    ) -> anyhow::Result<()> {
        tracing::info!("Disabling index: {doc:?}");
        let id = doc.id();
        let new_config = match doc.into_value().config {
            IndexConfig::Database {
                spec,
                on_disk_state,
                persistence_index_id,
            } => match on_disk_state {
                DatabaseIndexState::Enabled => IndexConfig::Database {
                    spec,
                    on_disk_state: DatabaseIndexState::Backfilled { staged: true },
                    persistence_index_id,
                },
                _ => {
                    anyhow::bail!("Index is not enabled, so it cannot be disabled");
                },
            },
            IndexConfig::Text {
                spec,
                on_disk_state,
            } => match on_disk_state {
                TextIndexState::SnapshottedAt(snapshot) => IndexConfig::Text {
                    spec,
                    on_disk_state: TextIndexState::Backfilled {
                        snapshot,
                        staged: true,
                    },
                },
                _ => {
                    anyhow::bail!("Index is not enabled, so it cannot be disabled");
                },
            },
            IndexConfig::Vector {
                spec,
                on_disk_state,
            } => match on_disk_state {
                VectorIndexState::SnapshottedAt(snapshot) => IndexConfig::Vector {
                    spec,
                    on_disk_state: VectorIndexState::Backfilled {
                        snapshot,
                        staged: true,
                    },
                },
                _ => {
                    anyhow::bail!("Index is not enabled, so it cannot be disabled");
                },
            },
        };
        anyhow::ensure!(new_config.is_backfilled(), "Index is not backfilled");
        SystemMetadataModel::new_global(self.tx)
            .patch(id, patch_value!("config" =>  Some(new_config.try_into()?))?)
            .await?;
        Ok(())
    }

    /// Collect all the indexes in the new schema.
    ///
    /// An earlier step (JSON conversion) is responsible for ensuring that index
    /// names are unique.
    async fn indexes_in_new_schema(
        &mut self,
        tables_in_schema: &BTreeMap<TableName, TableDefinition>,
    ) -> anyhow::Result<BTreeMap<IndexName, IndexMetadata<TableName>>> {
        let mut indexes_in_schema: BTreeMap<IndexName, IndexMetadata<TableName>> = BTreeMap::new();
        for (table_name, table_schema) in tables_in_schema {
            for (index_descriptor, index_schema) in &table_schema.indexes {
                let index_name = IndexName::new(table_name.clone(), index_descriptor.clone())?;
                let exists = indexes_in_schema.insert(
                    index_name.clone(),
                    IndexMetadata::new_backfilling(
                        *self.tx.begin_timestamp(),
                        index_name.clone(),
                        index_schema.fields.clone(),
                    ),
                );
                anyhow::ensure!(exists.is_none(), "Index appears twice: {index_name}");
            }
            for (index_descriptor, index_schema) in &table_schema.staged_db_indexes {
                let index_name = IndexName::new(table_name.clone(), index_descriptor.clone())?;
                let exists = indexes_in_schema.insert(
                    index_name.clone(),
                    IndexMetadata::new_staged_backfilling(
                        *self.tx.begin_timestamp(),
                        index_name.clone(),
                        index_schema.fields.clone(),
                    ),
                );
                anyhow::ensure!(exists.is_none(), "Index appears twice: {index_name}");
            }
            for (index_descriptor, index_schema) in &table_schema.text_indexes {
                let index_name = IndexName::new(table_name.clone(), index_descriptor.clone())?;
                let exists = indexes_in_schema.insert(
                    index_name.clone(),
                    IndexMetadata::new_backfilling_text_index(
                        index_name.clone(),
                        index_schema.search_field.clone(),
                        index_schema.filter_fields.clone(),
                    ),
                );
                anyhow::ensure!(exists.is_none(), "Index appears twice: {index_name}");
            }
            for (index_descriptor, index_schema) in &table_schema.staged_text_indexes {
                let index_name = IndexName::new(table_name.clone(), index_descriptor.clone())?;
                let exists = indexes_in_schema.insert(
                    index_name.clone(),
                    IndexMetadata::new_staged_backfilling_text_index(
                        index_name.clone(),
                        index_schema.search_field.clone(),
                        index_schema.filter_fields.clone(),
                    ),
                );
                anyhow::ensure!(exists.is_none(), "Index appears twice: {index_name}");
            }
            for (index_descriptor, index_schema) in &table_schema.vector_indexes {
                let index_name = IndexName::new(table_name.clone(), index_descriptor.clone())?;
                let exists = indexes_in_schema.insert(
                    index_name.clone(),
                    IndexMetadata::new_backfilling_vector_index(
                        index_name.clone(),
                        index_schema.vector_field.clone(),
                        index_schema.dimension,
                        index_schema.filter_fields.clone(),
                    ),
                );
                anyhow::ensure!(exists.is_none(), "Index appears twice: {index_name}");
            }
            for (index_descriptor, index_schema) in &table_schema.staged_vector_indexes {
                let index_name = IndexName::new(table_name.clone(), index_descriptor.clone())?;
                let exists = indexes_in_schema.insert(
                    index_name.clone(),
                    IndexMetadata::new_staged_backfilling_vector_index(
                        index_name.clone(),
                        index_schema.vector_field.clone(),
                        index_schema.dimension,
                        index_schema.filter_fields.clone(),
                    ),
                );
                anyhow::ensure!(exists.is_none(), "Index appears twice: {index_name}");
            }
        }
        Ok(indexes_in_schema)
    }

    ///  Given a set of tables from a not yet fully committed schema,
    ///  returns the difference between the indexes in those not yet committed
    ///  tables and the indexes in storage. We compare only the developer config
    ///  and pending/enabled state of the indexes to determine the diff.
    pub async fn get_index_diff(
        &mut self,
        namespace: TableNamespace,
        tables_in_new_schema: &BTreeMap<TableName, TableDefinition>,
    ) -> anyhow::Result<IndexDiff> {
        let indexes_in_schema = self.indexes_in_new_schema(tables_in_new_schema).await?;
        let indexes_no_longer_in_schema: HashMap<
            IndexName,
            Vec<ParsedDocument<DeveloperIndexMetadata>>,
        > = self
            .get_application_indexes(namespace)
            .await?
            .into_iter()
            .fold(HashMap::new(), |mut acc, index| {
                if !indexes_in_schema.contains_key(&index.name) {
                    acc.entry(index.name.clone()).or_default().push(index);
                }
                acc
            });

        let mut diff = IndexDiff::default();
        for (_, new_index) in indexes_in_schema {
            match self.compare_new_and_existing_indexes(namespace, new_index)? {
                IndexComparison::Added(index) => diff.added.push(index),
                IndexComparison::Identical(index) => diff.identical.push(index),
                IndexComparison::Disabled(index) => diff.disabled.push(index),
                IndexComparison::Enabled(index) => diff.enabled.push(index),
                IndexComparison::Replaced {
                    replaced,
                    replacement,
                } => {
                    for index in replaced {
                        diff.dropped.push(index);
                    }
                    match replacement {
                        ReplacementIndex::NewOrUpdated(index) => diff.added.push(index),
                        ReplacementIndex::Identical(index) => diff.identical.push(index),
                    }
                },
            }
        }

        for (name, mut indexes) in indexes_no_longer_in_schema {
            anyhow::ensure!(
                !name.is_system_owned(),
                "Preparing to drop a system index: {:?}",
                name,
            );
            diff.dropped.append(&mut indexes);
        }

        Ok(diff)
    }

    fn identical_or_replaced(
        &mut self,
        mut existing_index: ParsedDocument<DeveloperIndexMetadata>,
        new_index: DeveloperIndexMetadata,
    ) -> IndexComparison {
        if existing_index.config.same_spec(&new_index.config) {
            if existing_index.config.is_staged() == new_index.config.is_staged() {
                IndexComparison::Identical(existing_index)
            } else {
                // Staged status changed - use the previous on-disk state
                // Toggle the staged status and return enabled/disabled to
                // inform callsite.
                if existing_index.config.is_staged() {
                    existing_index.config.set_staged(false);
                    IndexComparison::Enabled(existing_index)
                } else {
                    existing_index.config.set_staged(true);
                    IndexComparison::Disabled(existing_index)
                }
            }
        } else {
            IndexComparison::Replaced {
                replaced: vec![existing_index],
                replacement: ReplacementIndex::NewOrUpdated(new_index),
            }
        }
    }

    fn compare_new_and_existing_indexes(
        &mut self,
        namespace: TableNamespace,
        new_index: DeveloperIndexMetadata,
    ) -> anyhow::Result<IndexComparison> {
        let pending_index = self.pending_index_metadata(namespace, &new_index.name)?;
        let enabled_index = self.enabled_index_metadata(namespace, &new_index.name)?;
        let mut table_model = TableModel::new(self.tx);
        let pending_index = pending_index
            .map(|doc| table_model.doc_table_id_to_name(doc))
            .transpose()?;
        let enabled_index = enabled_index
            .map(|doc| table_model.doc_table_id_to_name(doc))
            .transpose()?;

        Ok(match (enabled_index, pending_index) {
            (None, None) => IndexComparison::Added(new_index),
            (Some(enabled_index), None) => self.identical_or_replaced(enabled_index, new_index),
            (None, Some(pending_index)) => self.identical_or_replaced(pending_index, new_index),
            (Some(enabled_index), Some(pending_index)) => {
                let mut pending_comparison =
                    self.identical_or_replaced(pending_index.clone(), new_index.clone());
                let mut enabled_comparison =
                    self.identical_or_replaced(enabled_index.clone(), new_index);
                if let IndexComparison::Identical(index) = &mut enabled_comparison {
                    // Allows using a still active index for the current push if a pending
                    // schema/push mutates the index, but hasn't been enabled
                    // yet
                    anyhow::ensure!(index == &enabled_index);
                    enabled_comparison = IndexComparison::Replaced {
                        replaced: vec![pending_index],
                        replacement: ReplacementIndex::Identical(enabled_index),
                    };
                    return Ok(enabled_comparison);
                }
                if let IndexComparison::Replaced {
                    replaced,
                    replacement: _,
                } = &mut pending_comparison
                {
                    // If the pending index has been mutated, we need to replace both the
                    // pending and enabled indexes.
                    anyhow::ensure!(*replaced == vec![pending_index.clone()]);
                    *replaced = vec![enabled_index, pending_index];
                } else if let IndexComparison::Identical(index) = &mut pending_comparison {
                    // If the pending index is identical to the new index, we need to replace
                    // the enabled index with the pending index.
                    anyhow::ensure!(index == &pending_index);
                    pending_comparison = IndexComparison::Replaced {
                        replaced: vec![enabled_index],
                        replacement: ReplacementIndex::Identical(pending_index),
                    };
                }
                pending_comparison
            },
        })
    }

    /// Inserts new and updated mutated indexes so they can be backfilled.
    /// Returns the complete index diff, even though only the additions are
    /// immediately applied (the rest will be applied in apply_config)
    #[fastrace::trace]
    pub async fn prepare_new_and_mutated_indexes(
        &mut self,
        namespace: TableNamespace,
        schema: &DatabaseSchema,
    ) -> anyhow::Result<IndexDiff> {
        let diff: IndexDiff = self.get_index_diff(namespace, &schema.tables).await?;

        // If an index is currently pending and we're mutating it, we need to drop the
        // currently pending index immediately so that we avoid having multiple pending
        // indexes with the same name. This is still atomic(ish) because users cannot be
        // using the pending indexes we drop here.
        let dropped: Vec<ParsedDocument<IndexMetadata<TableName>>> = diff
            .dropped
            .iter()
            .filter(|index| !index.config.is_enabled())
            .cloned()
            .collect();

        let added = diff.added.clone();

        tracing::info!(
            "Preparing new and mutated indexes. Adding {}. Dropping {}.",
            added.len(),
            dropped.len(),
        );
        for index in &dropped {
            self.drop_index(index.id()).await?;
        }
        for index in &diff.added {
            self.add_application_index(namespace, index.clone()).await?;
        }

        // Replace all the enabled/disabled ones in-place to update their `staged`
        // status. The actual enabling/disabling will be done at finish_push
        // (apply_config).
        for index in diff.enabled.iter().chain(diff.disabled.iter()) {
            let (id, value) = index.clone().into_id_and_value();
            let new_config = value.config.try_into()?;
            SystemMetadataModel::new_global(self.tx)
                .patch(id, patch_value!("config" =>  Some(new_config))?)
                .await?;
        }

        Ok(diff)
    }

    pub fn indexed_fields(
        &mut self,
        stable_index_name: &StableIndexName,
        printable_index_name: &IndexName,
    ) -> anyhow::Result<IndexedFields> {
        let resolved_index_name = stable_index_name
            .tablet_index_name()
            .with_context(|| index_not_found_error(printable_index_name))?;
        let metadata =
            self.require_enabled_index_metadata(printable_index_name, resolved_index_name)?;
        match metadata.config.clone() {
            IndexConfig::Database {
                spec: DatabaseIndexSpec { fields },
                ..
            } => Ok(fields),
            _ => anyhow::bail!(index_not_a_database_index_error(printable_index_name)),
        }
    }

    /// Returns the index metadata for the given name if it's enabled or fails
    /// with a descriptive error if the index is either missing or not
    /// enabled.
    ///
    /// Queries and most other use cases should use this method or
    /// enable_index_metadata isntead of pending_index_metadata.
    fn require_enabled_index_metadata(
        &mut self,
        printable_index_name: &IndexName,
        resolved_index_name: &TabletIndexName,
    ) -> anyhow::Result<ParsedDocument<TabletIndexMetadata>> {
        // Because require_enabled does not mutate table_mapping, we can clone it here
        // to avoid duplicate mutable references to self.tx. If require_enabled
        // ever does start mutating the mapping, then this would be unsafe.
        Ok(self
            .tx
            .index
            .require_enabled(
                &mut self.tx.reads,
                resolved_index_name,
                printable_index_name,
            )?
            .metadata()
            .clone())
    }

    /// Returns the index metadata for the given name if it's in the enabled
    /// state or None if the index either can't be found or is not enabled.
    ///
    /// Queries and most other use cases should use
    /// require_enabled_index_metadata or this method instead of
    /// pending_index_metadata.
    pub fn enabled_index_metadata(
        &mut self,
        namespace: TableNamespace,
        index_name: &IndexName,
    ) -> anyhow::Result<Option<ParsedDocument<TabletIndexMetadata>>> {
        self._index_metadata(namespace, index_name, |indexes, reads, index_name| {
            indexes.get_enabled(reads, &index_name)
        })
    }

    /// Returns the index metadata for the given name if it's not yet enabled or
    /// None if the index either can't be found or is enabled.
    ///
    /// Only use this method when you're mutating indexes or their state.
    /// Queries and most other use cases should use enabled_index_metadata
    /// or require_enabled_index_metadata instead.
    pub fn pending_index_metadata(
        &mut self,
        namespace: TableNamespace,
        index_name: &IndexName,
    ) -> anyhow::Result<Option<ParsedDocument<TabletIndexMetadata>>> {
        self._index_metadata(namespace, index_name, |indexes, reads, index_name| {
            indexes.get_pending(reads, &index_name)
        })
    }

    fn pending_resolved_index_metadata(
        &mut self,
        index_name: &TabletIndexName,
    ) -> Option<ParsedDocument<TabletIndexMetadata>> {
        let index = self.tx.index.get_pending(&mut self.tx.reads, index_name)?;
        Some(index.metadata.clone())
    }

    fn _index_metadata<'b>(
        &'b mut self,
        namespace: TableNamespace,
        index_name: &IndexName,
        getter: impl FnOnce(
            &'b mut TransactionIndex,
            &'b mut TransactionReadSet,
            TabletIndexName,
        ) -> Option<&'b Index>,
    ) -> anyhow::Result<Option<ParsedDocument<TabletIndexMetadata>>> {
        if !self
            .tx
            .table_mapping()
            .namespace(namespace)
            .name_exists(index_name.table())
        {
            return Ok(None);
        }
        let index_name = self.resolve_index_name(namespace, index_name)?;
        Ok(getter(&mut self.tx.index, &mut self.tx.reads, index_name)
            .map(|index| index.metadata.clone()))
    }

    pub fn stable_index_name(
        &mut self,
        namespace: TableNamespace,
        index_name: &IndexName,
        table_filter: TableFilter,
    ) -> anyhow::Result<StableIndexName> {
        if self
            .tx
            .virtual_system_mapping()
            .is_virtual_table(index_name.table())
        {
            let physical_index_name = self
                .tx
                .virtual_system_mapping()
                .virtual_to_system_index(index_name)?
                .clone();
            Ok(StableIndexName::Virtual(
                index_name.clone(),
                self.resolve_index_name(namespace, &physical_index_name)?,
            ))
        } else if self
            .tx
            .table_mapping()
            .namespace(namespace)
            .name_exists(index_name.table())
        {
            match table_filter {
                TableFilter::IncludePrivateSystemTables => Ok(StableIndexName::Physical(
                    self.resolve_index_name(namespace, index_name)?,
                )),
                TableFilter::ExcludePrivateSystemTables => {
                    if index_name.table().is_system() {
                        Ok(StableIndexName::Missing(index_name.clone()))
                    } else {
                        Ok(StableIndexName::Physical(
                            self.resolve_index_name(namespace, index_name)?,
                        ))
                    }
                },
            }
        } else {
            Ok(StableIndexName::Missing(index_name.clone()))
        }
    }

    fn resolve_index_name(
        &mut self,
        namespace: TableNamespace,
        index_name: &IndexName,
    ) -> anyhow::Result<TabletIndexName> {
        index_name.clone().map_table(
            &self
                .tx
                .table_mapping()
                .namespace(namespace)
                .name_to_tablet(),
        )
    }

    /// Returns by_id indexes for *all tablets*, including hidden ones.
    pub async fn by_id_indexes(&mut self) -> anyhow::Result<BTreeMap<TabletId, IndexRef>> {
        let all_indexes = self.get_all_indexes()?;
        all_indexes
            .filter(|index| index.name.is_by_id())
            .map(|index| Ok((*index.name.table(), IndexRef::try_from(index)?)))
            .collect()
    }

    pub async fn by_id_index_metadata(
        &mut self,
        tablet_id: TabletId,
    ) -> anyhow::Result<ParsedDocument<TabletIndexMetadata>> {
        self.all_indexes_on_table(tablet_id)
            .await?
            .into_iter()
            .find(|index| index.name.is_by_id())
            .ok_or_else(|| anyhow::anyhow!("by_id index missing for {tablet_id}"))
    }

    /// All indexes (system and developer-defined and
    /// backfilling/backfilled/enabled) for a single table.
    pub async fn all_indexes_on_table(
        &mut self,
        tablet_id: TabletId,
    ) -> anyhow::Result<Vec<ParsedDocument<TabletIndexMetadata>>> {
        let all_indexes = self.get_all_indexes()?;
        Ok(all_indexes
            .filter(|index| *index.name.table() == tablet_id)
            .cloned()
            .collect())
    }

    /// Returns all registered indexes (both system and developer-defined)
    /// including both pending and enabled indexes.
    ///
    /// Because of mutated indexes, there may be up to two indexes with the same
    /// name (but different configurations).
    pub fn get_all_indexes(
        &mut self,
    ) -> anyhow::Result<impl Iterator<Item = &'_ ParsedDocument<TabletIndexMetadata>>> {
        self.take_indexes_dependency()?;
        Ok(self.get_all_indexes_untracked())
    }

    pub fn take_indexes_dependency(&mut self) -> anyhow::Result<()> {
        let index_by_id = TabletIndexName::by_id(
            self.tx
                .table_mapping()
                .namespace(TableNamespace::Global)
                .id(&INDEX_TABLE)
                .context("_index should exist")?
                .tablet_id,
        );
        self.tx.reads.record_indexed_directly(
            index_by_id,
            IndexedFields::by_id(),
            Interval::all(),
            &self.tx.limits,
        )?;
        Ok(())
    }

    pub fn get_all_indexes_untracked(
        &self,
    ) -> impl Iterator<Item = &'_ ParsedDocument<TabletIndexMetadata>> {
        self.tx.index.index_registry().all_indexes()
    }

    /// Returns all search indexes (text and vector) on non-empty tables.
    pub async fn get_all_non_empty_search_indexes(
        &mut self,
    ) -> anyhow::Result<Vec<ParsedDocument<TabletIndexMetadata>>> {
        self.take_indexes_dependency()?;
        let mut non_empty_indexes = vec![];
        for index in self.get_all_indexes_untracked() {
            match index.config {
                IndexConfig::Text { .. } | IndexConfig::Vector { .. } => (),
                IndexConfig::Database { .. } => continue,
            };
            let table = *index.name.table();
            let Some(count) = self.tx.count_snapshot.count(table).await? else {
                return Err(table_summary_bootstrapping_error(Some(
                    "Table count unavailable while bootstrapping",
                )));
            };
            if count != 0 {
                non_empty_indexes.push(index.clone());
            }
        }
        Ok(non_empty_indexes)
    }

    /// Returns the index metadata matching the given id or fails if the
    /// document is missing or not an index.
    pub async fn require_index_by_id(
        &mut self,
        index_id: ResolvedDocumentId,
    ) -> anyhow::Result<ParsedDocument<TabletIndexMetadata>> {
        let document = self
            .tx
            .get(index_id)
            .await?
            .with_context(|| format!("Missing document for index id {index_id:?}"))?;
        TabletIndexMetadata::from_document(document)
    }

    /// Returns all registered indexes that are system owned including both
    /// pending and enabled indexes.
    ///
    /// Because of mutated indexes, there may be up to two indexes with the same
    /// name (but different configurations).
    pub async fn get_system_indexes(
        &mut self,
        namespace: TableNamespace,
    ) -> anyhow::Result<Vec<ParsedDocument<DeveloperIndexMetadata>>> {
        self.get_indexes(IndexCategory::System, namespace).await
    }

    /// Returns all registered indexes that aren't system owned including both
    /// pending and enabled indexes.
    ///
    /// Because of mutated indexes, there may be up to two indexes with the same
    /// name (but different configurations).
    pub async fn get_application_indexes(
        &mut self,
        namespace: TableNamespace,
    ) -> anyhow::Result<Vec<ParsedDocument<DeveloperIndexMetadata>>> {
        self.get_indexes(IndexCategory::Application, namespace)
            .await
    }

    async fn get_indexes(
        &mut self,
        category: IndexCategory,
        namespace: TableNamespace,
    ) -> anyhow::Result<Vec<ParsedDocument<DeveloperIndexMetadata>>> {
        self.take_indexes_dependency()?;
        self.tx.take_table_mapping_dep();
        let table_mapping = self.tx.metadata.table_mapping();
        let mut result = vec![];
        for doc in self.get_all_indexes_untracked() {
            if !category.belongs(doc, table_mapping) {
                continue;
            }
            let tablet_id = *doc.name.table();
            if table_mapping.tablet_namespace(tablet_id)? != namespace {
                continue;
            }
            let doc = doc
                .clone()
                .map(|metadata| metadata.map_table(&table_mapping.tablet_to_name()))?;
            result.push(doc);
        }
        Ok(result)
    }

    pub async fn drop_index(&mut self, index_id: ResolvedDocumentId) -> anyhow::Result<()> {
        SystemMetadataModel::new_global(self.tx)
            .delete(index_id)
            .await?;
        Ok(())
    }

    pub async fn drop_system_index(
        &mut self,
        namespace: TableNamespace,
        index_name: IndexName,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(index_name.table().is_system());
        let system_index = self
            .get_system_indexes(namespace)
            .await?
            .into_iter()
            .find(|index| index.name == index_name);
        if let Some(system_index) = system_index {
            tracing::info!("Dropping system index {index_name}");
            self.drop_index(system_index.id()).await?;
        }
        Ok(())
    }

    pub async fn copy_indexes_to_table(
        &mut self,
        namespace: TableNamespace,
        source_table: &TableName,
        target_table: TabletId,
    ) -> anyhow::Result<()> {
        // Copy over enabled indexes from existing active table, if any.
        let Some(active_table_id) = self
            .tx
            .table_mapping()
            .namespace(namespace)
            .id_if_exists(source_table)
        else {
            return Ok(());
        };
        for index in IndexModel::new(self.tx)
            .all_indexes_on_table(active_table_id)
            .await?
        {
            anyhow::ensure!(
                !index.config.is_backfilling(),
                ErrorMetadata::bad_request(
                    "InvalidImport",
                    format!(
                        "{source_table} is still backfilling indexes, so it cannot be replaced. \
                         Wait for indexes to complete backfilling"
                    )
                )
            );
            if !index.config.is_enabled() {
                // Only copy Enabled indexes, otherwise we might get naming conflicts.
                continue;
            }
            if index.name.is_by_id_or_creation_time() {
                // by_id and by_creation_time already created.
                continue;
            }
            let index_name = if index.name.descriptor().is_reserved() {
                TabletIndexName::new_reserved(target_table, index.name.descriptor().clone())?
            } else {
                TabletIndexName::new(target_table, index.name.descriptor().clone())?
            };
            let metadata = match index.into_value().config {
                IndexConfig::Database {
                    spec: DatabaseIndexSpec { fields },
                    ..
                } => IndexMetadata::new_backfilling(*self.tx.begin_timestamp(), index_name, fields),
                IndexConfig::Text {
                    spec:
                        TextIndexSpec {
                            search_field,
                            filter_fields,
                        },
                    ..
                } => IndexMetadata::new_backfilling_text_index(
                    index_name,
                    search_field,
                    filter_fields,
                ),
                IndexConfig::Vector {
                    spec:
                        VectorIndexSpec {
                            dimensions,
                            vector_field,
                            filter_fields,
                        },
                    ..
                } => IndexMetadata::new_backfilling_vector_index(
                    index_name,
                    vector_field,
                    dimensions,
                    filter_fields,
                ),
            };
            SystemMetadataModel::new_global(self.tx)
                .insert_metadata(&INDEX_TABLE, metadata.try_into()?)
                .await?;
        }
        Ok(())
    }

    // Check if the system index is ready for all the given tables.
    // Useful for streaming import - waiting for the system indexes to be ready
    // for all the tables before proceeding with the import.
    pub async fn indexes_ready(
        &mut self,
        index_descriptor: &IndexDescriptor,
        indexes: &BTreeSet<TableName>,
    ) -> anyhow::Result<bool> {
        let index_metadata = indexes
            .iter()
            .map(|table_name| {
                let index_name =
                    IndexName::new_reserved(table_name.clone(), index_descriptor.clone())?;
                // We really just want pending indexes here, but since it's convenient, we're
                // also verifying that all requested tables have the expected
                // index using enabled_index_metadata.
                let mut model = IndexModel::new(self.tx);
                let index_metadata = model
                    .pending_index_metadata(TableNamespace::root_component(), &index_name)?
                    .or(model
                        .enabled_index_metadata(TableNamespace::root_component(), &index_name)?)
                    .context(ErrorMetadata::bad_request(
                        "MissingIndex",
                        format!("Missing index: {index_name}"),
                    ))?;
                Ok(index_metadata)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let are_all_indexes_ready = index_metadata
            .iter()
            .all(|metadata| !metadata.config.is_backfilling());
        Ok(are_all_indexes_ready)
    }
}

enum IndexCategory {
    System,
    Application,
}

impl IndexCategory {
    fn belongs(
        &self,
        index: &ParsedDocument<TabletIndexMetadata>,
        table_mapping: &TableMapping,
    ) -> bool {
        let is_system = index.name.descriptor().is_reserved()
            || table_mapping.is_system_tablet(*index.name.table());
        let is_active = table_mapping.is_active(*index.name.table());
        is_active
            && match self {
                Self::System => is_system,
                Self::Application => !is_system,
            }
    }
}

enum IndexComparison {
    Added(DeveloperIndexMetadata),
    Identical(ParsedDocument<DeveloperIndexMetadata>),
    Replaced {
        replaced: Vec<ParsedDocument<DeveloperIndexMetadata>>,
        replacement: ReplacementIndex,
    },
    /// Enable a staged index
    Enabled(ParsedDocument<DeveloperIndexMetadata>),
    /// Disable a staged index
    Disabled(ParsedDocument<DeveloperIndexMetadata>),
}

enum ReplacementIndex {
    /// This replacement index is not yet in storage or its definition has
    /// changed.
    NewOrUpdated(DeveloperIndexMetadata),
    /// The replacement index is in storage and its definition has not changed.
    Identical(ParsedDocument<DeveloperIndexMetadata>),
}
