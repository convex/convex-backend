use std::{
    collections::BTreeMap,
    fmt::Display,
    hash::{
        Hash,
        Hasher,
    },
    ops::Deref,
};

use derive_more::{
    Deref,
    FromStr,
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    value::RawValue,
    Value as JsonValue,
};
use strum::AsRefStr;
use uuid::Uuid;

use crate::{
    Timestamp,
    UdfPath,
};

mod json;

#[derive(
    Copy, Clone, Debug, Default, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize, Hash,
)]
pub struct QueryId(u32);

impl QueryId {
    pub fn new(id: u32) -> Self {
        QueryId(id)
    }

    pub fn get_id(&self) -> u32 {
        self.0
    }
}

impl Display for QueryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Eq,
    PartialEq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    Hash,
    derive_more::From,
    derive_more::Into,
    derive_more::Display,
    derive_more::Deref,
)]
pub struct QuerySetVersion(u32);
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Eq,
    PartialEq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    Hash,
    derive_more::From,
    derive_more::Into,
    derive_more::Display,
    derive_more::Deref,
)]
pub struct IdentityVersion(u32);

impl QuerySetVersion {
    pub fn incr(&mut self) -> Self {
        self.0 += 1;
        *self
    }
}
impl IdentityVersion {
    pub fn incr(&mut self) -> Self {
        self.0 += 1;
        *self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Query {
    pub query_id: QueryId,
    pub udf_path: UdfPath,
    pub args: SerializedArgs,

    /// Query journals are only specified on reconnect. Also old clients
    /// (<=0.2.1) don't send them.
    pub journal: Option<SerializedQueryJournal>,

    /// For internal use by Convex dashboard. Only works with admin auth.
    /// Allows calling a query within a component directly.
    pub component_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuerySetModification {
    Add(Query),
    Remove { query_id: QueryId },
}

#[derive(Clone, Debug)]
pub struct SerializedArgs(
    Box<RawValue>,
);

impl PartialEq for SerializedArgs {
    fn eq(&self, other: &Self) -> bool {
        self.0.get() == other.0.get()
    }
}

impl Eq for SerializedArgs {}
impl Hash for SerializedArgs {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.get().hash(state);
    }
}

impl SerializedArgs {
    /// `value` should be a valid serialized `ConvexArray`; this is unchecked
    pub fn from_raw(value: Box<RawValue>) -> Self {
        Self(value)
    }

    pub fn from_args(value: Vec<JsonValue>) -> Result<Self, serde_json::Error> {
        let raw_value = serde_json::value::to_raw_value(&value)?;
        Ok(Self(raw_value))
    }

    pub fn from_slice(value: &[u8]) -> Result<Self, serde_json::Error> {
        Ok(Self(serde_json::from_slice(value)?))
    }

    pub fn heap_size(&self) -> usize {
        self.0.get().len()
    }

    pub fn get(&self) -> &str {
        self.0.get()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        <Box<str>>::from(self.0).into_boxed_bytes().into_vec()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, AsRefStr)]
pub enum ClientMessage {
    Connect {
        session_id: SessionId,
        connection_count: u32,
        last_close_reason: String,
        max_observed_timestamp: Option<Timestamp>,
        client_ts: Option<u64>,
    },
    ModifyQuerySet {
        base_version: QuerySetVersion,
        new_version: QuerySetVersion,
        modifications: Vec<QuerySetModification>,
    },
    Mutation {
        request_id: SessionRequestSeqNumber,
        udf_path: UdfPath,
        args: SerializedArgs,
        /// For internal use by Convex dashboard. Only works with admin auth.
        /// Allows calling a mutation within a component directly.
        component_path: Option<String>,
    },
    Action {
        request_id: SessionRequestSeqNumber,
        udf_path: UdfPath,
        args: SerializedArgs,
        /// For internal use by Convex dashboard. Only works with admin auth.
        /// Allows calling an action within a component directly.
        component_path: Option<String>,
    },
    Authenticate {
        base_version: IdentityVersion,
        token: AuthenticationToken,
    },
    Event(ClientEvent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientEvent {
    pub event_type: String,
    pub event: JsonValue,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserIdentifier(pub String);
impl UserIdentifier {
    pub fn construct(issuer_name: &str, subject: &str) -> Self {
        Self(format!("{issuer_name}|{subject}"))
    }
}

impl Deref for UserIdentifier {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// TODO: Make issuer and subject not optional to match TypeScript
// type and runtime behavior. Requires all FunctionTesters
// to require them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UserIdentityAttributes {
    pub token_identifier: UserIdentifier,
    pub issuer: Option<String>,
    pub subject: Option<String>,
    pub name: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: Option<String>,
    pub preferred_username: Option<String>,
    pub profile_url: Option<String>,
    pub picture_url: Option<String>,
    pub website_url: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub gender: Option<String>,
    pub birthday: Option<String>,
    pub timezone: Option<String>,
    pub language: Option<String>,
    pub phone_number: Option<String>,
    pub phone_number_verified: Option<bool>,
    pub address: Option<String>,
    /// Stored as RFC3339 string
    pub updated_at: Option<String>,

    pub custom_claims: BTreeMap<String, String>,
}

impl Default for UserIdentityAttributes {
    fn default() -> Self {
        UserIdentityAttributes {
            token_identifier: UserIdentifier::construct("convex", "fake_user"),
            subject: None,
            issuer: None,
            name: None,
            email: None,
            given_name: None,
            family_name: None,
            nickname: None,
            preferred_username: None,
            profile_url: None,
            picture_url: None,
            website_url: None,
            email_verified: None,
            gender: None,
            birthday: None,
            timezone: None,
            language: None,
            phone_number: None,
            phone_number_verified: None,
            address: None,
            updated_at: None,
            custom_claims: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub enum AuthenticationToken {
    /// Admin key issued by a KeyBroker, potentially acting as a user.
    Admin(String, Option<UserIdentityAttributes>),
    /// OpenID Connect JWT
    User(String),
    #[default]
    /// Logged out.
    None,
}

/// The serialized representation of the query journal for pagination.
pub type SerializedQueryJournal = Option<String>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateModification<V> {
    QueryUpdated {
        query_id: QueryId,
        value: V,
        log_lines: LogLinesMessage,
        journal: SerializedQueryJournal,
    },
    QueryFailed {
        query_id: QueryId,
        error_message: String,
        log_lines: LogLinesMessage,
        journal: SerializedQueryJournal,
        error_data: Option<V>,
    },
    QueryRemoved {
        query_id: QueryId,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct StateVersion {
    pub query_set: QuerySetVersion,
    pub identity: IdentityVersion,
    pub ts: Timestamp,
}

impl StateVersion {
    pub fn initial() -> Self {
        Self {
            query_set: QuerySetVersion::default(),
            identity: IdentityVersion::default(),
            ts: Timestamp::MIN,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerMessage<V: 'static> {
    Transition {
        start_version: StateVersion,
        end_version: StateVersion,
        modifications: Vec<StateModification<V>>,
        /// The difference between the timestamp in `ClientMessage::Connect` and
        /// the timestamp when the client message was received by the server.
        client_clock_skew: Option<i64>,
        /// The timestamp right before this message was sent back to the
        /// client.
        server_ts: Option<Timestamp>,
    },
    TransitionChunk {
        /// The chunk of the serialized Transition message.
        chunk: String,
        /// 0-indexed part number.
        part_number: u32,
        /// Total number of parts for this transition.
        total_parts: u32,
        /// All chunks of the same transition share this ID.
        transition_id: String,
    },
    MutationResponse {
        request_id: SessionRequestSeqNumber,
        result: Result<V, ErrorPayload<V>>,
        ts: Option<Timestamp>,
        log_lines: LogLinesMessage,
    },
    ActionResponse {
        request_id: SessionRequestSeqNumber,
        result: Result<V, ErrorPayload<V>>,
        log_lines: LogLinesMessage,
    },
    AuthError {
        error_message: String,
        base_version: Option<IdentityVersion>,
        // We want to differentiate between "updating auth starting at version `base_version`
        // failed" and "auth at version `base_version` is invalid" (e.g. it expired)
        auth_update_attempted: Option<bool>,
    },
    FatalError {
        error_message: String,
    },
    Ping,
}

impl<V: 'static> ServerMessage<V> {
    pub fn inject_server_ts(&mut self, ts: Timestamp) {
        match self {
            Self::Transition { server_ts, .. } => *server_ts = Some(ts),
            _ => {},
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ErrorPayload<V: 'static> {
    /// From any error, redacted from prod deployments.
    Message(String),
    /// From ConvexError, never redacted.
    /// `message` is generic, partly for backwards compatibility.
    ErrorData { message: String, data: V },
}

impl<V: 'static> ErrorPayload<V> {
    pub fn get_message(&self) -> &str {
        match self {
            ErrorPayload::Message(message) => message,
            ErrorPayload::ErrorData { message, .. } => message,
        }
    }

    pub fn get_data(&self) -> Option<&V> {
        match self {
            ErrorPayload::Message(..) => None,
            ErrorPayload::ErrorData { message: _, data } => Some(data),
        }
    }
}

/// List of log lines from a Convex function execution.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LogLinesMessage(pub Vec<String>);

#[derive(Copy, Clone, Debug, Deref, Eq, FromStr, PartialEq)]
pub struct SessionId(Uuid);

impl SessionId {
    pub fn new(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<SessionId> for Uuid {
    fn from(id: SessionId) -> Self {
        id.0
    }
}

// The seq number of a request with a session. Uniquely identifies a
// modification request within a session.
pub type SessionRequestSeqNumber = u32;
