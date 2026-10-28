use std::sync::Arc;

use common::{
    bootstrap_model::index::{
        vector_index::{
            FragmentedVectorSegment,
            VectorIndexBackfillState,
            VectorIndexSnapshotData,
            VectorIndexState,
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
    runtime::block_in_place,
    try_anyhow,
    types::{
        IndexId,
        SearchIndexMetricLabels,
        Timestamp,
        WriteTimestamp,
    },
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use futures::{
    future::BoxFuture,
    FutureExt,
};
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
use value::ResolvedDocumentId;

use crate::{
    memory_index::MemoryVectorIndex,
    metrics::{
        self,
        finish_index_manager_update_timer,
        VectorIndexType,
    },
    qdrant_index::QdrantSchema,
    query::{
        InternalVectorSearch,
        VectorSearchQueryResult,
    },
    searcher::VectorSearcher,
    CompiledVectorSearch,
    VectorIndexWriteSize,
};

#[derive(Clone)]
pub struct VectorIndexManager {
    pub indexes: IndexState,
}

#[derive(Clone)]
pub enum IndexState {
    Bootstrapping(OrdMap<IndexId, VectorIndexState>),
    Ready(OrdMap<IndexId, (VectorIndexState, MemoryVectorIndex)>),
}

impl IndexState {
    pub fn insert(
        &mut self,
        id: IndexId,
        state: VectorIndexState,
        memory_index: MemoryVectorIndex,
    ) {
        match self {
            IndexState::Bootstrapping(indexes) => {
                indexes.insert(id, state);
            },
            IndexState::Ready(indexes) => {
                indexes.insert(id, (state, memory_index));
            },
        };
    }

    pub fn update(
        &mut self,
        id: &IndexId,
        new_vector_index_state: Option<VectorIndexState>,
        mutate_memory: impl FnOnce(&mut MemoryVectorIndex) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        match self {
            IndexState::Bootstrapping(indexes) => {
                if let Some(new_vector_index_state) = new_vector_index_state {
                    anyhow::ensure!(
                        indexes.insert(*id, new_vector_index_state).is_some(),
                        format!("Missing vector index for {id}"),
                    );
                }
            },
            IndexState::Ready(indexes) => match indexes.entry(*id) {
                Entry::Vacant(_) => anyhow::bail!("Missing vector index for {id}"),
                Entry::Occupied(ref mut e) => {
                    if let Some(new_vector_index_state) = new_vector_index_state {
                        e.get_mut().0 = new_vector_index_state;
                    }
                    mutate_memory(&mut e.get_mut().1)?;
                },
            },
        }
        Ok(())
    }

    pub fn delete(&mut self, id: &IndexId) {
        match self {
            IndexState::Bootstrapping(indexes) => {
                indexes.remove(id);
            },
            IndexState::Ready(indexes) => {
                indexes.remove(id);
            },
        }
    }
}

fn get_vector_index_states(
    registry: &IndexRegistry,
) -> anyhow::Result<OrdMap<IndexId, VectorIndexState>> {
    let mut indexes = OrdMap::new();

    for index in registry.all_vector_indexes() {
        let IndexConfig::Vector {
            spec: _,
            ref on_disk_state,
        } = index.config
        else {
            continue;
        };
        indexes.insert(index.id().internal_id().into(), on_disk_state.clone());
    }
    Ok(indexes)
}

impl VectorIndexManager {
    pub fn is_bootstrapping(&self) -> bool {
        matches!(self.indexes, IndexState::Bootstrapping(..))
    }

    pub fn bootstrap_index_metadata(registry: &IndexRegistry) -> anyhow::Result<Self> {
        let vector_indexes_and_metadata = get_vector_index_states(registry)?;
        let indexes = IndexState::Bootstrapping(vector_indexes_and_metadata);
        Ok(Self { indexes })
    }

    pub fn backfilled_and_enabled_index_sizes(
        &self,
    ) -> anyhow::Result<impl Iterator<Item = (IndexId, usize)> + '_> {
        Ok(self
            .require_ready_indexes()?
            .iter()
            .map(|(id, (_, idx))| (*id, idx.size())))
    }

    pub fn num_transactions(&self, index_id: IndexId) -> anyhow::Result<Option<usize>> {
        let Some((_, index)) = self.require_ready_indexes()?.get(&index_id) else {
            return Ok(None);
        };
        Ok(Some(index.num_transactions()))
    }

    fn require_ready_indexes(
        &self,
    ) -> anyhow::Result<&OrdMap<IndexId, (VectorIndexState, MemoryVectorIndex)>> {
        if let IndexState::Ready(ref indexes) = self.indexes {
            Ok(indexes)
        } else {
            anyhow::bail!(ErrorMetadata::feature_temporarily_unavailable(
                "VectorIndexesUnavailable",
                "Vector indexes are bootstrapping and not yet available for use",
            ))
        }
    }

    fn require_ready_index(
        &self,
        id: &IndexId,
    ) -> anyhow::Result<Option<&(VectorIndexState, MemoryVectorIndex)>> {
        Ok(self.require_ready_indexes()?.get(id))
    }

    pub fn update(
        &mut self,
        index_registry: &IndexRegistry,
        deletion: Option<&ResolvedDocument>,
        insertion: Option<&ResolvedDocument>,
        ts: WriteTimestamp,
    ) -> anyhow::Result<VectorIndexWriteSize> {
        let mut write_size = VectorIndexWriteSize(0);
        let timer = metrics::index_manager_update_timer();
        let Some(id) = deletion.as_ref().or(insertion.as_ref()).map(|d| d.id()) else {
            finish_index_manager_update_timer(timer, metrics::IndexUpdateType::None);
            return Ok(write_size);
        };
        if self.update_vector_index_metadata(id, index_registry, deletion, insertion, ts)? {
            finish_index_manager_update_timer(timer, metrics::IndexUpdateType::IndexMetadata);
            return Ok(write_size);
        }

        if let IndexState::Ready(..) = self.indexes {
            write_size =
                self.update_vector_index_contents(id, index_registry, deletion, insertion, ts)?;
            finish_index_manager_update_timer(timer, metrics::IndexUpdateType::Document);
            return Ok(write_size);
        }
        finish_index_manager_update_timer(timer, metrics::IndexUpdateType::None);
        Ok(write_size)
    }

    fn update_vector_index_contents(
        &mut self,
        id: ResolvedDocumentId,
        index_registry: &IndexRegistry,
        deletion: Option<&ResolvedDocument>,
        insertion: Option<&ResolvedDocument>,
        ts: WriteTimestamp,
    ) -> anyhow::Result<VectorIndexWriteSize> {
        let mut write_size = VectorIndexWriteSize(0);
        for index in index_registry.vector_indexes_by_table(id.tablet_id) {
            let IndexConfig::Vector { ref spec, .. } = index.metadata.config else {
                continue;
            };
            let qdrant_schema = QdrantSchema::new(spec);
            let old_value = deletion.as_ref().and_then(|d| qdrant_schema.index(d));
            let new_value = insertion.as_ref().and_then(|d| qdrant_schema.index(d));

            // We need to add the size of the document id to the write size because it's
            // also stored in the vector index.
            write_size.0 += (qdrant_schema.estimate_vector_size() + id.size()) as u64;
            self.indexes.update(&index.id(), None, |memory_index| {
                memory_index.update(id.internal_id(), ts, old_value, new_value)
            })?;
        }
        Ok(write_size)
    }

    fn update_vector_index_metadata(
        &mut self,
        id: ResolvedDocumentId,
        index_registry: &IndexRegistry,
        deletion: Option<&ResolvedDocument>,
        insertion: Option<&ResolvedDocument>,
        ts: WriteTimestamp,
    ) -> anyhow::Result<bool> {
        if id.tablet_id != index_registry.index_table() {
            return Ok(false);
        }
        match (deletion, insertion) {
            (None, Some(insertion)) => {
                let metadata = IndexMetadata::try_from(insertion.value().clone().0)?;
                if let IndexConfig::Vector {
                    ref on_disk_state, ..
                } = metadata.config
                {
                    let VectorIndexState::Backfilling(state) = on_disk_state else {
                        anyhow::bail!(
                            "Inserted new search index that wasn't backfilling: {metadata:?}"
                        );
                    };
                    let index = VectorIndexState::Backfilling(state.clone());
                    self.indexes.insert(
                        insertion.id().internal_id().into(),
                        index,
                        MemoryVectorIndex::new(ts),
                    );

                    metrics::log_index_created()
                }
            },
            (Some(prev_version), Some(next_version)) => {
                let prev_metadata: ParsedDocument<IndexMetadata<_>> = prev_version.parse()?;
                let next_metadata: ParsedDocument<IndexMetadata<_>> = next_version.parse()?;
                // We can truncate in-memory indexes as we backfill since we know everything
                // through last_segment_ts is now on disk. Prevents unbounded growth of
                // in-memory indexes.
                if let IndexConfig::Vector {
                    on_disk_state: VectorIndexState::Backfilling(next_backfill_state),
                    ..
                } = &next_metadata.config
                    && let Some(cursor) = &next_backfill_state.cursor
                {
                    let last_segment_ts = cursor.last_segment_ts;
                    self.indexes
                        .update(&id.internal_id().into(), None, |memory_index| {
                            memory_index.truncate(last_segment_ts.succ()?)
                        })?;
                }
                let (old_snapshot, new_snapshot, staged) =
                    match (&prev_metadata.config, &next_metadata.config) {
                        (
                            IndexConfig::Vector {
                                on_disk_state:
                                    VectorIndexState::Backfilling(VectorIndexBackfillState { .. }),
                                ..
                            },
                            IndexConfig::Vector {
                                on_disk_state:
                                    VectorIndexState::Backfilling(VectorIndexBackfillState {
                                        staged,
                                        ..
                                    }),
                                ..
                            },
                        ) => (None, None, *staged),
                        (
                            IndexConfig::Vector {
                                on_disk_state: VectorIndexState::Backfilling { .. },
                                ..
                            },
                            IndexConfig::Vector {
                                on_disk_state: VectorIndexState::Backfilled { snapshot, staged },
                                ..
                            },
                        ) => (None, Some(snapshot), *staged),
                        (
                            IndexConfig::Vector {
                                on_disk_state:
                                    VectorIndexState::Backfilled {
                                        snapshot: old_snapshot,
                                        ..
                                    },
                                ..
                            },
                            IndexConfig::Vector {
                                on_disk_state: VectorIndexState::SnapshottedAt(new_snapshot),
                                ..
                            },
                        ) => (Some(old_snapshot), Some(new_snapshot), false),
                        (
                            IndexConfig::Vector {
                                on_disk_state:
                                    VectorIndexState::Backfilled {
                                        snapshot: old_snapshot,
                                        ..
                                    },
                                ..
                            },
                            IndexConfig::Vector {
                                on_disk_state:
                                    VectorIndexState::Backfilled {
                                        snapshot: new_snapshot,
                                        staged,
                                    },
                                ..
                            },
                        ) => (Some(old_snapshot), Some(new_snapshot), *staged),
                        (
                            IndexConfig::Vector {
                                on_disk_state: VectorIndexState::SnapshottedAt(old_snapshot),
                                ..
                            },
                            IndexConfig::Vector {
                                on_disk_state: VectorIndexState::SnapshottedAt(new_snapshot),
                                ..
                            },
                        ) => (Some(old_snapshot), Some(new_snapshot), false),
                        (
                            IndexConfig::Vector {
                                on_disk_state: VectorIndexState::SnapshottedAt(old_snapshot),
                                ..
                            },
                            IndexConfig::Vector {
                                on_disk_state:
                                    VectorIndexState::Backfilled {
                                        snapshot: new_snapshot,
                                        staged,
                                    },
                                ..
                            },
                        ) => {
                            anyhow::ensure!(
                                old_snapshot == new_snapshot,
                                "Snapshot mismatch when disabling vector index"
                            );
                            anyhow::ensure!(staged, "Disabled vector index must be staged");
                            (Some(old_snapshot), Some(new_snapshot), *staged)
                        },
                        (IndexConfig::Vector { .. }, _) | (_, IndexConfig::Vector { .. }) => {
                            anyhow::bail!(
                                "Invalid index type transition: {prev_metadata:?} to \
                                 {next_metadata:?}"
                            );
                        },
                        _ => (None, None, false),
                    };
                if let Some(new_snapshot) = new_snapshot {
                    let is_newly_enabled =
                        !prev_metadata.config.is_enabled() && next_metadata.config.is_enabled();
                    let is_updated_snapshot = if let Some(old_snapshot) = old_snapshot {
                        old_snapshot.ts < new_snapshot.ts
                    } else {
                        true
                    };

                    if is_newly_enabled || is_updated_snapshot {
                        let is_next_index_enabled = next_metadata.config.is_enabled();
                        let updated_state = if is_next_index_enabled {
                            VectorIndexState::SnapshottedAt(new_snapshot.clone())
                        } else {
                            VectorIndexState::Backfilled {
                                snapshot: new_snapshot.clone(),
                                staged,
                            }
                        };

                        self.indexes.update(
                            &id.internal_id().into(),
                            Some(updated_state),
                            |memory_index| memory_index.truncate(new_snapshot.ts.succ()?),
                        )?;

                        if !prev_metadata.into_value().config.is_enabled() && is_next_index_enabled
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
                if metadata.is_vector_index() {
                    self.indexes.delete(&deletion.id().internal_id().into());
                    metrics::log_index_deleted();
                }
            },
            _ => panic!("Had neither a deletion nor insertion despite checking above"),
        }
        Ok(true)
    }

    pub async fn vector_search(
        &self,
        index: &Index,
        query: InternalVectorSearch,
        searcher: Arc<dyn VectorSearcher>,
        search_storage: Arc<dyn Storage>,
    ) -> anyhow::Result<Vec<VectorSearchQueryResult>> {
        let timer = metrics::search_timer(&SEARCHLIGHT_CLUSTER_NAME);
        let result: anyhow::Result<_> = try_anyhow!({
            let IndexConfig::Vector { ref spec, .. } = index.metadata.config else {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "IndexNotAVectorIndexError",
                    format!(
                        "Index {} is not a vector index",
                        query.printable_index_name()?
                    )
                ));
            };
            let Some((vector_index, memory_index)) = self.require_ready_index(&index.id())? else {
                anyhow::bail!("Vector index {:?} not available", index.id());
            };
            let qdrant_schema = QdrantSchema::new(spec);
            let VectorIndexState::SnapshottedAt(snapshot) = vector_index else {
                anyhow::bail!(index_backfilling_error(&query.printable_index_name()?));
            };
            let metric_labels =
                SearchIndexMetricLabels::new(Some(index.id()), None::<&str>).to_owned();
            let (disk_revisions, vector_index_type) = match snapshot.data {
                VectorIndexSnapshotData::Unknown(_) => {
                    anyhow::bail!(index_backfilling_error(&query.printable_index_name()?))
                },
                VectorIndexSnapshotData::MultiSegment(ref segments) => (
                    self.multi_segment_search(
                        query,
                        searcher,
                        segments,
                        search_storage,
                        qdrant_schema,
                        memory_index,
                        snapshot.ts,
                        metric_labels,
                    )
                    .await?,
                    VectorIndexType::MultiSegment,
                ),
            };
            (disk_revisions, vector_index_type)
        });
        match result {
            Ok((disk_revisions, vector_index_type)) => {
                metrics::finish_search(timer, &disk_revisions, vector_index_type);
                Ok(disk_revisions)
            },
            Err(e) => {
                if e.is_bad_request() {
                    timer.finish_developer_error();
                }
                Err(e)
            },
        }
    }

    async fn multi_segment_search(
        &self,
        query: InternalVectorSearch,
        searcher: Arc<dyn VectorSearcher>,
        segments: &Vec<FragmentedVectorSegment>,
        search_storage: Arc<dyn Storage>,
        qdrant_schema: QdrantSchema,
        memory_index: &MemoryVectorIndex,
        ts: Timestamp,
        metric_labels: SearchIndexMetricLabels<'static>,
    ) -> anyhow::Result<Vec<VectorSearchQueryResult>> {
        self.compile_search_and_truncate(
            query,
            qdrant_schema,
            memory_index,
            ts,
            metric_labels,
            |qdrant_schema, compiled_query, overfetch_delta, labels| {
                async move {
                    let timer = metrics::searchlight_client_execute_timer(
                        VectorIndexType::MultiSegment,
                        &SEARCHLIGHT_CLUSTER_NAME,
                    );
                    let total_segments = segments.len();
                    let metric_labels = labels;
                    let results = searcher
                        .execute_multi_segment_vector_query(
                            search_storage.clone(),
                            segments
                                .iter()
                                .cloned()
                                .map(|segment| segment.to_paths_proto())
                                .try_collect()?,
                            qdrant_schema,
                            compiled_query.clone(),
                            overfetch_delta as u32,
                            metric_labels,
                        )
                        .await?;
                    metrics::log_num_segments_searched_total(total_segments);
                    metrics::finish_searchlight_client_execute(timer, &results);
                    Ok(results)
                }
                .boxed()
            },
        )
        .await
    }

    async fn compile_search_and_truncate<'a>(
        &'a self,
        query: InternalVectorSearch,
        qdrant_schema: QdrantSchema,
        memory_index: &MemoryVectorIndex,
        ts: Timestamp,
        metric_labels: SearchIndexMetricLabels<'static>,
        call_searchlight: impl FnOnce(
            QdrantSchema,
            CompiledVectorSearch,
            usize,
            SearchIndexMetricLabels<'static>,
        )
            -> BoxFuture<'a, anyhow::Result<Vec<VectorSearchQueryResult>>>,
    ) -> anyhow::Result<Vec<VectorSearchQueryResult>> {
        let compiled_query = qdrant_schema.compile(query)?;
        let updated_matches = memory_index.updated_matches(ts, &compiled_query)?;
        let overfetch_delta = updated_matches.len();
        metrics::log_searchlight_overfetch_delta(overfetch_delta);
        let mut disk_revisions = call_searchlight(
            qdrant_schema,
            compiled_query.clone(),
            overfetch_delta,
            metric_labels,
        )
        .await?;

        block_in_place(|| {
            // Filter out revisions that are no longer latest.
            disk_revisions.retain(|r| !updated_matches.contains(&r.id));

            let memory_revisions = memory_index.query(ts, &compiled_query)?;

            disk_revisions.extend(memory_revisions);
            let original_len = disk_revisions.len();
            disk_revisions.sort_by(|a, b| a.cmp(b).reverse());
            disk_revisions.truncate(compiled_query.limit as usize);
            metrics::log_num_discarded_revisions(original_len - disk_revisions.len());

            anyhow::Ok(())
        })?;

        Ok(disk_revisions)
    }

    pub fn total_in_memory_size(&self) -> usize {
        if let IndexState::Ready(ref indexes) = self.indexes {
            indexes
                .iter()
                .map(|(_, (_, memory_index))| memory_index.size())
                .sum()
        } else {
            0
        }
    }

    pub fn in_memory_sizes(&self) -> Vec<(IndexId, usize)> {
        if let IndexState::Ready(ref indexes) = self.indexes {
            indexes.iter().map(|(id, (_, s))| (*id, s.size())).collect()
        } else {
            vec![]
        }
    }

    /// In-memory index sizes used to decide whether to reject writes that are
    /// too large. Excludes backfilling indexes: they aren't queryable, their
    /// in-memory contents are kept bounded by the backfill flusher (which
    /// truncates the memory index as it persists progress), and they should
    /// never block writes to the table.
    pub fn flushable_in_memory_index_sizes(&self) -> Vec<(IndexId, usize)> {
        if let IndexState::Ready(ref indexes) = self.indexes {
            indexes
                .iter()
                .filter(|(_, (state, _))| !matches!(state, VectorIndexState::Backfilling(_)))
                .map(|(id, (_, s))| (*id, s.size()))
                .collect()
        } else {
            vec![]
        }
    }
}
