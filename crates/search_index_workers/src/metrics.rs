use metrics::{
    log_distribution_with_labels,
    register_convex_histogram,
    StaticMetricLabel,
    StatusTimer,
    Timer,
    STATUS_LABEL,
};
use prometheus::VMHistogramVec;
use search::metrics::{
    SearchType,
    SEARCH_TYPE_LABEL,
};

pub enum SearchWriterLockWaiter {
    Compactor,
    Flusher,
}

const SEARCH_WRITER_WAITER_LABEL: &str = "waiter";

impl SearchWriterLockWaiter {
    fn tag(&self) -> StaticMetricLabel {
        let label = match self {
            SearchWriterLockWaiter::Compactor => "compactor",
            SearchWriterLockWaiter::Flusher => "flusher",
        };
        StaticMetricLabel::new(SEARCH_WRITER_WAITER_LABEL, label)
    }
}

register_convex_histogram!(
    SEARCH_WRITER_LOCK_WAIT_SECONDS,
    "The amount of time spent waiting for the writer lock to commit a vector/text index metadata \
     change",
    &[SEARCH_TYPE_LABEL, SEARCH_WRITER_WAITER_LABEL]
);
pub fn search_writer_lock_wait_timer(
    waiter: SearchWriterLockWaiter,
    search_type: SearchType,
) -> Timer<VMHistogramVec> {
    let mut timer = Timer::new_with_labels(&SEARCH_WRITER_LOCK_WAIT_SECONDS);
    timer.add_label(search_type.tag());
    timer.add_label(waiter.tag());
    timer
}

const MERGE_LABEL: &str = "merge_required";

pub enum SearchIndexMergeType {
    Unknown,
    Required,
    NotRequired,
}

impl SearchIndexMergeType {
    fn metric_label(&self) -> StaticMetricLabel {
        let label = match self {
            SearchIndexMergeType::Unknown => "unknown",
            SearchIndexMergeType::Required => "required",
            SearchIndexMergeType::NotRequired => "not_required",
        };
        StaticMetricLabel::new(MERGE_LABEL, label)
    }
}

register_convex_histogram!(
    SEARCH_COMPACTION_MERGE_COMMIT_SECONDS,
    "Time to merge deletes and commit after compaction",
    &[STATUS_LABEL[0], SEARCH_TYPE_LABEL, MERGE_LABEL],
);
pub fn search_compaction_merge_commit_timer(search_type: SearchType) -> StatusTimer {
    let mut timer = StatusTimer::new(&SEARCH_COMPACTION_MERGE_COMMIT_SECONDS);
    timer.add_label(search_type.tag());
    timer.add_label(SearchIndexMergeType::Unknown.metric_label());
    timer
}

register_convex_histogram!(
    SEARCH_FLUSH_MERGE_COMMIT_SECONDS,
    "Time to merge deletes and commit after flushing",
    &[STATUS_LABEL[0], SEARCH_TYPE_LABEL, MERGE_LABEL],
);
pub fn search_flush_merge_commit_timer(search_type: SearchType) -> StatusTimer {
    let mut timer = StatusTimer::new(&SEARCH_FLUSH_MERGE_COMMIT_SECONDS);
    timer.add_label(search_type.tag());
    timer.add_label(SearchIndexMergeType::Unknown.metric_label());
    timer
}

pub fn finish_search_index_merge_timer(mut timer: StatusTimer, merge_type: SearchIndexMergeType) {
    timer.replace_label(
        SearchIndexMergeType::Unknown.metric_label(),
        merge_type.metric_label(),
    );
    timer.finish();
}

register_convex_histogram!(
    DOCUMENTS_PER_NEW_SEARCH_SEGMENT_TOTAL,
    "Total number of documents in a newly built search index segment.",
    &[SEARCH_TYPE_LABEL],
);
pub fn log_documents_per_new_search_segment(count: u64, search_type: SearchType) {
    log_distribution_with_labels(
        &DOCUMENTS_PER_NEW_SEARCH_SEGMENT_TOTAL,
        count as f64,
        vec![search_type.tag()],
    );
}

register_convex_histogram!(
    DOCUMENTS_PER_SEARCH_SEGMENT_TOTAL,
    "Total number of documents in a specific search segment, including documents that were added \
     to the segment but are deleted and excluded from any search results",
    &[SEARCH_TYPE_LABEL],
);
pub fn log_documents_per_search_segment(count: u64, search_type: SearchType) {
    log_distribution_with_labels(
        &DOCUMENTS_PER_SEARCH_SEGMENT_TOTAL,
        count as f64,
        vec![search_type.tag()],
    );
}

register_convex_histogram!(
    NON_DELETED_DOCUMENTS_PER_SEARCH_SEGMENT_TOTAL,
    "Total number of non-deleted documents in a specific search segment, excluding documents that \
     were added to the segment but are deleted and excluded from any search results",
    &[SEARCH_TYPE_LABEL],
);
pub fn log_non_deleted_documents_per_search_segment(count: u64, search_type: SearchType) {
    log_distribution_with_labels(
        &NON_DELETED_DOCUMENTS_PER_SEARCH_SEGMENT_TOTAL,
        count as f64,
        vec![search_type.tag()],
    );
}

register_convex_histogram!(
    DOCUMENTS_PER_SEARCH_INDEX_TOTAL,
    "Total number of documents across all segments in a search index, including documents that \
     were added to the index but are deleted and excluded from any search results",
    &[SEARCH_TYPE_LABEL],
);
pub fn log_documents_per_search_index(count: u64, search_type: SearchType) {
    log_distribution_with_labels(
        &DOCUMENTS_PER_SEARCH_INDEX_TOTAL,
        count as f64,
        vec![search_type.tag()],
    );
}
register_convex_histogram!(
    NON_DELETED_DOCUMENTS_PER_SEARCH_INDEX_TOTAL,
    "Total number of non-deleted documents across all segments in a search index segment, \
     excluding documents that were added to the index but are deleted and excluded from any \
     search results",
    &[SEARCH_TYPE_LABEL],
);
pub fn log_non_deleted_documents_per_search_index(count: u64, search_type: SearchType) {
    log_distribution_with_labels(
        &NON_DELETED_DOCUMENTS_PER_SEARCH_INDEX_TOTAL,
        count as f64,
        vec![search_type.tag()],
    );
}

const COMPACTION_REASON_LABEL: &str = "compaction_reason";

#[derive(Clone, Copy, Debug)]
pub enum CompactionReason {
    SmallSegments,
    LargeSegments,
    Deletes,
}

impl CompactionReason {
    fn metric_label(&self) -> StaticMetricLabel {
        let label = match self {
            CompactionReason::SmallSegments => "small",
            CompactionReason::LargeSegments => "large",
            CompactionReason::Deletes => "deletes",
        };
        StaticMetricLabel::new(COMPACTION_REASON_LABEL, label)
    }
}

register_convex_histogram!(
    COMPACTION_BUILD_ONE_SECONDS,
    "Time to run a single vector/text index compaction",
    &[STATUS_LABEL[0], COMPACTION_REASON_LABEL, SEARCH_TYPE_LABEL],
);
pub fn compaction_build_one_timer(
    search_type: SearchType,
    reason: CompactionReason,
) -> StatusTimer {
    let mut timer = StatusTimer::new(&COMPACTION_BUILD_ONE_SECONDS);
    timer.add_label(search_type.tag());
    timer.add_label(reason.metric_label());
    timer
}

register_convex_histogram!(
    COMPACTION_COMPACTED_SEGMENTS_TOTAL,
    "Total number of compacted segments",
    &[SEARCH_TYPE_LABEL],
);
pub fn log_compaction_total_segments(total_segments: usize, search_type: SearchType) {
    log_distribution_with_labels(
        &COMPACTION_COMPACTED_SEGMENTS_TOTAL,
        total_segments as f64,
        vec![search_type.tag()],
    );
}

register_convex_histogram!(
    COMPACTION_COMPACTED_SEGMENT_NUM_DOCUMENTS_TOTAL,
    "The number of documents in the newly generated compacted segment",
    &[SEARCH_TYPE_LABEL],
);
pub fn log_compaction_compacted_segment_num_documents_total(
    total_vectors: u64,
    search_type: SearchType,
) {
    log_distribution_with_labels(
        &COMPACTION_COMPACTED_SEGMENT_NUM_DOCUMENTS_TOTAL,
        total_vectors as f64,
        vec![search_type.tag()],
    );
}

register_convex_histogram!(
    DATABASE_SEARCH_INDEX_BUILD_ONE_SECONDS,
    "Time to build one (multisegment) search index",
    &[STATUS_LABEL[0], SEARCH_TYPE_LABEL],
);
pub fn build_one_search_index_timer(search_type: SearchType) -> StatusTimer {
    let mut timer = StatusTimer::new(&DATABASE_SEARCH_INDEX_BUILD_ONE_SECONDS);
    timer.add_label(search_type.tag());
    timer
}
