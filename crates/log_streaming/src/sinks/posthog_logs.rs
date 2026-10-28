use std::{
    collections::BTreeSet,
    sync::{
        atomic::Ordering,
        Arc,
    },
};

use anyhow::Context;
use bytes::Bytes;
use common::{
    backoff::Backoff,
    http::{
        fetch::FetchClient,
        HttpRequestStream,
        APPLICATION_JSON_CONTENT_TYPE,
    },
    log_lines::LogLevel,
    log_streaming::{
        LogEvent,
        LogEventFormatVersion,
        LogTopic,
        StructuredLogEvent,
    },
    runtime::Runtime,
};
use http::{
    header::{
        AUTHORIZATION,
        CONTENT_TYPE,
    },
    HeaderMap,
    HeaderValue,
};
use model::log_sinks::types::posthog_logs::{
    PostHogLogsConfig,
    DEFAULT_POSTHOG_HOST,
};
use parking_lot::Mutex;
use serde_json::{
    json,
    Value as JsonValue,
};
use tokio::sync::mpsc;

use crate::{
    consts,
    metrics::posthog_logs_sink_network_egress_bytes,
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

pub struct PostHogLogsSink<RT: Runtime> {
    runtime: RT,
    endpoint_url: reqwest::Url,
    api_key: String,
    service_name: String,
    filter: SinkFilter,
    fetch_client: Arc<dyn FetchClient>,
    events_receiver: mpsc::Receiver<Vec<Arc<LogEvent>>>,
    backoff: Backoff,
    deployment_metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
    egress_counter: EgressCounter,
    failure_reporter: SinkFailureReporter,
}

impl<RT: Runtime> PostHogLogsSink<RT> {
    pub async fn start(
        runtime: RT,
        config: PostHogLogsConfig,
        subscribed_topics: Option<BTreeSet<LogTopic>>,
        fetch_client: Arc<dyn FetchClient>,
        deployment_metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
        egress_counter: EgressCounter,
        failure_reporter: SinkFailureReporter,
        should_verify: bool,
    ) -> anyhow::Result<LogSinkClient> {
        tracing::info!("Starting PostHogLogsSink");
        let (tx, rx) = mpsc::channel(consts::POSTHOG_LOGS_SINK_EVENTS_BUFFER_SIZE);

        let host = config.host.as_deref().unwrap_or(DEFAULT_POSTHOG_HOST);
        let endpoint_url = format!("{host}/i/v1/logs");

        let service_name = config
            .service_name
            .unwrap_or_else(|| deployment_metadata.lock().deployment_name.clone());

        let mut sink = Self {
            runtime: runtime.clone(),
            deployment_metadata,
            endpoint_url: endpoint_url.parse()?,
            api_key: config.api_key.into_value(),
            service_name,
            filter: SinkFilter::for_version(LogEventFormatVersion::V2, subscribed_topics),
            fetch_client,
            events_receiver: rx,
            backoff: Backoff::new(
                consts::POSTHOG_LOGS_SINK_INITIAL_BACKOFF,
                consts::POSTHOG_LOGS_SINK_MAX_BACKOFF,
            ),
            egress_counter,
            failure_reporter,
        };

        if should_verify {
            sink.verify_creds().await?;
            tracing::info!("PostHogLogsSink verified!");
        }

        let handle = Arc::new(Mutex::new(runtime.spawn("posthog_logs_sink", sink.go())));
        let client = LogSinkClient {
            _handle: handle,
            events_sender: tx,
        };
        Ok(client)
    }

    async fn verify_creds(&mut self) -> anyhow::Result<()> {
        // PostHog's ingestion endpoints return 200 even for invalid project tokens,
        // so we use the /decide endpoint which actually validates the token.
        let mut decide_url = self.endpoint_url.clone();
        decide_url.set_path("/decide");
        decide_url.set_query(Some("v=3"));

        let payload = json!({
            "api_key": self.api_key,
            "distinct_id": "convex-verification",
        });
        let header_map = HeaderMap::from_iter([(CONTENT_TYPE, APPLICATION_JSON_CONTENT_TYPE)]);
        let body = Bytes::from(serde_json::to_vec(&payload)?);

        let response = self
            .fetch_client
            .fetch(HttpRequestStream {
                url: decide_url,
                method: http::Method::POST,
                headers: header_map,
                body: Some(Box::pin(futures::stream::once(async { Ok(body) }))),
                signal: Box::pin(futures::future::pending()),
            })
            .await;

        classify_sink_response(response).context("Failed to verify PostHog project token")
    }

    async fn go(mut self) {
        loop {
            match self.events_receiver.recv().await {
                None => {
                    tracing::warn!("Stopping PostHogLogsSink. Sender was closed.");
                    return;
                },
                Some(ev) => {
                    let batches = build_event_batches(
                        ev,
                        consts::POSTHOG_LOGS_SINK_MAX_LOGS_PER_BATCH,
                        &self.filter,
                    );

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

    fn build_resource_attributes(&self, metadata: &LoggingDeploymentMetadata) -> Vec<JsonValue> {
        let mut attrs = vec![
            json!({"key": "service.name", "value": {"stringValue": self.service_name}}),
            json!({"key": "convex.deployment.name", "value": {"stringValue": metadata.deployment_name}}),
        ];
        if let Some(dt) = metadata.deployment_type {
            attrs.push(
                json!({"key": "convex.deployment.type", "value": {"stringValue": dt.to_string()}}),
            );
        }
        if let Some(ref region) = metadata.deployment_region {
            attrs
                .push(json!({"key": "convex.deployment.region", "value": {"stringValue": region}}));
        }
        attrs
    }

    fn log_event_to_otlp_record(event: &LogEvent) -> anyhow::Result<JsonValue> {
        let time_unix_nano = (event.timestamp.as_ms_since_epoch()? as u128 * 1_000_000).to_string();

        let (severity_text, severity_number) = match &event.event {
            StructuredLogEvent::Console { log_line, .. } => match log_line.level {
                LogLevel::Error => ("ERROR", 17),
                LogLevel::Warn => ("WARN", 13),
                LogLevel::Info | LogLevel::Log => ("INFO", 9),
                LogLevel::Debug => ("DEBUG", 5),
            },
            StructuredLogEvent::FunctionExecution { error, .. } => {
                if error.is_some() {
                    ("ERROR", 17)
                } else {
                    ("INFO", 9)
                }
            },
            _ => ("INFO", 9),
        };

        // Serialize the full log event as the body
        let body_map = event.to_json_map(LogEventFormatVersion::V2)?;
        let body_str = serde_json::to_string(&body_map)?;

        let mut attributes = vec![];

        // Add the topic attribute
        let topic = match &event.event {
            StructuredLogEvent::Verification => "verification",
            StructuredLogEvent::Console { .. } => "console",
            StructuredLogEvent::FunctionExecution { .. } => "function_execution",
            StructuredLogEvent::DeploymentAuditLog { .. } => "deployment_audit_log",
            StructuredLogEvent::SchedulerStats { .. } => "scheduler_stats",
            StructuredLogEvent::ScheduledJobLag { .. } => "scheduled_job_lag",
            StructuredLogEvent::CurrentStorageUsage { .. } => "current_storage_usage",
            StructuredLogEvent::ConcurrencyStats { .. } => "concurrency_stats",
            StructuredLogEvent::Exception { .. } => "exception",
            StructuredLogEvent::StorageApiBandwidth { .. } => "storage_bandwidth",
            StructuredLogEvent::AiGatewayUsage { .. } => "ai_gateway_usage",
            StructuredLogEvent::LogStreamEgress { .. } => "log_stream_egress",
            StructuredLogEvent::CustomAudit { .. } => "custom_audit",
        };
        attributes.push(json!({"key": "convex.topic", "value": {"stringValue": topic}}));

        // Add function metadata when available
        match &event.event {
            StructuredLogEvent::Console { source, .. }
            | StructuredLogEvent::FunctionExecution { source, .. } => {
                attributes.push(
                    json!({"key": "convex.function.path", "value": {"stringValue": source.udf_path}}),
                );
                attributes.push(
                    json!({"key": "convex.function.type", "value": {"stringValue": source.udf_type.to_lowercase_string()}}),
                );
            },
            _ => {},
        }

        Ok(json!({
            "timeUnixNano": time_unix_nano,
            "severityText": severity_text,
            "severityNumber": severity_number,
            "body": { "stringValue": body_str },
            "attributes": attributes,
        }))
    }

    async fn process_events(
        &mut self,
        events: Vec<Arc<LogEvent>>,
        track_egress: bool,
    ) -> anyhow::Result<()> {
        crate::metrics::posthog_logs_sink_logs_received(events.len());

        let mut log_records = vec![];
        for event in &events {
            match Self::log_event_to_otlp_record(event) {
                Err(e) => tracing::warn!("Failed to convert log to OTLP record: {:?}", e),
                Ok(record) => log_records.push(record),
            }
        }

        if log_records.is_empty() {
            anyhow::bail!("Skipping an entire batch due to logs that failed to be serialized");
        }
        let batch_size = log_records.len();

        let deployment_metadata = self.deployment_metadata.lock().clone();
        let payload = json!({
            "resourceLogs": [{
                "resource": {
                    "attributes": self.build_resource_attributes(&deployment_metadata)
                },
                "scopeLogs": [{
                    "scope": { "name": "convex" },
                    "logRecords": log_records
                }]
            }]
        });

        self.send_batch(serde_json::to_vec(&payload)?, track_egress)
            .await?;
        crate::metrics::posthog_logs_sink_logs_sent(batch_size);

        Ok(())
    }

    async fn send_batch(&mut self, batch_json: Vec<u8>, track_egress: bool) -> anyhow::Result<()> {
        let header_map = HeaderMap::from_iter([
            (
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", self.api_key))?,
            ),
            (CONTENT_TYPE, APPLICATION_JSON_CONTENT_TYPE),
        ]);
        let batch_json = Bytes::from(batch_json);

        let mut last_failure = None;
        for _ in 0..consts::POSTHOG_LOGS_SINK_MAX_REQUEST_ATTEMPTS {
            let batch_json = batch_json.clone();
            let response = self
                .fetch_client
                .fetch(HttpRequestStream {
                    url: self.endpoint_url.clone(),
                    method: http::Method::POST,
                    headers: header_map.clone(),
                    body: Some(Box::pin(futures::stream::once(async { Ok(batch_json) }))),
                    signal: Box::pin(futures::future::pending()),
                })
                .await;

            if track_egress && let Ok(r) = &response {
                let num_bytes_egress = r.request_size.load(Ordering::Relaxed);
                utils::track_log_sink_bandwidth(
                    num_bytes_egress,
                    &self.egress_counter,
                    posthog_logs_sink_network_egress_bytes,
                );
            }

            match classify_sink_response(response) {
                Ok(()) => return Ok(()),
                Err(failure) if failure.is_rejected() => anyhow::bail!(failure),
                Err(failure) => {
                    let delay = self.backoff.fail(&mut self.runtime.rng());
                    tracing::warn!(
                        "Failed to send in PostHog Logs sink: {failure}. Waiting {delay:?} before \
                         retrying."
                    );
                    last_failure = Some(failure);
                    self.runtime.wait(delay).await;
                },
            }
        }

        anyhow::bail!(SinkEgressFailure::retries_exhausted(
            consts::POSTHOG_LOGS_SINK_MAX_REQUEST_ATTEMPTS,
            last_failure,
        ))
    }
}
