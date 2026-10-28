use async_trait::async_trait;

pub use self::{
    aws::AwsFirehose,
    local::LocalFileFirehose,
};

mod aws;
mod local;

/// A single delivery attempt. Callers own batching, retries, and accounting.
#[async_trait]
pub trait Firehose: Send + Sync {
    async fn send_batch(&self, records: Vec<String>) -> anyhow::Result<BatchResult>;
}

#[derive(Debug, Default)]
pub struct BatchResult {
    /// Indices refer to the input batch; records absent here were accepted.
    pub failures: Vec<RecordFailure>,
}

#[derive(Debug)]
pub struct RecordFailure {
    pub index: usize,
    pub code: String,
    pub message: String,
}
