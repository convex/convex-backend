//! The synchronous state machine for Convex. It's
//! recommended to use the higher level [`ConvexClient`] unless you are building
//! a framework.
//!
//! See docs for [`BaseConvexClient`].
use std::{
    cmp,
    collections::{
        BTreeMap,
        BTreeSet,
        VecDeque,
    },
    future::Future,
    pin::Pin,
};

use convex_sync_types::{
    types::SerializedArgs,
    AuthenticationToken,
    CanonicalizedUdfPath,
    ClientMessage,
    IdentityVersion,
    QueryId,
    QuerySetModification,
    QuerySetVersion,
    SessionRequestSeqNumber,
    StateModification,
    StateVersion,
    Timestamp,
    UdfPath,
};
use serde_json::json;
use tokio::sync::oneshot;

#[cfg(doc)]
use crate::ConvexClient;
use crate::{
    convex_logs,
    sync::{
        ReconnectProtocolReason,
        ServerMessage,
    },
    value::Value,
    ConvexError,
};

mod request_manager;
use request_manager::{
    RequestId,
    RequestManager,
};
mod query_result;
pub use query_result::{
    FunctionResult,
    QueryResults,
};

use self::request_manager::RequestType;

/// A callback that fetches an auth token. The `bool` parameter indicates
/// whether a forced refresh is requested (e.g. on websocket reconnect).
pub type AuthTokenFetcher = Box<
    dyn Fn(bool) -> Pin<Box<dyn Future<Output = anyhow::Result<AuthenticationToken>> + Send>>
        + Send
        + Sync,
>;

#[derive(Clone, Eq, PartialEq, PartialOrd, Ord, Debug)]
struct QueryToken(String);

#[derive(Clone, Debug)]
struct LocalQuery {
    id: QueryId,
    canonicalized_udf_path: CanonicalizedUdfPath,
    args: BTreeMap<String, Value>,
    num_subscribers: usize, // TODO: remove
    /// A unique index value for each subscription to this query.
    ///
    /// Must be incremented each time a new subscription is added, and never
    /// decremented.
    subscription_index: usize,
}

#[derive(Clone, Debug)]
struct Query {
    result: FunctionResult,
    _udf_path: CanonicalizedUdfPath,
    _args: BTreeMap<String, Value>,
}

/// An identifier for a single subscriber to a query.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct SubscriberId(QueryId, usize);

impl SubscriberId {
}

fn serialize_path_and_args(udf_path: UdfPath, args: BTreeMap<String, Value>) -> QueryToken {
    let json_path: String = udf_path.canonicalize().into();
    let json_args: serde_json::Value = Value::Array(vec![Value::Object(args)]).into();
    let json = json!({
        "udfPath": json_path,
        "args": json_args,
    });
    QueryToken(json.to_string())
}

#[derive(Default)]
struct LocalSyncState {
    next_query_id: QueryId,
    query_set_version: QuerySetVersion,
    query_set: BTreeMap<QueryToken, LocalQuery>,
    query_id_to_token: BTreeMap<QueryId, QueryToken>,
    latest_results: QueryResults,
    identity_version: IdentityVersion,
    auth_fetcher: Option<AuthTokenFetcher>,
}

impl LocalSyncState {
    fn subscribe(
        &mut self,
        udf_path: UdfPath,
        args: BTreeMap<String, Value>,
    ) -> (Option<ClientMessage>, SubscriberId) {
        let canonicalized_udf_path = udf_path.clone().canonicalize();
        let query_token = serialize_path_and_args(udf_path.clone(), args.clone());

        if let Some(existing_entry) = self.query_set.get_mut(&query_token) {
            // This is a new subscription to an existing query.
            existing_entry.num_subscribers += 1;
            existing_entry.subscription_index += 1;
            let query_id = existing_entry.id;
            let subscription = SubscriberId(query_id, existing_entry.subscription_index);
            let prev = self.latest_results.subscribers.insert(subscription);
            assert!(prev.is_none(), "INTERNAL BUG: Subscriber ID already taken.");
            return (None, subscription);
        }

        let query_id = self.next_query_id;
        self.next_query_id = QueryId::new(self.next_query_id.get_id() + 1);
        let base_version = self.query_set_version;
        let new_version = self.query_set_version.incr();

        let add = QuerySetModification::Add(convex_sync_types::Query {
            query_id,
            udf_path,
            args: SerializedArgs::from_args(vec![Value::Object(args.clone()).into()])
                .expect("Could not serialize query arguments"),
            journal: None,
            component_path: None,
        });
        let message = ClientMessage::ModifyQuerySet {
            base_version,
            new_version,
            modifications: vec![add],
        };

        let query = LocalQuery {
            id: query_id,
            canonicalized_udf_path,
            args,
            num_subscribers: 1,
            subscription_index: 0,
        };

        self.query_set.insert(query_token.clone(), query);
        self.query_id_to_token.insert(query_id, query_token.clone());
        let subscription = SubscriberId(query_id, 0);
        let prev = self.latest_results.subscribers.insert(subscription);
        assert!(prev.is_none(), "INTERNAL BUG: Subscriber ID already taken.");
        (Some(message), subscription)
    }

    fn remove_subscriber(&mut self, subscriber_id: SubscriberId) -> Option<ClientMessage> {
        let query_id = self
            .latest_results
            .subscribers
            .remove(&subscriber_id)
            .expect("INTERNAL BUG: Dropped unknown Subscriber ID")
            .0;
        let query_token = match self.query_token(query_id) {
            None => panic!("INTERNAL BUG: Unknown query id {query_id}"),
            Some(t) => t,
        };
        let local_query = match self.query_set.get_mut(&query_token) {
            None => panic!("INTERNAL BUG: No query found for query token {query_token:?}",),
            Some(q) => q,
        };

        // Update local state
        if local_query.num_subscribers > 1 {
            local_query.num_subscribers -= 1;
            return None;
        }
        self.query_set.remove(&query_token);
        self.query_id_to_token.remove(&query_id);
        self.latest_results.results.remove(&query_id);

        let base_version = self.query_set_version;
        let new_version = self.query_set_version.incr();

        let remove = QuerySetModification::Remove { query_id };
        Some(ClientMessage::ModifyQuerySet {
            base_version,
            new_version,
            modifications: vec![remove],
        })
    }

    fn query_token(&self, query_id: QueryId) -> Option<QueryToken> {
        self.query_id_to_token.get(&query_id).cloned()
    }

    fn query_args(&self, query_id: QueryId) -> Option<BTreeMap<String, Value>> {
        Some(
            self.query_set
                .get(&self.query_token(query_id)?)?
                .args
                .clone(),
        )
    }

    fn query_path(&self, query_id: QueryId) -> Option<CanonicalizedUdfPath> {
        Some(
            self.query_set
                .get(&self.query_token(query_id)?)?
                .canonicalized_udf_path
                .clone(),
        )
    }

    fn authenticate(&mut self, token: AuthenticationToken) -> ClientMessage {
        let base_version = self.identity_version;
        self.identity_version.incr();
        ClientMessage::Authenticate {
            base_version,
            token,
        }
    }

    async fn restart(&mut self) -> Vec<ClientMessage> {
        self.identity_version = IdentityVersion::default();
        let mut messages = Vec::new();

        // If we have a fetcher, get a fresh token for the new connection.
        if let Some(ref fetcher) = self.auth_fetcher {
            match fetcher(true).await {
                Ok(token) if token != AuthenticationToken::None => {
                    messages.push(ClientMessage::Authenticate {
                        base_version: self.identity_version,
                        token,
                    });
                    self.identity_version.incr();
                },
                Ok(_) => {},
                Err(e) => {
                    tracing::error!(
                        "Auth fetcher failed during reconnect: {e:?}. Skipping auth for this \
                         reconnect attempt."
                    );
                },
            }
        }

        let mut modifications = Vec::new();
        for local_query in self.query_set.values() {
            let add = QuerySetModification::Add(convex_sync_types::Query {
                query_id: local_query.id,
                udf_path: local_query.canonicalized_udf_path.clone().into(),
                args: SerializedArgs::from_args(vec![
                    Value::Object(local_query.args.clone()).into()
                ])
                .expect("Could not serialize query arguments"),
                journal: None,
                component_path: None,
            });
            modifications.push(add)
        }
        self.query_set_version = QuerySetVersion::default();
        let base_version = self.query_set_version;
        let new_version = self.query_set_version.incr();

        messages.push(ClientMessage::ModifyQuerySet {
            base_version,
            new_version,
            modifications,
        });

        messages
    }
}

#[derive(Debug)]
struct RemoteQuerySet {
    version: StateVersion,
    remote_query_set: BTreeMap<QueryId, FunctionResult>,
}

impl RemoteQuerySet {
    fn new() -> Self {
        Self {
            version: StateVersion::initial(),
            remote_query_set: Default::default(),
        }
    }

    fn transition(&mut self, transition: ServerMessage) -> Result<(), ReconnectProtocolReason> {
        let ServerMessage::Transition {
            start_version,
            end_version,
            modifications,
            client_clock_skew: _,
            server_ts: _,
        } = transition
        else {
            panic!("not transition");
        };
        if start_version != self.version {
            tracing::error!(
                "INTERNAL BUG: Protocol Error start_version {:?} is different from self.version \
                 {:?}",
                start_version,
                self.version
            );
            return Err("StartVersionMismatch".into());
        }
        for modification in modifications {
            match modification {
                StateModification::QueryUpdated {
                    query_id,
                    value,
                    log_lines,
                    journal: _,
                } => {
                    for log_line in log_lines.0 {
                        convex_logs!("{}", log_line);
                    }
                    self.remote_query_set
                        .insert(query_id, FunctionResult::Value(value));
                },
                StateModification::QueryFailed {
                    query_id,
                    error_message,
                    log_lines,
                    journal: _,
                    error_data,
                } => {
                    for log_line in log_lines.0 {
                        convex_logs!("{}", log_line);
                    }
                    let function_result = match error_data {
                        Some(v) => FunctionResult::ConvexError(ConvexError {
                            message: error_message,
                            data: v,
                        }),
                        None => FunctionResult::ErrorMessage(error_message),
                    };
                    self.remote_query_set.insert(query_id, function_result);
                },
                StateModification::QueryRemoved { query_id } => {
                    self.remote_query_set.remove(&query_id);
                },
            }
        }
        self.version = end_version;
        Ok(())
    }
}

#[derive(Default, Debug)]
struct OptimisticQueryResults {
    query_results: BTreeMap<QueryId, Query>,
}

impl OptimisticQueryResults {
    fn ingest_query_results_from_server(
        &mut self,
        server_query_results: BTreeMap<QueryId, Query>,
        _optimistic_updates_to_drop: BTreeSet<RequestId>,
    ) -> BTreeMap<QueryId, FunctionResult> {
        // TODO: use optimistic_updates_to_drop
        let old_query_results = self.query_results.clone();
        self.query_results = server_query_results;
        let mut changed_queries = BTreeMap::new();
        for (query_id, query) in self.query_results.iter() {
            let old_query = old_query_results.get(query_id);
            if match old_query {
                Some(old_query) => old_query.result != query.result,
                None => true,
            } {
                let result = query.result.clone();
                changed_queries.insert(*query_id, result);
            }
        }
        changed_queries
    }

    fn query_result(&self, query_id: QueryId) -> Option<FunctionResult> {
        self.query_results.get(&query_id).map(|q| q.result.clone())
    }
}

/// The synchronous state machine for the `ConvexClient`. It's recommended to
/// use the higher level `ConvexClient` unless you are building a framework.
///
/// This struct should be used instead of the `ConvexClient` when you want the
/// ability to build consistent client views. For example, in order to use your
/// own websocket manager or make a client compatible with another language
/// (e.g. Swift or Python).
///
/// For the latter use case, we strongly recommend you to take a look at the
/// implementation of the `ConvexClient`. The recommended pattern to use an
/// [`BaseConvexClient`] is to create a background thread to manage actions on
/// queries/mutations and incoming websocket connections, and use that to
/// advance the BaseConvexClient's state.
///
/// ## Managing Convex State
/// The main methods, [`subscribe`](Self::subscribe()),
/// [`unsubscribe`](Self::unsubscribe()), and
/// [`mutation`](Self::mutation()) directly correspond to its
/// equivalent for the external [ConvexClient].
///
/// The only different method is [`get_query`](Self::get_query()), which
/// returns the current value for a query given its query id. This method can be
/// used to synchronously request the current value, as opposed to a stream of
/// values in [`subscribe`](crate::ConvexClient::subscribe()).
///
/// **Note: these methods have the side effect of
/// adding messages to be sent to the server, so you would need to flush all
/// outgoing messages by looping on
/// [`pop_next_message`](Self::pop_next_message()) after each call of the above
/// functions.**
///
/// ## Watching for consistent updates to queries
/// To watch for consistent changes in query values, you can add the following
/// code to the background thread:
/// ```no_run
/// use convex::base_client::BaseConvexClient;
/// use convex::Value;
/// use convex_sync_types::ServerMessage;
///
/// fn on_receive_server_message(mut base_client: BaseConvexClient, msg: ServerMessage<Value>) {
///     let res = base_client.receive_message(msg).expect("Base client error");
///     if let Some(latest_result_map) = res {
///         for (subscriber_id, function_result) in latest_result_map.iter() {
///             // Notify components of the updated_value
///         }
///     }
/// }
/// ```
///
/// ## Managing Web Socket States
/// To manage websocket messages, use
/// [`receive_message`](Self::receive_message()) (for incoming messages from the
/// server) and [`pop_next_message`](Self::pop_next_message()) (for outgoing
/// messages to send to the server). **The [`BaseConvexClient`] does not
/// send these messages, so you will have to regularly monitor if there are
/// messages to be sent by calling
/// [`pop_next_message`](Self::pop_next_message()).**
///
/// Additionally, when the websocket reconnects, you should call
/// [`resend_ongoing_queries_mutations`](Self::resend_ongoing_queries_mutations()) and loop on
/// [`pop_next_message`](Self::pop_next_message()) to resend requests to the
/// Server to resubscribe to queries and perform ongoing mutations.
///
/// #### [`pop_next_message`](Self::pop_next_message()) should be called after the following methods:
/// - [`resend_ongoing_queries_mutations`](Self::resend_ongoing_queries_mutations())
/// - [`subscribe`](Self::unsubscribe())
/// - [`unsubscribe`](Self::unsubscribe())
/// - [`mutation`](Self::unsubscribe())
pub struct BaseConvexClient {
    state: LocalSyncState,
    remote_query_set: RemoteQuerySet,
    optimistic_query_results: OptimisticQueryResults,
    request_manager: RequestManager,
    next_request_id: SessionRequestSeqNumber,
    outgoing_message_queue: VecDeque<ClientMessage>,
    max_observed_timestamp: Option<Timestamp>,
}

impl BaseConvexClient {
    /// Construct a new [`BaseConvexClient`].
    pub fn new() -> Self {
        let request_manager = RequestManager::new();
        let state = LocalSyncState::default();
        let remote_query_set = RemoteQuerySet::new();
        let optimistic_query_results: OptimisticQueryResults = Default::default();
        let next_request_id: SessionRequestSeqNumber = 0;

        BaseConvexClient {
            request_manager,
            state,
            remote_query_set,
            optimistic_query_results,
            next_request_id,
            outgoing_message_queue: VecDeque::new(),
            max_observed_timestamp: None,
        }
    }

    /// Update state to be subscribed to a query and add subscription request to
    /// the outgoing message queue.
    ///
    /// After calling this, it is highly recommended to loop on
    /// [`pop_next_message`](Self::pop_next_message()) to flush websocket
    /// messages to the server.
    pub fn subscribe(&mut self, udf_path: UdfPath, args: BTreeMap<String, Value>) -> SubscriberId {
        let (modification, subscription) = self.state.subscribe(udf_path, args);
        if let Some(modification) = modification {
            self.outgoing_message_queue.push_back(modification);
        }
        subscription
    }

    /// Update state to be unsubscribed to a query and add unsubscription
    /// request to the outgoing message queue.
    ///
    /// After calling this, it is highly recommended to loop on
    /// [`pop_next_message`](Self::pop_next_message()) to flush websocket
    /// messages to the server.
    pub fn unsubscribe(&mut self, subscriber_id: SubscriberId) {
        let unsubscribe_message = self.state.remove_subscriber(subscriber_id);

        if let Some(message) = unsubscribe_message {
            self.outgoing_message_queue.push_back(message);
        }
    }

    /// Return the local value of a query.
    pub fn get_query(&self, query_id: QueryId) -> Option<FunctionResult> {
        self.local_query_result(query_id)
    }

    /// Track mutation and add mutation request to the outgoing message queue.
    ///
    /// After calling this, it is highly recommended to loop on
    /// [`pop_next_message`](Self::pop_next_message()) to flush websocket
    /// messages to the server.
    pub fn mutation(
        &mut self,
        udf_path: UdfPath,
        args: BTreeMap<String, Value>,
    ) -> oneshot::Receiver<FunctionResult> {
        let request_id = self.next_request_id;
        self.next_request_id = request_id + 1;
        tracing::info!("Starting mutation {udf_path} with id {request_id}");
        let message = ClientMessage::Mutation {
            request_id,
            udf_path,
            args: SerializedArgs::from_args(vec![Value::Object(args).into()])
                .expect("Failed to serialize arguments"),
            component_path: None,
        };

        let result_receiver = self.request_manager.track_request(
            &message,
            RequestId::new(request_id),
            RequestType::Mutation,
        );
        self.outgoing_message_queue.push_back(message);
        result_receiver
    }

    /// Track action and add action request to the outgoing message queue.
    ///
    /// After calling this, it is highly recommended to loop on
    /// [`pop_next_message`](Self::pop_next_message()) to flush websocket
    /// messages to the server.
    pub fn action(
        &mut self,
        udf_path: UdfPath,
        args: BTreeMap<String, Value>,
    ) -> oneshot::Receiver<FunctionResult> {
        let request_id = self.next_request_id;
        self.next_request_id = request_id + 1;
        tracing::info!("Starting action {udf_path:?} with id {request_id:?}");
        let message = ClientMessage::Action {
            request_id,
            udf_path,
            args: SerializedArgs::from_args(vec![Value::Object(args).into()]).unwrap(),
            component_path: None,
        };

        let result_receiver = self.request_manager.track_request(
            &message,
            RequestId::new(request_id),
            RequestType::Action,
        );
        self.outgoing_message_queue.push_back(message);
        result_receiver
    }

    /// Store (or clear) an auth token fetcher callback and update auth state.
    ///
    /// When a fetcher is provided it is invoked immediately (with
    /// `force_refresh=false`) and stored for future reconnects — on each
    /// websocket reconnect the fetcher is called again with
    /// `force_refresh=true`.
    ///
    /// When `None` is passed the stored fetcher is cleared and auth is unset.
    pub async fn set_auth_fetcher(&mut self, fetcher: Option<AuthTokenFetcher>) {
        match fetcher {
            Some(fetcher) => {
                match fetcher(false).await {
                    Ok(token) => {
                        let message = self.state.authenticate(token);
                        self.outgoing_message_queue.push_back(message);
                    },
                    Err(e) => {
                        tracing::error!("Auth token fetcher failed: {e:?}");
                    },
                }
                self.state.auth_fetcher = Some(fetcher);
            },
            None => {
                self.state.auth_fetcher = None;
                let message = self.state.authenticate(AuthenticationToken::None);
                self.outgoing_message_queue.push_back(message);
            },
        }
    }

    /// Pop the next message from the outgoing message queue.
    ///
    /// Note that this does not *send* the message because the Internal client
    /// has no awareness of websockets. After popping the next message, it is
    /// the caller's responsibility to actually send it.
    pub fn pop_next_message(&mut self) -> Option<ClientMessage> {
        self.outgoing_message_queue.pop_front()
    }

    fn observe_timestamp(&mut self, ts: Timestamp) {
        if let Some(max_observed_timestamp) = self.max_observed_timestamp {
            self.max_observed_timestamp = Some(cmp::max(ts, max_observed_timestamp));
        } else {
            self.max_observed_timestamp = Some(ts);
        }
    }

    /// Returns the maximum timestamp observed by the client.
    pub fn max_observed_timestamp(&self) -> Option<Timestamp> {
        self.max_observed_timestamp
    }

    /// Given a message from a Server, update the base state accordingly.
    pub fn receive_message(
        &mut self,
        message: ServerMessage,
    ) -> Result<Option<QueryResults>, ReconnectProtocolReason> {
        match message {
            ServerMessage::Transition { end_version, .. } => {
                self.observe_timestamp(end_version.ts);
                self.remote_query_set.transition(message)?;
                let completed_requests = self
                    .request_manager
                    .remove_and_notify_completed(end_version.ts);
                let changed_query_ids = self.on_query_result_changes(completed_requests)?;
                for (id, result) in changed_query_ids {
                    self.state.latest_results.results.insert(id, result);
                }
                return Ok(Some(self.state.latest_results.clone()));
            },
            ServerMessage::MutationResponse {
                request_id,
                result,
                ts,
                log_lines,
            } => {
                for log_line in log_lines.0 {
                    convex_logs!("{}", log_line);
                }

                if let Some(ts) = ts {
                    self.observe_timestamp(ts);
                }
                let request_id = RequestId::new(request_id);
                self.request_manager.update_request(
                    &request_id,
                    RequestType::Mutation,
                    result.into(),
                    ts,
                )?;
            },
            ServerMessage::AuthError {
                error_message,
                base_version,
                ..
            } => {
                tracing::error!(
                    "AuthError: {error_message} for identity version {base_version:?}. Restarting \
                     protocol."
                );
                return Err(format!(
                    "AuthError: {error_message} for identity version {base_version:?}"
                ));
            },
            ServerMessage::FatalError { error_message } => {
                tracing::error!("FatalError: {error_message}. Restarting protocol.");
                return Err(format!("FatalError: {error_message}"));
            },
            ServerMessage::ActionResponse {
                request_id,
                result,
                log_lines,
            } => {
                for log_line in log_lines.0 {
                    convex_logs!("{}", log_line);
                }
                let request_id = RequestId::new(request_id);
                self.request_manager.update_request(
                    &request_id,
                    RequestType::Action,
                    result.into(),
                    None,
                )?;
            },
            ServerMessage::Ping => {
                // Do nothing
            },
            ServerMessage::TransitionChunk { .. } => {
                // The Rust client should never receive TransitionChunk messages
                // as this feature is only enabled for npm clients
                return Err("Unexpected TransitionChunk message received".to_string());
            },
        }
        Ok(None)
    }

    /// Grab a snapshot of the latest query results to all subscribed queries.
    pub fn latest_results(&self) -> &QueryResults {
        &self.state.latest_results
    }

    /// Resend all subscribed queries and ongoing mutations. Should be used once
    /// the websocket closes and reconnects.
    pub async fn resend_ongoing_queries_mutations(&mut self) {
        // Clear any stale messages from the queue. During reconnection
        // retries, messages can accumulate from previous failed attempts
        // or from subscription changes made while disconnected. Since
        // restart() rebuilds the full query set and resets version
        // numbers, any pre-existing messages would have stale versions
        // that conflict with the fresh restart messages.
        self.outgoing_message_queue.clear();

        let state_restart_messages = self.state.restart().await;
        let mut ongoing_mutation_messages = self.request_manager.restart();

        self.remote_query_set = RemoteQuerySet::new();
        for state_restart_message in state_restart_messages {
            self.outgoing_message_queue.push_back(state_restart_message);
        }
        self.outgoing_message_queue
            .append(&mut ongoing_mutation_messages);
    }

    fn on_query_result_changes(
        &mut self,
        completed_requests: BTreeSet<RequestId>,
    ) -> Result<BTreeMap<QueryId, FunctionResult>, ReconnectProtocolReason> {
        let remote_query_results = &self.remote_query_set.remote_query_set;
        let mut query_id_to_value = BTreeMap::new();
        for (query_id, result) in remote_query_results.iter() {
            let Some(_udf_path) = self.state.query_path(*query_id) else {
                // It's possible that we've already unsubscribed to this query but
                // the server hasn't learned about that yet. If so, ignore this one.
                continue;
            };
            let _args = self
                .state
                .query_args(*query_id)
                .expect("INTERNAL BUG: Query args exist, but not query path.");
            query_id_to_value.insert(
                *query_id,
                Query {
                    result: result.clone(),
                    _udf_path,
                    _args,
                },
            );
        }
        Ok(self
            .optimistic_query_results
            .ingest_query_results_from_server(query_id_to_value, completed_requests))
    }

    fn local_query_result(&self, query_id: QueryId) -> Option<FunctionResult> {
        self.optimistic_query_results.query_result(query_id)
    }
}

/// Macro used for piping UDF logs to a custom formatter that exposes
/// just the log content, without any additional Rust metadata.
#[macro_export]
macro_rules! convex_logs {
    (target: $target:expr, $($arg:tt)+) => {
        tracing::event!(target: "convex_logs", tracing::Level::DEBUG, $($arg)+);
        // Additional custom behavior can be added here
    };
    ($($arg:tt)+) => {
        tracing::event!(target: "convex_logs", tracing::Level::DEBUG, $($arg)+);
        // Additional custom behavior can be added here
    };
}
