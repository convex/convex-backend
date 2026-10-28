use std::{
    collections::HashMap,
    fs::{
        self,
        File,
    },
    io::BufWriter,
    path::{
        Path,
        PathBuf,
    },
    sync::{
        atomic::AtomicBool,
        Arc,
    },
};

use anyhow::Context;
use atomic_refcell::AtomicRefCell;
use common::{
    deleted_bitset::DeletedBitset,
    id_tracker::StaticIdTracker,
    runtime::tokio_spawn_blocking,
};
use parking_lot::{
    Mutex,
    RwLock,
};
use qdrant_common::cpu::CpuPermit;
use qdrant_segment::vector_storage::{
    appendable_mmap_dense_vector_storage::open_appendable_memmap_vector_storage,
    memmap_dense_vector_storage::open_memmap_vector_storage,
};
use qdrant_segment::{
    common::{
        rocksdb_wrapper::{
            db_options,
            // open_db,
            DB_MAPPING_CF,
            DB_PAYLOAD_CF,
            DB_VECTOR_CF,
            DB_VERSIONS_CF,
        },
        version::StorageVersion,
    },
    entry::entry_point::SegmentEntry,
    id_tracker::IdTracker,
    index::{
        hnsw_index::{
            graph_links::GraphLinksMmap,
            hnsw::HNSWIndex,
        },
        plain_payload_index::PlainIndex,
        struct_payload_index::StructPayloadIndex,
        PayloadIndex,
        VectorIndexEnum,
    },
    payload_storage::on_disk_payload_storage::OnDiskPayloadStorage,
    segment::{
        Segment,
        SegmentVersion,
        VectorData,
    },
    segment_constructor::{
        get_vector_index_path,
        get_vector_storage_path,
        segment_builder::SegmentBuilder,
        PAYLOAD_INDEX_PATH,
    },
    types::{
        Distance,
        HnswConfig,
        Indexes,
        PayloadStorageType,
        PointIdType,
        SegmentConfig,
        SegmentType,
        VectorDataConfig,
        VectorStorageType,
        DEFAULT_FULL_SCAN_THRESHOLD,
        DEFAULT_HNSW_EF_CONSTRUCT,
    },
    vector_storage::VectorStorage,
};
use rocksdb::DB;
use serde_json::Value as JsonValue;
use uuid::Uuid;
use value::base64;

use crate::{
    id_tracker::{
        VectorMemoryIdTracker,
        VectorStaticIdTracker,
    },
    metrics,
    qdrant_index::TIMESTAMP_FIELD,
};

const UUID_TABLE_FILENAME: &str = "uuids.table";
const DELETED_BITSET_FILENAME: &str = "deleted.bitset";
pub(crate) const DEFAULT_VECTOR_NAME: &str = "default_vector";

pub(crate) fn segment_config(
    dimension: usize,
    mutable: bool,
    max_indexing_threads: usize,
) -> SegmentConfig {
    let index = if mutable {
        Indexes::Plain {}
    } else {
        let hnsw_config = HnswConfig {
            // Number of edges per node in the index graph. Larger the value -
            // more accurate the search, more space required.
            m: 16,
            // Number of neighbours to consider during the index building.
            // Larger  the value - more accurate the search, more
            // time required to build index.
            ef_construct: DEFAULT_HNSW_EF_CONSTRUCT,
            // Minimal size (in KiloBytes) of vectors for additional
            // payload-based indexing. If payload chunk is smaller
            // than `full_scan_threshold_kb` additional indexing
            // won't be used - in this case full-scan search should be
            // preferred by query planner and additional indexing
            // is not required. Note: 1Kb = 1 vector of size 256
            full_scan_threshold: DEFAULT_FULL_SCAN_THRESHOLD,
            max_indexing_threads,
            on_disk: Some(true),
            // Custom M param for hnsw graph built for payload index. If not
            // set, default M will be used.
            payload_m: None,
        };
        Indexes::Hnsw(hnsw_config)
    };
    let vector_storage_type = if mutable {
        VectorStorageType::ChunkedMmap
    } else {
        VectorStorageType::Mmap
    };
    let vector_data_config = VectorDataConfig {
        size: dimension,
        distance: Distance::Cosine,
        storage_type: vector_storage_type,
        index,
        quantization_config: None,
    };
    SegmentConfig {
        vector_data: HashMap::from([(DEFAULT_VECTOR_NAME.to_string(), vector_data_config)]),
        sparse_vector_data: Default::default(),
        payload_storage_type: PayloadStorageType::OnDisk,
    }
}

pub fn create_mutable_segment(
    path: &Path,
    id_tracker: Arc<AtomicRefCell<VectorMemoryIdTracker>>,
    dimension: usize,
    segment_config: SegmentConfig,
) -> anyhow::Result<Segment> {
    fs::create_dir_all(path)?;

    let vector_db_names = vec![format!("{DB_VECTOR_CF}-{DEFAULT_VECTOR_NAME}")];
    let database = open_db(path, &vector_db_names, false)?;
    let payload_storage = OnDiskPayloadStorage::open(database.clone())?;

    let payload_index_path = path.join(PAYLOAD_INDEX_PATH);
    let payload_index = StructPayloadIndex::open(
        Arc::new(AtomicRefCell::new(payload_storage.into())),
        id_tracker.clone(),
        &payload_index_path,
        true,
    )?;
    let payload_index = Arc::new(AtomicRefCell::new(payload_index));

    let stopped = AtomicBool::new(false);
    let vector_storage_path = get_vector_storage_path(path, DEFAULT_VECTOR_NAME);
    let vector_storage = open_appendable_memmap_vector_storage(
        &vector_storage_path,
        dimension,
        Distance::Cosine,
        &stopped,
    )?;
    let point_count = id_tracker.borrow().total_point_count();
    let vector_count = vector_storage.borrow().total_vector_count();
    anyhow::ensure!(point_count == vector_count);

    let vector_index = VectorIndexEnum::Plain(PlainIndex::new(
        id_tracker.clone(),
        vector_storage.clone(),
        payload_index.clone(),
    ));
    let vector_index = Arc::new(AtomicRefCell::new(vector_index));
    let vector_data = VectorData {
        vector_storage,
        vector_index,
        quantized_vectors: Arc::new(Default::default()),
    };
    let segment = Segment {
        version: None,
        persisted_version: Arc::new(Mutex::new(None)),
        current_path: path.to_owned(),
        id_tracker,
        vector_data: HashMap::from([(DEFAULT_VECTOR_NAME.to_string(), vector_data)]),
        segment_type: SegmentType::Plain,
        appendable_flag: true,
        payload_index,
        segment_config,
        error_status: None,
        database,
        flush_thread: Mutex::new(None),
    };
    segment.save_current_state()?;
    SegmentVersion::save(path)?;
    Ok(segment)
}

/// A set of paths for a fragmented vector segment where `segment` points to a
/// single tar file generated by snapshotting a qdrant segment.
#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub struct VectorDiskSegmentPaths {
    pub segment: PathBuf,
    pub uuids: PathBuf,
    pub deleted_bitset: PathBuf,
}

/// A set of paths for a fragmented vector segment where `segment_dir` points to
/// a directory into which we've unpacked a tar file generated from snapshotting
/// a qdrant segment.
#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub struct UntarredVectorDiskSegmentPaths {
    segment_dir: PathBuf,
    uuids: PathBuf,
    deleted_bitset: PathBuf,
}

impl UntarredVectorDiskSegmentPaths {
    pub fn from(untarred_segment: PathBuf, paths: VectorDiskSegmentPaths) -> Self {
        Self::new(untarred_segment, paths.uuids, paths.deleted_bitset)
    }

    // We could verify that untarred_segment is a directory here, but we don't
    // want to do blocking IO on tokio's threads and we can't rely on tokio
    // being present to safely do non-blocking IO.
    pub fn new(untarred_segment: PathBuf, uuids: PathBuf, deleted_bitset: PathBuf) -> Self {
        Self {
            segment_dir: untarred_segment,
            uuids,
            deleted_bitset,
        }
    }
}

#[derive(Debug)]
pub struct VectorDiskSegmentValues {
    pub paths: VectorDiskSegmentPaths,
    pub num_vectors: u32,
    pub num_deleted: u32,
}

pub fn build_disk_segment(
    segment: &Segment,
    tmp_path: &Path,
    disk_path: &Path,
    segment_config: SegmentConfig,
) -> anyhow::Result<VectorDiskSegmentValues> {
    merge_disk_segments(vec![(None, segment)], tmp_path, disk_path, segment_config)
}

pub fn merge_disk_segments_hnsw(
    segments: Vec<(Option<UntarredVectorDiskSegmentPaths>, &Segment)>,
    dimension: usize,
    tmp_path: &Path,
    disk_path: &Path,
) -> anyhow::Result<VectorDiskSegmentValues> {
    let segment_config = segment_config(dimension, false, 4);
    merge_disk_segments(segments, tmp_path, disk_path, segment_config)
}

/// Reads the `_ts` (revision timestamp) a point's payload was written with,
/// used to pick which duplicate copy to keep.
fn read_point_ts(segment: &Segment, internal_id: u32) -> anyhow::Result<u64> {
    let payload = segment.payload_index.borrow().payload(internal_id)?;
    let ts = payload
        .0
        .get(TIMESTAMP_FIELD)
        .and_then(JsonValue::as_str)
        .and_then(|ts_b64| base64::decode_urlsafe(ts_b64).ok())
        .and_then(|ts_bytes| <[u8; 8]>::try_from(&ts_bytes[..]).ok())
        .map(u64::from_le_bytes)
        .context("Missing _ts field")?;
    Ok(ts)
}

/// Resolves documents that are live (non-deleted) in more than one of the input
/// segments by keeping the newest copy (by `_ts`) and marking the older copies
/// deleted in their (in-memory) deleted bitsets.
///
/// This should be a no-op: each document is supposed to live in exactly one
/// segment. But a historical flusher bug could leave the stale copy of an
/// *updated* document live in an older segment while its new copy lived in a
/// newer one.
fn resolve_duplicate_points(
    segments: &[(Option<UntarredVectorDiskSegmentPaths>, &Segment)],
) -> anyhow::Result<()> {
    // Cheap first pass over just the id trackers: find external ids that are
    // live in more than one segment.
    let mut locations: HashMap<Uuid, Vec<(usize, u32)>> = HashMap::new();
    for (segment_idx, (_paths, segment)) in segments.iter().enumerate() {
        let id_tracker = segment.id_tracker.borrow();
        for internal_id in id_tracker.iter_ids() {
            let Some(PointIdType::Uuid(uuid)) = id_tracker.external_id(internal_id) else {
                anyhow::bail!("Segment point {internal_id} has no external UUID");
            };
            locations
                .entry(uuid)
                .or_default()
                .push((segment_idx, internal_id));
        }
    }

    let mut num_deduped = 0;
    for (uuid, copies) in locations {
        if copies.len() < 2 {
            continue;
        }
        // Only read payloads (to compare timestamps) for the rare duplicates.
        let mut newest: Option<(u64, usize, u32)> = None;
        for &(segment_idx, internal_id) in &copies {
            let ts = read_point_ts(segments[segment_idx].1, internal_id)?;
            if newest.map(|(latest_ts, ..)| ts > latest_ts).unwrap_or(true) {
                newest = Some((ts, segment_idx, internal_id));
            }
        }
        let (_, keep_segment_idx, keep_internal_id) =
            newest.expect("`copies` is non-empty so `newest` is always set");
        for &(segment_idx, internal_id) in &copies {
            if segment_idx == keep_segment_idx && internal_id == keep_internal_id {
                continue;
            }
            // Soft-delete the stale copy so `update_from` (which only copies
            // live points via `iter_ids`) excludes it from the merge.
            segments[segment_idx]
                .1
                .id_tracker
                .borrow_mut()
                .drop(PointIdType::Uuid(uuid))?;
            num_deduped += 1;
        }
        tracing::info!(
            "Resolved duplicate vector document {uuid} present in {} segments; kept the newest \
             copy and deleted {} stale copies. This indicates segments poisoned by a past flusher \
             bug.",
            copies.len(),
            copies.len() - 1,
        );
    }

    if num_deduped > 0 {
        metrics::log_compaction_deduped_documents(num_deduped);
    }
    Ok(())
}

pub fn merge_disk_segments(
    segments: Vec<(Option<UntarredVectorDiskSegmentPaths>, &Segment)>,
    tmp_path: &Path,
    disk_path: &Path,
    segment_config: SegmentConfig,
) -> anyhow::Result<VectorDiskSegmentValues> {
    // Create separate tmp paths to avoid unexpected overlap with the original
    // segment or various intermediate outputs. Because qdrant restores segments
    // to paths adjacent to the path where we save the snapshot, we want to
    // ensure that the directory where we create the snapshot (disk_path) is
    // clean.
    let tmp_segment_path = tmp_path.join("merged_segment");
    std::fs::create_dir(tmp_segment_path.clone())?;
    let segment_tmp_dir_path = tmp_path.join("merged_segment_tmp");
    std::fs::create_dir(segment_tmp_dir_path.clone())?;
    let snapshot_tmp_path = tmp_path.join("snapshot_tmp");
    std::fs::create_dir(snapshot_tmp_path.clone())?;

    resolve_duplicate_points(&segments)?;

    let mut segment_builder =
        SegmentBuilder::new(&tmp_segment_path, &segment_tmp_dir_path, &segment_config)?;
    let stopped = AtomicBool::new(false);
    for (paths, segment) in segments {
        tracing::info!("Updating new segment with segment from paths {:?}", paths);
        segment_builder.update_from(segment, &stopped)?;
        if let Some(ref segment) = segment_builder.segment {
            anyhow::ensure!(segment.id_tracker.borrow().deleted_point_count() == 0);
        }
    }
    let permit = CpuPermit::dummy(4);
    let disk_segment = segment_builder.build(permit, &stopped)?;

    // The disk segment we just built was using a qdrant id tracker. We now need to
    // construct our own id tracker with the same set of ids. We could do this
    // by making SegmentBuilder use our id tracker if this turns out to be a
    // performance issue. However doing so requires a lot of additional code to
    // duplicate the logic SegmentBuilder is using to create the segment.
    // Ideally the performance penalty here is small relative to the overall
    // build cost and is worth the simpler code.
    let mut memory_tracker = VectorMemoryIdTracker::new();
    let borrowed_tracker = disk_segment.id_tracker.borrow();
    // All deletes should be hard deletes. Deleted vectors should be excluded
    // entirely, not marked as soft deleted.
    anyhow::ensure!(borrowed_tracker.deleted_point_count() == 0);

    for internal_id in borrowed_tracker.iter_internal() {
        let external_id = borrowed_tracker
            .external_id(internal_id)
            .expect("Missing external id!");
        memory_tracker.set_link(external_id, internal_id)?;
    }

    // Sanity check that we included all of the points.
    anyhow::ensure!(memory_tracker.total_point_count() == borrowed_tracker.total_point_count());

    // Sanity check that we didn't insert multiple vectors for any given id.
    let total_point_count = memory_tracker.total_point_count();
    let vector_data = disk_segment.vector_data.get(DEFAULT_VECTOR_NAME).unwrap();
    let vector_count = vector_data.vector_storage.borrow().total_vector_count();
    let num_deleted = memory_tracker.deleted_point_count();
    anyhow::ensure!(vector_count == total_point_count);
    // Writing the new segment should have removed all deletes.
    anyhow::ensure!(num_deleted == 0);

    let memory_tracker = Arc::new(AtomicRefCell::new(memory_tracker));
    let segment = snapshot_segment(
        &memory_tracker,
        &disk_segment,
        &snapshot_tmp_path,
        disk_path,
    )?;
    Ok(VectorDiskSegmentValues {
        paths: segment,
        num_vectors: total_point_count as u32,
        num_deleted: num_deleted as u32,
    })
}

pub fn snapshot_segment(
    id_tracker: &Arc<AtomicRefCell<VectorMemoryIdTracker>>,
    segment: &Segment,
    tmp_path: &Path,
    index_path: &Path,
) -> anyhow::Result<VectorDiskSegmentPaths> {
    let segment_path = segment.take_snapshot(tmp_path, index_path)?;

    // Write out our additional index files for the ID tracker.
    let mut id_tracker = id_tracker.borrow_mut();
    let uuids_path = index_path.join(UUID_TABLE_FILENAME);
    {
        let mut out = BufWriter::new(File::create(uuids_path.clone())?);
        id_tracker.write_uuids(&mut out)?;
        out.into_inner()?.sync_all()?;
    }
    let deleted_bitset_path = index_path.join(DELETED_BITSET_FILENAME);
    {
        let mut out = BufWriter::new(File::create(deleted_bitset_path.clone())?);
        id_tracker.write_deleted_bitset(&mut out)?;
        out.into_inner()?.sync_all()?;
    }
    Ok(VectorDiskSegmentPaths {
        segment: segment_path,
        uuids: uuids_path,
        deleted_bitset: deleted_bitset_path,
    })
}

pub async fn restore_segment_from_tar(archive_path: &Path) -> anyhow::Result<PathBuf> {
    // This is taken directly from Qdrant's tests...
    let segment_id = archive_path
        .file_stem()
        .and_then(|f| f.to_str())
        .unwrap()
        .to_owned();

    // As is this...
    let out_path = archive_path
        .parent()
        .expect("Failed to obtain parent for archive")
        .join(&segment_id);

    let archive_path = archive_path.to_owned();
    tokio_spawn_blocking("segment_restore_snapshot", move || {
        Segment::restore_snapshot(&archive_path, &segment_id)
    })
    .await??;

    Ok(out_path)
}

fn open_db<T: AsRef<str>>(
    path: &Path,
    vector_pathes: &[T],
    read_only: bool,
) -> Result<Arc<RwLock<DB>>, rocksdb::Error> {
    let mut column_families = vec![DB_PAYLOAD_CF, DB_MAPPING_CF, DB_VERSIONS_CF];
    for vector_path in vector_pathes {
        column_families.push(vector_path.as_ref());
    }

    let db = if read_only {
        DB::open_cf_for_read_only(&db_options(), path, column_families, false)?
    } else {
        DB::open_cf(&db_options(), path, column_families)?
    };

    Ok(Arc::new(RwLock::new(db)))
}

pub fn load_disk_segment(paths: UntarredVectorDiskSegmentPaths) -> anyhow::Result<Segment> {
    let untarred_path = paths.segment_dir;
    if !SegmentVersion::check_exists(&untarred_path) {
        anyhow::bail!("Missing segment version for {untarred_path:?}");
    }
    let stored_version = SegmentVersion::load(&untarred_path)?;
    let app_version = SegmentVersion::current();
    anyhow::ensure!(stored_version == app_version);

    let segment_state = Segment::load_state(&untarred_path)?;
    let vector_config = &segment_state.config.vector_data[DEFAULT_VECTOR_NAME];
    let segment_config = &segment_state.config;
    let vector_db_names = vec![format!("{DB_VECTOR_CF}-{DEFAULT_VECTOR_NAME}")];
    let database = open_db(&untarred_path, &vector_db_names, true)?;

    let payload_storage = OnDiskPayloadStorage::open(database.clone())?;

    let deleted_bitset = DeletedBitset::load_from_path(paths.deleted_bitset)?;
    let id_tracker = VectorStaticIdTracker {
        id_tracker: StaticIdTracker::load_from_path(paths.uuids)?,
        deleted_bitset,
    };
    let id_tracker = Arc::new(AtomicRefCell::new(id_tracker));

    let payload_index_path = untarred_path.join(PAYLOAD_INDEX_PATH);

    let payload_index = StructPayloadIndex::open_read_only(
        Arc::new(AtomicRefCell::new(payload_storage.into())),
        id_tracker.clone(),
        &payload_index_path,
    )?;
    let payload_index = Arc::new(AtomicRefCell::new(payload_index));

    let vector_storage_path = get_vector_storage_path(&untarred_path, DEFAULT_VECTOR_NAME);
    let vector_index_path = get_vector_index_path(&untarred_path, DEFAULT_VECTOR_NAME);
    let vector_storage = match vector_config.storage_type {
        VectorStorageType::Memory => anyhow::bail!("VectorStorageType::Memory is unsupported"),
        VectorStorageType::Mmap => open_memmap_vector_storage(
            &vector_storage_path,
            vector_config.size,
            vector_config.distance,
        )?,
        VectorStorageType::ChunkedMmap => {
            let stopped = AtomicBool::new(false);
            open_appendable_memmap_vector_storage(
                &vector_storage_path,
                vector_config.size,
                vector_config.distance,
                &stopped,
            )?
        },
    };
    let point_count = id_tracker.borrow().total_point_count();
    let vector_count = vector_storage.borrow().total_vector_count();
    anyhow::ensure!(vector_count == point_count);

    let vector_index = match vector_config.index {
        qdrant_segment::types::Indexes::Plain {} => VectorIndexEnum::Plain(PlainIndex::new(
            id_tracker.clone(),
            vector_storage.clone(),
            payload_index.clone(),
        )),
        qdrant_segment::types::Indexes::Hnsw(ref hnsw_config) => {
            anyhow::ensure!(
                hnsw_config.on_disk.unwrap_or(false),
                "HNSW indexes are only supported when they're written to disk!"
            );
            VectorIndexEnum::HnswMmap(HNSWIndex::<GraphLinksMmap>::open(
                &vector_index_path,
                id_tracker.clone(),
                vector_storage.clone(),
                Arc::new(AtomicRefCell::new(None)),
                payload_index.clone(),
                hnsw_config.clone(),
            )?)
        },
    };
    let vector_index = Arc::new(AtomicRefCell::new(vector_index));

    let vector_data = VectorData {
        vector_storage,
        vector_index,
        quantized_vectors: Arc::new(AtomicRefCell::new(None)),
    };
    let segment = Segment {
        version: segment_state.version,
        persisted_version: Arc::new(Mutex::new(segment_state.version)),
        current_path: untarred_path,
        id_tracker,
        vector_data: HashMap::from([(DEFAULT_VECTOR_NAME.to_string(), vector_data)]),
        segment_type: if segment_config.is_any_vector_indexed() {
            SegmentType::Indexed
        } else {
            SegmentType::Plain
        },
        appendable_flag: false,
        payload_index,
        segment_config: segment_config.clone(),
        error_status: None,
        database,
        flush_thread: Mutex::new(None),
    };
    Ok(segment)
}

pub trait SegmentConfigExt {
    fn dimensions(&self) -> usize;
}

impl SegmentConfigExt for SegmentConfig {
    fn dimensions(&self) -> usize {
        self.vector_data[DEFAULT_VECTOR_NAME].size
    }
}
