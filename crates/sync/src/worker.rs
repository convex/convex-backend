use std::{
    collections::BTreeMap,
    sync::{
        atomic::{
            AtomicUsize,
            Ordering,
        },
        Arc,
    },
    time::Duration,
};

use ::metrics::StatusTimer;
use application::{
    api::{
        is_recoverable_subscription_stream_failure,
        ApplicationApi,
        ExecuteQueryTimestamp,
        SubscriptionClient,
        SubscriptionTrait,
        SubscriptionValidity,
    },
    redaction::{
        RedactedJsError,
        RedactedLogLines,
    },
    RedactedActionError,
    RedactedMutationError,
};
use common::{
    backoff::Backoff,
    components::{
        CanonicalizedComponentFunctionPath,
        ComponentPath,
        ExportPath,
    },
    errors::report_error,
    fastrace_helpers::get_sampled_span,
    heap_size::HeapSize,
    http::ResolvedHostname,
    knobs::{
        SEARCH_INDEXES_UNAVAILABLE_RETRY_DELAY,
        SYNC_MAX_SEND_TRANSITION_COUNT,
        SYNC_WORKER_QUERY_RETRY_INITIAL_BACKOFF_MS,
        SYNC_WORKER_QUERY_RETRY_MAX_BACKOFF_SECS,
        SYNC_WORKER_UPDATE_QUERIES_RETRY_INITIAL_BACKOFF_MS,
        SYNC_WORKER_UPDATE_QUERIES_RETRY_MAX_BACKOFF_SECS,
    },
    runtime::{
        assert_send,
        try_join,
        Runtime,
        WithTimeout,
    },
    types::{
        FunctionCaller,
        QueryInvocation,
        UdfType,
    },
    value::JsonPackedValue,
    version::ClientVersion,
    RequestContext,
    RequestId,
    RequestMetadata,
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use fastrace::prelude::*;
use futures::{
    future::{
        self,
        BoxFuture,
        Fuse,
    },
    select_biased,
    stream::{
        self,
        Buffered,
        FuturesUnordered,
    },
    Future,
    FutureExt,
    StreamExt,
    TryStreamExt,
};
use keybroker::Identity;
use model::session_requests::types::SessionRequestIdentifier;
use sync_types::{
    AuthenticationToken,
    ClientMessage,
    IdentityVersion,
    QueryId,
    QuerySetModification,
    QuerySetVersion,
    SerializedQueryJournal,
    SessionId,
    StateModification,
    StateVersion,
    Timestamp,
    UdfPath,
};
use tokio::sync::{
    mpsc,
    mpsc::error::{
        SendError,
        TrySendError,
    },
};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    metrics::{
        self,
        connect_timer,
        log_action_args_size,
        log_mutation_args_size,
        log_query_modification_args_size,
        modify_query_to_transition_timer,
        mutation_queue_timer,
        TypedClientEvent,
    },
    state::{
        NeedsAuthRevalidation,
        QueryToFetch,
        SyncState,
    },
    subscription_reconnect::SubscriptionReconnectRateLimiter,
    ServerMessage,
};

// Buffer up to a thousand function and mutations executions.
const OPERATION_QUEUE_BUFFER_SIZE: usize = 1000;
const UPDATE_QUERY_CONCURRENCY: usize = 20;
const SYNC_WORKER_PROCESS_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug)]
pub struct SyncWorkerConfig {
    pub client_version: ClientVersion,
    pub supports_transition_chunks: bool,
    pub subscription_reconnect_rate_limiter: Option<Arc<SubscriptionReconnectRateLimiter>>,
}

impl Default for SyncWorkerConfig {
    fn default() -> Self {
        Self {
            client_version: ClientVersion::unknown(),
            supports_transition_chunks: false,
            subscription_reconnect_rate_limiter: None,
        }
    }
}

/// Creates a channel which allows the sender to track the buffer size and
/// opt-in to slow down if the buffer becomes too large.
pub fn measurable_unbounded_channel() -> (SingleFlightSender, SingleFlightReceiver) {
    let buffer_size_bytes = Arc::new(AtomicUsize::new(0));
    // The channel is used to send/receive "size reduced" notifications.
    let (size_reduced_tx, size_reduced_rx) = mpsc::channel(1);
    let (tx, rx) = mpsc::unbounded_channel();
    (
        SingleFlightSender {
            inner: tx,
            transition_count: buffer_size_bytes.clone(),
            count_reduced_rx: size_reduced_rx,
        },
        SingleFlightReceiver {
            inner: rx,
            transition_count: buffer_size_bytes,
            size_reduced_tx,
        },
    )
}

/// Wrapper around UnboundedSender that counts Transition messages,
/// allowing single-flighting, i.e. skipping transitions if the client is
/// backlogged on receiving them.
pub struct SingleFlightSender {
    inner: mpsc::UnboundedSender<(ServerMessage, tokio::time::Instant)>,

    transition_count: Arc<AtomicUsize>,
    count_reduced_rx: mpsc::Receiver<()>,
}

impl SingleFlightSender {
    pub fn send(
        &mut self,
        msg: (ServerMessage, tokio::time::Instant),
    ) -> Result<(), SendError<(ServerMessage, tokio::time::Instant)>> {
        if matches!(&msg.0, ServerMessage::Transition { .. }) {
            self.transition_count.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.send(msg)
    }

    pub fn transition_count(&self) -> usize {
        self.transition_count.load(Ordering::SeqCst)
    }

    // Waits until a single message has been received implying the size of the
    // buffer have been reduced. Note that if multiple messages are received
    // between calls, this will fire only once.
    pub async fn message_consumed(&mut self) {
        self.count_reduced_rx.recv().await;
    }
}

pub struct SingleFlightReceiver {
    inner: mpsc::UnboundedReceiver<(ServerMessage, tokio::time::Instant)>,

    transition_count: Arc<AtomicUsize>,
    size_reduced_tx: mpsc::Sender<()>,
}

impl SingleFlightReceiver {
    pub async fn next(&mut self) -> Option<(ServerMessage, tokio::time::Instant)> {
        let result = self.inner.recv().await;
        if let Some(msg) = &result {
            if matches!(msg.0, ServerMessage::Transition { .. }) {
                self.transition_count.fetch_sub(1, Ordering::SeqCst);
            }
            // Don't block if channel is full.
            _ = self.size_reduced_tx.try_send(());
        }
        result
    }

    pub fn try_next(&mut self) -> Option<(ServerMessage, tokio::time::Instant)> {
        let result = self.inner.try_recv().ok();
        if let Some(msg) = &result {
            if matches!(msg.0, ServerMessage::Transition { .. }) {
                self.transition_count.fetch_sub(1, Ordering::SeqCst);
            }
            // Don't block if channel is full.
            _ = self.size_reduced_tx.try_send(());
        }
        result
    }
}

pub(crate) const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

pub struct SyncWorker<RT: Runtime> {
    api: Arc<dyn ApplicationApi>,
    config: SyncWorkerConfig,
    rt: RT,
    state: SyncState,
    host: ResolvedHostname,

    rx: mpsc::UnboundedReceiver<(ClientMessage, tokio::time::Instant)>,
    tx: SingleFlightSender,

    // Queue of pending functions or mutations. For time being, we only execute
    // a single one since this is less error prone model for the developer.
    mutation_futures: Buffered<ReceiverStream<BoxFuture<'static, anyhow::Result<ServerMessage>>>>,
    mutation_sender: mpsc::Sender<BoxFuture<'static, anyhow::Result<ServerMessage>>>,
    pending_mutation_operations: usize,

    action_futures: FuturesUnordered<BoxFuture<'static, anyhow::Result<ServerMessage>>>,

    transition_future: Option<Fuse<BoxFuture<'static, anyhow::Result<TransitionState>>>>,

    // Has an update been scheduled for the future?
    update_scheduled: bool,

    /// If we've seen a FeatureTemporarilyUnavailable error, wait for this
    /// Future to resolve before retrying
    unavailable_query_retry_future: Option<Fuse<BoxFuture<'static, ()>>>,

    /// Timers to track time between handling ModifyQuerySet message and sending
    /// the Transition with the update
    modify_query_to_transition_timers: BTreeMap<QuerySetVersion, StatusTimer>,

    on_connect: Option<(StatusTimer, Box<dyn FnOnce(SessionId) + Send>)>,
    partition_id: u64,
    request_metadata: RequestMetadata,

    /// The difference between the client's clock and the server's clock, in
    /// milliseconds. Includes latency between the client and server.
    client_clock_skew: Option<i64>,
}

enum QueryResult {
    Rerun {
        result: Result<JsonPackedValue, RedactedJsError>,
        log_lines: RedactedLogLines,
        journal: SerializedQueryJournal,
    },
    /// Skip returning results of this query because a feature it depends on
    /// (e.g. search indexes or table summaries) is temporarily unavailable
    TemporarilyUnavailable,
    Refresh,
}

/// Whether a query in the new query set can reuse its existing subscription or
/// has to be rerun.
enum SubscriptionState {
    Reusable(Arc<dyn SubscriptionTrait>),
    NeedsRerun(QueryInvocation),
}

type CompletedQueryUpdate = (QueryId, QueryResult, Option<Arc<dyn SubscriptionTrait>>);

enum UpdateQueryOutcome {
    Completed(CompletedQueryUpdate),
    RecoverableFailure(anyhow::Error),
}

async fn join_update_query_task(
    name: &'static str,
    task: impl Future<Output = anyhow::Result<UpdateQueryOutcome>> + Send + 'static,
) -> anyhow::Result<CompletedQueryUpdate> {
    // The marker crosses the spawned task as a value because `try_join` recaptures
    // errors.
    match try_join(name, task).await? {
        UpdateQueryOutcome::Completed(result) => Ok(result),
        UpdateQueryOutcome::RecoverableFailure(error) => Err(error),
    }
}

async fn join_update_query_tasks<F>(
    name: &'static str,
    tasks: impl Iterator<Item = F> + Send,
) -> anyhow::Result<Vec<CompletedQueryUpdate>>
where
    F: Future<Output = anyhow::Result<UpdateQueryOutcome>> + Send + 'static,
{
    assert_send(
        stream::iter(tasks.map(|task| join_update_query_task(name, task)))
            .buffer_unordered(UPDATE_QUERY_CONCURRENCY)
            .try_collect(),
    )
    .await
}

struct TransitionState {
    udf_results: Vec<(QueryId, QueryResult, Arc<dyn SubscriptionTrait>)>,
    state_modifications: BTreeMap<QueryId, StateModification<JsonPackedValue>>,
    current_version: StateVersion,
    new_version: StateVersion,
    timer: StatusTimer,
    temporarily_unavailable: bool,
}

impl<RT: Runtime> SyncWorker<RT> {
    pub fn new(
        api: Arc<dyn ApplicationApi>,
        rt: RT,
        host: ResolvedHostname,
        config: SyncWorkerConfig,
        rx: mpsc::UnboundedReceiver<(ClientMessage, tokio::time::Instant)>,
        tx: SingleFlightSender,
        on_connect: Box<dyn FnOnce(SessionId) + Send>,
        partition_id: u64,
        request_metadata: RequestMetadata,
    ) -> Self {
        let (mutation_sender, receiver) = mpsc::channel(OPERATION_QUEUE_BUFFER_SIZE);
        let mutation_futures = ReceiverStream::new(receiver).buffered(1); // Execute at most one operation at a time.
        SyncWorker {
            api,
            config,
            rt,
            state: SyncState::new(partition_id),
            host,
            rx,
            tx,
            mutation_futures,
            mutation_sender,
            pending_mutation_operations: 0,
            action_futures: FuturesUnordered::new(),
            transition_future: None,
            update_scheduled: false,
            unavailable_query_retry_future: None,
            modify_query_to_transition_timers: BTreeMap::new(),
            on_connect: Some((connect_timer(), on_connect)),
            partition_id,
            request_metadata,
            client_clock_skew: None,
        }
    }

    fn schedule_update(&mut self) {
        self.update_scheduled = true;
    }

    fn schedule_unavailable_query_retry(&mut self) {
        if self.unavailable_query_retry_future.is_none() {
            let rt = self.rt.clone();
            self.unavailable_query_retry_future = Some(
                async move {
                    rt.wait(*SEARCH_INDEXES_UNAVAILABLE_RETRY_DELAY).await;
                }
                .boxed()
                .fuse(),
            );
        }
    }

    /// Run the sync protocol worker, returning `Ok(())` on clean exit and `Err`
    /// if there's an exceptional protocol condition that should shutdown
    /// the WebSocket.
    pub async fn go(&mut self) -> anyhow::Result<()> {
        match self.go_inner().await {
            Err(error) if is_recoverable_subscription_stream_failure(&error) => {
                self.delay_recoverable_subscription_failure().await;
                Err(error)
            },
            result => result,
        }
    }

    async fn go_inner(&mut self) -> anyhow::Result<()> {
        let mut ping_timeout = self.rt.wait(HEARTBEAT_INTERVAL);
        let mut pending = future::pending().boxed().fuse();
        let mut unavailable_retry_pending = future::pending().boxed().fuse();

        // Create a new subscription client for every sync socket. Thus we don't require
        // the subscription client to auto-recover on connection failures.
        let subscription_client: Arc<dyn SubscriptionClient> =
            self.api.subscription_client(&self.host).await?.into();

        // Starts off as a future that is never ready, as there's no identity that may
        // expire.
        'top: loop {
            self.state.validate()?;
            let maybe_response = select_biased! {
                message = self.rx.recv().fuse() => {
                    let (message, received_time) = match message {
                        Some(m) => m,
                        None => break 'top,
                    };
                    self.handle_message(message).await?;
                    let delay = self.rt.monotonic_now() - received_time;
                    metrics::log_process_client_message_delay(delay);
                    None
                },
                // TODO(presley): If I swap this with futures below, tests break.
                // We need to provide a guarantee that we can't transition to a
                // timestamp past a pending mutation or otherwise optimistic updates
                // might be flaky. To do that, we need to behave differently if we
                // have pending operation future or not.
                result = self.mutation_futures.next().fuse() => {
                    let message = match result {
                        Some(m) => {
                            self.pending_mutation_operations -= 1;
                            m?
                        },
                        None => panic!("mutation_futures sender dropped prematurely"),
                    };
                    self.schedule_update();
                    Some(message)
                },
                result = self.action_futures.select_next_some() => {
                    self.schedule_update();
                    Some(result?)
                },
                result = self.state.next_invalidated_query().fuse() => {
                    let _ = result?;
                    self.schedule_update();
                    None
                },
                transition_state = self.transition_future.as_mut().unwrap_or(&mut pending) => {
                    self.transition_future = None;
                    Some(self.finish_update_queries(transition_state?)?)
                },
                _ = self.unavailable_query_retry_future
                        .as_mut()
                        .unwrap_or(&mut unavailable_retry_pending) => {
                    tracing::info!("Scheduling an update to queries after a query failed because of async bootstrapping.");
                    self.unavailable_query_retry_future = None;
                    self.schedule_update();
                    None
                },
                _ = self.tx.message_consumed().fuse() => {
                    // Wake up if any message is consumed from the send buffer
                    // in case update_scheduled is True.
                    None
                }
                _ = ping_timeout => Some(ServerMessage::Ping {}),
            };
            // If there is a message to return to the client, send it.
            if let Some(response) = maybe_response {
                assert!(
                    !matches!(response, ServerMessage::FatalError { .. })
                        && !matches!(response, ServerMessage::AuthError { .. }),
                    "fatal errors are returned above when handling special error types",
                );
                // Break and exit cleanly if the websocket is dead.
                ping_timeout = self.rt.wait(HEARTBEAT_INTERVAL);
                let transition_heap_size = response.heap_size();
                metrics::log_transition_size(transition_heap_size);
                if self.tx.send((response, self.rt.monotonic_now())).is_err() {
                    break 'top;
                }
            }
            // Send update unless the send channel already contains enough transitions,
            // and unless we are already computing an update.
            if self.update_scheduled
                && self.tx.transition_count() < *SYNC_MAX_SEND_TRANSITION_COUNT
                && self.transition_future.is_none()
            {
                let identity = self.revalidate_identity().await?;
                let new_transition_future =
                    self.begin_update_queries(identity, subscription_client.clone())?;
                self.transition_future = Some(new_transition_future.boxed().fuse());
                self.update_scheduled = false;
            }
        }
        Ok(())
    }

    /// Delays an idle socket's reconnect behind its query-weighted reservation.
    ///
    /// Without a limiter or with pending client work, this returns immediately.
    /// While waiting, heartbeats continue and client events do not bypass the
    /// wait; admission or new work lets the caller close the socket so the
    /// browser reconnects normally.
    pub(crate) async fn delay_recoverable_subscription_failure(&mut self) {
        let query_count = self.state.num_queries();
        metrics::log_subscription_reconnect_affected_query_set_size(query_count);
        let Some(rate_limiter) = &self.config.subscription_reconnect_rate_limiter else {
            return;
        };
        if self.state.has_client_work_pending()
            || self.pending_mutation_operations > 0
            || !self.action_futures.is_empty()
        {
            metrics::log_subscription_reconnect_delay("client_activity");
            metrics::log_subscription_reconnect_wait(Duration::ZERO);
            return;
        }

        let start = self.rt.monotonic_now();
        let mut reservation = rate_limiter.reserve(self.partition_id, query_count, start);
        let mut ping_timeout = self.rt.wait(HEARTBEAT_INTERVAL);
        loop {
            tokio::select! {
                biased;
                _ = reservation.wait(&self.rt) => {
                    metrics::log_subscription_reconnect_delay("admitted");
                    metrics::log_subscription_reconnect_wait(self.rt.monotonic_now() - start);
                    return;
                },
                message = self.rx.recv() => match message {
                    None => {
                        metrics::log_subscription_reconnect_delay("browser_closed");
                        metrics::log_subscription_reconnect_wait(self.rt.monotonic_now() - start);
                        return;
                    },
                    Some((ClientMessage::Event(_), _)) => continue,
                    Some((
                        ClientMessage::Connect { .. }
                        | ClientMessage::ModifyQuerySet { .. }
                        | ClientMessage::Mutation { .. }
                        | ClientMessage::Action { .. }
                        | ClientMessage::Authenticate { .. },
                        _,
                    )) => {
                        metrics::log_subscription_reconnect_delay("client_activity");
                        metrics::log_subscription_reconnect_wait(self.rt.monotonic_now() - start);
                        return;
                    },
                },
                _ = &mut ping_timeout => {
                    if self.tx.send((ServerMessage::Ping {}, self.rt.monotonic_now())).is_err() {
                        metrics::log_subscription_reconnect_delay("ping_failed");
                        metrics::log_subscription_reconnect_wait(self.rt.monotonic_now() - start);
                        return;
                    }
                    ping_timeout = self.rt.wait(HEARTBEAT_INTERVAL);
                },
            }
        }
    }

    pub fn identity_version(&self) -> IdentityVersion {
        self.state.current_version().identity
    }

    pub fn parse_admin_component_path(
        component_path: &str,
        udf_path: &UdfPath,
        identity: &Identity,
    ) -> anyhow::Result<CanonicalizedComponentFunctionPath> {
        let path = ComponentPath::deserialize(Some(component_path))?;
        anyhow::ensure!(
            path.is_root() || identity.is_admin() || identity.is_system(),
            "Only admin or system users can call functions on non-root components directly"
        );
        let path = CanonicalizedComponentFunctionPath {
            component: path,
            udf_path: udf_path.clone().canonicalize(),
        };
        Ok(path)
    }

    async fn handle_message(&mut self, message: ClientMessage) -> anyhow::Result<()> {
        let timer = metrics::handle_message_timer(&message);
        match message {
            ClientMessage::Connect {
                session_id,
                last_close_reason,
                max_observed_timestamp,
                connection_count,
                client_ts,
            } => {
                if let Some((timer, on_connect)) = self.on_connect.take() {
                    timer.finish();
                    on_connect(session_id);
                }

                if let Some(ts) = client_ts {
                    let server_ts = self.rt.unix_timestamp().as_ms_since_epoch()?;
                    self.client_clock_skew = i64::try_from(ts)
                        .ok()
                        .zip(i64::try_from(server_ts).ok())
                        .and_then(|(client_ts, server_ts)| client_ts.checked_sub(server_ts));
                }

                self.state.set_session_id(session_id);
                if let Some(max_observed_timestamp) = max_observed_timestamp {
                    let latest_timestamp = *self
                        .api
                        .latest_timestamp(&self.host, RequestId::new())
                        .await?;
                    if max_observed_timestamp > latest_timestamp {
                        // Unless there is a bug, this means the client have communicated
                        // with a backend that have database writes we are not aware of. If
                        // we serve the request, we will get a linearizability violation.
                        // Instead error and report. It is possible we have to eventually turn
                        // into a client error if there are bogus custom client implementations
                        // but lets keep it as server one for now.
                        metrics::log_linearizability_violation(
                            max_observed_timestamp.secs_since_f64(latest_timestamp),
                        );
                        anyhow::bail!(
                            "Client has observed a timestamp {max_observed_timestamp:?} ahead of \
                             the backend latest known timestamp {latest_timestamp:?}",
                        );
                    }
                }
                metrics::log_connect(self.partition_id, last_close_reason, connection_count)
            },
            ClientMessage::ModifyQuerySet {
                base_version,
                new_version,
                modifications,
            } => {
                let total_args_size = modifications
                    .iter()
                    .filter_map(|m| match m {
                        QuerySetModification::Add(q) => Some(q.args.get().len()),
                        QuerySetModification::Remove { .. } => None,
                    })
                    .sum();
                log_query_modification_args_size(total_args_size);
                self.state
                    .modify_query_set(base_version, new_version, modifications)?;
                self.schedule_update();
                self.modify_query_to_transition_timers
                    .insert(new_version, modify_query_to_transition_timer());
            },
            ClientMessage::Mutation {
                request_id,
                udf_path,
                args,
                component_path,
            } => {
                log_mutation_args_size(args.get().len());
                let identity = self.revalidate_identity().await?;
                let mutation_identifier =
                    self.state.session_id().map(|id| SessionRequestIdentifier {
                        session_id: id,
                        request_id,
                    });
                let server_request_id = match self.state.session_id() {
                    Some(id) => RequestId::new_for_ws_session(id, request_id),
                    None => RequestId::new(),
                };
                let root = get_sampled_span(
                    &self.host.deployment_name,
                    "sync-worker/mutation",
                    &mut self.rt.rng(),
                )
                .with_property(|| ("udf_type", UdfType::Mutation.to_lowercase_string()))
                .with_property(|| ("udf_path", udf_path.to_string()));
                let rt = self.rt.clone();
                let client_version = self.config.client_version.clone();
                let timer = mutation_queue_timer();
                let api = self.api.clone();
                let host = self.host.clone();
                let caller = FunctionCaller::SyncWorker(client_version);
                let request_metadata = self.request_metadata.clone();

                let mutation_queue_size =
                    self.mutation_sender.max_capacity() - self.mutation_sender.capacity();
                root.add_property(|| ("mutation_queue_size", mutation_queue_size.to_string()));

                let future = async move {
                    rt.with_timeout("mutation", SYNC_WORKER_PROCESS_TIMEOUT, async move {
                        timer.finish();
                        let request_context =
                            RequestContext::new(server_request_id, request_metadata);
                        let result = match component_path {
                            None => {
                                api.execute_public_mutation(
                                    &host,
                                    request_context,
                                    identity,
                                    ExportPath::from(udf_path.canonicalize()),
                                    args,
                                    caller,
                                    mutation_identifier,
                                    Some(mutation_queue_size),
                                )
                                .in_span(root)
                                .await?
                            },
                            Some(ref p) => {
                                let path =
                                    Self::parse_admin_component_path(p, &udf_path, &identity)?;
                                api.execute_admin_mutation(
                                    &host,
                                    request_context,
                                    identity,
                                    path,
                                    args,
                                    caller,
                                    mutation_identifier,
                                    Some(mutation_queue_size),
                                )
                                .in_span(root)
                                .await?
                            },
                        };
                        let response = match result {
                            Ok(udf_return) => ServerMessage::MutationResponse {
                                request_id,
                                result: Ok(udf_return.value),
                                ts: Some(udf_return.ts),
                                log_lines: udf_return.log_lines.into(),
                            },
                            Err(RedactedMutationError { error, log_lines }) => {
                                ServerMessage::MutationResponse {
                                    request_id,
                                    result: Err(error.into_error_payload()),
                                    ts: None,
                                    log_lines: log_lines.into(),
                                }
                            },
                        };
                        Ok(response)
                    })
                    .await
                }
                .boxed();
                self.mutation_sender.try_send(future).map_err(|err| {
                    if matches!(err, TrySendError::Full(..)) {
                        anyhow::anyhow!(ErrorMetadata::rate_limited(
                            "TooManyConcurrentMutations",
                            format!(
                                "Too many concurrent mutations. Only up to \
                                 {OPERATION_QUEUE_BUFFER_SIZE} pending mutations allowed on a \
                                 single websocket."
                            ),
                        ))
                    } else {
                        anyhow::anyhow!("Failed to send to mutation channel: {err}")
                    }
                })?;
                self.pending_mutation_operations += 1;
            },
            ClientMessage::Action {
                request_id,
                udf_path,
                args,
                component_path,
            } => {
                log_action_args_size(args.get().len());
                let identity = self.revalidate_identity().await?;

                let api = self.api.clone();
                let host = self.host.clone();
                let client_version = self.config.client_version.clone();
                let request_metadata = self.request_metadata.clone();
                let server_request_id = match self.state.session_id() {
                    Some(id) => RequestId::new_for_ws_session(id, request_id),
                    None => RequestId::new(),
                };
                let root = get_sampled_span(
                    &self.host.deployment_name,
                    "sync-worker/action",
                    &mut self.rt.rng(),
                )
                .with_property(|| ("udf_type", UdfType::Action.to_lowercase_string()))
                .with_property(|| ("udf_path", udf_path.to_string()));
                let future = async move {
                    let caller = FunctionCaller::SyncWorker(client_version);
                    let request_context = RequestContext::new(server_request_id, request_metadata);
                    let result = match component_path {
                        None => {
                            api.execute_public_action(
                                &host,
                                request_context,
                                identity,
                                ExportPath::from(udf_path.canonicalize()),
                                args,
                                caller,
                            )
                            .in_span(root)
                            .await?
                        },
                        Some(ref p) => {
                            let path = Self::parse_admin_component_path(p, &udf_path, &identity)?;
                            api.execute_admin_action(
                                &host,
                                request_context,
                                identity,
                                path,
                                args,
                                caller,
                            )
                            .in_span(root)
                            .await?
                        },
                    };
                    let response = match result {
                        Ok(udf_return) => ServerMessage::ActionResponse {
                            request_id,
                            result: Ok(udf_return.value),
                            log_lines: udf_return.log_lines.into(),
                        },
                        Err(RedactedActionError { error, log_lines }) => {
                            ServerMessage::ActionResponse {
                                request_id,
                                result: Err(error.into_error_payload()),
                                log_lines: log_lines.into(),
                            }
                        },
                    };
                    Ok(response)
                }
                .boxed();
                anyhow::ensure!(
                    self.action_futures.len() <= OPERATION_QUEUE_BUFFER_SIZE,
                    ErrorMetadata::rate_limited(
                        "TooManyInflightActionsForSingleClient",
                        format!(
                            "Inflight actions overloaded for a single client, max concurrency: \
                             {OPERATION_QUEUE_BUFFER_SIZE}"
                        )
                    )
                );
                self.action_futures.push(future);
            },
            ClientMessage::Authenticate {
                token: auth_token,
                base_version,
            } => {
                let identity = self.fetch_identity(auth_token.clone()).await?;
                self.state
                    .modify_identity(identity, auth_token, base_version)?;
                self.schedule_update();
            },
            ClientMessage::Event(client_event) => {
                tracing::info!(
                    "Event with type {}: {}",
                    client_event.event_type,
                    client_event.event
                );
                match TypedClientEvent::try_from(client_event) {
                    Ok(typed_client_event) => match typed_client_event {
                        TypedClientEvent::ClientConnect { marks } => {
                            metrics::log_client_connect_timings(self.partition_id, marks)
                        },
                        TypedClientEvent::ClientReceivedTransition {
                            transition_transit_time,
                            message_length,
                        } => {
                            metrics::log_client_transition(transition_transit_time, message_length)
                        },
                        TypedClientEvent::NetworkRecoveryReconnect { time_saved_ms } => {
                            tracing::info!(
                                "Network recovery reconnect saved {:.1}s of waiting",
                                time_saved_ms / 1000.0
                            );
                            metrics::log_network_recovery_reconnect(
                                self.partition_id,
                                time_saved_ms,
                            )
                        },
                    },
                    Err(_) => (),
                }
            },
        };

        timer.finish();
        Ok(())
    }

    async fn revalidate_identity(&mut self) -> anyhow::Result<Identity> {
        match self.state.identity(self.rt.system_time())? {
            Ok(identity) => Ok(identity),
            Err(NeedsAuthRevalidation(auth_token)) => {
                let identity = self.fetch_identity(auth_token).await?;
                self.state.update_revalidated_identity(identity.clone());
                Ok(identity)
            },
        }
    }

    async fn fetch_identity(
        &mut self,
        auth_token: AuthenticationToken,
    ) -> anyhow::Result<Identity> {
        let identity_result = self
            .api
            .authenticate(
                &self.host,
                RequestContext::new(RequestId::new(), self.request_metadata.clone()),
                auth_token,
            )
            .await;
        let identity = match identity_result {
            Ok(identity) => identity,
            Err(error) => return Err(authentication_update_error(error)),
        };
        Ok(identity)
    }

    fn begin_update_queries(
        &mut self,
        identity: Identity,
        subscriptions_client: Arc<dyn SubscriptionClient>,
    ) -> anyhow::Result<impl Future<Output = anyhow::Result<TransitionState>> + use<RT>> {
        let root = get_sampled_span(
            &self.host.deployment_name,
            "sync-worker/update-queries",
            &mut self.rt.rng(),
        )
        .with_property(|| ("udf_type", UdfType::Query.to_lowercase_string()));
        let _guard = root.set_local_parent();
        let timer = metrics::update_queries_timer();
        let current_version = self.state.current_version();

        let (modifications, new_query_version, new_identity_version) =
            self.state.take_modifications();

        let mut identity_version = current_version.identity;
        let identity_changed = new_identity_version > identity_version;
        if identity_changed {
            // If the identity version has changed, invalidate all existing tokens.
            // TODO(CX-737): Don't invalidate queries that don't examine auth state.
            // TODO(CX-737): Don't invalidate the queries if the User the is the same
            // only with refreshed token. This is a bit tricky because:
            // - We need to prove that query does not depend on token issue/expiration time.
            // - We need to make rpc to backend to compare the properties since Usher can't
            // validate auth tokens. Alternatively, we make Usher be able to validate tokens
            // long term.
            self.state.take_subscriptions();
            identity_version = new_identity_version;
        }

        // Step 1: Add or remove queries from our query set.
        let mut state_modifications = BTreeMap::new();
        for modification in modifications {
            match modification {
                QuerySetModification::Add(query) => {
                    self.state.insert(query)?;
                },
                QuerySetModification::Remove { query_id } => {
                    self.state.remove(query_id)?;
                    state_modifications
                        .insert(query_id, StateModification::QueryRemoved { query_id });
                },
            }
        }

        // Step 2: Take all remaining subscriptions.
        let remaining_subscriptions = self.state.take_subscriptions();

        // Step 3: Refresh subscriptions up to new_ts and run queries which
        // subscriptions are no longer current.
        let api = self.api.clone();
        let rt = self.rt.clone();
        let need_fetch: Vec<_> = self.state.need_fetch().collect();
        let host = self.host.clone();
        let client_version = self.config.client_version.clone();
        let partition_id = self.partition_id;
        let request_metadata = self.request_metadata.clone();
        let mut backoff = Backoff::new(
            *SYNC_WORKER_UPDATE_QUERIES_RETRY_INITIAL_BACKOFF_MS,
            *SYNC_WORKER_UPDATE_QUERIES_RETRY_MAX_BACKOFF_SECS,
        );
        Ok(async move {
            loop {
                // Always transition to the latest timestamp. In the future,
                // when we have Sync Worker running on the edge, we can remove this
                // call by making self.update_scheduled to be a Option<Timestamp>,
                // and set it accordingly based on the operation that triggered the
                // Transition. We would choose the latest timestamp available at
                // the edge for the initial sync.
                let new_ts = *api.latest_timestamp(&host, RequestId::new()).await?;
                let new_version = StateVersion {
                    ts: new_ts,
                    // We only bump the query set version when the client modifies
                    // the query set
                    query_set: new_query_version,
                    identity: identity_version,
                };
                // TODO: On `run_update_queries` retries, we don't keep around successful
                // results even though only one query may have failed. We should
                // consider adding the successful results, so we don't have to
                // duplicate work on a single failure.
                match Self::run_update_queries(
                    api.clone(),
                    rt.clone(),
                    host.clone(),
                    request_metadata.clone(),
                    need_fetch.clone(),
                    identity.clone(),
                    identity_changed,
                    client_version.clone(),
                    partition_id,
                    subscriptions_client.clone(),
                    remaining_subscriptions.clone(),
                    new_ts,
                )
                .await
                {
                    Err(e) if e.is_out_of_retention() => {
                        metrics::log_sync_worker_update_queries_retry(partition_id);
                        let wait = backoff.fail(&mut rt.rng());
                        let err_msg = format!(
                            "Failed to update queries for deployment {}. Retrying in {} ms.",
                            host.deployment_name,
                            wait.as_millis()
                        );
                        tracing::error!(err_msg);
                        report_error(&mut e.context(err_msg)).await;
                        rt.wait(wait).await;
                        continue;
                    },
                    other => {
                        let (udf_results, temporarily_unavailable) = other?;
                        break Ok(TransitionState {
                            udf_results,
                            state_modifications,
                            current_version,
                            new_version,
                            timer,
                            temporarily_unavailable,
                        });
                    },
                }
            }
        }
        .in_span(root))
    }

    async fn run_update_queries(
        api: Arc<dyn ApplicationApi>,
        rt: RT,
        host: ResolvedHostname,
        request_metadata: RequestMetadata,
        need_fetch: Vec<QueryToFetch>,
        identity: Identity,
        identity_changed: bool,
        client_version: ClientVersion,
        partition_id: u64,
        subscriptions_client: Arc<dyn SubscriptionClient>,
        mut remaining_subscriptions: BTreeMap<QueryId, Arc<dyn SubscriptionTrait>>,
        new_ts: Timestamp,
    ) -> anyhow::Result<(
        Vec<(QueryId, QueryResult, Arc<dyn SubscriptionTrait>)>,
        bool,
    )> {
        let future_results = join_update_query_tasks(
            "update_query",
            need_fetch.into_iter().map(move |to_fetch| {
                let QueryToFetch {
                    query,
                    has_run_before,
                } = to_fetch;
                let api = api.clone();
                let rt = rt.clone();
                let host = host.clone();
                let request_metadata = request_metadata.clone();
                let identity_ = identity.clone();
                let client_version = client_version.clone();
                let current_subscription = remaining_subscriptions.remove(&query.query_id);
                let subscriptions_client = subscriptions_client.clone();
                async move {
                    LocalSpan::add_property(|| ("udf_path", query.udf_path.to_string()));
                    let subscription_state = match current_subscription {
                        Some(subscription) => match subscription.extend_validity(new_ts).await {
                            Ok(SubscriptionValidity::Valid) => {
                                SubscriptionState::Reusable(subscription)
                            },
                            Ok(SubscriptionValidity::Invalid { invalid_ts }) => {
                                metrics::log_query_invalidated(partition_id, invalid_ts, new_ts);
                                SubscriptionState::NeedsRerun(QueryInvocation::Invalidated)
                            },
                            Err(error) if is_recoverable_subscription_stream_failure(&error) => {
                                return Ok(UpdateQueryOutcome::RecoverableFailure(error));
                            },
                            Err(error) => return Err(error),
                        },
                        None if has_run_before && identity_changed => {
                            SubscriptionState::NeedsRerun(QueryInvocation::IdentityChange)
                        },
                        None if has_run_before => {
                            SubscriptionState::NeedsRerun(QueryInvocation::Invalidated)
                        },
                        None => SubscriptionState::NeedsRerun(QueryInvocation::Fresh),
                    };
                    let (query_result, subscription) = match subscription_state {
                        SubscriptionState::Reusable(subscription) => {
                            (QueryResult::Refresh, Some(subscription))
                        },
                        SubscriptionState::NeedsRerun(invocation) => {
                            // We failed to refresh the subscription or it was invalid to start
                            // with. Rerun the query.
                            let caller = FunctionCaller::SyncWorker(client_version);

                            // This query run might have been triggered due to invalidation
                            // of a subscription. The sync worker is effectively the owner
                            // of the query so we do not want to re-use the original query
                            // request id.
                            let mut backoff = Backoff::new(
                                *SYNC_WORKER_QUERY_RETRY_INITIAL_BACKOFF_MS,
                                *SYNC_WORKER_QUERY_RETRY_MAX_BACKOFF_SECS,
                            );
                            let udf_return_result = loop {
                                let request_context =
                                    RequestContext::new(RequestId::new(), request_metadata.clone());
                                let result = match query.component_path {
                                    None => {
                                        api.execute_public_query(
                                            &host,
                                            request_context,
                                            identity_.clone(),
                                            ExportPath::from(query.udf_path.clone().canonicalize()),
                                            query.args.clone(),
                                            caller.clone(),
                                            ExecuteQueryTimestamp::At(new_ts),
                                            query.journal.clone(),
                                            Some(invocation),
                                        )
                                        .await
                                    },
                                    Some(ref p) => {
                                        let path = Self::parse_admin_component_path(
                                            p,
                                            &query.udf_path,
                                            &identity_,
                                        )?;
                                        api.execute_admin_query(
                                            &host,
                                            request_context,
                                            identity_.clone(),
                                            path,
                                            query.args.clone(),
                                            caller.clone(),
                                            ExecuteQueryTimestamp::At(new_ts),
                                            query.journal.clone(),
                                            Some(invocation),
                                        )
                                        .await
                                    },
                                };
                                match result {
                                    Err(e) if is_retriable_sync_worker_error(&e) => {
                                        metrics::log_sync_worker_query_retry(partition_id);
                                        let wait = backoff.fail(&mut rt.rng());
                                        let err_msg = format!(
                                            "Failed to run query for deployment {}. Retrying in \
                                             {} ms.",
                                            host.deployment_name,
                                            wait.as_millis()
                                        );
                                        tracing::error!(err_msg);
                                        report_error(&mut e.context(err_msg)).await;
                                        rt.wait(wait).await;
                                        continue;
                                    },
                                    _ => break result,
                                }
                            };
                            match udf_return_result {
                                Err(e) => {
                                    if e.is_feature_temporarily_unavailable() {
                                        (QueryResult::TemporarilyUnavailable, None)
                                    } else {
                                        anyhow::bail!(e)
                                    }
                                },
                                Ok(udf_return) => {
                                    let subscription = match subscriptions_client
                                        .subscribe(udf_return.token)
                                        .await
                                    {
                                        Ok(subscription) => subscription,
                                        Err(error)
                                            if is_recoverable_subscription_stream_failure(
                                                &error,
                                            ) =>
                                        {
                                            return Ok(UpdateQueryOutcome::RecoverableFailure(
                                                error,
                                            ));
                                        },
                                        Err(error) => return Err(error),
                                    };
                                    (
                                        QueryResult::Rerun {
                                            result: udf_return.result,
                                            log_lines: udf_return.log_lines,
                                            journal: udf_return.journal,
                                        },
                                        Some(subscription),
                                    )
                                },
                            }
                        },
                    };
                    Ok::<_, anyhow::Error>(UpdateQueryOutcome::Completed((
                        query.query_id,
                        query_result,
                        subscription,
                    )))
                }
            }),
        )
        .await?;

        let mut udf_results = vec![];
        let mut temporarily_unavailable = false;
        for (query_id, result, maybe_subscription) in future_results {
            if matches!(result, QueryResult::TemporarilyUnavailable) {
                temporarily_unavailable = true;
            }
            if let Some(subscription) = maybe_subscription {
                udf_results.push((query_id, result, subscription));
            }
        }

        Ok((udf_results, temporarily_unavailable))
    }

    fn finish_update_queries(
        &mut self,
        TransitionState {
            udf_results,
            mut state_modifications,
            current_version,
            new_version,
            timer,
            temporarily_unavailable,
        }: TransitionState,
    ) -> anyhow::Result<ServerMessage> {
        for (query_id, result, subscription) in udf_results {
            match result {
                QueryResult::Rerun {
                    result,
                    log_lines,
                    journal,
                } => {
                    let modification = self.state.complete_fetch(
                        query_id,
                        result,
                        log_lines,
                        journal,
                        subscription,
                    )?;
                    let Some(modification) = modification else {
                        continue;
                    };
                    state_modifications.insert(query_id, modification);
                },
                QueryResult::Refresh => {
                    self.state.refill_subscription(query_id, subscription)?;
                },
                QueryResult::TemporarilyUnavailable => {
                    anyhow::bail!(
                        "No QueryResult::TemporarilyUnavailable should have a udf result and \
                         subscription"
                    )
                },
            }
        }

        if temporarily_unavailable {
            self.schedule_unavailable_query_retry();
        }

        // Resubscribe for queries that don't have an active invalidation
        // future.
        self.state.fill_invalidation_futures()?;

        // Step 6: Send our transition to the client and update our version.
        self.state.advance_version(new_version)?;
        let transition = ServerMessage::Transition {
            start_version: current_version,
            end_version: new_version,
            modifications: state_modifications.into_values().collect(),
            client_clock_skew: self.client_clock_skew,
            server_ts: None,
        };
        timer.finish();
        metrics::log_query_set_size(self.partition_id, self.state.num_queries());
        // Only retain timers for queries that haven't been updated yet. Finish the
        // timers for everything up through the new version.
        let finished_timers = self
            .modify_query_to_transition_timers
            .extract_if(.., |version, _| *version <= new_version.query_set);
        for (_, timer) in finished_timers {
            timer.finish();
        }
        Ok(transition)
    }
}

fn is_retriable_sync_worker_error(err: &anyhow::Error) -> bool {
    err.is_misdirected_request()
        || err.is_operational_internal_server_error()
        || err.is_overloaded()
        || err.is_rejected_before_execution()
}

fn is_authentication_rejection(error: &anyhow::Error) -> bool {
    error.is_unauthenticated() || error.is_forbidden() || error.is_bad_request()
}

pub(crate) fn authentication_update_error(error: anyhow::Error) -> anyhow::Error {
    if !is_authentication_rejection(&error) {
        return error;
    }
    ErrorMetadata::auth_update_failed(error.short_msg().to_string(), error.msg().to_string()).into()
}
