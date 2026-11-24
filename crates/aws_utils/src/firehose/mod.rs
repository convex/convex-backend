use async_trait::async_trait;

pub use self::{
    aws::AwsFirehose,
    local::LocalFileFirehose,
};

mod aws;
mod local;

/// AWS's per-record limit for `PutRecordBatch`
pub const MAX_RECORD_BYTES: usize = 1_024_000;
/// AWS's per-request record count limit for `PutRecordBatch`
pub const MAX_BATCH_RECORDS: usize = 500;
/// AWS's per-request size limit for `PutRecordBatch`
pub const MAX_BATCH_BYTES: usize = 4_000_000;

/// Indicates a permanent error with the record batch in `send_batch`.
#[derive(Debug)]
pub struct BatchRejected;

impl std::fmt::Display for BatchRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Firehose rejected the batch")
    }
}

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
