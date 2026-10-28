use std::sync::{
    atomic::Ordering,
    Arc,
};

use anyhow::Context;
use bytes::Bytes;
use chrono::{
    DateTime,
    Utc,
};
use common::{
    backoff::Backoff,
    http::{
        fetch::FetchClient,
        HttpRequestStream,
        APPLICATION_JSON_CONTENT_TYPE,
    },
    log_streaming::{
        LogEvent,
        StructuredLogEvent,
    },
    runtime::Runtime,
};
use http::{
    header::CONTENT_TYPE,
    HeaderMap,
};
use model::log_sinks::types::posthog_logs::DEFAULT_POSTHOG_HOST;
use parking_lot::Mutex;
use serde_json::{
    json,
    Value as JsonValue,
};
use tokio::sync::mpsc;

use crate::{
    consts,
    metrics::posthog_et_sink_network_egress_bytes,
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

pub struct PostHogErrorTrackingSink<RT: Runtime> {
    runtime: RT,
    capture_url: reqwest::Url,
    api_key: String,
    fetch_client: Arc<dyn FetchClient>,
    events_receiver: mpsc::Receiver<Vec<Arc<LogEvent>>>,
    backoff: Backoff,
    deployment_metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
    egress_counter: EgressCounter,
    failure_reporter: SinkFailureReporter,
}

impl<RT: Runtime> PostHogErrorTrackingSink<RT> {
    pub async fn start(
        runtime: RT,
        config: model::log_sinks::types::posthog_error_tracking::PostHogErrorTrackingConfig,
        fetch_client: Arc<dyn FetchClient>,
        deployment_metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
        egress_counter: EgressCounter,
        failure_reporter: SinkFailureReporter,
        should_verify: bool,
    ) -> anyhow::Result<LogSinkClient> {
        tracing::info!("Starting PostHogErrorTrackingSink");
        let (tx, rx) = mpsc::channel(consts::POSTHOG_ET_SINK_EVENTS_BUFFER_SIZE);

        let host = config.host.as_deref().unwrap_or(DEFAULT_POSTHOG_HOST);
        let capture_url = format!("{host}/i/v0/e/");

        let mut sink = Self {
            runtime: runtime.clone(),
            deployment_metadata,
            capture_url: capture_url.parse()?,
            api_key: config.api_key.into_value(),
            fetch_client,
            events_receiver: rx,
            backoff: Backoff::new(
                consts::POSTHOG_ET_SINK_INITIAL_BACKOFF,
                consts::POSTHOG_ET_SINK_MAX_BACKOFF,
            ),
            egress_counter,
            failure_reporter,
        };

        if should_verify {
            sink.verify_creds().await?;
            tracing::info!("PostHogErrorTrackingSink verified!");
        }

        let handle = Arc::new(Mutex::new(runtime.spawn("posthog_et_sink", sink.go())));
        let client = LogSinkClient {
            _handle: handle,
            events_sender: tx,
        };
        Ok(client)
    }

    async fn verify_creds(&mut self) -> anyhow::Result<()> {
        // PostHog's ingestion endpoints return 200 even for invalid project tokens,
        // so we use the /decide endpoint which actually validates the token.
        let mut decide_url = self.capture_url.clone();
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
                    tracing::warn!("Stopping PostHogErrorTrackingSink. Sender was closed.");
                    return;
                },
                Some(ev) => {
                    let batches = build_event_batches(
                        ev,
                        consts::POSTHOG_ET_SINK_MAX_LOGS_PER_BATCH,
                        &SinkFilter::OnlyExceptions,
                    );

                    for batch in batches {
                        match self.process_events(batch).await {
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

    async fn process_events(&mut self, events: Vec<Arc<LogEvent>>) -> anyhow::Result<()> {
        crate::metrics::posthog_et_sink_logs_received(events.len());

        if events.is_empty() {
            return Ok(());
        }

        let deployment_metadata = self.deployment_metadata.lock().clone();
        let mut batch_events = vec![];

        for event in &events {
            let StructuredLogEvent::Exception {
                error,
                user_identifier,
                source,
                udf_server_version,
                request_metadata: _,
            } = &event.event
            else {
                continue;
            };

            let distinct_id = user_identifier
                .as_ref()
                .map(|u| u.to_string())
                .unwrap_or_else(|| deployment_metadata.deployment_name.clone());

            let timestamp: DateTime<Utc> = event.timestamp.as_system_time().into();
            let timestamp_str = timestamp.to_rfc3339();

            // Build frames (NOT reversed — PostHog expects oldest-to-newest)
            let frames: Vec<JsonValue> = error
                .frames
                .as_ref()
                .map(|js_frames| {
                    js_frames
                        .0
                        .iter()
                        .map(|frame| {
                            let function = frame
                                .function_name
                                .as_deref()
                                .or(frame.method_name.as_deref())
                                .unwrap_or("<anonymous>");
                            json!({
                                "platform": "custom",
                                "lang": "javascript",
                                "filename": frame.file_name,
                                "function": function,
                                "lineno": frame.line_number,
                                "colno": frame.column_number,
                                "in_app": true,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            let exception_sources: Vec<&str> = error
                .frames
                .as_ref()
                .map(|f| {
                    f.0.iter()
                        .filter_map(|frame| frame.file_name.as_deref())
                        .collect()
                })
                .unwrap_or_default();

            let exception_functions: Vec<&str> = error
                .frames
                .as_ref()
                .map(|f| {
                    f.0.iter()
                        .map(|frame| {
                            frame
                                .function_name
                                .as_deref()
                                .or(frame.method_name.as_deref())
                                .unwrap_or("<anonymous>")
                        })
                        .collect()
                })
                .unwrap_or_default();

            let lib_version = udf_server_version
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let mut exception = json!({
                "type": "Error",
                "value": error.message,
                "mechanism": { "handled": false, "type": "generic" },
            });
            if !frames.is_empty() {
                exception["stacktrace"] = json!({
                    "type": "raw",
                    "frames": frames,
                });
            }

            let capture_event = json!({
                "event": "$exception",
                "distinct_id": distinct_id,
                "timestamp": timestamp_str,
                "properties": {
                    "$exception_list": [exception],
                    "$exception_level": "error",
                    "$exception_types": ["Error"],
                    "$exception_values": [error.message],
                    "$exception_sources": exception_sources,
                    "$exception_functions": exception_functions,
                    "$lib": "convex",
                    "$lib_version": lib_version,
                    "convex_function": source.udf_path,
                    "convex_function_type": source.udf_type.to_lowercase_string(),
                    "convex_function_runtime": source.module_environment.as_sentry_tag(),
                    "convex_deployment": deployment_metadata.deployment_name,
                    "convex_deployment_type": deployment_metadata.deployment_type.map(|dt| dt.to_string()),
                    "convex_request_id": source.context.request_id.to_string(),
                }
            });

            batch_events.push(capture_event);
        }

        if batch_events.is_empty() {
            return Ok(());
        }
        let batch_size = batch_events.len();

        let payload = json!({
            "api_key": self.api_key,
            "batch": batch_events,
        });

        self.send_batch(serde_json::to_vec(&payload)?).await?;
        crate::metrics::posthog_et_sink_logs_sent(batch_size);

        Ok(())
    }

    async fn send_batch(&mut self, batch_json: Vec<u8>) -> anyhow::Result<()> {
        // PostHog capture API uses the project token in the body, no Authorization
        // header
        let header_map = HeaderMap::from_iter([(CONTENT_TYPE, APPLICATION_JSON_CONTENT_TYPE)]);
        let batch_json = Bytes::from(batch_json);

        let mut last_failure = None;
        for _ in 0..consts::POSTHOG_ET_SINK_MAX_REQUEST_ATTEMPTS {
            let batch_json = batch_json.clone();
            let response = self
                .fetch_client
                .fetch(HttpRequestStream {
                    url: self.capture_url.clone(),
                    method: http::Method::POST,
                    headers: header_map.clone(),
                    body: Some(Box::pin(futures::stream::once(async { Ok(batch_json) }))),
                    signal: Box::pin(futures::future::pending()),
                })
                .await;

            if let Ok(r) = &response {
                let num_bytes_egress = r.request_size.load(Ordering::Relaxed);
                utils::track_log_sink_bandwidth(
                    num_bytes_egress,
                    &self.egress_counter,
                    posthog_et_sink_network_egress_bytes,
                );
            }

            match classify_sink_response(response) {
                Ok(()) => return Ok(()),
                Err(failure) if failure.is_rejected() => anyhow::bail!(failure),
                Err(failure) => {
                    let delay = self.backoff.fail(&mut self.runtime.rng());
                    tracing::warn!(
                        "Failed to send in PostHog Error Tracking sink: {failure}. Waiting \
                         {delay:?} before retrying."
                    );
                    last_failure = Some(failure);
                    self.runtime.wait(delay).await;
                },
            }
        }

        anyhow::bail!(SinkEgressFailure::retries_exhausted(
            consts::POSTHOG_ET_SINK_MAX_REQUEST_ATTEMPTS,
            last_failure,
        ))
    }
}
