use std::str::FromStr;

use anyhow::Context;
use common::{
    runtime::Runtime,
    sha256::{
        DigestHeader,
        Sha256Digest,
    },
    sync::spsc,
};
use errors::ErrorMetadata;
use futures::{
    stream::BoxStream,
    StreamExt,
    TryStreamExt,
};
use headers::{
    Header,
    HeaderValue,
};
use model::file_storage::FileStorageId;
use usage_tracking::StorageUsageTracker;
use value::id_v6::DeveloperDocumentId;

use super::task_executor::TaskExecutor;
use crate::environment::{
    action::task::{
        FileResponse,
        TaskId,
        TaskResponse,
        TaskResponseEnum,
    },
    helpers::{
        with_argument_error,
        ArgName,
    },
};

impl<RT: Runtime> TaskExecutor<RT> {
    #[convex_macro::instrument_future]
    pub async fn run_storage_store(
        &self,
        body_stream: spsc::UnboundedReceiver<anyhow::Result<bytes::Bytes>>,
        content_type: Option<String>,
        content_length: Option<String>,
        digest: Option<String>,
    ) -> anyhow::Result<DeveloperDocumentId> {
        let content_length = content_length
            .map(|c| -> anyhow::Result<headers::ContentLength> {
                Ok(headers::ContentLength(c.parse()?))
            })
            .transpose()
            .map_err(|e| ErrorMetadata::bad_request("InvalidContentLengthHeader", e.to_string()))?;
        let content_type = content_type
            .map(|c| -> anyhow::Result<headers::ContentType> {
                Ok(headers::ContentType::from(mime::Mime::from_str(&c)?))
            })
            .transpose()
            .map_err(|e| ErrorMetadata::bad_request("InvalidContentTypeHeader", e.to_string()))?;
        let digest = digest
            .map(|header_string| -> anyhow::Result<Sha256Digest> {
                let header_value = HeaderValue::from_str(&header_string)?;
                let digest_header = DigestHeader::decode(&mut std::iter::once(&header_value))?;
                Ok(digest_header.0)
            })
            .transpose()
            .map_err(|e| ErrorMetadata::bad_request("InvalidDigestHeader", e.to_string()))?;

        let entry = self
            .file_storage
            .upload_file(
                content_length,
                content_type.clone(),
                body_stream.into_stream(),
                digest,
            )
            .await?;
        let size = entry.size;
        let (component_path, storage_doc_id) = self
            .action_callbacks
            .storage_store_file_entry(self.identity.clone(), self.component_id(), entry)
            .await?;

        self.usage_tracker
            .track_storage_call(component_path.clone(), "store", content_type)
            .await
            .track_storage_ingress(component_path, "store".to_string(), size as u64)
            .await;

        Ok(storage_doc_id)
    }

    #[convex_macro::instrument_future]
    pub async fn run_storage_get(
        &self,
        task_id: TaskId,
        storage_id: String,
        stream_id: uuid::Uuid,
    ) {
        match self.run_storage_get_inner(storage_id, stream_id).await {
            Err(e) => {
                let _ = self.task_retval_sender.send(TaskResponse::TaskDone {
                    task_id,
                    variant: Err(e),
                });
            },
            Ok(None) => {
                let _ = self.task_retval_sender.send(TaskResponse::TaskDone {
                    task_id,
                    variant: Ok(TaskResponseEnum::StorageGet(None)),
                });
            },
            Ok(Some((stream, result))) => {
                let _ = self.task_retval_sender.send(TaskResponse::TaskDone {
                    task_id,
                    variant: Ok(TaskResponseEnum::StorageGet(Some(result))),
                });
                let _ = self.send_stream(stream_id, Some(stream)).await;
            },
        }
    }

    async fn run_storage_get_inner(
        &self,
        storage_id: String,
        stream_id: uuid::Uuid,
    ) -> anyhow::Result<
        Option<(
            BoxStream<'static, anyhow::Result<bytes::Bytes>>,
            FileResponse,
        )>,
    > {
        let storage_id: FileStorageId = with_argument_error("storage.getMetadata", || {
            storage_id.parse().context(ArgName("storageId"))
        })?;

        let file_entry = self
            .action_callbacks
            .storage_get_file_entry(self.identity.clone(), self.component_id(), storage_id)
            .await?;
        let (component_path, file_entry) = match file_entry {
            None => return Ok(None),
            Some(f) => f,
        };
        let file_stream = self
            .file_storage
            .get_file_stream(component_path, file_entry, self.usage_tracker.clone())
            .await?;

        let content_length = file_stream.content_length.0;
        let content_type = file_stream.content_type.as_ref().map(|c| c.to_string());
        let stream = file_stream.map_err(|e| e.into()).boxed();
        let r = FileResponse {
            body_stream_id: stream_id,
            content_length,
            content_type,
        };
        Ok(Some((stream, r)))
    }
}
