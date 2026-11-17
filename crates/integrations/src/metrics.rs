use metrics::{
    log_counter,
    log_counter_with_labels,
    register_convex_counter,
    StaticMetricLabel,
};

register_convex_counter!(
    DATADOG_SINK_LOGS_RECEIVED_TOTAL,
    "Number of received logs by the Datadog integration",
    &["version"],
);
pub fn datadog_integration_logs_received(count: usize, version: &'static str) {
    log_counter_with_labels(
        &DATADOG_SINK_LOGS_RECEIVED_TOTAL,
        count as u64,
        vec![StaticMetricLabel::new("version", version)],
    );
}

register_convex_counter!(
    DATADOG_SINK_LOGS_SENT_TOTAL,
    "Number of logs sent out successfully by Datadog integration",
    &["version"],
);
pub fn datadog_integration_logs_sent(count: usize, version: &'static str) {
    log_counter_with_labels(
        &DATADOG_SINK_LOGS_SENT_TOTAL,
        count as u64,
        vec![StaticMetricLabel::new("version", version)],
    );
}

register_convex_counter!(
    WEBHOOK_SINK_LOGS_RECEIVED_TOTAL,
    "Number of received logs by the Webhook integration",
);
pub fn webhook_integration_logs_received(count: usize) {
    log_counter(&WEBHOOK_SINK_LOGS_RECEIVED_TOTAL, count as u64);
}

register_convex_counter!(
    WEBHOOK_SINK_LOGS_SENT_TOTAL,
    "Number of logs sent out successfully by Webhook integration",
);
pub fn webhook_integration_logs_sent(count: usize) {
    log_counter(&WEBHOOK_SINK_LOGS_SENT_TOTAL, count as u64);
}

register_convex_counter!(
    LOG_SINK_EGRESS_FAILURES_TOTAL,
    "Number of integration requests a customer endpoint refused or failed to answer",
    &["sink_type", "outcome", "phase"],
);
/// `integration_type` comes from `IntegrationType::as_str` and `outcome` from
/// `IntegrationEgressFailure::outcome_label`; both are fixed sets. Never label
/// by deployment or instance.
fn integration_egress_failure_in(
    integration_type: &'static str,
    outcome: &'static str,
    phase: &'static str,
) {
    log_counter_with_labels(
        &LOG_SINK_EGRESS_FAILURES_TOTAL,
        1,
        vec![
            StaticMetricLabel::new("sink_type", integration_type),
            StaticMetricLabel::new("outcome", outcome),
            StaticMetricLabel::new("phase", phase),
        ],
    );
}

/// A batch an already-running integration failed to deliver.
pub fn integration_egress_failure(integration_type: &'static str, outcome: &'static str) {
    integration_egress_failure_in(integration_type, outcome, "runtime")
}

/// A credential check that failed while starting an integration.
pub fn integration_verification_failure(integration_type: &'static str, outcome: &'static str) {
    integration_egress_failure_in(integration_type, outcome, "startup")
}

register_convex_counter!(
    LOG_MANAGER_LOGS_RECEIVED_TOTAL,
    "Number of logs received by log manager",
);
pub fn log_manager_logs_received(count: usize) {
    log_counter(&LOG_MANAGER_LOGS_RECEIVED_TOTAL, count as u64);
}

register_convex_counter!(
    LOG_EVENT_DROPPED_OVERFLOW_ERROR_TOTAL,
    "Number of log events dropped due to an overflowing LogManager channel"
);
pub fn log_event_dropped_overflow_error(count: usize) {
    log_counter(&LOG_EVENT_DROPPED_OVERFLOW_ERROR_TOTAL, count as u64)
}

register_convex_counter!(
    LOG_EVENT_DROPPED_DISCONNECTED_ERROR_TOTAL,
    "Number of log events dropped due to a disconnected LogManager channel"
);
pub fn log_event_dropped_disconnected_error(count: usize) {
    log_counter(&LOG_EVENT_DROPPED_DISCONNECTED_ERROR_TOTAL, count as u64)
}

register_convex_counter!(
    LOG_EVENT_TOTAL,
    "Total number of log events sent through LogManager channel",
);
pub fn log_event_total(count: usize) {
    log_counter(&LOG_EVENT_TOTAL, count as u64)
}

register_convex_counter!(
    AXIOM_SINK_LOGS_RECEIVED_TOTAL,
    "Number of received logs by the Axiom integration",
    &["version"],
);
pub fn axiom_integration_logs_received(count: usize, version: &'static str) {
    log_counter_with_labels(
        &AXIOM_SINK_LOGS_RECEIVED_TOTAL,
        count as u64,
        vec![StaticMetricLabel::new("version", version)],
    );
}

register_convex_counter!(
    AXIOM_SINK_LOGS_SENT_TOTAL,
    "Number of logs sent out successfully by Axiom integration",
    &["version"],
);
pub fn axiom_integration_logs_sent(count: usize, version: &'static str) {
    log_counter_with_labels(
        &AXIOM_SINK_LOGS_SENT_TOTAL,
        count as u64,
        vec![StaticMetricLabel::new("version", version)],
    );
}

register_convex_counter!(
    SENTRY_SINK_LOGS_RECEIVED_TOTAL,
    "Number of received logs by the Sentry integration",
);
pub fn sentry_integration_logs_received(count: usize) {
    log_counter(&SENTRY_SINK_LOGS_RECEIVED_TOTAL, count as u64);
}

register_convex_counter!(
    SENTRY_SINK_LOGS_SENT_TOTAL,
    "Number of logs sent out successfully by Sentry integration",
);
pub fn sentry_integration_logs_sent(count: usize) {
    log_counter(&SENTRY_SINK_LOGS_SENT_TOTAL, count as u64);
}

register_convex_counter!(
    AXIOM_SINK_NETWORK_EGRESS_BYTES_TOTAL,
    "Number of bytes sent by Axiom integration",
);
pub fn axiom_integration_network_egress_bytes(bytes: u64) {
    log_counter(&AXIOM_SINK_NETWORK_EGRESS_BYTES_TOTAL, bytes);
}

register_convex_counter!(
    DATADOG_SINK_NETWORK_EGRESS_BYTES_TOTAL,
    "Number of bytes sent by Datadog integration",
);
pub fn datadog_integration_network_egress_bytes(bytes: u64) {
    log_counter(&DATADOG_SINK_NETWORK_EGRESS_BYTES_TOTAL, bytes);
}

register_convex_counter!(
    WEBHOOK_SINK_NETWORK_EGRESS_BYTES_TOTAL,
    "Number of bytes sent by Webhook integration",
);
pub fn webhook_integration_network_egress_bytes(bytes: u64) {
    log_counter(&WEBHOOK_SINK_NETWORK_EGRESS_BYTES_TOTAL, bytes);
}

register_convex_counter!(
    POSTHOG_LOGS_SINK_LOGS_RECEIVED_TOTAL,
    "Number of received logs by the PostHog Logs integration",
);
pub fn posthog_logs_integration_logs_received(count: usize) {
    log_counter(&POSTHOG_LOGS_SINK_LOGS_RECEIVED_TOTAL, count as u64);
}

register_convex_counter!(
    POSTHOG_LOGS_SINK_LOGS_SENT_TOTAL,
    "Number of logs sent out successfully by PostHog Logs integration",
);
pub fn posthog_logs_integration_logs_sent(count: usize) {
    log_counter(&POSTHOG_LOGS_SINK_LOGS_SENT_TOTAL, count as u64);
}

register_convex_counter!(
    POSTHOG_LOGS_SINK_NETWORK_EGRESS_BYTES_TOTAL,
    "Number of bytes sent by PostHog Logs integration",
);
pub fn posthog_logs_integration_network_egress_bytes(bytes: u64) {
    log_counter(&POSTHOG_LOGS_SINK_NETWORK_EGRESS_BYTES_TOTAL, bytes);
}

register_convex_counter!(
    POSTHOG_ET_SINK_LOGS_RECEIVED_TOTAL,
    "Number of received logs by the PostHog Error Tracking integration",
);
pub fn posthog_et_integration_logs_received(count: usize) {
    log_counter(&POSTHOG_ET_SINK_LOGS_RECEIVED_TOTAL, count as u64);
}

register_convex_counter!(
    POSTHOG_ET_SINK_LOGS_SENT_TOTAL,
    "Number of logs sent out successfully by PostHog Error Tracking integration",
);
pub fn posthog_et_integration_logs_sent(count: usize) {
    log_counter(&POSTHOG_ET_SINK_LOGS_SENT_TOTAL, count as u64);
}

register_convex_counter!(
    POSTHOG_ET_SINK_NETWORK_EGRESS_BYTES_TOTAL,
    "Number of bytes sent by PostHog Error Tracking integration",
);
pub fn posthog_et_integration_network_egress_bytes(bytes: u64) {
    log_counter(&POSTHOG_ET_SINK_NETWORK_EGRESS_BYTES_TOTAL, bytes);
}
