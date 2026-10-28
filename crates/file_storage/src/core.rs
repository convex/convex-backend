use std::{
    collections::BTreeMap,
    io,
    ops::Bound,
    sync::Arc,
};

use anyhow::Context;
use bytes::Bytes;
use common::{
    components::{
        ComponentId,
        ComponentPath,
    },
    document::{
        ParseDocument,
        ParsedDocument,
        PendingDocument,
    },
    http::RequestDestination,
    knobs::{
        UDF_FILE_STORAGE_DOWNLOAD_TIMEOUT,
        UDF_FILE_STORAGE_UPLOAD_TIMEOUT,
    },
    pause::Fault,
    runtime::{
        Runtime,
        UnixTimestamp,
        WithTimeout,
    },
    sha256::Sha256Digest,
    types::{
        ConvexOrigin,
        ObjectKey,
        StorageUuid,
    },
};
use database::{
    PendingFileUpload,
    Transaction,
};
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use futures::{
    stream::{
        self,
        BoxStream,
    },
    Stream,
    StreamExt,
    TryStreamExt,
};
use futures_async_stream::stream;
use headers::{
    ContentLength,
    ContentRange,
    ContentType,
};
use keybroker::{
    FunctionRunnerKeyBroker,
    Identity,
};
use maplit::btreemap;
use model::{
    canonical_urls::CanonicalUrlsModel,
    file_storage::{
        types::FileStorageEntry,
        BatchKey,
        FileStorageId,
        FileStorageModel,
    },
};
use storage::{
    Storage,
    StorageExt,
    Upload,
    UploadExt,
};
use usage_tracking::{
    StorageCallTracker,
    StorageUsageTracker,
};
use value::{
    id_v6::DeveloperDocumentId,
    ResolvedDocumentId,
    TableNamespace,
};

use crate::{
    metrics::{
        self,
        log_get_file_chunk_size,
        GetFileType,
    },
    FileDownloads,
    FileStorage,
    FileStream,
    TransactionalFileStorage,
    DOWNLOAD_FILE,
    FILE_DOWNLOAD_FAILED_SHORT_MSG,
    FILE_UPLOAD_FAILED_SHORT_MSG,
    UPLOAD_PENDING_FILE,
};

const MAX_CHUNK_SIZE: usize = 32 * 1024;

impl<RT: Runtime> TransactionalFileStorage<RT> {
    pub fn new(rt: RT, storage: Arc<dyn Storage>, convex_origin: ConvexOrigin) -> Self {
        Self {
            rt,
            storage,
            convex_origin,
        }
    }

    pub fn generate_upload_url_with_origin(
        &self,
        origin_override: Option<ConvexOrigin>,
        key_broker: &FunctionRunnerKeyBroker,
        issued_ts: UnixTimestamp,
        component: ComponentId,
    ) -> anyhow::Result<String> {
        let token = key_broker.issue_store_file_authorization(&self.rt, issued_ts, component)?;
        let origin = origin_override.unwrap_or_else(|| self.convex_origin.clone());

        Ok(format!("{origin}/api/storage/upload?token={token}"))
    }

    pub async fn generate_upload_url(
        &self,
        tx: &mut Transaction<RT>,
        key_broker: &FunctionRunnerKeyBroker,
        issued_ts: UnixTimestamp,
        component: ComponentId,
    ) -> anyhow::Result<String> {
        let origin_override = CanonicalUrlsModel::new(tx)
            .get_canonical_urls()
            .await?
            .get(&RequestDestination::ConvexCloud)
            .map(|canonical_url| ConvexOrigin::from(&canonical_url.url));
        self.generate_upload_url_with_origin(origin_override, key_broker, issued_ts, component)
    }

    pub async fn get_url(
        &self,
        tx: &mut Transaction<RT>,
        component: ComponentId,
        storage_id: FileStorageId,
    ) -> anyhow::Result<Option<String>> {
        self.get_url_batch(tx, component, btreemap! { 0 => storage_id })
            .await
            .remove(&0)
            .context("batch_key missing")?
    }

    pub async fn get_url_batch(
        &self,
        tx: &mut Transaction<RT>,
        component: ComponentId,
        storage_ids: BTreeMap<BatchKey, FileStorageId>,
    ) -> BTreeMap<BatchKey, anyhow::Result<Option<String>>> {
        let canonical_origin = match CanonicalUrlsModel::new(tx)
            .get_canonical_urls()
            .await
            .context("Fetching canonical urls failed")
        {
            Ok(canonical_urls) => canonical_urls
                .get(&RequestDestination::ConvexCloud)
                .map_or_else(
                    || self.convex_origin.clone(),
                    |canonical_url| ConvexOrigin::from(&canonical_url.url),
                ),
            Err(e) => {
                // Return the error for each requested storage id.
                return storage_ids
                    .into_keys()
                    .map(|batch_key| (batch_key, Err(e.clone_error())))
                    .collect();
            },
        };
        let files = self
            .get_file_entry_batch(tx, component.into(), storage_ids)
            .await;
        let component_query = component
            .serialize_to_string()
            .map(|s| format!("?component={s}"))
            .unwrap_or_default();
        files
            .into_iter()
            .map(|(batch_key, result)| {
                (
                    batch_key,
                    result.map(|file| {
                        file.map(|entry| {
                            format!(
                                "{canonical_origin}/api/storage/{}{}",
                                entry.storage_id, component_query
                            )
                        })
                    }),
                )
            })
            .collect()
    }

    /// Deletes a file's `_file_storage` entry, returning the id of its row.
    pub async fn delete(
        &self,
        tx: &mut Transaction<RT>,
        namespace: TableNamespace,
        storage_id: FileStorageId,
    ) -> anyhow::Result<ResolvedDocumentId> {
        let Some(id) = self._delete(tx, namespace, storage_id.clone()).await? else {
            anyhow::bail!(ErrorMetadata::bad_request(
                "StorageIdNotFound",
                format!("storage id {storage_id} not found"),
            ));
        };
        Ok(id)
    }

    pub async fn get_file_entry(
        &self,
        tx: &mut Transaction<RT>,
        namespace: TableNamespace,
        storage_id: FileStorageId,
    ) -> anyhow::Result<Option<FileStorageEntry>> {
        self.get_file_entry_batch(tx, namespace, btreemap! { 0 => storage_id })
            .await
            .remove(&0)
            .context("batch_key missing")?
    }

    pub async fn get_file_entry_batch(
        &self,
        tx: &mut Transaction<RT>,
        namespace: TableNamespace,
        storage_ids: BTreeMap<BatchKey, FileStorageId>,
    ) -> BTreeMap<BatchKey, anyhow::Result<Option<FileStorageEntry>>> {
        FileStorageModel::new(tx, namespace)
            .get_file_batch(storage_ids)
            .await
            .into_iter()
            .map(|(batch_key, result)| (batch_key, result.map(|r| r.map(|r| r.into_value()))))
            .collect()
    }

    #[fastrace::trace]
    pub async fn get_file_stream(
        &self,
        component_path: ComponentPath,
        file: FileStorageEntry,
        usage_tracker: impl StorageUsageTracker + Clone + 'static,
    ) -> anyhow::Result<FileStream> {
        let sha256 = file.sha256.clone();

        let mut result = self
            .file_stream(
                component_path,
                file,
                (Bound::Included(0), Bound::Unbounded),
                usage_tracker,
                GetFileType::All,
            )
            .await?;
        result.sha256 = Some(sha256);
        result.content_range = None;
        Ok(result)
    }

    pub async fn get_file_range_stream(
        &self,
        component_path: ComponentPath,
        file: FileStorageEntry,
        bytes_range: (Bound<u64>, Bound<u64>),
        usage_tracker: impl StorageUsageTracker + Clone + 'static,
    ) -> anyhow::Result<FileStream> {
        self.file_stream(
            component_path,
            file,
            bytes_range,
            usage_tracker,
            GetFileType::Range,
        )
        .await
    }

    async fn file_stream(
        &self,
        component_path: ComponentPath,
        file: FileStorageEntry,
        bytes_range: (Bound<u64>, Bound<u64>),
        usage_tracker: impl StorageUsageTracker + Clone + 'static,
        get_file_type: GetFileType,
    ) -> anyhow::Result<FileStream> {
        let FileStorageEntry {
            storage_key,
            size,
            content_type,
            ..
        } = file;

        let content_type = content_type.as_ref().map(|ct| ct.parse()).transpose()?;

        let storage_get_stream = self
            .storage
            .get_range(&storage_key, bytes_range)
            .await?
            .with_context(|| format!("object {storage_key:?} not found"))?;
        let content_range = (size != 0)
            .then(|| ContentRange::bytes(bytes_range, size as u64))
            .transpose()?;
        let stream = storage_get_stream.stream;
        let content_length = ContentLength(storage_get_stream.content_length as u64);

        let call_tracker = usage_tracker
            .track_storage_call(component_path.clone(), "get range", content_type.clone())
            .await;

        Ok(FileStream {
            sha256: None,
            content_length,
            content_range,
            content_type,
            inner: Self::track_stream_usage(component_path, stream, get_file_type, call_tracker),
            bytes_read_so_far: 0,
            on_complete: Vec::new(),
        })
    }

    #[stream(boxed, item = io::Result<Bytes>)]
    async fn track_stream_usage(
        component_path: ComponentPath,
        stream: BoxStream<'static, futures::io::Result<Bytes>>,
        get_file_type: GetFileType,
        storage_call_tracker: Box<dyn StorageCallTracker>,
    ) {
        let stream = stream.flat_map(|bytes| {
            // The input chunk size here depends on the Storage implementation. Our upstream
            // provider seems to send chunks between 1kb and 16kb. Our
            // file storage will send entire files (80+MB).
            // The chunk size here determines the maximum amount we will round up a
            // customer if they read a single byte. The larger our chunk size, the more we
            // round for that byte. So we set a maximum chunk size to limit the maximum
            // amount we round up if the upstream provider sends us a large chunk.
            stream::iter(if let Ok(bytes) = bytes {
                if bytes.len() <= MAX_CHUNK_SIZE {
                    vec![Ok(bytes)]
                } else {
                    bytes
                        .chunks(MAX_CHUNK_SIZE)
                        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
                        .collect::<Vec<_>>()
                }
            } else {
                vec![bytes]
            })
        });
        let mut stream = std::pin::pin!(stream);
        while let Some(bytes) = stream.next().await {
            if let Ok(ref bytes) = bytes {
                let bytes_size = bytes.len() as u64;
                log_get_file_chunk_size(bytes_size, get_file_type);
                storage_call_tracker
                    .track_storage_egress(
                        component_path.clone(),
                        "get_range".to_string(),
                        bytes_size,
                    )
                    .await;
            }
            yield bytes;
        }
    }

    async fn _delete(
        &self,
        tx: &mut Transaction<RT>,
        namespace: TableNamespace,
        storage_id: FileStorageId,
    ) -> anyhow::Result<Option<ResolvedDocumentId>> {
        let deleted = FileStorageModel::new(tx, namespace)
            .delete_file(storage_id, Identity::system())
            .await?;
        Ok(deleted.map(|entry| entry.id()))
    }

    /// `upload_file` just uploads a file to storage. It does not save the file
    /// in the _file_storage system table and it does not count towards
    /// usage. The caller is responsible to call `store_file_entry` to
    /// actually persist the entry and manually account for usage.
    #[fastrace::trace]
    pub async fn upload_file(
        &self,
        content_length: Option<ContentLength>,
        content_type: Option<ContentType>,
        file: impl Stream<Item = anyhow::Result<impl Into<Bytes>>> + Send,
        expected_sha256: Option<Sha256Digest>,
    ) -> anyhow::Result<FileStorageEntry> {
        let storage_id = StorageUuid::from(self.rt.new_uuid_v4());

        tracing::info!("Uploading with content length {content_length:?}");
        let timer = metrics::store_file_timer();

        let mut upload = self.storage.start_upload().await?;
        let file = file.map(|chunk| chunk.map(|chunk| chunk.into()));
        let (size, actual_sha256) = upload.try_write_parallel_and_hash(file).await?;
        if let Some(expected_sha256) = expected_sha256
            && expected_sha256 != actual_sha256
        {
            let msg = format!(
                "Sha256 mismatch. Expected: {} Actual: {}",
                expected_sha256.as_base64(),
                actual_sha256.as_base64()
            );

            anyhow::bail!(ErrorMetadata::bad_request("Sha256Mismatch", msg));
        }

        // Key in underlying storage is a different UUID from the one we hand out.
        let storage_key = upload.complete().await?;

        let elapsed = timer.finish();
        tracing::info!(
            "Wrote file {size} to {storage_key:?}. Total:{elapsed:?} ContentType:{content_type:?}",
        );

        let entry = FileStorageEntry {
            storage_id,
            storage_key,
            sha256: actual_sha256,
            size: size.try_into()?,
            content_type: content_type.map(|ct| ct.to_string()),
        };

        Ok(entry)
    }

    /// Uploads the files stored by the transaction
    #[fastrace::trace]
    pub async fn upload_pending_files(
        &self,
        tx: &mut Transaction<RT>,
        usage_tracker: &dyn StorageUsageTracker,
    ) -> anyhow::Result<()> {
        let pending = tx.get_pending_file_uploads()?;
        let writes = tx.writes().as_flat()?;
        let mut files = vec![];
        for (id, upload) in pending {
            let new_document = writes
                .get(&id)
                .and_then(|update| update.new_document.as_ref())
                .with_context(|| format!("Pending upload for missing _file_storage entry {id}"))?;
            let PendingDocument::Concrete(document) = new_document else {
                anyhow::bail!("_file_storage entry {id} has a pending body");
            };
            let entry: ParsedDocument<FileStorageEntry> = document.clone().parse()?;
            files.push((entry.into_value(), upload));
        }
        self.rt
            .with_timeout(
                "upload_pending_files",
                *UDF_FILE_STORAGE_UPLOAD_TIMEOUT,
                futures::future::try_join_all(
                    files
                        .iter()
                        .map(|(entry, upload)| self.upload_pending_file(entry, upload)),
                ),
            )
            .await
            .context(ErrorMetadata::overloaded(
                FILE_UPLOAD_FAILED_SHORT_MSG,
                "Failed to upload the files this mutation stored. Try again later.",
            ))?;
        for (entry, upload) in files {
            Self::track_pending_file_usage(entry, upload.component_path, usage_tracker).await?;
        }
        tx.mark_files_uploaded()
    }

    async fn upload_pending_file(
        &self,
        entry: &FileStorageEntry,
        upload: &PendingFileUpload,
    ) -> anyhow::Result<()> {
        let timer = metrics::store_file_timer();
        if let Fault::Error(e) = self.rt.pause_client().wait(UPLOAD_PENDING_FILE).await {
            return Err(e);
        }
        self.storage
            .put_object(entry.storage_key.clone(), upload.bytes.clone())
            .await
            .with_context(|| format!("Failed to upload file {}", entry.storage_id))?;
        timer.finish();
        Ok(())
    }

    async fn track_pending_file_usage(
        entry: FileStorageEntry,
        component_path: ComponentPath,
        usage_tracker: &dyn StorageUsageTracker,
    ) -> anyhow::Result<()> {
        let content_type = entry
            .content_type
            .as_ref()
            .map(|ct| ct.parse())
            .transpose()?;
        usage_tracker
            .track_storage_call(component_path.clone(), "store", content_type)
            .await
            .track_storage_ingress(component_path, "store".to_string(), entry.size as u64)
            .await;
        Ok(())
    }

    #[fastrace::trace]
    pub async fn download_files(
        &self,
        downloads: &FileDownloads,
        files: Vec<(ComponentPath, FileStorageEntry)>,
        usage_tracker: impl StorageUsageTracker + Clone + 'static,
    ) -> anyhow::Result<()> {
        let remaining = UDF_FILE_STORAGE_DOWNLOAD_TIMEOUT.saturating_sub(downloads.download_time());
        let start = self.rt.monotonic_now();
        let result = self
            .rt
            .with_timeout(
                "download_files",
                remaining,
                futures::future::try_join_all(files.into_iter().map(|(component_path, entry)| {
                    self.download_file(component_path, entry, usage_tracker.clone())
                })),
            )
            .await;
        downloads.add_download_time(start.elapsed());
        let files = result.context(ErrorMetadata::overloaded(
            FILE_DOWNLOAD_FAILED_SHORT_MSG,
            "Failed to download the files this mutation read. Try again later.",
        ))?;
        downloads.insert(files);
        Ok(())
    }

    async fn download_file(
        &self,
        component_path: ComponentPath,
        entry: FileStorageEntry,
        usage_tracker: impl StorageUsageTracker + Clone + 'static,
    ) -> anyhow::Result<(ObjectKey, Bytes)> {
        if let Fault::Error(e) = self.rt.pause_client().wait(DOWNLOAD_FILE).await {
            return Err(e);
        }
        let storage_key = entry.storage_key.clone();
        let size = entry.size;
        let chunks: Vec<Bytes> = self
            .get_file_stream(component_path, entry, usage_tracker)
            .await?
            .try_collect()
            .await?;
        let contents = Bytes::from(chunks.concat());
        anyhow::ensure!(
            contents.len() as i64 == size,
            "Downloaded {} bytes of {storage_key:?}, expected {size}",
            contents.len(),
        );
        Ok((storage_key, contents))
    }

    /// Stores a file entry generated by upload_file(). The caller is
    /// responsible to track usage. If you are outside of the
    /// isolate environment, it is recommended to use FileStorage::store_file
    /// that performs all necessary steps instead.
    pub async fn store_file_entry(
        &self,
        tx: &mut Transaction<RT>,
        namespace: TableNamespace,
        entry: FileStorageEntry,
    ) -> anyhow::Result<(ResolvedDocumentId, DeveloperDocumentId)> {
        let system_id = FileStorageModel::new(tx, namespace)
            .store_file(entry)
            .await?;
        let virtual_id = tx
            .virtual_system_mapping()
            .system_resolved_id_to_virtual_developer_id(system_id)?;

        Ok((system_id, virtual_id))
    }
}

impl<RT: Runtime> FileStorage<RT> {
    pub async fn store_file(
        &self,
        namespace: TableNamespace,
        content_length: Option<ContentLength>,
        content_type: Option<ContentType>,
        file: impl Stream<Item = anyhow::Result<impl Into<Bytes>>> + Send,
        expected_sha256: Option<Sha256Digest>,
        usage_tracker: &dyn StorageUsageTracker,
    ) -> anyhow::Result<DeveloperDocumentId> {
        let entry = self
            .transactional_file_storage
            .upload_file(content_length, content_type, file, expected_sha256)
            .await?;
        self.store_entry(namespace, entry, usage_tracker).await
    }

    /// Record the existence of a file that has already been uploaded to the
    /// underlying storage implementation.
    pub async fn store_entry(
        &self,
        namespace: TableNamespace,
        entry: FileStorageEntry,
        usage_tracker: &dyn StorageUsageTracker,
    ) -> anyhow::Result<DeveloperDocumentId> {
        let size = entry.size;
        let content_type = entry
            .content_type
            .as_ref()
            .map(|ct| ct.parse())
            .transpose()?;

        // Start/Complete transaction after the slow upload process
        // to avoid OCC risk.
        let mut tx = self.database.begin(Identity::system()).await?;
        let (_, virtual_id) = self
            .transactional_file_storage
            .store_file_entry(&mut tx, namespace, entry)
            .await?;
        let component_path = tx.must_component_path(ComponentId::from(namespace))?;
        self.database
            .commit_with_write_source(tx, "file_storage_store_file")
            .await?;

        usage_tracker
            .track_storage_call(component_path.clone(), "store", content_type)
            .await
            .track_storage_ingress(component_path, "store".to_string(), size as u64)
            .await;
        Ok(virtual_id)
    }
}
