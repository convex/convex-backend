use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
};

use common::{
    backoff::Backoff,
    errors::report_error,
    log_streaming::{
        LogEvent,
        LogEventFormatVersion,
    },
    runtime::Runtime,
};
use parking_lot::Mutex;
use serde_json::Value as JsonValue;
use tokio::sync::mpsc;

use crate::{
    consts,
    IntegrationClient,
};

pub const LOG_EVENT_FORMAT_FOR_LOCAL_INTEGRATION: LogEventFormatVersion = LogEventFormatVersion::V2;

pub struct LocalIntegration<RT: Runtime> {
    runtime: RT,
    events_receiver: mpsc::Receiver<Vec<Arc<LogEvent>>>,
    config: LocalIntegrationConfig,
}

#[derive(Clone, Debug)]
pub struct LocalIntegrationConfig {
    path: PathBuf,
}

impl FromStr for LocalIntegrationConfig {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self { path: s.parse()? })
    }
}

impl<RT: Runtime> LocalIntegration<RT> {
    pub async fn start(
        runtime: RT,
        config: LocalIntegrationConfig,
    ) -> anyhow::Result<IntegrationClient> {
        let (tx, rx) = mpsc::channel(consts::LOCAL_INTEGRATION_EVENTS_BUFFER_SIZE);

        let integration = Self {
            runtime: runtime.clone(),
            events_receiver: rx,
            config: config.clone(),
        };

        let handle = Arc::new(Mutex::new(
            runtime.spawn("local_integration", integration.go()),
        ));

        let client = IntegrationClient {
            _handle: handle,
            events_sender: tx,
        };
        tracing::info!("Started LocalIntegration at {:?}", config.path);
        Ok(client)
    }

    async fn go(mut self) {
        let mut backoff = Backoff::new(
            consts::LOCAL_INTEGRATION_INITIAL_BACKOFF,
            consts::LOCAL_INTEGRATION_MAX_BACKOFF,
        );

        loop {
            match self.events_receiver.recv().await {
                None => {
                    // The sender was closed, event loop should shutdown
                    tracing::warn!("Stopping LocalIntegration. Sender was closed.");
                    return;
                },
                Some(events) => {
                    while let Err(mut e) = self.process_events(events.clone()).await {
                        let delay = backoff.fail(&mut self.runtime.rng());
                        tracing::error!(
                            "Error emitting event in LocalIntegration: {e:?}. Waiting {delay:?}ms \
                             before retrying"
                        );
                        report_error(&mut e).await;
                        self.runtime.wait(delay).await;
                    }
                    backoff.reset();
                },
            }
        }
    }

    async fn process_events(&mut self, events: Vec<Arc<LogEvent>>) -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(self.config.path.clone())?;
        let num_events = events.len();
        for event in events {
            let fields: serde_json::Map<String, JsonValue> =
                event.to_json_map(LOG_EVENT_FORMAT_FOR_LOCAL_INTEGRATION)?;
            let mut event = serde_json::to_vec(&fields)?;
            event.extend_from_slice("\n".as_bytes());
            file.write_all(&event)?;
        }
        file.sync_all()?;
        tracing::debug!(
            "Wrote {} events to file: {:?}",
            num_events,
            self.config.path.clone()
        );
        Ok(())
    }
}
