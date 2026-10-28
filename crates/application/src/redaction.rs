use std::fmt;

use anyhow::Context;
use common::{
    errors::JsError,
    log_lines::LogLines,
    RequestId,
};
use http::StatusCode;
use pb::common::{
    RedactedJsError as RedactedJsErrorProto,
    RedactedLogLines as RedactedLogLinesProto,
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::json;
use sync_types::{
    types::ErrorPayload,
    LogLinesMessage,
};
use udf::HttpActionResponsePart;
use value::{
    sha256::Sha256,
    ConvexValue,
    JsonPackedValue,
};

/// List of log lines from a Convex function execution, redacted to only
/// contain information that clients are allowed to see.
///
/// The level of redaction will depend on the configuration of the backend and
/// could be anything from full information to completely stripped out.
#[derive(Debug, Serialize, Deserialize)]
pub struct RedactedLogLines(Vec<String>);

impl RedactedLogLines {
    pub fn from_log_lines(log_lines: LogLines, block_logging: bool) -> Self {
        Self(if block_logging {
            vec![]
        } else {
            log_lines
                .into_iter()
                .flat_map(|l| l.to_pretty_strings())
                .collect()
        })
    }

    pub fn empty() -> Self {
        Self(vec![])
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<RedactedLogLines> for LogLinesMessage {
    fn from(l: RedactedLogLines) -> Self {
        Self(l.0)
    }
}

impl From<RedactedLogLines> for RedactedLogLinesProto {
    fn from(value: RedactedLogLines) -> Self {
        Self { log_lines: value.0 }
    }
}

impl TryFrom<RedactedLogLinesProto> for RedactedLogLines {
    type Error = anyhow::Error;

    fn try_from(msg: RedactedLogLinesProto) -> anyhow::Result<Self> {
        Ok(Self(msg.log_lines))
    }
}

/// An Error emitted from a Convex Function execution. redacted to only
/// contain information that clients are allowed to see.
///
/// The level of redaction will depend on the configuration of the backend and
/// could be anything from full information to completely stripped out.
#[derive(thiserror::Error, Debug)]
pub struct RedactedJsError {
    error: JsError,
    block_logging: bool,
    request_id: RequestId,
}

impl RedactedJsError {
    pub fn from_js_error(error: JsError, block_logging: bool, request_id: RequestId) -> Self {
        Self {
            error,
            block_logging,
            request_id,
        }
    }

    pub fn custom_data_if_any(self) -> Option<ConvexValue> {
        self.error.custom_data
    }

    pub fn into_error_payload(self) -> ErrorPayload<JsonPackedValue> {
        let message = format!("{self}");
        if let Some(data) = self.custom_data_if_any() {
            ErrorPayload::ErrorData {
                message,
                data: JsonPackedValue::pack(data),
            }
        } else {
            ErrorPayload::Message(message)
        }
    }

    /// Update the given digest with the contents of this error in a way that's
    /// suitable for comparing the content, not than origin, of the error.
    ///
    /// request_id is excluded because it's based on the calling context and
    /// does not influence the content of the underlying error.
    pub fn deduplication_hash(&self, digest: &mut Sha256) {
        digest.update(self.error.to_string().as_bytes());
        digest.update(if self.block_logging { &[1u8] } else { &[0u8] });
    }

    /// Format the exception when it is or will be nested inside of another
    /// RedactedJsError
    ///
    /// In particular we don't want to print 'Server Error' or the request id
    /// multiple times in the same stack trace.
    pub fn nested_to_string(&self) -> String {
        if self.block_logging {
            "Server Error".to_string()
        } else {
            format!("{}", self.error)
        }
    }

    pub fn to_http_response_parts(self) -> Vec<HttpActionResponsePart> {
        let code = if self.block_logging {
            "Server Error".to_string()
        } else {
            format!("Server Error: {}", self.error.message)
        };
        let code = format!("[Request ID: {}] {}", self.request_id, code);
        let mut body = json!({
            "code": code,
        });
        if !self.block_logging {
            body["trace"] = self.error.to_string().into();
        }
        if let Some(custom_data) = self.custom_data_if_any() {
            body["data"] = custom_data.into();
        }
        HttpActionResponsePart::from_json(StatusCode::INTERNAL_SERVER_ERROR, body)
    }
}

impl fmt::Display for RedactedJsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[Request ID: {}] Server Error", self.request_id)?;
        if !self.block_logging {
            write!(f, "\n{}", self.error)?;
        }
        Ok(())
    }
}

impl TryFrom<RedactedJsError> for RedactedJsErrorProto {
    type Error = anyhow::Error;

    fn try_from(value: RedactedJsError) -> anyhow::Result<Self> {
        Ok(Self {
            error: Some(value.error.try_into()?),
            block_logging: Some(value.block_logging),
            request_id: Some(value.request_id.into()),
        })
    }
}

impl TryFrom<RedactedJsErrorProto> for RedactedJsError {
    type Error = anyhow::Error;

    fn try_from(msg: RedactedJsErrorProto) -> anyhow::Result<Self> {
        let error = msg.error.context("Missing `error` field")?.try_into()?;
        let block_logging = msg.block_logging.context("Missing `block_logging` field")?;
        let request_id = msg
            .request_id
            .context("Missing `request_id` field")?
            .try_into()?;
        Ok(RedactedJsError {
            error,
            block_logging,
            request_id,
        })
    }
}
