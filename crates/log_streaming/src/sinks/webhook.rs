use std::{
    collections::BTreeSet,
    ops::Deref,
    sync::{
        atomic::Ordering,
        Arc,
    },
};

use aws_lc_rs::hmac::{
    self,
    HMAC_SHA256,
};
use bytes::Bytes;
use common::{
    backoff::Backoff,
    http::{
        fetch::FetchClient,
        HttpRequest,
        APPLICATION_JSON_CONTENT_TYPE,
    },
    log_streaming::{
        LogEvent,
        LogEventFormatVersion,
        LogTopic,
    },
    runtime::{
        Runtime,
        WithTimeout,
    },
};
use const_hex::ToHexExt;
use http::{
    header::CONTENT_TYPE,
    HeaderValue,
};
use model::log_sinks::types::webhook::{
    WebhookConfig,
    WebhookFormat,
};
use parking_lot::Mutex;
use reqwest::header::HeaderMap;
use serde::Serialize;
use serde_json::Value as JsonValue;
use tokio::sync::mpsc;

use crate::{
    consts,
    metrics::webhook_sink_network_egress_bytes,
    sinks::{
        failure::{
            classify_sink_response,
            SinkEgressFailure,
            SinkFailureReporter,
        },
        utils::{
            self,
            build_event_batches,
            EgressCounter,
            SinkFilter,
        },
    },
    LogSinkClient,
    LoggingDeploymentMetadata,
};

pub const LOG_EVENT_FORMAT_FOR_WEBHOOK: LogEventFormatVersion = LogEventFormatVersion::V2;

#[derive(Serialize, Debug, Clone)]
struct WebhookLogEvent<'a> {
    #[serde(flatten)]
    event: serde_json::Map<String, JsonValue>,
    convex: &'a LoggingDeploymentMetadata,
}

impl<'a> WebhookLogEvent<'a> {
    fn new(
        event: LogEvent,
        deployment_metadata: &'a LoggingDeploymentMetadata,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            event: event.to_json_map(LOG_EVENT_FORMAT_FOR_WEBHOOK)?,
            convex: deployment_metadata,
        })
    }
}

pub struct WebhookSink<RT: Runtime> {
    runtime: RT,
    config: WebhookConfig,
    filter: SinkFilter,
    fetch_client: Arc<dyn FetchClient>,
    events_receiver: mpsc::Receiver<Vec<Arc<LogEvent>>>,
    backoff: Backoff,
    deployment_metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
    egress_counter: EgressCounter,
    failure_reporter: SinkFailureReporter,
}

impl<RT: Runtime> WebhookSink<RT> {
    pub async fn start(
        runtime: RT,
        config: WebhookConfig,
        subscribed_topics: Option<BTreeSet<LogTopic>>,
        fetch_client: Arc<dyn FetchClient>,
        deployment_metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
        egress_counter: EgressCounter,
        failure_reporter: SinkFailureReporter,
        should_verify: bool,
    ) -> anyhow::Result<LogSinkClient> {
        tracing::info!("Starting WebhookSink");
        let (tx, rx) = mpsc::channel(consts::WEBHOOK_SINK_EVENTS_BUFFER_SIZE);

        let mut sink = Self {
            runtime: runtime.clone(),
            config,
            filter: SinkFilter::for_version(LOG_EVENT_FORMAT_FOR_WEBHOOK, subscribed_topics),
            fetch_client,
            events_receiver: rx,
            backoff: Backoff::new(
                consts::WEBHOOK_SINK_INITIAL_BACKOFF,
                consts::WEBHOOK_SINK_MAX_BACKOFF,
            ),
            deployment_metadata,
            egress_counter,
            failure_reporter,
        };

        if should_verify {
            sink.verify_initial_request().await?;
            tracing::info!("WebhookSink verified!");
        }

        let handle = Arc::new(Mutex::new(runtime.spawn("webhook_sink", sink.go())));

        Ok(LogSinkClient {
            _handle: handle,
            events_sender: tx,
        })
    }

    async fn verify_initial_request(&mut self) -> anyhow::Result<()> {
        let verification_event = LogEvent::default_for_verification(&self.runtime)?;
        let deployment_metadata = self.deployment_metadata.lock().clone();
        let payload = WebhookLogEvent::new(verification_event, &deployment_metadata)?;
        self.send_batch(vec![payload], true, false).await?;

        Ok(())
    }

    async fn go(mut self) {
        loop {
            match self.events_receiver.recv().await {
                None => {
                    // The sender was closed, event loop should shutdown
                    tracing::warn!("Stopping WebhookSink. Sender was closed.");
                    return;
                },
                Some(ev) => {
                    // Split events into batches
                    let batches = build_event_batches(
                        ev,
                        consts::WEBHOOK_SINK_MAX_LOGS_PER_BATCH,
                        &self.filter,
                    );

                    // Process each batch and send to Datadog
                    for batch in batches {
                        let track_egress = utils::batch_has_non_egress_events(&batch);
                        match self.process_events(batch, track_egress).await {
                            Ok(()) => {
                                self.backoff.reset();
                                self.failure_reporter.reset();
                            },
                            Err(e) => self.failure_reporter.record_failure(e).await,
                        }
                    }
                },
            }
        }
    }

    async fn send_batch(
        &mut self,
        batch: Vec<WebhookLogEvent<'_>>,
        is_verification: bool,
        track_egress: bool,
    ) -> anyhow::Result<()> {
        let mut batch_json: Vec<JsonValue> = vec![];
        for ev in batch {
            batch_json.push(serde_json::to_value(ev)?);
        }
        let payload = match self.config.format {
            WebhookFormat::Json => serde_json::to_vec(&JsonValue::Array(batch_json))?,
            WebhookFormat::Jsonl => batch_json
                .into_iter()
                .map(|v| Ok(serde_json::to_vec(&v)?))
                .collect::<anyhow::Result<Vec<Vec<u8>>>>()?
                .join("\n".as_bytes()),
        };
        let payload = Bytes::from(payload);

        // Create HMAC-SHA256 signature
        let s_key = hmac::Key::new(HMAC_SHA256, self.config.hmac_secret.as_ref());
        let signature: String = hmac::sign(&s_key, &payload).encode_hex();

        let mut headers = HeaderMap::from_iter([(CONTENT_TYPE, APPLICATION_JSON_CONTENT_TYPE)]);
        headers.append(
            "x-webhook-signature",
            HeaderValue::from_str(format!("sha256={signature}").as_str())?,
        );

        // Make request in a loop that retries on transient errors
        let mut last_failure = None;
        let max_attempts = if is_verification {
            consts::WEBHOOK_SINK_VERIFICATION_MAX_ATTEMPTS
        } else {
            consts::WEBHOOK_SINK_MAX_REQUEST_ATTEMPTS
        };
        for _ in 0..max_attempts {
            let response = self
                .runtime
                .with_timeout(
                    "webhook_sink_request",
                    consts::WEBHOOK_SINK_REQUEST_TIMEOUT,
                    self.fetch_client.fetch(
                        HttpRequest {
                            url: self.config.url.clone(),
                            method: http::Method::POST,
                            headers: headers.clone(),
                            body: Some(payload.clone()),
                        }
                        .into(),
                    ),
                )
                .await;

            if !is_verification
                && track_egress
                && let Ok(r) = &response
            {
                let num_bytes_egress = r.request_size.load(Ordering::Relaxed);
                utils::track_log_sink_bandwidth(
                    num_bytes_egress,
                    &self.egress_counter,
                    webhook_sink_network_egress_bytes,
                );
            }

            match classify_sink_response(response) {
                Ok(()) => return Ok(()),
                Err(failure) if failure.is_rejected() => anyhow::bail!(failure),
                Err(failure) => {
                    let delay = self.backoff.fail(&mut self.runtime.rng());
                    tracing::warn!(
                        "Failed to send in Webhook sink, waiting {delay:?} before retrying: \
                         {failure}"
                    );
                    last_failure = Some(failure);
                    self.runtime.wait(delay).await;
                },
            }
        }

        anyhow::bail!(SinkEgressFailure::retries_exhausted(
            max_attempts,
            last_failure
        ))
    }

    async fn process_events(
        &mut self,
        events: Vec<Arc<LogEvent>>,
        track_egress: bool,
    ) -> anyhow::Result<()> {
        crate::metrics::webhook_sink_logs_received(events.len());

        let mut values_to_send = vec![];
        let deployment_metadata = self.deployment_metadata.lock().clone();
        for event in events {
            match WebhookLogEvent::new(event.deref().clone(), &deployment_metadata) {
                Err(e) => tracing::warn!("failed to convert log to JSON: {:?}", e),
                Ok(v) => values_to_send.push(v),
            }
        }

        if values_to_send.is_empty() {
            anyhow::bail!("skipping an entire batch due to logs that failed to be processed");
        }
        let batch_size = values_to_send.len();

        self.send_batch(values_to_send, false, track_egress).await?;
        crate::metrics::webhook_sink_logs_sent(batch_size);

        Ok(())
    }
}
