use std::ops::{
    Deref,
    DerefMut,
};

use anyhow::Context;
use errors::ErrorMetadata;
use serde::Serialize;
use serde_json::Value as JsonValue;
pub use sync_types::{
    SessionId,
    SessionRequestSeqNumber,
    Timestamp,
};
use value::heap_size::{
    HeapSize,
    WithHeapSize,
};

use crate::{
    execution_context::{
        ClientIp,
        ClientUserAgent,
        ExecutionContext,
        RequestId,
    },
    knobs::{
        AUDIT_LOG_MAX_LINES,
        AUDIT_LOG_MAX_LINE_SIZE_BYTES,
        AUDIT_LOG_MAX_TOTAL_SIZE_BYTES,
    },
    runtime::{
        Runtime,
        UnixTimestamp,
    },
};

/// List of user-space audit log lines from a Convex function execution.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct AuditLogLines(WithHeapSize<Vec<AuditLogLine>>);

impl AuditLogLines {
    pub fn resolve_bodies(&self, vars: &AuditLogVars) -> anyhow::Result<ResolvedAuditLogLines> {
        let max_lines = *AUDIT_LOG_MAX_LINES;
        anyhow::ensure!(
            self.0.len() <= max_lines,
            ErrorMetadata::bad_request(
                "TooManyAuditLogLines",
                format!("Function execution exceeded the maximum of {max_lines} audit log lines.")
            )
        );
        let logs = self
            .0
            .iter()
            .map(|log| log.resolve_body(vars))
            .collect::<anyhow::Result<Vec<ResolvedAuditLogLine>>>()?;
        let total_max_size: usize = logs.iter().map(|l| l.max_size).sum();
        let max_total = *AUDIT_LOG_MAX_TOTAL_SIZE_BYTES;
        anyhow::ensure!(
            total_max_size <= max_total,
            ErrorMetadata::bad_request(
                "AuditLogLinesTooLarge",
                format!(
                    "The total maximum possible size of audit log lines from a single function \
                     execution is {max_total} bytes, but this execution could produce up to \
                     {total_max_size} bytes."
                )
            )
        );
        let timestamp = vars.now;
        Ok(ResolvedAuditLogLines { logs, timestamp })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AuditLogLine {
    pub body: JsonValue,
}

#[derive(Clone, Debug)]
pub struct ResolvedAuditLogLines {
    pub logs: Vec<ResolvedAuditLogLine>,
    /// This timestamp is only used when emitting to log streams
    pub timestamp: UnixTimestamp,
}

impl ResolvedAuditLogLines {
    pub fn to_json_strings(self) -> anyhow::Result<Vec<String>> {
        self.logs
            .into_iter()
            .map(|l| serde_json::to_string(&l.into_value()).map_err(anyhow::Error::from))
            .collect()
    }
}

/// A resolved audit log line whose body has all sentinel objects replaced
/// with concrete values.
#[derive(Clone, Debug)]
pub struct ResolvedAuditLogLine {
    body: JsonValue,
    /// Upper bound on the serialized size of `body`
    max_size: usize,
}

impl ResolvedAuditLogLine {
    pub fn into_value(self) -> JsonValue {
        self.body
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConvexActorVar {
    Token {
        member_id: Option<u64>,
        token_id: u64,
        client_id: Option<String>,
    },
    Member {
        member_id: u64,
    },
}

#[derive(Serialize)]
pub struct AuditLogVars {
    request_id: RequestId,
    ip: Option<ClientIp>,
    user_agent: Option<ClientUserAgent>,
    now: UnixTimestamp,
    convex_actor: Option<ConvexActorVar>,
}

impl AuditLogVars {
    /// The maximum length (in bytes) of any single audit log var when
    /// serialized to JSON.
    // 1026 = 2 * ClientUserAgent::MAX_LENGTH + 2 surrounding JSON quotes. The
    // worst-case is a User-Agent consisting of 512 characters that need to be
    // escaped (like " or \)
    pub const MAX_VAR_LENGTH: usize = 1026;

    pub fn from_context(
        context: ExecutionContext,
        convex_actor: Option<ConvexActorVar>,
        rt: &impl Runtime,
    ) -> anyhow::Result<Self> {
        let vars = AuditLogVars {
            ip: context.request_metadata.ip,
            request_id: context.request_id,
            now: rt.unix_timestamp(),
            user_agent: context.request_metadata.user_agent,
            convex_actor,
        };
        vars.check_var_lengths()?;
        Ok(vars)
    }

    fn check_var_lengths(&self) -> anyhow::Result<()> {
        let Self {
            request_id,
            ip,
            user_agent,
            now,
            convex_actor,
        } = self;
        let serialized = [
            ("requestId", serde_json::to_string(request_id)?),
            ("ip", serde_json::to_string(ip)?),
            ("userAgent", serde_json::to_string(user_agent)?),
            ("now", serde_json::to_string(&now.as_ms_since_epoch()?)?),
            ("convexActor", serde_json::to_string(convex_actor)?),
        ];
        for (name, s) in serialized {
            anyhow::ensure!(
                s.len() <= Self::MAX_VAR_LENGTH,
                "Audit log var \"{name}\" serialized length {} exceeds max {}. This should be \
                 impossible.",
                s.len(),
                Self::MAX_VAR_LENGTH,
            );
        }
        Ok(())
    }
}

impl AuditLogLine {
    /// Resolve all `{ "$var": "<name>" }` sentinel objects in the body,
    /// returning a [`ResolvedAuditLogLine`] with the substitutions applied.
    pub fn resolve_body(&self, vars: &AuditLogVars) -> anyhow::Result<ResolvedAuditLogLine> {
        let body_size = serde_json::to_string(&self.body)?.len();
        let mut body = self.body.clone();
        let num_vars = resolve_vars(&mut body, vars)?;
        // Slight overcount: each sentinel's serialized size (e.g. `{"$var":"now"}`) is
        // already counted in `body_size` and again as part of the per-var budget.
        let max_size = body_size + num_vars * AuditLogVars::MAX_VAR_LENGTH;
        let max_line = *AUDIT_LOG_MAX_LINE_SIZE_BYTES;
        anyhow::ensure!(
            max_size <= max_line,
            ErrorMetadata::bad_request(
                "AuditLogLineTooLarge",
                format!(
                    "An audit log line may have a maximum possible size of {max_line} bytes, but \
                     this line could be up to {max_size} bytes."
                )
            )
        );
        Ok(ResolvedAuditLogLine { body, max_size })
    }
}

/// Check if a JSON value is a `{ "$var": "<name>" }` sentinel and return the
/// var name if so.
fn as_var_sentinel(value: &JsonValue) -> Option<&str> {
    let obj = value.as_object()?;
    if obj.len() != 1 {
        return None;
    }
    obj.get("$var")?.as_str()
}

/// Resolve all the vars in the given value and return the total number of vars
fn resolve_vars(value: &mut JsonValue, vars: &AuditLogVars) -> anyhow::Result<usize> {
    let AuditLogVars {
        request_id,
        ip,
        user_agent,
        now,
        convex_actor,
    } = vars;
    let mut num_vars = 0;
    if let Some(var_name) = as_var_sentinel(value) {
        match var_name {
            "requestId" => *value = serde_json::to_value(request_id)?,
            "ip" => *value = serde_json::to_value(ip)?,
            "userAgent" => *value = serde_json::to_value(user_agent)?,
            "now" => *value = serde_json::to_value(now.as_ms_since_epoch()?)?,
            "convexActor" => *value = serde_json::to_value(convex_actor)?,
            _ => anyhow::bail!(ErrorMetadata::bad_request(
                "UnknownAuditLogVar",
                format!("Unknown audit log variable: \"{var_name}\""),
            )),
        }
        return Ok(1);
    }
    match value {
        JsonValue::Object(map) => {
            for v in map.values_mut() {
                num_vars += resolve_vars(v, vars)?;
            }
        },
        JsonValue::Array(arr) => {
            for v in arr.iter_mut() {
                num_vars += resolve_vars(v, vars)?;
            }
        },
        _ => {},
    }
    Ok(num_vars)
}

impl Deref for AuditLogLines {
    type Target = WithHeapSize<Vec<AuditLogLine>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for AuditLogLines {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Vec<AuditLogLine>> for AuditLogLines {
    fn from(value: Vec<AuditLogLine>) -> Self {
        Self(value.into())
    }
}

impl IntoIterator for AuditLogLines {
    type IntoIter = <Vec<AuditLogLine> as IntoIterator>::IntoIter;
    type Item = AuditLogLine;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl FromIterator<AuditLogLine> for AuditLogLines {
    fn from_iter<T: IntoIterator<Item = AuditLogLine>>(iter: T) -> Self {
        Self(iter.into_iter().collect::<Vec<_>>().into())
    }
}

impl HeapSize for AuditLogLine {
    fn heap_size(&self) -> usize {
        self.body.heap_size()
    }
}

impl From<AuditLogLine> for pb::outcome::AuditLogLine {
    fn from(value: AuditLogLine) -> Self {
        pb::outcome::AuditLogLine {
            body_json: Some(value.body.to_string()),
        }
    }
}

impl TryFrom<pb::outcome::AuditLogLine> for AuditLogLine {
    type Error = anyhow::Error;

    fn try_from(value: pb::outcome::AuditLogLine) -> Result<Self, Self::Error> {
        Ok(AuditLogLine {
            body: value.body_json.context("Missing body")?.parse()?,
        })
    }
}
