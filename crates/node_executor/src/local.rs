use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{
            AtomicUsize,
            Ordering,
        },
        Arc,
        Mutex as StdMutex,
        Weak,
    },
    time::{
        Duration,
        Instant,
    },
};

use anyhow::Context;
use async_trait::async_trait;
use common::log_lines::LogLine;
use errors::ErrorMetadata;
use futures::{
    select_biased,
    FutureExt,
};
use futures_async_stream::try_stream;
use isolate::bundled_js::node_executor_file;
use rand::Rng;
use reqwest::Client;
use serde_json::Value as JsonValue;
use tempfile::TempDir;
use tokio::{
    process::{
        Child,
        Command as TokioCommand,
    },
    sync::{
        mpsc,
        Mutex,
    },
};

use crate::{
    executor::{
        ExecutorRequest,
        InvokeResponse,
        NodeExecutor,
        ARGS_TOO_LARGE_RESPONSE_MESSAGE,
        EXECUTE_TIMEOUT_RESPONSE_JSON,
    },
    handle_node_executor_stream,
    InvokeCompletion,
    NodeExecutorStreamPart,
};

const HEALTH_CHECK_INTERVAL: Duration = Duration::from_millis(100);
const MAX_HEALTH_CHECK_ATTEMPTS: u32 = 50;

pub struct LocalNodeExecutor {
    slot: Arc<Mutex<ExecutorSlot>>,
    flight: Arc<Flight>,
    config: LocalNodeExecutorConfig,
}

struct ExecutorSlot {
    process: Option<Arc<InnerLocalNodeExecutor>>,
    started_at: Option<Instant>,
    reaper_started: bool,
}

struct Flight {
    count: AtomicUsize,
    idle_since: StdMutex<Option<Instant>>,
}

struct InFlightGuard(Arc<Flight>);

impl InFlightGuard {
    fn enter(flight: &Arc<Flight>) -> Self {
        flight.count.fetch_add(1, Ordering::AcqRel);
        Self(flight.clone())
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if self.0.count.fetch_sub(1, Ordering::AcqRel) == 1
            && let Ok(mut idle_since) = self.0.idle_since.lock()
        {
            *idle_since = Some(Instant::now());
        }
    }
}

struct LocalNodeExecutorConfig {
    node_process_timeout: Duration,
    /// Overrides the initial callback retry backoff in the spawned node
    /// process (read by syscalls.ts at module load). Tests zero this so
    /// callbacks retrying against an unreachable backend settle within test
    /// timeouts.
    callback_initial_backoff: Option<Duration>,
}

struct InnerLocalNodeExecutor {
    _source_dir: TempDir,
    client: reqwest::Client,
    _server_handle: Child,
}

impl InnerLocalNodeExecutor {
    async fn new(config: &LocalNodeExecutorConfig) -> anyhow::Result<Self> {
        tracing::info!("Initializing inner local node executor");
        // Create a single temp directory for both source files and Node.js temp files
        let source_dir = TempDir::new()?;
        let (source, source_map) =
            node_executor_file("local.cjs").expect("local.cjs not generated!");
        let source_map = source_map.context("Missing local.cjs.map")?;
        let source_path = source_dir.path().join("local.cjs");
        let source_map_path = source_dir.path().join("local.cjs.map");
        fs::write(&source_path, source.as_bytes())?;
        fs::write(source_map_path, source_map.as_bytes())?;
        tracing::info!(
            "Using local node executor. Source: {}",
            source_path.to_str().expect("Path is not UTF-8 string?"),
        );

        let socket_path = if cfg!(unix) {
            source_dir.path().join(".executor.sock")
        } else if cfg!(windows) {
            PathBuf::from(format!(
                r"\\.\pipe\cvx-node-executor-{:016x}",
                rand::rng().random::<u64>()
            ))
        } else {
            panic!("not supported");
        };
        let server_handle =
            Self::start_node_with_listener(config, &source_path, &source_dir, &socket_path).await?;
        // Don't keep idle connections in the pool. The Node HTTP server closes
        // idle keep-alive connections after its (default 5s) `keepAliveTimeout`,
        // but hyper's pool would hold one much longer and reuse it right as the
        // server closes it, surfacing as a spurious "connection reset by peer".
        // Opening a fresh connection per request is cheap over a local socket.
        let mut client_builder = Client::builder().pool_max_idle_per_host(0);
        #[cfg(unix)]
        {
            client_builder = client_builder.unix_socket(socket_path);
        }
        #[cfg(windows)]
        {
            client_builder = client_builder.windows_named_pipe(socket_path);
        }
        let client = client_builder.build()?;

        // Wait for the Node process to be ready to handle HTTP requests.
        for _ in 0..MAX_HEALTH_CHECK_ATTEMPTS {
            if Self::check_server_health(&client).await? {
                return Ok(Self {
                    _source_dir: source_dir,
                    client,
                    _server_handle: server_handle,
                });
            }
            tokio::time::sleep(HEALTH_CHECK_INTERVAL).await;
        }
        anyhow::bail!("Node executor server failed to start and become healthy")
    }

    async fn check_node_version(node_path: &str) -> anyhow::Result<()> {
        let cmd = TokioCommand::new(node_path)
            .arg("--version")
            .output()
            .await?;
        let version = String::from_utf8_lossy(&cmd.stdout);

        if !version.starts_with("v22.")
            && !version.starts_with("v24.")
            && !version.starts_with("v26.")
        {
            anyhow::bail!(ErrorMetadata::bad_request(
                "DeploymentNotConfiguredForNodeActions",
                "Deployment is not configured to deploy \"use node\" actions. \
                 Node.js v22, 24, or 26 is not installed. \
                 Install a supported Node.js version with nvm (https://github.com/nvm-sh/nvm) \
                 to deploy Node.js actions."
            ))
        }
        Ok(())
    }

    async fn check_server_health(client: &Client) -> anyhow::Result<bool> {
        match client
            .get("http://localhost/health".to_string())
            .timeout(Duration::from_secs(1))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => Ok(true),
            _ => Ok(false),
        }
    }

    async fn start_node_with_listener(
        config: &LocalNodeExecutorConfig,
        source_path: &PathBuf,
        temp_dir: &TempDir,
        socket_path: &PathBuf,
    ) -> anyhow::Result<Child> {
        let node_path = "node";
        Self::check_node_version(node_path).await?;

        let mut cmd = TokioCommand::new(node_path);
        cmd.arg(source_path)
            .arg("--ipc-path")
            .arg(socket_path)
            .arg("--tempdir")
            .arg(temp_dir.path())
            .kill_on_drop(true);
        if let Some(backoff) = config.callback_initial_backoff {
            cmd.env(
                "CALLBACK_INITIAL_BACKOFF_MS",
                backoff.as_millis().to_string(),
            );
        }

        let child = cmd.spawn()?;

        Ok(child)
    }
}

impl LocalNodeExecutor {
    pub async fn new(node_process_timeout: Duration) -> anyhow::Result<Self> {
        let executor = Self {
            slot: Arc::new(Mutex::new(ExecutorSlot {
                process: None,
                started_at: None,
                reaper_started: false,
            })),
            flight: Arc::new(Flight {
                count: AtomicUsize::new(0),
                idle_since: StdMutex::new(None),
            }),
            config: LocalNodeExecutorConfig {
                node_process_timeout,
                callback_initial_backoff: None,
            },
        };

        Ok(executor)
    }

    #[try_stream(ok = NodeExecutorStreamPart, error = anyhow::Error)]
    async fn response_stream(config: &LocalNodeExecutorConfig, mut response: reqwest::Response) {
        let mut timeout_future = Box::pin(tokio::time::sleep(config.node_process_timeout));
        let timeout_future = &mut timeout_future;
        loop {
            let process_chunk = async {
                select_biased! {
                    chunk = response.chunk().fuse() => {
                        let chunk = chunk?;
                        match chunk {
                            Some(chunk) => {
                                anyhow::Ok(NodeExecutorStreamPart::Chunk(chunk))
                            }
                            None => {
                                anyhow::Ok(NodeExecutorStreamPart::InvokeComplete(
                                    InvokeCompletion::Success,
                                ))
                            }
                        }
                    },
                    _ = timeout_future.fuse() => {
                        anyhow::Ok(NodeExecutorStreamPart::InvokeComplete(
                            InvokeCompletion::ExplicitError(InvokeResponse {
                                response: EXECUTE_TIMEOUT_RESPONSE_JSON.clone(),
                                aws_request_id: None,
                            }),
                        ))
                    },
                }
            };
            let part = process_chunk.await?;
            if let NodeExecutorStreamPart::InvokeComplete(_) = part {
                yield part;
                break;
            } else {
                yield part;
            }
        }
    }
}

#[async_trait]
impl NodeExecutor for LocalNodeExecutor {
    fn enable(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn invoke(
        &self,
        request: ExecutorRequest,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
    ) -> anyhow::Result<InvokeResponse> {
        let process = {
            let mut slot = self.slot.lock().await;
            if slot.process.is_none() {
                let created = InnerLocalNodeExecutor::new(&self.config)
                    .await
                    .context("Failed to create inner local node executor")?;
                slot.process = Some(Arc::new(created));
                slot.started_at = Some(Instant::now());
                if let Ok(mut idle_since) = self.flight.idle_since.lock() {
                    *idle_since = None;
                }
                self.spawn_reaper(&mut slot);
            }
            slot.process.as_ref().unwrap().clone()
        };
        let _in_flight = InFlightGuard::enter(&self.flight);
        let result = self
            .invoke_with_client(&process, request, log_line_sender)
            .await;
        drop(process);
        result
    }

    fn shutdown(&self) {}
}

impl LocalNodeExecutor {
    fn spawn_reaper(&self, slot: &mut ExecutorSlot) {
        let Some(period) = reaper_period(
            *common::knobs::NODE_EXECUTOR_IDLE_TIMEOUT,
            *common::knobs::NODE_EXECUTOR_MAX_LIFETIME,
        ) else {
            return;
        };
        if slot.reaper_started {
            return;
        }
        slot.reaper_started = true;
        let weak = Arc::downgrade(&self.slot);
        let flight = self.flight.clone();
        tokio::spawn(async move {
            reaper_loop(weak, flight, period).await;
        });
    }

    async fn retire_if_current(&self, process: &Arc<InnerLocalNodeExecutor>) {
        let mut slot = self.slot.lock().await;
        if slot
            .process
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, process))
        {
            slot.process.take();
            slot.started_at = None;
        }
    }

    async fn invoke_with_client(
        &self,
        process: &Arc<InnerLocalNodeExecutor>,
        request: ExecutorRequest,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
    ) -> anyhow::Result<InvokeResponse> {
        let client = process.client.clone();
        let request_json = JsonValue::try_from(request)?;

        let response_result = client
            .post("http://localhost/invoke".to_string())
            .json(&request_json)
            .timeout(self.config.node_process_timeout)
            .send()
            .await;
        let response = match response_result {
            Ok(response) => response,
            Err(e) => {
                if e.is_timeout() {
                    return Ok(InvokeResponse {
                        response: EXECUTE_TIMEOUT_RESPONSE_JSON.clone(),
                        aws_request_id: None,
                    });
                } else if e.is_connect() {
                    // Connection error likely means the Node server crashed (e.g., OOM).
                    // Drop the dead server so it will be restarted on next invoke.
                    tracing::warn!("Node server connection failed, dropping server: {e}");
                    self.retire_if_current(process).await;
                    return Err(anyhow::anyhow!(e).context("Node server request failed"));
                } else {
                    return Err(anyhow::anyhow!(e).context("Node server request failed"));
                }
            },
        };

        if let Err(e) = response.error_for_status_ref() {
            if e.status() == Some(reqwest::StatusCode::PAYLOAD_TOO_LARGE) {
                return Err(
                    anyhow::anyhow!(e.without_url()).context(ErrorMetadata::bad_request(
                        "ArgsTooLarge",
                        ARGS_TOO_LARGE_RESPONSE_MESSAGE,
                    )),
                );
            }
            let error = response.text().await?;
            anyhow::bail!("Node executor server returned error: {}", error);
        }
        let stream = Self::response_stream(&self.config, response);
        let stream = Box::pin(stream);
        let result = handle_node_executor_stream(log_line_sender, stream).await?;
        match result {
            Ok(payload) => {
                if payload
                    .get("exitingProcess")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    // Drop the server if it claims to be exiting.
                    self.retire_if_current(process).await;
                }
                Ok(InvokeResponse {
                    response: payload,
                    aws_request_id: None,
                })
            },
            Err(e) => Ok(e),
        }
    }
}

fn reaper_period(idle_timeout: Duration, max_lifetime: Duration) -> Option<Duration> {
    let shortest = match (idle_timeout.is_zero(), max_lifetime.is_zero()) {
        (true, true) => return None,
        (false, true) => idle_timeout,
        (true, false) => max_lifetime,
        (false, false) => idle_timeout.min(max_lifetime),
    };
    let quarter = Duration::from_secs_f64(shortest.as_secs_f64() / 4.0);
    Some(quarter.clamp(Duration::from_secs(1), Duration::from_secs(30)))
}

fn should_retire(
    idle_timeout: Duration,
    max_lifetime: Duration,
    in_flight: usize,
    idle_for: Duration,
    alive_for: Duration,
) -> bool {
    if in_flight > 0 {
        return false;
    }
    let idle_hit = !idle_timeout.is_zero() && idle_for >= idle_timeout;
    let lifetime_hit = !max_lifetime.is_zero() && alive_for >= max_lifetime;
    idle_hit || lifetime_hit
}

async fn reaper_loop(weak: Weak<Mutex<ExecutorSlot>>, flight: Arc<Flight>, period: Duration) {
    let idle_timeout = *common::knobs::NODE_EXECUTOR_IDLE_TIMEOUT;
    let max_lifetime = *common::knobs::NODE_EXECUTOR_MAX_LIFETIME;
    loop {
        tokio::time::sleep(period).await;
        let Some(slot) = weak.upgrade() else {
            return;
        };
        let mut slot = slot.lock().await;
        let Some(started_at) = slot.started_at else {
            continue;
        };
        if slot.process.is_none() {
            continue;
        }
        let idle_since = flight.idle_since.lock().ok().and_then(|idle| *idle);
        let Some(idle_since) = idle_since else {
            continue;
        };
        let now = Instant::now();
        let idle_for = now.saturating_duration_since(idle_since);
        let alive_for = now.saturating_duration_since(started_at);
        if should_retire(
            idle_timeout,
            max_lifetime,
            flight.count.load(Ordering::Acquire),
            idle_for,
            alive_for,
        ) {
            tracing::info!(
                "Retiring local node executor (idle {:.2}s, alive {:.2}s)",
                idle_for.as_secs_f64(),
                alive_for.as_secs_f64(),
            );
            slot.process.take();
            slot.started_at = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(value: u64) -> Duration {
        Duration::from_secs(value)
    }

    #[test]
    fn reaper_wakes_a_quarter_of_the_shortest_deadline() {
        assert_eq!(reaper_period(secs(0), secs(0)), None);
        assert_eq!(reaper_period(secs(40), secs(0)), Some(secs(10)));
        assert_eq!(reaper_period(secs(40), secs(400)), Some(secs(10)));
        assert_eq!(reaper_period(secs(0), secs(80)), Some(secs(20)));
        assert_eq!(reaper_period(secs(8), secs(0)), Some(secs(2)));
        assert_eq!(reaper_period(secs(300), secs(0)), Some(secs(30)));
        assert_eq!(reaper_period(secs(3600), secs(86400)), Some(secs(30)));
        assert_eq!(reaper_period(secs(2), secs(0)), Some(secs(1)));
    }

    #[test]
    fn retire_decision_matches_measured_vectors() {
        let cases = [
            (300, 0, 0, 299, 299, false),
            (300, 0, 0, 300, 300, true),
            (1, 1, 1, 3600, 3600, false),
            (1, 1, 0, 3600, 3600, true),
            (300, 3600, 0, 5, 3599, false),
            (300, 3600, 0, 5, 3600, true),
            (0, 3600, 0, 86400, 60, false),
            (0, 0, 0, 86400, 86400, false),
        ];
        for (idle, life, in_flight, idle_for, alive_for, retire) in cases {
            assert_eq!(
                should_retire(
                    secs(idle),
                    secs(life),
                    in_flight,
                    secs(idle_for),
                    secs(alive_for),
                ),
                retire,
                "idle {idle} life {life} in_flight {in_flight} idle_for {idle_for} alive \
                 {alive_for}",
            );
        }
    }

    #[test]
    fn three_hundred_second_idle_retires_on_the_330_second_wake() {
        let tick = reaper_period(secs(300), secs(0)).unwrap();
        let finished_at = secs(1);
        let mut wake = tick;
        let mut retired_at = None;
        while wake < secs(1000) {
            let idle_for = wake.saturating_sub(finished_at);
            if should_retire(secs(300), secs(0), 0, idle_for, wake) {
                retired_at = Some((wake, idle_for));
                break;
            }
            wake += tick;
        }
        assert_eq!(retired_at, Some((secs(330), secs(329))));
    }
}
