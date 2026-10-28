use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
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
        HttpRequestStream,
        APPLICATION_JSON_CONTENT_TYPE,
    },
    log_streaming::{
        LogEvent,
        LogEventFormatVersion,
        LogTopic,
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
use model::log_sinks::types::axiom::AxiomConfig;
use parking_lot::Mutex;
use serde::{
    Serialize,
    Serializer,
};
use tokio::sync::mpsc;

use crate::{
    consts,
    metrics::axiom_sink_network_egress_bytes,
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

#[derive(Serialize, Debug, Clone)]
pub struct AxiomLogEvent<'a> {
    _time: u64,
    #[serde(serialize_with = "serialize_log_event")]
    data: (&'a LogEvent, LogEventFormatVersion),
    attributes: &'a BTreeMap<String, String>,
    convex: &'a LoggingDeploymentMetadata,
}

fn serialize_log_event<S: Serializer>(
    &(event, format): &(&LogEvent, LogEventFormatVersion),
    serializer: S,
) -> Result<S::Ok, S::Error> {
    event.to_json_serializer(format, serializer)
}

impl<'a> AxiomLogEvent<'a> {
    fn new(
        event: &'a LogEvent,
        attributes: &'a BTreeMap<String, String>,
        format: LogEventFormatVersion,
        deployment_metadata: &'a LoggingDeploymentMetadata,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            _time: event.timestamp.as_ms_since_epoch()?,
            data: (event, format),
            attributes,
            convex: deployment_metadata,
        })
    }
}

pub struct AxiomSink<RT: Runtime> {
    runtime: RT,
    axiom_url: reqwest::Url,
    api_key: String,
    attributes: BTreeMap<String, String>,
    log_event_format: LogEventFormatVersion,
    filter: SinkFilter,
    fetch_client: Arc<dyn FetchClient>,
    events_receiver: mpsc::Receiver<Vec<Arc<LogEvent>>>,
    backoff: Backoff,
    deployment_metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
    egress_counter: EgressCounter,
    failure_reporter: SinkFailureReporter,
}

impl<RT: Runtime> AxiomSink<RT> {
    pub async fn start(
        runtime: RT,
        config: AxiomConfig,
        subscribed_topics: Option<BTreeSet<LogTopic>>,
        fetch_client: Arc<dyn FetchClient>,
        deployment_metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
        egress_counter: EgressCounter,
        failure_reporter: SinkFailureReporter,
        should_verify: bool,
    ) -> anyhow::Result<LogSinkClient> {
        tracing::info!("Starting AxiomSink");
        let (tx, rx) = mpsc::channel(consts::AXIOM_SINK_EVENTS_BUFFER_SIZE);

        let base_url = config
            .ingest_url
            .as_deref()
            .unwrap_or("https://api.axiom.co");
        let is_default_url = base_url == "https://api.axiom.co";
        let axiom_url = if is_default_url {
            format!(
                "{}/v1/datasets/{:}/ingest",
                base_url,
                config.dataset_name.clone()
            )
        } else {
            format!("{}/v1/ingest/{:}", base_url, config.dataset_name.clone())
        };

        let mut sink = Self {
            runtime: runtime.clone(),
            deployment_metadata,
            axiom_url: axiom_url.parse()?,
            api_key: config.api_key.into_value(),
            attributes: config
                .attributes
                .into_iter()
                .map(|a| (a.key, a.value))
                .collect(),
            filter: SinkFilter::for_version(config.version, subscribed_topics),
            log_event_format: config.version,
            fetch_client,
            events_receiver: rx,
            backoff: Backoff::new(
                consts::AXIOM_SINK_INITIAL_BACKOFF,
                consts::AXIOM_SINK_MAX_BACKOFF,
            ),
            egress_counter,
            failure_reporter,
        };

        if should_verify {
            sink.verify_creds().await?;
            tracing::info!("AxiomSink verified!");
        }

        let handle = Arc::new(Mutex::new(runtime.spawn("axiom_sink", sink.go())));
        let client = LogSinkClient {
            _handle: handle,
            events_sender: tx,
        };
        Ok(client)
    }

    async fn verify_creds(&mut self) -> anyhow::Result<()> {
        let verification_event = LogEvent::default_for_verification(&self.runtime)?;
        let deployment_metadata = self.deployment_metadata.lock().clone();
        let payload = AxiomLogEvent::new(
            &verification_event,
            &self.attributes,
            self.log_event_format,
            &deployment_metadata,
        )?;
        self.send_batch(serde_json::to_vec(&vec![payload])?, true, false)
            .await?;

        Ok(())
    }

    async fn go(mut self) {
        loop {
            match self.events_receiver.recv().await {
                None => {
                    // The sender was closed, event loop should shutdown
                    tracing::warn!("Stopping AxiomSink. Sender was closed.");
                    return;
                },
                Some(ev) => {
                    // Split events into batches
                    let batches = build_event_batches(
                        ev,
                        consts::AXIOM_SINK_MAX_LOGS_PER_BATCH,
                        &self.filter,
                    );

                    // Process each batch and send to Axiom
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
        batch_json: Vec<u8>,
        is_verification: bool,
        track_egress: bool,
    ) -> anyhow::Result<()> {
        let header_map = HeaderMap::from_iter([
            (
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", self.api_key))?,
            ),
            (CONTENT_TYPE, APPLICATION_JSON_CONTENT_TYPE),
        ]);
        let batch_json = Bytes::from(batch_json);

        // Make request in a loop that retries on transient errors
        let mut last_failure = None;
        for _ in 0..consts::AXIOM_SINK_MAX_REQUEST_ATTEMPTS {
            let batch_json = batch_json.clone();
            let response = self
                .fetch_client
                .fetch(HttpRequestStream {
                    url: self.axiom_url.clone(),
                    method: http::Method::POST,
                    headers: header_map.clone(),
                    body: Some(Box::pin(futures::stream::once(async { Ok(batch_json) }))),
                    signal: Box::pin(futures::future::pending()),
                })
                .await;

            if !is_verification
                && track_egress
                && let Ok(r) = &response
            {
                let num_bytes_egress = r.request_size.load(Ordering::Relaxed);
                utils::track_log_sink_bandwidth(
                    num_bytes_egress,
                    &self.egress_counter,
                    axiom_sink_network_egress_bytes,
                );
            }

            match classify_sink_response(response) {
                Ok(()) => return Ok(()),
                Err(failure) if failure.is_rejected() => anyhow::bail!(failure),
                Err(failure) => {
                    let delay = self.backoff.fail(&mut self.runtime.rng());
                    tracing::warn!(
                        "Failed to send in Axiom sink: {failure}. Waiting {delay:?} before \
                         retrying."
                    );
                    last_failure = Some(failure);
                    self.runtime.wait(delay).await;
                },
            }
        }

        anyhow::bail!(SinkEgressFailure::retries_exhausted(
            consts::AXIOM_SINK_MAX_REQUEST_ATTEMPTS,
            last_failure,
        ))
    }

    async fn process_events(
        &mut self,
        events: Vec<Arc<LogEvent>>,
        track_egress: bool,
    ) -> anyhow::Result<()> {
        let log_event_format_version = match self.log_event_format {
            LogEventFormatVersion::V1 => "1",
            LogEventFormatVersion::V2 => "2",
        };
        crate::metrics::axiom_sink_logs_received(events.len(), log_event_format_version);

        let mut values_to_send = vec![];
        let deployment_metadata = self.deployment_metadata.lock().clone();
        for event in &events {
            match AxiomLogEvent::new(
                event,
                &self.attributes,
                self.log_event_format,
                &deployment_metadata,
            ) {
                Err(e) => tracing::warn!("failed to convert log to JSON: {:?}", e),
                Ok(v) => values_to_send.push(v),
            }
        }

        if values_to_send.is_empty() {
            anyhow::bail!("skipping an entire batch due to logs that failed to be serialized");
        }
        let batch_size = values_to_send.len();

        self.send_batch(serde_json::to_vec(&values_to_send)?, false, track_egress)
            .await?;
        crate::metrics::axiom_sink_logs_sent(batch_size, log_event_format_version);

        Ok(())
    }
}
