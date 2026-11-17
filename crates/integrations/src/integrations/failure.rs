use common::{
    errors::report_error,
    http::HttpResponseStream,
};
use http::StatusCode;
use model::integrations::types::IntegrationType;

use crate::metrics;

/// Why an outbound request from an integration to a customer's endpoint failed.
///
/// Deliberately not an `ErrorMetadata`: an integration is a background egress
/// worker, so there is no inbound request to fail, no status code to return,
/// and nothing to propagate into JS out of a syscall. See
/// `ErrorMetadata::is_deterministic_user_error`.
#[derive(Debug)]
pub enum IntegrationEgressFailure {
    /// The endpoint returned a status that should not be retried within the
    /// current batch.
    Rejected { status: StatusCode },
    /// A 5xx, a transport failure, or an exhausted retry budget.
    Transient { description: String },
}

impl IntegrationEgressFailure {
    /// The retry budget ran out. `last` is the final attempt's failure, absent
    /// only if no attempt was made.
    pub fn retries_exhausted(attempts: usize, last: Option<Self>) -> Self {
        let description = match last {
            Some(last) => format!("gave up after {attempts} attempts, last failure: {last}"),
            None => format!("gave up after {attempts} attempts"),
        };
        Self::Transient { description }
    }

    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected { .. })
    }

    /// `outcome` label on the integration failure counters; a fixed set of two.
    pub fn outcome_label(&self) -> &'static str {
        match self {
            Self::Rejected { .. } => "rejected",
            Self::Transient { .. } => "transient",
        }
    }
}

impl std::fmt::Display for IntegrationEgressFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected { status } => write!(f, "endpoint rejected the request with {status}"),
            Self::Transient { description } => write!(f, "{description}"),
        }
    }
}

impl std::error::Error for IntegrationEgressFailure {}

/// Classify an outbound log-integration response without reading its body,
/// which may contain customer data echoed by the endpoint.
pub fn classify_integration_response(
    response: anyhow::Result<HttpResponseStream>,
) -> Result<(), IntegrationEgressFailure> {
    let response = match response {
        Ok(response) => response,
        // Only the outermost message: the chain below it can name our egress
        // proxy, and this string can reach customers through startup failure
        // state without passing through `strip_pii`.
        Err(e) => {
            return Err(IntegrationEgressFailure::Transient {
                description: format!("{e}"),
            });
        },
    };
    let status = response.status;
    if !(status.is_client_error() || status.is_server_error()) {
        return Ok(());
    }
    if is_retryable(status) {
        return Err(IntegrationEgressFailure::Transient {
            description: format!("endpoint returned {status}"),
        });
    }
    Err(IntegrationEgressFailure::Rejected { status })
}

/// Retry 5xx responses within the current batch. Among 4xx responses, 408 and
/// 425 are timing, 429 is throttling, and 421 is an HTTP/2 connection-reuse
/// artifact. Other 4xx responses are retried when the next batch arrives.
fn is_retryable(status: StatusCode) -> bool {
    status.is_server_error()
        || matches!(
            status,
            StatusCode::REQUEST_TIMEOUT
                | StatusCode::MISDIRECTED_REQUEST
                | StatusCode::TOO_EARLY
                | StatusCode::TOO_MANY_REQUESTS
        )
}

/// Handles an integration's failed batches: reports egress failures without
/// sending them to Sentry, reports our own failures to Sentry, and holds the
/// egress log line to one per run of failures.
pub struct IntegrationFailureReporter {
    integration_type: IntegrationType,
    logged_since_success: bool,
}

impl IntegrationFailureReporter {
    pub fn new(integration_type: IntegrationType) -> Self {
        Self {
            integration_type,
            logged_since_success: false,
        }
    }

    pub fn reset(&mut self) {
        self.logged_since_success = false;
    }

    pub async fn record_failure(&mut self, mut error: anyhow::Error) {
        let Some(failure) = error.downcast_ref::<IntegrationEgressFailure>() else {
            tracing::error!(
                "Error emitting log event batch in {} integration: {error:?}",
                self.integration_type.as_str()
            );
            report_error(&mut error).await;
            return;
        };

        metrics::integration_egress_failure(
            self.integration_type.as_str(),
            failure.outcome_label(),
        );
        if !self.logged_since_success {
            self.logged_since_success = true;
            tracing::warn!(
                "{} integration failed to emit a log event batch: {failure}",
                self.integration_type.as_str(),
            );
        }
    }
}
