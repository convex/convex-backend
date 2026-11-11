use async_trait::async_trait;
use common::log_lines::LogLine;
use tokio::sync::mpsc;

use crate::executor::{
    ExecutorRequest,
    InvokeResponse,
    NodeExecutor,
};
pub struct NoopNodeExecutor {}

impl NoopNodeExecutor {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl NodeExecutor for NoopNodeExecutor {
    fn enable(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn invoke(
        &self,
        _request: ExecutorRequest,
        _log_line_sender: mpsc::UnboundedSender<LogLine>,
    ) -> anyhow::Result<InvokeResponse> {
        anyhow::bail!("NoopNodeExecutor cannot be used to invoke code.");
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
