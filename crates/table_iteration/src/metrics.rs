use metrics::{
    prometheus::VMHistogramVec,
    register_convex_histogram,
    StaticMetricLabel,
    Timer,
};

register_convex_histogram!(
    TABLE_ITERATOR_PAGE_SECONDS,
    "Time spent in one phase of fetching a TableIterator page: `ts` picks the page's read \
     timestamp, `scan` runs the index scan",
    &["phase"]
);
pub fn page_timer(phase: &'static str) -> Timer<VMHistogramVec> {
    let mut timer = Timer::new_with_labels(&TABLE_ITERATOR_PAGE_SECONDS);
    timer.add_label(StaticMetricLabel::new("phase", phase));
    timer
}
