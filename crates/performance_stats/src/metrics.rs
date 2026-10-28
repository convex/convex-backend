use metrics::register_convex_int_gauge;

use crate::performance::JemallocStats;

register_convex_int_gauge!(
    APPLICATION_PROCESS_MEMORY_BYTES,
    "Process-level memory statistics in bytes",
    &["component"]
);
pub fn log_process_level_stats(memory_usage: usize, jemalloc: &JemallocStats) {
    APPLICATION_PROCESS_MEMORY_BYTES
        .with_label_values(&["memory_usage"])
        .set(memory_usage as i64);
    APPLICATION_PROCESS_MEMORY_BYTES
        .with_label_values(&["jemalloc_allocated"])
        .set(jemalloc.allocated as i64);
    APPLICATION_PROCESS_MEMORY_BYTES
        .with_label_values(&["jemalloc_active"])
        .set(jemalloc.active as i64);
    APPLICATION_PROCESS_MEMORY_BYTES
        .with_label_values(&["jemalloc_metadata"])
        .set(jemalloc.metadata as i64);
    APPLICATION_PROCESS_MEMORY_BYTES
        .with_label_values(&["jemalloc_resident"])
        .set(jemalloc.resident as i64);
    APPLICATION_PROCESS_MEMORY_BYTES
        .with_label_values(&["jemalloc_mapped"])
        .set(jemalloc.mapped as i64);
    APPLICATION_PROCESS_MEMORY_BYTES
        .with_label_values(&["jemalloc_retained"])
        .set(jemalloc.retained as i64);
}
