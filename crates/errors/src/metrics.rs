use metrics::{
    log_counter_with_labels,
    register_convex_counter,
    StaticMetricLabel,
};

register_convex_counter!(pub BAD_REQUEST_ERROR_TOTAL, "Count of bad request errors");
register_convex_counter!(pub CLIENT_DISCONNECT_ERROR_TOTAL, "Count of client disconnect errors");
register_convex_counter!(pub RATE_LIMITED_ERROR_TOTAL, "Count of rate limited errors");
register_convex_counter!(pub SYNC_AUTH_ERROR_TOTAL, "Count of sync auth errors");
register_convex_counter!(pub FORBIDDEN_ERROR_TOTAL, "Count of forbidden errors");
register_convex_counter!(pub COMMIT_RACE_TOTAL, "Total count of commit race errors");
register_convex_counter!(
    pub SAMPLED_CLIENT_ERROR_TOTAL,
    "Count of client-fault errors that are sampled before reaching Sentry, counted at full rate",
    &["family"]
);

pub fn log_sampled_client_error(family: &'static str) {
    log_counter_with_labels(
        &SAMPLED_CLIENT_ERROR_TOTAL,
        1,
        vec![StaticMetricLabel::new("family", family)],
    );
}
