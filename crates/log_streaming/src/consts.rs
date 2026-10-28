use std::time::Duration;

// LogManager
pub const MANAGER_INITIAL_BACKOFF: Duration = Duration::from_secs(1);
pub const MANAGER_MAX_BACKOFF: Duration = Duration::from_secs(30);
pub const SINK_STARTUP_TIMEOUT: Duration = Duration::from_secs(15);

// MockSink
// LocalSink
pub const LOCAL_SINK_EVENTS_BUFFER_SIZE: usize = 50;
pub const LOCAL_SINK_INITIAL_BACKOFF: Duration = Duration::from_secs(1);
pub const LOCAL_SINK_MAX_BACKOFF: Duration = Duration::from_secs(10);

// Datadog
pub const DD_SINK_EVENTS_BUFFER_SIZE: usize = 8;
pub const DD_SINK_INITIAL_BACKOFF: Duration = Duration::from_millis(500);
pub const DD_SINK_MAX_BACKOFF: Duration = Duration::from_secs(60);
pub const DD_SINK_MAX_REQUEST_ATTEMPTS: usize = 6;
/// Datadog's logs intake API accepts at most 1000 array entries per payload.
/// https://docs.datadoghq.com/api/latest/logs/#send-logs
pub const DD_SINK_MAX_LOGS_PER_BATCH: usize = 1000;
/// Headroom under the intake API's 5MB uncompressed payload limit.
pub const DD_SINK_MAX_BATCH_BYTES: usize = 4 << 20;

// Axiom
pub const AXIOM_SINK_EVENTS_BUFFER_SIZE: usize = 8;
pub const AXIOM_SINK_INITIAL_BACKOFF: Duration = Duration::from_millis(500);
pub const AXIOM_SINK_MAX_BACKOFF: Duration = Duration::from_secs(60);
pub const AXIOM_SINK_MAX_REQUEST_ATTEMPTS: usize = 6;
/// This is Axiom's hard limit: https://axiom.co/docs/send-data/ingest#limits
/// In practice, this is impossible to hit in one batch since the LogManager
/// aggregation recv buffer size (LOG_MANAGER_EVENT_RECV_BUFFER_SIZE) is
/// controlled by a knob which, by default, is much less than this.
pub const AXIOM_SINK_MAX_LOGS_PER_BATCH: usize = 10000;

// Webhook
pub const WEBHOOK_SINK_EVENTS_BUFFER_SIZE: usize = 8;
pub const WEBHOOK_SINK_INITIAL_BACKOFF: Duration = Duration::from_secs(1);
pub const WEBHOOK_SINK_MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Bound on a single request attempt. Without it, a connection the remote end
/// drops without a FIN/RST hangs the sink until the kernel TCP retransmission
/// timeout (~16 minutes), during which the events buffer overflows and logs
/// are dropped.
pub const WEBHOOK_SINK_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const WEBHOOK_SINK_MAX_REQUEST_ATTEMPTS: usize = 6;
pub const WEBHOOK_SINK_VERIFICATION_MAX_ATTEMPTS: usize = 3;
pub const WEBHOOK_SINK_MAX_LOGS_PER_BATCH: usize = 128;

// Sentry
pub const SENTRY_SINK_EVENTS_BUFFER_SIZE: usize = 8;
pub const SENTRY_SINK_INITIAL_BACKOFF: Duration = Duration::from_millis(500);
pub const SENTRY_SINK_MAX_BACKOFF: Duration = Duration::from_secs(60);
pub const SENTRY_SINK_MAX_LOGS_PER_BATCH: usize = 100;

// PostHog Logs
pub const POSTHOG_LOGS_SINK_EVENTS_BUFFER_SIZE: usize = 8;
pub const POSTHOG_LOGS_SINK_INITIAL_BACKOFF: Duration = Duration::from_millis(500);
pub const POSTHOG_LOGS_SINK_MAX_BACKOFF: Duration = Duration::from_secs(60);
pub const POSTHOG_LOGS_SINK_MAX_REQUEST_ATTEMPTS: usize = 6;
/// ~1.6MB at 4KB/log, under OTLP 2MB limit
pub const POSTHOG_LOGS_SINK_MAX_LOGS_PER_BATCH: usize = 400;

// PostHog Error Tracking
pub const POSTHOG_ET_SINK_EVENTS_BUFFER_SIZE: usize = 8;
pub const POSTHOG_ET_SINK_INITIAL_BACKOFF: Duration = Duration::from_millis(500);
pub const POSTHOG_ET_SINK_MAX_BACKOFF: Duration = Duration::from_secs(60);
pub const POSTHOG_ET_SINK_MAX_REQUEST_ATTEMPTS: usize = 6;
pub const POSTHOG_ET_SINK_MAX_LOGS_PER_BATCH: usize = 100;
