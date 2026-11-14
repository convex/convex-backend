use std::{
    cmp::Reverse,
    collections::{
        BTreeMap,
        BTreeSet,
        VecDeque,
    },
};

use convex_sync_types::{
    ClientMessage,
    Timestamp,
};
use tokio::sync::oneshot;

use crate::{
    sync::ReconnectProtocolReason,
    FunctionResult,
};

#[derive(Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Debug)]
pub struct RequestId(u32);
impl RequestId {
    pub fn new(id: u32) -> Self {
        RequestId(id)
    }
}

#[derive(Copy, Clone, PartialEq, PartialOrd, Ord, Eq)]
pub enum RequestType {
    Mutation,
    Action,
}

#[derive(Clone, PartialEq, PartialOrd, Ord, Eq)]
pub enum RequestStatus {
    Requested,
    Completed,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    pub id: RequestId,
    pub typ: RequestType,
    pub status: RequestStatus,
    pub ts: Option<Timestamp>,
    pub value: Option<FunctionResult>,
    pub message: ClientMessage,
}

impl Request {
    pub fn new(id: RequestId, typ: RequestType, message: ClientMessage) -> Self {
        Request {
            id,
            typ,
            status: RequestStatus::Requested,
            ts: None,
            value: None,
            message,
        }
    }

    pub fn update_value(&mut self, value: FunctionResult) {
        self.value = Some(value);
    }

    pub fn update_timestamp(&mut self, ts: Option<Timestamp>) {
        self.ts = ts;
    }
}

pub struct RequestManager {
    ongoing_requests: BTreeMap<RequestId, (Request, oneshot::Sender<FunctionResult>)>,
}

impl RequestManager {
    pub fn new() -> Self {
        RequestManager {
            ongoing_requests: BTreeMap::new(),
        }
    }

    /// Record the server's response to a request.
    ///
    /// Returns whether the request completed immediately (actions and failed
    /// mutations) rather than waiting for a transition past its timestamp.
    ///
    /// Responses to requests that are no longer tracked or already completed
    /// are ignored. They happen when a mutation is resent after a reconnect:
    /// the first response is kept, as in the JS client's `request_manager.ts`.
    pub fn update_request(
        &mut self,
        request_id: &RequestId,
        request_type: RequestType,
        value: FunctionResult,
        ts: Option<Timestamp>,
    ) -> Result<bool, ReconnectProtocolReason> {
        let Some((request, _)) = self.ongoing_requests.get_mut(request_id) else {
            tracing::debug!("Ignoring response to request {request_id:?}, which already completed");
            return Ok(false);
        };
        if request.typ != request_type {
            return Err("Mismatched request type from server".to_string());
        };
        if request.status == RequestStatus::Completed {
            tracing::debug!("Ignoring duplicate response to request {request_id:?}");
            return Ok(false);
        }
        let errored = matches!(
            value,
            FunctionResult::ErrorMessage(_) | FunctionResult::ConvexError(_)
        );
        request.update_value(value);
        request.update_timestamp(ts);
        request.status = RequestStatus::Completed;

        // Actions and errored mutations are ok to complete immediately
        if request_type == RequestType::Action || errored {
            self._remove_and_notify_completed(request_id);
            return Ok(true);
        }
        Ok(false)
    }

    /// Removes and notifies every completed request whose result is visible
    /// at `ts`, the timestamp of the latest query results the client has
    /// received from the server. Returns the IDs of the removed requests.
    pub fn remove_and_notify_completed(&mut self, ts: Timestamp) -> BTreeSet<RequestId> {
        let mut completed_requests = BTreeSet::new();
        for (id, (request, _)) in self.ongoing_requests.iter() {
            // A mutation's result is only visible in query results once a
            // transition reaches the timestamp it committed at.
            let is_completed = request.status == RequestStatus::Completed
                && request.ts.is_none_or(|request_ts| request_ts <= ts);
            if is_completed {
                completed_requests.insert(*id);
            }
        }
        for id in completed_requests.iter() {
            self._remove_and_notify_completed(id);
        }
        completed_requests
    }

    fn _remove_and_notify_completed(&mut self, request_id: &RequestId) {
        let (request, sender) = self
            .ongoing_requests
            .remove(request_id)
            .expect("INTERNAL BUG: request_id must be present");
        if let Err(value) = sender.send(
            request
                .value
                .expect("INTERNAL BUG: Value missing on completed request"),
        ) {
            tracing::info!(
                "Request {request_id:?} completed with result {value:?}, but result receiver was \
                 dropped"
            );
        }
    }

    pub fn track_request(
        &mut self,
        message: &ClientMessage,
        request_id: RequestId,
        request_type: RequestType,
    ) -> oneshot::Receiver<FunctionResult> {
        let (tx, rx) = oneshot::channel();
        let request = Request::new(request_id, request_type, message.clone());
        self.ongoing_requests.insert(request_id, (request, tx));
        rx
    }

    /// Returns the messages to resend after a reconnect.
    ///
    /// Incomplete requests made before `auth_changed_at` fail instead, so they
    /// aren't resent as a different user than the one who made them.
    pub fn restart(&mut self, auth_changed_at: RequestId) -> VecDeque<ClientMessage> {
        let made_before_auth_change: Vec<RequestId> = self
            .ongoing_requests
            .range(..auth_changed_at)
            .filter(|(_, (request, _))| request.status != RequestStatus::Completed)
            .map(|(id, _)| *id)
            .collect();
        for id in made_before_auth_change {
            let (_, sender) = self
                .ongoing_requests
                .remove(&id)
                .expect("INTERNAL BUG: request_id must be present");
            let _ = sender.send(FunctionResult::ErrorMessage(
                "Authentication changed before the request completed".to_string(),
            ));
        }

        // Sort ongoing requests by timestamp
        let mut ordered_requests = Vec::from_iter(self.ongoing_requests.values());
        ordered_requests.sort_by_key(|(req, _)| Reverse(req.ts));

        let mut messages = VecDeque::new();
        for (request, _) in ordered_requests {
            messages.push_back(request.message.clone());
        }
        messages
    }
}
