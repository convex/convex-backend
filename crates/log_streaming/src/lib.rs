#![feature(coroutines)]
#![feature(future_join)]
#![feature(never_type)]
pub mod consts;
mod metrics;
pub mod sinks;

use std::{
    collections::BTreeMap,
    fmt::Formatter,
    sync::{
        atomic::{
            AtomicBool,
            AtomicUsize,
            Ordering,
        },
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use common::{
    backoff::Backoff,
    document::ParsedDocument,
    errors::{
        report_error,
        report_error_sync,
    },
    http::fetch::FetchClient,
    knobs,
    log_streaming::{
        LogEvent,
        LogSender,
        LogTopic,
        StructuredLogEvent,
    },
    runtime::{
        shutdown_and_join,
        Runtime,
        SpawnHandle,
        TimeoutError,
        WithTimeout,
    },
    types::DeploymentType,
};
use database::{
    Database,
    SystemMetadataModel,
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use futures::{
    pin_mut,
    select_biased,
    FutureExt,
};
use keybroker::Identity;
use model::{
    backend_info::BackendInfoModel,
    log_sinks::{
        types::{
            LogSinksRow,
            SinkConfig,
            SinkState,
            SinkType,
        },
        LogSinksModel,
    },
};
use parking_lot::{
    Mutex,
    RwLock,
};
use sentry::TransportFactory;
use serde::Serialize;
use sinks::{
    local_sink::LocalSink,
    posthog_error_tracking::PostHogErrorTrackingSink,
    posthog_logs::PostHogLogsSink,
    sentry::SentrySink,
};
use tokio::sync::mpsc;
use usage_tracking::UsageCounter;

use crate::sinks::{
    axiom::AxiomSink,
    datadog::DatadogSink,
    failure::{
        SinkEgressFailure,
        SinkFailureReporter,
    },
    utils::EgressCounter,
    webhook::WebhookSink,
};

/// Public worker for the LogManager.
///
/// See `log_streaming/README.md` for more info.
#[derive(Clone)]
pub struct LogManagerClient {
    handle: Arc<Mutex<Option<Box<dyn SpawnHandle>>>>,
    event_sender: mpsc::Sender<Arc<LogEvent>>,
    active_sinks_count: Arc<AtomicUsize>,
    entitlement_enabled: Arc<AtomicBool>,
}

impl LogManagerClient {
    fn send_logs_inner(&self, logs: Vec<LogEvent>) -> Result<(), SendLogBatchError> {
        let mut dropped = 0;
        let mut num_left = logs.len();
        let total = logs.len();
        for log in logs.into_iter() {
            match self.event_sender.try_send(Arc::new(log)) {
                Err(mpsc::error::TrySendError::Full(_)) => {
                    dropped += 1;
                },
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    return Err(SendLogBatchError {
                        dropped,
                        total,
                        skipped_from_disconnect: Some(num_left),
                    });
                },
                Ok(()) => {},
            }
            num_left -= 1;
        }
        if dropped > 0 {
            Err(SendLogBatchError {
                dropped,
                total,
                skipped_from_disconnect: None,
            })
        } else {
            Ok(())
        }
    }

    pub fn set_entitlement_enabled(&self, enabled: bool) {
        self.entitlement_enabled.store(enabled, Ordering::Relaxed)
    }

    fn get_active_sinks_count(&self) -> usize {
        self.active_sinks_count.load(Ordering::Relaxed)
    }

    pub fn is_entitlement_enabled(&self) -> bool {
        self.entitlement_enabled.load(Ordering::Relaxed)
    }

    fn is_active(&self) -> bool {
        self.is_entitlement_enabled() && self.get_active_sinks_count() > 0
    }
}

#[async_trait]
impl LogSender for LogManagerClient {
    async fn shutdown(&self) -> anyhow::Result<()> {
        let handle = self.handle.lock().take();
        if let Some(handle) = handle {
            shutdown_and_join(handle).await?;
        }
        Ok(())
    }

    fn send_logs(&self, logs: Vec<LogEvent>) {
        // Only route to an active log sink
        if !self.is_active() {
            return;
        }
        let total = logs.len();
        let result = self.send_logs_inner(logs);
        if let Err(e) = result {
            if let Some(skipped) = e.skipped_from_disconnect {
                metrics::log_event_dropped_disconnected_error(skipped);
                // Overflows are common so don't pollute Sentry unless something actually bad
                // happened, i.e. the LogManager events chain disconnected.
                report_error_sync(
                    &mut anyhow::Error::from(e.clone())
                        .context(ErrorMetadata::operational_internal_server_error()),
                );
                tracing::error!("Log event(s) skipped from disconnect: {e}");
            }
            metrics::log_event_dropped_overflow_error(e.dropped);
        }

        metrics::log_event_total(total);
    }
}

#[derive(Debug, Clone)]
pub struct SendLogBatchError {
    pub dropped: usize,
    pub total: usize,
    pub skipped_from_disconnect: Option<usize>, // Some iff channel disconnected
}

impl std::error::Error for SendLogBatchError {}

impl std::fmt::Display for SendLogBatchError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(num_skipped) = self.skipped_from_disconnect {
            write!(
                f,
                "Dropped {:}/{:} logs due to overflow. Skipped {:}/{:} due to disconnect.",
                self.dropped, self.total, num_skipped, self.total
            )
        } else {
            write!(
                f,
                "Dropped {:}/{:} logs due to overflow.",
                self.dropped, self.total
            )
        }
    }
}

/// The central abstraction for handling log events. This is responsible for
/// forwarding logs to LogSinks. Ideally, any shared batching/aggregation logic
/// is done here to deduplicate work at the LogSinks.
///
/// This is also responsible for selective routing of LogEvents. In the future
/// selective routing could be a feature we expose to customers but, for now, we
/// just use it for selectively filtering internal logs.
pub struct LogManager<RT: Runtime> {
    runtime: RT,
    database: Database<RT>,
    fetch_client: Arc<dyn FetchClient>,
    sentry_transport: Arc<dyn TransportFactory>,
    sinks: Arc<RwLock<BTreeMap<SinkType, LogSinkClient>>>,
    event_receiver: mpsc::Receiver<Arc<LogEvent>>,
    /// Cloned sender for feeding log events back into the pipeline (e.g.,
    /// LogStreamEgress events emitted by sinks).
    event_sender: mpsc::Sender<Arc<LogEvent>>,
    instance_name: String,
    deployment_region: Option<String>,
    /// How many sinks are active right now?
    active_sinks_count: Arc<AtomicUsize>,
    usage_counter: usage_tracking::UsageCounter,
    egress_counter: EgressCounter,
}

#[derive(Serialize, Debug, Clone)]
pub struct LoggingDeploymentMetadata {
    deployment_name: String,
    deployment_type: Option<DeploymentType>,
    deployment_ref: Option<String>,
    project_name: Option<String>,
    project_slug: Option<String>,
    deployment_region: Option<String>,
}

impl<RT: Runtime> LogManager<RT> {
    pub async fn start(
        runtime: RT,
        database: Database<RT>,
        fetch_client: Arc<dyn FetchClient>,
        sentry_transport: Arc<dyn TransportFactory>,
        instance_name: String,
        deployment_region: Option<String>,
        entitlement_enabled: bool,
        usage_counter: UsageCounter,
    ) -> LogManagerClient {
        let (req_tx, req_rx) = mpsc::channel(*knobs::LOG_MANAGER_EVENT_RECV_BUFFER_SIZE);

        let active_sinks_count = Arc::new(AtomicUsize::new(0));
        let egress_counter = EgressCounter::default();
        let worker = Self {
            runtime: runtime.clone(),
            database,
            fetch_client,
            sentry_transport,
            // Sinks are populated from the `_log_sinks` system table on startup
            sinks: Arc::new(RwLock::new(BTreeMap::new())),
            event_receiver: req_rx,
            event_sender: req_tx.clone(),
            instance_name,
            deployment_region,
            active_sinks_count: active_sinks_count.clone(),
            usage_counter,
            egress_counter,
        };

        let handle = Arc::new(Mutex::new(Some(runtime.spawn("log_manager", worker.go()))));
        let entitlement_enabled = Arc::new(AtomicBool::new(entitlement_enabled));
        LogManagerClient {
            handle,
            event_sender: req_tx,
            active_sinks_count,
            entitlement_enabled,
        }
    }

    /// Event loop
    async fn go(mut self) {
        tracing::info!("Starting LogManager");
        let mut backoff =
            Backoff::new(consts::MANAGER_INITIAL_BACKOFF, consts::MANAGER_MAX_BACKOFF);

        // Start listening to logs
        loop {
            let Err(mut e) = self.listen().await;
            let delay = backoff.fail(&mut self.runtime.rng());
            tracing::error!("LogManager failed, sleeping {delay:?}");
            report_error(&mut e).await;
            self.runtime.wait(delay).await;
        }
    }

    async fn listen(&mut self) -> anyhow::Result<!> {
        tracing::info!("Starting listening for logs");
        let sink_startup_worker_fut = Self::sink_startup_worker(
            &self.runtime,
            &self.database,
            self.fetch_client.clone(),
            self.sentry_transport.clone(),
            &self.sinks,
            self.instance_name.clone(),
            self.deployment_region.clone(),
            self.active_sinks_count.clone(),
            self.egress_counter.clone(),
        )
        .fuse();
        pin_mut!(sink_startup_worker_fut);

        let egress_emission_fut = Self::egress_emission_worker(
            &self.runtime,
            &self.event_sender,
            &self.egress_counter,
            &self.usage_counter,
        )
        .fuse();
        pin_mut!(egress_emission_fut);

        let log_event_listener_fut =
            Self::log_event_listener(&self.runtime, &mut self.event_receiver, &self.sinks).fuse();
        pin_mut!(log_event_listener_fut);

        // We use select_biased! here to not starve sink_startup_worker.
        // log_event_listener will usually be of a much higher throughput.
        select_biased! {
            r = sink_startup_worker_fut => {
                r?;
            },
            r = egress_emission_fut => {
                r?;
            },
            r = log_event_listener_fut => {
                r?;
            },
        }
    }

    /// This aggregates logs by emitting logs to sinks if the receiving channel
    /// is full or if a constant aggregation interval has passed.
    ///
    /// NOTE: aggregation could happen at the sink-level or at the
    /// manager-level, but we've decided to put it here for simplicity and
    /// for slightly less overhead (no need to have timers at each sink).
    /// This can just as easily be moved to sinks if required.
    async fn log_event_listener(
        runtime: &RT,
        rx: &mut mpsc::Receiver<Arc<LogEvent>>,
        sinks: &Arc<RwLock<BTreeMap<SinkType, LogSinkClient>>>,
    ) -> anyhow::Result<!> {
        loop {
            // Accumulate events as they arrive until the batch is full or the
            // aggregation interval passes, whichever comes first.
            let mut deadline = runtime.wait(Duration::from_millis(
                *knobs::LOG_MANAGER_AGGREGATION_INTERVAL_MILLIS,
            ));
            let max_batch_size = *knobs::LOG_MANAGER_EVENT_RECV_BUFFER_SIZE;
            let mut drained_events = vec![];
            while drained_events.len() < max_batch_size {
                let limit = max_batch_size - drained_events.len();
                let recv = rx.recv_many(&mut drained_events, limit).fuse();
                pin_mut!(recv);
                select_biased! {
                    num_received = recv => {
                        if num_received == 0 {
                            anyhow::bail!("log manager receive channel closed");
                        }
                    },
                    _ = deadline => break,
                }
            }

            if !drained_events.is_empty() {
                // Route events to sinks
                metrics::log_manager_logs_received(drained_events.len());
                Self::route_event_batch(drained_events, sinks)?;
            }
        }
    }

    // Routing logic will eventually go here. For now, this is just a fanout.
    fn route_event_batch(
        events: Vec<Arc<LogEvent>>,
        sinks: &Arc<RwLock<BTreeMap<SinkType, LogSinkClient>>>,
    ) -> anyhow::Result<()> {
        let sinks = sinks.read();
        sinks
            .iter()
            .map(|(ty, v)| (ty.clone(), v.events_sender.clone(), events.clone()))
            .try_for_each(|(ty, tx, events)| {
                match tx.try_send(events) {
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        // No need to report metrics here, can calculate this drop amount by the
                        // number of logs received metric for each sink
                        tracing::info!("Sink receive buffer full for {ty:?}, dropping logs.");
                        Ok(())
                    },
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        anyhow::bail!("Sink receive buffer for {ty:?} closed unexpectedly.");
                    },
                    Ok(()) => Ok(()),
                }
            })?;

        Ok(())
    }

    /// Subscribes to LOG_SINKS_TABLE and attempts to drive forward the
    /// state machine described in `SinkState`.
    ///
    /// Specifically the following logic is implemented
    /// - A sink in `Pending` state should be started and moved to the `Active`
    ///   state. Starting a sink involves invoking its `start` method and
    ///   storing the resulting LogSinkClient in the LogManager's local sink_map
    ///   so that logs will start routing.
    /// - If startup of a `Pending` sink fails, it sets its state to `Failed`,
    ///   otherwise it is set to `Active`.
    /// - A sink that is `Active` should be started. This can be violated on
    ///   backend restart so this is handled by setting the sink state to
    ///   `Pending` again.
    /// - A sink in `Tombstoned` state should have its corresponding row removed
    async fn sink_startup_worker(
        runtime: &RT,
        database: &Database<RT>,
        fetch_client: Arc<dyn FetchClient>,
        sentry_transport: Arc<dyn TransportFactory>,
        sinks: &Arc<RwLock<BTreeMap<SinkType, LogSinkClient>>>,
        instance_name: String,
        deployment_region: Option<String>,
        active_sinks_count: Arc<AtomicUsize>,
        egress_counter: EgressCounter,
    ) -> anyhow::Result<!> {
        // Deployment_type is populated within the loop based on a subscription
        // to BackendInfoModel. Since deployment_type can be updated dynamically
        // during the backend's lifetime, this is important (notably from prewarm ->
        // dev|prod)
        let metadata = Arc::new(Mutex::new(LoggingDeploymentMetadata {
            deployment_name: instance_name,
            deployment_type: None,
            deployment_ref: None,
            project_name: None,
            project_slug: None,
            deployment_region,
        }));
        loop {
            let mut tx = database.begin(Identity::system()).await?;

            let mut bi_model = BackendInfoModel::new(&mut tx);
            if let Some(bi) = bi_model.get().await? {
                let mut metadata_guard = metadata.lock();
                metadata_guard.deployment_type = Some(bi.deployment_type);
                metadata_guard.deployment_ref = bi.deployment_ref.clone();
                metadata_guard.project_name = bi.project_name.clone();
                metadata_guard.project_slug = bi.project_slug.clone();
            }

            let mut log_sinks_model = LogSinksModel::new(&mut tx);
            let sink_rows = log_sinks_model.get_all().await?;
            Self::sink_startup_worker_once(
                runtime,
                database,
                fetch_client.clone(),
                sentry_transport.clone(),
                sink_rows,
                sinks,
                active_sinks_count.clone(),
                metadata.clone(),
                egress_counter.clone(),
            )
            .await?;

            // Wait for changes to the table
            let token = tx.into_token()?;
            database.subscribe_and_wait_for_invalidation(token).await?;
        }
    }

    async fn sink_startup_worker_once(
        runtime: &RT,
        database: &Database<RT>,
        fetch_client: Arc<dyn FetchClient>,
        sentry_transport: Arc<dyn TransportFactory>,
        sink_rows: Vec<ParsedDocument<LogSinksRow>>,
        sinks: &Arc<RwLock<BTreeMap<SinkType, LogSinkClient>>>,
        active_sinks_count: Arc<AtomicUsize>,
        metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
        egress_counter: EgressCounter,
    ) -> anyhow::Result<()> {
        let (pending_sinks, inactive_sinks, tombstoned_sinks) = {
            let sinks = sinks.read();
            let pending_sinks: Vec<_> = sink_rows
                .iter()
                .filter(|row| matches!(row.status, SinkState::Pending | SinkState::Restarting))
                .cloned()
                .collect();
            let inactive_sinks: Vec<_> = sink_rows
                .iter()
                .filter(|row| {
                    matches!(row.status, SinkState::Active)
                        && !sinks.contains_key(&row.config.sink_type())
                })
                .cloned()
                .collect();
            let tombstoned_sinks: Vec<_> = sink_rows
                .iter()
                .filter(|row| matches!(row.status, SinkState::Tombstoned))
                .cloned()
                .collect();
            (pending_sinks, inactive_sinks, tombstoned_sinks)
        };

        let mut tx = database.begin(Identity::system()).await?;

        let custom_audit_allowed = BackendInfoModel::new(&mut tx)
            .is_custom_audit_logs_in_log_streams_allowed()
            .await?;

        // Order is important! Handle any `Tombstoned` rows first in case they were
        // replaced with a new LogSink for the same provider that's in state `Pending`.
        //
        // We could potentially drop events between removing the `Tombstoned` and
        // activating the new `Pending` provider, but this would happen
        // with the user deleting + re-adding the provider manually.
        //
        // There also might be multiple `Tombstoned` sinks in the case of multiple
        // updates to a sink before we reach this loop to clean them up.

        // Delete `Tombstoned` rows
        tracing::info!(
            "Found {:} sink(s) in `Tombstoned` state",
            tombstoned_sinks.len()
        );
        for row in tombstoned_sinks {
            tracing::info!("Deleting row {}", row.config);
            SystemMetadataModel::new_global(&mut tx)
                .delete(row.id())
                .await?;

            // Remove locally
            if sinks.write().remove(&row.config.sink_type()).is_some() {
                active_sinks_count.fetch_sub(1, Ordering::Relaxed);
            }
        }

        // Startup `Pending` sinks
        let mut model = LogSinksModel::new(&mut tx);
        tracing::info!("Found {:} sink(s) in Pending state.", pending_sinks.len());
        for row in pending_sinks {
            tracing::info!("Starting log sink {}", row.config);

            let sink_type = row.config.sink_type();
            let sink_id = row.developer_id();
            let mut sink_config = row.config.clone();
            // Ignore the `custom_audit` topic if the deployment isn't entitled.
            if !custom_audit_allowed && let Some(Some(topics)) = sink_config.topics_mut() {
                topics.remove(&LogTopic::CustomAudit);
            }
            let sink_status = row.status.clone();
            let timed_startup_result = runtime
                .with_timeout(
                    "sink startup timeout",
                    consts::SINK_STARTUP_TIMEOUT,
                    Self::config_to_log_sink_client(
                        runtime,
                        fetch_client.clone(),
                        sentry_transport.clone(),
                        sink_config,
                        metadata.clone(),
                        sink_status,
                        egress_counter.clone(),
                    ),
                )
                .await;

            match timed_startup_result {
                Err(mut e) => {
                    // An unreachable or rejecting endpoint is the customer's
                    // configuration to fix, so it is counted rather than
                    // reported; anything else is ours and keeps full fidelity.
                    // `SINK_STARTUP_TIMEOUT` fires as a bare `TimeoutError`, so
                    // a hung endpoint only lands here.
                    let reason = if let Some(failure) = e.downcast_ref::<SinkEgressFailure>() {
                        metrics::log_sink_verification_failure(
                            sink_type.as_str(),
                            failure.outcome_label(),
                        );
                        format!("{e:#}")
                    } else if e.downcast_ref::<TimeoutError>().is_some() {
                        metrics::log_sink_verification_failure(sink_type.as_str(), "transient");
                        "Timed out verifying the log stream endpoint".to_string()
                    } else {
                        report_error(&mut e).await;
                        e.user_facing_message()
                    };
                    tracing::error!("Moving sink {sink_type:?} to Failed state. Reason: {reason}");

                    model
                        .patch_status(sink_id, SinkState::Failed { reason })
                        .await?;
                },
                Ok(sink_client) => {
                    model.patch_status(sink_id, SinkState::Active {}).await?;

                    // Save locally
                    sinks.write().insert(sink_type, sink_client);
                    // Ordering::Relaxed is okay here since logs don't need to be
                    // picked up immediately by an active sink and they can start
                    // a little earlier as well. Reordering isn't a big deal.
                    active_sinks_count.fetch_add(1, Ordering::Relaxed);
                },
            }
        }

        // Set inactive rows to `Restarting` (skips verification)
        tracing::info!("Found {:} inactive sink(s).", inactive_sinks.len());
        for row in inactive_sinks {
            tracing::info!(
                "Found log sink with Active status that has not started. Updating to Restarting \
                 status and restarting: {}",
                row.config
            );
            model
                .patch_status(row.developer_id(), SinkState::Restarting)
                .await?;
        }

        // Commit
        database
            .commit_with_write_source(tx, "log_sink_worker")
            .await?;

        Ok(())
    }

    /// Starts the LogSinkClient for this config
    async fn config_to_log_sink_client(
        runtime: &RT,
        fetch_client: Arc<dyn FetchClient>,
        sentry_transport: Arc<dyn TransportFactory>,
        config: SinkConfig,
        metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
        status: SinkState,
        egress_counter: EgressCounter,
    ) -> anyhow::Result<LogSinkClient> {
        // Only verify credentials for sinks in Pending state (not Restarting)
        let should_verify = matches!(status, SinkState::Pending);
        let failure_reporter = SinkFailureReporter::new(config.sink_type());

        match config {
            SinkConfig::Local(path) => LocalSink::start(runtime.clone(), path.parse()?).await,
            SinkConfig::Datadog(config) => {
                let topics = config.topics.clone();
                DatadogSink::start(
                    runtime.clone(),
                    fetch_client,
                    config,
                    topics,
                    metadata,
                    egress_counter.clone(),
                    failure_reporter,
                    should_verify,
                )
                .await
            },
            SinkConfig::Webhook(config) => {
                let topics = config.topics.clone();
                WebhookSink::start(
                    runtime.clone(),
                    config,
                    topics,
                    fetch_client,
                    metadata,
                    egress_counter.clone(),
                    failure_reporter,
                    should_verify,
                )
                .await
            },
            SinkConfig::Axiom(config) => {
                let topics = config.topics.clone();
                AxiomSink::start(
                    runtime.clone(),
                    config,
                    topics,
                    fetch_client,
                    metadata,
                    egress_counter.clone(),
                    failure_reporter,
                    should_verify,
                )
                .await
            },
            SinkConfig::Sentry(config) => {
                SentrySink::start(
                    runtime.clone(),
                    config,
                    sentry_transport,
                    metadata.clone(),
                    should_verify,
                )
                .await
            },
            SinkConfig::PostHogLogs(config) => {
                let topics = config.topics.clone();
                PostHogLogsSink::start(
                    runtime.clone(),
                    config,
                    topics,
                    fetch_client,
                    metadata,
                    egress_counter.clone(),
                    failure_reporter,
                    should_verify,
                )
                .await
            },
            SinkConfig::PostHogErrorTracking(config) => {
                PostHogErrorTrackingSink::start(
                    runtime.clone(),
                    config,
                    fetch_client,
                    metadata,
                    egress_counter.clone(),
                    failure_reporter,
                    should_verify,
                )
                .await
            },
            SinkConfig::S3Export(_) => {
                // The application worker advances this export; the log manager
                // owns the integration's lifecycle in `_log_sinks`.
                let (events_sender, mut events_receiver) = mpsc::channel(1);
                let handle = runtime.spawn("analytics_export_sink", async move {
                    while events_receiver.recv().await.is_some() {}
                });
                Ok(LogSinkClient {
                    _handle: Arc::new(Mutex::new(handle)),
                    events_sender,
                })
            },
        }
    }

    /// Periodically drains accumulated egress bytes, emits a
    /// `LogStreamEgress` event, and reports billing usage.
    async fn egress_emission_worker(
        runtime: &RT,
        event_sender: &mpsc::Sender<Arc<LogEvent>>,
        egress_counter: &EgressCounter,
        usage_counter: &UsageCounter,
    ) -> anyhow::Result<!> {
        loop {
            runtime.wait(Duration::from_secs(60)).await;
            let egress_bytes = egress_counter.swap(0, Ordering::Relaxed);
            if egress_bytes > 0 {
                // Emit log stream event
                let _ = event_sender.try_send(Arc::new(LogEvent {
                    timestamp: runtime.unix_timestamp(),
                    event: StructuredLogEvent::LogStreamEgress { egress_bytes },
                }));

                // Report billing usage
                let usage_tracker = usage_tracking::FunctionUsageTracker::new();
                usage_tracker.track_fetch_egress("log_stream_payload".to_string(), egress_bytes);
                let stats = usage_tracker.gather_user_stats();
                usage_counter
                    .track_call(
                        common::types::UdfIdentifier::SystemJob("log_stream_payload".to_string()),
                        common::execution_context::ExecutionId::new(),
                        common::RequestId::new(),
                        usage_tracking::CallType::LogStreamPayload,
                        true,
                        stats,
                    )
                    .await;
            }
        }
    }
}

pub struct LogSinkClient {
    _handle: Arc<Mutex<Box<dyn SpawnHandle>>>,
    events_sender: mpsc::Sender<Vec<Arc<LogEvent>>>,
}
