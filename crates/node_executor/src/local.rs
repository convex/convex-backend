use std::{
    fs,
    path::PathBuf,
    sync::Arc,
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
    inner: Arc<Mutex<Option<InnerLocalNodeExecutor>>>,
    config: LocalNodeExecutorConfig,
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

        if !version.starts_with("v22.") && !version.starts_with("v24.") {
            anyhow::bail!(ErrorMetadata::bad_request(
                "DeploymentNotConfiguredForNodeActions",
                "Deployment is not configured to deploy \"use node\" actions. \
                 Node.js v22 or 24 is not installed. \
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
            inner: Arc::new(Mutex::new(None)),
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
        let client = {
            let mut inner = self.inner.lock().await;
            if inner.is_none() {
                *inner = Some(
                    InnerLocalNodeExecutor::new(&self.config)
                        .await
                        .context("Failed to create inner local node executor")?,
                )
            }
            let inner = inner.as_ref().unwrap();
            inner.client.clone()
        };
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
                    self.inner.lock().await.take();
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
                    self.inner.lock().await.take();
                }
                Ok(InvokeResponse {
                    response: payload,
                    aws_request_id: None,
                })
            },
            Err(e) => Ok(e),
        }
    }

    fn shutdown(&self) {}
}
