use std::{
    cmp::{
        max,
        min,
    },
    collections::{
        BTreeMap,
        BTreeSet,
    },
    ops::Bound,
    time::Duration,
};

use common::{
    bootstrap_model::index::{
        text_index::{
            TextIndexSnapshot,
            TextIndexState,
        },
        vector_index::VectorIndexState,
        IndexConfig,
        TabletIndexMetadata,
    },
    document::ParsedDocument,
    errors::report_error,
    knobs::UDF_EXECUTOR_OCC_MAX_RETRIES,
    persistence::{
        RepeatablePersistence,
        TimestampRange,
    },
    persistence_helpers::RevisionPair,
    query::Order,
    runtime::{
        try_join_buffer_unordered,
        Runtime,
    },
    types::{
        IndexId,
        RepeatableTimestamp,
        WriteTimestamp,
    },
};
use errors::ErrorMetadataAnyhowExt;
use futures::{
    future,
    Stream,
    TryStreamExt,
};
use indexing::index_registry::IndexRegistry;
use search::{
    DiskIndex,
    MemoryTextIndex,
    SnapshotInfo,
    TantivySearchIndexSchema,
    TextIndex,
    TextIndexManager,
    TextIndexManagerState,
};
use sync_types::{
    backoff::Backoff,
    Timestamp,
};
use value::{
    NamespacedTableMapping,
    TabletId,
};
use vector::{
    IndexState,
    MemoryVectorIndex,
    QdrantSchema,
    VectorIndexManager,
};

use crate::{
    bootstrap_model::index_workers::load_metadata_fast_forward_ts,
    committer::CommitterClient,
    metrics::log_document_skipped,
};

pub const FINISHED_BOOTSTRAP_UPDATES: &str = "finished_bootstrap_updates";

pub struct SearchIndexBootstrapWorker<RT: Runtime> {
    runtime: RT,
    index_registry: IndexRegistry,
    persistence: RepeatablePersistence,
    table_mapping: NamespacedTableMapping,
    committer_client: CommitterClient,
    backoff: Backoff,
}

const INITIAL_BACKOFF: Duration = Duration::from_millis(10);
const MAX_BACKOFF: Duration = Duration::from_secs(5);

struct IndexesToBootstrap {
    table_to_text_indexes: BTreeMap<TabletId, Vec<TextIndexBootstrapData>>,
    table_to_vector_indexes: BTreeMap<TabletId, Vec<VectorIndexBootstrapData>>,
    /// Timestamp to walk the document log from to get all of the revisions
    /// since the last write to disk.
    oldest_index_ts: Timestamp,
}

pub struct BootstrappedSearchIndexes {
    pub text_index_manager: TextIndexManager,
    pub vector_index_manager: VectorIndexManager,
    pub tables_with_indexes: BTreeSet<TabletId>,
}

impl IndexesToBootstrap {
    fn create(
        upper_bound: RepeatableTimestamp,
        indexes_with_fast_forward_ts: Vec<(ParsedDocument<TabletIndexMetadata>, Option<Timestamp>)>,
    ) -> anyhow::Result<Self> {
        let mut table_to_vector_indexes: BTreeMap<_, Vec<_>> = BTreeMap::new();
        let mut table_to_text_indexes: BTreeMap<_, Vec<_>> = BTreeMap::new();
        // We keep track of latest ts we can bootstrap from for each vector index.
        let mut oldest_index_ts = *upper_bound;

        for (index_doc, fast_forward_ts) in indexes_with_fast_forward_ts {
            let is_enabled = index_doc.config.is_enabled();
            let (index_id, index_metadata) = index_doc.into_id_and_value();
            match index_metadata.config {
                IndexConfig::Vector {
                    on_disk_state,
                    ref spec,
                    ..
                } => {
                    let qdrant_schema = QdrantSchema::new(spec);
                    let ts = match on_disk_state {
                        VectorIndexState::Backfilled {
                            snapshot: ref snapshot_info,
                            ..
                        }
                        | VectorIndexState::SnapshottedAt(ref snapshot_info) => {
                            // Use fast forward ts instead of snapshot ts.
                            let current_index_ts =
                                max(fast_forward_ts.unwrap_or_default(), snapshot_info.ts);
                            oldest_index_ts = min(oldest_index_ts, current_index_ts);
                            snapshot_info.ts
                        },
                        VectorIndexState::Backfilling(_) => upper_bound.succ()?,
                    };
                    let vector_index_bootstrap_data = VectorIndexBootstrapData {
                        index_id: index_id.internal_id().into(),
                        on_disk_state,
                        memory_index: MemoryVectorIndex::new(WriteTimestamp::Committed(ts.succ()?)),
                        qdrant_schema,
                    };
                    if let Some(vector_indexes) =
                        table_to_vector_indexes.get_mut(index_metadata.name.table())
                    {
                        vector_indexes.push(vector_index_bootstrap_data);
                    } else {
                        table_to_vector_indexes.insert(
                            *index_metadata.name.table(),
                            vec![vector_index_bootstrap_data],
                        );
                    }
                },
                IndexConfig::Text {
                    ref spec,
                    on_disk_state,
                } => {
                    let text_index = match on_disk_state {
                        TextIndexState::Backfilling(_) => {
                            // We'll start a new memory search index starting at the next commit
                            // after our persistence upper bound. After
                            // bootstrapping, all commits after
                            // `persistence.upper_bound()` will flow through `Self::update`, so our
                            // memory index contains all revisions `>=
                            // persistence.upper_bound().succ()?`.
                            let memory_index = MemoryTextIndex::new(WriteTimestamp::Committed(
                                upper_bound.succ()?,
                            ));
                            TextIndex::Backfilling { memory_index }
                        },
                        TextIndexState::Backfilled {
                            snapshot:
                                TextIndexSnapshot {
                                    data,
                                    ts: disk_ts,
                                    version,
                                },
                            staged: _,
                        }
                        | TextIndexState::SnapshottedAt(TextIndexSnapshot {
                            data,
                            ts: disk_ts,
                            version,
                        }) => {
                            let current_index_ts =
                                max(disk_ts, fast_forward_ts.unwrap_or_default());
                            oldest_index_ts = min(oldest_index_ts, current_index_ts);
                            let memory_index =
                                MemoryTextIndex::new(WriteTimestamp::Committed(disk_ts.succ()?));
                            let snapshot = SnapshotInfo {
                                disk_index: DiskIndex::try_from(data)?,
                                disk_index_ts: current_index_ts,
                                disk_index_version: version,
                                memory_index,
                            };
                            if is_enabled {
                                TextIndex::Ready(snapshot)
                            } else {
                                TextIndex::Backfilled(snapshot)
                            }
                        },
                    };
                    let tantivy_schema = TantivySearchIndexSchema::new(spec);
                    let text_index_bootstrap_data = TextIndexBootstrapData {
                        index_id: index_id.internal_id().into(),
                        text_index,
                        tantivy_schema,
                    };
                    if let Some(text_indexes) =
                        table_to_text_indexes.get_mut(index_metadata.name.table())
                    {
                        text_indexes.push(text_index_bootstrap_data);
                    } else {
                        table_to_text_indexes.insert(
                            *index_metadata.name.table(),
                            vec![text_index_bootstrap_data],
                        );
                    }
                },
                _ => continue,
            };
        }
        Ok(Self {
            table_to_text_indexes,
            table_to_vector_indexes,
            oldest_index_ts,
        })
    }

    fn tables_with_indexes(&self) -> BTreeSet<TabletId> {
        self.table_to_text_indexes
            .keys()
            .chain(self.table_to_vector_indexes.keys())
            .copied()
            .collect()
    }

    async fn bootstrap(
        mut self,
        persistence: &RepeatablePersistence,
    ) -> anyhow::Result<BootstrappedSearchIndexes> {
        let timer = crate::metrics::bootstrap_timer();
        let upper_bound = persistence.upper_bound();
        let mut num_revisions = 0;
        let mut total_size = 0;

        let range = TimestampRange::new((
            Bound::Excluded(self.oldest_index_ts),
            Bound::Included(*upper_bound),
        ));
        let tables_with_indexes = self.tables_with_indexes();
        let revision_stream =
            stream_revision_pairs_for_indexes(&tables_with_indexes, persistence, range);
        futures::pin_mut!(revision_stream);

        tracing::info!(
            "Starting search index bootstrap at {} with upper bound {}",
            self.oldest_index_ts,
            upper_bound
        );
        while let Some(revision_pair) = revision_stream.try_next().await? {
            num_revisions += 1;
            total_size += revision_pair.document().map(|d| d.size()).unwrap_or(0);
            if let Some(vector_indexes_to_update) = self
                .table_to_vector_indexes
                .get_mut(&revision_pair.id.table())
            {
                for vector_index in vector_indexes_to_update {
                    vector_index.update(&revision_pair)?;
                }
            }
            if let Some(text_indexes_to_update) = self
                .table_to_text_indexes
                .get_mut(&revision_pair.id.table())
            {
                for text_index in text_indexes_to_update {
                    text_index.update(&revision_pair)?;
                }
            }
            if num_revisions % 500 == 0 {
                let percent_progress =
                    (u64::from(revision_pair.ts()) - u64::from(self.oldest_index_ts)) as f64
                        / (u64::from(*upper_bound) - u64::from(self.oldest_index_ts)) as f64
                        * 100.0;
                tracing::info!(
                    "Processed ts {}, estimated progress: ({:.1}%)",
                    revision_pair.ts(),
                    percent_progress
                );
            }
        }

        tracing::info!(
            "Loaded {num_revisions} revisions ({total_size} bytes) in {:?}.",
            timer.elapsed()
        );
        crate::metrics::finish_bootstrap(num_revisions, total_size, timer);

        Ok(self.finish())
    }

    fn finish(self) -> BootstrappedSearchIndexes {
        let tables_with_indexes = self.tables_with_indexes();
        let text_index_manager = TextIndexManager::new(TextIndexManagerState::Ready(
            self.table_to_text_indexes
                .into_values()
                .flat_map(|text_indexes| {
                    text_indexes
                        .into_iter()
                        .map(
                            |TextIndexBootstrapData {
                                 index_id,
                                 text_index: search_index,
                                 tantivy_schema: _,
                             }| (index_id, search_index),
                        )
                        .collect::<Vec<_>>()
                })
                .collect(),
        ));
        let indexes = IndexState::Ready(
            self.table_to_vector_indexes
                .into_values()
                .flat_map(|vector_indexes| {
                    vector_indexes
                        .into_iter()
                        .map(
                            |VectorIndexBootstrapData {
                                 index_id,
                                 on_disk_state,
                                 memory_index,
                                 qdrant_schema: _,
                             }| {
                                (index_id, (on_disk_state, memory_index))
                            },
                        )
                        .collect::<Vec<_>>()
                })
                .collect(),
        );
        let vector_index_manager = VectorIndexManager { indexes };
        BootstrappedSearchIndexes {
            text_index_manager,
            vector_index_manager,
            tables_with_indexes,
        }
    }
}

#[derive(Clone)]
struct TextIndexBootstrapData {
    index_id: IndexId,
    text_index: TextIndex,
    tantivy_schema: TantivySearchIndexSchema,
}

impl TextIndexBootstrapData {
    fn update(&mut self, revision_pair: &RevisionPair) -> anyhow::Result<()> {
        let memory_index = self.text_index.memory_index_mut();
        match memory_index.min_ts() {
            WriteTimestamp::Pending => {
                anyhow::bail!(
                    "Found a pending write timestamp for search memory index created during \
                     bootstrapping. This should always be a committed timestamp."
                )
            },
            WriteTimestamp::Committed(ts) => {
                // Skip updates for revision pairs that have already been written to disk.
                if ts > revision_pair.ts() {
                    return Ok(());
                }
            },
        }
        memory_index.update(
            revision_pair.id.internal_id(),
            WriteTimestamp::Committed(revision_pair.ts()),
            revision_pair
                .prev_document()
                .map(|d| anyhow::Ok((self.tantivy_schema.index_into_terms(d)?, d.creation_time())))
                .transpose()?,
            revision_pair
                .document()
                .map(|d| anyhow::Ok((self.tantivy_schema.index_into_terms(d)?, d.creation_time())))
                .transpose()?,
        )
    }
}

#[derive(Clone)]
struct VectorIndexBootstrapData {
    index_id: IndexId,
    on_disk_state: VectorIndexState,
    memory_index: MemoryVectorIndex,
    qdrant_schema: QdrantSchema,
}

impl VectorIndexBootstrapData {
    fn update(&mut self, revision_pair: &RevisionPair) -> anyhow::Result<()> {
        match self.memory_index.min_ts() {
            WriteTimestamp::Pending => {
                anyhow::bail!(
                    "Found a pending write timestamp for vector memory index created during \
                     bootstrapping. This should always be a committed timestamp."
                )
            },
            WriteTimestamp::Committed(ts) => {
                // Skip updates for revision pairs that have already been written to disk.
                if ts > revision_pair.ts() {
                    return Ok(());
                }
            },
        }
        self.memory_index.update(
            revision_pair.id.internal_id(),
            WriteTimestamp::Committed(revision_pair.ts()),
            revision_pair
                .prev_document()
                .and_then(|d| self.qdrant_schema.index(d)),
            revision_pair
                .document()
                .and_then(|d| self.qdrant_schema.index(d)),
        )
    }
}

/// Streams revision pairs for documents in the indexed tables.
pub fn stream_revision_pairs_for_indexes<'a>(
    tables_with_indexes: &'a BTreeSet<TabletId>,
    persistence: &'a RepeatablePersistence,
    range: TimestampRange,
) -> impl Stream<Item = anyhow::Result<RevisionPair>> + 'a {
    persistence
        .load_revision_pairs(None /* tablet_id */, range, Order::Asc)
        .try_filter(|revision| {
            let is_in_indexed_table = tables_with_indexes.contains(&revision.id.table());
            if !is_in_indexed_table {
                log_document_skipped();
            }
            future::ready(is_in_indexed_table)
        })
}

impl<RT: Runtime> SearchIndexBootstrapWorker<RT> {
    pub(crate) fn new(
        runtime: RT,
        index_registry: IndexRegistry,
        persistence: RepeatablePersistence,
        table_mapping: NamespacedTableMapping,
        committer_client: CommitterClient,
    ) -> Self {
        Self {
            runtime,
            index_registry,
            table_mapping,
            persistence,
            committer_client,
            backoff: Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF),
        }
    }

    pub async fn start(mut self) {
        let timer = crate::metrics::search_and_vector_bootstrap_timer();
        loop {
            if let Err(e) = self.run().await {
                let delay = self.backoff.fail(&mut self.runtime.rng());
                // Forgive OCC errors < N to match UDF mutation retry behavior.
                if !e.is_occ() || (self.backoff.failures() as usize) > *UDF_EXECUTOR_OCC_MAX_RETRIES
                {
                    report_error(&mut e.context("SearchAndVectorBootstrapWorker died")).await;
                    tracing::error!(
                        "SearchIndexBoostrapWorker died, num_failures: {}. Backing off for {}ms",
                        self.backoff.failures(),
                        delay.as_millis()
                    );
                } else {
                    tracing::trace!(
                        "SearchIndexBoostrapWorker occed, retrying. num_failures: {}, backoff: \
                         {}ms",
                        self.backoff.failures(),
                        delay.as_millis(),
                    )
                }
                self.runtime.wait(delay).await;
            } else {
                tracing::info!("SearchIndexBoostrapWorker finished!");
                break;
            }
        }
        timer.finish();
    }

    async fn run(&mut self) -> anyhow::Result<()> {
        let bootstrapped_indexes = self.bootstrap().await?;
        let pause_client = self.runtime.pause_client();
        pause_client.wait(FINISHED_BOOTSTRAP_UPDATES).await;
        self.committer_client
            .finish_search_and_vector_bootstrap(
                bootstrapped_indexes,
                self.persistence.upper_bound(),
            )
            .await
    }

    async fn bootstrap(&self) -> anyhow::Result<BootstrappedSearchIndexes> {
        // Load all of the fast forward timestamps first to ensure that we stay within
        // the comparatively short valid time for the persistence snapshot
        let snapshot = self
            .persistence
            .read_snapshot(self.persistence.upper_bound())?;
        let registry = self.index_registry.clone();
        let table_mapping = self.table_mapping.clone();
        let get_index_futs = self
            .index_registry
            .all_text_and_vector_indexes()
            .into_iter()
            .map(move |index| {
                let registry = registry.clone();
                let table_mapping = table_mapping.clone();
                let snapshot = snapshot.clone();
                async move {
                    let fast_forward_ts = load_metadata_fast_forward_ts(
                        &registry,
                        &snapshot,
                        &table_mapping,
                        index.id(),
                    )
                    .await?;
                    anyhow::Ok((index, fast_forward_ts))
                }
            });
        let indexes_with_fast_forward_ts =
            try_join_buffer_unordered("get_index_futs", get_index_futs).await?;
        let indexes_to_bootstrap = IndexesToBootstrap::create(
            self.persistence.upper_bound(),
            indexes_with_fast_forward_ts,
        )?;
        indexes_to_bootstrap.bootstrap(&self.persistence).await
    }
}
