use anyhow::Context;
use common::{
    http::fetch::FetchAborted,
    runtime::Runtime,
};
use errors::ErrorMetadata;
use futures::{
    stream::BoxStream,
    StreamExt,
};

use super::task_executor::TaskExecutor;
use crate::{
    convert_v8::{
        DOMException,
        DOMExceptionName,
    },
    environment::action::task::{
        FormPart,
        FormPartFile,
        TaskResponse,
    },
};

// The maximum size of a multipart form body is 20 MiB. Multipart forms are
// parsed in memory (because FormData allows accessing entries in arbitrary
// order), so this limit protects the server from running out of memory. Other
// HTTP action request bodies are streamed to the isolate and have no size
// limit in the backend.
pub const MULTIPART_BODY_LIMIT: u64 = 20 << 20;

impl<RT: Runtime> TaskExecutor<RT> {
    // Sends a stream to javascript by sending TaskResponse::StreamExtend
    // repeatedly. Any errors are sent with StreamExtend, and the number of bytes
    // sent are returned on success.
    pub async fn send_stream(
        &self,
        stream_id: uuid::Uuid,
        stream: Option<BoxStream<'static, anyhow::Result<bytes::Bytes>>>,
    ) -> Result<usize, ()> {
        let mut size = 0;
        if let Some(mut stream) = stream {
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Err(e) => {
                        let error = match e.downcast::<FetchAborted>() {
                            Ok(aborted) => {
                                DOMException::new(aborted.to_string(), DOMExceptionName::AbortError)
                                    .into()
                            },
                            Err(e) => {
                                ErrorMetadata::bad_request("StreamFailed", e.to_string()).into()
                            },
                        };
                        _ = self.task_retval_sender.send(TaskResponse::StreamExtend {
                            stream_id,
                            chunk: Err(error),
                        });
                        return Err(());
                    },
                    Ok(chunk) => {
                        size += chunk.len();
                        _ = self.task_retval_sender.send(TaskResponse::StreamExtend {
                            stream_id,
                            chunk: Ok(Some(chunk)),
                        });
                    },
                }
            }
        }
        // Successfully sent all chunks.
        _ = self.task_retval_sender.send(TaskResponse::StreamExtend {
            stream_id,
            chunk: Ok(None),
        });
        Ok(size)
    }

    pub async fn run_parse_multi_part(
        &self,
        content_type: String,
        request_stream: BoxStream<'static, anyhow::Result<bytes::Bytes>>,
    ) -> anyhow::Result<Vec<FormPart>> {
        // NOTE we use multer instead of axum::extract::Multipart because
        // of https://github.com/tokio-rs/axum/issues/3131
        let boundary = multer::parse_boundary(&content_type).with_context(|| {
            ErrorMetadata::bad_request(
                "InvalidMultiPartForm",
                format!("multi-part form invalid boundary: '{content_type}'"),
            )
        })?;
        let mut multipart = multer::Multipart::with_constraints(
            request_stream,
            boundary,
            multer::Constraints::new()
                .size_limit(multer::SizeLimit::new().whole_stream(MULTIPART_BODY_LIMIT)),
        );
        let mut results = vec![];
        while let Some(field) = multipart.next_field().await.map_err(map_multer_error)? {
            let name = field
                .name()
                .with_context(|| {
                    ErrorMetadata::bad_request(
                        "InvalidMultiPartForm",
                        "multi-part form entry missing 'name'",
                    )
                })?
                .to_string();
            let (file, text) = match field.file_name() {
                None => (None, Some(field.text().await.map_err(map_multer_error)?)),
                Some(file_name) => {
                    let file_name = Some(file_name.to_string());
                    let content_type = field.content_type().map(|c| c.to_string());
                    let data = field.bytes().await.map_err(map_multer_error)?;
                    (
                        Some(FormPartFile {
                            content_type,
                            file_name,
                            data,
                        }),
                        None,
                    )
                },
            };
            results.push(FormPart { name, text, file });
        }
        Ok(results)
    }
}

fn map_multer_error(e: multer::Error) -> anyhow::Error {
    match &e {
        // Internal errors.
        multer::Error::StreamReadFailed(_)
        | multer::Error::LockFailure
        | multer::Error::UnknownField { .. } => e.into(),
        // User errors.
        multer::Error::StreamSizeExceeded { .. }
        | multer::Error::FieldSizeExceeded { .. }
        | multer::Error::IncompleteFieldData { .. }
        | multer::Error::IncompleteHeaders
        | multer::Error::ReadHeaderFailed(_)
        | multer::Error::DecodeHeaderName { .. }
        | multer::Error::DecodeHeaderValue { .. }
        | multer::Error::IncompleteStream
        | multer::Error::NoMultipart
        | multer::Error::DecodeContentType(_)
        | multer::Error::NoBoundary => ErrorMetadata::bad_request(
            "InvalidMultiPartForm",
            format!("invalid multi-part form: '{e}'"),
        )
        .into(),
        _ => e.into(),
    }
}
