use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    ops::RangeToInclusive,
    path::PathBuf,
    sync::Arc,
};

use anyhow::Context;
use async_trait::async_trait;
use common::{
    bootstrap_model::index::{
        text_index::{
            FragmentedTextSegment,
            TextIndexSnapshot,
            TextIndexSnapshotData,
            TextIndexSpec,
            TextIndexState,
            TextSnapshotVersion,
        },
        IndexConfig,
        TabletIndexMetadata,
    },
    document::{
        ParsedDocument,
        ResolvedDocument,
    },
    index::{
        IndexKey,
        IndexKeyBytes,
    },
    persistence::{
        DocumentRevisionStream,
        DocumentStream,
        RepeatablePersistence,
        TimestampRange,
    },
    persistence_helpers::{
        DocumentRevision,
        RevisionPair,
    },
    query::Order,
    runtime::{
        try_join_buffer_unordered,
        RateLimiter,
        Runtime,
    },
    types::{
        IndexId,
        SearchIndexMetricLabels,
    },
};
use database::{
    Database,
    Snapshot,
};
use futures::{
    StreamExt as _,
    TryStreamExt,
};
use search::{
    build_new_segment,
    disk_index::upload_text_segment,
    fetch_term_ordinals_and_remap_deletes,
    metrics::SearchType,
    searcher::{
        FragmentedTextStorageKeys,
        SegmentTermMetadataFetcher,
    },
    NewTextSegment,
    PreviousTextSegments,
    Searcher,
    SegmentStatisticsUpdates,
    TantivySearchIndexSchema,
    UpdatableTextSegment,
};
use storage::Storage;
use sync_types::Timestamp;
use value::{
    DeveloperDocumentId,
    TableNumber,
    TabletId,
};

use crate::index_meta::{
    SearchIndex,
    SearchIndexConfig,
    SearchOnDiskState,
    SearchSnapshot,
    SegmentStatistics,
    SegmentType,
    SnapshotData,
};

#[derive(Clone, Debug)]
pub struct TextSearchIndex;

impl SegmentType<TextSearchIndex> for FragmentedTextSegment {
    fn id(&self) -> &str {
        &self.id
    }

    fn statistics(&self) -> anyhow::Result<TextStatistics> {
        Ok(TextStatistics {
            num_indexed_documents: self.num_indexed_documents,
            num_deleted_documents: self.num_deleted_documents,
        })
    }

    fn total_size_bytes(
        &self,
        _config: &<TextSearchIndex as SearchIndex>::Spec,
    ) -> anyhow::Result<u64> {
        Ok(self.size_bytes_total)
    }
}

#[derive(Clone)]
pub struct BuildTextIndexArgs {
    pub search_storage: Arc<dyn Storage>,
    pub segment_term_metadata_fetcher: Arc<dyn SegmentTermMetadataFetcher>,
}

#[async_trait]
impl SearchIndex for TextSearchIndex {
    type BuildIndexArgs = BuildTextIndexArgs;
    type DocStream<'a> = DocumentRevisionStream<'a>;
    type NewSegment = NewTextSegment;
    type PreviousSegments = PreviousTextSegments;
    type Schema = TantivySearchIndexSchema;
    type Segment = FragmentedTextSegment;
    type Spec = TextIndexSpec;
    type Statistics = TextStatistics;

    fn get_config(config: &IndexConfig) -> Option<SearchIndexConfig<Self>> {
        let IndexConfig::Text {
            on_disk_state,
            spec,
        } = config
        else {
            return None;
        };
        Some(SearchIndexConfig {
            spec: spec.clone(),
            on_disk_state: match on_disk_state.clone() {
                TextIndexState::Backfilling(state) => SearchOnDiskState::Backfilling(state),
                TextIndexState::Backfilled { snapshot, staged } => SearchOnDiskState::Backfilled {
                    snapshot: snapshot.into(),
                    staged,
                },
                TextIndexState::SnapshottedAt(snapshot) => {
                    SearchOnDiskState::SnapshottedAt(snapshot.into())
                },
            },
        })
    }

    // When iterating over the document log for partial segments, we must iterate in
    // reverse timestamp order to match assumptions made in build_disk_index
    // that allow for greater efficiency.
    fn partial_document_order() -> Order {
        Order::Desc
    }

    fn get_index_sizes(snapshot: Snapshot) -> anyhow::Result<BTreeMap<IndexId, usize>> {
        Ok(snapshot
            .text_indexes
            .backfilled_and_enabled_index_sizes()?
            .collect())
    }

    fn is_version_current(snapshot: &SearchSnapshot<Self>) -> bool {
        // TODO(sam): This doesn't match the current persistence version based check,
        // but it's closer to what vector search does.
        snapshot.data.is_version_current()
    }

    fn new_schema(config: &Self::Spec) -> Self::Schema {
        TantivySearchIndexSchema::new(config)
    }

    async fn download_previous_segments(
        storage: Arc<dyn Storage>,
        segments: Vec<Self::Segment>,
    ) -> anyhow::Result<Self::PreviousSegments> {
        let segments: Vec<_> = try_join_buffer_unordered(
            "download_text_meta",
            segments
                .into_iter()
                .map(move |segment| UpdatableTextSegment::download(segment, storage.clone())),
        )
        .await?;
        let segments = segments
            .into_iter()
            .map(|updatable_segment| (updatable_segment.segment_key().clone(), updatable_segment))
            .collect();
        Ok(PreviousTextSegments(segments))
    }

    async fn upload_previous_segments(
        storage: Arc<dyn Storage>,
        segments: Self::PreviousSegments,
    ) -> anyhow::Result<Vec<Self::Segment>> {
        try_join_buffer_unordered(
            "upload_text_metadata",
            segments
                .0
                .into_values()
                .map(move |segment| segment.upload_metadata(storage.clone())),
        )
        .await
    }

    fn estimate_document_size(schema: &Self::Schema, doc: &ResolvedDocument) -> u64 {
        schema.estimate_size(doc)
    }

    fn load_doc_stream<'a, RT: Runtime>(
        database: &'a Database<RT>,
        tablet_id: TabletId,
        range: TimestampRange,
        order: Order,
        rate_limiter: &'a RateLimiter<RT>,
    ) -> DocumentRevisionStream<'a> {
        database.load_revision_pairs_in_table(tablet_id, range, order, rate_limiter)
    }

    fn table_scan_stream_to_doc_stream<'a>(
        documents: DocumentStream<'a>,
    ) -> DocumentRevisionStream<'a> {
        documents
            .map(|result| {
                let entry = result?;
                anyhow::ensure!(entry.value.is_some(), "Document must exist");
                Ok(RevisionPair {
                    id: entry.id,
                    rev: DocumentRevision {
                        ts: entry.ts,
                        document: entry.value,
                    },
                    prev_rev: None,
                })
            })
            .boxed()
    }

    fn walk_document_log_for_updates<'a>(
        scan_doc_stream: DocumentRevisionStream<'a>,
        reader: &'a RepeatablePersistence,
        tablet_id: TabletId,
        table_number: TableNumber,
        range: TimestampRange,
        filter_id_range: RangeToInclusive<IndexKeyBytes>,
    ) -> DocumentRevisionStream<'a> {
        let log_stream = reader
            .load_revision_pairs(Some(tablet_id), range, Order::Desc)
            .try_filter(move |revision_pair| {
                let doc_id_index_key = IndexKey::new(
                    vec![],
                    DeveloperDocumentId::new(table_number, revision_pair.id.internal_id()),
                );
                futures::future::ready(filter_id_range.contains(&doc_id_index_key.to_bytes()))
            })
            .boxed();
        scan_doc_stream.chain(log_stream).boxed()
    }

    async fn build_disk_index(
        schema: &Self::Schema,
        index_path: &PathBuf,
        documents: DocumentRevisionStream<'_>,
        previous_segments: &mut Self::PreviousSegments,
        lower_bound_ts: Option<Timestamp>,
        BuildTextIndexArgs {
            search_storage,
            segment_term_metadata_fetcher,
        }: BuildTextIndexArgs,
    ) -> anyhow::Result<Option<Self::NewSegment>> {
        build_new_segment(
            documents,
            schema.clone(),
            index_path,
            previous_segments,
            segment_term_metadata_fetcher,
            search_storage,
            lower_bound_ts,
        )
        .await
    }

    async fn upload_new_segment<RT: Runtime>(
        rt: &RT,
        storage: Arc<dyn Storage>,
        new_segment: Self::NewSegment,
    ) -> anyhow::Result<Self::Segment> {
        upload_text_segment(rt, storage, new_segment).await
    }

    fn extract_metadata(
        metadata: ParsedDocument<TabletIndexMetadata>,
    ) -> anyhow::Result<(Self::Spec, SearchOnDiskState<Self>)> {
        let (on_disk_state, spec) = match metadata.into_value().config {
            IndexConfig::Database { .. } | IndexConfig::Vector { .. } => {
                anyhow::bail!("Index type changed!")
            },
            IndexConfig::Text {
                spec,
                on_disk_state,
            } => (on_disk_state, spec),
        };
        Ok((spec, SearchOnDiskState::from(on_disk_state)))
    }

    fn new_index_config(spec: Self::Spec, new_state: SearchOnDiskState<Self>) -> IndexConfig {
        let on_disk_state = TextIndexState::from(new_state);
        IndexConfig::Text {
            on_disk_state,
            spec,
        }
    }

    fn search_type() -> SearchType {
        SearchType::Text
    }

    async fn execute_compaction(
        searcher: Arc<dyn Searcher>,
        search_storage: Arc<dyn Storage>,
        _config: &Self::Spec,
        segments: Vec<Self::Segment>,
    ) -> anyhow::Result<Self::Segment> {
        searcher
            .execute_text_compaction(
                search_storage,
                segments
                    .into_iter()
                    .map(FragmentedTextStorageKeys::from)
                    .collect(),
                SearchIndexMetricLabels::unknown(),
            )
            .await
    }

    async fn merge_deletes(
        previous_segments: &mut Self::PreviousSegments,
        revision_stream: DocumentRevisionStream<'_>,
        build_index_args: Self::BuildIndexArgs,
        schema: Self::Schema,
        document_log_lower_bound: Timestamp,
    ) -> anyhow::Result<()> {
        // Keep track of the document IDs we've either added to our new segment or
        // deleted from a previous segment. Because we process in reverse order, we
        // may encounter each document id multiple times, but we only want to add or
        // delete them once.
        let mut document_ids_processed = BTreeSet::new();
        // Deleted documents that either have a revision in a previous segment that we
        // will eventually encounter in the log and delete. Or that were added and
        // deleted within our new segment's time window and can be ignored.
        let mut dangling_deletes = BTreeSet::new();
        let mut segment_statistics_updates = SegmentStatisticsUpdates::new();
        futures::pin_mut!(revision_stream);
        while let Some(revision_pair) = revision_stream.try_next().await? {
            // We need to pass the schema through here.
            // Update segment statistics
            // For each document, three possible outcomes:
            // 1. We add the document to our new segment
            // 2. We delete the document from a previous segment
            // 3. We ignore the document because it was both added and removed within the
            //    time bounds for our new segment.
            let convex_id = revision_pair.id.internal_id();

            // Skip documents we have already added to the segment, but update dangling
            // deletes
            if document_ids_processed.contains(&convex_id) {
                continue;
            }
            document_ids_processed.insert(convex_id);

            // Addition
            if let Some(_new_document) = revision_pair.document() {
                // If we have already processed a delete for this document at a higher
                // timestamp, we can ignore it. Otherwise, add it to the segment.
                if dangling_deletes.contains(&convex_id) {
                    dangling_deletes.remove(&convex_id);
                }
            }

            // Delete
            if let Some(prev_rev) = revision_pair.prev_rev
                && let Some(prev_document) = prev_rev.document.as_ref()
            {
                if prev_rev.ts > document_log_lower_bound {
                    // This document might be an add, or might be replaced earlier in the log,
                    // we don't know, so we need to process it again
                    // later.
                    dangling_deletes.insert(prev_document.id().internal_id());
                    document_ids_processed.remove(&prev_document.id().internal_id());
                } else {
                    let segment_key = previous_segments
                        .delete_document(prev_document.id().internal_id())?
                        .context(
                            "Missing segment even though revision is not in our time bounds",
                        )?;
                    let segment = previous_segments
                        .0
                        .get(&segment_key)
                        .context("Segment key not found")?;

                    let terms = schema.index_into_terms(prev_document)?;
                    segment_statistics_updates
                        .on_document_deleted(&segment.original.segment_key, terms);
                }
            }
        }
        anyhow::ensure!(
            dangling_deletes.is_empty(),
            "Dangling deletes is not empty. A document was deleted that is not present in other \
             segments nor in this stream"
        );

        let segments_term_metadata = fetch_term_ordinals_and_remap_deletes(
            build_index_args.search_storage.clone(),
            build_index_args.segment_term_metadata_fetcher.clone(),
            segment_statistics_updates.term_deletes_by_segment,
            SearchIndexMetricLabels::unknown(),
        )
        .await?;
        previous_segments.update_term_deletion_metadata(segments_term_metadata)?;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct TextStatistics {
    pub num_indexed_documents: u64,
    pub num_deleted_documents: u64,
}

impl From<SearchOnDiskState<TextSearchIndex>> for TextIndexState {
    fn from(value: SearchOnDiskState<TextSearchIndex>) -> Self {
        match value {
            SearchOnDiskState::Backfilling(state) => Self::Backfilling(state),
            SearchOnDiskState::Backfilled { snapshot, staged } => Self::Backfilled {
                snapshot: snapshot.into(),
                staged,
            },
            SearchOnDiskState::SnapshottedAt(snapshot) => Self::SnapshottedAt(snapshot.into()),
        }
    }
}

impl From<TextIndexState> for SearchOnDiskState<TextSearchIndex> {
    fn from(value: TextIndexState) -> Self {
        match value {
            TextIndexState::Backfilling(state) => Self::Backfilling(state),
            TextIndexState::Backfilled { snapshot, staged } => Self::Backfilled {
                snapshot: snapshot.into(),
                staged,
            },
            TextIndexState::SnapshottedAt(snapshot) => Self::SnapshottedAt(snapshot.into()),
        }
    }
}

impl SegmentStatistics for TextStatistics {
    fn add(lhs: anyhow::Result<Self>, rhs: anyhow::Result<Self>) -> anyhow::Result<Self> {
        let lhs = lhs?;
        let rhs = rhs?;
        Ok(Self {
            num_indexed_documents: lhs.num_indexed_documents + rhs.num_indexed_documents,
            num_deleted_documents: lhs.num_deleted_documents + rhs.num_deleted_documents,
        })
    }

    fn num_documents(&self) -> u64 {
        self.num_indexed_documents
    }

    fn num_non_deleted_documents(&self) -> u64 {
        self.num_indexed_documents - self.num_deleted_documents
    }
}

impl From<TextIndexSnapshot> for SearchSnapshot<TextSearchIndex> {
    fn from(snapshot: TextIndexSnapshot) -> Self {
        Self {
            ts: snapshot.ts,
            data: snapshot.data.into(),
        }
    }
}

impl From<SearchSnapshot<TextSearchIndex>> for TextIndexSnapshot {
    fn from(value: SearchSnapshot<TextSearchIndex>) -> Self {
        Self {
            ts: value.ts,
            data: value.data.into(),
            version: TextSnapshotVersion::V2UseStringIds,
        }
    }
}

impl From<SnapshotData<FragmentedTextSegment>> for TextIndexSnapshotData {
    fn from(value: SnapshotData<FragmentedTextSegment>) -> Self {
        match value {
            SnapshotData::Unknown(obj) => TextIndexSnapshotData::Unknown(obj),
            SnapshotData::MultiSegment(segments) => TextIndexSnapshotData::MultiSegment(segments),
        }
    }
}

impl From<TextIndexSnapshotData> for SnapshotData<FragmentedTextSegment> {
    fn from(value: TextIndexSnapshotData) -> Self {
        match value {
            TextIndexSnapshotData::Unknown(obj) => SnapshotData::Unknown(obj),
            TextIndexSnapshotData::MultiSegment(segments) => SnapshotData::MultiSegment(segments),
        }
    }
}
