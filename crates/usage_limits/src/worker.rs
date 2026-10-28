//! The background worker that evaluates the meter against the configured
//! limits and acts on the result.

use std::{
    cmp::Ordering,
    collections::{
        HashMap,
        HashSet,
    },
    sync::Arc,
    time::{
        Duration,
        SystemTime,
    },
};

use common::{
    backoff::Backoff,
    errors::report_error,
    execution_context::RequestMetadata,
    knobs::USAGE_LIMIT_EVALUATE_INTERVAL,
    log_streaming::LogSender,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    types::UsageLimitStopState,
};
use database::Database;
use futures::{
    pin_mut,
    select_biased,
    FutureExt,
};
use keybroker::Identity;
use model::{
    backend_state::BackendStateModel,
    deployment_audit_log::{
        types::{
            DeploymentAuditLogEvent,
            DeploymentAuditLogEventKind,
        },
        DeploymentAuditLogModel,
    },
    usage_limits::{
        types::{
            UsageLimitConfig,
            UsageLimitWindow,
        },
        UsageLimitsModel,
    },
};
use value::{
    DeveloperDocumentId,
    ResolvedDocumentId,
};

use super::{
    meter::{
        ExceededUsageLimit,
        UsageMeter,
    },
    notifier::{
        UsageLimitNotification,
        UsageLimitNotifier,
    },
    stores::window_range,
};

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// The high-water mark of usage-limit reporting for a single limit
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct UsageReportingWatermark {
    window_start: SystemTime,
    highest_reported: u64,
}

/// Background worker that evaluates recorded usage against the configured
/// limits: it sets the deployment's usage-limit stop state when a `Disable`
/// limit is crossed or cleared, records a `UsageLimitExceeded` audit event
/// once per limit per window, and a `ChangeUsageLimitStopState` event on
/// each disable/re-enable transition. It loads the limit configs and
/// subscribes to `_usage_limits`, reloading whenever the table changes.
pub struct UsageLimitWorker<RT: Runtime> {
    rt: RT,
    database: Database<RT>,
    log_manager_client: Arc<dyn LogSender>,
    /// Sink for newly-exceeded limits (e.g. the postalservice webhook in
    /// Convex Cloud). Fire-and-forget, so a slow or failing notifier never
    /// stalls evaluation or blocks a limit from being marked reported.
    notifier: Arc<dyn UsageLimitNotifier>,
    meter: Arc<UsageMeter>,
    /// The reporting watermark for each limit, so we emit one
    /// `UsageLimitExceeded` audit event per meaningful crossing. Within a
    /// window, usage only ever increases (every metric is a cumulative
    /// counter), so exceeding a limit implies every smaller limit is exceeded
    /// too. Reporting only moves forward: we re-report on a later window, or on
    /// a higher crossed value within the same window. See
    /// [`Self::mark_reported`].
    reported: HashMap<DeveloperDocumentId, UsageReportingWatermark>,
    /// Whether `reported` has been rehydrated from the audit log this
    /// process lifetime. Rehydration happens lazily, on the first evaluation
    /// that finds an exceeded limit, and keeps the once-per-limit-per-window
    /// deduplication durable across restarts.
    primed: bool,
}

impl<RT: Runtime> UsageLimitWorker<RT> {
    pub async fn start(
        rt: RT,
        database: Database<RT>,
        log_manager_client: Arc<dyn LogSender>,
        notifier: Arc<dyn UsageLimitNotifier>,
        meter: Arc<UsageMeter>,
    ) {
        tracing::info!("Starting UsageLimitWorker");
        let mut worker = Self {
            rt,
            database,
            log_manager_client,
            notifier,
            meter,
            reported: HashMap::new(),
            primed: false,
        };
        let mut backoff = Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF);
        loop {
            match worker.run().await {
                Ok(()) => backoff.reset(),
                Err(mut e) => {
                    report_error(&mut e).await;
                    let delay = backoff.fail(&mut worker.rt.rng());
                    worker.rt.wait(delay).await;
                },
            }
        }
    }

    /// Load the limit configs, subscribe to their table, and evaluate on a
    /// fixed interval; returns when the configs change so the caller
    /// reloads.
    async fn run(&mut self) -> anyhow::Result<()> {
        let mut tx = self.database.begin(Identity::system()).await?;
        let configs: Vec<(ResolvedDocumentId, UsageLimitConfig)> = UsageLimitsModel::new(&mut tx)
            .list()
            .await?
            .into_iter()
            .map(|config| {
                let id = config.id();
                (id, config.into_value())
            })
            .collect();
        let token = tx.into_token()?;
        self.prune_reported(&configs);
        self.meter.refresh_configs(configs);
        let database = self.database.clone();
        let invalidated = database.subscribe_and_wait_for_invalidation(token).fuse();
        pin_mut!(invalidated);
        loop {
            self.evaluate_once().await?;
            select_biased! {
                result = invalidated => {
                    result?;
                    return Ok(());
                },
                _ = self.rt.wait(*USAGE_LIMIT_EVALUATE_INTERVAL).fuse() => {},
            }
        }
    }

    /// Record that `id`'s limit was reported at `limit` in the window starting
    /// at `window_start`. Reporting only ever moves forward: a strictly later
    /// window replaces the entry and resets its value; the same window keeps
    /// the highest value seen (usage only climbs, so the highest crossing
    /// subsumes the lower ones); an earlier window is stale and ignored. This
    /// upholds the invariant even if a caller ever marks out of order.
    fn mark_reported(&mut self, id: DeveloperDocumentId, window_start: SystemTime, limit: u64) {
        self.reported
            .entry(id)
            .and_modify(|w| match window_start.cmp(&w.window_start) {
                Ordering::Greater => {
                    w.window_start = window_start;
                    w.highest_reported = limit;
                },
                Ordering::Equal => w.highest_reported = w.highest_reported.max(limit),
                Ordering::Less => {},
            })
            .or_insert(UsageReportingWatermark {
                window_start,
                highest_reported: limit,
            });
    }

    /// Drop reporting watermarks for limits absent from `configs`.
    /// A limit recreated later gets a fresh id, so its watermark starts clean.
    fn prune_reported(&mut self, configs: &[(ResolvedDocumentId, UsageLimitConfig)]) {
        let live: HashSet<DeveloperDocumentId> = configs
            .iter()
            .map(|(id, _)| DeveloperDocumentId::from(*id))
            .collect();
        self.reported.retain(|id, _| live.contains(id));
    }

    async fn evaluate_once(&mut self) -> anyhow::Result<()> {
        let now = self.rt.system_time();
        let evaluation = self.meter.evaluate(now)?;

        // Limits at or over threshold that haven't been reported in their
        // current window yet. Prime the dedup map from the audit log on the
        // first report this process lifetime so a restart doesn't re-emit.
        let unreported = |reported: &HashMap<DeveloperDocumentId, UsageReportingWatermark>,
                          e: &ExceededUsageLimit| {
            match reported.get(&DeveloperDocumentId::from(e.id)) {
                // Report only on a forward crossing: a later window, or the
                // same window at a higher value. An earlier window is stale.
                Some(w) => match e.window_start.cmp(&w.window_start) {
                    Ordering::Greater => true,
                    Ordering::Equal => e.config.limit > w.highest_reported,
                    Ordering::Less => false,
                },
                None => true,
            }
        };
        let mut newly_exceeded: Vec<ExceededUsageLimit> = evaluation
            .exceeded
            .into_iter()
            .filter(|e| unreported(&self.reported, e))
            .collect();
        if !newly_exceeded.is_empty() && !self.primed {
            self.prime_reported(now).await?;
            self.primed = true;
            newly_exceeded.retain(|e| unreported(&self.reported, e));
        }

        // Apply the desired stop state and record any audit events in one
        // transaction. `set_usage_limit_stop_state` returns the prior state
        // only when it actually changed, so the deployment's persisted state
        // is the source of truth for enable/disable transitions — restart
        // safe without extra in-memory tracking.
        let mut tx = self.database.begin(Identity::system()).await?;
        let previous = BackendStateModel::new(&mut tx)
            .set_usage_limit_stop_state(evaluation.desired_stop_state)
            .await?;

        let mut events: Vec<DeploymentAuditLogEvent> = newly_exceeded
            .iter()
            .map(|exceeded| DeploymentAuditLogEvent::UsageLimitExceeded {
                id: String::from(DeveloperDocumentId::from(exceeded.id)),
                config: exceeded.config.clone(),
            })
            .collect();
        // `previous` is `Some` only on a real change: record the transition
        // alongside the per-limit exceeded events.
        if let Some(prev) = previous {
            let old_state = prev.usage_limit;
            let new_state = evaluation.desired_stop_state;
            match new_state {
                UsageLimitStopState::Disabled => {
                    tracing::warn!("Usage limit exceeded: deployment disabled")
                },
                UsageLimitStopState::None => {
                    tracing::info!("Usage back under all limits: deployment re-enabled")
                },
            }
            events.push(DeploymentAuditLogEvent::ChangeUsageLimitStopState {
                old_state,
                new_state,
            });
        }

        // No state change and nothing new to report: drop the read-only tx.
        if events.is_empty() {
            return Ok(());
        }

        DeploymentAuditLogModel::new(&mut tx)
            .insert(events.clone(), &RequestMetadata::system())
            .await?;
        // `commit_with_audit_log_events` isn't used here because the limits
        // are marked reported between this commit and streaming the logs
        // below.
        let ts = self
            .database
            .commit_with_write_source(tx, "usage_limit_enforcement")
            .await?;
        // Mark reported once the audit events are durable, before the
        // best-effort log streaming below, so a later failure doesn't
        // re-commit the same events on the next evaluation.
        for exceeded in &newly_exceeded {
            self.mark_reported(
                DeveloperDocumentId::from(exceeded.id),
                exceeded.window_start,
                exceeded.config.limit,
            );
        }
        let notifications = newly_exceeded
            .iter()
            .map(|exceeded| {
                Ok(UsageLimitNotification {
                    metric: exceeded.config.metric,
                    window: exceeded.config.window,
                    limit_type: exceeded.config.limit_type,
                    limit: exceeded.config.limit,
                    window_reset: window_range(exceeded.config.window, now)?.end,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        self.notifier.notify_exceeded(notifications);
        let logs = events
            .into_iter()
            .map(|event| {
                DeploymentAuditLogEvent::to_log_event(event, UnixTimestamp::from_nanos(ts.into()))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        self.log_manager_client.send_logs(logs);
        Ok(())
    }

    /// Rebuild `reported` from past `UsageLimitExceeded` audit events.
    ///
    /// Scans from the start of the current calendar month — the widest
    /// supported window, which bounds every window that could still need
    /// deduplication. Each event marks its limit reported in the window
    /// containing the event, computed from the event's own config and
    /// timestamp, so deduplication applies exactly to windows still in
    /// progress.
    async fn prime_reported(&mut self, now: SystemTime) -> anyhow::Result<()> {
        let from_ts_ms = window_range(UsageLimitWindow::Month, now)?
            .start
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_millis() as u64;
        const PAGE_SIZE: usize = 100;
        let mut cursor = None;
        loop {
            let mut tx = self.database.begin(Identity::system()).await?;
            let (entries, next_cursor) = DeploymentAuditLogModel::new(&mut tx)
                .list_events_from_time(
                    from_ts_ms,
                    Some(DeploymentAuditLogEventKind::UsageLimitExceeded),
                    cursor,
                    PAGE_SIZE,
                )
                .await?;
            for entry in entries {
                let DeploymentAuditLogEvent::UsageLimitExceeded { id, config } = entry.action
                else {
                    continue;
                };
                let Ok(id) = DeveloperDocumentId::decode(&id) else {
                    continue;
                };
                let event_ts =
                    SystemTime::UNIX_EPOCH + Duration::from_millis(entry.create_time as u64);
                self.mark_reported(
                    id,
                    window_range(config.window, event_ts)?.start,
                    config.limit,
                );
            }
            match next_cursor {
                Some(next_cursor) => cursor = Some(next_cursor),
                None => return Ok(()),
            }
        }
    }
}
