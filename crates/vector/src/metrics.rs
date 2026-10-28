use metrics::{
    cluster_label,
    log_counter,
    log_counter_with_labels,
    log_distribution,
    register_convex_counter,
    register_convex_histogram,
    MetricLabel,
    StaticMetricLabel,
    StatusTimer,
    CLUSTER_LABEL,
    STATUS_LABEL,
};

use crate::{
    qdrant_index::QdrantVectorIndexType,
    query::CompiledVectorFilter,
    CompiledVectorSearch,
    VectorSearchQueryResult,
};

pub enum IndexUpdateType {
    IndexMetadata,
    Document,
    None,
}

impl IndexUpdateType {
    fn tag(&self) -> &'static str {
        match self {
            IndexUpdateType::IndexMetadata => "index_metadata",
            IndexUpdateType::Document => "document",
            IndexUpdateType::None => "none",
        }
    }
}

register_convex_histogram!(
    VECTOR_INDEX_MANAGER_UPDATE_SECONDS,
    "Duration of a vector index update",
    &[STATUS_LABEL[0], "index_update_type"],
);
pub fn index_manager_update_timer() -> StatusTimer {
    let mut timer = StatusTimer::new(&VECTOR_INDEX_MANAGER_UPDATE_SECONDS);
    timer.add_label(StaticMetricLabel::new("index_update_type", "unknown"));
    timer
}

pub fn finish_index_manager_update_timer(
    mut timer: StatusTimer,
    index_update_type: IndexUpdateType,
) {
    timer.replace_label(
        StaticMetricLabel::new("index_update_type", "unknown"),
        StaticMetricLabel::new("index_update_type", index_update_type.tag()),
    );
    timer.finish();
}

register_convex_histogram!(
    VECTOR_SEARCH_SCHEMA_COMPILE_SECONDS,
    "Time to compile a search schema",
    &STATUS_LABEL
);
pub fn compile_timer() -> StatusTimer {
    StatusTimer::new(&VECTOR_SEARCH_SCHEMA_COMPILE_SECONDS)
}

register_convex_histogram!(
    VECTOR_SEARCH_SEARCHLIGHT_OVERFETCH_DELTA_TOTAL,
    "Size of the vector searchlight overfetch delta"
);
pub fn log_searchlight_overfetch_delta(overfetch_delta: usize) {
    log_distribution(
        &VECTOR_SEARCH_SEARCHLIGHT_OVERFETCH_DELTA_TOTAL,
        overfetch_delta as f64,
    );
}

register_convex_histogram!(
    VECTOR_SEARCH_SEARCHLIGHT_CLIENT_EXECUTE_SECONDS,
    "Time to execute a vector query against Searchlight",
    &[STATUS_LABEL[0], VECTOR_INDEX_TYPE_LABEL, CLUSTER_LABEL],
);
pub fn searchlight_client_execute_timer(
    vector_index_type: VectorIndexType,
    cluster: &'static str,
) -> StatusTimer {
    let mut timer = StatusTimer::new(&VECTOR_SEARCH_SEARCHLIGHT_CLIENT_EXECUTE_SECONDS);
    timer.add_label(vector_index_type_label(vector_index_type));
    timer.add_label(cluster_label(cluster));
    timer
}

register_convex_histogram!(
    VECTOR_SEARCH_SEARCHLIGHT_CLIENT_RESULTS_TOTAL,
    "Number of vector results from Searchlight"
);
pub fn finish_searchlight_client_execute(
    timer: StatusTimer,
    results: &Vec<VectorSearchQueryResult>,
) {
    log_distribution(
        &VECTOR_SEARCH_SEARCHLIGHT_CLIENT_RESULTS_TOTAL,
        results.len() as f64,
    );
    timer.finish();
}

register_convex_histogram!(
    VECTOR_SEARCH_NUM_DISCARDED_REVISIONS_TOTAL,
    "Number of vector discarded revisions"
);
pub fn log_num_discarded_revisions(discarded_revisions: usize) {
    log_distribution(
        &VECTOR_SEARCH_NUM_DISCARDED_REVISIONS_TOTAL,
        discarded_revisions as f64,
    );
}

register_convex_histogram!(
    VECTOR_SEARCH_NUMBER_OF_SEGMENTS_TOTAL,
    "Number of vector segments searched for a multi segment vector index"
);
pub fn log_num_segments_searched_total(num_segments: usize) {
    log_distribution(&VECTOR_SEARCH_NUMBER_OF_SEGMENTS_TOTAL, num_segments as f64);
}

fn log_vector_search_total(filter: &str) {
    log_counter_with_labels(
        &VECTOR_SEARCH_COMPILE_TOTAL,
        1,
        vec![MetricLabel::new("filter_type", filter)],
    );
}

register_convex_counter!(
    VECTOR_SEARCH_COMPILE_TOTAL,
    "Number of vector searches that are compiled",
    &["filter_type"]
);
register_convex_histogram!(
    VECTOR_SEARCH_COMPILE_FILTER_IN_TOTAL,
    "Number of terms in an IN vector search filter",
);
register_convex_histogram!(
    VECTOR_SEARCH_VECTOR_LENGTH_TOTAL,
    "The size of the vector being searched by vector search",
);
pub fn log_compiled_query(query: &CompiledVectorSearch) {
    if query.filter_conditions.is_empty() {
        log_vector_search_total("none");
    } else if query.filter_conditions.len() == 1 {
        for filter in query.filter_conditions.values() {
            match filter {
                CompiledVectorFilter::Eq(_) => log_vector_search_total("eq"),
                CompiledVectorFilter::In(vec) => {
                    log_vector_search_total("in");
                    log_distribution(&VECTOR_SEARCH_COMPILE_FILTER_IN_TOTAL, vec.len() as f64);
                },
            }
        }
    } else {
        log_vector_search_total("multifield");
    }
    log_distribution(
        &VECTOR_SEARCH_VECTOR_LENGTH_TOTAL,
        query.vector.len() as f64,
    )
}

register_convex_histogram!(
    VECTOR_INDEX_MANAGER_SEARCH_SECONDS,
    "Total vector search duration",
    &[STATUS_LABEL[0], VECTOR_INDEX_TYPE_LABEL, CLUSTER_LABEL],
);
pub fn search_timer(cluster: &'static str) -> StatusTimer {
    let mut timer = StatusTimer::new(&VECTOR_INDEX_MANAGER_SEARCH_SECONDS);
    timer.add_label(vector_index_type_label(VectorIndexType::Unknown));
    timer.add_label(cluster_label(cluster));
    timer
}

register_convex_histogram!(
    VECTOR_INDEX_MANAGER_RESULTS_TOTAL,
    "Number of results from the vector index manager"
);

pub fn finish_search(
    mut timer: StatusTimer,
    results: &Vec<VectorSearchQueryResult>,
    vector_index_type: VectorIndexType,
) {
    log_distribution(&VECTOR_INDEX_MANAGER_RESULTS_TOTAL, results.len() as f64);
    timer.add_label(vector_index_type_label(vector_index_type));
    timer.finish();
}

register_convex_counter!(
    VECTOR_UPDATE_INDEX_CREATED_TOTAL,
    "Number of vector indexes created"
);
pub fn log_index_created() {
    log_counter(&VECTOR_UPDATE_INDEX_CREATED_TOTAL, 1);
}

register_convex_counter!(
    VECTOR_UPDATE_INDEX_BACKFILLED_TOTAL,
    "Number of vector indexes backfilled"
);
pub fn log_index_backfilled() {
    log_counter(&VECTOR_UPDATE_INDEX_BACKFILLED_TOTAL, 1);
}

register_convex_counter!(
    VECTOR_COMPACTION_DEDUPED_DOCUMENTS_TOTAL,
    "Number of duplicate vector documents resolved during compaction by deleting the older copy. \
     A non-zero value indicates segments that were poisoned by a past flusher bug are being \
     healed; a sustained non-zero rate indicates a regression that is re-introducing duplicates."
);
pub fn log_compaction_deduped_documents(count: usize) {
    log_counter(&VECTOR_COMPACTION_DEDUPED_DOCUMENTS_TOTAL, count as u64);
}

register_convex_counter!(
    VECTOR_UPDATE_INDEX_ADVANCED_TOTAL,
    "Number of vector indexes advanced in time"
);
pub fn log_index_advanced() {
    log_counter(&VECTOR_UPDATE_INDEX_ADVANCED_TOTAL, 1);
}
register_convex_counter!(
    VECTOR_UPDATE_INDEX_DELETED_TOTAL,
    "Number of vector index deletions"
);
pub fn log_index_deleted() {
    log_counter(&VECTOR_UPDATE_INDEX_DELETED_TOTAL, 1);
}

const QDRANT_VECTOR_INDEX_TYPE: &str = "index_type";

impl QdrantVectorIndexType {
    fn metric_label(&self) -> StaticMetricLabel {
        let index_string = match self {
            QdrantVectorIndexType::Plain => "plain",
            QdrantVectorIndexType::HNSW => "hnsw",
        };
        StaticMetricLabel::new(QDRANT_VECTOR_INDEX_TYPE, index_string)
    }
}

register_convex_histogram!(
    QDRANT_SEGMENT_MEMORY_BUILD_SECONDS,
    "The amount of time it takes to build the appendable memory qdrant segment",
    &STATUS_LABEL,
);
pub fn qdrant_segment_memory_build_timer() -> StatusTimer {
    StatusTimer::new(&QDRANT_SEGMENT_MEMORY_BUILD_SECONDS)
}
register_convex_histogram!(
    QDRANT_SEGMENT_DISK_BUILD_SECONDS,
    "The amount of time it takes to build the hnsw indexed immutable qdrant segment",
    &[STATUS_LABEL[0], QDRANT_VECTOR_INDEX_TYPE],
);
pub fn qdrant_segment_disk_build_timer(disk_index_type: QdrantVectorIndexType) -> StatusTimer {
    let mut timer = StatusTimer::new(&QDRANT_SEGMENT_DISK_BUILD_SECONDS);
    timer.add_label(disk_index_type.metric_label());
    timer
}

#[derive(Clone, Copy, Debug)]
pub enum VectorIndexType {
    MultiSegment,
    Unknown,
}

pub const VECTOR_INDEX_TYPE_LABEL: &str = "vector_index_type";
pub fn vector_index_type_label(vector_index_type: VectorIndexType) -> StaticMetricLabel {
    let type_str = match vector_index_type {
        VectorIndexType::MultiSegment => "multi_segment",
        VectorIndexType::Unknown => "unknown",
    };
    StaticMetricLabel::new(VECTOR_INDEX_TYPE_LABEL, type_str)
}
