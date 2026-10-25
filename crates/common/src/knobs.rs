//! Tunable limits and parameters for conductors.
//! These knobs can all be overridden in production by setting a key in Consul
//! at `conductor/<partition-id>/knobs/<knob-name>` and the desired value.
//!
//! See go/knobs-playbook for instructions.
//!
//! Every knob here should have a comment explaining what it's for and the
//! upper/lower bounds if applicable so an oncall engineer can adjust these
//! safely for a backend if needed.
//!
//! When running locally, these knobs can all be overridden with an environment
//! variable.
#![deny(missing_docs)]

use std::{
    cmp::{
        max,
        min,
    },
    num::{
        NonZeroU32,
        NonZeroUsize,
    },
    path::PathBuf,
    sync::LazyLock,
    time::Duration,
};

use cmd_util::env::env_config;

use crate::fastrace_helpers::SamplingConfig;

/// This exists solely to allow knobs to have separate defaults for local
/// execution and prod (running in Nomad). Don't export this outside of
/// this module. We assume that if we're running in Nomad, we're in production
/// which for now is always true.
static IS_PROD: LazyLock<bool> = LazyLock::new(|| std::env::var("NOMAD_ALLOC_ID").is_ok());

/// Returns the `local` value if we are running locally, and the `prod` value if
/// we are running in production. Just syntactic sugar to shorten the knob
/// declarations below.
/// Note that it's generally a bad idea to have separate configurations for
/// local development and production as our local test environment won't match
/// what's really running in production, but there are a few knobs where the
/// production default would make local development slower/harder.
fn prod_override<T>(local_value: T, prod_value: T) -> T {
    if *IS_PROD {
        return prod_value;
    }
    local_value
}

/// Set a consistent thread stack size regardless of environment. This is
/// 2x Rust's default: https://doc.rust-lang.org/nightly/std/thread/index.html#stack-size
pub static RUNTIME_STACK_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("RUNTIME_STACK_SIZE", 4 * 1024 * 1024));

/// 0 -> default (number of cores)
pub static RUNTIME_WORKER_THREADS: LazyLock<usize> =
    LazyLock::new(|| env_config("RUNTIME_WORKER_THREADS", 0));

/// Disable the Tokio scheduler's LIFO slot optimization, which may
/// help with tail latencies until they improve its implementation.
/// See https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html#method.disable_lifo_slot.
pub static RUNTIME_DISABLE_LIFO_SLOT: LazyLock<bool> =
    LazyLock::new(|| env_config("RUNTIME_DISABLE_LIFO_SLOT", true));

/// Maximum size of the UDF cache. Default 100MiB.
pub static UDF_CACHE_MAX_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("UDF_CACHE_MAX_SIZE", 104857600));

/// Maximum size of the shared UDF cache in Conductor. Default 1GiB.
pub static SHARED_UDF_CACHE_MAX_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("SHARED_UDF_CACHE_MAX_SIZE", 1024 * 1048576));

/// Fraction of `getUserIdentity()` calls from non-root components that log
/// the component path.
pub static COMPONENT_GET_USER_IDENTITY_LOG_SAMPLE_RATIO: LazyLock<f64> =
    LazyLock::new(|| env_config("COMPONENT_GET_USER_IDENTITY_LOG_SAMPLE_RATIO", 0.01));

/// How many UDF execution logs to keep in memory.
pub static MAX_UDF_EXECUTION: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_UDF_EXECUTION", 1000));

/// What is the metrics aggregation window for UDF metrics?
pub static UDF_METRICS_BUCKET_WIDTH: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("UDF_METRICS_BUCKET_WIDTH_SECS", 60)));

/// How many UDF metrics buckets do we keep in-memory? This defaults to 60s * 60
/// = 1 hour.
pub static UDF_METRICS_MAX_BUCKETS: LazyLock<usize> =
    LazyLock::new(|| env_config("UDF_METRICS_MAX_BUCKETS", 60));

/// Minimum duration to record in a histogram bucket.
pub static UDF_METRICS_MIN_DURATION: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("UDF_METRICS_MIN_DURATION_MS", 1)));

/// Maximum duration to record in a histogram bucket.
pub static UDF_METRICS_MAX_DURATION: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_millis(env_config("UDF_METRICS_MAX_DURATION_MS", 15 * 60 * 1000))
});

/// How many significant figures to store in a histogram bucket.
pub static UDF_METRICS_SIGNIFICANT_FIGURES: LazyLock<u8> =
    LazyLock::new(|| env_config("UDF_METRICS_SIGNIFICANT_FIGURES", 2));

/// How often to flush function activity reports to analytics (in seconds).
pub static UDF_ANALYTICS_POLL_TIME: LazyLock<u64> =
    LazyLock::new(|| env_config("UDF_ANALYTICS_POLL_TIME", 60));

/// How often conductor's app-metrics seed worker wakes up to check for
/// deployments whose Databricks seed query is due.
pub static APP_METRICS_SEED_SWEEP_INTERVAL: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("APP_METRICS_SEED_SWEEP_INTERVAL_SECS", 10)));

/// How often the usage-limit worker evaluates recorded usage against the
/// configured limits.
pub static USAGE_LIMIT_EVALUATE_INTERVAL: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("USAGE_LIMIT_EVALUATE_INTERVAL_SECS", 10)));

/// How long a graceful shutdown waits for in-flight usage-limit notifications
/// to finish delivering before cancelling the stragglers. A single delivery
/// retries with backoff, so this bounds the worst-case wait while keeping
/// shutdown finite.
pub static USAGE_LIMIT_NOTIFY_DRAIN_TIMEOUT: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("USAGE_LIMIT_NOTIFY_DRAIN_TIMEOUT_SECS", 5 * 60))
});

/// Upper bound on concurrently-tracked usage-limit delivery tasks. Each task
/// can live for its full retry schedule, so an unbounded backlog (e.g.
/// postalservice down while limits keep crossing) would grow memory without
/// limit; past this, new notifications are dropped rather than enqueued.
pub static USAGE_LIMIT_MAX_IN_FLIGHT_NOTIFICATIONS: LazyLock<usize> =
    LazyLock::new(|| env_config("USAGE_LIMIT_MAX_IN_FLIGHT_NOTIFICATIONS", 10_000));

/// webhook endpoint that receives "usage limit exceeded"
/// notifications.
/// Defaults to the empty string, which (together with a missing
/// `DEPLOYMENT_USAGE_WEBHOOK_SECRET`) disables usage-limit emails.
pub static DEPLOYMENT_USAGE_WEBHOOK_URL: LazyLock<String> =
    LazyLock::new(|| env_config("DEPLOYMENT_USAGE_WEBHOOK_URL", "".to_string()));

/// Delay before re-querying Databricks to backfill the most recent window that
/// Databricks had not yet ingested at initial seed time. Databricks ingestion
/// lags, so each deployment is seeded once at load and once
/// again after this delay.
pub static APP_METRICS_SEED_GAP_FILL_DELAY: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("APP_METRICS_SEED_GAP_FILL_DELAY_SECS", 90 * 60))
});

/// Maximum number of deployments included in a single Databricks app-metrics
/// seed query batch (a comma-separated `instance_names` parameter). Extra due
/// deployments are processed on subsequent sweeps. Kept small to keep each
/// query's result well under the Databricks INLINE-disposition 25 MiB response
/// cap, which fails (rather than truncates) the whole statement if exceeded.
/// Estimate: 100 bytes per row * 13 metrics * 150 data points = ~.2 MiB per
/// deployment
pub static APP_METRICS_SEED_MAX_BATCH: LazyLock<usize> =
    LazyLock::new(|| env_config("APP_METRICS_SEED_MAX_BATCH", 100));

/// Maximum random delay the app-metrics seed worker waits before its first
/// sweep. A uniformly random delay in `[0, this]` is chosen once at startup.
/// This spreads the initial Databricks query across conductors so a
/// fleet-wide restart (e.g. a bad deploy crash-looping many conductors) does
/// not align every conductor's re-seed into a thundering herd on Databricks.
pub static APP_METRICS_SEED_STARTUP_JITTER: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("APP_METRICS_SEED_STARTUP_JITTER_SECS", 15 * 60))
});

/// Kill switch for conductor's app-metrics seed worker. When set, the worker
/// does not run and deployments are not tracked for seeding.
pub static KILL_APP_METRICS_SEED_WORKER: LazyLock<bool> =
    LazyLock::new(|| env_config("KILL_APP_METRICS_SEED_WORKER", false));

/// Conductor-scoped kill switch for AI gateway token minting. Function
/// execution remains available, but calls to `getServiceToken("ai-gateway")`
/// fail before a token is signed.
pub static DISABLE_AI_GATEWAY: LazyLock<bool> =
    LazyLock::new(|| env_config("DISABLE_AI_GATEWAY", false));

/// Minimum spacing between a deployment's deploy reports to big brain. Deploys
/// landing within this window of the previous report coalesce into a single
/// request carrying the newest deploy time.
pub static DEPLOY_REPORT_MIN_INTERVAL: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("DEPLOY_REPORT_MIN_INTERVAL_SECS", 2)));

/// Databricks query id (UUID) for the conductor app-metrics seed query, which
/// takes a comma-separated `instance_names` parameter and returns rolled up
/// usage data. Defaults to the the empty string, which means the worker will be
/// disabled.
pub static APP_METRICS_SEED_QUERY_ID: LazyLock<String> = LazyLock::new(|| {
    env_config(
        "APP_METRICS_SEED_QUERY_ID",
        "093c29d3-1001-4bbc-9315-eff7e9f9f0c2".to_string(),
    )
});

/// How often the heap worker reports metrics.
pub static HEAP_WORKER_REPORT_INTERVAL_SECONDS: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("HEAP_WORKER_REPORT_INTERVAL_SECONDS", 30)));

/// This is our official action timeout. This is how much the user code
/// should be allowed to run. Note that we buffer some overhead and the actual
/// Node.js process timeout is higher. We also have separate timeout for V8
/// syscalls.
///
/// NOTE: If you update this, make sure to update the actions resource limits in
/// the docs.
pub static V8_ACTION_USER_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("V8_ACTION_USER_TIMEOUT_SECS", 1800)));

/// This is the action timeout for Node.js actions. This is how much the user
/// code should be allowed to run. Note that we buffer some overhead and the
/// actual Node.js process timeout is higher.
///
/// NOTE: If you update this, make sure to update the actions resource limits in
/// the docs.
pub static NODE_ACTION_USER_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("NODE_ACTION_USER_TIMEOUT_SECS", 600)));

/// Upper bound for user action execution across the V8 and Node runtimes.
pub static MAX_ACTION_USER_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| max(*V8_ACTION_USER_TIMEOUT, *NODE_ACTION_USER_TIMEOUT));

/// Minimum remaining lifetime required when a cached service token is
/// returned. This must remain below the shortest JWT lifetime issued by a
/// supported control plane.
pub static GET_SERVICE_TOKEN_GUARANTEED_LIFETIME: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config(
        "GET_SERVICE_TOKEN_GUARANTEED_LIFETIME_SECONDS",
        2 * 60,
    ))
});

/// Maximum number of deployment-and-attribution-specific service tokens held
/// by one local backend.
pub static SERVICE_TOKEN_CACHE_CAPACITY: LazyLock<u64> =
    LazyLock::new(|| env_config("SERVICE_TOKEN_CACHE_CAPACITY", 1024));

/// Ideally, we should have no timeout here but we are relying on defense in
/// depth in case somehow the upstream get stuck. Use very high timeout here.
///
/// This should be at least V8_ACTION_USER_TIMEOUT
pub static FUNRUN_RUN_FUNCTION_TIMEOUT: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(
        env_config("FUNRUN_RUN_FUNCTION_TIMEOUT_SECS", 35 * 60)
            .max(V8_ACTION_USER_TIMEOUT.as_secs()),
    )
});

/// Max number of rows we will read when calculating document deltas.
pub static DOCUMENT_DELTAS_LIMIT: LazyLock<usize> =
    LazyLock::new(|| env_config("DOCUMENT_DELTAS_LIMIT", 128));

/// Max number of rows we will read when calculating snapshot pages.
/// Each document can be up to `crate::document::MAX_USER_SIZE`
/// Note that this is a pro feature, so we can afford more memory.
pub static SNAPSHOT_LIST_LIMIT: LazyLock<usize> =
    LazyLock::new(|| env_config("SNAPSHOT_LIST_LIMIT", 1024));

/// Max duration we will spend calculating a single page.
pub static SNAPSHOT_LIST_TIME_LIMIT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("SNAPSHOT_LIST_TIME_LIMIT_SECONDS", 60)));

/// Target number of entries `DataSyncIterator` will emit in a single page. A
/// page *may* emit more than this: all of a transaction's writes are emitted on
/// the same page, since a page boundary never splits a transaction, so a single
/// transaction larger than this limit overflows the page.
/// This should be an exact multiple of `MYSQL_MAX_QUERY_BATCH_SIZE` to avoid
/// fetching a partial batch's worth of extra documents at each page boundary.
pub static DATA_SYNC_PAGE_SIZE_LIMIT: LazyLock<usize> =
    LazyLock::new(|| env_config("DATA_SYNC_PAGE_SIZE_LIMIT", 20000));

/// Soft limit on the total byte size of documents emitted in a single
/// `DataSyncIterator` page, bounding page/response size on deployments with
/// large documents. Like `DATA_SYNC_PAGE_SIZE_LIMIT`, a transaction is never
/// split across pages, so a single transaction larger than this overflows it.
pub static DATA_SYNC_PAGE_BYTES_LIMIT: LazyLock<usize> =
    LazyLock::new(|| env_config("DATA_SYNC_PAGE_BYTES_LIMIT", 1 << 26)); // 64 MiB

/// Max number of rows `DataSyncIterator` will read from persistence when
/// computing a single page. A page can read many more rows than it emits when
/// catching up the document log with a narrow table filter (lots of skipping).
///
/// Note that `DataSyncIterator` never splits up transactions, so large
/// transactions will still exceed this value.
///
/// Must be `>= DATA_SYNC_PAGE_SIZE_LIMIT`: the `by_id` dimension reads up to
/// `page_size_limit` rows per page, so the row-read bound can't be smaller than
/// the page-size bound. We clamp up to enforce this regardless of the env var.
pub static DATA_SYNC_MAX_ROWS_READ: LazyLock<usize> =
    LazyLock::new(|| env_config("DATA_SYNC_MAX_ROWS_READ", 32768).max(*DATA_SYNC_PAGE_SIZE_LIMIT));

/// Minimum interval between writes to a sync's `_data_sync_progress` row.
/// Pages that complete within this window of the row's last update are skipped
/// (no transaction is committed), throttling progress writes so a fast sync
/// doesn't write the row on every page. A settled, fully caught-up snapshot
/// (and any change in the sync's state variant) is persisted regardless of
/// this interval so a sync's final progress is never lost to throttling.
pub static DATA_SYNC_PROGRESS_WRITE_INTERVAL: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_millis(env_config("DATA_SYNC_PROGRESS_WRITE_INTERVAL_MS", 5000))
});

/// How close `synced_ts` must be to the latest repeatable timestamp before
/// `DataSyncIterator` iterates the `by_id` dimension. When `synced_ts` falls
/// further behind than this, the iterator catches up along the `ts` dimension
/// instead, keeping `synced_ts` within index retention.
pub static DATA_SYNC_BY_ID_FRESHNESS: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("DATA_SYNC_BY_ID_FRESHNESS_SECONDS", 30)));

/// The size of the log manager's event receive buffer.
pub static LOG_MANAGER_EVENT_RECV_BUFFER_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("LOG_MANAGER_EVENT_RECV_BUFFER_SIZE", 4096));

/// The aggregation interval at which the log manager empties its event receiver
/// buffer.
pub static LOG_MANAGER_AGGREGATION_INTERVAL_MILLIS: LazyLock<u64> =
    LazyLock::new(|| env_config("LOG_MANAGER_AGGREGATION_INTERVAL", 5000));

/// Max number of times a mutation can retry due to OCC conflicts.
pub static UDF_EXECUTOR_OCC_MAX_RETRIES: LazyLock<usize> =
    LazyLock::new(|| env_config("UDF_EXECUTOR_OCC_MAX_RETRIES", 4));

/// Initial backoff when we encounter an OCC conflict.
pub static UDF_EXECUTOR_OCC_INITIAL_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("UDF_EXECUTOR_OCC_INITIAL_BACKOFF_MS", 100)));

/// Maximum expontial backoff when facing repeated OCC conflicts.
pub static UDF_EXECUTOR_OCC_MAX_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("UDF_EXECUTOR_OCC_MAX_BACKOFF_MS", 2000)));

/// Max number of times `finish_push` retries due to OCC conflicts. It reads the
/// `_index` documents that `start_push` just created, contending with the
/// index/search backfill workers, so it needs more room than the default
/// `MAX_OCC_FAILURES` to ride out that churn.
pub static FINISH_PUSH_MAX_OCC_FAILURES: LazyLock<u32> =
    LazyLock::new(|| env_config("FINISH_PUSH_MAX_OCC_FAILURES", 8));

/// Initial backoff when the scheduler encounters an OCC conflict
pub static SCHEDULER_OCC_INITIAL_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("SCHEDULER_OCC_INITIAL_BACKOFF_MS", 100)));

/// Maximum backoff when the scheduler faces repeated OCC conflicts
pub static SCHEDULER_OCC_MAX_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("SCHEDULER_OCC_MAX_BACKOFF_MS", 60 * 1000)));

/// The time for which a backend will stay around, after getting preempted,
/// answering health checks but not serving traffic.
///
/// This MUST be set to >= the `healthy_deadline` in backend.nomad. Otherwise
/// a pre-emption loop can be triggered. The old backend process dies due to
/// this timeout, and then is restarted by nomad because the new deployment
/// hasn't become healthy yet. The restarted old process then pre-empts the new
/// process, causing it to die and be restarted etc.
pub static LEASE_LOST_COOL_DOWN: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config(
        "LEASE_LOST_COOL_DOWN_SECS",
        prod_override(5, 130),
    ))
});

/// How long the queue must be nonempty before we consider traffic to be
/// "congested" and start shedding traffic. When we are idle (not congested) it
/// is how long each request can live in the queue.
pub static CODEL_QUEUE_IDLE_EXPIRATION_MILLIS: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("CODEL_QUEUE_IDLE_EXPIRATION_MILLIS", 5000)));
/// How long each request can live in the queue when we are congested.
pub static CODEL_QUEUE_CONGESTED_EXPIRATION_MILLIS: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_millis(env_config("CODEL_QUEUE_CONGESTED_EXPIRATION_MILLIS", 50))
});

/// Default page size (in number of docuemnts) used when loading documents from
/// the database.
pub static DEFAULT_DOCUMENTS_PAGE_SIZE: LazyLock<u32> =
    LazyLock::new(|| env_config("DEFAULT_DOCUMENTS_PAGE_SIZE", 100));

/// Maximum number of documents it's okay to load into memory at once.
/// Note each document can be up to `::value::MAX_SIZE`.
pub static DOCUMENTS_IN_MEMORY: LazyLock<usize> =
    LazyLock::new(|| env_config("DOCUMENTS_IN_MEMORY", 512));

/// Number of times to retry on retriable errors (out-of-retention, database
/// timeout) in TableIterator
pub static TABLE_ITERATOR_MAX_RETRIES: LazyLock<u32> =
    LazyLock::new(|| env_config("TABLE_ITERATOR_MAX_RETRIES", 1));

/// Documents per `by_id` page when the SchemaWorker walks a table.
pub static SCHEMA_WORKER_PAGE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("SCHEMA_WORKER_PAGE_SIZE", 1000));

/// Number of schema-validation table walks allowed to run concurrently.
pub static SCHEMA_WORKER_WALK_CONCURRENCY: LazyLock<usize> =
    LazyLock::new(|| env_config("SCHEMA_WORKER_WALK_CONCURRENCY", 1));

/// Length of the HTTP server TCP backlog.
pub static HTTP_SERVER_TCP_BACKLOG: LazyLock<u32> =
    LazyLock::new(|| env_config("HTTP_SERVER_TCP_BACKLOG", 256));

/// The max concurrent of concurrent HTTP requests. This also limits Node.js
/// action callbacks concurrency since those go over http.
pub static HTTP_SERVER_MAX_CONCURRENT_REQUESTS: LazyLock<usize> =
    LazyLock::new(|| env_config("HTTP_SERVER_MAX_CONCURRENT_REQUESTS", 1024));

/// Max number of user writes in a transaction. Make sure to also increase
/// `MAX_INSERT_SIZE` in mysql/src/lib.rs and postgres/src/lib.rs.
pub static TRANSACTION_MAX_NUM_USER_WRITES: LazyLock<usize> =
    LazyLock::new(|| env_config("TRANSACTION_MAX_NUM_USER_WRITES", 16000));

/// Max size of user writes in a transaction, in bytes
pub static TRANSACTION_MAX_USER_WRITE_SIZE_BYTES: LazyLock<usize> = LazyLock::new(|| {
    env_config("TRANSACTION_MAX_USER_WRITE_SIZE_BYTES", 1 << 24) // 16 MiB
});

/// Soft cap on document rows the committer combines into one batched
/// persistence write.
pub static COMMITTER_MAX_WRITE_BATCH_DOCUMENTS: LazyLock<usize> =
    LazyLock::new(|| env_config("COMMITTER_MAX_WRITE_BATCH_DOCUMENTS", 64));

/// Soft cap on the serialized bytes of one batched persistence write.
pub static COMMITTER_MAX_WRITE_BATCH_BYTES: LazyLock<u64> = LazyLock::new(|| {
    env_config("COMMITTER_MAX_WRITE_BATCH_BYTES", 1 << 16) // 64 KiB
});

/// How long the committer holds a partially-filled write batch open for commits
/// that are still arriving, once batching engages.
pub static COMMITTER_MAX_COMMIT_DELAY: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("COMMITTER_MAX_COMMIT_DELAY_MS", 1)));

/// How many batched persistence writes the committer keeps in flight at once.
pub static COMMITTER_MAX_CONCURRENT_WRITE_BATCHES: LazyLock<usize> =
    LazyLock::new(|| env_config::<usize>("COMMITTER_MAX_CONCURRENT_WRITE_BATCHES", 16).max(1));

/// How many persistence writes must be in flight, counting the one about to
/// start, before the committer combines commits into batches.
pub static COMMITTER_BATCH_WRITE_THRESHOLD: LazyLock<usize> =
    LazyLock::new(|| env_config("COMMITTER_BATCH_WRITE_THRESHOLD", 3));

/// How many pre-validation batches the committer keeps in flight at once.
pub static COMMITTER_MAX_CONCURRENT_PRE_VALIDATIONS: LazyLock<usize> =
    LazyLock::new(|| env_config::<usize>("COMMITTER_MAX_CONCURRENT_PRE_VALIDATIONS", 32).max(1));

/// Most commits conflict-checked together in one pre-validation batch.
pub static COMMITTER_MAX_PRE_VALIDATE_BATCH_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config::<usize>("COMMITTER_MAX_PRE_VALIDATE_BATCH_SIZE", 1).max(1));

/// SnapshotManager maintains a bounded time range of versions,
/// determined by `MAX_TRANSACTION_WINDOW`, allowing the `Database` layer to
/// begin a transaction in any timestamp within that range.
pub static MAX_TRANSACTION_WINDOW: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("MAX_TRANSACTION_WINDOW_SECONDS", 10)));

/// Maximum size in bytes of arguments to a function.
pub static FUNCTION_MAX_ARGS_SIZE: LazyLock<usize> = LazyLock::new(|| {
    env_config("FUNCTION_MAX_ARGS_SIZE", 1 << 24) // 16 MiB
});

/// Maximum size in bytes of the result of a function.
/// Has to be smaller than MAX_BACKEND_RPC_RESPONSE_SIZE
pub static FUNCTION_MAX_RESULT_SIZE: LazyLock<usize> = LazyLock::new(|| {
    env_config("FUNCTION_MAX_RESULT_SIZE", 1 << 24) // 16 MiB
});

/// Maximum size in bytes of a task request (`task_submit`) a `"use rust"` guest
/// may hand to the host. Larger requests are rejected (the import returns -1)
/// before any guest memory is read.
pub static RUST_MAX_TASK_REQUEST_BYTES: LazyLock<usize> =
    LazyLock::new(|| env_config("RUST_MAX_TASK_REQUEST_BYTES", 64 << 20)); // 64 MiB

/// Maximum number of unfinished tasks (`ctx.runQuery`, `fetch`, ...) one
/// `"use rust"` action may have at a time. Further `task_submit` calls return
/// -1 until some task completes.
pub static RUST_MAX_OUTSTANDING_TASKS: LazyLock<usize> =
    LazyLock::new(|| env_config("RUST_MAX_OUTSTANDING_TASKS", 1000));

/// Maximum number of `"use rust"` actions executing at the same time in this
/// process. Mirrors MAX_ISOLATE_WORKERS, the bound on concurrent V8 actions.
pub static RUST_MAX_CONCURRENT_ACTIONS: LazyLock<usize> = LazyLock::new(|| {
    env_config(
        "RUST_MAX_CONCURRENT_ACTIONS",
        *MAX_ISOLATE_WORKERS,
    )
});

/// Persist compiled `"use rust"` modules (`Module::serialize`) so a backend
/// restart does not recompile them.
pub static RUST_WASM_DISK_CACHE: LazyLock<bool> =
    LazyLock::new(|| env_config("RUST_WASM_DISK_CACHE", true));

/// Directory of the on-disk compiled-module cache.
pub static RUST_WASM_CACHE_DIR: LazyLock<String> = LazyLock::new(|| {
    env_config(
        "RUST_WASM_CACHE_DIR",
        "convex_local_storage/rust-wasm-cache".to_string(),
    )
});

// ---- rust-native mode (contracts/NATIVE.md §5.6) ----

/// `off` or `worker`: whether `"use rust"` modules pushed as native cdylibs
/// run in worker processes.
pub static RUST_NATIVE_MODE: LazyLock<String> =
    LazyLock::new(|| env_config("RUST_NATIVE_MODE", "off".to_string()));
/// `strict`, `best-effort` or `off`; empty means the platform default.
pub static RUST_NATIVE_SANDBOX: LazyLock<String> =
    LazyLock::new(|| env_config("RUST_NATIVE_SANDBOX", String::new()));
/// Required to run native workers with no sandbox (`RUST_NATIVE_SANDBOX=off`,
/// or the default on a platform without one, i.e. anywhere but Linux): user
/// code then has the backend's full filesystem, network and process access.
pub static RUST_NATIVE_ALLOW_UNSANDBOXED: LazyLock<bool> =
    LazyLock::new(|| env_config("RUST_NATIVE_ALLOW_UNSANDBOXED", false));
/// Allows the socket syscalls inside workers.
pub static RUST_NATIVE_ALLOW_NETWORK: LazyLock<bool> =
    LazyLock::new(|| env_config("RUST_NATIVE_ALLOW_NETWORK", false));
/// Where native artifacts and worker tmp dirs live.
pub static RUST_NATIVE_DIR: LazyLock<String> = LazyLock::new(|| {
    env_config(
        "RUST_NATIVE_DIR",
        "convex_local_storage/rust-native".to_string(),
    )
});
/// Memory limit per worker.
pub static RUST_NATIVE_MEMORY_BYTES: LazyLock<u64> =
    LazyLock::new(|| env_config("RUST_NATIVE_MEMORY_BYTES", 512 << 20));
/// Watchdog slack above the memory limit.
pub static RUST_NATIVE_RSS_SLACK_BYTES: LazyLock<u64> =
    LazyLock::new(|| env_config("RUST_NATIVE_RSS_SLACK_BYTES", 64 << 20));
/// Stack size of the worker's call thread.
pub static RUST_NATIVE_STACK_BYTES: LazyLock<u64> =
    LazyLock::new(|| env_config("RUST_NATIVE_STACK_BYTES", 8 << 20));
/// Idle workers kept per pool.
pub static RUST_NATIVE_MIN_IDLE: LazyLock<usize> =
    LazyLock::new(|| env_config("RUST_NATIVE_MIN_IDLE", 1));
/// Idle workers over all pools. Defaults to the CPU count (at least 16): a
/// burst of N concurrent calls leaves N workers behind, and a cap below the
/// usual concurrency makes every burst respawn workers (~10 MB RSS each while
/// idle; the idle reaper returns them after `RUST_NATIVE_IDLE_TIMEOUT_SECS`).
/// The runtime also clamps it to `RUST_MAX_CONCURRENT_ACTIONS`.
pub static RUST_NATIVE_MAX_IDLE: LazyLock<usize> = LazyLock::new(|| {
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    env_config("RUST_NATIVE_MAX_IDLE", cpus.max(16))
});
/// Upper bound on threads that spawn worker processes in parallel. Spawn
/// threads are started on demand and idle ones stay parked. Defaults to
/// min(CPU count, 8).
pub static RUST_NATIVE_SPAWN_THREADS: LazyLock<usize> = LazyLock::new(|| {
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    env_config("RUST_NATIVE_SPAWN_THREADS", cpus.clamp(1, 8))
});
/// Start a worker in the background when a native artifact is first resolved,
/// so the dlopen of a freshly written library (hundreds of ms on macOS) does
/// not land on the first call. Never blocks the caller.
pub static RUST_NATIVE_PREWARM: LazyLock<bool> =
    LazyLock::new(|| env_config("RUST_NATIVE_PREWARM", true));
/// The RSS probe runs on every Nth call of a worker (the first call always,
/// and any call slower than `RUST_NATIVE_PROBE_SLOW_CALL_MS`). 1 probes every
/// call. RSS above `RUST_NATIVE_RECYCLE_RSS_BYTES` is then noticed up to N - 1
/// calls late; the worker memory limit still applies. The runaway-thread
/// check is not sampled: it runs at every release.
pub static RUST_NATIVE_PROBE_EVERY: LazyLock<u32> =
    LazyLock::new(|| env_config("RUST_NATIVE_PROBE_EVERY", 8));
/// Calls at least this long (ms) are always probed.
pub static RUST_NATIVE_PROBE_SLOW_CALL_MS: LazyLock<u64> =
    LazyLock::new(|| env_config("RUST_NATIVE_PROBE_SLOW_CALL_MS", 20));
/// Idle workers older than this are reaped.
pub static RUST_NATIVE_IDLE_TIMEOUT_SECS: LazyLock<u64> =
    LazyLock::new(|| env_config("RUST_NATIVE_IDLE_TIMEOUT_SECS", 300));
/// A worker is recycled after this many calls (1 for strict parity with wasm).
pub static RUST_NATIVE_MAX_CALLS_PER_WORKER: LazyLock<u32> =
    LazyLock::new(|| env_config("RUST_NATIVE_MAX_CALLS_PER_WORKER", 1000));
/// A worker whose RSS is above this at release is recycled.
pub static RUST_NATIVE_RECYCLE_RSS_BYTES: LazyLock<u64> =
    LazyLock::new(|| env_config("RUST_NATIVE_RECYCLE_RSS_BYTES", 256 << 20));
/// The reaper recycles an idle native worker whose RSS is above this many
/// bytes, but only while the same function set has another idle worker
/// (0 disables). Backstop for freed memory the allocator does not return.
pub static RUST_NATIVE_IDLE_RECYCLE_RSS_BYTES: LazyLock<u64> =
    LazyLock::new(|| env_config("RUST_NATIVE_IDLE_RECYCLE_RSS_BYTES", 24 << 20));
/// How long a spawn waits for the worker's `Ready`.
pub static RUST_NATIVE_SPAWN_TIMEOUT_MS: LazyLock<u64> =
    LazyLock::new(|| env_config("RUST_NATIVE_SPAWN_TIMEOUT_MS", 5000));
/// Push-time size limit of a native artifact.
pub static RUST_NATIVE_MAX_ARTIFACT_BYTES: LazyLock<u64> =
    LazyLock::new(|| env_config("RUST_NATIVE_MAX_ARTIFACT_BYTES", 180 << 20));
/// Privilege drop for workers; both must be set. Empty means no drop.
pub static RUST_NATIVE_UID: LazyLock<String> =
    LazyLock::new(|| env_config("RUST_NATIVE_UID", String::new()));
/// See `RUST_NATIVE_UID`.
pub static RUST_NATIVE_GID: LazyLock<String> =
    LazyLock::new(|| env_config("RUST_NATIVE_GID", String::new()));
/// Delegated cgroup subtree for worker memory limits. Empty means none.
pub static RUST_NATIVE_CGROUP: LazyLock<String> =
    LazyLock::new(|| env_config("RUST_NATIVE_CGROUP", String::new()));
/// Comma separated extension of the shared-library allowlist (library names
/// at push time; absolute entries are also readable by the sandbox).
pub static RUST_NATIVE_EXTRA_LIBS: LazyLock<String> =
    LazyLock::new(|| env_config("RUST_NATIVE_EXTRA_LIBS", String::new()));
/// Use an embedded wasm when native is unavailable.
pub static RUST_NATIVE_FALLBACK_WASM: LazyLock<bool> =
    LazyLock::new(|| env_config("RUST_NATIVE_FALLBACK_WASM", true));

/// Maximum size in bytes of the result of a system function.
/// Larger than FUNCTION_MAX_RESULT_SIZE but smaller than
/// MAX_BACKEND_RPC_RESPONSE_SIZE
pub static SYSTEM_FUNCTION_MAX_RESULT_SIZE: LazyLock<usize> = LazyLock::new(|| {
    env_config("SYSTEM_FUNCTION_MAX_RESULT_SIZE", 24 * (1 << 20)) // 24 MiB
});

/// When a function exceeds FUNCTION_LIMIT_WARNING_RATIO * a corresponding
/// limit value, we add a warning log line.
pub static FUNCTION_LIMIT_WARNING_RATIO: LazyLock<f64> = LazyLock::new(|| {
    env_config("FUNCTION_LIMIT_WARNING_RATIO", 0.8) // 80%
});

/// Maximum size in bytes of a single audit log line's serialized JSON body.
pub static AUDIT_LOG_MAX_LINE_SIZE_BYTES: LazyLock<usize> = LazyLock::new(|| {
    env_config("AUDIT_LOG_MAX_LINE_SIZE_BYTES", 100_000) // 100 KB
});

/// Maximum total size in bytes of all audit log lines emitted in a single
/// function execution, including audit logs from nested function calls.
pub static AUDIT_LOG_MAX_TOTAL_SIZE_BYTES: LazyLock<usize> = LazyLock::new(|| {
    env_config("AUDIT_LOG_MAX_TOTAL_SIZE_BYTES", 4_000_000) // 4 MB
});

/// Maximum number of audit log lines that can be emitted in a single function
/// execution, including audit logs from nested function calls.
pub static AUDIT_LOG_MAX_LINES: LazyLock<usize> =
    LazyLock::new(|| env_config("AUDIT_LOG_MAX_LINES", 500));

/// Maximum heap size in bytes that audit log lines accumulated during a single
/// function execution may occupy.
pub static AUDIT_LOG_MAX_HEAP_SIZE_BYTES: LazyLock<usize> = LazyLock::new(|| {
    env_config("AUDIT_LOG_MAX_HEAP_SIZE_BYTES", 4_000_000) // 4 MB
});

/// We might generate a number of system documents for each UDF write. For
/// example, creating 4000 user documents in new tables, might result in adding
/// an additional 8000 system documents. If we hit this error, this is a system
/// error, not a developer one. If you increase this value, make sure to also
/// increase MAX_INSERT_SIZE in mysql/src/lib.rs and postgres/src/lib.rs.
pub static TRANSACTION_MAX_SYSTEM_NUM_WRITES: LazyLock<usize> =
    LazyLock::new(|| env_config("TRANSACTION_MAX_SYSTEM_NUM_WRITES", 40000));

/// We write user modules in system tables and those can get quite large.
/// Similar to the above if we hit this limit, we should count this as system
/// error and do a use case specific validation to avoid hitting this.
pub static TRANSACTION_MAX_SYSTEM_WRITE_SIZE_BYTES: LazyLock<usize> = LazyLock::new(|| {
    env_config("TRANSACTION_MAX_SYSTEM_WRITE_SIZE_BYTES", 1 << 27) // 128 MiB
});

/// Maximum number of scheduled transactions.
pub static TRANSACTION_MAX_NUM_SCHEDULED: LazyLock<usize> =
    LazyLock::new(|| env_config("TRANSACTION_MAX_NUM_SCHEDULED", 1000));

/// Maximum number of scheduled jobs to cancel in a single transaction.
pub static MAX_JOBS_CANCEL_BATCH: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_JOBS_CANCEL_BATCH", 1000));

/// Maximum size of a single scheduled function's arguments.
/// This is not currently enforced.
/// TODO: ideally this should be MAX_USER_SIZE.
pub static MAX_SCHEDULED_JOB_ARGUMENT_SIZE_BYTES: LazyLock<usize> = LazyLock::new(|| {
    env_config("MAX_SCHEDULED_JOB_ARGUMENT_SIZE_BYTES", 4 << 20) // 4 MiB
});

/// Maximum total size of the arguments to all functions scheduled in a single
/// transaction.
pub static TRANSACTION_MAX_SCHEDULED_TOTAL_ARGUMENT_SIZE_BYTES: LazyLock<usize> =
    LazyLock::new(|| {
        env_config(
            "TRANSACTION_MAX_SCHEDULED_TOTAL_ARGUMENT_SIZE_BYTES",
            1 << 24,
        ) // 16 MiB
    });

/// Maximum number of files a single transaction may write to file storage.
pub static TRANSACTION_MAX_NUM_FILES_WRITTEN: LazyLock<usize> =
    LazyLock::new(|| env_config("TRANSACTION_MAX_NUM_FILES_WRITTEN", 10));

/// Maximum total size of the files written to file storage by a single
/// transaction.
pub static TRANSACTION_MAX_FILE_WRITE_SIZE_BYTES: LazyLock<usize> = LazyLock::new(|| {
    env_config("TRANSACTION_MAX_FILE_WRITE_SIZE_BYTES", 1 << 24) // 16 MiB
});

/// Maximum number of files a single transaction may read from file storage.
pub static TRANSACTION_MAX_NUM_FILES_READ: LazyLock<usize> =
    LazyLock::new(|| env_config("TRANSACTION_MAX_NUM_FILES_READ", 10));

/// Maximum total size of the files read from file storage by a single
/// transaction.
pub static TRANSACTION_MAX_FILE_READ_SIZE_BYTES: LazyLock<usize> = LazyLock::new(|| {
    env_config("TRANSACTION_MAX_FILE_READ_SIZE_BYTES", 1 << 24) // 16 MiB
});

/// Number of scheduled jobs that can execute in parallel.
// Note that the current algorithm for executing ready jobs has up to
// SCHEDULED_JOB_EXECUTION_PARALLELISM overhead for every executed job, so we
// don't want to set this number too high.
pub static SCHEDULED_JOB_EXECUTION_PARALLELISM: LazyLock<usize> =
    LazyLock::new(|| env_config("SCHEDULED_JOB_EXECUTION_PARALLELISM", 8));

/// Initial backoff in milliseconds on a system error from a scheduled job.
pub static SCHEDULED_JOB_INITIAL_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("SCHEDULED_JOB_INITIAL_BACKOFF_MS", 500)));

/// Max backoff in seconds on a system error from a scheduled job.
/// Scheduled jobs can hit many OCCs, so we may need to slow them down if they
/// hit errors repeatedly.
pub static SCHEDULED_JOB_MAX_BACKOFF: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("SCHEDULED_JOB_MAX_BACKOFF_SECS", 2 * 60 * 60))
});

/// Initial backoff in milliseconds on a system error from the scheduled job
/// garbage collector.
pub static SCHEDULED_JOB_GARBAGE_COLLECTION_INITIAL_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| {
        Duration::from_millis(env_config(
            "SCHEDULED_JOB_GARBAGE_COLLECTION_INITIAL_BACKOFF_MS",
            1000,
        ))
    });

/// Max backoff in seconds on a system error from the scheduled job garbage
/// collector.
pub static SCHEDULED_JOB_GARBAGE_COLLECTION_MAX_BACKOFF: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config(
        "SCHEDULED_JOB_GARBAGE_COLLECTION_MAX_BACKOFF_SECS",
        30,
    ))
});

/// How long completed scheduled jobs are kept before getting garbage collected.
pub static SCHEDULED_JOB_RETENTION: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config(
        "SCHEDULED_JOB_RETENTION",
        60 * 60 * 24 * 7, // 1 week
    ))
});

/// Maximum number of scheduled jobs to garbage collect in a single transaction
pub static SCHEDULED_JOB_GARBAGE_COLLECTION_BATCH_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("SCHEDULED_JOB_GARBAGE_COLLECTION_BATCH_SIZE", 100));

/// Delay between runs of the scheduled job garbage collector.
/// If too low, the garbage collector will run frequently with small batches,
/// which is less efficient. If too high, the garbage collector might fall
/// behind.
pub static SCHEDULED_JOB_GARBAGE_COLLECTION_DELAY: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("SCHEDULED_JOB_GARBAGE_COLLECTION_DELAY", 1)));

/// Exclusive upper bound, in seconds, for the stable random offset applied to
/// cron runs so jobs sharing a schedule don't all fire at once and spike load.
/// A job whose previous run is exactly schedule-aligned draws a fresh offset,
/// which is how jobs created before splaying pick one up. Keep this at or
/// below 60 so a run remains within its scheduled minute; set it to 0 to
/// disable splaying.
pub static CRON_SPLAY_SECONDS: LazyLock<u64> =
    LazyLock::new(|| env_config("CRON_SPLAY_SECONDS", 60));

/// Maximum number of syscalls that can run in a batch together when
/// awaited in parallel. Higher values improve latency, while lower ones
/// protect one isolate from hogging database connections.
pub static MAX_SYSCALL_BATCH_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_SYSCALL_BATCH_SIZE", 16));

/// Maximum depth of query/mutation -> query/mutation calls within the reactor.
/// We put a low limit on this for now so users with infinite loops won't starve
/// all of the threads on a single node.
pub static MAX_REACTOR_CALL_DEPTH: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_REACTOR_CALL_DEPTH", 8));

/// Default number of records to fetch from an index if a prefetch hint is not
/// provided.
pub static DEFAULT_QUERY_PREFETCH: LazyLock<usize> =
    LazyLock::new(|| env_config("DEFAULT_QUERY_PREFETCH", 100));

/// Number of rows that can be read in a transaction.
pub static TRANSACTION_MAX_READ_SIZE_ROWS: LazyLock<usize> =
    LazyLock::new(|| env_config("TRANSACTION_MAX_READ_SIZE_ROWS", 32000));

/// Number of bytes that can be read in a transaction.
pub static TRANSACTION_MAX_READ_SIZE_BYTES: LazyLock<usize> = LazyLock::new(|| {
    env_config("TRANSACTION_MAX_READ_SIZE_BYTES", 1 << 24) // 16 MiB
});

/// Maximum size in bytes of the in-memory index cache carried across
/// transactions. When a write pushes the cache past this size the entire cache
/// is dropped, forcing subsequent reads to go to the database.
pub static MAX_TRANSACTION_CACHE_SIZE_BYTES: LazyLock<usize> = LazyLock::new(|| {
    env_config("MAX_TRANSACTION_CACHE_SIZE_BYTES", 1 << 24) // 16 MiB
});

/// Maximum number of intervals that can be read in a transaction.
pub static TRANSACTION_MAX_READ_SET_INTERVALS: LazyLock<usize> =
    LazyLock::new(|| env_config("TRANSACTION_MAX_READ_SET_INTERVALS", 4096));

/// Intervals that can be read in a transaction before warning.
pub static TRANSACTION_WARN_READ_SET_INTERVALS: LazyLock<usize> =
    LazyLock::new(|| env_config("TRANSACTION_WARN_READ_SET_INTERVALS", 3072));

/// Write max_repeatable_ts if there have been no commits for this duration.
/// The actual period is jittered between 1x and 2x this value.
pub static MAX_REPEATABLE_TIMESTAMP_IDLE_FREQUENCY: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config(
        "MAX_REPEATABLE_TIMESTAMP_IDLE_FREQUENCY",
        60 * 60,
    ))
});

/// This is the max duration between a Commit and bumping max_repeatable_ts.
/// When reading from a follower persistence, we can only read commits at
/// timestamps <= max_repeatable_ts (because commits > max_repeatable_ts are
/// actively being written), so this is the delay between a commit and the
/// commit being visible from db-verifier and other follower reads.
pub static MAX_REPEATABLE_TIMESTAMP_COMMIT_DELAY: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("MAX_REPEATABLE_TIMESTAMP_COMMIT_DELAY", 5)));

/// The maximum delay between runs of retention, this is now only used for error
/// backoff and the initial delay when backend is started.
pub static MAX_RETENTION_DELAY_SECONDS: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("RETENTION_DELETE_FREQUENCY", 60)));

/// How many parallel threads to use for deleting index entries that have
/// expired.
pub static INDEX_RETENTION_DELETE_PARALLEL: LazyLock<usize> =
    LazyLock::new(|| env_config("INDEX_RETENTION_DELETE_PARALLEL", 4));

/// How many parallel threads to use for deleting document log entries that have
/// expired.
pub static DOCUMENT_RETENTION_DELETE_PARALLEL: LazyLock<usize> =
    LazyLock::new(|| env_config("DOCUMENT_RETENTION_DELETE_PARALLEL", 1));

/// INDEX_RETENTION_DELAY determines the size of the index retention window.
///
/// Larger window means we keep around old snapshots for longer, which can cause
/// performance problems in UDFs if there are many tombstones.
///
/// Smaller window means we break snapshot reads faster.
pub static INDEX_RETENTION_DELAY: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("INDEX_RETENTION_DELAY", 4 * 60)));

/// How often each conductor checks that the MySQL V6 `indexes_log_<bucket>`
/// tables its cluster needs exist, and that buckets past retention are gone.
pub static INDEXES_LOG_MAINTENANCE_INTERVAL: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("INDEXES_LOG_MAINTENANCE_INTERVAL", 60).max(1))
});

/// How many `indexes_log_<bucket>` tables to create beyond the one taking
/// writes now.
pub static INDEXES_LOG_LOOKAHEAD_BUCKETS: LazyLock<usize> =
    LazyLock::new(|| env_config("INDEXES_LOG_LOOKAHEAD_BUCKETS", 3));

/// DOCUMENT_RETENTION_DELAY determines the size of the document retention
/// window.
///
/// Larger window means we keep around longer windows for our write ahead log
/// to be valid.
///
/// Smaller window means we keep less historical data around.
pub static DOCUMENT_RETENTION_DELAY: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("DOCUMENT_RETENTION_DELAY", 60 * 60 * 24 * 14))
});

/// When to start rejecting new additions to the search memory index.
pub static TEXT_INDEX_SIZE_HARD_LIMIT: LazyLock<usize> =
    LazyLock::new(|| env_config("SEARCH_INDEX_SIZE_HARD_LIMIT", 100 * (1 << 20))); // 100 MiB

/// When to start rejecting new additions to the vector memory index.
/// Because they're closely related, this is also used by the vector compaction
/// worker to determine the largest size for a "small" segment. Small segments
/// are merged more aggressively by the compaction worker than large segments.
pub static VECTOR_INDEX_SIZE_HARD_LIMIT: LazyLock<usize> =
    LazyLock::new(|| env_config("VECTOR_INDEX_SIZE_HARD_LIMIT", 100 * (1 << 20))); // 100 MiB

/// Whether indexes will be backfilled. Likely only disabled if index backfill
/// is breaking an instance.
pub static ENABLE_INDEX_BACKFILL: LazyLock<bool> =
    LazyLock::new(|| env_config("INDEX_BACKFILL_ENABLE", true));

/// Maximum number of index chunks processed per second during a backfill.
pub static INDEX_BACKFILL_CHUNK_RATE: LazyLock<NonZeroU32> =
    LazyLock::new(|| env_config("INDEX_BACKFILL_CHUNK_RATE", NonZeroU32::new(16).unwrap()));

/// The page size to use when reading the table for an index backfill.
pub static INDEX_BACKFILL_READ_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("INDEX_BACKFILL_READ_SIZE", 500));

/// How many index entries to write within a single database transaction.
/// Value is a tradeoff between grouping work, vs tying up resources on the
/// database, vs holding all entries in memory.
pub static INDEX_BACKFILL_CHUNK_SIZE: LazyLock<NonZeroU32> =
    LazyLock::new(|| env_config("INDEX_BACKFILL_CHUNK_SIZE", NonZeroU32::new(1024).unwrap()));

/// Number of workers to use for index backfill.
pub static INDEX_BACKFILL_WORKERS: LazyLock<usize> =
    LazyLock::new(|| env_config("INDEX_BACKFILL_WORKERS", 4));

/// How often to persist index backfill progress updates.
pub static INDEX_BACKFILL_PROGRESS_INTERVAL: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("INDEX_BACKFILL_PROGRESS_INTERVAL_SECONDS", 1))
});

/// Time between index backfill marker cleanup passes while cleanup is making
/// progress.
pub static INDEX_BACKFILL_MARKER_CLEANUP_BUSY_INTERVAL_SECONDS: LazyLock<Duration> =
    LazyLock::new(|| {
        Duration::from_secs_f64(env_config(
            "INDEX_BACKFILL_MARKER_CLEANUP_BUSY_INTERVAL_SECONDS",
            0.01,
        ))
    });

/// Time between index backfill marker cleanup passes when no markers were
/// deleted.
pub static INDEX_BACKFILL_MARKER_CLEANUP_IDLE_INTERVAL_SECONDS: LazyLock<Duration> =
    LazyLock::new(|| {
        Duration::from_secs_f64(env_config(
            "INDEX_BACKFILL_MARKER_CLEANUP_IDLE_INTERVAL_SECONDS",
            60.0,
        ))
    });

/// Chunk size of index entries for deleting from Persistence.
pub static INDEX_RETENTION_DELETE_CHUNK: LazyLock<usize> =
    LazyLock::new(|| env_config("INDEX_RETENTION_DELETE_CHUNK", 512));

/// Chunk size of documents for deleting from Persistence.
pub static DOCUMENT_RETENTION_DELETE_CHUNK: LazyLock<NonZeroU32> = LazyLock::new(|| {
    env_config(
        "DOCUMENT_RETENTION_DELETE_CHUNK",
        NonZeroU32::new(256).unwrap(),
    )
});

/// Batch size of index entries to delete between checkpoints.
pub static RETENTION_DELETE_BATCH: LazyLock<usize> =
    LazyLock::new(|| env_config("RETENTION_DELETE_BATCH", 10000));

/// Time in between batches of deletes for document retention. This value is
/// also used to jitter document retention on startup to avoid a thundering
/// herd.
pub static DOCUMENT_RETENTION_BATCH_INTERVAL_SECONDS: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs_f64(env_config(
        "DOCUMENT_RETENTION_BATCH_INTERVAL_SECONDS",
        60.0,
    ))
});

/// Documents-per-second rate limit for document retention
/// Note that while this serves as an upper bound, retention speed is mostly
/// limited by `DOCUMENT_RETENTION_BATCH_INTERVAL_SECONDS`,
/// `DOCUMENT_RETENTION_DELETE_CHUNK`, and `DOCUMENT_RETENTION_DELETE_PARALLEL`
/// TODO(ENG-10039): Increase this after retention has caught up so we don't get
/// too behind on partitions that have high write rates.
pub static DOCUMENT_RETENTION_RATE_LIMIT: LazyLock<NonZeroU32> = LazyLock::new(|| {
    env_config(
        "DOCUMENT_RETENTION_RATE_LIMIT",
        NonZeroU32::new(256).unwrap(),
    )
});

/// How frequently document and index retention workers should write
/// checkpoints to the persistence globals table in seconds
pub static RETENTION_CHECKPOINT_PERIOD_SECS: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("RETENTION_CHECKPOINT_PERIOD_SECS", 60 * 5)));

/// Maximum scanned documents within a single run for document retention unless
/// there are a bunch of writes at single timestamp. Then, we go until there are
/// no more writes at that timestamp.
pub static DOCUMENT_RETENTION_MAX_SCANNED_DOCUMENTS: LazyLock<usize> =
    LazyLock::new(|| env_config("DOCUMENT_RETENTION_MAX_SCANNED_DOCUMENTS", 10000));

/// Chunk size for SQL queries deleting documents from Deleting tables.
pub static DELETE_TABLET_CHUNK_SIZE: LazyLock<u16> =
    LazyLock::new(|| env_config("DELETE_TABLET_CHUNK_SIZE", 256));

/// Size at which a search index will be queued for snapshotting.
pub static SEARCH_INDEX_SIZE_SOFT_LIMIT: LazyLock<usize> =
    LazyLock::new(|| env_config("SEARCH_INDEX_SIZE_SOFT_LIMIT", 10 * (1 << 20))); // 10 MiB

/// Don't allow search index workers to have more than an hour of uncheckpointed
/// data.
///
/// For search/vector index workers - Note that fast-forwarding will keep the
/// index's timestamp up-to-date if its table hasn't had any writes. This isn't
/// perfect since ideally we'd bound the number and total size of log entries
/// read for bootstrapping, but it's good enough until we have better commit
/// statistics that aren't reset at restart. It's still expensive to walk the
/// DocumentRevisionStream to build new segments, so this value needs to be low
/// enough to not block the search index flushers for too long, or else writes
/// will start failing. This is why we set this value lower for pro users (10m).
pub static SEARCH_WORKERS_MAX_CHECKPOINT_AGE: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("SEARCH_WORKERS_MAX_CHECKPOINT_AGE", 3600)));

/// Don't fast-forward an index less than ten seconds forward so we don't
/// amplify every commit into another write when the system is under heavy load.
pub static DATABASE_WORKERS_POLL_INTERVAL: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("DATABASE_WORKERS_POLL_INTERVAL", 20)));

/// When the persisted table summary is within this threshold of the current
/// timestamp, we'll tell the committer to process any remaining writes and
/// finish the bootstrap.
pub static TABLE_SUMMARY_BOOTSTRAP_RECENT_THRESHOLD: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config(
        "TABLE_SUMMARY_BOOTSTRAP_RECENT_THRESHOLD_SECS",
        10,
    ))
});

/// The minimum time to retain the WriteLog, note that we will never retain for
/// less, even if WRITE_LOG_SOFT_MAX_SIZE_BYTES is exceeded.
pub static WRITE_LOG_MIN_RETENTION_SECS: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("WRITE_LOG_MIN_RETENTION_SECS", 30)));

/// The maximum time to retain the WriteLog, note that the WriteLog might be
/// trimmed sooner if it size exceeds WRITE_LOG_MAX_SIZE_BYTES.
pub static WRITE_LOG_MAX_RETENTION_SECS: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("WRITE_LOG_MAX_RETENTION_SECS", 300)));

/// The maximum size of the write log. Notes:
/// - the write log will be trimmed based on WRITE_LOG_MAX_RETENTION_SECS, and
///   thus it might never reach this size.
/// - the write log always retains at least WRITE_LOG_MIN_RETENTION_SECS, and
///   thus we could exceed this limit. The reason we do that is to allow some
///   minimum buffer for queries to refresh after execution.
pub static WRITE_LOG_SOFT_MAX_SIZE_BYTES: LazyLock<usize> =
    LazyLock::new(|| env_config("WRITE_LOG_SOFT_MAX_SIZE_BYTES", 50 * 1024 * 1024));

/// How frequently system tables are cleaned up.
pub static SYSTEM_TABLE_CLEANUP_FREQUENCY: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config(
        "SYSTEM_TABLE_CLEANUP_FREQUENCY_SECONDS",
        30 * 60,
    ))
});

/// Number of rows fetched and potentially deleted in a single transaction.
pub static SYSTEM_TABLE_CLEANUP_CHUNK_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("SYSTEM_TABLE_CLEANUP_CHUNK_SIZE", 64));

/// Maximum number of rows deleted per second.
/// This should not exceed the maximum rate that retention can process
/// tombstones, which is about 300.
pub static SYSTEM_TABLE_ROWS_PER_SECOND: LazyLock<NonZeroU32> = LazyLock::new(|| {
    env_config(
        "SYSTEM_TABLE_CLEANUP_ROWS_PER_SECOND",
        NonZeroU32::new(256).unwrap(),
    )
});

/// We can potentially reduce this window by changing
/// clients to track how long they have been open and throw an alert after
/// too many days. See go/idempotent-mutations
/// Zero indicates that there is no maximum.
pub static MAX_SESSION_CLEANUP_DURATION: LazyLock<Option<Duration>> = LazyLock::new(|| {
    let hours = env_config("MAX_SESSION_CLEANUP_DURATION_HOURS", 2 * 7 * 24);
    if hours > 0 {
        Some(Duration::from_secs(60 * 60 * hours))
    } else {
        None
    }
});

/// Number of concurrent commits to use for deleting session requests.
pub static SESSION_CLEANUP_DELETE_CONCURRENCY: LazyLock<usize> =
    LazyLock::new(|| env_config("SESSION_CLEANUP_DELETE_CONCURRENCY", 2));

/// Snapshots that expired more than this number of days ago are purged
/// from storage.
pub static MAX_EXPIRED_SNAPSHOT_AGE: LazyLock<Duration> = LazyLock::new(|| {
    let days = env_config("MAX_EXPIRED_SNAPSHOT_AGE_DAYS", 30);
    Duration::from_days(days)
});

/// Size at which a vector index will be queued for snapshotting vector indexes.
pub static VECTOR_INDEX_SIZE_SOFT_LIMIT: LazyLock<usize> =
    LazyLock::new(|| env_config("VECTOR_INDEX_SIZE_SOFT_LIMIT", 30 * (1 << 20))); // 30 MiB

/// Max number of threads used to build a disk index.
pub static VECTOR_INDEX_THREADS: LazyLock<usize> =
    LazyLock::new(|| env_config("VECTOR_INDEX_THREADS", 4));

/// Configures the vector and search index workers' rate limit on pages
/// processed per second. This is the default rate limit for anything a user
/// might be waiting on. It's initialized high enough that it effectively does
/// not rate limit. We keep the knob so that in an emergency we can reduce it.
///
/// NOTE: This number is multiplied with DEFAULT_DOCUMENTS_PAGE_SIZE, do not
/// set this value to such a larg number that doing so will overflow!
pub static SEARCH_WORKER_PAGES_PER_SECOND: LazyLock<NonZeroU32> = LazyLock::new(|| {
    env_config(
        "SEARCH_WORKER_PAGES_PER_SECOND",
        NonZeroU32::new(1000).unwrap(),
    )
});

/// Configures the vector and search index workers' rate limit on pages
/// processed per second for non-user facing rebuilds (well mostly non-user
/// facing, a user facing backfill might get stuck behind one of these).
///
/// The default is low so that an inadvertent metadata change or a deliberate
/// index backfill does not cause a thundering herd.
///
/// NOTE: This number is multiplied with DEFAULT_DOCUMENTS_PAGE_SIZE, do not
/// set this value to such a larg number that doing so will overflow!
pub static SEARCH_WORKER_PASSIVE_PAGES_PER_SECOND: LazyLock<NonZeroU32> = LazyLock::new(|| {
    env_config(
        "SEARCH_WORKER_PASSIVE_PAGES_PER_SECOND",
        NonZeroU32::new(10).unwrap(),
    )
});

/// Default page size (in number of docuemnts) used when loading documents from
/// the database for building a vector index.
pub static VECTOR_INDEX_WORKER_PAGE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("VECTOR_INDEX_WORKER_PAGE_SIZE", 128));

/// Timeout on "user time" spent during a UDF.
pub static DATABASE_UDF_USER_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("DATABASE_UDF_USER_TIMEOUT_SECONDS", 1)));

/// Timeout on the "system time" during a UDF -- i.e. syscalls.
// The user limits are not very tight, which requires us to have a high
// syscall timeout. When the database is healthy, we should never have UDF
// run into SYSTEM_TIMEOUT. UDFs are allowed to do up to 4096 unique queries,
// which at 1.6ms average, will take 6.4 seconds to run in sequence. We should
// aim to lower the SYSTEM_TIMEOUT limit over time by adding real parallelism
// and adding limit on number of `awaits` which is lower than the number of
// queries.
pub static DATABASE_UDF_SYSTEM_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("DATABASE_UDF_SYSTEM_TIMEOUT_SECONDS", 15)));

/// Timeout on the time it takes to analyze code during a push.
pub static ISOLATE_ANALYZE_USER_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("ISOLATE_ANALYZE_USER_TIMEOUT_SECONDS", 4)));

/// Increasing the size of the queue helps us deal with bursty requests. This is
/// a CoDel queue [https://queue.acm.org/detail.cfm?id=2209336], which will
/// switch from FIFO to LIFO queue when overloaded, in order to process as much
/// as possible and avoid a congestion collapse. The primary downside of
/// increasing this is memory usage from the UDF arguments.
pub static ISOLATE_QUEUE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("ISOLATE_QUEUE_SIZE", 2000));

/// The maximum length of time to wait to start running a function when the
/// isolate scheduler is idle.
pub static ISOLATE_QUEUE_IDLE_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("ISOLATE_QUEUE_IDLE_TIMEOUT_MS", 2000)));

/// The maximum length of time to wait to start running a function when the
/// isolate scheduler is "congested", according to the CoDel queue.
pub static ISOLATE_QUEUE_CONGESTED_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("ISOLATE_QUEUE_CONGESTED_TIMEOUT_MS", 200)));

/// Maximum number of isolate worker threads in a function runner process.
pub static MAX_ISOLATE_WORKERS: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_ISOLATE_WORKERS", 300));

/// The size of the pending commits in the committer queue. This is a FIFO
/// queue, so if the queue is too large, we run into a risk of all requests
/// waiting too long and no requests going through during overload. The size of
/// each commit request is also typically larger than a isolate request. For
/// time being, allow 128 slots, which is the maximum number of isolate threads
/// in any process.
pub static COMMITTER_QUEUE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("COMMITTER_QUEUE_SIZE", 128));

/// Maximum number of commits that are concurrently being processed by the
/// committer. This includes prevalidation, on-thread validation, writing to the
/// database, and publishing the commit.
pub static COMMITTER_MAX_CONCURRENT_COMMITS: LazyLock<usize> =
    LazyLock::new(|| env_config("COMMITTER_MAX_CONCURRENT_COMMITS", 512));

/// 0 -> default (number of cores)
pub static V8_THREADS: LazyLock<u32> = LazyLock::new(|| env_config("V8_THREADS", 0));

/// If false, each UDF runs in its own isolate with its own heap.
/// If true, each UDF runs in the same isolate in its own context, sharing a
/// heap.
pub static REUSE_ISOLATES: LazyLock<bool> = LazyLock::new(|| env_config("REUSE_ISOLATES", true));

/// Duration in seconds before an idle isolate is recreated
pub static ISOLATE_IDLE_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("ISOLATE_IDLE_TIMEOUT_SECONDS", 600)));

/// The maximum amount of time an isolate can be used before being recreated.
pub static ISOLATE_MAX_LIFETIME: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("ISOLATE_MAX_LIFETIME_SECONDS", 60 * 60)));

/// System timeout for V8 actions.
/// This doesn't count most syscalls, but it does count module loading.
pub static V8_ACTION_SYSTEM_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("V8_ACTION_SYSTEM_TIMEOUT_SECONDS", 5 * 60)));

/// The maximum amount of time a query or mutation waits for a concurrency
/// permit before being rejected.
pub static APPLICATION_FUNCTION_RUNNER_SEMAPHORE_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| {
        Duration::from_millis(env_config(
            "APPLICATION_FUNCTION_RUNNER_SEMAPHORE_TIMEOUT",
            5000,
        ))
    });

/// The maximum amount of time an action waits for a concurrency permit before
/// being rejected.
pub static APPLICATION_FUNCTION_RUNNER_ACTION_SEMAPHORE_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| {
        Duration::from_millis(env_config(
            "APPLICATION_FUNCTION_RUNNER_ACTION_SEMAPHORE_TIMEOUT",
            10000,
        ))
    });

/// The maximum write rate (per second) allowed for mutations and import
/// Default 4 MiB
pub static MAX_BYTES_WRITTEN_PER_SECOND: LazyLock<u64> =
    LazyLock::new(|| env_config("MAX_BYTES_WRITTEN_PER_SECOND", 4 * 1024 * 1024));

/// Proposed new limit for write rate (per second)
/// Default 1 MiB
pub static PROPOSED_MAX_BYTES_WRITTEN_PER_SECOND: LazyLock<u64> =
    LazyLock::new(|| env_config("PROPOSED_MAX_BYTES_WRITTEN_PER_SECOND", 1024 * 1024));

/// The maximum rate (per second) of document and index rows committed to
/// persistence, enforced on mutations and imports. Each document write also
/// writes one row per index on its table. 0 disables the limit.
pub static MAX_ROWS_WRITTEN_PER_SECOND: LazyLock<Option<u64>> =
    LazyLock::new(|| Some(env_config("MAX_ROWS_WRITTEN_PER_SECOND", 0)).filter(|&rows| rows > 0));

/// The time window (in milliseconds) used to track write throughput.
pub static WRITE_THROUGHPUT_WINDOW: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("WRITE_THROUGHPUT_WINDOW", 1000)));

/// Default max function concurrency limit for basic plan instances.
/// This value is used as the default for all APPLICATION_MAX_CONCURRENT
/// constants and is also used to determine if an instance is on a pro plan.
pub static DEFAULT_APPLICATION_MAX_FUNCTION_CONCURRENCY: usize = 16;

/// The maximum number of queries that can be run concurrently by an
/// application.
///
/// This is a per backend limit applied before FunctionRunner implementations.
///
/// The value here may be overridden by big brain.
pub static APPLICATION_MAX_CONCURRENT_QUERIES: LazyLock<usize> = LazyLock::new(|| {
    env_config(
        "APPLICATION_MAX_CONCURRENT_QUERIES",
        DEFAULT_APPLICATION_MAX_FUNCTION_CONCURRENCY,
    )
});

/// The maximum number of mutations that can be run concurrently by an
/// application.
///
/// This is a per backend limit applied before FunctionRunner implementations.
///
/// The value here may be overridden by big brain.
pub static APPLICATION_MAX_CONCURRENT_MUTATIONS: LazyLock<usize> = LazyLock::new(|| {
    env_config(
        "APPLICATION_MAX_CONCURRENT_MUTATIONS",
        DEFAULT_APPLICATION_MAX_FUNCTION_CONCURRENCY,
    )
});

/// The maximum number of v8 actions that can be run concurrently by an
/// application.
///
/// This is a higher level limit applied before FunctionRunner implementations.
///
/// This does apply to HTTP actions and does NOT apply to Node actions
///
/// Node actions are limited by the APPLICATION_MAX_CONCURRENT_NODE_ACTIONS
/// knob.
///
/// The value here may be overridden by big brain.
pub static APPLICATION_MAX_CONCURRENT_V8_ACTIONS: LazyLock<usize> =
    LazyLock::new(|| env_config("APPLICATION_MAX_CONCURRENT_V8_ACTIONS", 64));

/// The maximum number of node actions that can be run concurrently by an
/// application
///
/// Node actions are not sent through FunctionRunner implementations, so this is
/// a limit on the number of actions sent to AWS. AWS also has a global maximum
/// number of total concurrent actions across all backends. If we hit the AWS
/// limit, we'll see 429 error responses for node actions.
///
/// The value here may be overridden by big brain.
pub static APPLICATION_MAX_CONCURRENT_NODE_ACTIONS: LazyLock<usize> =
    LazyLock::new(|| env_config("APPLICATION_MAX_CONCURRENT_NODE_ACTIONS", 64));

/// The maximum number of concurrent package uploads during
/// `/api/deploy2/start_push` + `/api/deploy2/evaluate_push`.
pub static APPLICATION_MAX_CONCURRENT_UPLOADS: LazyLock<usize> =
    LazyLock::new(|| env_config("APPLICATION_MAX_CONCURRENT_UPLOADS", 4));

/// The number of modules to analyze concurrently during a push.
///
/// This only applies to isolate modules, not node ones.
pub static ANALYZE_CONCURRENCY: LazyLock<usize> =
    LazyLock::new(|| env_config("ANALYZE_CONCURRENCY", 4));

/// Set a 64MB limit on the heap size.
pub static ISOLATE_MAX_USER_HEAP_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("ISOLATE_MAX_USER_HEAP_SIZE", 1 << 26));

/// Allow for some objects to persist between contexts, not necessarily created
/// by the UDF.
pub static ISOLATE_MAX_HEAP_EXTRA_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("ISOLATE_MAX_HEAP_EXTRA_SIZE", 1 << 25));

/// Set a separate 64MB limit on ArrayBuffer allocations.
pub static ISOLATE_MAX_ARRAY_BUFFER_TOTAL_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("ISOLATE_MAX_ARRAY_BUFFER_TOTAL_SIZE", 1 << 26));

/// The heap limit for a function run in the wasm runtime, in bytes. One budget
/// covers what V8 splits between its heap and its ArrayBuffer pool, and it
/// counts only what a deployment's modules and the function allocate, not the
/// runtime's own globals. Defaults to V8's heap limit,
/// `ISOLATE_MAX_USER_HEAP_SIZE` plus `ISOLATE_MAX_HEAP_EXTRA_SIZE`.
pub static WASM_UDF_MAX_HEAP_SIZE: LazyLock<usize> = LazyLock::new(|| {
    env_config(
        "WASM_UDF_MAX_HEAP_SIZE",
        *ISOLATE_MAX_USER_HEAP_SIZE + *ISOLATE_MAX_HEAP_EXTRA_SIZE,
    )
});

/// How far a wasm function's linear memory may grow past its starting size, as
/// a multiple of `WASM_UDF_MAX_HEAP_SIZE`. The heap limit counts the bytes live
/// allocations requested; this backstop bounds what that count leaves out, the
/// allocator's bookkeeping and fragmentation, which a function that frees and
/// reallocates in mixed sizes can make large since linear memory never shrinks.
pub static WASM_UDF_MEMORY_HEADROOM_FACTOR: LazyLock<f64> =
    LazyLock::new(|| env_config("WASM_UDF_MEMORY_HEADROOM_FACTOR", 2.0));

/// Chunk sizes: 1, 2, 3, ..., MAX_DYNAMIC_SMART_CHUNK_SIZE incrementing by 1.
/// These chunk sizes allow small (common) batches to be handled in a single
/// chunk, while limiting the size of a chunk (don't overload the db), and
/// keeping the number of distinct queries small (for query plan caching).
pub static MYSQL_MAX_DYNAMIC_SMART_CHUNK_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MYSQL_MAX_DYNAMIC_SMART_CHUNK_SIZE", 8));
/// More chunks sizes: 1, 2, 4, 8, 16, 32, ..., MYSQL_SMART_CHUNK_MAX_SIZE
/// doubling. Max packet size is 16MiB.
pub static MYSQL_MAX_CHUNK_BYTES: LazyLock<usize> =
    LazyLock::new(|| env_config("MYSQL_MAX_CHUNK_BYTES", 10 << 20));

/// Most rows `fill_chunks` puts in one statement. Past ~2,200 `OR`ed primary
/// keys MySQL full-scans and locks the table; far larger statements can
/// outlast `MYSQL_TIMEOUT` on a cold cache.
pub static MYSQL_MAX_FILL_CHUNK_ROWS: LazyLock<usize> =
    LazyLock::new(|| env_config("MYSQL_MAX_FILL_CHUNK_ROWS", 1024));

/// Page size to fall back to when Vitess rejects large result sets.
/// Vitess limits query results to 64MiB. As documents can be up to 1MiB (plus
/// some overhead) and system documents can be larger still, we may need to fall
/// back to a much smaller page size if we hit the limit while loading
/// documents. If the fallback page size still fails, we'll try page size 1
/// before giving up.
pub static MYSQL_FALLBACK_PAGE_SIZE: LazyLock<u32> =
    LazyLock::new(|| env_config("MYSQL_FALLBACK_PAGE_SIZE", 5));

/// Timeout for all operations on MySQL connections, Vitess timeout is 20s so
/// set lower than that
pub static MYSQL_TIMEOUT: LazyLock<u64> = LazyLock::new(|| env_config("MYSQL_TIMEOUT_SECONDS", 19));

/// Maximum number of connections to MySQL
pub static MYSQL_MAX_CONNECTIONS: LazyLock<usize> =
    LazyLock::new(|| env_config("MYSQL_MAX_CONNECTIONS", 128));

/// Minimum number of rows to read from MySQL in a single query.
pub static MYSQL_MIN_QUERY_BATCH_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MYSQL_MIN_QUERY_BATCH_SIZE", 1));

/// Maximum number of rows to read from MySQL in a single query.
pub static MYSQL_MAX_QUERY_BATCH_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MYSQL_MAX_QUERY_BATCH_SIZE", 5000));

/// We dynamically increase the batch size up to this threshold if client keeps
/// fetching more results. This helps correct for tombstones, long prefixes and
/// wrong client size estimates.
pub static MYSQL_MAX_QUERY_DYNAMIC_BATCH_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MYSQL_MAX_QUERY_DYNAMIC_BATCH_SIZE", 8));

/// Target number of rows deleted per second from each logical table during
/// deployment cleanup. Lower values reduce database load at the cost of slower
/// cleanup; the value must be nonzero.
pub static DEPLOYMENT_DELETION_TARGET_ROWS_PER_SECOND: LazyLock<NonZeroU32> = LazyLock::new(|| {
    env_config(
        "DEPLOYMENT_DELETION_TARGET_ROWS_PER_SECOND",
        NonZeroU32::new(5000).unwrap(),
    )
});

/// Maximum number of rows deleted in one MySQL deployment cleanup query. Lower
/// values reduce the size and lock duration of each transaction at the cost of
/// more queries; the value must be nonzero.
pub static DEPLOYMENT_DELETION_BATCH_SIZE: LazyLock<NonZeroU32> = LazyLock::new(|| {
    env_config(
        "DEPLOYMENT_DELETION_BATCH_SIZE",
        NonZeroU32::new(2500).unwrap(),
    )
});

/// Close a connection after it has been idle for some time. RDS proxy closes
/// connections after idle_client_timeout in mysql.tf, which should be
/// configured to be higher than this.
///
/// This is overridden to 30 min in funrun
pub static MYSQL_INACTIVE_CONNECTION_LIFETIME: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("MYSQL_INACTIVE_CONNECTION_LIFETIME_SECS", 90))
});

/// Force recycles a database connections after this period. RDS proxy pins
/// connections if the SQL query which exceeded the 16384 byte limit. Having a
/// hard limit on connection lifetime helps us reduce pinning and improve
/// connection reuse.
///
/// This is overridden to 30 min in funrun
pub static MYSQL_MAX_CONNECTION_LIFETIME: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("MYSQL_MAX_CONNECTION_LIFETIME_SECS", 600)));

/// The largest chunk `smart_chunks` produces, in rows.
pub static MYSQL_SMART_CHUNK_MAX_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MYSQL_SMART_CHUNK_MAX_SIZE", 128));

/// Which encoding version to use for newly written documents
pub static MYSQL_DOCUMENT_ENCODING: LazyLock<u8> =
    LazyLock::new(|| env_config("MYSQL_DOCUMENT_ENCODING", 1));

/// How many times to retry MySQL queries that fail with operational errors.
pub static MYSQL_MAX_QUERY_RETRIES: LazyLock<u32> =
    LazyLock::new(|| env_config("MYSQL_MAX_QUERY_RETRIES", 1));

/// Maximum number of connections to Postgres
pub static POSTGRES_MAX_CONNECTIONS: LazyLock<usize> =
    LazyLock::new(|| env_config("POSTGRES_MAX_CONNECTIONS", 128));

/// Maximum number of cached statements per Postgres connection
pub static POSTGRES_MAX_CACHED_STATEMENTS: LazyLock<NonZeroUsize> = LazyLock::new(|| {
    env_config(
        "POSTGRES_MAX_CACHED_STATEMENTS",
        NonZeroUsize::new(128).unwrap(),
    )
});

/// Close Postgres connections after being idle for this long.
pub static POSTGRES_INACTIVE_CONNECTION_LIFETIME: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("POSTGRES_INACTIVE_CONNECTION_LIFETIME_SECS", 90))
});

/// How many actions "ops" (e.g. syscalls) can execute concurrently.
pub static MAX_CONCURRENT_ACTION_OPS: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_CONCURRENT_ACTION_OPS", 8));

/// Maximum count of transitions within the web socket server message buffer.
/// When this limit is reached, the web socket worker will temporary stop
/// computing and sending transition messages to the client.
pub static SYNC_MAX_SEND_TRANSITION_COUNT: LazyLock<usize> =
    LazyLock::new(|| env_config("SYNC_MAX_SEND_TRANSITION_COUNT", 2));

/// Max Axiom sink attributes. This is a knob just in case a user actually hits
/// the limit but has an Enterprise Axiom plan that lets them use more than the
/// limit we've configured.
pub static AXIOM_MAX_ATTRIBUTES: LazyLock<usize> =
    LazyLock::new(|| env_config("AXIOM_MAX_ATTRIBUTES", 1024));

/// If a qdrant Segment's estimated byte is is <= this threshold, we'll build a
/// plain index without HNSW. The default value is 2.5x qdrants. We hope this is
/// ok due to payload filtering, compaction and aiming for 30mb indexes via
/// VECTOR_INDEX_SIZE_SOFT_LIMIT. This is > VECTOR_INDEX_SIZE_SOFT_LIMIT so that
/// have some wiggle room if we build a slightly larger than expected segment.
pub static MULTI_SEGMENT_FULL_SCAN_THRESHOLD_KB: LazyLock<usize> =
    LazyLock::new(|| env_config("MULTI_SEGMENT_FULL_SCAN_THRESHOLD_KB", 50_000));

/// The maximum size that we will compact any given set of segments to.
/// Default to using 3 segments per 1.1 million vectors.
pub static SEGMENT_MAX_SIZE_BYTES: LazyLock<u64> =
    LazyLock::new(|| env_config("SEGMENT_MAX_SIZE_BYTES", (1100000 * 2048 * 4) / 3_u64));
/// The minimum number of segments we will compact in one pass.
pub static MIN_COMPACTION_SEGMENTS: LazyLock<u64> =
    LazyLock::new(|| env_config("MIN_COMPACTION_SEGMENTS", 3));
/// The maximum number of segments to compact in one request.
pub static MAX_COMPACTION_SEGMENTS: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_COMPACTION_SEGMENTS", 10));
/// The maximum percentage of a Segment that can be deleted before we will
/// recompact that segment to remove deleted vectors
/// This number must be between 0 and 1.
pub static MAX_SEGMENT_DELETED_PERCENTAGE: LazyLock<f64> =
    LazyLock::new(|| env_config("MAX_SEGMENT_DELETED_PERCENTAGE", 0.2));

/// Whether to run queries, mutations, HTTP actions, and v8 actions in Funrun
/// (true) or InProcessFunctionRunner (false).
pub static UDF_USE_FUNRUN: LazyLock<bool> = LazyLock::new(|| env_config("UDF_USE_FUNRUN", true));

/// Whether transactional subfunctions (runQuery/runMutation from inside a
/// query/mutation) should run in the same isolate as the caller.
pub static SUBFUNCTIONS_IN_SAME_ISOLATE: LazyLock<bool> =
    LazyLock::new(|| env_config("SUBFUNCTIONS_IN_SAME_ISOLATE", false));

/// The amount of time to wait for the primary request to finish before starting
/// a second backup request when running a vector search.
pub static VECTOR_BACKUP_REQUEST_DELAY_MILLIS: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("VECTOR_BACKUP_REQUEST_DELAY_MILLIS", 30)));

/// Whether to use prepared statements or not in Persistence.
pub static DATABASE_USE_PREPARED_STATEMENTS: LazyLock<bool> =
    LazyLock::new(|| env_config("DATABASE_USE_PREPARED_STATEMENTS", false));

/// The amount of time to allow for downloading a single file in the archive
/// cache on searchlight before timing out. If a fetch times out, no progress is
/// made and a subsequent request for the same file will start from the
/// beginning. If this number is too low relative to our disk throughput,
/// archive sizes and concurrent fetches, it can cause congestion collapse when
/// searchlight is restarted.
pub static ARCHIVE_FETCH_TIMEOUT_SECONDS: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("ARCHIVE_FETCH_TIMEOUT_SECONDS", 150)));

/// The total size of source maps, across all deployments and module versions,
/// that will be held in memory at once.
pub static SOURCE_MAP_CACHE_MAX_SIZE_BYTES: LazyLock<u64> =
    LazyLock::new(|| env_config("SOURCE_MAP_CACHE_MAX_SIZE_BYTES", 100_000_000));

/// The maximum number of concurrent source package fetches we'll allow when
/// filling the source map cache.
pub static SOURCE_MAP_CACHE_MAX_CONCURRENCY: LazyLock<usize> =
    LazyLock::new(|| env_config("SOURCE_MAP_CACHE_MAX_CONCURRENCY", 200));

/// The maximum number of queued source map cache requests.
pub static SOURCE_MAP_CACHE_QUEUE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("SOURCE_MAP_CACHE_QUEUE_SIZE", 400));

/// The maximum size of the in memory index cache in Funrun in bytes.
pub static FUNRUN_INDEX_CACHE_SIZE: LazyLock<u64> =
    LazyLock::new(|| env_config("FUNRUN_INDEX_CACHE_SIZE", 50_000_000)); // 50 MB

/// The maximum number of concurrent index cache requests in Funrun.
pub static FUNRUN_INDEX_CACHE_CONCURRENCY: LazyLock<usize> =
    LazyLock::new(|| env_config("FUNRUN_INDEX_CACHE_CONCURRENCY", 200));

/// The maximum number of queued index cache requests in funrun.
pub static FUNRUN_INDEX_CACHE_QUEUE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("FUNRUN_INDEX_CACHE_QUEUE_SIZE", 400));

/// The maximum size of the module cache in Funrun in bytes.
pub static FUNRUN_MODULE_CACHE_SIZE: LazyLock<u64> =
    LazyLock::new(|| env_config("FUNRUN_MODULE_CACHE_SIZE", 250_000_000));

/// The maximum number of concurrent module cache requests in Funrun.
pub static FUNRUN_MODULE_MAX_CONCURRENCY: LazyLock<usize> =
    LazyLock::new(|| env_config("FUNRUN_MODULE_MAX_CONCURRENCY", 200));

/// The maximum number of queued module cache requests in funrun.
pub static FUNRUN_MODULE_QUEUE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("FUNRUN_MODULE_QUEUE_SIZE", 400));

/// The maximum size of the module code cache in Funrun in bytes.
pub static FUNRUN_CODE_CACHE_SIZE: LazyLock<u64> =
    LazyLock::new(|| env_config("FUNRUN_CODE_CACHE_SIZE", 500_000_000));

/// The maximum number of fetch clients Funrun would create.
pub static FUNRUN_FETCH_CLIENT_CACHE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("FUNRUN_FETCH_CLIENT_CACHE_SIZE", 100));

/// The maximum number of concurrent requests a single client can make to a
/// single Funrun server.
/// NOTE: When changing this value, ensure that the following parameters
/// for pro satisfy this inequality. This makes sure that if one funrun instance
/// is down, we still have enough capacity to satisfy requests if a backend is
/// maxing out its concurrent function call limits.
///
/// max_concurrent_queries +
/// max_concurrent_mutations + max_concurrent_v8_actions +
/// max_concurrent_http_actions < (number of funrun nodes - 1) *
/// FUNRUN_CLIENT_MAX_REQUESTS_PER_UPSTREAM
pub static FUNRUN_CLIENT_MAX_REQUESTS_PER_UPSTREAM: LazyLock<usize> =
    LazyLock::new(|| env_config("FUNRUN_CLIENT_MAX_REQUESTS_PER_UPSTREAM", 15));

/// The maximum number of retries a Funrun client will perform. The client only
/// retries overloaded errors.
pub static FUNRUN_CLIENT_MAX_RETRIES: LazyLock<usize> =
    LazyLock::new(|| env_config("FUNRUN_CLIENT_MAX_RETRIES", 4));

/// Value between 1 and 100 representing the percent of a the shared scheduler's
/// worker pool that a single client can use.
pub static FUNRUN_SCHEDULER_MAX_PERCENT_PER_CLIENT: LazyLock<usize> =
    LazyLock::new(|| env_config("FUNRUN_SCHEDULER_MAX_PERCENT_PER_CLIENT", 50));

/// Name of the service to discover for when connecting to Funrun (e.g.
/// funrun-default, funrun-staging, etc.)
pub static FUNRUN_CLUSTER_NAME: LazyLock<String> =
    LazyLock::new(|| env_config("FUNRUN_CLUSTER_NAME", String::from("funrun-default")));

/// Name of the service to discover for when connecting to the Funruns using
/// Ticketmaster (e.g. ticketed-funrun-default, ticketed-funrun-staging, etc.)
pub static TICKETED_FUNRUN_CLUSTER_NAME: LazyLock<String> = LazyLock::new(|| {
    env_config(
        "TICKETED_FUNRUN_CLUSTER_NAME",
        String::from("ticketed-funrun-default"),
    )
});

/// Name of the service to discover for when connecting to Searchlight. (e.g.
/// searchlight-default, searchlight-staging, etc.)
// cluster is created.
pub static SEARCHLIGHT_CLUSTER_NAME: LazyLock<String> = LazyLock::new(|| {
    env_config(
        "SEARCHLIGHT_CLUSTER_NAME",
        String::from("searchlight-default"),
    )
});

/// Name of the service to discover for when connecting to Ticketmaster (e.g.
/// ticketmaster, ticketmaster-staging, etc.)
pub static TICKETMASTER_CLUSTER_NAME: LazyLock<String> =
    LazyLock::new(|| env_config("TICKETMASTER_CLUSTER_NAME", String::from("ticketmaster")));

/// Timeout on each probe request, bounding how long a hung deployment holds
/// one of the round's `probe_concurrency` slots. 99.995% of probes finish
/// inside a second, and the handful a week that run longer take over five.
///
/// Also bounds picking the round's targets, which is a handful of indexed
/// queries and so the same order of work; without it a hung pick stops the
/// probe loop with its gauges frozen.
pub static PROBER_PROBE_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("PROBER_PROBE_TIMEOUT", 5)));

/// The maximum number of CPU cores that can be used simultaneously by the
/// isolates. Zero means no limit.
pub static FUNRUN_ISOLATE_ACTIVE_THREADS: LazyLock<usize> =
    LazyLock::new(|| env_config("FUNRUN_ISOLATE_ACTIVE_THREADS", 0));

/// Isolate worker usage at which the funrun load reporter's
/// `effective_load` saturates to 1.0.
pub static FUNRUN_TARGET_ISOLATE_WORKER_USAGE: LazyLock<f64> =
    LazyLock::new(|| env_config("FUNRUN_TARGET_ISOLATE_WORKER_USAGE", 0.75));

/// CPU utilization at which the funrun load reporter's
/// `effective_load` saturates to 1.0.
pub static FUNRUN_TARGET_CPU_USAGE: LazyLock<f64> =
    LazyLock::new(|| env_config("FUNRUN_TARGET_CPU_USAGE", 0.90));

/// Linux CPU PSI at which the funrun load reporter's `effective_load` saturates
/// to 1.0. Pressure rises before pure utilization does, so this gives the
/// router an earlier signal that a host is starting to struggle.
pub static FUNRUN_MAX_CPU_PRESSURE: LazyLock<f64> =
    LazyLock::new(|| env_config("FUNRUN_MAX_CPU_PRESSURE", 0.20));

/// How long to splay deploying AWS Lambdas due to changes in the backend. This
/// knob doesn't delay deploys that are required due to the user pushing new
/// node actions. Only affects deploys on startup triggered by changes to
/// backend/node-executor code.
///
/// AWS has a rate limit of 15/s on their APIs, so we need this to be roughly
/// larger than (N / 15) where N is the number of instances with lambdas.
///
/// You can check go/num-instances-with-lambdas
pub static AWS_LAMBDA_DEPLOY_SPLAY: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("AWS_LAMBDA_DEPLOY_SPLAY_SECONDS", 172800)));

/// How long of a window to debounce static lambda deployments. Don't allow too
/// many static deploys in a small window to protect the infrastructure.
pub static AWS_LAMBDA_STATIC_DEBOUNCE_DELAY: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("AWS_LAMBDA_STATIC_DEBOUNCE_DELAY_SECONDS", 30))
});

/// Whether instances should self-manage cleanup of their old Lambda versions in
/// a background worker.
pub static AWS_LAMBDA_SELF_CLEANUP_ENABLED: LazyLock<bool> =
    LazyLock::new(|| env_config("AWS_LAMBDA_SELF_CLEANUP_ENABLED", true));

/// Maximum initial startup jitter for the per-instance Lambda version cleanup
/// worker. The worker will wait up to this long before starting the first
/// cleanup pass, or will start whenever a new lambda version is deployed.
pub static AWS_LAMBDA_CLEANUP_INITIAL_JITTER_MAX_SECS: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config(
        "AWS_LAMBDA_CLEANUP_INITIAL_JITTER_MAX_SECS",
        86400,
    ))
});

/// The maximum number of requests to send using a single AWS Lambda client.
/// Empirical tests have shown that AWS servers allows up to 128 concurrent
/// streams over a single http2 connection.
pub static AWS_LAMBDA_CLIENT_MAX_CONCURRENT_REQUESTS: LazyLock<usize> =
    LazyLock::new(|| env_config("AWS_LAMBDA_MAX_CONCURRENT_STREAMS_PER_CONNECTION", 100));

/// Memory limit in MB for the static AWS Lambda (executes user actions with
/// pre-deployed code). If you update this value, make sure to update the
/// actions resource limits in the docs.
pub static AWS_STATIC_LAMBDA_MEMORY_LIMIT_MB: LazyLock<i32> =
    LazyLock::new(|| env_config("AWS_STATIC_LAMBDA_MEMORY_LIMIT_MB", 512));

/// Disk size limit in MB for the static AWS Lambda.
pub static AWS_STATIC_LAMBDA_DISK_SIZE_MB: LazyLock<i32> =
    LazyLock::new(|| env_config("AWS_STATIC_LAMBDA_DISK_SIZE_MB", 512));

/// Memory limit in MB for the dynamic AWS Lambda (handles BuildDeps, Analyze,
/// and fallback Execute requests when the static lambda has not been deployed
/// yet). The dynamic memory limit must be >= the static limit to be able to
/// execute fallback requests.
pub static AWS_DYNAMIC_LAMBDA_MEMORY_LIMIT_MB: LazyLock<i32> =
    LazyLock::new(|| env_config("AWS_DYNAMIC_LAMBDA_MEMORY_LIMIT_MB", 4096));

/// Disk size limit in MB for the dynamic AWS Lambda. The dynamic disk size must
/// be >= the static limit to be able to execute fallback requests.
pub static AWS_DYNAMIC_LAMBDA_DISK_SIZE_MB: LazyLock<i32> =
    LazyLock::new(|| env_config("AWS_DYNAMIC_LAMBDA_DISK_SIZE_MB", 2048));

/// The maximum number of times to retry analyze requests for node actions.
pub static NODE_ANALYZE_MAX_RETRIES: LazyLock<usize> =
    LazyLock::new(|| env_config("NODE_ANALYZE_MAX_RETRIES", 3));

/// The number of seconds backend should wait for requests to drain before
/// shutting down after SIGINT.
pub static BACKEND_REQUEST_DRAIN_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("BACKEND_REQUEST_DRAIN_TIMEOUT", 15)));

/// The kinesis firehose name for streaming usage metrics to the data
// large body of water.
pub static BACKEND_USAGE_FIREHOSE_NAME: LazyLock<Option<String>> = LazyLock::new(|| {
    let result = env_config(
        "BACKEND_USAGE_FIREHOSE_NAME",
        prod_override("", "cvx-firehose-usage-prod").to_string(),
    );
    if !result.is_empty() {
        Some(result)
    } else {
        None
    }
});

/// Local usage Firehose output as JSONL; ignored in production.
pub static USAGE_FIREHOSE_FILE: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    if *IS_PROD {
        return None;
    }
    let result: String = env_config("USAGE_FIREHOSE_FILE", String::new());
    (!result.is_empty()).then(|| PathBuf::from(result))
});

/// Local audit Firehose output with egress accounting; ignored in production.
pub static AUDIT_LOG_FIREHOSE_FILE: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    if *IS_PROD {
        return None;
    }
    let result: String = env_config("AUDIT_LOG_FIREHOSE_FILE", String::new());
    (!result.is_empty()).then(|| PathBuf::from(result))
});

/// Maximum buffered usage-aggregation keys across the process, shared by
/// every aggregated event kind. Key strings are bounded function and component
/// paths, so this bounds aggregation memory. At capacity, existing keys
/// accumulate and new keys lose their usage.
pub static USAGE_AGGREGATION_MAX_KEYS: LazyLock<usize> =
    LazyLock::new(|| env_config("USAGE_AGGREGATION_MAX_KEYS", 100_000));

/// Firehose stream for AI gateway usage rows, landing at
/// `s3://cvx-data-lake-prod/ai_usage/`. Its own stream, not the backend usage
/// one, so AI ingest lag can't stall usage metering. Empty turns emit off.
pub static AI_GATEWAY_USAGE_FIREHOSE: LazyLock<Option<String>> = LazyLock::new(|| {
    let result = env_config(
        "AI_GATEWAY_USAGE_FIREHOSE",
        prod_override("", "cvx-firehose-ai_usage-prod").to_string(),
    );
    if !result.is_empty() {
        Some(result)
    } else {
        None
    }
});

/// If usage tracking worker takes longer than this, trace details to logs.
pub static USAGE_TRACKING_WORKER_SLOW_TRACE_THRESHOLD: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config(
        "USAGE_TRACKING_WORKER_SLOW_TRACE_THRESHOLD_SECONDS",
        120,
    ))
});

/// How many `_file_storage` documents the deployment must hold before the
/// usage gauges' file storage total resumes from its previous sync rather than
/// syncing the storage tables again. Resuming catches up along the document
/// log, which reads every table's revisions in the timestamp range, not just
/// `_file_storage`'s, so below this many documents a fresh sync is cheaper.
pub static FILE_STORAGE_SIZE_MIN_DOCUMENTS_TO_RESUME: LazyLock<i64> =
    LazyLock::new(|| env_config("FILE_STORAGE_SIZE_MIN_DOCUMENTS_TO_RESUME", 4096));

/// The maximum amount of time a mutation can take to upload stored files.
pub static UDF_FILE_STORAGE_UPLOAD_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("UDF_FILE_STORAGE_UPLOAD_TIMEOUT_SECONDS", 5)));

/// Maximum amount of time a mutation can spend downloading files before
/// failing.
pub static UDF_FILE_STORAGE_DOWNLOAD_TIMEOUT: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("UDF_FILE_STORAGE_DOWNLOAD_TIMEOUT_SECONDS", 5))
});

/// How many times to rerun a mutation with file storage failures before
/// returning the error to its caller.
pub static UDF_FILE_STORAGE_MAX_RETRIES: LazyLock<usize> =
    LazyLock::new(|| env_config("UDF_FILE_STORAGE_MAX_RETRIES", 2));

/// Initial backoff before rerunning a mutation with file storage failures.
pub static UDF_FILE_STORAGE_INITIAL_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("UDF_FILE_STORAGE_INITIAL_BACKOFF_MS", 500)));

/// Maximum backoff before rerunning a mutation with file storage failures.
pub static UDF_FILE_STORAGE_MAX_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("UDF_FILE_STORAGE_MAX_BACKOFF_MS", 2000)));

/// The number of events we can accumulate in the buffer that's used to send
/// events from our business logic to our firehose client.
///
/// We use try_send to send data over a channel, so it's possible we send faster
/// than the receiving thread can read the data. The buffer helps mitigate that.
///
/// Each item in the buffer is a Vec of events. If we assume that each event is
/// 100 bytes, each Vec might have 100 events and we keep 2000 of these, then
/// worst case our usage is ~100MB.
pub static FIREHOSE_BUFFER_SIZE_COUNT: LazyLock<usize> =
    LazyLock::new(|| env_config("FIREHOSE_BUFFER_SIZE_COUNT", 2000));

/// The maximum amount of data we'll populate in a single AWS Kinesis Firehose
/// Record.
///
/// Each Record in AWS has a maximum size of 1000 KiB. PutRecordBatch (which
/// we'll use if a single list of events exceeds this buffer size) has a maximum
/// limit of 4 MiB of records. This controls only the size of the Record, not
/// the size of the batch. We do not currently make any attempt to control the
/// batch size.
pub static FIREHOSE_MAX_BATCH_SIZE_BYTES: LazyLock<usize> =
    LazyLock::new(|| env_config("FIREHOSE_MAX_BATCH_SIZE_BYTES", 900 * 1024));

/// The amount of time we'll allow firehose data to sit in memory while it
/// remains under our buffer size. Once this timeout is exceeded for the oldest
/// buffered record, we'll send our entire buffer.
pub static FIREHOSE_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("FIREHOSE_TIMEOUT", 60)));

/// The initial backoff time for index workers when a failure occurs.
pub static INDEX_WORKERS_INITIAL_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("INDEX_WORKERS_INITIAL_BACKOFF", 500)));

/// The maximum backoff time for index workers when a failure occurs.
pub static INDEX_WORKERS_MAX_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("INDEX_WORKERS_MAX_BACKOFF", 30 * 1000)));

/// The maximum backoff time for search index flusher workers when a failure
/// occurs. This shouldn't be set too high because flushes are required for
/// write throughput.
pub static SEARCH_INDEX_FLUSHER_MAX_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("SEARCH_INDEX_FLUSHER_MAX_BACKOFF", 30)));

/// The maximum backoff time for search compactor workers when a failure occurs.
/// This can be set relatively high because compaction is not a critical
/// operation. If compaction fails, latency for searches may increase from
/// searching too many segments. Setting this too low can overwhelm searchlight
/// on persistent failures.
pub static SEARCH_COMPACTOR_MAX_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("SEARCH_COMPACTOR_MAX_BACKOFF", 10 * 60)));

/// The initial backoff time for search compactor workers when a failure occurs.
/// This can be set relatively high because compaction is not a critical
/// operation. If compaction fails, latency for searches may increase from
/// searching too many segments. Setting this too low can overwhelm searchlight
/// on persistent failures.
pub static SEARCH_COMPACTOR_INITIAL_BACKOFF: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("SEARCH_COMPACTOR_INITIAL_BACKOFF", 60)));

/// The maximum size for Funrun run function request messages. The user-level
/// limit is 16MiB for args (as counted by ConvexValue::size), but the actual
/// wire encoding can be 3-4x that, so we set a higher limit here.
///
/// Note that analyze has a much higher limit because it includes user source
/// code.
pub static MAX_FUNRUN_RUN_FUNCTION_REQUEST_MESSAGE_SIZE: LazyLock<usize> = LazyLock::new(|| {
    env_config(
        "MAX_FUNRUN_RUN_FUNCTION_REQUEST_MESSAGE_SIZE",
        1 << 26, // 64MiB
    )
});

/// The maximum size for Funrun run function response messages. 8MiB for reads +
/// 16 MiB for writes + 16MiB for function result + 8MiB for log lines and a 8
/// MiB buffer for the smaller fields.
pub static MAX_FUNRUN_RUN_FUNCTION_RESPONSE_MESSAGE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_FUNRUN_RUN_FUNCTION_RESPONSE_MESSAGE_SIZE", 1 << 26)); // 64 MiB

/// The maximum size for Backend HTTP and GRPC action callbacks. This is 8MiB
/// for path and args, plus a generous buffer for the smaller fields This should
/// also be enough for vector and text search callbacks.
pub static MAX_BACKEND_RPC_REQUEST_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_BACKEND_RPC_REQUEST_SIZE", 1 << 25)); // 32 MiB

/// The maximum size for Backend HTTP and GRPC action callbacks.
/// This needs to contain 16MiB for FUNCTION_MAX_RESULT_SIZE + 4MiB for log
/// lines + 4MiB for smaller fields.
pub static MAX_BACKEND_RPC_RESPONSE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_BACKEND_RPC_RESPONSE_SIZE", 1 << 25)); // 32 MiB

/// The maximum size of byte chunks used when transmitting HTTP request/response
/// bodies as part of HTTP Actions.
pub static MAX_BACKEND_RPC_HTTP_CHUNK_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_BACKEND_RPC_HTTP_CHUNK_SIZE", 1 << 23)); // 8 MiB

/// The maximum size for requests to the backend public API. Must be at least 16
/// MiB for function arguments.
pub static MAX_BACKEND_PUBLIC_API_REQUEST_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_BACKEND_PUBLIC_API_REQUEST_SIZE", (1 << 24) + 2000)); // 16 MiB

/// The maximum size for GRPC responses from searchlight.
pub static MAX_SEARCHLIGHT_RESPONSE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_SEARCHLIGHT_RESPONSE_SIZE", 1 << 23)); // 8 MiB

/// The maximum size for GRPC responses from searchlight.
pub static MAX_SEARCHLIGHT_REQUEST_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_SEARCHLIGHT_REQUEST_SIZE", 1 << 23)); // 8 MiB

/// Background database workers wake up periodically, check to see if something
/// has changed, then either go back to sleep or do work. Most workers determine
/// if something has changed at least in part by comparing the number of commits
/// since the last time they woke up. This works perfectly if there's exactly
/// one worker. But we actually have N workers, so if each of them wakes up and
/// does work whenever there's a new commit, they're all constantly doing work
/// because of each other.
///
/// So a simple way to avoid this is to require there to at least N commits
/// rather than >=1 commit. As long as N is greater than the number of workers,
/// each worker will wake up, commit once, then go back to sleep until something
/// other than the other workers happens.
///
/// We must also ensure that workers advance periodically to ensure that we can
/// run document retention. Database times are bumped periodically
/// even if no writes occur. So any worker that checks this should always have
/// some maximum period of time after which they checkpoint unconditionally.
pub static DATABASE_WORKERS_MIN_COMMITS: LazyLock<usize> =
    LazyLock::new(|| env_config("DATABASE_WORKERS_MIN_COMMITS", 500));

/// Checkpoint table summaries unconditionally -- even with no writes -- once
/// every 4 hours. This must stay bounded so the checkpoint keeps advancing and
/// document retention can make progress (see [`DATABASE_WORKERS_MIN_COMMITS`]).
pub static TABLE_SUMMARY_MAX_CHECKPOINT_AGE: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("TABLE_SUMMARY_MAX_CHECKPOINT_AGE", 4 * 60 * 60))
});

/// When an deployment has committed writes since the last table summary
/// checkpoint, checkpoint (and republish table shapes) at least this often.
/// This bounds how stale the published table shapes -- the dashboard's
/// generated schema and streaming export's column names -- can get on
/// deployments with write traffic too low to hit
/// [`DATABASE_WORKERS_MIN_COMMITS`]. Deployments with no writes at all fall
/// back to the slower [`TABLE_SUMMARY_MAX_CHECKPOINT_AGE`] cadence.
pub static TABLE_SUMMARY_MAX_STALENESS: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("TABLE_SUMMARY_MAX_AGE_WITH_WRITES", 10 * 60)));

/// The TableSummaryWorker must checkpoint every
/// [`TABLE_SUMMARY_MAX_CHECKPOINT_AGE`] seconds even if nothing has changed.
/// However, to prevent all instances from checkpointing at the same time, we'll
/// add a jitter of up to ±TABLE_SUMMARY_AGE_JITTER_SECONDS.
///
/// Note: the configured value is capped at
/// `TABLE_SUMMARY_MAX_CHECKPOINT_AGE/2`.
pub static TABLE_SUMMARY_AGE_JITTER_SECONDS: LazyLock<f32> =
    LazyLock::new(|| env_config("TABLE_SUMMARY_AGE_JITTER_SECONDS", 900.0));

/// HTTP requests to backend will time out after this duration has passed.
///
/// See https://docs.rs/tower-http/0.5.0/tower_http/timeout/struct.TimeoutLayer.html
pub static HTTP_SERVER_TIMEOUT_DURATION: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("HTTP_SERVER_TIMEOUT_SECONDS", 300)));

/// The limit on the request size to /push_config.
// Schema and code bundle pushes must be less than this.
pub static MAX_PUSH_BYTES: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_PUSH_BYTES", 200_000_000));

/// Maximum buffered size, in bytes, of one client-driven snapshot import part.
pub static MAX_IMPORT_UPLOAD_PART_BYTES: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_IMPORT_UPLOAD_PART_BYTES", 64 * 1024 * 1024));

/// The limit on the request size to /echo. Limits requests to 128MiB to help
/// mitigate DDoS attacks.
pub static MAX_ECHO_BYTES: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_ECHO_BYTES", 128 * 1024 * 1024));

/// The limit on the number of user modules in a push bundle.
pub static MAX_USER_MODULES: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_USER_MODULES", 4096));

/// Percentage of request traces that should sampled.
///
/// Sampling config is a JSON object with the following format:
/// {
///     instanceOverrides: {
///         instanceName1: [{routeRegexp1: fraction1}, {routeRegexp2:
/// fraction2}, ...],         instanceName2: [{routeRegexp1: fraction1},
/// {routeRegexp2: fraction2}, ...],         ...
///     },
///     routeOverrides: [{routeRegexp: fraction}, ...],
///     defaultFraction: fraction
/// }
///
/// (see also the doc comment on SamplingConfigJson).
///
/// These apply in order -- the instance overrides take precedence over route
/// overrides, which take precedence over the default fraction.
///
/// When in doubt, write out a test case to verify the behavior in
/// fastrace_helpers.rs.
///
/// It's often easiest to craft the JSON in a repl (e.g. node, browser console),
/// and then stringify it at the end.
///
/// See `fastrace_helpers.rs` for more examples
pub static REQUEST_TRACE_SAMPLE_CONFIG: LazyLock<SamplingConfig> = LazyLock::new(|| {
    env_config(
        "REQUEST_TRACE_SAMPLE_CONFIG",
        prod_override(
            SamplingConfig::default(),
            r#"{"defaultFraction":0.00001,"routeOverrides":[{"routeRegexp":"/api/push_config","fraction":0.1}, {"routeRegexp":"conductor/load-instance","fraction":0.01}, {"routeRegexp":"usage_tracking_worker/send_usage","fraction":0.01}, {"routeRegexp":"commit","fraction":0.01}]}"#
                .parse()
                .unwrap(),
        ),
    )
});

/// Size of the cache for access token authentication
pub static AUTH_CACHE_SIZE: LazyLock<u64> = LazyLock::new(|| env_config("AUTH_CACHE_SIZE", 1000));

/// Length of time an entry to the access authentication cache is valid
pub static AUTH_CACHE_TTL_SECONDS: LazyLock<u64> =
    LazyLock::new(|| env_config("AUTH_CACHE_TTL_SECONDS", 30));

/// Request body limit for airbyte streaming import requests
pub static AIRBYTE_STREAMING_IMPORT_REQUEST_SIZE_LIMIT: LazyLock<usize> = LazyLock::new(|| {
    env_config(
        "AIRBYTE_STREAMING_IMPORT_REQUEST_SIZE_LIMIT",
        10 * (2 << 20),
    )
});

/// The maximum number of backends to keep open connections to.
pub static USHER_BACKEND_CLIENTS_CACHE_SIZE: LazyLock<u64> =
    LazyLock::new(|| env_config("USHER_BACKEND_CLIENTS_CACHE_SIZE", 5000));

/// The maximum number of concurrent streams over a single tonic channel.
/// Providing a limit helps us not run into any implementation limits.
pub static USHER_MAX_CONCURRENT_STREAMS_PER_CHANNEL: LazyLock<usize> =
    LazyLock::new(|| env_config("USHER_MAX_CONCURRENT_STREAMS_PER_CHANNEL", 500));

/// Like FUNCTION_MAX_ARGS_SIZE - but while still serialized as JSON. Needs to
/// be larger. Also needs to fit within the GRPC limits we configure with
/// MAX_BACKEND_RPC_REQUEST_SIZE,
pub static USHER_MAX_JSON_ARGS_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("USHER_MAX_JSON_ARGS_SIZE", 30 * 1024 * 1024)); // 30MB
                                                                                //
/// Usher cache for service discovery lookups (partition -> address)
pub static USHER_SERVICE_CACHE_MAX_ENTRIES: LazyLock<u64> =
    LazyLock::new(|| env_config("USHER_SERVICE_CACHE_MAX_ENTRIES", 1000));

/// Usher cache for instance -> partition lookups.
/// The five-minute idle expiry bounds idle memory, while the higher capacity
/// leaves headroom for the per-host working set without size-based churn.
pub static USHER_PARTITION_CACHE_MAX_ENTRIES: LazyLock<u64> =
    LazyLock::new(|| env_config("USHER_PARTITION_CACHE_MAX_ENTRIES", 100_000));

/// Initial backoff duration when retrying a query in the sync worker.
pub static SYNC_WORKER_QUERY_RETRY_INITIAL_BACKOFF_MS: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_millis(env_config(
        "SYNC_WORKER_QUERY_RETRY_INITIAL_BACKOFF_MS",
        500,
    ))
});

/// Maximum backoff duration when retrying a query in the sync worker.
pub static SYNC_WORKER_QUERY_RETRY_MAX_BACKOFF_SECS: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("SYNC_WORKER_QUERY_RETRY_MAX_BACKOFF_SECS", 600))
});

/// Initial backoff duration when running `update_queries` in the sync worker.
pub static SYNC_WORKER_UPDATE_QUERIES_RETRY_INITIAL_BACKOFF_MS: LazyLock<Duration> =
    LazyLock::new(|| {
        Duration::from_millis(env_config(
            "SYNC_WORKER_UPDATE_QUERIES_RETRY_INITIAL_BACKOFF_MS",
            3000,
        ))
    });

/// Maximum backoff duration when retrying `update_queries` in the sync worker.
pub static SYNC_WORKER_UPDATE_QUERIES_RETRY_MAX_BACKOFF_SECS: LazyLock<Duration> =
    LazyLock::new(|| {
        Duration::from_secs(env_config(
            "SYNC_WORKER_UPDATE_QUERIES_RETRY_MAX_BACKOFF_SECS",
            600,
        ))
    });

/// If an import is taking longer than a day, it's a problem (and our fault).
/// But the customer is probably no longer waiting so we should fail the import.
/// If an import takes more than a week, the file may be deleted from S3.
pub static MAX_IMPORT_AGE: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("MAX_IMPORT_AGE_SECONDS", 7 * 24 * 60 * 60)));

/// Maximum bytes in an imported JSONL or logical CSV line, including line
/// endings. Quoted CSV fields can span physical lines.
pub static SNAPSHOT_IMPORT_MAX_LINE_SIZE_BYTES: LazyLock<usize> =
    LazyLock::new(|| env_config("SNAPSHOT_IMPORT_MAX_LINE_SIZE_BYTES", 16 << 20));

/// Max staleness in seconds of a partition loader result before we allow
/// refreshing. If a request tries to update the partition loader and this
/// duration has not passed since the last refresh, a stale value will be used.
pub static PARTITION_LOADER_MAX_STALE_SECS: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("PARTITION_LOADER_MAX_STALE_SECS", 1)));

/// The maximum number of bytes to buffer in an multipart upload.
/// There may be stricter limits imposed by the storage provider, but this is
/// the target max size for the buffer to protect against memory exhaustion.
/// The maximum number of parts is 10000, so this imposes a max file size, which
/// defaults to 10000 * 200MiB = 2TB. When reducing this knob, make sure the
/// maximum file size can fit a snapshot export of the instance.
/// Storage uploads can run in parallel, and the buffers get cloned during
/// upload, so make sure this knob times ~8 can fit in memory.
///
/// Defaults to 200MiB.
pub static STORAGE_MAX_INTERMEDIATE_PART_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("STORAGE_MAX_INTERMEDIATE_PART_SIZE", 200 * (1 << 20)));

/// Minimum number of milliseconds a commit needs to take to send traces to
/// honeycomb.
pub static COMMIT_TRACE_THRESHOLD: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("COMMIT_TRACE_THRESHOLD", 200)));

/// How many instances a Conductor will try to simultaneously load (on startup,
/// or when it discovers new instances) Going too high means that the Conductor
/// may be unable to serve requests for already-loaded instances in a timely
/// manner, or that we may exhaust CPU on the physical host
pub static INSTANCE_LOADER_CONCURRENCY: LazyLock<usize> =
    LazyLock::new(|| env_config("INSTANCE_LOADER_CONCURRENCY", 16));

/// Whether or not to use a rate limiter when loading instances
pub static INSTANCE_LOADER_USE_RATE_LIMITER: LazyLock<bool> =
    LazyLock::new(|| env_config("INSTANCE_LOADER_USE_RATE_LIMITER", true));

/// The number of instances that can be loaded per second when the rate limiter
/// is in use. The default value of 4 means that for a Conductor with 5000
/// instances, we'd take about 20 minutes to load all instances with infinite
/// concurrency.
pub static INSTANCE_LOADER_INSTANCES_PER_SECOND: LazyLock<NonZeroU32> = LazyLock::new(|| {
    env_config(
        "INSTANCE_LOADER_INSTANCES_PER_SECOND",
        NonZeroU32::new(4).unwrap(),
    )
});

/// The max number of storage files that can be fetched concurrently during
/// export. Concurrency is also limited by `EXPORT_MAX_INFLIGHT_PREFETCH_BYTES`.
pub static EXPORT_STORAGE_GET_CONCURRENCY: LazyLock<usize> =
    LazyLock::new(|| env_config("EXPORT_STORAGE_GET_CONCURRENCY", 128).max(1));

/// The max number of bytes that can be prefetched concurrently from storage
/// files during export.
///
/// Files larger than this will be not be fetched concurrently with other files.
pub static EXPORT_MAX_INFLIGHT_PREFETCH_BYTES: LazyLock<usize> = LazyLock::new(|| {
    env_config(
        "EXPORT_MAX_INFLIGHT_PREFETCH_BYTES",
        32 * 1024 * 1024, // 32MiB
    )
    .clamp(1, u32::MAX as usize)
});

/// The page size to the table iterator in the export worker.
pub static EXPORT_WORKER_PAGE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("EXPORT_WORKER_PAGE_SIZE", 1000));

/// How often to update the client-facing progress message during export.
pub static EXPORT_PROGRESS_UPDATE_INTERVAL: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("EXPORT_PROGRESS_UPDATE_INTERVAL_SECS", 5)));

/// Whether or not a service should propagate all upstream traces or perform its
/// own sampling
pub static PROPAGATE_UPSTREAM_TRACES: LazyLock<bool> =
    LazyLock::new(|| env_config("PROPAGATE_UPSTREAM_TRACES", true));

/// The maximum allowed age of /list_snapshot's timestamp.
pub static LIST_SNAPSHOT_MAX_AGE_SECS: LazyLock<Duration> = LazyLock::new(|| {
    // /list_snapshot is only used for the initial sync of documents.
    // So in valid uses of the endpoint, this doesn’t need to be longer
    // than the maximum duration of an initial sync.
    // We restrict the caller from calling with a timestamp that’s too far in the
    // past because the implementation of the endpoint walks the entire transaction
    // log starting from now, which might be too much work on big deployments.
    Duration::from_secs(env_config("LIST_SNAPSHOT_MAX_AGE_SECS", 5 * 24 * 60 * 60))
});

/// The length of the SubscriptionsWorker's input queue.
/// If the worker starts to lag then subscriptions will fail once this many
/// messages are unread.
pub static SUBSCRIPTIONS_WORKER_QUEUE_SIZE: LazyLock<usize> =
    LazyLock::new(|| env_config("SUBSCRIPTIONS_WORKER_QUEUE_SIZE", 10000));

/// The number of SubscriptionManager instances to run per SubscriptionsWorker.
/// Incoming subscription requests are randomly sharded across these managers
/// to distribute load and avoid overloading a single manager.
pub static NUM_SUBSCRIPTION_MANAGERS: LazyLock<usize> =
    LazyLock::new(|| env_config("NUM_SUBSCRIPTION_MANAGERS", 1));

/// Time to wait before scheduling update queries in the sync worker after a
/// search query fails because indexes are bootstrapping.
pub static SEARCH_INDEXES_UNAVAILABLE_RETRY_DELAY: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("SEARCH_INDEXES_UNAVAILABLE_RETRY_DELAY", 3)));

/// The maximum number of subscriptions that can be invalidated immediately. If
/// there are more, they will be splayed out.
pub static SUBSCRIPTION_INVALIDATION_DELAY_THRESHOLD: LazyLock<usize> =
    LazyLock::new(|| env_config("SUBSCRIPTION_INVALIDATION_DELAY_THRESHOLD", 200));

/// How much to splay subscription invalidations. More precisely, this is the
/// average number of milliseconds to wait between notifying subscriptions
/// invalidated by the same commit.
pub static SUBSCRIPTION_INVALIDATION_DELAY_MULTIPLIER: LazyLock<u64> = LazyLock::new(|| {
    env_config(
        "SUBSCRIPTION_INVALIDATION_DELAY_MULTIPLIER",
        5, /* ms */
    )
});

/// When advancing the write log takes longer than this amount, log extra
/// details.
pub static SUBSCRIPTION_ADVANCE_LOG_TRACING_THRESHOLD: LazyLock<u64> =
    LazyLock::new(|| env_config("SUBSCRIPTION_ADVANCE_LOG_TRACING_THRESHOLD", 10));

/// How many concurrent index backfill threads to run concurrently.
pub static INDEX_BACKFILL_CONCURRENCY: LazyLock<usize> =
    LazyLock::new(|| env_config("INDEX_BACKFILL_CONCURRENCY", 8));

/// The max size of the global HTTP cache, in bytes. This is only used for
/// getting auth metadata right now.
pub static HTTP_CACHE_SIZE: LazyLock<u64> =
    LazyLock::new(|| env_config("HTTP_CACHE_SIZE", 16 * 1024 * 1024));

/// Maximum number of environment variables that can be stored.
pub static ENV_VAR_LIMIT: LazyLock<usize> = LazyLock::new(|| env_config("ENV_VAR_LIMIT", 512));

/// Maximum total size in bytes of all environment variables (names + values).
pub static ENV_VAR_TOTAL_SIZE_LIMIT: LazyLock<usize> =
    LazyLock::new(|| env_config("ENV_VAR_TOTAL_SIZE_LIMIT", 512 * 1024));

/// If set, disable the /metrics endpoint
pub static DISABLE_METRICS_ENDPOINT: LazyLock<bool> =
    LazyLock::new(|| env_config("DISABLE_METRICS_ENDPOINT", false));

/// If set, skip stripping PII from errors before they are reported. Useful for
/// local debugging where the operator wants to see the original error contents.
pub static SHOW_PII_IN_ERRORS: LazyLock<bool> =
    LazyLock::new(|| env_config("SHOW_PII_IN_ERRORS", false));

/// Maximum size in bytes of the SharedIndexCache
/// Default 512 MiB
pub static INDEX_CACHE_SIZE: LazyLock<u64> =
    LazyLock::new(|| env_config("INDEX_CACHE_SIZE", 512 * 1024 * 1024));

/// Timeout on sending a message to the committer. Commits will fail if this
/// timeout elapses and the commit queue is still full.
pub static SEND_COMMIT_MESSAGE_TIMEOUT_MILLIS: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_millis(env_config("SEND_COMMIT_MESSAGE_TIMEOUT_MILLIS", 500)));

/// Admin identities expire after this delay. Then they need revalidation. This
/// needs to be longer than the longest living sessions that maintain a single
/// Identity. Notably V8_ACTION_USER_TIMEOUT and NODE_ACTION_USER_TIMEOUT.
pub static ADMIN_IDENTITY_EXPIRATION_DELAY: LazyLock<Duration> = LazyLock::new(|| {
    max(
        *V8_ACTION_USER_TIMEOUT,
        Duration::from_secs(env_config("ADMIN_IDENTITY_EXPIRATION_DELAY_SECS", 2000)),
    )
});

/// Admin identities should consider revalidating after this delay. This number
/// must be smaller than ADMIN_IDENTITY_EXPIRATION_DELAY
pub static ADMIN_IDENTITY_REVALIDATION_DELAY: LazyLock<Duration> = LazyLock::new(|| {
    min(
        *ADMIN_IDENTITY_EXPIRATION_DELAY,
        Duration::from_secs(env_config("ADMIN_IDENTITY_REVALIDATION_DELAY_SECS", 60)),
    )
});

/// If set, 404 on a bad path request instead of 200 + js error message
///
/// Knob defaults to false -> in an emergency we can use it to differentiate
/// 404 traffic (eg if a customer is getting hammered).
///
/// Long term, we want to make this the default, but it is tricky, because we
/// need to reconcile the behavior on batch HTTP endpoints and on websocket.
pub static UDF_404_ON_BAD_PATH: LazyLock<bool> =
    LazyLock::new(|| env_config("UDF_404_ON_BAD_PATH", false));

/// Percentage of index page queries to send to the database to verify that
/// cache results match.
pub static INDEX_CACHE_VERIFY_PERCENT: LazyLock<u8> =
    LazyLock::new(|| env_config("INDEX_CACHE_VERIFY_PERCENT", 100));

/// Initial backoff for retrying failed persistence writes.
pub static INITIAL_PERSISTENCE_WRITES_BACKOFF: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_millis(env_config("INITIAL_PERSISTENCE_WRITES_BACKOFF_MS", 100))
});

/// Max backoff for retrying failed persistence writes.
pub static MAX_PERSISTENCE_WRITES_BACKOFF: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_millis(env_config("MAX_PERSISTENCE_WRITES_BACKOFF_MS", 10 * 1000))
});

/// HTTP/2 keepalive PING interval for proxied reqwest clients. Without
/// keepalive a connection the peer drops without a TCP FIN/RST (e.g. a
/// serverless receiver cycling a container mid-request) is awaited until the
/// request or kernel TCP timeout (~16 min), stalling a sequential consumer like
/// the webhook log sink. 15s interval/timeout matches usher. No-op on HTTP/1.1.
pub static HTTP2_CLIENT_KEEPALIVE_INTERVAL: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("HTTP2_CLIENT_KEEPALIVE_INTERVAL_SECONDS", 15))
});

/// Timeout for an unacked HTTP/2 keepalive PING before the connection is
/// dropped.
pub static HTTP2_CLIENT_KEEPALIVE_TIMEOUT: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("HTTP2_CLIENT_KEEPALIVE_TIMEOUT_SECONDS", 15)));

/// Longest lifetime `ctx.auth.signJwt` may give a token.
pub static DEPLOYMENT_JWT_MAX_LIFETIME: LazyLock<Duration> = LazyLock::new(|| {
    Duration::from_secs(env_config("DEPLOYMENT_JWT_MAX_LIFETIME_SECONDS", 60 * 60))
});

/// How long verifiers may cache the deployment JWKS served at
/// `/.well-known/jwks.json`.
pub static DEPLOYMENT_JWKS_MAX_AGE: LazyLock<Duration> =
    LazyLock::new(|| Duration::from_secs(env_config("DEPLOYMENT_JWKS_MAX_AGE_SECONDS", 300)));

/// Max number of log lines in a single V8 action
pub static MAX_LOG_LINES_V8_ACTIONS: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_LOG_LINES_V8_ACTIONS", 1024).max(1));

/// Max number of log lines in a single Node action. Sent to the Node executor
/// with each request.
pub static MAX_LOG_LINES_NODE_ACTIONS: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_LOG_LINES_NODE_ACTIONS", 1024).max(1));

/// Max number of log lines in a single query or mutation
pub static MAX_LOG_LINES_DATABASE_UDFS: LazyLock<usize> =
    LazyLock::new(|| env_config("MAX_LOG_LINES_DATABASE_UDFS", 256).max(1));
