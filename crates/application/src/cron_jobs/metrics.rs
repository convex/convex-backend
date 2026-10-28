use std::time::Duration;

use errors::ErrorMetadataAnyhowExt;
use metrics::{
    log_counter,
    log_counter_with_labels,
    log_distribution,
    log_gauge,
    register_convex_counter,
    register_convex_gauge,
    register_convex_histogram,
    StaticMetricLabel,
    STATUS_LABEL,
};

register_convex_counter!(
    CRON_JOB_RESULT_TOTAL,
    "Number of cron job results",
    &STATUS_LABEL
);
register_convex_histogram!(
    CRON_JOB_PREV_FAILURES_TOTAL,
    "Num previous failures retried before success",
);
pub fn log_cron_job_success(prev_failures: u32) {
    log_counter_with_labels(
        &CRON_JOB_RESULT_TOTAL,
        1,
        vec![StaticMetricLabel::STATUS_SUCCESS],
    );
    log_distribution(&CRON_JOB_PREV_FAILURES_TOTAL, prev_failures as f64);
}
pub fn log_cron_job_failure(e: &anyhow::Error) {
    let label_value = e.metric_status_label_value();
    log_counter_with_labels(
        &CRON_JOB_RESULT_TOTAL,
        1,
        vec![StaticMetricLabel::new("status", label_value)],
    )
}

register_convex_histogram!(CRON_JOB_EXECUTION_LAG_SECONDS, "Cron job execution lag");
pub fn log_cron_job_execution_lag(lag: Duration) {
    log_distribution(&CRON_JOB_EXECUTION_LAG_SECONDS, lag.as_secs_f64());
}

register_convex_gauge!(
    CRON_JOB_BACKLOG_SECONDS,
    "Age of the oldest due cron run; climbs while the executor is not starting due crons. Unlike \
     the execution lag histogram, the gauge holds its last value while the executor loop is \
     wedged, so a stall is still visible. One executor per deployment reports it, so it is only \
     meaningful on dedicated partitions, which host a single deployment"
);
pub fn log_cron_job_backlog(backlog: Duration) {
    log_gauge(&CRON_JOB_BACKLOG_SECONDS, backlog.as_secs_f64())
}

register_convex_counter!(
    CRON_JOB_EXECUTOR_POLLS_TOTAL,
    "Iterations of the cron job executor loop; zero while backlog is nonzero means the executor \
     is wedged (the loop re-runs every 5s when work is waiting)"
);
pub fn log_cron_job_executor_poll() {
    log_counter(&CRON_JOB_EXECUTOR_POLLS_TOTAL, 1)
}

register_convex_gauge!(
    CRON_JOB_RUNNING_JOBS_INFO,
    "Cron runs the executor has started and not yet seen finish. Nonzero while no results land \
     means runs are hanging in flight, which the backlog gauge cannot show because a started run \
     is no longer due. Reported per executor, so it is only meaningful on dedicated partitions, \
     which host a single deployment"
);
pub fn log_running_jobs(num_running: usize) {
    log_gauge(&CRON_JOB_RUNNING_JOBS_INFO, num_running as f64)
}

register_convex_counter!(
    CRON_JOB_EXECUTOR_ERRORS_TOTAL,
    "Failures of the cron job executor loop"
);
pub fn log_cron_job_executor_error() {
    log_counter(&CRON_JOB_EXECUTOR_ERRORS_TOTAL, 1)
}
