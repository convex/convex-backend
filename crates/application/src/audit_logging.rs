use std::sync::Arc;

use aws_utils::firehose::{
    AwsFirehose,
    Firehose,
    LocalFileFirehose,
};
use common::{
    audit_log_lines::ResolvedAuditLogLines,
    knobs::AUDIT_LOG_FIREHOSE_FILE,
    log_streaming::{
        LogEvent,
        LogSender,
        StructuredLogEvent,
    },
};
use errors::ErrorMetadata;
use log_streaming::LogManagerClient;
use model::audit_log_config::validate_audit_log_firehose_stream_name;
use usage_tracking::FunctionUsageTracker;

const FIREHOSE_ROUND_INCREMENTS_BYTES: u64 = 5000;

/// AuditLogClient implementation that forwards audit logs to log streams.
#[derive(Clone)]
pub struct AuditLogClient {
    log_stream_client: LogManagerClient,
    firehose_client: Option<Arc<dyn Firehose>>,
}

impl AuditLogClient {
    pub async fn new(
        log_stream_client: LogManagerClient,
        firehose_stream_name: Option<String>,
        deployment_name: &String,
    ) -> anyhow::Result<Self> {
        let firehose_client: Option<Arc<dyn Firehose>> =
            if let Some(path) = &*AUDIT_LOG_FIREHOSE_FILE {
                tracing::info!("Writing audit logs to {}", path.display());
                Some(Arc::new(LocalFileFirehose::new(path.clone())))
            } else if let Some(firehose_name) = firehose_stream_name {
                validate_audit_log_firehose_stream_name(&firehose_name, deployment_name)?;
                Some(Arc::new(AwsFirehose::new(firehose_name).await?))
            } else {
                None
            };
        Ok(Self {
            log_stream_client,
            firehose_client,
        })
    }

    fn send_to_log_streams(
        &self,
        ResolvedAuditLogLines { logs, timestamp }: ResolvedAuditLogLines,
    ) {
        let events = logs
            .into_iter()
            .map(|b| LogEvent {
                timestamp,
                event: StructuredLogEvent::CustomAudit {
                    body: b.into_value(),
                },
            })
            .collect();
        self.log_stream_client.send_logs(events);
    }

    #[fastrace::trace]
    pub async fn send_logs(
        &self,
        logs: ResolvedAuditLogLines,
        usage_tracker: &FunctionUsageTracker,
    ) -> anyhow::Result<()> {
        let Some(firehose_client) = &self.firehose_client else {
            self.send_to_log_streams(logs);
            return Ok(());
        };

        let records = logs.to_json_strings()?;
        let egress = calculate_audit_log_egress(&records);
        let result = firehose_client.send_batch(records).await?;
        if !result.failures.is_empty() {
            for failure in result.failures.iter().take(5) {
                tracing::error!(
                    "Firehose error while delivering audit logs: {}: {}",
                    failure.code,
                    failure.message,
                );
            }
            anyhow::bail!(ErrorMetadata::bad_request(
                "AuditLogFailed",
                "Failed to deliver audit logs"
            ));
        }
        usage_tracker.track_audit_log_egress(egress);

        Ok(())
    }
}

fn calculate_audit_log_egress(records: &Vec<String>) -> u64 {
    records
        .iter()
        .map(|record| {
            (record.len() as u64).div_ceil(FIREHOSE_ROUND_INCREMENTS_BYTES)
                * FIREHOSE_ROUND_INCREMENTS_BYTES
        })
        .sum()
}
