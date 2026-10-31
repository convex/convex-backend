use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        Mutex as StdMutex,
    },
    time::Duration,
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
        Mutex as AsyncMutex,
        Notify,
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
    inner: AsyncMutex<Option<InnerLocalNodeExecutor>>,
    lifecycle: Arc<ExecutorLifecycle>,
    config: LocalNodeExecutorConfig,
}

struct ExecutorLifecycle {
    state: StdMutex<ExecutorLifecycleState>,
    state_changed: Notify,
}

struct ExecutorLifecycleState {
    active_invocations: usize,
    startup_in_progress: bool,
    shutdown: ExecutorShutdownState,
}

enum ExecutorShutdownState {
    Running,
    ShuttingDown { shutdown_claimed: bool },
    ShutDown,
}

struct InvocationLease {
    lifecycle: Arc<ExecutorLifecycle>,
}

struct StartupLease {
    lifecycle: Arc<ExecutorLifecycle>,
}

struct ShutdownLease {
    lifecycle: Arc<ExecutorLifecycle>,
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
    source_dir: TempDir,
    client: reqwest::Client,
    server_handle: Child,
}

impl ExecutorLifecycle {
    fn new() -> Self {
        Self {
            state: StdMutex::new(ExecutorLifecycleState {
                active_invocations: 0,
                startup_in_progress: false,
                shutdown: ExecutorShutdownState::Running,
            }),
            state_changed: Notify::new(),
        }
    }

    fn begin_invocation(self: &Arc<Self>) -> anyhow::Result<InvocationLease> {
        let mut state = self.state.lock().expect("Executor lifecycle lock poisoned");
        if !matches!(state.shutdown, ExecutorShutdownState::Running) {
            anyhow::bail!("Node executor is shutting down");
        }
        state.active_invocations += 1;
        Ok(InvocationLease {
            lifecycle: self.clone(),
        })
    }

    async fn begin_startup(self: &Arc<Self>) -> anyhow::Result<StartupLease> {
        loop {
            let state_changed = self.state_changed.notified();
            {
                let mut state = self.state.lock().expect("Executor lifecycle lock poisoned");
                if !matches!(state.shutdown, ExecutorShutdownState::Running) {
                    anyhow::bail!("Node executor is shutting down");
                }
                if !state.startup_in_progress {
                    state.startup_in_progress = true;
                    return Ok(StartupLease {
                        lifecycle: self.clone(),
                    });
                }
            }
            state_changed.await;
        }
    }

    async fn begin_shutdown(self: &Arc<Self>) -> Option<ShutdownLease> {
        loop {
            let state_changed = self.state_changed.notified();
            {
                let mut state = self.state.lock().expect("Executor lifecycle lock poisoned");
                let active_invocations = state.active_invocations;
                match &mut state.shutdown {
                    ExecutorShutdownState::Running => {
                        state.shutdown = ExecutorShutdownState::ShuttingDown {
                            shutdown_claimed: false,
                        };
                    },
                    ExecutorShutdownState::ShutDown => return None,
                    ExecutorShutdownState::ShuttingDown { .. } => {},
                }
                if let ExecutorShutdownState::ShuttingDown { shutdown_claimed } =
                    &mut state.shutdown
                    && active_invocations == 0
                    && !*shutdown_claimed
                {
                    *shutdown_claimed = true;
                    return Some(ShutdownLease {
                        lifecycle: self.clone(),
                    });
                }
            }
            state_changed.await;
        }
    }

    fn finish_invocation(&self) {
        let mut state = self.state.lock().expect("Executor lifecycle lock poisoned");
        state.active_invocations -= 1;
        if state.active_invocations == 0 {
            self.state_changed.notify_waiters();
        }
    }

    fn finish_startup(&self) {
        let mut state = self.state.lock().expect("Executor lifecycle lock poisoned");
        state.startup_in_progress = false;
        self.state_changed.notify_waiters();
    }

    fn finish_shutdown(&self) {
        let mut state = self.state.lock().expect("Executor lifecycle lock poisoned");
        state.shutdown = ExecutorShutdownState::ShutDown;
        self.state_changed.notify_waiters();
    }
}

impl Drop for InvocationLease {
    fn drop(&mut self) {
        self.lifecycle.finish_invocation();
    }
}

impl Drop for StartupLease {
    fn drop(&mut self) {
        self.lifecycle.finish_startup();
    }
}

impl Drop for ShutdownLease {
    fn drop(&mut self) {
        self.lifecycle.finish_shutdown();
    }
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
        // Don't keep idle connections in the pool. The Node HTTP server closes
        // idle keep-alive connections after its (default 5s) `keepAliveTimeout`,
        // but hyper's pool would hold one much longer and reuse it right as the
        // server closes it, surfacing as a spurious "connection reset by peer".
        // Opening a fresh connection per request is cheap over a local socket.
        let mut client_builder = Client::builder().pool_max_idle_per_host(0);
        #[cfg(unix)]
        {
            client_builder = client_builder.unix_socket(socket_path.clone());
        }
        #[cfg(windows)]
        {
            client_builder = client_builder.windows_named_pipe(socket_path.clone());
        }
        let client = client_builder.build()?;
        let server_handle =
            Self::start_node_with_listener(config, &source_path, &source_dir, &socket_path).await?;

        let executor = Self {
            source_dir,
            client,
            server_handle,
        };
        let health_error = 'health: {
            // Wait for the Node process to be ready to handle HTTP requests.
            for _ in 0..MAX_HEALTH_CHECK_ATTEMPTS {
                match Self::check_server_health(&executor.client).await {
                    Ok(true) => return Ok(executor),
                    Ok(false) => {},
                    Err(error) => break 'health error,
                }
                tokio::time::sleep(HEALTH_CHECK_INTERVAL).await;
            }
            anyhow::anyhow!("Node executor server failed to start and become healthy")
        };
        if let Err(cleanup_error) = executor.shutdown().await {
            tracing::warn!("Failed to stop unhealthy Node executor server: {cleanup_error:#}");
        }
        Err(health_error)
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
            .arg("--parent-pid")
            .arg(std::process::id().to_string())
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

    async fn shutdown(mut self) -> anyhow::Result<()> {
        match self.server_handle.start_kill() {
            Ok(()) => {},
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {},
            Err(error) => return Err(error).context("Failed to terminate Node executor server"),
        }
        self.server_handle
            .wait()
            .await
            .context("Failed to wait for Node executor server to exit")?;
        drop(self.source_dir);
        Ok(())
    }
}

impl LocalNodeExecutor {
    pub async fn new(node_process_timeout: Duration) -> anyhow::Result<Self> {
        let executor = Self {
            inner: AsyncMutex::new(None),
            lifecycle: Arc::new(ExecutorLifecycle::new()),
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

    async fn client(&self) -> anyhow::Result<Client> {
        loop {
            if let Some(client) = self
                .inner
                .lock()
                .await
                .as_ref()
                .map(|inner| inner.client.clone())
            {
                return Ok(client);
            }

            let _startup = self.lifecycle.begin_startup().await?;
            if let Some(client) = self
                .inner
                .lock()
                .await
                .as_ref()
                .map(|inner| inner.client.clone())
            {
                return Ok(client);
            }

            let inner = InnerLocalNodeExecutor::new(&self.config)
                .await
                .context("Failed to create inner local node executor")?;
            let client = inner.client.clone();
            *self.inner.lock().await = Some(inner);
            return Ok(client);
        }
    }

    async fn discard_inner(&self) {
        let inner = self.inner.lock().await.take();
        if let Some(inner) = inner
            && let Err(cleanup_error) = inner.shutdown().await
        {
            tracing::warn!("Failed to stop Node executor server: {cleanup_error:#}");
        }
    }

    async fn invoke_with_client(
        &self,
        client: reqwest::Client,
        request: ExecutorRequest,
        log_line_sender: mpsc::UnboundedSender<LogLine>,
    ) -> anyhow::Result<InvokeResponse> {
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
                    self.discard_inner().await;
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
                    self.discard_inner().await;
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
        let _invocation = self.lifecycle.begin_invocation()?;
        let client = self.client().await?;
        self.invoke_with_client(client, request, log_line_sender)
            .await
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        let Some(_shutdown) = self.lifecycle.begin_shutdown().await else {
            return Ok(());
        };
        let inner = self.inner.lock().await.take();
        if let Some(inner) = inner {
            inner.shutdown().await?;
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_stops_server_and_cleans_socket() -> anyhow::Result<()> {
        let executor = InnerLocalNodeExecutor::new(&LocalNodeExecutorConfig {
            node_process_timeout: Duration::from_secs(1),
            callback_initial_backoff: None,
        })
        .await?;
        let socket_path = executor.source_dir.path().join(".executor.sock");
        assert!(socket_path.exists());

        executor.shutdown().await?;

        assert!(!socket_path.exists());
        Ok(())
    }

    async fn wait_for_shutdown_to_start(lifecycle: &ExecutorLifecycle) {
        for _ in 0..100 {
            if !matches!(
                lifecycle
                    .state
                    .lock()
                    .expect("Executor lifecycle lock poisoned")
                    .shutdown,
                ExecutorShutdownState::Running
            ) {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("shutdown did not start");
    }

    #[tokio::test]
    async fn shutdown_rejects_invocation_that_could_start_replacement() {
        let lifecycle = Arc::new(ExecutorLifecycle::new());
        let invocation = lifecycle.begin_invocation().unwrap();
        let shutdown_lifecycle = lifecycle.clone();
        let shutdown = tokio::spawn(async move {
            let shutdown = shutdown_lifecycle.begin_shutdown().await.unwrap();
            drop(shutdown);
        });

        wait_for_shutdown_to_start(&lifecycle).await;
        assert!(lifecycle.begin_invocation().is_err());

        drop(invocation);
        shutdown.await.unwrap();
        assert!(lifecycle.begin_invocation().is_err());
    }

    #[tokio::test]
    async fn shutdown_waits_for_active_invocation() {
        let lifecycle = Arc::new(ExecutorLifecycle::new());
        let invocation = lifecycle.begin_invocation().unwrap();
        let shutdown_lifecycle = lifecycle.clone();
        let shutdown = tokio::spawn(async move {
            let shutdown = shutdown_lifecycle.begin_shutdown().await.unwrap();
            drop(shutdown);
        });

        wait_for_shutdown_to_start(&lifecycle).await;
        assert!(!shutdown.is_finished());

        drop(invocation);
        shutdown.await.unwrap();
    }
}
