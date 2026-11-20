//! The recording path: mapping usage events to per-metric deltas and the
//! [`UsageEventLogger`] decorator that feeds them into the meter.

use std::sync::Arc;

use async_trait::async_trait;
use common::{
    runtime::Runtime,
    types::ModuleEnvironment,
};
use events::usage::{
    UsageEvent,
    UsageEventLogger,
};
use model::usage_limits::types::UsageLimitMetric;

use super::meter::UsageMeter;

/// Per-metric usage deltas (raw units: calls, bytes, GB, GB·s) derived from a
/// batch of usage events, mirroring the pipeline rollups that
/// [`UsageLimitMetric::from_seed_metric`] maps.
pub fn usage_deltas(events: &[UsageEvent]) -> Vec<(UsageLimitMetric, f64)> {
    fn gb_s(compute_mb_ms: u64) -> f64 {
        compute_mb_ms as f64 / 1024.0 / 1000.0
    }
    let mut deltas = Vec::new();
    for event in events {
        // Match events exhaustively so a new one is a compile error.
        match event {
            UsageEvent::FunctionCall { fields } => {
                if fields.is_tracked {
                    deltas.push((UsageLimitMetric::FunctionCalls, fields.calls as f64));
                }
                // Compute in GB·s; the GB-hour limit converts to GB·s at
                // evaluation via `limit_in_raw_units`. Zero for calls not
                // charged for compute, e.g. cache hits.
                let compute = gb_s(fields.compute_mb_ms);
                match fields.tag.as_str() {
                    "action" | "http_action" => {
                        // Match the runtime exhaustively so a new one is a
                        // compile error.
                        match fields.environment.parse::<ModuleEnvironment>() {
                            Ok(ModuleEnvironment::Node | ModuleEnvironment::Rust) => {
                                deltas
                                    .push((UsageLimitMetric::ActionComputeNodeJsGBHours, compute));
                            },
                            Ok(ModuleEnvironment::Isolate) => {
                                deltas
                                    .push((UsageLimitMetric::ActionComputeConvexGBHours, compute));
                                // CPU compute (user-execution time, excluding
                                // time blocked on I/O) is charged for the
                                // Convex runtime only.
                                if let Some(user_mb_ms) = fields.user_compute_mb_ms {
                                    deltas.push((
                                        UsageLimitMetric::ActionComputeCpuGBHours,
                                        gb_s(user_mb_ms),
                                    ));
                                }
                            },
                            Ok(ModuleEnvironment::Invalid) | Err(_) => {},
                        }
                    },
                    // The usage pipeline groups everything that isn't an action
                    // as query/mutation compute.
                    _ => deltas.push((UsageLimitMetric::QueryMutationComputeGBHours, compute)),
                }
            },
            // Storage calls bill as function calls: the seed pipeline's
            // `udf_storage_calls` and `storage_calls` rollups fold into the
            // `function_calls` bucket.
            UsageEvent::FunctionStorageCalls { count, .. } => {
                deltas.push((UsageLimitMetric::FunctionCalls, *count as f64));
            },
            UsageEvent::StorageCall { count, .. } => {
                deltas.push((UsageLimitMetric::FunctionCalls, *count as f64));
            },
            UsageEvent::DatabaseBandwidth {
                ingress_v2,
                egress_v2,
                ..
            } => {
                deltas.push((
                    UsageLimitMetric::DatabaseIoGB,
                    ingress_v2.saturating_add(*egress_v2) as f64,
                ));
            },
            // Data egress counts network egress plus storage-layer egress,
            // matching the pipeline's `network_egress`,
            // `storage_bandwidth_egress`, and `udf_storage_bandwidth_egress`
            // rollups.
            UsageEvent::NetworkBandwidth { egress, .. }
            | UsageEvent::StorageBandwidth { egress, .. }
            | UsageEvent::FunctionStorageBandwidth { egress, .. } => {
                deltas.push((UsageLimitMetric::DataEgressGB, *egress as f64));
            },
            // Search usage records in GB, matching the search rollups.
            UsageEvent::TextQuery { bytes_searched, .. }
            | UsageEvent::VectorQuery { bytes_searched, .. } => {
                const BYTES_PER_GB: f64 = (1u64 << 30) as f64;
                deltas.push((
                    UsageLimitMetric::SearchQueryGB,
                    *bytes_searched as f64 / BYTES_PER_GB,
                ));
            },
            // Index bandwidth and audit-log egress are outside the metered
            // limit metrics, and read-limit insights are diagnostics.
            UsageEvent::VectorBandwidth { .. }
            | UsageEvent::TextWrites { .. }
            | UsageEvent::AuditLogBandwidth { .. }
            | UsageEvent::InsightReadLimit { .. } => {},
            // Current* events are point-in-time storage gauges; the metered
            // limit metrics count usage deltas.
            UsageEvent::CurrentVectorStorage { .. }
            | UsageEvent::CurrentTextStorage { .. }
            | UsageEvent::CurrentDatabaseStorage { .. }
            | UsageEvent::CurrentFileStorage { .. }
            | UsageEvent::CurrentDocumentCounts { .. }
            | UsageEvent::CurrentBackendState { .. } => {},
        }
    }
    deltas
}

/// [`UsageEventLogger`] decorator that records usage-limit deltas from the
/// usage-event stream before forwarding it. All billable usage flows through
/// this logger, so enforcement counts the same usage as billing.
pub struct UsageLimitRecorder<RT: Runtime> {
    rt: RT,
    meter: Arc<UsageMeter>,
    inner: Arc<dyn UsageEventLogger>,
}

impl<RT: Runtime> UsageLimitRecorder<RT> {
    pub fn new(rt: RT, meter: Arc<UsageMeter>, inner: Arc<dyn UsageEventLogger>) -> Arc<Self> {
        Arc::new(Self { rt, meter, inner })
    }
}

impl<RT: Runtime> std::fmt::Debug for UsageLimitRecorder<RT> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsageLimitRecorder").finish_non_exhaustive()
    }
}

#[async_trait]
impl<RT: Runtime> UsageEventLogger for UsageLimitRecorder<RT> {
    async fn record_async(&self, events: Vec<UsageEvent>) {
        let deltas = usage_deltas(&events);
        if !deltas.is_empty() {
            self.meter.record(self.rt.system_time(), &deltas);
        }
        self.inner.record_async(events).await
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        self.inner.shutdown().await
    }
}
