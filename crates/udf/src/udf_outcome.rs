use std::time::Duration;

use anyhow::Context;
use common::{
    audit_log_lines::{
        AuditLogLine,
        AuditLogLines,
    },
    components::CanonicalizedComponentFunctionPath,
    errors::JsError,
    identity::InertIdentity,
    log_lines::{
        LogLine,
        LogLines,
    },
    query_journal::QueryJournal,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
};
use pb::{
    common::{
        function_result::Result as FunctionResultTypeProto,
        FunctionResult as FunctionResultProto,
    },
    outcome::UdfOutcome as UdfOutcomeProto,
};
use rand::Rng;
use sync_types::types::SerializedArgs;
use value::{
    heap_size::HeapSize,
    JsonPackedValue,
    PendingValue,
};

use crate::{
    validation::ValidatedPathAndArgs,
    SyscallTrace,
};

#[derive(Debug, Clone)]
pub struct UdfOutcome {
    pub path: CanonicalizedComponentFunctionPath,
    pub arguments: SerializedArgs,
    pub identity: InertIdentity,
    pub observed_identity: bool,

    pub rng_seed: [u8; 32],
    pub observed_rng: bool,

    pub unix_timestamp: UnixTimestamp,
    pub observed_time: bool,

    pub log_lines: LogLines,
    pub journal: QueryJournal,
    pub audit_log_lines: AuditLogLines,

    // QueryUdfOutcomes are stored in the Udf level cache, which is why we would like
    // them to have more compact representation.
    //
    // Mutations and subqueries called by mutations can have unresolved commit_ts, but top-level
    // queries should be concrete.
    pub result: Result<JsonPackedValue<PendingValue>, JsError>,

    pub syscall_trace: SyscallTrace,

    pub udf_server_version: Option<semver::Version>,
    pub memory_in_mb: u64,
    // TODO(ENG-10204): Make required
    pub user_execution_time: Option<Duration>,
}

impl HeapSize for UdfOutcome {
    fn heap_size(&self) -> usize {
        self.path.udf_path.heap_size()
            + self.arguments.heap_size()
            + self.identity.heap_size()
            + self.log_lines.heap_size()
            + self.journal.heap_size()
            + self.result.heap_size()
            + self.syscall_trace.heap_size()
    }
}

#[derive(Debug, Clone)]
pub struct NestedUdfOutcome {
    pub observed_identity: bool,
    pub observed_rng: bool,
    pub observed_time: bool,
    pub log_lines: LogLines,
    pub audit_log_lines: AuditLogLines,
    pub journal: QueryJournal,
    pub result: Result<PendingValue, JsError>,
    pub syscall_trace: SyscallTrace,
}

impl TryFrom<UdfOutcome> for UdfOutcomeProto {
    type Error = anyhow::Error;

    fn try_from(
        UdfOutcome {
            path: _,
            arguments: _,
            identity: _,
            observed_identity,
            rng_seed,
            observed_rng,
            unix_timestamp,
            observed_time,
            log_lines,
            audit_log_lines,
            journal,
            result,
            syscall_trace,
            udf_server_version: _,
            memory_in_mb,
            user_execution_time,
        }: UdfOutcome,
    ) -> anyhow::Result<Self> {
        let result = match result {
            Ok(value) => FunctionResultTypeProto::JsonPackedValue(value.as_str().to_string()),
            Err(js_error) => FunctionResultTypeProto::JsError(js_error.try_into()?),
        };
        Ok(Self {
            rng_seed: Some(rng_seed.to_vec()),
            observed_rng: Some(observed_rng),
            unix_timestamp: Some(unix_timestamp.into()),
            observed_time: Some(observed_time),
            log_lines: log_lines.into_iter().map(|l| l.into()).collect(),
            audit_log_lines: audit_log_lines.into_iter().map(|l| l.into()).collect(),
            journal: Some(journal.into()),
            result: Some(FunctionResultProto {
                result: Some(result),
            }),
            syscall_trace: Some(syscall_trace.try_into()?),
            observed_identity: Some(observed_identity),
            memory_in_mb,
            user_execution_time: user_execution_time.map(|t| t.try_into()).transpose()?,
        })
    }
}

impl UdfOutcome {
    /// Used for synthesizing an outcome when we encounter an error before
    /// reaching the isolate.
    pub fn from_error(
        js_error: JsError,
        path: CanonicalizedComponentFunctionPath,
        arguments: SerializedArgs,
        identity: InertIdentity,
        rt: impl Runtime,
        udf_server_version: Option<semver::Version>,
    ) -> anyhow::Result<Self> {
        Ok(UdfOutcome {
            path,
            arguments,
            identity,
            rng_seed: rt.rng().random(),
            observed_rng: false,
            unix_timestamp: rt.unix_timestamp(),
            observed_time: false,
            log_lines: vec![].into(),
            audit_log_lines: vec![].into(),
            journal: QueryJournal::new(),
            result: Err(js_error),
            syscall_trace: SyscallTrace::new(),
            udf_server_version,
            observed_identity: false,
            memory_in_mb: 0,
            user_execution_time: Some(Duration::ZERO),
        })
    }

    pub(crate) fn from_proto(
        UdfOutcomeProto {
            rng_seed,
            observed_rng,
            unix_timestamp,
            observed_time,
            log_lines,
            audit_log_lines,
            journal,
            result,
            syscall_trace,
            observed_identity,
            memory_in_mb,
            user_execution_time,
        }: UdfOutcomeProto,
        path_and_args: ValidatedPathAndArgs,
        identity: InertIdentity,
    ) -> anyhow::Result<Self> {
        let rng_seed = rng_seed.context("Missing rng_seed")?;
        let rng_seed = rng_seed
            .as_slice()
            .try_into()
            .context("Invalid rng_seed length")?;
        let result = result.context("Missing result")?;
        let result = match result.result {
            Some(FunctionResultTypeProto::JsonPackedValue(value)) => {
                Ok(JsonPackedValue::from_network(value)?)
            },
            Some(FunctionResultTypeProto::JsError(js_error)) => Err(js_error.try_into()?),
            None => anyhow::bail!("Missing result"),
        };
        let (path, arguments, udf_server_version) = path_and_args.consume();
        let log_lines = log_lines.into_iter().map(LogLine::try_from).try_collect()?;
        let audit_log_lines = audit_log_lines
            .into_iter()
            .map(AuditLogLine::try_from)
            .try_collect()?;
        Ok(Self {
            path: path.for_logging(),
            arguments,
            identity,
            rng_seed,
            observed_rng: observed_rng.unwrap_or_default(),
            unix_timestamp: unix_timestamp
                .context("Missing unix_timestamp")?
                .try_into()?,
            observed_time: observed_time.unwrap_or_default(),
            log_lines,
            audit_log_lines,
            journal: journal.context("Missing journal")?.try_into()?,
            result,
            syscall_trace: syscall_trace.context("Missing syscall_trace")?.try_into()?,
            udf_server_version,
            observed_identity: observed_identity.context("Missing identity")?,
            memory_in_mb,
            user_execution_time: user_execution_time.map(|d| d.try_into()).transpose()?,
        })
    }
}
