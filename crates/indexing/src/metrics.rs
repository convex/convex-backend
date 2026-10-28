use metrics::{
    log_counter,
    log_counter_with_labels,
    log_distribution,
    log_gauge,
    prometheus::{
        VMHistogram,
        VMHistogramVec,
    },
    register_convex_counter,
    register_convex_gauge,
    register_convex_histogram,
    IntoLabel,
    StaticMetricLabel,
    StatusTimer,
    Timer,
};

register_convex_counter!(
    TRANSACTION_INDEX_CACHE_HIT_TOTAL,
    "Count of transaction index cache reads, labeled with cache hits",
    &["hit"]
);

pub fn log_transaction_cache_query(hit: bool) {
    log_counter_with_labels(
        &TRANSACTION_INDEX_CACHE_HIT_TOTAL,
        1,
        vec![StaticMetricLabel::new("hit", hit.as_label())],
    );
}

register_convex_counter!(
    TRANSACTION_INDEX_CACHE_CLEARED_TOTAL,
    "Count of times transaction cache was cleared"
);
pub fn log_index_cache_cleared() {
    log_counter(&TRANSACTION_INDEX_CACHE_CLEARED_TOTAL, 1);
}

register_convex_histogram!(
    TRANSACTION_INDEX_CACHE_MAX_SIZE_BYTES,
    "Size of transaction cache before pruning to only intervals read"
);
pub fn log_transaction_index_cache_size(bytes: usize) {
    log_distribution(&TRANSACTION_INDEX_CACHE_MAX_SIZE_BYTES, bytes as f64);
}

register_convex_histogram!(
    TRANSACTION_INDEX_CACHE_RETAINED_SIZE_BYTES,
    "Size of transaction cache after pruning to just the intervals read"
);
pub fn log_transaction_index_cache_retained_size(bytes: usize) {
    log_distribution(&TRANSACTION_INDEX_CACHE_RETAINED_SIZE_BYTES, bytes as f64);
}

register_convex_histogram!(
    INDEX_PAGE_SECONDS,
    "Time to execute IndexReader::index_page in seconds",
    &["source", "status"]
);
pub fn index_page_timer(source: &'static str) -> StatusTimer {
    let mut t = StatusTimer::new(&INDEX_PAGE_SECONDS);
    t.add_label(StaticMetricLabel::new("source", source));
    t
}

register_convex_counter!(
    INDEX_PAGE_POINT_LOOKUP_TOTAL,
    "Count of index_page calls where the interval is a point lookup (single key)"
);
pub fn log_index_page_point_lookup() {
    log_counter(&INDEX_PAGE_POINT_LOOKUP_TOTAL, 1);
}

register_convex_counter!(
    INDEX_CACHE_INVALIDATION_TOTAL,
    "Count of IndexCache entries invalidated by writes"
);
pub fn log_index_cache_invalidation() {
    log_counter(&INDEX_CACHE_INVALIDATION_TOTAL, 1);
}

register_convex_counter!(
    INDEX_CACHE_SIZE_EVICTION_TOTAL,
    "Count of IndexCache entries evicted because the cache exceeded its capacity"
);
pub fn log_index_cache_size_eviction() {
    log_counter(&INDEX_CACHE_SIZE_EVICTION_TOTAL, 1);
}

register_convex_histogram!(
    INDEX_CACHE_GET_SECONDS,
    "Time to execute IndexCache::get in seconds",
    &["status"],
);
pub fn index_cache_get_timer() -> Timer<VMHistogramVec> {
    Timer::new_with_labels(&INDEX_CACHE_GET_SECONDS)
}

register_convex_histogram!(
    INDEX_CACHE_POPULATE_SECONDS,
    "Time to execute IndexCache::populate in seconds",
    &["result"],
);
pub fn index_cache_populate_timer() -> Timer<VMHistogramVec> {
    Timer::new_with_labels(&INDEX_CACHE_POPULATE_SECONDS)
}

register_convex_histogram!(
    INDEX_CACHE_APPLY_WRITES_SECONDS,
    "Time to execute IndexCache::apply_writes in seconds"
);
pub fn cache_apply_writes_timer() -> Timer<VMHistogram> {
    Timer::new(&INDEX_CACHE_APPLY_WRITES_SECONDS)
}

register_convex_gauge!(
    INDEX_CACHE_BYTES,
    "Approximate size in bytes used by IndexCache"
);
pub fn log_index_cache_size(size: u64) {
    log_gauge(&INDEX_CACHE_BYTES, size as f64);
}
