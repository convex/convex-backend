#![feature(impl_trait_in_assoc_type)]
use std::borrow::Cow;

use ::metrics::StaticMetricLabel;
use http::StatusCode;
use prometheus::IntCounter;
use tungstenite::protocol::{
    frame::coding::CloseCode,
    CloseFrame,
};

mod metrics;

/// ErrorMetadata object can be attached to an anyhow error chain via
/// `.context(e /*ErrorMetadata*/)`. It is a generic object to be used
/// across the codebase to tag errors with information that is used to classify.
///
/// The msg is conveyed as a user facing error message if it makes it to the
/// client.
///
/// The short_msg is used as a tag - available for tests and for metrics
/// logging - to have a message that is resilient to changes in copy. Some
/// protocols may opt to send the short_msg as a separate field (eg ws close
/// code and HTTP endpoint response json).
#[derive(thiserror::Error, Clone, Debug, PartialEq, Eq)]
#[error("{msg}")]
pub struct ErrorMetadata {
    /// The error code associated with this ErrorMetadata
    pub code: ErrorCode,
    /// short ScreamingCamelCase. Usable in tests for string matching
    /// w/ a standard test helper.
    /// Eg InvalidModuleName
    pub short_msg: Cow<'static, str>,
    /// human readable - developer facing. Should be longer and descriptive.
    /// Eg "The module name is invalid because it contains an invalid character"
    pub msg: Cow<'static, str>,

    // Optional source of the error (i.e. a service name)
    // If present, this implies that the error originated in an upstream
    // service call (and may have already been reported to Sentry).
    pub r#source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCode {
    BadRequest,
    Conflict,
    Unauthenticated,
    AuthUpdateFailed,
    Forbidden,
    NotFound,
    ClientDisconnect,
    RateLimited,

    Overloaded,
    FeatureTemporarilyUnavailable,
    RejectedBeforeExecution,
    OCC {
        table_name: Option<String>,
        document_id: Option<String>,
        write_source: Option<String>,
        component_path: Option<String>,
        is_system: bool,
        /// The timestamp of the conflicting write, if known.
        /// Used by retry loops to wait until the write is observable.
        write_ts: Option<u64>,
    },
    PaginationLimit,
    OutOfRetention,

    OperationalInternalServerError,
    MisdirectedRequest,
    TooEarly,
}

/// Information about an OCC error, used for logging and diagnostics.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct OccInfo {
    pub table_name: Option<String>,
    pub document_id: Option<String>,
    pub write_source: Option<String>,
    pub component_path: Option<String>,
    pub retry_count: Option<u64>,
}

struct SampledClientErrorFamily {
    /// `family` label on `SAMPLED_CLIENT_ERROR_TOTAL`; must stay a small fixed
    /// set, never derived from tenant data.
    label: &'static str,
    sample_rate: f64,
}

impl ErrorMetadata {
    /// Returns an error containing no information other than a HTTP status
    /// code. This should only be used in cases where there is no
    /// information we'd like to display (perhaps for security reasons) and
    /// returning an empty JSON object is undesirable.
    pub fn opaque(code: ErrorCode) -> Self {
        Self {
            code,
            short_msg: Cow::Borrowed(""),
            msg: Cow::Borrowed(""),
            source: None,
        }
    }

    /// Bad Request. Maps to 400 in HTTP.
    ///
    /// The short_msg should be a CapitalCamelCased describing the error.
    /// The msg should be a descriptive message targeted toward the developer.
    pub fn bad_request(
        short_msg: impl Into<Cow<'static, str>>,
        msg: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            code: ErrorCode::BadRequest,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: None,
        }
    }

    /// Conflict. Maps to 409 in HTTP.
    ///
    /// The short_msg should be a CapitalCamelCased describing the error (eg
    /// DuplicateInstallation). The msg should be a descriptive message targeted
    /// toward the developer.
    pub fn conflict(
        short_msg: impl Into<Cow<'static, str>>,
        msg: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            code: ErrorCode::Conflict,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: None,
        }
    }

    /// Resource not found. Maps to 404 in HTTP. This is not considered
    /// a deterministic user error. It should typically be used when the
    /// resource can't be currently found, e.g. the backend is not currently
    /// in service discovery. If the UDF is missing, this should throw
    /// `bad_request`` instead, which is a deterministic user error.
    ///
    /// The short_msg should be a CapitalCamelCased describing the error (eg
    /// FileNotFound). The msg should be a descriptive message targeted
    /// toward the developer.
    pub fn not_found(
        short_msg: impl Into<Cow<'static, str>>,
        msg: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            code: ErrorCode::NotFound,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: None,
        }
    }

    /// Not authenticated. Maps to 401 in HTTP.
    ///
    /// The short_msg should be a CapitalCamelCased describing the error (eg
    /// InvalidHeader). The msg should be a descriptive message targeted
    /// toward the developer.
    pub fn unauthenticated(
        short_msg: impl Into<Cow<'static, str>>,
        msg: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            code: ErrorCode::Unauthenticated,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: None,
        }
    }

    /// This is a special case of `unauthenticated` that is used when updating
    /// the auth token failed (as opposed to an existing token expiring or
    /// otherwise becoming invalid).
    ///
    /// The short_msg should be a CapitalCamelCased describing the error (eg
    /// InvalidHeader). The msg should be a descriptive message targeted
    /// toward the developer.
    pub fn auth_update_failed(
        short_msg: impl Into<Cow<'static, str>>,
        msg: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            code: ErrorCode::AuthUpdateFailed,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: None,
        }
    }

    /// Forbidden. Maps to 403 in HTTP.
    ///
    /// The short_msg should be a CapitalCamelCased describing the error (eg
    /// TooManyTeams). The msg should be a descriptive message targeted
    /// toward the developer.
    pub fn forbidden(
        short_msg: impl Into<Cow<'static, str>>,
        msg: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            code: ErrorCode::Forbidden,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: None,
        }
    }

    /// Client disconnected the connection.
    pub fn client_disconnect() -> Self {
        Self {
            code: ErrorCode::ClientDisconnect,
            short_msg: CLIENT_DISCONNECTED.into(),
            msg: CLIENT_DISCONNECTED_MSG.into(),
            source: None,
        }
    }

    /// Conductor recevied a request intended for an instance it does not serve.
    /// This error is intended to be used by Usher to retry the request and/or
    /// update its caches, and should not directly be sent back to the user.
    pub fn misdirected_request() -> Self {
        Self {
            code: ErrorCode::MisdirectedRequest,
            short_msg: "MisdirectedRequest".into(),
            msg: "Service temporarily unavailable".into(),
            source: None,
        }
    }

    /// RateLimited. Maps to 429 in HTTP.
    ///
    /// The short_msg should be a CapitalCamelCased describing the error (eg
    /// TooManyTeams). The msg should be a descriptive message targeted
    /// toward the developer.
    ///
    /// In the user facing `msg`, be very clear about what is actionable here.
    /// Some rate limits require paying for more resources, while others are
    /// indicative of incorrect user behavior (eg > 1000 concurrent mutations
    /// over a single websocket).
    pub fn rate_limited(
        short_msg: impl Into<Cow<'static, str>>,
        msg: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            code: ErrorCode::RateLimited,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: None,
        }
    }

    /// Operational Internal Server Error (maps to 500 in HTTP)
    ///
    /// Produces a very general error message for the user. Should be
    /// used in situations where the error is caused by a known operational
    /// source of downtime (eg during a restart or backend code push)
    pub fn operational_internal_server_error() -> Self {
        Self {
            code: ErrorCode::OperationalInternalServerError,
            short_msg: INTERNAL_SERVER_ERROR.into(),
            msg: INTERNAL_SERVER_ERROR_MSG.into(),
            source: None,
        }
    }

    /// Internal error with a user visible message indicating that the user has
    /// hit some defensive limit in Convex. Maps to 503 in HTTP.
    ///
    /// Ideally no user would ever these errors, but we have some systems that
    /// do not currently scale. Throwing an overloaded in the short term in
    /// these cases is preferable to the instance falling over.
    ///
    /// If the limit being hit is used for pagination limiting, use that error
    /// instead of this method.
    ///
    /// If you do not need a custom error message, do not use this method.
    /// Instead use anyhow without any ErrorMetadata, which will automatically
    /// be shown to the user as a generic internal server error.
    ///
    /// The short_msg should be a CapitalCamelCased describing the error (eg
    /// InvalidHeader). The msg should be a descriptive message targeted
    /// toward the developer.
    pub fn overloaded(
        short_msg: impl Into<Cow<'static, str>>,
        msg: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            code: ErrorCode::Overloaded,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: None,
        }
    }

    /// Indicates that a "less critical" feature is not yet available, e.g. due
    /// to an instance restarting. If a query encounters this error type, it
    /// will cause
    pub fn feature_temporarily_unavailable(
        short_msg: impl Into<Cow<'static, str>>,
        msg: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            code: ErrorCode::FeatureTemporarilyUnavailable,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: None,
        }
    }

    // This is similar to `overloaded` but also guarantees the request was
    // rejected before it has been started. You should generally prefer to use
    // `overloaded`` instead of this error code and decide if an operation is safe
    // to retry based on the fact if its idempotent. This error code can be used
    // in very specific situations, e.g. actions that have been rejected before
    // they have been started, and thus can be safely retries.
    pub fn rejected_before_execution(
        short_msg: impl Into<Cow<'static, str>>,
        msg: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            code: ErrorCode::RejectedBeforeExecution,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: None,
        }
    }

    /// Internal Optimistic Concurrency Control / Commit Race Error.
    ///
    /// These come from sqlx, or are caused by OCCs on system tables.
    pub fn system_occ(write_ts: Option<u64>, write_source: Option<String>) -> Self {
        Self {
            code: ErrorCode::OCC {
                table_name: None,
                document_id: None,
                write_source,
                component_path: None,
                is_system: true,
                write_ts,
            },
            short_msg: OCC_ERROR.into(),
            msg: OCC_ERROR_MSG.into(),
            source: None,
        }
    }

    /// User-caused Optimistic Concurrency Control / Commit Race Error
    pub fn user_occ(
        info: Option<OccInfo>,
        description: Option<String>,
        write_ts: Option<u64>,
    ) -> Self {
        let table_description = info
            .as_ref()
            .and_then(|i| i.table_name.as_ref())
            .map(|t| format!("the \"{t}\" table"))
            .unwrap_or("some table".to_owned());
        let write_source_description = description
            .map(|source| format!("{source}. "))
            .unwrap_or_default();
        let info = info.unwrap_or_default();
        Self {
            code: ErrorCode::OCC {
                table_name: info.table_name,
                document_id: info.document_id,
                write_source: info.write_source,
                component_path: info.component_path,
                is_system: false,
                write_ts,
            },
            short_msg: OCC_ERROR.into(),
            msg: format!(
                "Documents read from or written to {table_description} \
                changed while this mutation was being run and on every \
                subsequent retry. {write_source_description}See https://docs.convex.dev/error#1",
            )
            .into(),
            source: None,
        }
    }

    /// Responds with HTTP 425 Too Early.
    /// Not intended to be user-facing, but is used to indicate to the
    /// consistency checker that it should skip this unloaded instance rather
    /// than considering it an inconsistency.
    pub fn too_early() -> Self {
        Self {
            code: ErrorCode::TooEarly,
            short_msg: "TooEarly".into(),
            msg: "Deployment is not loaded yet, try again later".into(),
            source: None,
        }
    }

    /// Out of Retention
    ///
    /// An error we produce if executing a read at a point that has been removed
    /// due to retention.
    pub fn out_of_retention() -> Self {
        Self {
            code: ErrorCode::OutOfRetention,
            short_msg: INTERNAL_SERVER_ERROR.into(),
            msg: INTERNAL_SERVER_ERROR_MSG.into(),
            source: None,
        }
    }

    /// Hit some kind of external facing pagination limit (eg too many
    /// documents, too much memory used).
    ///
    /// The short_msg should be a CapitalCamelCased describing the error (eg
    /// QueryScannedTooManyDocuments).
    /// The msg should be a descriptive message targeted toward the developer.
    pub fn pagination_limit(
        short_msg: impl Into<Cow<'static, str>>,
        msg: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            code: ErrorCode::PaginationLimit,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: None,
        }
    }

    pub fn from_http_status_code(
        code: StatusCode,
        short_msg: impl Into<Cow<'static, str>>,
        msg: impl Into<Cow<'static, str>>,
    ) -> Option<Self> {
        let code = ErrorCode::from_http_status_code(code)?;
        Some(Self {
            code,
            short_msg: short_msg.into(),
            msg: msg.into(),
            source: None,
        })
    }

    pub fn is_occ(&self) -> bool {
        matches!(self.code, ErrorCode::OCC { .. })
    }

    pub fn is_pagination_limit(&self) -> bool {
        self.code == ErrorCode::PaginationLimit
    }

    pub fn is_unauthenticated(&self) -> bool {
        self.code == ErrorCode::Unauthenticated
    }

    pub fn is_auth_update_failed(&self) -> bool {
        self.code == ErrorCode::AuthUpdateFailed
    }

    pub fn is_out_of_retention(&self) -> bool {
        self.code == ErrorCode::OutOfRetention
    }

    pub fn is_bad_request(&self) -> bool {
        self.code == ErrorCode::BadRequest
    }

    pub fn is_not_found(&self) -> bool {
        self.code == ErrorCode::NotFound
    }

    pub fn is_overloaded(&self) -> bool {
        self.code == ErrorCode::Overloaded
    }

    pub fn is_rate_limited(&self) -> bool {
        self.code == ErrorCode::RateLimited
    }

    pub fn is_operational_internal_server_error(&self) -> bool {
        self.code == ErrorCode::OperationalInternalServerError
    }

    pub fn is_rejected_before_execution(&self) -> bool {
        self.code == ErrorCode::RejectedBeforeExecution
    }

    pub fn is_feature_temporarily_unavailable(&self) -> bool {
        self.code == ErrorCode::FeatureTemporarilyUnavailable
    }

    pub fn is_forbidden(&self) -> bool {
        self.code == ErrorCode::Forbidden
    }

    pub fn is_misdirected_request(&self) -> bool {
        self.code == ErrorCode::MisdirectedRequest
    }

    pub fn is_too_early(&self) -> bool {
        self.code == ErrorCode::TooEarly
    }

    pub fn is_client_disconnect(&self) -> bool {
        self.code == ErrorCode::ClientDisconnect
    }

    /// Return true if this error is deterministically caused by user. If so,
    /// we can propagate it into JS out of a syscall, and cache it if it is the
    /// full UDF result.
    pub fn is_deterministic_user_error(&self) -> bool {
        match self.code {
            ErrorCode::BadRequest
            | ErrorCode::Conflict
            | ErrorCode::PaginationLimit
            | ErrorCode::Unauthenticated
            | ErrorCode::AuthUpdateFailed
            | ErrorCode::Forbidden => true,
            ErrorCode::OperationalInternalServerError
            | ErrorCode::ClientDisconnect
            | ErrorCode::NotFound
            | ErrorCode::RateLimited
            | ErrorCode::OCC { .. }
            | ErrorCode::OutOfRetention
            | ErrorCode::Overloaded
            | ErrorCode::FeatureTemporarilyUnavailable
            | ErrorCode::RejectedBeforeExecution
            | ErrorCode::MisdirectedRequest
            | ErrorCode::TooEarly => false,
        }
    }

    /// Single source of truth for a family's sample rate and counter label, so
    /// the two cannot drift apart.
    fn sampled_client_error_family(&self) -> Option<SampledClientErrorFamily> {
        let family = match (&self.code, &*self.short_msg) {
            (ErrorCode::Forbidden, "Unauthorized") => SampledClientErrorFamily {
                label: "forbidden_unauthorized",
                sample_rate: 0.001,
            },
            (ErrorCode::Forbidden, "OperationNotPermitted") => SampledClientErrorFamily {
                label: "operation_not_permitted",
                sample_rate: 0.01,
            },
            _ => return None,
        };
        Some(family)
    }

    /// Must run before the Sentry sampling gate: sampled counts hide
    /// regressions that fan out across many small tenants instead of spiking on
    /// one.
    pub fn log_sampled_client_error(&self) {
        if let Some(family) = self.sampled_client_error_family() {
            crate::metrics::log_sampled_client_error(family.label);
        }
    }

    /// Returns the level at which the given error should report to sentry
    /// INFO -> it's a client-at-fault error
    /// WARNING -> it's a server-at-fault error that is expected
    /// ERROR -> it's a server-at-fault error that is unexpected (probably a
    /// bug)
    /// FATAL -> it crashes the backend
    ///
    /// Also return an optional sampling rate for this type of error
    pub fn should_report_to_sentry(&self) -> Option<(sentry::Level, Option<f64>)> {
        // Sentry considers errors invalid if this field is empty.
        if self.short_msg.is_empty() {
            return None;
        }

        // Ahead of the match so a family's rate wins over the catch-all arm for
        // its `ErrorCode`.
        if let Some(family) = self.sampled_client_error_family() {
            return Some((sentry::Level::Info, Some(family.sample_rate)));
        }

        match self.code {
            ErrorCode::ClientDisconnect => None,
            ErrorCode::BadRequest if self.short_msg == "BackendIsNotRunning" => None,
            ErrorCode::BadRequest
                if ["WSMessageInvalidJson", "InvalidConnectionHeader"]
                    .contains(&&*self.short_msg) =>
            {
                Some((sentry::Level::Info, Some(0.001)))
            },
            ErrorCode::BadRequest
            | ErrorCode::Conflict
            | ErrorCode::Forbidden
            | ErrorCode::MisdirectedRequest
            | ErrorCode::TooEarly => Some((sentry::Level::Info, None)),
            // Sample errors that only matter at high volume, to reduce load on sentry.
            ErrorCode::Unauthenticated
            | ErrorCode::NotFound
            | ErrorCode::AuthUpdateFailed
            | ErrorCode::PaginationLimit => Some((sentry::Level::Info, Some(0.001))),

            // Sample operational errors - since they only matter at high volume
            ErrorCode::OutOfRetention | ErrorCode::OperationalInternalServerError => {
                Some((sentry::Level::Warning, Some(0.1)))
            },
            ErrorCode::RejectedBeforeExecution => Some((sentry::Level::Warning, Some(0.01))),

            // Sampling for OCC/Overloaded/RateLimited, since we only really care about the
            // details if they happen at high volume.
            ErrorCode::RateLimited => Some((sentry::Level::Info, Some(0.001))),
            ErrorCode::OCC {
                is_system: false, ..
            } => Some((sentry::Level::Warning, Some(0.001))),
            ErrorCode::OCC {
                is_system: true, ..
            } => Some((sentry::Level::Warning, Some(0.01))),
            ErrorCode::Overloaded if self.short_msg == "CommitterFullError" => {
                Some((sentry::Level::Warning, Some(0.001)))
            },
            // we want to see these a bit more than the others above
            ErrorCode::Overloaded | ErrorCode::FeatureTemporarilyUnavailable => {
                Some((sentry::Level::Warning, Some(0.1)))
            },
        }
    }

    fn metric_server_error_label_value(&self) -> Option<&'static str> {
        match self.code {
            ErrorCode::BadRequest
            | ErrorCode::Conflict
            | ErrorCode::PaginationLimit
            | ErrorCode::Unauthenticated
            | ErrorCode::AuthUpdateFailed
            | ErrorCode::Forbidden
            | ErrorCode::ClientDisconnect
            | ErrorCode::MisdirectedRequest
            | ErrorCode::TooEarly
            | ErrorCode::RateLimited
            | ErrorCode::NotFound => None,
            ErrorCode::OCC { .. } => Some("occ"),
            ErrorCode::OutOfRetention => Some("out_of_retention"),
            ErrorCode::Overloaded => Some("overloaded"),
            ErrorCode::FeatureTemporarilyUnavailable => Some("feature_unavailable"),
            ErrorCode::RejectedBeforeExecution => Some("rejected_before_execution"),
            ErrorCode::OperationalInternalServerError => Some("operational"),
        }
    }

    pub fn metric_server_error_label(&self) -> Option<StaticMetricLabel> {
        self.metric_server_error_label_value()
            .map(|v| StaticMetricLabel::new("type", v))
    }

    pub fn custom_metric(&self) -> Option<&'static IntCounter> {
        match self.code {
            ErrorCode::BadRequest => Some(&crate::metrics::BAD_REQUEST_ERROR_TOTAL),
            ErrorCode::Conflict => None,
            ErrorCode::ClientDisconnect => Some(&crate::metrics::CLIENT_DISCONNECT_ERROR_TOTAL),
            ErrorCode::RateLimited => Some(&crate::metrics::RATE_LIMITED_ERROR_TOTAL),
            ErrorCode::Unauthenticated | ErrorCode::AuthUpdateFailed => {
                Some(&crate::metrics::SYNC_AUTH_ERROR_TOTAL)
            },
            ErrorCode::Forbidden => Some(&crate::metrics::FORBIDDEN_ERROR_TOTAL),
            ErrorCode::OCC { .. } => Some(&crate::metrics::COMMIT_RACE_TOTAL),
            ErrorCode::NotFound => None,
            ErrorCode::PaginationLimit => None,
            ErrorCode::OutOfRetention => None,
            ErrorCode::Overloaded => None,
            ErrorCode::FeatureTemporarilyUnavailable => None,
            ErrorCode::RejectedBeforeExecution => None,
            ErrorCode::OperationalInternalServerError => None,
            ErrorCode::MisdirectedRequest => None,
            ErrorCode::TooEarly => None,
        }
    }

    pub fn close_frame(&self) -> Option<CloseFrame> {
        let code = match self.code {
            ErrorCode::NotFound
            | ErrorCode::PaginationLimit
            | ErrorCode::Forbidden
            | ErrorCode::ClientDisconnect => Some(CloseCode::Normal),
            ErrorCode::OCC { .. }
            | ErrorCode::OutOfRetention
            | ErrorCode::Overloaded
            | ErrorCode::FeatureTemporarilyUnavailable
            | ErrorCode::RateLimited
            | ErrorCode::RejectedBeforeExecution
            | ErrorCode::MisdirectedRequest
            | ErrorCode::TooEarly => Some(CloseCode::Again),
            ErrorCode::OperationalInternalServerError => Some(CloseCode::Error),
            // These ones are client errors - so no close code - the client
            // will handle and close the connection instead.
            ErrorCode::BadRequest
            | ErrorCode::Unauthenticated
            | ErrorCode::AuthUpdateFailed
            | ErrorCode::Conflict => None,
        }?;
        // According to the WebSocket protocol specification (RFC 6455), the reason
        // string (if present) is limited to 123 bytes. This is because the
        // Close frame may contain a body, with the first two bytes representing
        // the close code followed by the optional reason string. The whole
        // Close frame's payload is limited to 125 bytes, so after accounting for
        // the 2-byte close code, 123 bytes remain for the reason string.
        let mut reason = self.short_msg.to_string();
        reason.truncate(123);
        let reason = reason.into();
        Some(CloseFrame { code, reason })
    }
}

impl ErrorCode {
    fn http_status_code(&self) -> StatusCode {
        match self {
            ErrorCode::BadRequest | ErrorCode::PaginationLimit => StatusCode::BAD_REQUEST,
            ErrorCode::Conflict => StatusCode::CONFLICT,
            // HTTP has the unfortunate naming of 401 as unauthorized when it's
            // really about authentication.
            // https://stackoverflow.com/questions/3297048/403-forbidden-vs-401-unauthorized-http-responses
            ErrorCode::Unauthenticated | ErrorCode::AuthUpdateFailed => StatusCode::UNAUTHORIZED,
            ErrorCode::Forbidden => StatusCode::FORBIDDEN,
            ErrorCode::NotFound => StatusCode::NOT_FOUND,
            ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::OperationalInternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::OCC { .. }
            | ErrorCode::OutOfRetention
            | ErrorCode::Overloaded
            | ErrorCode::FeatureTemporarilyUnavailable
            | ErrorCode::RejectedBeforeExecution => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::TooEarly => StatusCode::TOO_EARLY,
            ErrorCode::ClientDisconnect => StatusCode::REQUEST_TIMEOUT,
            ErrorCode::MisdirectedRequest => StatusCode::MISDIRECTED_REQUEST,
        }
    }

    pub fn grpc_status_code(&self) -> tonic::Code {
        match self {
            ErrorCode::BadRequest => tonic::Code::InvalidArgument,
            ErrorCode::Conflict => tonic::Code::AlreadyExists,
            ErrorCode::Unauthenticated | ErrorCode::AuthUpdateFailed => {
                tonic::Code::Unauthenticated
            },
            ErrorCode::Forbidden => tonic::Code::FailedPrecondition,
            ErrorCode::NotFound => tonic::Code::NotFound,
            ErrorCode::ClientDisconnect => tonic::Code::Aborted,
            ErrorCode::Overloaded
            | ErrorCode::FeatureTemporarilyUnavailable
            | ErrorCode::RejectedBeforeExecution
            | ErrorCode::RateLimited => tonic::Code::ResourceExhausted,
            ErrorCode::OCC { .. } => tonic::Code::ResourceExhausted,
            ErrorCode::PaginationLimit => tonic::Code::InvalidArgument,
            ErrorCode::OutOfRetention => tonic::Code::OutOfRange,
            ErrorCode::OperationalInternalServerError => tonic::Code::Internal,
            ErrorCode::MisdirectedRequest | ErrorCode::TooEarly => tonic::Code::FailedPrecondition,
        }
    }

    pub fn from_http_status_code(code: StatusCode) -> Option<Self> {
        match code {
            StatusCode::UNAUTHORIZED => Some(ErrorCode::Unauthenticated),
            StatusCode::FORBIDDEN => Some(ErrorCode::Forbidden),
            StatusCode::NOT_FOUND => Some(ErrorCode::NotFound),
            StatusCode::TOO_MANY_REQUESTS => Some(ErrorCode::RateLimited),
            StatusCode::TOO_EARLY => Some(ErrorCode::TooEarly),
            StatusCode::MISDIRECTED_REQUEST => Some(ErrorCode::MisdirectedRequest),
            // Tries to categorize in one of the above more specific 4xx codes first,
            // otherwise categorizes as a general 4xx via BadRequest
            v if v.is_client_error() => Some(ErrorCode::BadRequest),
            v if v.is_server_error() => Some(ErrorCode::Overloaded),
            _ => None,
        }
    }
}

pub trait ErrorMetadataAnyhowExt {
    fn is_occ(&self) -> bool;
    fn occ_info(&self) -> Option<OccInfo>;
    fn occ_write_ts(&self) -> Option<u64>;
    fn is_pagination_limit(&self) -> bool;
    fn is_unauthenticated(&self) -> bool;
    fn is_auth_update_failed(&self) -> bool;
    fn is_out_of_retention(&self) -> bool;
    fn is_bad_request(&self) -> bool;
    fn is_not_found(&self) -> bool;
    fn is_overloaded(&self) -> bool;
    fn is_rate_limited(&self) -> bool;
    fn is_operational_internal_server_error(&self) -> bool;
    fn is_rejected_before_execution(&self) -> bool;
    fn is_feature_temporarily_unavailable(&self) -> bool;
    fn is_forbidden(&self) -> bool;
    fn is_too_early(&self) -> bool;
    fn should_report_to_sentry(&self) -> Option<(sentry::Level, Option<f64>)>;
    fn is_deterministic_user_error(&self) -> bool;
    fn is_misdirected_request(&self) -> bool;
    fn is_client_disconnect(&self) -> bool;
    fn user_facing_message(&self) -> String;
    fn short_msg(&self) -> &str;
    fn msg(&self) -> &str;
    fn metric_server_error_label(&self) -> Option<StaticMetricLabel>;
    fn metric_status_label_value(&self) -> &'static str;
    fn close_frame(&self) -> Option<CloseFrame>;
    fn http_status(&self) -> StatusCode;
    fn map_error_metadata<F: FnOnce(ErrorMetadata) -> ErrorMetadata>(self, f: F) -> Self;
    fn wrap_error_message<F>(self, f: F) -> Self
    where
        F: FnOnce(String) -> String;
    fn clone_error(&self) -> Self;
}

impl ErrorMetadataAnyhowExt for anyhow::Error {
    /// Returns true if error is tagged as OCC
    fn is_occ(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_occ();
        }
        false
    }

    fn occ_info(&self) -> Option<OccInfo> {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return match &e.code {
                ErrorCode::OCC {
                    table_name,
                    document_id,
                    write_source,
                    component_path,
                    is_system: _,
                    write_ts: _,
                } => Some(OccInfo {
                    table_name: table_name.clone(),
                    document_id: document_id.clone(),
                    write_source: write_source.clone(),
                    component_path: component_path.clone(),
                    retry_count: None,
                }),
                _ => None,
            };
        }
        None
    }

    fn occ_write_ts(&self) -> Option<u64> {
        self.downcast_ref::<ErrorMetadata>()
            .and_then(|e| match &e.code {
                ErrorCode::OCC { write_ts, .. } => *write_ts,
                _ => None,
            })
    }

    /// Returns true if error is tagged as PaginationLimit
    fn is_pagination_limit(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_pagination_limit();
        }
        false
    }

    /// Returns true if error is tagged as Unauthenticated
    fn is_unauthenticated(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_unauthenticated();
        }
        false
    }

    /// Returns true if error is tagged as AuthUpdateFailed
    fn is_auth_update_failed(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_auth_update_failed();
        }
        false
    }

    /// Returns true if error is tagged as OutOfRetention
    fn is_out_of_retention(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_out_of_retention();
        }
        false
    }

    /// Returns true if error is tagged as BadRequest
    fn is_bad_request(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_bad_request();
        }
        false
    }

    /// Returns true if error is tagged as NotFound
    fn is_not_found(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_not_found();
        }
        false
    }

    /// Returns true if error is tagged as Overloaded
    fn is_overloaded(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_overloaded();
        }
        false
    }

    fn is_rate_limited(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_rate_limited();
        }
        false
    }

    /// Returns true if error is tagged as Overloaded
    fn is_operational_internal_server_error(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_operational_internal_server_error();
        }
        false
    }

    /// Returns true if error is tagged as RejectedBeforeExecution
    fn is_rejected_before_execution(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_rejected_before_execution();
        }
        false
    }

    /// Returns true if error is tagged as FeatureTemporarilyUnavailable
    fn is_feature_temporarily_unavailable(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_feature_temporarily_unavailable();
        }
        false
    }

    /// Returns true if error is tagged as Forbidden
    fn is_forbidden(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_forbidden();
        }
        false
    }

    fn is_misdirected_request(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_misdirected_request();
        }
        false
    }

    fn is_client_disconnect(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_client_disconnect();
        }
        false
    }

    fn is_too_early(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_too_early();
        }
        false
    }

    /// Returns the level at which the given error should report to sentry
    /// INFO -> it's a client-at-fault error
    /// WARNING -> it's a server-at-fault error that is expected
    /// ERROR -> it's a server-at-fault error that is unexpected (probably a
    /// bug)
    /// FATAL -> it crashes the backend
    fn should_report_to_sentry(&self) -> Option<(sentry::Level, Option<f64>)> {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.should_report_to_sentry();
        }
        Some((sentry::Level::Error, None))
    }

    /// Return true if this error is deterministically caused by user. If so,
    /// we can propagate it into JS out of a syscall, and cache it if it is the
    /// full UDF result.
    /// We can also use it to determine if clients should be requested to retry.
    fn is_deterministic_user_error(&self) -> bool {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.is_deterministic_user_error();
        }
        false
    }

    fn user_facing_message(&self) -> String {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.to_string();
        }
        INTERNAL_SERVER_ERROR_MSG.to_string()
    }

    /// Return the short_msg associated with this Error
    fn short_msg(&self) -> &str {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return &e.short_msg;
        }
        INTERNAL_SERVER_ERROR
    }

    /// Return the descriptive msg associated with this Error
    fn msg(&self) -> &str {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return &e.msg;
        }
        INTERNAL_SERVER_ERROR_MSG
    }

    /// Return the tag to use on a server error metric
    fn metric_server_error_label(&self) -> Option<StaticMetricLabel> {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.metric_server_error_label();
        }
        Some(StaticMetricLabel::new("type", "internal"))
    }

    /// Return the tag to use on a server status metric
    fn metric_status_label_value(&self) -> &'static str {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return match e.metric_server_error_label_value() {
                Some(v) => v,
                None => match e.code {
                    ErrorCode::Unauthenticated => {
                        StaticMetricLabel::STATUS_UNAUTHENTICATED_ERROR
                            .split_key_value()
                            .1
                    },
                    ErrorCode::AuthUpdateFailed => {
                        StaticMetricLabel::STATUS_AUTH_UPDATE_FAILED_ERROR
                            .split_key_value()
                            .1
                    },
                    _ => {
                        StaticMetricLabel::STATUS_DEVELOPER_ERROR
                            .split_key_value()
                            .1
                    },
                },
            };
        }
        StaticMetricLabel::STATUS_ERROR.split_key_value().1
    }

    /// Return the CloseCode to use on websocket
    fn close_frame(&self) -> Option<CloseFrame> {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.close_frame();
        }
        Some(CloseFrame {
            code: CloseCode::Error,
            reason: INTERNAL_SERVER_ERROR.to_owned().into(),
        })
    }

    /// Return the HttpStatus code to use on response
    fn http_status(&self) -> StatusCode {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>() {
            return e.code.http_status_code();
        }
        StatusCode::INTERNAL_SERVER_ERROR
    }

    fn map_error_metadata<F>(self, f: F) -> Self
    where
        F: FnOnce(ErrorMetadata) -> ErrorMetadata,
    {
        if let Some(e) = self.downcast_ref::<ErrorMetadata>().cloned() {
            return self.context(f(e));
        }
        self
    }

    /// Wrap the underlying error message, maintaining the underlying error
    /// metadata short code if it exists.
    fn wrap_error_message<F>(mut self, f: F) -> Self
    where
        F: FnOnce(String) -> String,
    {
        if let Some(ref mut em) = self.downcast_mut::<ErrorMetadata>() {
            // Underlying ErrorMetadata. Modify in place.
            em.msg = f(em.msg.to_string()).into();
            return self;
        }

        // No underlying code. Just use .context()
        let new_msg = f(self.to_string());
        self.context(new_msg)
    }

    // NB: This function loses the backtrace for `e` and creates a new backtrace.
    fn clone_error(&self) -> Self {
        match self.downcast_ref::<ErrorMetadata>() {
            Some(error_metadata) => error_metadata.clone().into(),
            None => anyhow::anyhow!("{self:#}"),
        }
    }
}

pub const INTERNAL_SERVER_ERROR_MSG: &str = "Your request couldn't be completed. Try again later.";
pub const INTERNAL_SERVER_ERROR: &str = "InternalServerError";
pub const OCC_ERROR_MSG: &str = "Data read or written in \
                                 this mutation changed while it was being run. Consider reducing \
                                 the amount of data read by using indexed queries with selective \
                                 index range expressions (https://docs.convex.dev/database/indexes/).";
pub const OCC_ERROR: &str = "OptimisticConcurrencyControlFailure";
const CLIENT_DISCONNECTED_MSG: &str = "Client disconnected";
const CLIENT_DISCONNECTED: &str = "ClientDisconnected";
