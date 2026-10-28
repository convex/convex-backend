use std::{
    collections::{
        btree_map::Entry,
        BTreeMap,
        BTreeSet,
    },
    iter,
    slice,
};

use common::{
    bootstrap_model::index::{
        database_index::{
            DatabaseIndexSpec,
            DatabaseIndexState,
            IndexedFields,
        },
        text_index::TextIndexSpec,
        IndexConfig,
        TabletIndexMetadata,
        INDEX_BY_TABLE_ID_VIRTUAL_INDEX_DESCRIPTOR,
        INDEX_TABLE,
        TABLE_ID_FIELD_PATH,
    },
    document::{
        PackedDocument,
        ParseDocument,
        ParsedDocument,
        ResolvedDocument,
    },
    document_index_keys::{
        DocumentIndexKeys,
        IndexKeyUpdate,
        IndexUpdate,
        SearchIndexKeyValue,
        SearchValueTokens,
        Update,
    },
    index::{
        IndexKey,
        IndexKeyBytes,
    },
    query::FilterValue as SearchFilterValue,
    types::{
        DatabaseIndexUpdate,
        DatabaseIndexValue,
        GenericIndexName,
        IndexId,
        IndexName,
        IndexRef,
        IndexWriteMode,
        PrevIndexEntry,
        TabletIndexName,
        WriteTimestamp,
    },
};
use errors::ErrorMetadata;
use imbl::OrdMap;
use itertools::Itertools;
use value::{
    ConvexString,
    ConvexValue,
    FieldPath,
    ResolvedDocumentId,
    TableMapping,
    TableNamespace,
    TableNumber,
    TabletId,
};

/// [`IndexRegistry`] maintains the metadata for indexes, indicating
/// which indexes exist in the system and which are ready to use. It is a
/// derived view of the `_index` system table,
///
/// This data structure *must* match a transaction's view of the underlying
/// table.
///
/// New indexes are registered via [`TransactionState::add_index_metadata`],
///
/// Index names are expected to be unique, except that database indexes can have
/// up to two entries per unique name, one pending and one enabled.
///
/// Enabled indexes are available to be queried and used by applications.
/// Pending indexes are backfilling or backfilled but not yet enabled indexes
/// that should not be used by applications.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IndexRegistry {
    index_table: TabletId,
    index_table_number: TableNumber,
    // Indexes that are enabled and ready to be queried against.
    enabled_indexes: OrdMap<TabletIndexName, Index>,
    // Indexes that are not enabled for queries, typically backfilling, waiting to be
    // committed, or disabled.
    pending_indexes: OrdMap<TabletIndexName, Index>,
}

impl IndexRegistry {
    pub fn index_table(&self) -> TabletId {
        self.index_table
    }

    pub fn index_table_number(&self) -> TableNumber {
        self.index_table_number
    }

    /// Fill out all of our index metadata given the latest version of each
    /// document in the `_index` table. After initializing each index, mark
    /// all of them as completed since we'll be streaming in all non
    /// `_index` documents later.
    #[fastrace::trace]
    pub fn bootstrap<Doc: ParseDocument<TabletIndexMetadata>>(
        table_mapping: &TableMapping,
        index_documents: impl Iterator<Item = Doc>,
    ) -> anyhow::Result<Self> {
        let index_table = table_mapping
            .namespace(TableNamespace::Global)
            .name_to_tablet()(INDEX_TABLE.clone())?;
        let index_table_number = table_mapping.tablet_number(index_table)?;
        let mut index = Self {
            index_table,
            index_table_number,
            enabled_indexes: OrdMap::new(),
            pending_indexes: OrdMap::new(),
        };

        let meta_index_name = GenericIndexName::by_id(index_table);
        let mut meta_index = None;
        let mut regular_indexes = vec![];

        for document in index_documents {
            let metadata = document.parse()?;
            anyhow::ensure!(metadata.id().tablet_id == index_table);
            if metadata.name == meta_index_name {
                anyhow::ensure!(meta_index.is_none());
                meta_index = Some(metadata);
            } else {
                regular_indexes.push(metadata);
            }
        }

        let meta_index = meta_index
            .ok_or_else(|| anyhow::anyhow!("Missing `by_id` index for {}", INDEX_TABLE))?;

        // First insert the `_index` table scan index.
        index.insert(Index::new(meta_index));

        // Then insert the rest of the indexes.
        for metadata in regular_indexes {
            index.insert(Index::new(metadata));
        }

        Ok(index)
    }

    // Verifies and applies and update.
    pub fn update(
        &mut self,
        deletion: Option<&ResolvedDocument>,
        insertion: Option<&ResolvedDocument>,
    ) -> anyhow::Result<()> {
        self.verify_update(deletion, insertion)?;
        self.apply_verified_update(deletion, insertion);
        Ok(())
    }

    /// Returns the index keys for `document` for all the registered indexes on
    /// its table.
    ///
    /// N.B.: if `D` is a `ResolvedDocument` the returned keys are `IndexKey`s,
    /// but if it's a `PackedDocument` then this function returns
    /// `IndexKeyBytes` directly.
    pub(crate) fn index_keys<'a, D: IndexedDocument>(
        &'a self,
        document: &'a D,
    ) -> impl Iterator<Item = (&'a Index, IndexRef, IndexWriteMode, D::IndexKey)> + 'a {
        iter::from_coroutine(
            #[coroutine]
            move || {
                for index in self.indexes_by_table(document.id().tablet_id) {
                    // Only database indexes have keys in persistence; the ref
                    // and write mode come from the same config, so callers
                    // need not convert or look them up.
                    if let IndexConfig::Database {
                        spec: DatabaseIndexSpec { fields },
                        on_disk_state,
                        persistence_index_id,
                    } = &index.metadata.config
                    {
                        yield (
                            index,
                            IndexRef::from_parts(index.id(), *persistence_index_id),
                            on_disk_state.write_mode(),
                            document.index_key_bytes(&fields[..]),
                        );
                    }
                }
            },
        )
    }

    /// `deletion` carries the old revision's write timestamp, so every update
    /// that supersedes one of its entries can say which. A revision still
    /// pending in the same transaction has no persisted entry to supersede;
    /// the committer's resolved update names the committed one.
    pub fn index_updates<'a>(
        &'a self,
        deletion: Option<(&'a ResolvedDocument, WriteTimestamp)>,
        insertion: Option<&'a ResolvedDocument>,
    ) -> Vec<DatabaseIndexUpdate> {
        let mut updates = BTreeMap::new();
        if let Some((old_document, old_ts)) = deletion {
            let prev = match old_ts {
                WriteTimestamp::Committed(ts) => Some(PrevIndexEntry {
                    ts,
                    document_id: old_document.id_with_table_id(),
                }),
                WriteTimestamp::Pending => None,
            };
            for (index, index_ref, mode, index_key) in self.index_keys(old_document) {
                updates.insert(
                    (index.id(), index_key.clone()),
                    DatabaseIndexUpdate {
                        mode,
                        index: index_ref,
                        key: index_key,
                        value: DatabaseIndexValue::Deleted,
                        prev,
                        is_system_index: index.name().descriptor().is_reserved(),
                    },
                );
            }
        }
        if let Some(new_document) = insertion {
            for (index, index_ref, mode, index_key) in self.index_keys(new_document) {
                let entry = updates.entry((index.id(), index_key.clone()));
                let prev = match &entry {
                    Entry::Occupied(superseded) => superseded.get().prev,
                    Entry::Vacant(_) => None,
                };
                entry.insert_entry(DatabaseIndexUpdate {
                    mode,
                    index: index_ref,
                    key: index_key,
                    value: DatabaseIndexValue::NonClustered(new_document.id()),
                    prev,
                    is_system_index: index.name().descriptor().is_reserved(),
                });
            }
        }
        updates.into_values().collect()
    }

    /// A document's update to one text index's key, or `None` if `index` isn't
    /// a text index or the document exists on neither side of the update.
    fn text_index_key_update<F>(
        index: &Index,
        old_doc: Option<&PackedDocument>,
        new_doc: Option<&PackedDocument>,
        search_tokenizer: &F,
    ) -> Option<Update<SearchIndexKeyValue>>
    where
        F: Fn(ConvexString) -> SearchValueTokens,
    {
        let IndexConfig::Text {
            spec:
                TextIndexSpec {
                    search_field,
                    filter_fields,
                },
            ..
        } = &index.metadata.config
        else {
            return None;
        };
        let compute_search_key = |doc: &PackedDocument| {
            let filter_values = filter_fields
                .iter()
                .map(|field| {
                    let value = doc.value().get_path(field);
                    let bytes = SearchFilterValue::from_search_value(value.as_ref());
                    (field.clone(), bytes)
                })
                .collect();
            let search_field_value = match doc.value().get_path(search_field) {
                Some(ConvexValue::String(string)) => Some(search_tokenizer(string)),
                _ => None,
            };
            SearchIndexKeyValue {
                filter_values,
                search_field: search_field.clone(),
                search_field_value,
            }
        };
        let update = Update {
            old: old_doc.map(compute_search_key),
            new: new_doc.map(compute_search_key),
        };
        (update.old.is_some() || update.new.is_some()).then_some(update)
    }

    /// A document's updates to enabled indexes
    pub fn document_index_keys<F>(
        &self,
        id: ResolvedDocumentId,
        old_document: Option<&PackedDocument>,
        new_document: Option<&PackedDocument>,
        search_tokenizer: F,
    ) -> DocumentIndexKeys
    where
        F: Fn(ConvexString) -> SearchValueTokens,
    {
        let database_keys = self
            .iter_database_index_keys(id, old_document, new_document)
            .map(|(index_name, update)| (index_name, IndexKeyUpdate::Database(update)));
        let text_keys = self
            .enabled_indexes_for_table(id.tablet_id)
            .filter_map(|index| {
                let update = Self::text_index_key_update(
                    index,
                    old_document,
                    new_document,
                    &search_tokenizer,
                )?;
                Some((index.name(), IndexKeyUpdate::Text(update)))
            });
        let map = database_keys
            .chain(text_keys)
            .map(|(index_name, update)| {
                (
                    index_name,
                    IndexUpdate {
                        document_id: id,
                        update,
                        new_document: new_document.cloned(),
                    },
                )
            })
            .collect();
        DocumentIndexKeys(map)
    }

    /// A document's updates to the keys of enabled database indexes.
    ///
    /// Text index keys need the document's text tokenized, which is expensive
    /// enough that callers who only conflict-check database indexes use this
    /// rather than [`Self::document_index_keys`].
    pub fn database_index_keys<'a>(
        &'a self,
        id: ResolvedDocumentId,
        old_document: Option<&'a PackedDocument>,
        new_document: Option<&'a PackedDocument>,
    ) -> impl Iterator<Item = (TabletIndexName, Update<IndexKeyBytes>)> + 'a {
        self.iter_database_index_keys(id, old_document, new_document)
    }

    /// The single source of which database indexes a document update touches,
    /// so that [`Self::document_index_keys`] and [`Self::database_index_keys`]
    /// can't disagree about it.
    fn iter_database_index_keys<'a>(
        &'a self,
        id: ResolvedDocumentId,
        old_document: Option<&'a PackedDocument>,
        new_document: Option<&'a PackedDocument>,
    ) -> impl Iterator<Item = (TabletIndexName, Update<IndexKeyBytes>)> + 'a {
        let enabled = self
            .enabled_indexes_for_table(id.tablet_id)
            .filter_map(move |index| {
                let IndexConfig::Database {
                    spec: DatabaseIndexSpec { fields },
                    ..
                } = &index.metadata.config
                else {
                    return None;
                };
                let update = Self::database_index_key_update(fields, old_document, new_document)?;
                Some((index.name(), update))
            });
        let by_table_id = (id.tablet_id == self.index_table)
            .then(|| self.index_by_table_id_keys(old_document, new_document));
        enabled.chain(by_table_id)
    }

    /// A document's update to one database index's key, or `None` if the
    /// document exists on neither side of the update.
    fn database_index_key_update(
        fields: &[FieldPath],
        old_doc: Option<&PackedDocument>,
        new_doc: Option<&PackedDocument>,
    ) -> Option<Update<IndexKeyBytes>> {
        let update = Update {
            old: old_doc.map(|doc| doc.index_key_bytes(fields)),
            new: new_doc.map(|doc| doc.index_key_bytes(fields)),
        };
        (update.old.is_some() || update.new.is_some()).then_some(update)
    }

    /// The keys for `_index.by_table_id`, a virtual index that doesn't exist in
    /// the registry but that transactions writing to a table take a read
    /// dependency on.
    fn index_by_table_id_keys(
        &self,
        old_document: Option<&PackedDocument>,
        new_document: Option<&PackedDocument>,
    ) -> (TabletIndexName, Update<IndexKeyBytes>) {
        let index_name = GenericIndexName::new(
            self.index_table,
            INDEX_BY_TABLE_ID_VIRTUAL_INDEX_DESCRIPTOR.clone(),
        )
        .expect("invalid built-in index name");
        let fields = slice::from_ref(&*TABLE_ID_FIELD_PATH);
        let update = Update {
            old: old_document.map(|doc| doc.index_key_bytes(fields)),
            new: new_document.map(|doc| doc.index_key_bytes(fields)),
        };
        (index_name, update)
    }

    // Verifies if an update is valid.
    fn verify_update(
        &self,
        old_document: Option<&ResolvedDocument>,
        new_document: Option<&ResolvedDocument>,
    ) -> anyhow::Result<()> {
        // Checks performed when updating a document.
        if let (Some(old_document), Some(new_document)) = (&old_document, &new_document) {
            anyhow::ensure!(old_document.id() == new_document.id());
            anyhow::ensure!(old_document.table() == new_document.table());
            if old_document.id().tablet_id == self.index_table() {
                let old_metadata = TabletIndexMetadata::from_document((*old_document).clone())?;
                let new_metadata = TabletIndexMetadata::from_document((*new_document).clone())?;
                anyhow::ensure!(
                    old_metadata.name.table() == new_metadata.name.table(),
                    "Can't change indexed table"
                );
                if old_metadata.name.is_by_id_or_creation_time() {
                    anyhow::ensure!(
                        old_metadata.name == new_metadata.name,
                        "Can't rename system defined index {}",
                        old_metadata.name
                    );
                }
                anyhow::ensure!(
                    old_metadata.config.same_spec(&new_metadata.config),
                    "Can't modify index spec for existing indexes {}",
                    old_metadata.name
                );
            }
        }
        // Checks performed when updating or removing a document.
        if let Some(old_document) = old_document {
            if old_document.id().tablet_id == self.index_table() {
                let metadata = TabletIndexMetadata::from_document(old_document.clone())?;
                let index_name = metadata.name.clone();
                if !self.enabled_indexes.contains_key(&index_name)
                    && !self.pending_indexes.contains_key(&index_name)
                {
                    anyhow::bail!("Updating nonexistent index {}", metadata.name);
                }
            }
            if !self
                .enabled_indexes
                .contains_key(&GenericIndexName::by_id(old_document.id().tablet_id))
            {
                anyhow::bail!("Removing document that doesn't exist in index");
            }
        }
        // Checks performed when updating or adding a document.
        if let Some(new_document) = new_document {
            let tablet_id = new_document.id().tablet_id;
            anyhow::ensure!(
                self.enabled_indexes
                    .contains_key(&GenericIndexName::by_id(tablet_id)),
                "Missing `by_id` index for table {}",
                tablet_id,
            );
            if tablet_id == self.index_table() {
                let metadata = TabletIndexMetadata::from_document(new_document.clone())?;

                // Only the "by_id" index is allowed on the index table itself. The IndexWorker
                // cannot backfill the `_index` table itself, as it loads all index records into
                // memory first when bootstrapping. After loading these records at the latest
                // snapshot, it doesn't make sense to retraverse the `_index` table
                // historically.
                if metadata.name.table() == &self.index_table {
                    anyhow::ensure!(metadata.name.is_by_id());
                }

                if metadata.name.is_by_id() {
                    if let IndexConfig::Database { on_disk_state, .. } = &metadata.config {
                        anyhow::ensure!(
                            *on_disk_state == DatabaseIndexState::Enabled,
                            "All `by_id` indexes should be enabled: {:?}",
                            metadata
                        );
                    } else {
                        anyhow::bail!("`by_id` index must be a database index")
                    }
                }

                // An index cannot be created if another index exists with the same name
                // and same state. The existing index of the same name and state must be deleted
                // first. Note indexes can be edited, e.g. to change state from
                // Backfilling to Enabled.
                let enabled_index = self.enabled_indexes.get(&metadata.name);
                let pending_index = self.pending_indexes.get(&metadata.name);
                Self::verify_index_state(enabled_index, pending_index, &metadata)?;
            }
        }
        Ok(())
    }

    fn verify_index_state(
        enabled_index: Option<&Index>,
        pending_index: Option<&Index>,
        metadata: &ParsedDocument<TabletIndexMetadata>,
    ) -> anyhow::Result<()> {
        if metadata.config.is_enabled() {
            anyhow::ensure!(
                Self::existing_is_none_or_equal(enabled_index, metadata),
                "Cannot create a second enabled index with name {}, \ncurrent: {enabled_index:?}, \
                 \nnew: {metadata:?}",
                metadata.name,
            );
        } else {
            anyhow::ensure!(
                Self::existing_is_none_or_equal(pending_index, metadata),
                "Cannot create a second pending index with name {}, \n current: {pending_index:?} \
                 \nnew: {metadata:?}",
                metadata.name,
            )
        }
        Ok(())
    }

    fn existing_is_none_or_equal(
        existing: Option<&Index>,
        updated: &ParsedDocument<TabletIndexMetadata>,
    ) -> bool {
        existing.is_none() || existing.unwrap().id() == updated.id().internal_id().into()
    }

    // Applies an already verified update. Should call verify_update beforehand
    // or the function might panic. Returns if the index registry has been
    // modified.
    pub fn apply_verified_update(
        &mut self,
        deletion: Option<&ResolvedDocument>,
        insertion: Option<&ResolvedDocument>,
    ) -> bool {
        let mut modified = false;
        if let Some(old_document) = deletion
            && old_document.id().tablet_id == self.index_table()
        {
            let index = TabletIndexMetadata::from_document(old_document.clone()).unwrap();
            self.remove(&index);
            modified = true;
        }
        if let Some(new_document) = insertion {
            // The default index should exist for a table before adding
            // any documents.
            let table_id = new_document.id().tablet_id;
            if table_id == self.index_table() {
                let metadata = TabletIndexMetadata::from_document(new_document.clone()).unwrap();
                let index = Index::new(metadata);
                self.insert(index);
                modified = true;
            }
        }

        modified
    }

    pub fn all_tables_with_indexes(&self) -> Vec<TabletId> {
        self.all_indexes()
            .map(|index| *index.name.table())
            .sorted()
            .dedup()
            .collect()
    }

    pub fn all_text_indexes(&self) -> Vec<ParsedDocument<TabletIndexMetadata>> {
        self.all_indexes()
            .filter(|index| index.is_text_index())
            .cloned()
            .collect()
    }

    pub fn all_vector_indexes(&self) -> Vec<ParsedDocument<TabletIndexMetadata>> {
        self.all_indexes()
            .filter(|index| index.is_vector_index())
            .cloned()
            .collect()
    }

    pub fn all_text_and_vector_indexes(&self) -> Vec<ParsedDocument<TabletIndexMetadata>> {
        self.all_indexes()
            .filter(|index| index.is_text_index() || index.is_vector_index())
            .cloned()
            .collect()
    }

    pub fn all_database_indexes(&self) -> Vec<ParsedDocument<TabletIndexMetadata>> {
        self.all_indexes()
            .filter(|index| index.is_database_index())
            .cloned()
            .collect()
    }

    pub fn enabled_index_by_index_id(&self, index_id: &IndexId) -> Option<&Index> {
        self.enabled_indexes
            .values()
            .find(|index| *index_id == index.id())
    }

    pub fn pending_index_by_index_id(&self, index_id: &IndexId) -> Option<&Index> {
        self.pending_indexes
            .values()
            .find(|index| *index_id == index.id())
    }

    pub fn all_indexes(&self) -> impl Iterator<Item = &ParsedDocument<TabletIndexMetadata>> {
        self.enabled_indexes
            .values()
            .chain(self.pending_indexes.values())
            .map(|index| index.metadata())
    }

    pub fn all_database_index_configs(
        &self,
    ) -> BTreeMap<IndexId, (TabletIndexName, IndexedFields)> {
        self.all_indexes()
            .filter_map(|index| {
                let index_id = index.id().internal_id().into();
                let index_name = index.name.clone();
                match &index.config {
                    IndexConfig::Database { spec, .. } => {
                        Some((index_id, (index_name, spec.fields.clone())))
                    },
                    IndexConfig::Text { .. } | IndexConfig::Vector { .. } => None,
                }
            })
            .collect()
    }

    pub fn all_enabled_indexes(&self) -> Vec<ParsedDocument<TabletIndexMetadata>> {
        self.enabled_indexes
            .values()
            .map(|index| index.metadata())
            .cloned()
            .collect()
    }

    pub fn enabled_indexes_for_table(
        &self,
        tablet_id: TabletId,
    ) -> impl Iterator<Item = &'_ Index> {
        self.enabled_indexes
            .range(TabletIndexName::min_for_table(tablet_id)..)
            .take_while(move |(name, _)| *name.table() == tablet_id)
            .map(|(_, index)| index)
    }

    pub fn pending_indexes_for_table(
        &self,
        tablet_id: TabletId,
    ) -> impl Iterator<Item = &'_ Index> {
        self.pending_indexes
            .range(TabletIndexName::min_for_table(tablet_id)..)
            .take_while(move |(name, _)| *name.table() == tablet_id)
            .map(|(_, index)| index)
    }

    pub fn by_id_indexes(&self) -> anyhow::Result<BTreeMap<TabletId, IndexRef>> {
        self.all_enabled_indexes()
            .into_iter()
            .filter(|index| index.name.is_by_id())
            .map(|index| Ok((*index.name.table(), IndexRef::try_from(&index)?)))
            .collect()
    }

    /// Returns true if there are neither pending nor enabled indexes for the
    /// given table.
    pub fn has_no_indexes(&self, tablet_id: TabletId) -> bool {
        let table_indexes: Vec<&Index> = self.indexes_by_table(tablet_id).collect();
        table_indexes.is_empty()
    }

    pub fn text_indexes_by_table(
        &self,
        tablet_id: TabletId,
    ) -> impl Iterator<Item = &'_ Index> + '_ {
        // We only support storing one search index with a given name at at time, so
        // unlike database indexes, we're not overly concerned with the state.
        self.indexes_by_table(tablet_id)
            .filter(|index| index.metadata.is_text_index())
    }

    pub fn vector_indexes_by_table(
        &self,
        tablet_id: TabletId,
    ) -> impl Iterator<Item = &'_ Index> + '_ {
        self.indexes_by_table(tablet_id)
            .filter(|index| index.metadata.is_vector_index())
    }

    /// Returns both enabled and pending indexes for the given table.
    ///
    /// Multiple Indexes with a given name will be returned if an index is
    /// mutated but the mutated version is not yet enabled.
    pub(crate) fn indexes_by_table(
        &self,
        tablet_id: TabletId,
    ) -> impl Iterator<Item = &'_ Index> + '_ {
        self.enabled_indexes_for_table(tablet_id)
            .chain(self.pending_indexes_for_table(tablet_id))
    }

    pub fn enabled_index_metadata(
        &self,
        name: &TabletIndexName,
    ) -> Option<ParsedDocument<TabletIndexMetadata>> {
        self.get_enabled(name).map(|index| index.metadata.clone())
    }

    pub fn require_enabled(
        &self,
        index_name: &TabletIndexName,
        printable_index_name: &IndexName,
    ) -> anyhow::Result<Index> {
        let enabled = self.get_enabled(index_name);
        if let Some(enabled) = enabled {
            return Ok(enabled.clone());
        }
        match self.get_pending(index_name) {
            Some(index) => {
                if index.metadata.config.is_staged() {
                    anyhow::bail!(index_staged_error(printable_index_name))
                } else {
                    anyhow::bail!(index_backfilling_error(printable_index_name))
                }
            },
            None => {
                anyhow::bail!(index_not_found_error(printable_index_name))
            },
        }
    }

    pub fn get_enabled(&self, index_name: &TabletIndexName) -> Option<&Index> {
        self.enabled_indexes.get(index_name)
    }

    pub fn get_pending(&self, index_name: &TabletIndexName) -> Option<&Index> {
        self.pending_indexes.get(index_name)
    }

    pub fn must_get_by_id(&self, tablet_id: TabletId) -> anyhow::Result<&Index> {
        self.get_enabled(&TabletIndexName::by_id(tablet_id))
            .ok_or_else(|| anyhow::anyhow!("No `by_id` index for table {}", tablet_id))
    }

    fn insert(&mut self, index: Index) -> Option<Index> {
        let name = index.name();
        if index.metadata.config.is_enabled() {
            self.enabled_indexes.insert(name, index)
        } else {
            self.pending_indexes.insert(name, index)
        }
    }

    fn remove(&mut self, to_remove: &ParsedDocument<TabletIndexMetadata>) {
        let remove_from = if to_remove.config.is_enabled() {
            &mut self.enabled_indexes
        } else {
            &mut self.pending_indexes
        };
        let removed = remove_from.remove(&to_remove.name);
        if let Some(removed) = removed {
            if removed.id() != to_remove.id().internal_id().into() {
                panic!("Tried to remove a different index with the same name");
            }
        } else {
            panic!("Tried to remove a non-existent index, or an index in the wrong state");
        }
    }

    pub fn index_ids(&self) -> BTreeSet<IndexId> {
        self.enabled_indexes
            .iter()
            .chain(self.pending_indexes.iter())
            .map(|(_name, index)| index.id())
            .collect()
    }

}

pub trait IndexedDocument {
    type IndexKey;
    fn id(&self) -> ResolvedDocumentId;
    fn index_key_bytes(&self, fields: &[FieldPath]) -> Self::IndexKey;
}

impl IndexedDocument for ResolvedDocument {
    type IndexKey = IndexKey;

    fn id(&self) -> ResolvedDocumentId {
        self.id()
    }

    fn index_key_bytes(&self, fields: &[FieldPath]) -> IndexKey {
        self.index_key(fields)
    }
}
impl IndexedDocument for PackedDocument {
    type IndexKey = IndexKeyBytes;

    fn id(&self) -> ResolvedDocumentId {
        self.id()
    }

    fn index_key_bytes(&self, fields: &[FieldPath]) -> IndexKeyBytes {
        self.index_key_owned(fields)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub metadata: ParsedDocument<TabletIndexMetadata>,
}

impl TryFrom<&Index> for IndexRef {
    type Error = anyhow::Error;

    fn try_from(index: &Index) -> anyhow::Result<Self> {
        Self::try_from(&index.metadata)
    }
}

impl Index {
    fn new(metadata: ParsedDocument<TabletIndexMetadata>) -> Self {
        Self { metadata }
    }

    pub fn id(&self) -> IndexId {
        self.metadata.id().internal_id().into()
    }

    pub fn name(&self) -> TabletIndexName {
        self.metadata.name.clone()
    }

    pub fn metadata(&self) -> &ParsedDocument<TabletIndexMetadata> {
        &self.metadata
    }
}

pub fn index_backfilling_error(name: &IndexName) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "IndexBackfillingError",
        format!("Index {name} is currently backfilling and not available to query yet."),
    )
}

pub fn index_staged_error(name: &IndexName) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "IndexStagedError",
        format!("Index {name} is currently staged and not available to query until it is enabled."),
    )
}

pub fn index_not_found_error(name: &IndexName) -> ErrorMetadata {
    ErrorMetadata::bad_request("IndexNotFoundError", format!("Index {name} not found."))
}
