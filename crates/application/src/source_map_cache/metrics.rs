use metrics::{
    register_convex_histogram,
    StatusTimer,
    STATUS_LABEL,
};

register_convex_histogram!(
    SOURCE_MAP_CACHE_GET_SECONDS,
    "Time taken to retrieve a source map from the cache",
    &STATUS_LABEL
);

pub fn source_map_cache_get_timer() -> StatusTimer {
    StatusTimer::new(&SOURCE_MAP_CACHE_GET_SECONDS)
}
