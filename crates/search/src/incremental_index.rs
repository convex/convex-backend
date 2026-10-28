use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    iter::zip,
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use anyhow::Context;
use common::{
    bootstrap_model::index::text_index::FragmentedTextSegment,
    bounded_thread_pool::BoundedThreadPool,
    id_tracker::StaticIdTracker,
    persistence::DocumentRevisionStream,
    runtime::{
        try_join_buffer_unordered,
        Runtime,
    },
    types::{
        ObjectKey,
        SearchIndexMetricLabels,
        Timestamp,
    },
};
use futures::TryStreamExt;
use storage::Storage;
use tantivy::{
    directory::MmapDirectory,
    fastfield::AliveBitSet,
    schema::Field,
    DocAddress,
    DocId,
    Index,
    IndexBuilder,
    SingleSegmentIndexWriter,
    Term,
};
use tempfile::TempDir;
use text_search::tracker::{
    load_alive_bitset,
    FieldTermMetadata,
    MemoryDeletionTracker,
    SearchMemoryIdTracker,
    SegmentTermMetadata,
};
use tokio::fs;
use value::InternalId;

use crate::{
    archive::cache::{
        ArchiveCacheManager,
        CachedArchive,
    },
    constants::CONVEX_EN_TOKENIZER,
    convex_en,
    disk_index::{
        download_single_file_zip,
        upload_single_file,
        upload_text_segment,
    },
    metrics::{
        log_compacted_segment_size_bytes,
        log_text_document_indexed,
        SearchType,
    },
    searcher::{
        FieldDeletions,
        FragmentedTextStorageKeys,
        SegmentTermMetadataFetcher,
        TermDeletionsByField,
        TermValue,
    },
    DocumentTerm,
    SearchFileType,
    TantivySearchIndexSchema,
};

/// The maximum size of a segment in bytes. 10MB.
const SEGMENT_MAX_SIZE_BYTES: usize = 10_000_000;

pub(crate) const ID_TRACKER_PATH: &str = "id_tracker";
pub(crate) const ALIVE_BITSET_PATH: &str = "tantivy_alive_bitset";
pub(crate) const DELETED_TERMS_PATH: &str = "deleted_terms";

pub struct NewTextSegment {
    pub paths: TextSegmentPaths,
    /// The total number of indexed documents in this segment, including
    /// documents that were added and then marked as deleted.
    pub num_indexed_documents: u64,
    pub size_bytes_total: u64,
}

#[derive(Clone)]
pub struct TextSegmentPaths {
    pub index_path: PathBuf,
    pub id_tracker_path: PathBuf,
    pub alive_bit_set_path: PathBuf,
    pub deleted_terms_path: PathBuf,
}

pub struct UpdatableTextSegment {
    id_tracker: StaticIdTracker,
    pub(crate) deletion_tracker: MemoryDeletionTracker,
    pub original: FragmentedTextSegment,
    /// The number of files deleted since this struct was created (not the total
    /// number of deletes in the segment).
    num_deletes_in_updated: u64,
}

impl UpdatableTextSegment {
    pub fn segment_key(&self) -> &ObjectKey {
        &self.original.segment_key
    }

    fn delete(&mut self, tantivy_id: DocId) {
        self.num_deletes_in_updated += 1;
        self.deletion_tracker.alive_bitset.remove(tantivy_id);
    }

    fn update_term_metadata(
        &mut self,
        segment_term_metadata: SegmentTermMetadata,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.num_deletes_in_updated > 0,
            "Trying to update term metadata without any new deletes?",
        );
        self.deletion_tracker
            .update_term_metadata(segment_term_metadata);
        Ok(())
    }

    fn get_tantivy_id(&self, convex_id: InternalId) -> Option<DocId> {
        if let Some(tantivy_id) = self.id_tracker.lookup(convex_id.0)
            && self.deletion_tracker.alive_bitset.contains(tantivy_id)
        {
            Some(tantivy_id)
        } else {
            None
        }
    }

    pub async fn upload_metadata(
        self,
        storage: Arc<dyn Storage>,
    ) -> anyhow::Result<FragmentedTextSegment> {
        if self.num_deletes_in_updated == 0 {
            return Ok(self.original);
        }

        let mut bitset_buf = vec![];
        let mut deleted_terms_buf = vec![];
        self.deletion_tracker
            .write(&mut bitset_buf, &mut deleted_terms_buf)?;

        let mut bitset_slice = bitset_buf.as_slice();
        let upload_bitset = upload_single_file(
            &mut bitset_slice,
            "alive_bitset".to_string(),
            storage.clone(),
            SearchFileType::TextAliveBitset,
        );
        let mut deleted_terms_slice = deleted_terms_buf.as_slice();
        let upload_deleted_terms = upload_single_file(
            &mut deleted_terms_slice,
            "deleted_terms".to_string(),
            storage.clone(),
            SearchFileType::TextDeletedTerms,
        );
        let (alive_bitset_key, deleted_terms_table_key) =
            futures::try_join!(upload_bitset, upload_deleted_terms)?;
        Ok(FragmentedTextSegment {
            deleted_terms_table_key,
            alive_bitset_key,
            num_deleted_documents: self.original.num_deleted_documents
                + self.num_deletes_in_updated,
            ..self.original
        })
    }

    pub async fn download(
        original: FragmentedTextSegment,
        storage: Arc<dyn Storage>,
    ) -> anyhow::Result<Self> {
        // Temp dir is fine because we're loading these into memory immediately.
        let tmp_dir = TempDir::new()?;

        let id_tracker_path = tmp_dir.path().join(ID_TRACKER_PATH);
        download_single_file_zip(&original.id_tracker_key, &id_tracker_path, storage.clone())
            .await?;
        let id_tracker = StaticIdTracker::load_from_path(id_tracker_path)?;

        let alive_bitset_path = tmp_dir.path().join(ALIVE_BITSET_PATH);
        download_single_file_zip(
            &original.alive_bitset_key,
            &alive_bitset_path,
            storage.clone(),
        )
        .await?;
        let deleted_terms_path = tmp_dir.path().join(DELETED_TERMS_PATH);
        download_single_file_zip(
            &original.deleted_terms_table_key,
            &deleted_terms_path,
            storage,
        )
        .await?;

        let deletion_tracker =
            MemoryDeletionTracker::load(&alive_bitset_path, &deleted_terms_path)?;

        Ok(UpdatableTextSegment {
            id_tracker,
            deletion_tracker,
            original,
            num_deletes_in_updated: 0,
        })
    }
}

#[derive(Default)]
pub struct PreviousTextSegments(pub BTreeMap<ObjectKey, UpdatableTextSegment>);

impl PreviousTextSegments {
    /// Returns the index to the segment containing the live document and the
    /// tantivy id, if it exists
    fn segment_for_document(&self, convex_id: InternalId) -> Option<(ObjectKey, DocId)> {
        for (segment_key, segment) in self.0.iter() {
            if let Some(tantivy_id) = segment.get_tantivy_id(convex_id) {
                return Some((segment_key.clone(), tantivy_id));
            }
        }
        None
    }

    /// Deletes a document (if present) and returns the segment key of the
    /// segment that it was deleted from
    pub fn delete_document(&mut self, convex_id: InternalId) -> anyhow::Result<Option<ObjectKey>> {
        let Some((segment_key, tantivy_id)) = self.segment_for_document(convex_id) else {
            return Ok(None);
        };
        self.must_get_mut(&segment_key).delete(tantivy_id);
        Ok(Some(segment_key.clone()))
    }

    /// Returns the segment with the given key, panics if not found. This is
    /// useful if we got the segment key from one of these other methods so we
    /// know the key exists.
    fn must_get_mut(&mut self, segment_key: &ObjectKey) -> &mut UpdatableTextSegment {
        self.0.get_mut(segment_key).expect("Segment key not found")
    }

    pub fn update_term_deletion_metadata(
        &mut self,
        segments_term_metadata: Vec<(ObjectKey, SegmentTermMetadata)>,
    ) -> anyhow::Result<()> {
        for (segment_key, segment_term_metadata) in segments_term_metadata {
            self.must_get_mut(&segment_key)
                .update_term_metadata(segment_term_metadata)?;
        }
        Ok(())
    }
}

pub struct SegmentStatisticsUpdates {
    pub term_deletes_by_segment: BTreeMap<ObjectKey, TermDeletionsByField>,
}
impl SegmentStatisticsUpdates {
    pub fn new() -> Self {
        Self {
            term_deletes_by_segment: BTreeMap::new(),
        }
    }

    pub fn on_document_deleted(&mut self, segment_key: &ObjectKey, terms: Vec<DocumentTerm>) {
        let term_deletes_for_segment = if let Some(term_values_and_delete_counts) =
            self.term_deletes_by_segment.get_mut(segment_key)
        {
            term_values_and_delete_counts
        } else {
            let term_values_and_delete_counts = TermDeletionsByField::default();
            self.term_deletes_by_segment
                .insert(segment_key.clone(), term_values_and_delete_counts);
            self.term_deletes_by_segment.get_mut(segment_key).unwrap()
        };

        let mut field_to_unique_term_value = BTreeMap::<Field, BTreeSet<TermValue>>::new();
        for document_term in terms {
            let term = Term::from(document_term);
            let field = term.field();
            let term_value = term.value_bytes().to_vec();
            term_deletes_for_segment.increment_num_terms_deleted(field);
            field_to_unique_term_value
                .entry(field)
                .or_default()
                .insert(term_value);
        }
        for (field, unique_terms) in field_to_unique_term_value {
            for term_value in unique_terms {
                term_deletes_for_segment.increment_num_docs_deleted_for_term(field, term_value);
            }
        }
    }
}

/// Builds a new segment from a stream of document revisions in descending
/// timestamp order, updating existing segments if a document was deleted.
///
/// Note the descending order requirement can be relaxed if the caller can
/// guarantee that no deletes will be present in the stream. A caller can do so
/// when providing this function with a stream from table iterator for example.
pub async fn build_new_segment(
    revision_stream: DocumentRevisionStream<'_>,
    tantivy_schema: TantivySearchIndexSchema,
    dir: &Path,
    previous_segments: &mut PreviousTextSegments,
    segment_term_metadata_fetcher: Arc<dyn SegmentTermMetadataFetcher>,
    search_storage: Arc<dyn Storage>,
    document_log_lower_bound: Option<Timestamp>,
) -> anyhow::Result<Option<NewTextSegment>> {
    let index_path = dir.join("index_path");
    fs::create_dir(&index_path).await?;
    let index = IndexBuilder::new()
        .schema(tantivy_schema.schema.clone())
        .create_in_dir(&index_path)?;
    index
        .tokenizers()
        .register(CONVEX_EN_TOKENIZER, convex_en());
    let mut segment_writer = SingleSegmentIndexWriter::new(index, SEGMENT_MAX_SIZE_BYTES)?;
    let mut new_id_tracker = SearchMemoryIdTracker::default();
    futures::pin_mut!(revision_stream);
    // Keep track of the document IDs we've either added to our new segment or
    // deleted from a previous segment. Because we process in reverse order, we
    // may encounter each document id multiple times, but we only want to add or
    // delete them once.
    let mut document_ids_processed = BTreeSet::new();
    // Deleted documents that either have a revision in a previous segment that we
    // will eventually encounter in the log and delete. Or that were added and
    // deleted within our new segment's time window and can be ignored.
    let mut dangling_deletes = BTreeSet::new();

    let mut num_indexed_documents = 0;

    let mut segment_statistics_updates = SegmentStatisticsUpdates::new();

    while let Some(revision_pair) = revision_stream.try_next().await? {
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
        if let Some(new_document) = revision_pair.document() {
            // If we have already processed a delete for this document at a higher
            // timestamp, we can ignore it. Otherwise, add it to the segment.
            if dangling_deletes.contains(&convex_id) {
                dangling_deletes.remove(&convex_id);
            } else {
                num_indexed_documents += 1;
                let tantivy_document =
                    tantivy_schema.index_into_tantivy_document(new_document, revision_pair.ts());
                log_text_document_indexed(&tantivy_schema, &tantivy_document);
                let doc_id = segment_writer.add_document(tantivy_document)?;
                new_id_tracker.set_link(convex_id, doc_id)?;
            }
        }

        // Delete
        if let Some(prev_rev) = revision_pair.prev_rev
            && let Some(prev_document) = prev_rev.document.as_ref()
        {
            if let Some(lower_bound_ts) = document_log_lower_bound
                && prev_rev.ts > lower_bound_ts
            {
                // This document might be an add, or might be replaced earlier in the log, we
                // don't know, so we need to process it again later.
                dangling_deletes.insert(prev_document.id().internal_id());
                document_ids_processed.remove(&prev_document.id().internal_id());
            } else {
                let segment_key = previous_segments
                    .delete_document(prev_document.id().internal_id())?
                    .context("Missing segment even though revision is not in our time bounds")?;
                let segment = previous_segments
                    .0
                    .get(&segment_key)
                    .context("Segment key not found")?;

                let terms = tantivy_schema.index_into_terms(prev_document)?;
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
        search_storage,
        segment_term_metadata_fetcher,
        segment_statistics_updates.term_deletes_by_segment,
        SearchIndexMetricLabels::unknown(),
    )
    .await?;
    previous_segments.update_term_deletion_metadata(segments_term_metadata)?;

    if num_indexed_documents == 0 {
        return Ok(None);
    }
    // Finalize the segment and write to trackers
    segment_writer.finalize()?;
    let new_deletion_tracker = MemoryDeletionTracker::new(new_id_tracker.num_ids() as u32);
    let alive_bit_set_path = dir.join(ALIVE_BITSET_PATH);
    let deleted_terms_path = dir.join(DELETED_TERMS_PATH);
    new_deletion_tracker.write_to_path(&alive_bit_set_path, &deleted_terms_path)?;
    let id_tracker_path = dir.join(ID_TRACKER_PATH);
    new_id_tracker.write(&id_tracker_path)?;
    let total_size_bytes = get_size(&index_path).await?
        + get_size(&id_tracker_path).await?
        + get_size(&alive_bit_set_path).await?
        + get_size(&deleted_terms_path).await?;
    let paths = TextSegmentPaths {
        index_path,
        id_tracker_path,
        alive_bit_set_path,
        deleted_terms_path,
    };
    Ok(Some(NewTextSegment {
        paths,
        num_indexed_documents,
        size_bytes_total: total_size_bytes,
    }))
}

async fn get_size(path: &PathBuf) -> anyhow::Result<u64> {
    let mut stack = vec![path.clone()];
    let mut total = 0;
    while let Some(path) = stack.pop() {
        let metadata = fs::metadata(&path).await?;
        if metadata.is_file() {
            total += metadata.len();
            continue;
        }
        if metadata.is_dir() {
            let mut read_dir = fs::read_dir(&path).await?;
            while let Some(entry) = read_dir.next_entry().await? {
                stack.push(entry.path());
            }
        }
    }
    Ok(total)
}

pub async fn fetch_term_ordinals_and_remap_deletes(
    storage: Arc<dyn Storage>,
    segment_term_metadata_fetcher: Arc<dyn SegmentTermMetadataFetcher>,
    term_deletes_by_segment: BTreeMap<ObjectKey, TermDeletionsByField>,
    labels: SearchIndexMetricLabels<'_>,
) -> anyhow::Result<Vec<(ObjectKey, SegmentTermMetadata)>> {
    let labels = labels.to_owned();
    let fetch_segment_metadata_futs = term_deletes_by_segment.into_iter().map(
        move |(segment_key, term_values_and_delete_counts)| {
            fetch_segment_term_ordinal_and_remap_deletes(
                storage.clone(),
                segment_key,
                segment_term_metadata_fetcher.clone(),
                term_values_and_delete_counts,
                labels.clone(),
            )
        },
    );

    let segments_term_metadata: Vec<_> =
        try_join_buffer_unordered("text_term_metadata", fetch_segment_metadata_futs).await?;
    Ok(segments_term_metadata)
}

async fn fetch_segment_term_ordinal_and_remap_deletes(
    storage: Arc<dyn Storage>,
    segment_key: ObjectKey,
    segment_term_metadata_fetcher: Arc<dyn SegmentTermMetadataFetcher>,
    deletions: TermDeletionsByField,
    labels: SearchIndexMetricLabels<'static>,
) -> anyhow::Result<(ObjectKey, SegmentTermMetadata)> {
    let term_values_by_field: BTreeMap<Field, Vec<TermValue>> = deletions
        .0
        .iter()
        .map(|(field, deletions)| {
            (
                *field,
                deletions
                    .term_value_to_deleted_documents
                    .keys()
                    .cloned()
                    .collect(),
            )
        })
        .collect();
    let term_ordinals_by_field = segment_term_metadata_fetcher
        .fetch_term_ordinals(
            storage.clone(),
            segment_key.clone(),
            term_values_by_field.clone(),
            labels.clone(),
        )
        .await?;
    let mut term_metadata_by_field = BTreeMap::new();
    for (field, field_deletions) in deletions.0 {
        let FieldDeletions {
            term_value_to_deleted_documents,
            num_terms_deleted,
        } = field_deletions;

        let mut term_documents_deleted = BTreeMap::new();
        let term_ordinals = term_ordinals_by_field
            .get(&field)
            .with_context(|| format!("Failed to fetch term ordinals for {field:?}"))?;
        let term_values = term_values_by_field
            .get(&field)
            .with_context(|| format!("Failed to find values for {field:?}"))?;
        for (value, ordinal) in zip(term_values, term_ordinals) {
            term_documents_deleted.insert(
                *ordinal,
                *term_value_to_deleted_documents
                    .get(value)
                    .with_context(|| format!("Failed to find deleted docs for {value:?}"))?,
            );
        }
        term_metadata_by_field.insert(
            field,
            FieldTermMetadata {
                term_documents_deleted,
                num_terms_deleted,
            },
        );
    }
    Ok((
        segment_key,
        SegmentTermMetadata {
            term_metadata_by_field,
        },
    ))
}

pub struct SearchSegmentForMerge {
    pub segment: Index,
    pub alive_bitset: AliveBitSet,
    pub id_tracker: StaticIdTracker,
}

/// The on-disk files of a text segment fetched through the archive cache.
/// `archives` keeps the files under `paths` on disk; drop it only once nothing
/// reads from them anymore.
pub struct FetchedTextSegment {
    pub paths: TextSegmentPaths,
    pub archives: Vec<CachedArchive>,
}

pub async fn fetch_text_segment<RT: Runtime>(
    archive_cache: ArchiveCacheManager<RT>,
    storage: Arc<dyn Storage>,
    keys: impl Into<FragmentedTextStorageKeys>,
    labels: SearchIndexMetricLabels<'_>,
) -> anyhow::Result<FetchedTextSegment> {
    let keys = keys.into();
    let labels = labels.to_owned();

    let fetch_index_path = archive_cache.get(
        storage.clone(),
        &keys.segment,
        SearchFileType::Text,
        labels.clone(),
    );
    let fetch_id_tracker_path = archive_cache.get_single_file(
        storage.clone(),
        &keys.id_tracker,
        SearchFileType::TextIdTracker,
        labels.clone(),
    );
    let fetch_alive_bitset_path = archive_cache.get_single_file(
        storage.clone(),
        &keys.alive_bitset,
        SearchFileType::TextAliveBitset,
        labels.clone(),
    );
    let fetch_deleted_terms = archive_cache.get_single_file(
        storage.clone(),
        &keys.deleted_terms_table,
        SearchFileType::TextDeletedTerms,
        labels,
    );

    let (index, id_tracker, alive_bitset, deleted_terms) = futures::try_join!(
        fetch_index_path,
        fetch_id_tracker_path,
        fetch_alive_bitset_path,
        fetch_deleted_terms
    )?;

    Ok(FetchedTextSegment {
        paths: TextSegmentPaths {
            index_path: index.path().to_owned(),
            id_tracker_path: id_tracker.path().to_owned(),
            alive_bit_set_path: alive_bitset.path().to_owned(),
            deleted_terms_path: deleted_terms.path().to_owned(),
        },
        archives: vec![index, id_tracker, alive_bitset, deleted_terms],
    })
}

pub fn open_text_segment_for_merge(
    paths: TextSegmentPaths,
) -> anyhow::Result<SearchSegmentForMerge> {
    let id_tracker = StaticIdTracker::load_from_path(paths.id_tracker_path)?;
    let alive_bitset = load_alive_bitset(&paths.alive_bit_set_path)?;

    let index = Index::open_in_dir(paths.index_path)?;
    Ok(SearchSegmentForMerge {
        segment: index,
        alive_bitset,
        id_tracker,
    })
}
pub async fn fetch_compact_and_upload_text_segment<RT: Runtime>(
    rt: &RT,
    storage: Arc<dyn Storage>,
    cache: ArchiveCacheManager<RT>,
    blocking_thread_pool: BoundedThreadPool<RT>,
    segments: Vec<FragmentedTextStorageKeys>,
    labels: SearchIndexMetricLabels<'_>,
) -> anyhow::Result<FragmentedTextSegment> {
    let _storage = storage.clone();
    let labels = labels.to_owned();
    let opened_segments: Vec<(SearchSegmentForMerge, Vec<CachedArchive>)> =
        try_join_buffer_unordered(
            "text_segment_merge",
            segments.into_iter().map(move |segment| {
                let pool = blocking_thread_pool.clone();
                let storage = storage.clone();
                let cache = cache.clone();
                let labels = labels.clone();
                async move {
                    let FetchedTextSegment { paths, archives } =
                        fetch_text_segment(cache, storage, segment, labels).await?;
                    let opened = pool
                        .execute(|| open_text_segment_for_merge(paths))
                        .await??;
                    anyhow::Ok((opened, archives))
                }
            }),
        )
        .await?;
    let (opened_segments, archives): (Vec<_>, Vec<_>) = opened_segments.into_iter().unzip();

    let dir = TempDir::new()?;
    let new_segment = merge_segments(opened_segments, dir.path()).await?;
    // The merge reads the fetched segments through mmaps that must stay valid
    // until it has finished.
    drop(archives);
    upload_text_segment(rt, _storage, new_segment).await
}

pub async fn merge_segments(
    search_segments: Vec<SearchSegmentForMerge>,
    dir: &Path,
) -> anyhow::Result<NewTextSegment> {
    let mut segments = vec![];
    let settings = search_segments
        .first()
        .context("Called merge_segments with an empty vec of segments")?
        .segment
        .settings()
        .clone();
    for s in &search_segments {
        let mut index_segments = s.segment.searchable_segments()?;
        anyhow::ensure!(
            index_segments.len() == 1,
            "Expected exactly one segment, found {}",
            index_segments.len()
        );
        segments.push(index_segments.pop().unwrap());
    }
    let alive_bitsets = search_segments
        .iter()
        .map(|s| Some(s.alive_bitset.clone()))
        .collect::<Vec<_>>();
    let total_alive = search_segments
        .iter()
        .fold(0, |acc, e| acc + e.alive_bitset.num_alive_docs());
    // tantivy's IndexMerger panics while writing fast fields when no input
    // segment has a live document.
    anyhow::ensure!(
        total_alive > 0,
        "Cannot merge {} segments with no live documents",
        search_segments.len()
    );

    let index_dir = dir.join("index_dir");
    fs::create_dir(&index_dir).await?;
    let mmap_directory = MmapDirectory::open(&index_dir)?;
    let (_merged_segment, id_mapping) =
        tantivy::merge_filtered_segments(&segments, settings, alive_bitsets, mmap_directory)?;
    anyhow::ensure!(
        total_alive == id_mapping.len(),
        "Total alive documents expected did not match merged segment id mapping"
    );
    let mut new_segment_id_tracker = SearchMemoryIdTracker::default();
    for (
        new_tantivy_id,
        DocAddress {
            segment_ord,
            doc_id: old_doc_id,
        },
    ) in id_mapping.iter_old_doc_addrs_enumerated()
    {
        let old_id_tracker = &search_segments[segment_ord as usize].id_tracker;
        let convex_id = old_id_tracker
            .get_convex_id(old_doc_id as usize)
            .with_context(|| {
                format!("Old id tracker for segment {segment_ord} is missing id {old_doc_id}")
            })?;
        new_segment_id_tracker.set_link(InternalId(convex_id), new_tantivy_id)?;
    }
    let num_docs = new_segment_id_tracker.num_ids();
    let id_tracker_path = dir.to_path_buf().join(ID_TRACKER_PATH);
    let num_indexed_documents = new_segment_id_tracker.num_ids() as u64;
    new_segment_id_tracker.write(&id_tracker_path)?;
    let tracker = MemoryDeletionTracker::new(num_docs as u32);
    let alive_bit_set_path = dir.to_path_buf().join(ALIVE_BITSET_PATH);
    let deleted_terms_path = dir.to_path_buf().join(DELETED_TERMS_PATH);
    tracker.write_to_path(&alive_bit_set_path, &deleted_terms_path)?;
    let size_bytes_total = get_size(&index_dir).await?;
    log_compacted_segment_size_bytes(size_bytes_total, SearchType::Text);
    Ok(NewTextSegment {
        num_indexed_documents,
        paths: TextSegmentPaths {
            index_path: index_dir,
            id_tracker_path,
            alive_bit_set_path,
            deleted_terms_path,
        },
        size_bytes_total,
    })
}
