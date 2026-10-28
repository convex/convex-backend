pub mod module_loader;
mod performance;
mod promise;
pub mod syscall_error;
mod version;

use std::ops::Deref;

use anyhow::Context;
use common::{
    log_lines::{
        LogLevel,
        LogLine,
    },
    runtime::UnixTimestamp,
};
use deno_core::{
    serde_v8,
    v8,
};
use errors::{
    ErrorCode,
    ErrorMetadata,
};
use serde_json::Value as JsonValue;
use value::TableName;

pub use self::{
    performance::{
        performance_unsupported,
        PerformanceApi,
    },
    promise::{
        resolve_promise,
        resolve_promise_allow_all_errors,
    },
    version::parse_version,
};

pub const MAX_LOG_LINE_LENGTH: usize = 32768;

/// The `[ERROR]` line a function's logs end with once it logs past
/// `max_log_lines`. It occupies the last of those slots, so callers stop
/// accepting user lines at `max_log_lines - 1`.
pub fn log_overflow_log_line(max_log_lines: usize, timestamp: UnixTimestamp) -> LogLine {
    LogLine::new_developer_log_line(
        LogLevel::Error,
        vec![format!(
            "Log overflow (maximum {max_log_lines}). Remaining log lines omitted."
        )],
        timestamp,
    )
}

#[derive(Debug, derive_more::Display)]
pub struct ArgName(pub &'static str);

pub fn with_argument_error<T>(
    name: &str,
    f: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    f().map_err(|e| {
        anyhow::anyhow!(ErrorMetadata::bad_request(
            "InvalidArgument",
            if let Some(ArgName(arg_name)) = e.downcast_ref() {
                if let Some(cause) = e.chain().nth(1) {
                    format!("Invalid argument `{arg_name}` for `{name}`: {cause}")
                } else {
                    format!("Invalid argument `{arg_name}` for `{name}`: {e}")
                }
            } else {
                format!("Invalid arguments for `{name}`: {e}")
            }
        ))
    })
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Phase {
    Importing,
    Executing,
}

pub fn json_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    json: JsonValue,
) -> anyhow::Result<v8::Local<'s, v8::Value>> {
    let value_v8 = serde_v8::to_v8(scope, json)?;
    Ok(value_v8)
}

/// Convert `RejectedBeforeExecution` error codes into `Overloaded`.
/// This is useful when calling nested UDFs as the code would otherwise leak out
/// of the _parent_ UDF, causing its caller to mistakenly believe the parent
/// call to be retriable.
pub fn remove_rejected_before_execution(mut e: anyhow::Error) -> anyhow::Error {
    if let Some(em) = e.downcast_mut::<ErrorMetadata>()
        && em.code == ErrorCode::RejectedBeforeExecution
    {
        em.code = ErrorCode::Overloaded;
    }
    e
}

/// For DB syscalls that take an explicit table name, checks that the
/// explicit table name that the user used (`requested_table_name`)
/// matches the name of the ID’s table (`actual_name_name`).
pub fn check_table_name(
    requested_table_name: &Option<String>,
    actual_table_name: &TableName,
) -> anyhow::Result<()> {
    if let Some(requested_table_name) = requested_table_name
        && requested_table_name != actual_table_name.deref()
    {
        return Err(ErrorMetadata::bad_request(
            "InvalidTable",
            format!(
                "expected to be an Id<\"{}\">, got Id<\"{}\"> instead.",
                requested_table_name,
                actual_table_name.deref()
            ),
        ))
        .context(ArgName("id"));
    }
    Ok(())
}
