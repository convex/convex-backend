use fastrace::{
    local::LocalSpan,
    Event,
};
use metrics::{
    log_counter_with_labels,
    log_distribution,
    log_distribution_with_labels,
    log_gauge,
    register_convex_counter,
    register_convex_gauge,
    register_convex_histogram,
    MetricLabel,
    StaticMetricLabel,
    Timer,
};
use prometheus::{
    VMHistogram,
    VMHistogramVec,
};

fn cache_metric_labels<'a>(table_name: &'a str) -> Vec<MetricLabel<'a>> {
    vec![MetricLabel::new("table", table_name)]
}

register_convex_counter!(
    MEMORY_INDEX_CACHE_GET_TOTAL,
    "Number of funrun index cache gets (hits + misses)",
    &["table"]
);
pub fn log_funrun_index_cache_get(table_name: &str) {
    log_counter_with_labels(
        &MEMORY_INDEX_CACHE_GET_TOTAL,
        1,
        cache_metric_labels(table_name),
    );
}

register_convex_histogram!(
    MEMORY_INDEX_CACHE_LOAD_INDEX_SECONDS,
    "Time to load an in-memory index for funrun",
    &["table"]
);
pub fn load_index_timer(table_name: &str) -> Timer<VMHistogramVec> {
    let mut t = Timer::new_with_labels(&MEMORY_INDEX_CACHE_LOAD_INDEX_SECONDS);
    t.add_label(StaticMetricLabel::new("table", table_name.to_owned()));
    t
}

register_convex_histogram!(
    MEMORY_INDEX_CACHE_LOADED_ROWS,
    "Number of rows loaded for an index",
    &["table"]
);
pub fn log_funrun_index_load_rows(rows: u64, table_name: &str) {
    log_distribution_with_labels(
        &MEMORY_INDEX_CACHE_LOADED_ROWS,
        rows as f64,
        cache_metric_labels(table_name),
    );
}

register_convex_histogram!(
    FUNCTION_RUNNER_BEGIN_TX_SECONDS,
    "Time to begin a transaction",
);
pub fn begin_tx_timer() -> Timer<VMHistogram> {
    Timer::new(&FUNCTION_RUNNER_BEGIN_TX_SECONDS)
}

register_convex_histogram!(MODULE_LOAD_SECONDS, "Time to load modules", &["source"]);
pub fn module_load_timer(source: &'static str) -> Timer<VMHistogramVec> {
    let mut timer = Timer::new_with_labels(&MODULE_LOAD_SECONDS);
    timer.add_label(MetricLabel::new_const("source", source));
    timer
}

register_convex_histogram!(
    MODULE_CACHE_SOURCE_SIZE_BYTES_TOTAL,
    "Size in bytes of module source code retrieved from cache",
);

register_convex_histogram!(
    MODULE_CACHE_SOURCE_MAP_SIZE_BYTES_TOTAL,
    "Size in bytes of module source maps retrieved from cache",
);
pub fn record_module_sizes(source_size: usize, source_map_size: Option<usize>) {
    log_distribution(&MODULE_CACHE_SOURCE_SIZE_BYTES_TOTAL, source_size as f64);
    if let Some(map_size) = source_map_size {
        log_distribution(&MODULE_CACHE_SOURCE_MAP_SIZE_BYTES_TOTAL, map_size as f64);
    }
    LocalSpan::add_event(Event::new("module_cache_get_module").with_properties(|| {
        [
            ("module_cache_source_size", source_size.to_string()),
            (
                "module_cache_source_map_size",
                source_map_size
                    .map(|s| s.to_string())
                    .unwrap_or("None".to_string()),
            ),
        ]
    }));
}

register_convex_gauge!(
    CODE_CACHE_SIZE_BYTES_TOTAL,
    "Size in bytes of the function runner V8 code cache",
);
pub fn record_code_cache_size(size: u64) {
    log_gauge(&CODE_CACHE_SIZE_BYTES_TOTAL, size as f64);
}
