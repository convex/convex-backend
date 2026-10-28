use std::time::Duration;

use errors::ErrorMetadataAnyhowExt;
use metrics::{
    log_counter,
    log_counter_with_labels,
    log_distribution,
    log_distribution_with_labels,
    log_gauge,
    prometheus::VMHistogram,
    register_convex_counter,
    register_convex_gauge,
    register_convex_histogram,
    StaticMetricLabel,
    StatusTimer,
    Timer,
    STATUS_LABEL,
};

register_convex_counter!(
    SCHEDULED_JOB_RESULT_TOTAL,
    "Count of schedule job results",
    &STATUS_LABEL
);
register_convex_histogram!(
    SCHEDULED_JOB_PREV_FAILURES_TOTAL,
    "Num previous failures retried before success or failed attempt",
    &STATUS_LABEL
);
pub fn log_scheduled_job_success(prev_failures: u32) {
    log_counter_with_labels(
        &SCHEDULED_JOB_RESULT_TOTAL,
        1,
        vec![StaticMetricLabel::STATUS_SUCCESS],
    );
    log_distribution_with_labels(
        &SCHEDULED_JOB_PREV_FAILURES_TOTAL,
        prev_failures as f64,
        vec![StaticMetricLabel::STATUS_SUCCESS],
    );
}
pub fn log_scheduled_job_failure(e: &anyhow::Error, prev_failures: u32) {
    let label_value = e.metric_status_label_value();
    log_counter_with_labels(
        &SCHEDULED_JOB_RESULT_TOTAL,
        1,
        vec![StaticMetricLabel::new("status", label_value)],
    );
    log_distribution_with_labels(
        &SCHEDULED_JOB_PREV_FAILURES_TOTAL,
        prev_failures as f64,
        vec![StaticMetricLabel::new("status", label_value)],
    );
}

register_convex_histogram!(
    SCHEDULED_JOB_EXECUTION_LAG_SECONDS,
    "Schedule job execution lag"
);
pub fn log_scheduled_job_execution_lag(lag: Duration) {
    log_distribution(&SCHEDULED_JOB_EXECUTION_LAG_SECONDS, lag.as_secs_f64());
}

register_convex_gauge!(
    SCHEDULED_JOB_BACKLOG_SECONDS,
    "Age of the oldest runnable scheduled job; climbs while the executor is not starting runnable \
     work. Unlike the execution lag histogram, the gauge holds its last value while the executor \
     loop is wedged, so a stall is still visible. One executor per deployment reports it, so it \
     is only meaningful on dedicated partitions, which host a single deployment"
);
pub fn log_scheduled_job_backlog(backlog: Duration) {
    log_gauge(&SCHEDULED_JOB_BACKLOG_SECONDS, backlog.as_secs_f64())
}

register_convex_gauge!(
    SCHEDULED_JOB_RUNNING_JOBS_INFO,
    "Scheduled jobs the executor has started and not yet seen finish. Nonzero while no results \
     land means jobs are hanging in flight, which the backlog gauge cannot show because a started \
     job is no longer runnable. Reported per executor, so it is only meaningful on dedicated \
     partitions, which host a single deployment"
);
pub fn log_running_jobs(num_running: usize) {
    log_gauge(&SCHEDULED_JOB_RUNNING_JOBS_INFO, num_running as f64)
}

register_convex_counter!(
    SCHEDULED_JOB_EXECUTOR_ERRORS_TOTAL,
    "Failures of the scheduled job executor loop"
);
pub fn log_scheduled_job_executor_error() {
    log_counter(&SCHEDULED_JOB_EXECUTOR_ERRORS_TOTAL, 1)
}

register_convex_histogram!(
    SCHEDULED_JOB_NUM_RUNNING_TOTAL,
    "Number of currently executing scheduled jobs"
);
pub fn log_num_running_jobs(num_running: usize) {
    log_distribution(&SCHEDULED_JOB_NUM_RUNNING_TOTAL, num_running as f64);
}

register_convex_histogram!(
    RUN_SCHEDULED_JOBS_LOOP_SECONDS,
    "Time to run a single loop of the scheduled job executor",
);
pub fn run_scheduled_jobs_loop() -> Timer<VMHistogram> {
    Timer::new(&RUN_SCHEDULED_JOBS_LOOP_SECONDS)
}

register_convex_histogram!(
    SCHEDULED_JOBS_QUERY_SECONDS,
    "Time to query for the next runnable scheduled job",
    &STATUS_LABEL
);
pub fn query_scheduled_jobs_timer() -> StatusTimer {
    StatusTimer::new(&SCHEDULED_JOBS_QUERY_SECONDS)
}
