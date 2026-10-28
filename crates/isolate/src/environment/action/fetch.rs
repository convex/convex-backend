use std::{
    sync::atomic::Ordering,
    time::Duration,
};

use ::metrics::StatusTimer;
use common::{
    http::{
        fetch::FetchAborted,
        HttpRequestStream,
        HttpResponseStream,
    },
    runtime::Runtime,
};
use errors::ErrorMetadata;

use super::task_executor::TaskExecutor;
use crate::{
    convert_v8::{
        DOMException,
        DOMExceptionName,
    },
    environment::action::task::{
        TaskId,
        TaskResponse,
        TaskResponseEnum,
    },
    http::HttpResponseV8,
    metrics,
};

impl<RT: Runtime> TaskExecutor<RT> {
    #[convex_macro::instrument_future]
    pub async fn run_fetch(
        &self,
        task_id: TaskId,
        request: HttpRequestStream,
        stream_id: uuid::Uuid,
    ) {
        let t = metrics::udf_fetch_timer();
        // Only log origin because query params might contain some PII.
        let origin = request.url.origin().unicode_serialization();
        let result = self.run_fetch_inner(request).await;
        let initial_response_time = t.elapsed();
        let (request_size, (body, response)) = match result.and_then(|response| {
            Ok((
                response.request_size.clone(),
                HttpResponseV8::from_response_stream(response, stream_id)?,
            ))
        }) {
            Ok(parts) => parts,
            Err(e) => {
                let error = match e.downcast::<FetchAborted>() {
                    Ok(aborted) => {
                        DOMException::new(aborted.to_string(), DOMExceptionName::AbortError).into()
                    },
                    // Other fetch errors are treated as developer errors since we have little
                    // control of what they request.
                    Err(e) => ErrorMetadata::bad_request("FetchFailed", format!("{e:#}")).into(),
                };
                _ = self.task_retval_sender.send(TaskResponse::TaskDone {
                    task_id,
                    variant: Err(error),
                });
                self.log_fetch_request(t, origin, Err(()), initial_response_time, 0);
                return;
            },
        };
        _ = self.task_retval_sender.send(TaskResponse::TaskDone {
            task_id,
            variant: Ok(TaskResponseEnum::Fetch(response)),
        });
        // After sending status and headers, send the body one chunk at a time.
        let stream_result = self.send_stream(stream_id, body).await;
        self.log_fetch_request(
            t,
            origin,
            stream_result,
            initial_response_time,
            request_size.load(Ordering::Relaxed),
        );
    }

    #[convex_macro::instrument_future]
    async fn run_fetch_inner(
        &self,
        request: HttpRequestStream,
    ) -> anyhow::Result<HttpResponseStream> {
        self.fetch_client.fetch(request).await
    }

    fn log_fetch_request(
        &self,
        t: StatusTimer,
        origin: String,
        success: Result<usize, ()>,
        initial_response_time: Duration,
        request_size: u64,
    ) {
        // Would love to log the error here or in sentry, but they might contain PII.
        tracing::info!(
            "Fetch to origin: {origin}, success: {}, initial_response_time: \
             {initial_response_time:?}, total_time: {:?}, request_size: {:?}, response_size: {:?}",
            success.is_ok(),
            t.elapsed(),
            request_size,
            success.ok(),
        );
        metrics::finish_udf_fetch_timer(t, success);
        self.usage_tracker.track_fetch_egress(origin, request_size);
    }
}
