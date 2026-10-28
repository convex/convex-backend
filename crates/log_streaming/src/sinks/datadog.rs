use std::{
    collections::BTreeSet,
    ops::Deref,
    sync::{
        atomic::Ordering,
        Arc,
    },
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
        StructuredLogEvent,
    },
    runtime::Runtime,
};
use http::header::CONTENT_TYPE;
use model::log_sinks::types::datadog::DatadogConfig;
use parking_lot::Mutex;
use reqwest::header::{
    HeaderMap,
    HeaderName,
    HeaderValue,
};
use serde::Serialize;
use serde_json::{
    value::RawValue,
    Value as JsonValue,
};
use tokio::sync::mpsc;

use crate::{
    consts,
    metrics::datadog_sink_network_egress_bytes,
    sinks::{
        failure::{
            classify_sink_response,
            SinkEgressFailure,
            SinkFailureReporter,
        },
        utils::{
            self,
            build_sized_batches,
            EgressCounter,
            SinkFilter,
        },
    },
    LogSinkClient,
    LoggingDeploymentMetadata,
};

const DD_API_KEY_HEADER: &str = "DD-API-KEY";

#[derive(Debug, Clone)]
pub struct DatadogMetadata {
    ddtags: String,
    hostname: String,
    service: Option<String>,
}

impl DatadogMetadata {
    pub fn new(ddtags: Vec<String>, instance_name: String, service: Option<String>) -> Self {
        let ddtags = ddtags.join(",");

        Self {
            ddtags,
            hostname: instance_name,
            service,
        }
    }
}

#[derive(Serialize, Debug, Clone)]
struct DatadogLogEvent<'a> {
    ddsource: String,
    ddtags: String,
    hostname: String,
    service: Option<String>,
    #[serde(flatten)]
    event: serde_json::Map<String, JsonValue>,
    convex: &'a LoggingDeploymentMetadata,
}

impl<'a> DatadogLogEvent<'a> {
    fn new(
        event: LogEvent,
        metadata: &DatadogMetadata,
        format: LogEventFormatVersion,
        deployment_metadata: &'a LoggingDeploymentMetadata,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            ddsource: "convex".to_string(),
            ddtags: metadata.ddtags.clone(),
            hostname: metadata.hostname.clone(),
            service: metadata.service.clone(),
            event: event.to_json_map(format)?,
            convex: deployment_metadata,
        })
    }
}

/// A log event serialized to its JSON object bytes, ready to be packed into a
/// batch payload.
struct SerializedEvent {
    json: Box<RawValue>,
    /// Whether the underlying event is a `LogStreamEgress` event, which is
    /// excluded from egress billing.
    is_egress: bool,
}

pub(crate) struct DatadogSink<RT: Runtime> {
    runtime: RT,
    fetch_client: Arc<dyn FetchClient>,
    dd_url: reqwest::Url,
    dd_api_key: String,
    metadata: DatadogMetadata,
    log_event_format: LogEventFormatVersion,
    filter: SinkFilter,
    events_receiver: mpsc::Receiver<Vec<Arc<LogEvent>>>,
    backoff: Backoff,
    deployment_metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
    egress_counter: EgressCounter,
    failure_reporter: SinkFailureReporter,
}

impl<RT: Runtime> DatadogSink<RT> {
    pub async fn start(
        runtime: RT,
        fetch_client: Arc<dyn FetchClient>,
        config: DatadogConfig,
        subscribed_topics: Option<BTreeSet<LogTopic>>,
        deployment_metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
        egress_counter: EgressCounter,
        failure_reporter: SinkFailureReporter,
        should_verify: bool,
    ) -> anyhow::Result<LogSinkClient> {
        tracing::info!("Starting DatadogSink");
        let (tx, rx) = mpsc::channel(consts::DD_SINK_EVENTS_BUFFER_SIZE);

        let metadata = DatadogMetadata::new(
            config.dd_tags,
            deployment_metadata.lock().deployment_name.clone(),
            config.service,
        );

        let filter = SinkFilter::for_version(config.version, subscribed_topics);
        let mut sink = Self {
            runtime: runtime.clone(),
            dd_url: config.site_location.get_logging_endpoint()?,
            dd_api_key: config.dd_api_key.into_value(),
            metadata,
            log_event_format: config.version,
            filter,
            events_receiver: rx,
            fetch_client,
            backoff: Backoff::new(consts::DD_SINK_INITIAL_BACKOFF, consts::DD_SINK_MAX_BACKOFF),
            deployment_metadata: deployment_metadata.clone(),
            egress_counter,
            failure_reporter,
        };

        if should_verify {
            sink.verify_creds().await?;
            tracing::info!("DatadogSink verified!");
        }

        let handle = Arc::new(Mutex::new(runtime.spawn("datadog_sink", sink.go())));
        let client = LogSinkClient {
            _handle: handle,
            events_sender: tx,
        };
        Ok(client)
    }

    async fn go(mut self) {
        loop {
            match self.events_receiver.recv().await {
                None => {
                    // The sender was closed, event loop should shutdown
                    tracing::warn!("Stopping DatadogSink. Sender was closed.");
                    return;
                },
                Some(ev) => {
                    let events: Vec<_> = ev
                        .into_iter()
                        .filter(|event| self.filter.allows(event))
                        .collect();
                    if events.is_empty() {
                        continue;
                    }
                    if let Err(e) = self.process_events(events).await {
                        self.failure_reporter.record_failure(e).await;
                    }
                },
            }
        }
    }

    /// Verify that an initial request succeeds
    async fn verify_creds(&mut self) -> anyhow::Result<()> {
        let verification_event = LogEvent::default_for_verification(&self.runtime)?;
        let deployment_metadata = self.deployment_metadata.lock().clone();
        let event = DatadogLogEvent::new(
            verification_event,
            &self.metadata,
            self.log_event_format,
            &deployment_metadata,
        )?;
        let event_bytes = serde_json::value::to_raw_value(&event)?;
        self.send_batch(vec![&event_bytes], true, false).await?;

        Ok(())
    }

    async fn send_batch(
        &mut self,
        events: Vec<&RawValue>,
        is_verification: bool,
        track_egress: bool,
    ) -> anyhow::Result<()> {
        let payload = Bytes::from(serde_json::to_vec(&events)?);
        drop(events);
        let header_map = HeaderMap::from_iter([
            (
                HeaderName::from_bytes(DD_API_KEY_HEADER.as_bytes())?,
                HeaderValue::from_str(&self.dd_api_key)?,
            ),
            (CONTENT_TYPE, APPLICATION_JSON_CONTENT_TYPE),
        ]);

        // Make request in a loop that retries on transient errors
        let mut last_failure = None;
        for _ in 0..consts::DD_SINK_MAX_REQUEST_ATTEMPTS {
            let response = self
                .fetch_client
                .fetch(
                    HttpRequest {
                        url: self.dd_url.clone(),
                        method: http::Method::POST,
                        headers: header_map.clone(),
                        body: Some(payload.clone()),
                    }
                    .into(),
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
                    datadog_sink_network_egress_bytes,
                );
            }

            match classify_sink_response(response) {
                Ok(()) => return Ok(()),
                Err(failure) if failure.is_rejected() => anyhow::bail!(failure),
                Err(failure) => {
                    let delay = self.backoff.fail(&mut self.runtime.rng());
                    tracing::warn!(
                        "Failed to send in Datadog sink: {failure}. Waiting {delay:?} before \
                         retrying."
                    );
                    last_failure = Some(failure);
                    self.runtime.wait(delay).await;
                },
            }
        }

        anyhow::bail!(SinkEgressFailure::retries_exhausted(
            consts::DD_SINK_MAX_REQUEST_ATTEMPTS,
            last_failure,
        ))
    }

    /// Serialize a drain's worth of events, pack them into batches within the
    /// entry and byte budgets, and send each batch. Send failures are recorded
    /// per batch so one failed batch doesn't drop the rest of the drain; the
    /// returned error covers only serialization failures.
    async fn process_events(&mut self, events: Vec<Arc<LogEvent>>) -> anyhow::Result<()> {
        let log_event_format_version = match self.log_event_format {
            LogEventFormatVersion::V1 => "1",
            LogEventFormatVersion::V2 => "2",
        };
        crate::metrics::datadog_sink_logs_received(events.len(), log_event_format_version);

        let mut serialized_events = vec![];
        let deployment_metadata = self.deployment_metadata.lock().clone();
        for event in events {
            let is_egress = matches!(event.event, StructuredLogEvent::LogStreamEgress { .. });
            let dd_event = DatadogLogEvent::new(
                event.deref().clone(),
                &self.metadata,
                self.log_event_format,
                &deployment_metadata,
            )
            .and_then(|v| serde_json::value::to_raw_value(&v).map_err(From::from));
            match dd_event {
                Err(e) => tracing::warn!("failed to convert log to JSON: {:?}", e),
                Ok(json) => serialized_events.push(SerializedEvent { json, is_egress }),
            }
        }

        if serialized_events.is_empty() {
            anyhow::bail!("skipping an entire drain due to logs that failed to be serialized");
        }

        let batches = build_sized_batches(
            &serialized_events,
            |ev| ev.json.get().len(),
            consts::DD_SINK_MAX_LOGS_PER_BATCH,
            consts::DD_SINK_MAX_BATCH_BYTES,
        );
        for batch in batches {
            let track_egress = batch.iter().any(|ev| !ev.is_egress);
            match self
                .send_batch(
                    batch.iter().map(|batch| &*batch.json).collect(),
                    false,
                    track_egress,
                )
                .await
            {
                Ok(()) => {
                    crate::metrics::datadog_sink_logs_sent(batch.len(), log_event_format_version);
                    self.backoff.reset();
                    self.failure_reporter.reset();
                },
                Err(e) => self.failure_reporter.record_failure(e).await,
            }
        }

        Ok(())
    }
}
