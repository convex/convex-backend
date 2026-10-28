use std::sync::Arc;

use common::{
    bootstrap_model::index::{
        text_index::{
            FragmentedTextSegment,
            TextIndexSnapshot,
            TextIndexSnapshotData,
            TextIndexState,
            TextSnapshotVersion,
        },
        IndexConfig,
        IndexMetadata,
    },
    document::{
        ParseDocument,
        ParsedDocument,
        ResolvedDocument,
    },
    knobs::SEARCHLIGHT_CLUSTER_NAME,
    query::{
        InternalSearch,
        SearchVersion,
    },
    types::{
        IndexId,
        IndexName,
        SearchIndexMetricLabels,
        Timestamp,
        WriteTimestamp,
    },
};
use errors::ErrorMetadata;
use imbl::{
    ordmap::Entry,
    OrdMap,
};
use indexing::index_registry::{
    index_backfilling_error,
    Index,
    IndexRegistry,
};
use storage::Storage;

use crate::{
    memory_index::MemoryTextIndex,
    metrics,
    query::CompiledQuery,
    QueryResults,
    Searcher,
    TantivySearchIndexSchema,
    TextIndexWriteSize,
    TextSearchResults,
};

#[derive(Clone)]
pub struct SnapshotInfo {
    pub disk_index: DiskIndex,
    pub disk_index_ts: Timestamp,
    pub disk_index_version: TextSnapshotVersion,
    pub memory_index: MemoryTextIndex,
}

#[derive(Clone)]
pub struct DiskIndex(Vec<FragmentedTextSegment>);

impl TryFrom<TextIndexSnapshotData> for DiskIndex {
    type Error = anyhow::Error;

    fn try_from(value: TextIndexSnapshotData) -> Result<Self, Self::Error> {
        Ok(match value {
            TextIndexSnapshotData::MultiSegment(segments) => Self(segments),
            TextIndexSnapshotData::Unknown(_) => anyhow::bail!("Unrecognized state: {:?}", value),
        })
    }
}

#[derive(Clone)]
pub enum TextIndex {
    Backfilling { memory_index: MemoryTextIndex },
    Backfilled(SnapshotInfo),
    Ready(SnapshotInfo),
}

impl TextIndex {
    fn memory_index(&self) -> &MemoryTextIndex {
        match self {
            TextIndex::Backfilling { memory_index } => memory_index,
            TextIndex::Backfilled(SnapshotInfo { memory_index, .. }) => memory_index,
            TextIndex::Ready(SnapshotInfo { memory_index, .. }) => memory_index,
        }
    }

    pub fn memory_index_mut(&mut self) -> &mut MemoryTextIndex {
        match self {
            TextIndex::Backfilling { memory_index } => memory_index,
            TextIndex::Backfilled(SnapshotInfo { memory_index, .. }) => memory_index,
            TextIndex::Ready(SnapshotInfo { memory_index, .. }) => memory_index,
        }
    }
}

#[derive(Clone)]
pub struct TextIndexManager {
    indexes: TextIndexManagerState,
}

#[derive(Clone)]
pub enum TextIndexManagerState {
    Bootstrapping,
    Ready(OrdMap<IndexId, TextIndex>),
}

impl TextIndexManager {
    pub fn is_bootstrapping(&self) -> bool {
        matches!(self.indexes, TextIndexManagerState::Bootstrapping)
    }

    pub fn new(indexes: TextIndexManagerState) -> Self {
        Self { indexes }
    }

    fn require_ready_indexes(&self) -> anyhow::Result<&OrdMap<IndexId, TextIndex>> {
        match self.indexes {
            TextIndexManagerState::Bootstrapping => {
                anyhow::bail!(ErrorMetadata::feature_temporarily_unavailable(
                    "SearchIndexesUnavailable",
                    "Search indexes bootstrapping and not yet available for use"
                ));
            },
            TextIndexManagerState::Ready(ref indexes) => Ok(indexes),
        }
    }

    fn get_snapshot_info(
        &self,
        index: &Index,
        printable_index_name: &IndexName,
    ) -> anyhow::Result<&SnapshotInfo> {
        let indexes = self.require_ready_indexes()?;
        let index = if let Some(index) = indexes.get(&index.id()) {
            index
        } else {
            anyhow::bail!("Search index {:?} not available", index.id());
        };

        match index {
            TextIndex::Backfilling { .. } | TextIndex::Backfilled(_) => {
                anyhow::bail!(index_backfilling_error(printable_index_name));
            },
            TextIndex::Ready(snapshot_info) => {
                anyhow::ensure!(
                    // If the search index was written to disk with a different format from
                    // how the current backend constructs search queries, assume the new
                    // search index is backfilling.
                    snapshot_info.disk_index_version == TextSnapshotVersion::current(),
                    index_backfilling_error(printable_index_name)
                );
                Ok(snapshot_info)
            },
        }
    }

    pub async fn search(
        &self,
        index: &Index,
        search: &InternalSearch,
        searcher: Arc<dyn Searcher>,
        search_storage: Arc<dyn Storage>,
        version: SearchVersion,
    ) -> anyhow::Result<QueryResults> {
        let timer = metrics::search_timer(&SEARCHLIGHT_CLUSTER_NAME);
        let tantivy_schema =
            TantivySearchIndexSchema::new_for_index(index, &search.printable_index_name()?)?;
        let (compiled_query, reads) = tantivy_schema.compile(search, version)?;

        let TextSearchResults {
            revisions_with_keys,
            filtered_bytes_searched,
        } = self
            .run_compiled_query(
                index,
                &search.printable_index_name()?,
                tantivy_schema,
                compiled_query,
                searcher,
                search_storage,
            )
            .await?;

        let results = QueryResults {
            revisions_with_keys,
            reads,
            filtered_bytes_searched,
        };
        metrics::finish_search(timer, &results.revisions_with_keys);
        Ok(results)
    }

    pub async fn text_search(
        &self,
        index: &Index,
        printable_index_name: &IndexName,
        query: pb::searchlight::TextQuery,
        searcher: Arc<dyn Searcher>,
        search_storage: Arc<dyn Storage>,
    ) -> anyhow::Result<TextSearchResults> {
        let timer = metrics::search_timer(&SEARCHLIGHT_CLUSTER_NAME);
        let tantivy_schema = TantivySearchIndexSchema::new_for_index(index, printable_index_name)?;
        let compiled_query =
            CompiledQuery::try_from_text_query_proto(query, tantivy_schema.search_field)?;

        let results = self
            .run_compiled_query(
                index,
                printable_index_name,
                tantivy_schema,
                compiled_query,
                searcher,
                search_storage,
            )
            .await?;
        metrics::finish_search(timer, &results.revisions_with_keys);
        Ok(results)
    }

    async fn run_compiled_query(
        &self,
        index: &Index,
        printable_index_name: &IndexName,
        tantivy_schema: TantivySearchIndexSchema,
        compiled_query: CompiledQuery,
        searcher: Arc<dyn Searcher>,
        search_storage: Arc<dyn Storage>,
    ) -> anyhow::Result<TextSearchResults> {
        // Ignore empty searches to avoid failures due to transient search
        // issues (e.g. bootstrapping).
        if compiled_query.is_empty() {
            tracing::debug!("Skipping empty search query");
            return Ok(TextSearchResults {
                revisions_with_keys: vec![],
                filtered_bytes_searched: 0,
            });
        }
        let SnapshotInfo {
            disk_index,
            disk_index_ts,
            memory_index,
            ..
        } = self.get_snapshot_info(index, printable_index_name)?;
        let metric_labels = SearchIndexMetricLabels::new(Some(index.id()), None::<&str>);
        tantivy_schema
            .search(
                compiled_query,
                memory_index,
                search_storage,
                disk_index.0.clone(),
                *disk_index_ts,
                searcher,
                metric_labels,
            )
            .await
    }

    pub fn backfilled_and_enabled_index_sizes(
        &self,
    ) -> anyhow::Result<impl Iterator<Item = (IndexId, usize)> + '_> {
        Ok(self
            .require_ready_indexes()?
            .iter()
            .filter_map(|(id, idx)| {
                let SnapshotInfo { memory_index, .. } = match idx {
                    TextIndex::Backfilled(snapshot) => snapshot,
                    TextIndex::Ready(snapshot) => snapshot,
                    TextIndex::Backfilling { .. } => return None,
                };
                Some((*id, memory_index.size()))
            }))
    }

    pub fn num_transactions(&self, index_id: IndexId) -> anyhow::Result<Option<usize>> {
        let Some(index) = self.require_ready_indexes()?.get(&index_id) else {
            return Ok(None);
        };
        let SnapshotInfo { memory_index, .. } = match index {
            TextIndex::Ready(snapshot) => snapshot,
            TextIndex::Backfilled(snapshot) => snapshot,
            TextIndex::Backfilling { .. } => return Ok(None),
        };

        Ok(Some(memory_index.num_transactions()))
    }

    pub fn update(
        &mut self,
        index_registry: &IndexRegistry,
        deletion: Option<&ResolvedDocument>,
        insertion: Option<&ResolvedDocument>,
        ts: WriteTimestamp,
    ) -> anyhow::Result<TextIndexWriteSize> {
        let mut write_size = TextIndexWriteSize(0);
        let TextIndexManagerState::Ready(ref mut indexes) = self.indexes else {
            return Ok(write_size);
        };
        let Some(id) = deletion.as_ref().or(insertion.as_ref()).map(|d| d.id()) else {
            return Ok(write_size);
        };
        let timer = metrics::index_manager_update_timer();

        // Handle index maintenance operations first.
        // TODO: Move this validation to `IndexRegistry` and make this function
        // infallible.
        if id.tablet_id == index_registry.index_table() {
            match (deletion, insertion) {
                (None, Some(insertion)) => {
                    let metadata = IndexMetadata::try_from(insertion.value().clone().0)?;
                    if let IndexConfig::Text {
                        ref on_disk_state, ..
                    } = metadata.config
                    {
                        let TextIndexState::Backfilling(_) = on_disk_state else {
                            anyhow::bail!(
                                "Inserted new search index that wasn't backfilling: {metadata:?}"
                            );
                        };
                        let memory_index = MemoryTextIndex::new(ts);
                        let index = TextIndex::Backfilling { memory_index };
                        indexes.insert(insertion.id().internal_id().into(), index);

                        metrics::log_index_created();
                    }
                },
                (Some(prev_version), Some(next_version)) => {
                    let prev_metadata: ParsedDocument<IndexMetadata<_>> = prev_version.parse()?;
                    let next_metadata: ParsedDocument<IndexMetadata<_>> = next_version.parse()?;
                    // We can truncate in-memory indexes as we backfill since we know everything
                    // through last_segment_ts is now on disk. Prevents unbounded growth of
                    // in-memory indexes.
                    if let IndexConfig::Text {
                        on_disk_state: TextIndexState::Backfilling(next_backfill_state),
                        ..
                    } = &next_metadata.config
                        && let Some(cursor) = &next_backfill_state.cursor
                        && let Some(index) = indexes.get_mut(&id.internal_id().into())
                    {
                        index
                            .memory_index_mut()
                            .truncate(cursor.last_segment_ts.succ()?)?;
                    }
                    let (old_snapshot, new_snapshot) =
                        match (&prev_metadata.config, &next_metadata.config) {
                            (
                                IndexConfig::Text {
                                    on_disk_state: TextIndexState::Backfilling { .. },
                                    ..
                                },
                                IndexConfig::Text {
                                    on_disk_state: TextIndexState::Backfilling { .. },
                                    ..
                                },
                            ) => (None, None),
                            (
                                IndexConfig::Text {
                                    on_disk_state: TextIndexState::Backfilling { .. },
                                    ..
                                },
                                IndexConfig::Text {
                                    on_disk_state:
                                        TextIndexState::Backfilled {
                                            snapshot,
                                            staged: _,
                                        },
                                    ..
                                },
                            ) => (None, Some(snapshot)),
                            (
                                IndexConfig::Text {
                                    on_disk_state:
                                        TextIndexState::Backfilled {
                                            snapshot: old_snapshot,
                                            staged: _,
                                        },
                                    ..
                                },
                                IndexConfig::Text {
                                    on_disk_state: TextIndexState::SnapshottedAt(new_snapshot),
                                    ..
                                },
                            ) => (Some(old_snapshot), Some(new_snapshot)),
                            (
                                IndexConfig::Text {
                                    on_disk_state:
                                        TextIndexState::Backfilled {
                                            snapshot: old_snapshot,
                                            staged: _,
                                        },
                                    ..
                                },
                                IndexConfig::Text {
                                    on_disk_state:
                                        TextIndexState::Backfilled {
                                            snapshot: new_snapshot,
                                            staged: _,
                                        },
                                    ..
                                },
                            ) => (Some(old_snapshot), Some(new_snapshot)),
                            (
                                IndexConfig::Text {
                                    on_disk_state: TextIndexState::SnapshottedAt(old_snapshot),
                                    ..
                                },
                                IndexConfig::Text {
                                    on_disk_state: TextIndexState::SnapshottedAt(new_snapshot),
                                    ..
                                },
                            ) => (Some(old_snapshot), Some(new_snapshot)),
                            (
                                IndexConfig::Text {
                                    on_disk_state: TextIndexState::SnapshottedAt(old_snapshot),
                                    ..
                                },
                                IndexConfig::Text {
                                    on_disk_state:
                                        TextIndexState::Backfilled {
                                            snapshot: new_snapshot,
                                            staged,
                                        },
                                    ..
                                },
                            ) => {
                                anyhow::ensure!(
                                    old_snapshot == new_snapshot,
                                    "Snapshot mismatch when disabling text index"
                                );
                                anyhow::ensure!(staged, "Disabled text index must be staged");
                                (Some(old_snapshot), Some(new_snapshot))
                            },
                            (IndexConfig::Text { .. }, _) | (_, IndexConfig::Text { .. }) => {
                                anyhow::bail!(
                                    "Invalid index type transition: {prev_metadata:?} to \
                                     {next_metadata:?}"
                                );
                            },
                            _ => (None, None),
                        };
                    if let Some(TextIndexSnapshot {
                        data: disk_index,
                        ts: disk_index_ts,
                        version: disk_index_version,
                    }) = new_snapshot
                    {
                        let is_newly_enabled =
                            !prev_metadata.config.is_enabled() && next_metadata.config.is_enabled();
                        let is_updated_snapshot = if let Some(old_snapshot) = old_snapshot {
                            old_snapshot.ts < *disk_index_ts
                        } else {
                            true
                        };

                        if is_newly_enabled || is_updated_snapshot {
                            let mut entry = match indexes.entry(id.internal_id().into()) {
                                Entry::Occupied(e) => e,
                                Entry::Vacant(..) => anyhow::bail!("Missing index for {id}"),
                            };
                            let memory_index = match entry.get() {
                                TextIndex::Backfilling { memory_index } => memory_index,
                                TextIndex::Backfilled(SnapshotInfo { memory_index, .. }) => {
                                    memory_index
                                },
                                TextIndex::Ready(SnapshotInfo { memory_index, .. }) => memory_index,
                            };

                            if let Some(old_snapshot) = old_snapshot {
                                anyhow::ensure!(old_snapshot.ts <= disk_index_ts);
                            } else {
                                anyhow::ensure!(
                                    memory_index.min_ts()
                                        <= WriteTimestamp::Committed(*disk_index_ts)
                                );
                            }

                            let disk_index = DiskIndex::try_from(disk_index.clone())?;
                            let mut memory_index = memory_index.clone();
                            memory_index.truncate(disk_index_ts.succ()?)?;
                            let snapshot = SnapshotInfo {
                                disk_index,
                                disk_index_ts: *disk_index_ts,
                                disk_index_version: *disk_index_version,
                                memory_index,
                            };

                            let is_next_index_enabled =
                                next_metadata.into_value().config.is_enabled();
                            *entry.get_mut() = if is_next_index_enabled {
                                TextIndex::Ready(snapshot)
                            } else {
                                TextIndex::Backfilled(snapshot)
                            };

                            if !prev_metadata.into_value().config.is_enabled()
                                && is_next_index_enabled
                            {
                                metrics::log_index_backfilled();
                            } else {
                                metrics::log_index_advanced();
                            }
                        }
                    }
                },
                (Some(deletion), None) => {
                    let metadata: ParsedDocument<IndexMetadata<_>> = deletion.parse()?;
                    if metadata.is_text_index() {
                        indexes.remove(&deletion.id().internal_id().into());
                        metrics::log_index_deleted();
                    }
                },
                _ => panic!("Had neither a deletion nor insertion despite checking above"),
            }
        }

        // Handle index updates for our existing search indexes.
        for index in index_registry.text_indexes_by_table(id.tablet_id) {
            let IndexConfig::Text { ref spec, .. } = index.metadata.config else {
                continue;
            };
            let tantivy_schema = TantivySearchIndexSchema::new(spec);
            let Some(index) = indexes.get_mut(&index.id()) else {
                continue;
            };
            let old_value = deletion
                .as_ref()
                .map(|d| anyhow::Ok((tantivy_schema.index_into_terms(d)?, d.creation_time())))
                .transpose()?;
            let new_terms = insertion
                .as_ref()
                .map(|d| anyhow::Ok((tantivy_schema.index_into_terms(d)?, d.creation_time())))
                .transpose()?;
            index
                .memory_index_mut()
                .update(id.internal_id(), ts, old_value, new_terms)?;
            if let Some(insertion) = insertion {
                write_size.0 += tantivy_schema.estimate_size(insertion);
            }
        }

        timer.finish();
        Ok(write_size)
    }

    pub fn total_in_memory_size(&self) -> usize {
        self.in_memory_sizes().iter().map(|(_, s)| s).sum()
    }

    pub fn in_memory_sizes(&self) -> Vec<(IndexId, usize)> {
        match self.indexes {
            TextIndexManagerState::Bootstrapping => vec![],
            TextIndexManagerState::Ready(ref indexes) => indexes
                .iter()
                .map(|(id, s)| (*id, s.memory_index().size()))
                .collect(),
        }
    }

    /// In-memory index sizes used to decide whether to reject writes that are
    /// too large. Excludes backfilling indexes: they aren't queryable, their
    /// in-memory contents are kept bounded by the backfill flusher (which
    /// truncates the memory index as it persists progress), and they should
    /// never block writes to the table.
    pub fn flushable_in_memory_index_sizes(&self) -> Vec<(IndexId, usize)> {
        match self.indexes {
            TextIndexManagerState::Bootstrapping => vec![],
            TextIndexManagerState::Ready(ref indexes) => indexes
                .iter()
                .filter(|(_, s)| !matches!(s, TextIndex::Backfilling { .. }))
                .map(|(id, s)| (*id, s.memory_index().size()))
                .collect(),
        }
    }

    pub fn consistency_check(&self) -> anyhow::Result<()> {
        let indexes = self.require_ready_indexes()?;
        for index in indexes.values() {
            index.memory_index().consistency_check()?;
        }
        Ok(())
    }

}
