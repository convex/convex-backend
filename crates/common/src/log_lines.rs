//! Common types representing database identifiers.
use std::{
    collections::BTreeMap,
    fmt::Display,
    ops::{
        Deref,
        DerefMut,
    },
    str::FromStr,
};

use futures::future::BoxFuture;
use itertools::Itertools;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as JsonValue;
pub use sync_types::{
    SessionId,
    SessionRequestSeqNumber,
    Timestamp,
};
use tokio::sync::mpsc;
use value::{
    heap_size::{
        HeapSize,
        WithHeapSize,
    },
    obj,
    remove_boolean,
    remove_int64,
    remove_nullable_object,
    remove_nullable_string,
    remove_string,
    remove_vec,
    remove_vec_of_strings,
    ConvexObject,
    ConvexValue,
};

use crate::{
    components::CanonicalizedComponentFunctionPath,
    runtime::UnixTimestamp,
};

pub const TRUNCATED_LINE_SUFFIX: &str = " (truncated due to length)";
pub const MAX_LOG_LINE_LENGTH: usize = 32768;
/// List of log lines from a Convex function execution.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct LogLines(WithHeapSize<Vec<LogLine>>);
pub type RawLogLines = WithHeapSize<Vec<String>>;

#[derive(Clone, Debug, PartialEq)]
pub enum LogLevel {
    Debug,
    Error,
    Warn,
    Info,
    Log,
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Debug => f.write_str("DEBUG"),
            LogLevel::Error => f.write_str("ERROR"),
            LogLevel::Warn => f.write_str("WARN"),
            LogLevel::Info => f.write_str("INFO"),
            LogLevel::Log => f.write_str("LOG"),
        }
    }
}

impl FromStr for LogLevel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let v = match s {
            "DEBUG" => LogLevel::Debug,
            "ERROR" => LogLevel::Error,
            "WARN" => LogLevel::Warn,
            "INFO" => LogLevel::Info,
            "LOG" => LogLevel::Log,
            _ => anyhow::bail!("Unknown log level"),
        };
        Ok(v)
    }
}

impl HeapSize for LogLevel {
    fn heap_size(&self) -> usize {
        0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SystemLogMetadata {
    pub code: String,
}

impl HeapSize for SystemLogMetadata {
    fn heap_size(&self) -> usize {
        self.code.heap_size()
    }
}

impl TryFrom<ConvexObject> for SystemLogMetadata {
    type Error = anyhow::Error;

    fn try_from(value: ConvexObject) -> Result<Self, Self::Error> {
        let mut fields = BTreeMap::from(value);
        let code = remove_string(&mut fields, "code")?;
        Ok(SystemLogMetadata { code })
    }
}

impl TryFrom<SystemLogMetadata> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(value: SystemLogMetadata) -> Result<Self, Self::Error> {
        Ok(ConvexValue::Object(obj!("code" => value.code)?))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogLineStructured {
    // Each of these is a string representation of an argument to `console.log`
    // or similar
    pub messages: WithHeapSize<Vec<String>>,
    pub level: LogLevel,
    pub is_truncated: bool,
    pub timestamp: UnixTimestamp,
    pub system_metadata: Option<SystemLogMetadata>,
}

impl LogLineStructured {
    pub fn to_pretty_string(&self) -> String {
        let LogLineStructured {
            messages,
            level,
            is_truncated,
            timestamp: _timestamp,
            system_metadata: _system_metadata,
        } = self;
        if *is_truncated {
            format!("[{level}] {}{TRUNCATED_LINE_SUFFIX}", messages.join(" "))
        } else {
            format!("[{level}] {}", messages.join(" "))
        }
    }

    pub fn to_json(
        self,
        sub_function_path: Option<CanonicalizedComponentFunctionPath>,
        allow_structured: bool,
        include_system_metadata: bool,
    ) -> anyhow::Result<JsonValue> {
        if !allow_structured {
            Ok(JsonValue::String(self.to_pretty_string()))
        } else {
            let LogLineStructured {
                messages,
                level,
                is_truncated,
                timestamp,
                system_metadata,
            } = self;
            let system_metadata = if include_system_metadata {
                system_metadata
            } else {
                None
            };
            let log_line_json = LogLineJson {
                messages: messages.into(),
                is_truncated,
                timestamp: timestamp.as_ms_since_epoch()?,
                level: level.into(),
                system_metadata: system_metadata.map(SystemLogMetadataJson::from),
                component_path: sub_function_path.as_ref().map(|p| p.component.to_string()),
                udf_path: sub_function_path.map(|p| p.udf_path.to_string()),
            };
            Ok(serde_json::to_value(log_line_json)?)
        }
    }

    pub fn new_developer_log_line(
        level: LogLevel,
        messages: Vec<String>,
        timestamp: UnixTimestamp,
    ) -> Self {
        // total length of messages joined by a space
        let total_length = messages
            .iter()
            .map(|m| m.len() + 1)
            .sum::<usize>()
            .saturating_sub(1);
        if total_length <= MAX_LOG_LINE_LENGTH {
            return Self {
                messages: messages.into(),
                level,
                is_truncated: false,
                timestamp,
                system_metadata: None,
            };
        }
        let mut total_length = 0;
        let mut truncated_messages: Vec<String> = vec![];
        for message in messages {
            let remaining_space = MAX_LOG_LINE_LENGTH - total_length;
            if message.len() <= remaining_space {
                total_length += message.len() + 1;
                truncated_messages.push(message);
            } else {
                let last_message =
                    message[..message.floor_char_boundary(remaining_space)].to_string();
                truncated_messages.push(last_message);
                break;
            }
        }
        Self {
            messages: truncated_messages.into(),
            level,
            is_truncated: true,
            timestamp,
            system_metadata: None,
        }
    }
}

impl Deref for LogLines {
    type Target = WithHeapSize<Vec<LogLine>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for LogLines {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Vec<LogLine>> for LogLines {
    fn from(value: Vec<LogLine>) -> Self {
        Self(value.into())
    }
}

impl IntoIterator for LogLines {
    type IntoIter = <Vec<LogLine> as IntoIterator>::IntoIter;
    type Item = LogLine;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl FromIterator<LogLine> for LogLines {
    fn from_iter<T: IntoIterator<Item = LogLine>>(iter: T) -> Self {
        Self(iter.into_iter().collect::<Vec<_>>().into())
    }
}

impl LogLines {
    pub fn to_jsons(
        self,
        allow_structured: bool,
        include_system_metadata: bool,
    ) -> anyhow::Result<Vec<JsonValue>> {
        self.into_iter()
            .map(|log_line| log_line.to_jsons(None, allow_structured, include_system_metadata))
            .flatten_ok()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.0
            .iter()
            .map(|log_line| match log_line {
                LogLine::Structured(_) => 1,
                LogLine::SubFunction { log_lines, .. } => log_lines.len(),
            })
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn truncated(self, mut len: usize) -> Self {
        let mut log_lines = Self::default();
        for log_line in self {
            if len == 0 {
                break;
            }
            match log_line {
                LogLine::Structured(line) => {
                    log_lines.push(LogLine::Structured(line));
                    len -= 1;
                },
                LogLine::SubFunction {
                    path,
                    log_lines: sub_log_lines,
                } => {
                    let sub_log_lines = sub_log_lines.truncated(len);
                    len -= sub_log_lines.len();
                    log_lines.push(LogLine::SubFunction {
                        path,
                        log_lines: sub_log_lines,
                    });
                },
            }
        }
        log_lines
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LogLine {
    Structured(LogLineStructured),
    SubFunction {
        path: CanonicalizedComponentFunctionPath,
        log_lines: LogLines,
    },
}

impl LogLine {
    pub fn to_pretty_strings(self) -> Vec<String> {
        match self {
            LogLine::Structured(log_line) => vec![log_line.to_pretty_string()],
            LogLine::SubFunction { path: _, log_lines } => log_lines
                .into_iter()
                .flat_map(LogLine::to_pretty_strings)
                .collect::<Vec<_>>(),
        }
    }

    pub fn to_jsons(
        self,
        sub_function_path: Option<CanonicalizedComponentFunctionPath>,
        allow_structured: bool,
        include_system_metadata: bool,
    ) -> anyhow::Result<Vec<JsonValue>> {
        if !allow_structured {
            Ok(self
                .to_pretty_strings()
                .into_iter()
                .map(JsonValue::String)
                .collect())
        } else {
            match self {
                LogLine::Structured(log_line) => {
                    let log_line_json = log_line.to_json(
                        sub_function_path,
                        allow_structured,
                        include_system_metadata,
                    )?;
                    Ok(vec![log_line_json])
                },
                LogLine::SubFunction { path, log_lines } => log_lines
                    .into_iter()
                    .map(|log_line| {
                        log_line.to_jsons(
                            Some(path.clone()),
                            allow_structured,
                            include_system_metadata,
                        )
                    })
                    .flatten_ok()
                    .collect(),
            }
        }
    }

    pub fn new_developer_log_line(
        level: LogLevel,
        messages: Vec<String>,
        timestamp: UnixTimestamp,
    ) -> Self {
        LogLine::Structured(LogLineStructured::new_developer_log_line(
            level, messages, timestamp,
        ))
    }

    pub fn new_system_log_line(
        level: LogLevel,
        messages: Vec<String>,
        timestamp: UnixTimestamp,
        system_log_metadata: SystemLogMetadata,
    ) -> Self {
        // Never truncate system log lines
        LogLine::Structured(LogLineStructured {
            messages: messages.into(),
            level,
            is_truncated: false,
            timestamp,
            system_metadata: Some(system_log_metadata),
        })
    }
}

impl HeapSize for LogLine {
    fn heap_size(&self) -> usize {
        match self {
            LogLine::Structured(LogLineStructured {
                messages,
                level,
                timestamp,
                is_truncated,
                system_metadata,
            }) => {
                messages.heap_size()
                    + level.heap_size()
                    + timestamp.heap_size()
                    + is_truncated.heap_size()
                    + system_metadata.heap_size()
            },
            LogLine::SubFunction { path, log_lines } => path.heap_size() + log_lines.heap_size(),
        }
    }
}

impl TryFrom<ConvexValue> for LogLine {
    type Error = anyhow::Error;

    fn try_from(value: ConvexValue) -> Result<Self, Self::Error> {
        let result = match value {
            ConvexValue::Object(o) => {
                let mut fields = BTreeMap::from(o);
                match remove_nullable_string(&mut fields, "kind")?.as_deref() {
                    None => {
                        let messages = remove_vec_of_strings(&mut fields, "messages")?;

                        let is_truncated = remove_boolean(&mut fields, "is_truncated")?;

                        let level = remove_string(&mut fields, "level")?;

                        let timestamp = remove_int64(&mut fields, "timestamp")?;
                        let system_metadata: Option<SystemLogMetadata> =
                            remove_nullable_object(&mut fields, "system_metadata")?;

                        LogLine::Structured(LogLineStructured {
                            messages: messages.into(),
                            is_truncated,
                            level: LogLevel::from_str(&level)?,
                            timestamp: UnixTimestamp::from_millis(timestamp.try_into()?),
                            system_metadata,
                        })
                    },
                    Some("SubFunction") => {
                        let component = remove_string(&mut fields, "component")?;
                        let udf_path = remove_string(&mut fields, "udf_path")?;
                        let log_lines = remove_vec(&mut fields, "log_lines")?;
                        LogLine::SubFunction {
                            path: CanonicalizedComponentFunctionPath {
                                component: component.parse()?,
                                udf_path: udf_path.parse()?,
                            },
                            log_lines: log_lines.into_iter().map(Self::try_from).collect::<Result<
                                LogLines,
                                _,
                            >>(
                            )?,
                        }
                    },
                    _ => anyhow::bail!("unrecognized kind of log line"),
                }
            },
            _ => anyhow::bail!("Invalid value type for log line"),
        };
        Ok(result)
    }
}

impl TryFrom<LogLine> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(value: LogLine) -> Result<Self, Self::Error> {
        let result = match value {
            LogLine::Structured(LogLineStructured {
                messages,
                level,
                is_truncated,
                timestamp,
                system_metadata,
            }) => {
                let timestamp_ms: i64 = timestamp.as_ms_since_epoch()?.try_into()?;
                let system_metadata_value = match system_metadata {
                    Some(m) => ConvexValue::try_from(m)?,
                    None => ConvexValue::Null,
                };
                ConvexValue::Object(obj!(
                    "messages" => messages.into_iter().map(ConvexValue::try_from).try_collect::<_, Vec<_>, _>()?,
                    "level" => level.to_string(),
                    "is_truncated" => is_truncated,
                    "timestamp" => timestamp_ms,
                    "system_metadata" => system_metadata_value,
                )?)
            },
            LogLine::SubFunction { path, log_lines } => ConvexValue::Object(obj!(
                "kind" => "SubFunction",
                "component" => path.component.to_string(),
                "udf_path" => path.udf_path.to_string(),
                "log_lines" => log_lines.into_iter().map(ConvexValue::try_from).try_collect::<_, Vec<_>, _>()?,
            )?),
        };
        Ok(result)
    }
}

// JSON representation of LogLevel with uppercase string serialization
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum LogLevelJson {
    #[serde(rename = "DEBUG")]
    Debug,
    #[serde(rename = "ERROR")]
    Error,
    #[serde(rename = "WARN")]
    Warn,
    #[serde(rename = "INFO")]
    Info,
    #[serde(rename = "LOG")]
    Log,
}

impl From<LogLevel> for LogLevelJson {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Debug => LogLevelJson::Debug,
            LogLevel::Error => LogLevelJson::Error,
            LogLevel::Warn => LogLevelJson::Warn,
            LogLevel::Info => LogLevelJson::Info,
            LogLevel::Log => LogLevelJson::Log,
        }
    }
}

impl From<LogLevelJson> for LogLevel {
    fn from(level: LogLevelJson) -> Self {
        match level {
            LogLevelJson::Debug => LogLevel::Debug,
            LogLevelJson::Error => LogLevel::Error,
            LogLevelJson::Warn => LogLevel::Warn,
            LogLevelJson::Info => LogLevel::Info,
            LogLevelJson::Log => LogLevel::Log,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogLineJson {
    pub messages: Vec<String>,
    pub is_truncated: bool,
    pub timestamp: u64,
    pub level: LogLevelJson,
    pub system_metadata: Option<SystemLogMetadataJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub udf_path: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemLogMetadataJson {
    pub code: String,
}

impl From<SystemLogMetadataJson> for SystemLogMetadata {
    fn from(value: SystemLogMetadataJson) -> SystemLogMetadata {
        SystemLogMetadata { code: value.code }
    }
}

impl From<SystemLogMetadata> for SystemLogMetadataJson {
    fn from(value: SystemLogMetadata) -> SystemLogMetadataJson {
        SystemLogMetadataJson { code: value.code }
    }
}

impl TryFrom<JsonValue> for SystemLogMetadata {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        let json_val: SystemLogMetadataJson = serde_json::from_value(value)?;
        Ok(SystemLogMetadata::from(json_val))
    }
}

impl TryFrom<SystemLogMetadata> for JsonValue {
    type Error = anyhow::Error;

    fn try_from(value: SystemLogMetadata) -> Result<Self, Self::Error> {
        Ok(serde_json::to_value(SystemLogMetadataJson::from(value))?)
    }
}

impl TryFrom<JsonValue> for LogLineStructured {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        let log_line_json: LogLineJson = serde_json::from_value(value)?;
        // Conversion from JsonValue -> LogLineStructured only happens when
        // we know it's not within a SubFunction.
        // Contrast with conversion from LogLineStructured -> JsonValue
        // which *can* happen within a SubFunction.
        anyhow::ensure!(
            log_line_json.component_path.is_none() && log_line_json.udf_path.is_none(),
            "SubFunction not supported"
        );
        Ok(LogLineStructured {
            messages: log_line_json.messages.into(),
            is_truncated: log_line_json.is_truncated,
            timestamp: UnixTimestamp::from_millis(log_line_json.timestamp),
            level: log_line_json.level.into(),
            system_metadata: log_line_json.system_metadata.map(SystemLogMetadata::from),
        })
    }
}

impl TryFrom<LogLineStructured> for JsonValue {
    type Error = anyhow::Error;

    fn try_from(value: LogLineStructured) -> Result<Self, Self::Error> {
        value.to_json(None, true, true)
    }
}

impl From<LogLine> for pb::outcome::LogLine {
    fn from(value: LogLine) -> Self {
        match value {
            LogLine::Structured(LogLineStructured {
                messages,
                level,
                is_truncated,
                timestamp,
                system_metadata,
            }) => pb::outcome::LogLine {
                log_type: Some(pb::outcome::log_line::LogType::Line(
                    pb::outcome::StructuredLogLine {
                        messages: messages.into(),
                        level: level.to_string(),
                        is_truncated,
                        timestamp: Some(timestamp.into()),
                        system_metadata: system_metadata
                            .map(|m| pb::outcome::SystemLogMetadata { code: m.code }),
                    },
                )),
            },
            LogLine::SubFunction { path, log_lines } => pb::outcome::LogLine {
                log_type: Some(pb::outcome::log_line::LogType::SubFunction(
                    pb::outcome::SubFunctionLogLines {
                        component_path: path.component.to_string(),
                        udf_path: path.udf_path.to_string(),
                        log_lines: log_lines
                            .into_iter()
                            .map(pb::outcome::LogLine::from)
                            .collect(),
                    },
                )),
            },
        }
    }
}

impl TryFrom<pb::outcome::LogLine> for LogLine {
    type Error = anyhow::Error;

    fn try_from(value: pb::outcome::LogLine) -> Result<Self, Self::Error> {
        let result = match value.log_type {
            Some(pb::outcome::log_line::LogType::Line(line)) => {
                LogLine::Structured(LogLineStructured {
                    messages: line.messages.into(),
                    is_truncated: line.is_truncated,
                    level: line.level.parse()?,
                    timestamp: line
                        .timestamp
                        .ok_or_else(|| anyhow::anyhow!("Missing timestamp"))?
                        .try_into()?,
                    system_metadata: line
                        .system_metadata
                        .map(|m| SystemLogMetadata { code: m.code }),
                })
            },
            Some(pb::outcome::log_line::LogType::SubFunction(sub_function)) => {
                LogLine::SubFunction {
                    path: CanonicalizedComponentFunctionPath {
                        component: sub_function.component_path.parse()?,
                        udf_path: sub_function.udf_path.parse()?,
                    },
                    log_lines: sub_function
                        .log_lines
                        .into_iter()
                        .map(LogLine::try_from)
                        .collect::<Result<LogLines, _>>()?,
                }
            },
            None => anyhow::bail!("`log_type` missing in LogLine proto"),
        };
        Ok(result)
    }
}

pub async fn run_function_and_collect_log_lines<Outcome>(
    get_outcome: BoxFuture<'_, Outcome>,
    mut log_line_receiver: mpsc::UnboundedReceiver<LogLine>,
    on_log_line: impl Fn(LogLine),
) -> (Outcome, LogLines) {
    let log_line_consumer = async move {
        let mut full_log_lines = vec![];
        while let Some(log_line) = log_line_receiver.recv().await {
            on_log_line(log_line.clone());
            full_log_lines.push(log_line)
        }
        LogLines::from(full_log_lines)
    };
    tokio::join!(get_outcome, log_line_consumer)
}
