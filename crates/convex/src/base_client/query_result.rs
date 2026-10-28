use convex_sync_types::{
    types::ErrorPayload,
    QueryId,
};
use imbl::{
    OrdMap,
    OrdSet,
};

use super::SubscriberId;
use crate::{
    ConvexError,
    Value,
};

/// Result of a Convex function (query/mutation/action).
///
/// The function returns a Convex value or an error message string.
#[derive(Clone, Eq, PartialEq)]
pub enum FunctionResult {
    /// The Convex value returned on a successful run of a Convex function
    Value(Value),
    /// The error message of a Convex function run that does not complete
    /// successfully.
    ErrorMessage(String),
    /// The error payload of a Convex function run that doesn't complete
    /// successfully, with an application-level error.
    ConvexError(ConvexError),
}

impl From<Result<Value, ErrorPayload<Value>>> for FunctionResult {
    fn from(result: Result<Value, ErrorPayload<Value>>) -> Self {
        match result {
            Ok(value) => FunctionResult::Value(value),
            Err(ErrorPayload::ErrorData { message, data }) => {
                FunctionResult::ConvexError(ConvexError { message, data })
            },
            Err(ErrorPayload::Message(message)) => FunctionResult::ErrorMessage(message),
        }
    }
}

impl From<FunctionResult> for Result<Value, ErrorPayload<Value>> {
    fn from(result: FunctionResult) -> Self {
        match result {
            FunctionResult::Value(value) => Ok(value),
            FunctionResult::ErrorMessage(error) => Err(ErrorPayload::Message(error)),
            FunctionResult::ConvexError(error) => Err(ErrorPayload::ErrorData {
                message: error.message,
                data: error.data,
            }),
        }
    }
}

impl std::fmt::Debug for FunctionResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FunctionResult::Value(value) => f.debug_tuple("Value").field(value).finish(),
            FunctionResult::ErrorMessage(error) => write!(f, "{error}"),
            FunctionResult::ConvexError(error) => {
                f.debug_tuple("ConvexError").field(error).finish()
            },
        }
    }
}

/// A mapping from [`SubscriberId`] to its current result [`FunctionResult`]
/// for each actively subscribed query.
#[derive(Clone, Default, Debug)]
pub struct QueryResults {
    pub(super) results: OrdMap<QueryId, FunctionResult>,
    pub(super) subscribers: OrdSet<SubscriberId>,
}

impl QueryResults {
    /// Get the [`FunctionResult`] for the given [`SubscriberId`]
    pub fn get(&self, subscriber_id: &SubscriberId) -> Option<&FunctionResult> {
        if !self.subscribers.contains(subscriber_id) {
            return None;
        };
        self.results.get(&subscriber_id.0)
    }

    /// Get the size of the map.
    pub fn len(&self) -> usize {
        self.subscribers.len()
    }

    /// Test whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.subscribers.is_empty()
    }

    /// Get an iterator over the subscriber_id/query_result pairs of the map.
    pub fn iter(&self) -> impl Iterator<Item = (&SubscriberId, Option<&FunctionResult>)> {
        self.subscribers.iter().map(|s| (s, self.results.get(&s.0)))
    }
}
