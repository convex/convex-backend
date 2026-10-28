use anyhow::Context;
use async_trait::async_trait;
use aws_sdk_firehose::{
    primitives::Blob,
    types::Record,
};

use super::{
    BatchResult,
    Firehose,
    RecordFailure,
};
use crate::must_config_from_env;

pub struct AwsFirehose {
    client: aws_sdk_firehose::Client,
    stream_name: String,
}

impl AwsFirehose {
    pub async fn new(stream_name: String) -> anyhow::Result<Self> {
        Ok(Self {
            client: firehose_client().await?,
            stream_name,
        })
    }
}

#[async_trait]
impl Firehose for AwsFirehose {
    async fn send_batch(&self, records: Vec<String>) -> anyhow::Result<BatchResult> {
        if records.is_empty() {
            return Ok(BatchResult::default());
        }
        let records = records
            .into_iter()
            .map(|record| {
                Record::builder()
                    .data(Blob::new(record.into_bytes()))
                    .build()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let result = self
            .client
            .put_record_batch()
            .delivery_stream_name(&self.stream_name)
            .set_records(Some(records))
            .send()
            .await?;
        Ok(batch_result(result.request_responses()))
    }
}

fn batch_result(responses: &[aws_sdk_firehose::types::PutRecordBatchResponseEntry]) -> BatchResult {
    BatchResult {
        failures: responses
            .iter()
            .enumerate()
            .filter_map(|(index, response)| {
                response.error_code().map(|code| RecordFailure {
                    index,
                    code: code.to_owned(),
                    message: response.error_message().unwrap_or_default().to_owned(),
                })
            })
            .collect(),
    }
}

/// Singleton firehose client to share the connection pool / TLS connector
/// across instances.
static FIREHOSE_CLIENT: tokio::sync::OnceCell<aws_sdk_firehose::Client> =
    tokio::sync::OnceCell::const_new();

async fn firehose_client() -> anyhow::Result<aws_sdk_firehose::Client> {
    FIREHOSE_CLIENT
        .get_or_try_init(|| async {
            let config = must_config_from_env()
                .await
                .context("AWS env variables are required when using AWS Firehose")?
                .load()
                .await;
            let client = aws_sdk_firehose::Client::new(&config);
            anyhow::Ok(client)
        })
        .await
        .cloned()
}
