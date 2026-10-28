use std::str::FromStr;

use anyhow::Context;
use derive_more::Display;
use rand::Rng;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use sync_types::types::SessionId;
use uuid::Uuid;
use value::{
    heap_size::HeapSize,
    id_v6::DeveloperDocumentId,
    sha256,
};

use crate::{
    components::ComponentId,
    types::FunctionCaller,
};

/// A client IP address extracted from HTTP headers, with max length
/// enforcement.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ClientIp(String);

impl ClientIp {
    pub const MAX_LENGTH: usize = 256;

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for ClientIp {
    fn from(mut value: String) -> Self {
        value.truncate(value.floor_char_boundary(Self::MAX_LENGTH));
        Self(value)
    }
}

/// A client user-agent string extracted from HTTP headers, with max length
/// enforcement.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ClientUserAgent(String);

impl ClientUserAgent {
    pub const MAX_LENGTH: usize = 512;

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for ClientUserAgent {
    fn from(mut value: String) -> Self {
        value.truncate(value.floor_char_boundary(Self::MAX_LENGTH));
        Self(value)
    }
}

/// Metadata about the HTTP request that triggered this function execution.
/// Fields are `None` for system-originated calls (scheduled jobs, cron jobs,
/// internal RPCs, etc.).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestMetadata {
    pub ip: Option<ClientIp>,
    pub user_agent: Option<ClientUserAgent>,
}

impl RequestMetadata {
    /// Create metadata for system-originated requests where there is no
    /// originating HTTP request (e.g. scheduled jobs, cron jobs, internal
    /// RPCs). Analogous to `Identity::system()`.
    pub fn system() -> Self {
        Self {
            ip: None,
            user_agent: None,
        }
    }

}

impl HeapSize for RequestMetadata {
    fn heap_size(&self) -> usize {
        self.ip.as_ref().map_or(0, |ip| ip.as_str().len())
            + self.user_agent.as_ref().map_or(0, |ua| ua.as_str().len())
    }
}

/// Context about the originating request, bundling the request ID with
/// metadata from the HTTP layer (IP, user agent). Threaded from the API
/// boundary down to where `ExecutionContext` is constructed.
#[derive(Clone, Debug)]
pub struct RequestContext {
    pub request_id: RequestId,
    pub request_metadata: RequestMetadata,
}

impl RequestContext {
    pub fn new(request_id: RequestId, request_metadata: RequestMetadata) -> Self {
        Self {
            request_id,
            request_metadata,
        }
    }

    /// Create a request context for system-originated calls that have a
    /// request ID but no HTTP metadata (e.g. cached queries, internal RPCs).
    pub fn new_for_system_request(request_id: RequestId) -> Self {
        Self {
            request_id,
            request_metadata: RequestMetadata::system(),
        }
    }

}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionContext {
    pub request_id: RequestId,
    // A unique ID per entry in the function logs.
    // In contrast to the `RequestId`, there is a 1-1 relationship between a single
    // function ExecutionId.
    pub execution_id: ExecutionId,
    /// The id of the scheduled job that triggered this UDF, if any.
    pub parent_scheduled_job: Option<(ComponentId, DeveloperDocumentId)>,
    /// False if this function was called as part of a request (e.g. action
    /// calling a mutation) TODO: This is a stop gap solution. The richer
    /// version of this would be something like parent_execution_id:
    /// Option<ExecutionId>
    is_root: bool,
    /// Metadata about the originating HTTP request (IP, user agent).
    pub request_metadata: RequestMetadata,
}

impl ExecutionContext {
    pub fn new(request_context: RequestContext, caller: &FunctionCaller) -> Self {
        Self {
            request_id: request_context.request_id,
            execution_id: ExecutionId::new(),
            parent_scheduled_job: caller.parent_scheduled_job(),
            is_root: caller.is_root(),
            request_metadata: request_context.request_metadata,
        }
    }

    pub fn new_from_parts(
        request_id: RequestId,
        execution_id: ExecutionId,
        parent_scheduled_job: Option<(ComponentId, DeveloperDocumentId)>,
        is_root: bool,
        request_metadata: RequestMetadata,
    ) -> Self {
        Self {
            request_id,
            execution_id,
            parent_scheduled_job,
            is_root,
            request_metadata,
        }
    }

    pub fn is_root(&self) -> bool {
        self.is_root
    }

    pub fn add_sentry_tags(&self, scope: &mut sentry::Scope) {
        scope.set_tag("request_id", &self.request_id);
        scope.set_tag("execution_id", self.execution_id);
    }
}

impl HeapSize for ExecutionContext {
    fn heap_size(&self) -> usize {
        self.request_id.heap_size()
            + self.execution_id.heap_size()
            + self
                .parent_scheduled_job
                .map_or(0, |(_, document_id)| document_id.heap_size())
            + self.is_root.heap_size()
            + self.request_metadata.heap_size()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Display, Serialize, Deserialize)]
#[display("{_0}")]
#[serde(transparent)]
pub struct RequestId(String);

impl RequestId {
    pub fn new() -> Self {
        let bytes = rand::rng().random::<[u8; 8]>();
        Self(const_hex::encode(bytes))
    }

    // This produces a RequestId based off of information provided by a WS client
    // via our sync protocol.
    pub fn new_for_ws_session(session_id: SessionId, ws_request_counter: u32) -> Self {
        let mut hash =
            sha256::Sha256::hash(format!("{}|{ws_request_counter}", *session_id).as_bytes())
                .as_hex();
        hash.truncate(16);
        Self(hash)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<RequestId> for String {
    fn from(value: RequestId) -> Self {
        value.0
    }
}

impl FromStr for RequestId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(RequestId(s.to_owned()))
    }
}

impl TryFrom<String> for RequestId {
    type Error = anyhow::Error;

    fn try_from(value: String) -> anyhow::Result<Self> {
        Ok(RequestId(value))
    }
}

impl HeapSize for RequestId {
    fn heap_size(&self) -> usize {
        self.0.heap_size()
    }
}

/// A unique ID per entry in the UDF logs. This can be group suboperations, like
/// database or storage calls, by the containing UDF.
///
/// In contrast to the `RequestId`, there is a 1-1 relationship between a single
/// UDF and its ExecutionId.
///
/// Execution ids are not meant to be human readable, but they must be globally
/// unique.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Display, Serialize, Deserialize)]
#[display("{_0}")]
#[serde(transparent)]
pub struct ExecutionId(Uuid);

impl Default for ExecutionId {
    fn default() -> Self {
        Self::new()
    }
}

impl FromStr for ExecutionId {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(ExecutionId(value.parse()?))
    }
}

impl ExecutionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl HeapSize for ExecutionId {
    fn heap_size(&self) -> usize {
        self.0.heap_size()
    }
}

impl From<ExecutionContext> for pb::common::ExecutionContext {
    fn from(value: ExecutionContext) -> Self {
        let (parent_component_id, parent_document_id) = value.parent_scheduled_job.unzip();
        pb::common::ExecutionContext {
            request_id: Some(value.request_id.into()),
            execution_id: Some(value.execution_id.to_string()),
            parent_scheduled_job_component_id: parent_component_id
                .and_then(|id| id.serialize_to_string()),
            parent_scheduled_job: parent_document_id.map(Into::into),
            is_root: Some(value.is_root),
            client_ip: value.request_metadata.ip.map(|ip| ip.into_string()),
            client_user_agent: value.request_metadata.user_agent.map(|ua| ua.into_string()),
        }
    }
}

impl TryFrom<pb::common::ExecutionContext> for ExecutionContext {
    type Error = anyhow::Error;

    fn try_from(value: pb::common::ExecutionContext) -> Result<Self, Self::Error> {
        let parent_component_id = ComponentId::deserialize_from_string(
            value.parent_scheduled_job_component_id.as_deref(),
        )?;
        let parent_document_id = value.parent_scheduled_job.map(|s| s.parse()).transpose()?;
        Ok(Self {
            request_id: RequestId::from_str(&value.request_id.context("Missing request id")?)?,
            execution_id: match &value.execution_id {
                Some(e) => ExecutionId::from_str(e)?,
                None => ExecutionId::new(),
            },
            parent_scheduled_job: parent_document_id.map(|id| (parent_component_id, id)),
            is_root: value.is_root.unwrap_or_default(),
            request_metadata: RequestMetadata {
                ip: value.client_ip.map(ClientIp::from),
                user_agent: value.client_user_agent.map(ClientUserAgent::from),
            },
        })
    }
}

impl From<ExecutionContext> for JsonValue {
    fn from(value: ExecutionContext) -> Self {
        let (parent_component_id, parent_document_id) = value.parent_scheduled_job.unzip();
        json!({
            "requestId": String::from(value.request_id),
            "executionId": value.execution_id.to_string(),
            "isRoot": value.is_root,
            "parentScheduledJob": parent_document_id.map(|id| id.to_string()),
            "parentScheduledJobComponentId": parent_component_id.unwrap_or(ComponentId::Root).serialize_to_string(),
            "ip": value.request_metadata.ip.map(|ip| ip.into_string()),
            "userAgent": value.request_metadata.user_agent.map(|ua| ua.into_string()),
        })
    }
}
