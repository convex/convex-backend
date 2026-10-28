use std::{
    borrow::Cow,
    sync::Arc,
};

use common::{
    backoff::Backoff,
    errors::report_error,
    log_streaming::{
        LogEvent,
        StructuredLogEvent,
    },
    runtime::{
        block_in_place,
        Runtime,
    },
};
use maplit::btreemap;
use model::log_sinks::types::sentry::{
    ExceptionFormatVersion,
    SentryConfig,
};
use parking_lot::Mutex;
use sentry::{
    protocol::{
        ClientSdkInfo,
        Event,
        Exception,
        Frame,
        Stacktrace,
    },
    ClientOptions,
    Envelope,
    Level,
    TransportFactory,
    User,
};
use serde_json::Value as JsonValue;
use tokio::sync::mpsc;

use crate::{
    consts,
    sinks::utils::{
        build_event_batches,
        SinkFilter,
    },
    LogSinkClient,
    LoggingDeploymentMetadata,
};

pub(crate) struct SentrySink {
    sentry_client: sentry::Client,
    events_receiver: mpsc::Receiver<Vec<Arc<LogEvent>>>,
    backoff: Backoff,
    deployment_metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
    config: SentryConfig,
}

impl SentrySink {
    pub async fn start<RT: Runtime>(
        runtime: RT,
        config: SentryConfig,
        transport: Arc<dyn TransportFactory>,
        deployment_metadata: Arc<Mutex<LoggingDeploymentMetadata>>,
        should_verify: bool,
    ) -> anyhow::Result<LogSinkClient> {
        tracing::info!("Starting SentrySink");
        let (tx, rx) = mpsc::channel(consts::SENTRY_SINK_EVENTS_BUFFER_SIZE);

        let sentry_client = sentry::Client::with_options(ClientOptions {
            dsn: Some(config.dsn.clone().into_value()),
            transport: Some(transport),
            ..Default::default()
        });
        anyhow::ensure!(sentry_client.is_enabled());
        let mut sink = Self {
            sentry_client,
            events_receiver: rx,
            backoff: Backoff::new(
                consts::SENTRY_SINK_INITIAL_BACKOFF,
                consts::SENTRY_SINK_MAX_BACKOFF,
            ),
            deployment_metadata,
            config,
        };

        if should_verify {
            sink.verify_creds().await?;
            tracing::info!("SentrySink verified!");
        }

        let handle = Arc::new(Mutex::new(runtime.spawn("sentry_sink", sink.go())));
        let client = LogSinkClient {
            _handle: handle,
            events_sender: tx,
        };
        Ok(client)
    }

    async fn verify_creds(&mut self) -> anyhow::Result<()> {
        let envelope = Envelope::new();
        self.sentry_client.send_envelope(envelope);
        block_in_place(|| self.sentry_client.flush(None));
        Ok(())
    }

    async fn go(mut self) {
        loop {
            match self.events_receiver.recv().await {
                None => {
                    // The sender was closed, event loop should shutdown
                    tracing::warn!("Stopping SentrySink. Sender was closed.");
                    return;
                },
                Some(ev) => {
                    // Split events into batches
                    let batches = build_event_batches(
                        ev,
                        consts::SENTRY_SINK_MAX_LOGS_PER_BATCH,
                        &SinkFilter::OnlyExceptions,
                    );

                    // Process each batch and send to Sentry
                    for batch in batches {
                        if let Err(mut e) = self.process_events(batch).await {
                            tracing::error!("Error emitting log event batch in SentrySink.");
                            report_error(&mut e).await;
                        } else {
                            self.backoff.reset();
                        }
                    }
                },
            }
        }
    }

    async fn process_events(&mut self, events: Vec<Arc<LogEvent>>) -> anyhow::Result<()> {
        crate::metrics::sentry_sink_logs_received(events.len());

        let num_exceptions = events.len();
        if num_exceptions == 0 {
            // No exceptions in this batch
            return Ok(());
        }

        for event in events {
            let StructuredLogEvent::Exception {
                error,
                user_identifier,
                source,
                udf_server_version,
                request_metadata,
            } = &event.event
            else {
                continue;
            };

            let exception = match self.config.version {
                ExceptionFormatVersion::V1 => {
                    // The legacy format used `type` to capture the error message
                    // and `value` to capture the stacktrace as a string. The newer
                    // format uses `value` to capture the error message and `stacktrace`
                    // to capture the stacktrace as an array of frames.
                    let ty = error.message.clone();
                    let stacktrace: Option<Vec<String>> = error
                        .frames
                        .as_ref()
                        .map(|frames| frames.0.iter().map(|frame| frame.to_string()).collect());
                    Exception {
                        ty: ty.to_string(),
                        value: stacktrace.map(|st| st.join("\n")),
                        ..Default::default()
                    }
                },
                ExceptionFormatVersion::V2 => {
                    let frames_for_sentry: Option<Vec<Frame>> =
                        error.frames.as_ref().map(|frames| {
                            frames
                                .0
                                .iter()
                                .rev()
                                .map(|frame| sentry::protocol::Frame::from(frame.clone()))
                                .collect()
                        });
                    let stacktrace_for_sentry = frames_for_sentry.map(|st| Stacktrace {
                        frames: st,
                        ..Default::default()
                    });
                    Exception {
                        ty: "Error".to_string(),
                        value: Some(error.message.clone()),
                        stacktrace: stacktrace_for_sentry,
                        ..Default::default()
                    }
                },
            };

            let mut tags = if let Some(ref tags) = self.config.tags {
                tags.iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect()
            } else {
                btreemap! {}
            };

            tags.insert("func".to_string(), source.udf_path.to_string());
            tags.insert(
                "func_type".to_string(),
                source.udf_type.to_lowercase_string().to_string(),
            );
            tags.insert(
                "func_runtime".to_string(),
                source.module_environment.as_sentry_tag().to_string(),
            );
            tags.insert(
                "request_id".to_string(),
                source.context.request_id.to_string(),
            );
            if let Some(cached) = source.cached {
                tags.insert("cached".to_string(), cached.to_string());
            };
            if let Some(path) = source.component_path.clone().serialize() {
                tags.insert("func_component".to_string(), path);
            }
            let deployment_metadata = self.deployment_metadata.lock();

            // The datadog error-tracking via the sentry SDK chokes on the sdk field
            // so we don't include it
            let sdk = if self.config.dsn.host().contains("sentry-intake.datadoghq") {
                None
            } else {
                Some(Cow::Owned(ClientSdkInfo {
                    name: "convex".to_string(),
                    version: udf_server_version
                        .clone()
                        .map(|v| v.to_string())
                        .unwrap_or("unknown".to_string()),
                    integrations: vec![],
                    packages: vec![],
                }))
            };
            // Add ConvexError data to the exception as a context
            let contexts = error.custom_data.clone().map_or(btreemap! {}, |data| {
                btreemap! {
                    "ConvexError".to_string() => sentry::protocol::Context::Other(btreemap! {
                        "data".to_string() => JsonValue::from(data),
                    }),
                }
            });

            let sentry_event = Event {
                exception: vec![exception].into(),
                level: Level::Error,
                timestamp: event.timestamp.as_system_time(),
                sdk,
                platform: "node".into(),
                user: Some(User {
                    id: user_identifier.clone().map(|i| i.to_string()),
                    ip_address: request_metadata
                        .ip
                        .clone()
                        .and_then(|ip| ip.as_str().parse().ok()),
                    ..Default::default()
                }),
                server_name: Some(deployment_metadata.deployment_name.clone().into()),
                environment: deployment_metadata
                    .deployment_type
                    .map(|dt| dt.as_sentry_tag().into()),
                tags,
                contexts,
                ..Default::default()
            };
            anyhow::ensure!(self.sentry_client.is_enabled());
            self.sentry_client.capture_event(sentry_event, None);
        }

        crate::metrics::sentry_sink_logs_sent(num_exceptions);
        Ok(())
    }
}
