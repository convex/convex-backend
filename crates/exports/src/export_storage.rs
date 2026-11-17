use std::{
    collections::BTreeMap,
    time::Instant,
};

use anyhow::Context;
use bytes::Bytes;
use common::{
    components::ComponentPath,
    document::ParseDocument,
    knobs::{
        EXPORT_MAX_INFLIGHT_PREFETCH_BYTES,
        EXPORT_PROGRESS_UPDATE_INTERVAL,
        EXPORT_STORAGE_GET_CONCURRENCY,
    },
    persistence::LatestDocument,
    runtime::Runtime,
    types::{
        IndexRef,
        TableName,
    },
};
use database::MultiTableIterator;
use fastrace::{
    future::FutureExt,
    Span,
};
use futures::{
    pin_mut,
    stream,
    Future,
    StreamExt,
    TryStreamExt,
};
use mime2ext::mime2ext;
use model::{
    exports::types::ExportRequestor,
    file_storage::{
        types::FileStorageEntry,
        FILE_STORAGE_TABLE,
        FILE_STORAGE_VIRTUAL_TABLE,
    },
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::json;
use storage::StorageExt;
use thousands::Separable;
use tokio_util::io::StreamReader;
use usage_tracking::{
    FunctionUsageTracker,
    StorageCallTracker,
    StorageUsageTracker,
};
use value::{
    TableNamespace,
    TabletId,
};

use crate::{
    ensure_file_storage_export_size,
    zip_uploader::ZipSnapshotUpload,
    ExportComponents,
};

pub(crate) async fn write_storage_table<'a, 'b: 'a, F, Fut, RT: Runtime>(
    components: &ExportComponents<RT>,
    path_prefix: &str,
    zip_snapshot_upload: &'a mut ZipSnapshotUpload<'b>,
    namespace: TableNamespace,
    component_path: &ComponentPath,
    table_iterator: &mut MultiTableIterator<RT>,
    by_id_indexes: &BTreeMap<TabletId, IndexRef>,
    system_tables: &BTreeMap<(TableNamespace, TableName), TabletId>,
    usage: &FunctionUsageTracker,
    requestor: ExportRequestor,
    update_progress: &F,
    in_component_str: &str,
    storage_total_entries: u64,
    file_storage_size: &mut u64,
) -> anyhow::Result<()>
where
    F: Fn(String) -> Fut + Send,
    Fut: Future<Output = anyhow::Result<()>> + Send,
{
    // _storage
    let tablet_id = system_tables
        .get(&(namespace, FILE_STORAGE_TABLE.clone()))
        .context("_file_storage does not exist")?;
    let by_id = by_id_indexes
        .get(tablet_id)
        .context("_file_storage.by_id does not exist")?;

    // First write metadata to _storage/documents.jsonl
    let mut table_upload = zip_snapshot_upload
        .start_system_table(path_prefix, FILE_STORAGE_VIRTUAL_TABLE.clone())
        .await?;
    {
        let stream = table_iterator.stream_documents_in_table(*tablet_id, *by_id, None);
        pin_mut!(stream);
        let mut num_storage_entries: u64 = 0;
        let mut last_log_time = Instant::now();
        while let Some(LatestDocument { value: doc, .. }) = stream.try_next().await? {
            let file_storage_entry = ParseDocument::<FileStorageEntry>::parse(doc)?;
            let file_size = u64::try_from(file_storage_entry.size)
                .context("file storage entry has a negative size")?;
            *file_storage_size = file_storage_size
                .checked_add(file_size)
                .context("total file storage size overflowed")?;
            ensure_file_storage_export_size(*file_storage_size)?;
            let virtual_storage_id = file_storage_entry.id().developer_id;
            let creation_time = f64::from(file_storage_entry.creation_time());
            table_upload
                .write_json_line(json!(FileStorageZipMetadata {
                    id: virtual_storage_id.encode(),
                    creation_time: Some(creation_time),
                    sha256: Some(file_storage_entry.sha256.as_base64()),
                    size: Some(file_storage_entry.size),
                    content_type: file_storage_entry.content_type.clone(),
                    internal_id: Some(file_storage_entry.storage_id.to_string()),
                }))
                .await?;
            num_storage_entries += 1;
            if last_log_time.elapsed() >= *EXPORT_PROGRESS_UPDATE_INTERVAL {
                tracing::info!(
                    "Export _storage metadata in progress: {num_storage_entries} entries written \
                     so far",
                );
                update_progress(format!(
                    "Backing up _storage{in_component_str}: {} / {} entries (metadata)",
                    num_storage_entries.separate_with_commas(),
                    storage_total_entries.separate_with_commas(),
                ))
                .await?;
                last_log_time = Instant::now();
            }
        }
        tracing::info!("Export _storage metadata complete: {num_storage_entries} entries",);
    }
    table_upload.complete().await?;

    let max_prefetch_bytes = *EXPORT_MAX_INFLIGHT_PREFETCH_BYTES;
    let inflight_bytes_semaphore = tokio::sync::Semaphore::new(max_prefetch_bytes);
    let files_stream = table_iterator
        .stream_documents_in_table(*tablet_id, *by_id, None)
        .map_ok(|LatestDocument { value: doc, .. }| async {
            let file_storage_entry = ParseDocument::<FileStorageEntry>::parse(doc)?;
            let virtual_storage_id = file_storage_entry.id().developer_id;
            // Add an extension, which isn't necessary for anything and might be incorrect,
            // but allows the file to be viewed at a glance in most cases.
            let extension_guess = file_storage_entry
                .content_type
                .as_ref()
                .and_then(mime2ext)
                .map(|extension| format!(".{extension}"))
                .unwrap_or_default();
            let path = format!(
                "{path_prefix}{}/{}{extension_guess}",
                FILE_STORAGE_VIRTUAL_TABLE,
                virtual_storage_id.encode()
            );
            let file_stream = components
                .file_storage
                .get(&file_storage_entry.storage_key)
                .await?
                .with_context(|| {
                    format!(
                        "file missing from storage: {} with key {:?}",
                        file_storage_entry.developer_id().encode(),
                        file_storage_entry.storage_key,
                    )
                })?;

            let content_type = file_storage_entry
                .content_type
                .as_ref()
                .map(|ct| ct.parse())
                .transpose()?;
            usage
                .track_storage_call(component_path.clone(), requestor.usage_tag(), content_type)
                .await;
            usage
                .track_storage_egress(
                    component_path.clone(),
                    requestor.usage_tag().to_string(),
                    file_stream.content_length as u64,
                )
                .await;

            if (file_stream.content_length as usize) < max_prefetch_bytes {
                let permit = inflight_bytes_semaphore
                    .acquire_many(file_stream.content_length as u32)
                    .await?;
                // Prefetch the file before passing it to the zip writer.
                // This can happen in parallel with other files.
                let bytes: Vec<Bytes> = file_stream
                    .stream
                    .try_collect()
                    .in_span(Span::enter_with_local_parent("prefetch_storage_file"))
                    .await?;
                let stream = StreamReader::new(stream::iter(bytes.into_iter().map(Ok)).boxed());
                Ok((path, stream, permit))
            } else {
                // Wait until all other ongoing prefetches are finished, then stream this file
                // serially.
                let permit = inflight_bytes_semaphore
                    .acquire_many(max_prefetch_bytes as u32)
                    .await?;
                // Note that fetching won't start until the reader is first polled (which won't
                // happen until it's passed to `stream_full_file`).
                Ok((path, file_stream.into_tokio_reader(), permit))
            }
        })
        .try_buffer_unordered(*EXPORT_STORAGE_GET_CONCURRENCY); // Note that this will return entries in an arbitrary order
    pin_mut!(files_stream);
    let mut num_files: u64 = 0;
    let mut last_log_time = Instant::now();
    while let Some((path, file_stream, permit)) = files_stream.try_next().await? {
        zip_snapshot_upload
            .stream_full_file(path, file_stream)
            .await?;
        drop(permit);
        num_files += 1;
        if last_log_time.elapsed() >= *EXPORT_PROGRESS_UPDATE_INTERVAL {
            tracing::info!(
                "Export _storage files in progress: {num_files} files downloaded so far",
            );
            update_progress(format!(
                "Backing up _storage{in_component_str}: {} / {} files (downloading)",
                num_files.separate_with_commas(),
                storage_total_entries.separate_with_commas(),
            ))
            .await?;
            last_log_time = Instant::now();
        }
    }
    tracing::info!("Export _storage files complete: {num_files} files");
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStorageZipMetadata {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_creationTime")]
    pub creation_time: Option<f64>,
    pub sha256: Option<String>,
    pub size: Option<i64>,
    pub content_type: Option<String>,
    pub internal_id: Option<String>,
}
