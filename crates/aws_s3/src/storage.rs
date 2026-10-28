use std::{
    env,
    path::Path,
    pin::Pin,
    sync::Arc,
    time::{
        Duration,
        SystemTime,
    },
};

use anyhow::Context;
use async_trait::async_trait;
use aws_config::retry::RetryConfig;
use aws_sdk_s3::{
    error::ProvideErrorMetadata,
    operation::{
        create_multipart_upload::builders::CreateMultipartUploadFluentBuilder,
        head_object::{
            HeadObjectError,
            HeadObjectOutput,
        },
        upload_part::builders::UploadPartFluentBuilder,
    },
    presigning::PresigningConfig,
    primitives::ByteStream,
    types::{
        ChecksumAlgorithm,
        CompletedMultipartUpload,
        CompletedPart,
        ServerSideEncryption,
    },
    Client,
};
use aws_utils::{
    are_checksums_disabled,
    is_range_prefetch_disabled,
    is_sse_disabled,
    must_s3_config_from_env,
    s3::S3Client,
};
use bytes::Bytes;
use common::{
    errors::report_error,
    knobs::STORAGE_MAX_INTERMEDIATE_PART_SIZE,
    runtime::Runtime,
    types::{
        FullyQualifiedObjectKey,
        ObjectKey,
    },
};
use errors::ErrorMetadata;
use futures::{
    future::{
        self,
        BoxFuture,
        Either,
    },
    stream,
    Future,
    FutureExt,
    Stream,
    StreamExt,
    TryStreamExt,
};
use pb::searchlight::storage_type::StorageType;
use serde_json::{
    json,
    Value as JsonValue,
};
use storage::{
    BufferedUpload,
    ClientDrivenUploadPartToken,
    ClientDrivenUploadToken,
    InvalidGetRangeError,
    LocalDirStorage,
    ObjectAttributes,
    ObjectListing,
    Storage,
    StorageCacheKey,
    StorageGetStream,
    StorageUseCase,
    Upload,
    UploadId,
    MAXIMUM_PARALLEL_UPLOADS,
};

use crate::{
    metrics::sign_url_timer,
    types::{
        ObjectPart,
        PartNumber,
    },
    ByteStreamCompat,
};

pub const ACCESS_KEY_INITIAL_BACKOFF: Duration = Duration::from_millis(500);
pub const ACCESS_KEY_MAX_BACKOFF: Duration = Duration::from_secs(5);

/// The following are not knobs because they are fixed by S3.
/// The part size we use starts at the min and doubles until the max,
/// which allows very large files but still supports fast uploads for small
/// files.
/// S3 minimum part size for multipart upload is 5MiB
const MIN_S3_INTERMEDIATE_PART_SIZE: usize = 5 * (1 << 20);
/// S3 maximum part size for multipart upload is 5GiB
const MAX_S3_INTERMEDIATE_PART_SIZE: usize = 5 * (1 << 30);

#[derive(Clone)]
pub struct S3Storage<RT: Runtime> {
    client: Client,
    bucket: String,

    // Prefix gets added as prefix to all keys.
    key_prefix: String,
    runtime: RT,
}

impl<RT: Runtime> std::fmt::Debug for S3Storage<RT> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Storage")
            .field("bucket", &self.bucket)
            .field("key_prefix", &self.key_prefix)
            .finish()
    }
}

impl<RT: Runtime> S3Storage<RT> {
    pub fn new_from_client(
        client: S3Client,
        use_case: StorageUseCase,
        key_prefix: String,
        runtime: RT,
    ) -> anyhow::Result<Self> {
        let bucket = s3_bucket_name(&use_case)?;
        Ok(Self {
            client: client.0,
            bucket,
            key_prefix,
            runtime,
        })
    }

    pub async fn new_with_prefix(
        bucket: String,
        key_prefix: String,
        runtime: RT,
    ) -> anyhow::Result<Self> {
        let client = s3_client().await?;
        let storage = Self {
            client,
            bucket,
            key_prefix,
            runtime,
        };
        Ok(storage)
    }

    pub async fn for_use_case(
        use_case: StorageUseCase,
        key_prefix: String,
        runtime: RT,
    ) -> anyhow::Result<Self> {
        let bucket_name = s3_bucket_name(&use_case)?;
        S3Storage::new_with_prefix(bucket_name, key_prefix, runtime).await
    }

    fn s3_key(&self, key: &ObjectKey) -> S3Key {
        S3Key(self.key_prefix.clone() + key)
    }

    /// Helper method to configure multipart upload builder with optional AWS
    /// headers for S3 compatibility with non-AWS services
    fn configure_multipart_upload_builder(
        &self,
        upload_builder: CreateMultipartUploadFluentBuilder,
    ) -> CreateMultipartUploadFluentBuilder {
        upload_builder
            .set_server_side_encryption(server_side_encryption())
            // Because we're using multipart uploads, we're really specifying the part
            // checksum algorithm here, so it needs to match what we use for
            // each part.
            .set_checksum_algorithm(checksum_algorithm())
    }

    async fn start_upload_with_key(&self, key: ObjectKey) -> anyhow::Result<S3Upload<RT>> {
        let s3_key = self.s3_key(&key);
        let upload_builder = self
            .client
            .create_multipart_upload()
            .bucket(self.bucket.clone())
            .key(&s3_key.0);

        let upload_builder = self.configure_multipart_upload_builder(upload_builder);

        let output = upload_builder
            .send()
            .await
            .context("Failed to create multipart upload")?;
        let upload_id = output
            .upload_id()
            .ok_or_else(|| anyhow::anyhow!("Multipart upload is missing an upload_id."))?;
        let s3_upload = S3Upload::new(
            self.client.clone(),
            self.bucket.clone(),
            upload_id.to_string().into(),
            key,
            s3_key,
            self.runtime.clone(),
        )
        .await?;
        Ok(s3_upload)
    }
}

/// Inverse of [`Storage::storage_type_proto`].
pub async fn storage_from_proto<RT: Runtime>(
    rt: RT,
    storage_type: StorageType,
) -> anyhow::Result<Arc<dyn Storage>> {
    Ok(match storage_type {
        StorageType::S3(prefix) => {
            Arc::new(S3Storage::new_with_prefix(prefix.bucket, prefix.prefix, rt).await?)
        },
        StorageType::Local(dir) => Arc::new(LocalDirStorage::new_at_path(
            rt,
            Path::new(&dir.path).to_owned(),
        )?),
    })
}

async fn s3_client() -> Result<Client, anyhow::Error> {
    static S3_CLIENT: tokio::sync::OnceCell<Client> = tokio::sync::OnceCell::const_new();
    let client = S3_CLIENT
        .get_or_try_init(|| async {
            let config = must_s3_config_from_env()
                .await
                .context("AWS env variables are required when using S3 storage")?;
            let s3_config = config.retry_config(RetryConfig::standard()).build();
            anyhow::Ok(Client::from_conf(s3_config))
        })
        .await?
        .clone();
    Ok(client)
}

struct ClientDrivenUpload {
    object_key: ObjectKey,
    upload_id: UploadId,
}

impl TryFrom<ClientDrivenUpload> for ClientDrivenUploadToken {
    type Error = anyhow::Error;

    fn try_from(value: ClientDrivenUpload) -> Result<Self, Self::Error> {
        let v = json!({
            "objectKey": value.object_key.to_string(),
            "uploadId": value.upload_id.to_string(),
        });
        Ok(ClientDrivenUploadToken(serde_json::to_string(&v)?))
    }
}

impl TryFrom<ClientDrivenUploadToken> for ClientDrivenUpload {
    type Error = anyhow::Error;

    fn try_from(value: ClientDrivenUploadToken) -> Result<Self, Self::Error> {
        let v: JsonValue = serde_json::from_str(&value.0)?;
        let object_key = v
            .get("objectKey")
            .context("missing objectKey")?
            .as_str()
            .context("objectKey should be str")?
            .try_into()?;
        let upload_id = v
            .get("uploadId")
            .context("missing uploadId")?
            .as_str()
            .context("uploadId should be str")?
            .to_string()
            .into();
        Ok(Self {
            object_key,
            upload_id,
        })
    }
}

#[async_trait]
impl<RT: Runtime> Storage for S3Storage<RT> {
    #[fastrace::trace]
    async fn start_upload(&self) -> anyhow::Result<Box<BufferedUpload>> {
        let key: ObjectKey = self.runtime.new_uuid_v4().to_string().try_into()?;
        let upload = BufferedUpload::new(
            self.start_upload_with_key(key).await?,
            MIN_S3_INTERMEDIATE_PART_SIZE,
            std::cmp::min(
                MAX_S3_INTERMEDIATE_PART_SIZE,
                *STORAGE_MAX_INTERMEDIATE_PART_SIZE,
            ),
        );
        Ok(Box::new(upload))
    }

    async fn start_client_driven_upload(&self) -> anyhow::Result<ClientDrivenUploadToken> {
        let key: ObjectKey = self.runtime.new_uuid_v4().to_string().try_into()?;
        let s3_key = self.s3_key(&key);
        let upload_builder = self
            .client
            .create_multipart_upload()
            .bucket(self.bucket.clone())
            .key(&s3_key.0);

        let upload_builder = self.configure_multipart_upload_builder(upload_builder);

        let output = upload_builder
            .send()
            .await
            .context("Failed to create multipart upload")?;
        let upload_id = output
            .upload_id()
            .ok_or_else(|| anyhow::anyhow!("Multipart upload is missing an upload_id."))?;
        ClientDrivenUpload {
            object_key: key,
            upload_id: upload_id.to_string().into(),
        }
        .try_into()
    }

    async fn upload_part(
        &self,
        token: ClientDrivenUploadToken,
        part_number: u16,
        part: Bytes,
    ) -> anyhow::Result<ClientDrivenUploadPartToken> {
        let part_number = PartNumber::try_from(part_number)
            .map_err(|e| ErrorMetadata::bad_request("Invalid part number", e.to_string()))?;
        let ClientDrivenUpload {
            object_key,
            upload_id,
        } = token.try_into()?;
        let s3_key = self.s3_key(&object_key);
        let mut s3_upload = S3Upload::new_client_driven(
            self.client.clone(),
            self.bucket.clone(),
            upload_id.to_string().into(),
            object_key,
            s3_key,
            self.runtime.clone(),
            vec![],
            part_number,
        )?;
        s3_upload.write(part).await?;
        let object_part = s3_upload
            .uploaded_parts
            .pop()
            .context("should have written part")?;
        object_part.try_into()
    }

    async fn finish_client_driven_upload(
        &self,
        token: ClientDrivenUploadToken,
        mut part_tokens: Vec<ClientDrivenUploadPartToken>,
    ) -> anyhow::Result<ObjectKey> {
        if part_tokens.is_empty() {
            // S3 doesn't like multi-part uploads with zero parts, so create
            // an empty part.
            part_tokens.push(self.upload_part(token.clone(), 1, Bytes::new()).await?);
        }
        let ClientDrivenUpload {
            object_key,
            upload_id,
        } = token.try_into()?;
        let s3_key = self.s3_key(&object_key);
        let uploaded_parts: Vec<_> = part_tokens
            .into_iter()
            .map(ObjectPart::try_from)
            .try_collect()?;
        let next_part_number = 1; // unused
        let s3_upload = Box::new(S3Upload::new_client_driven(
            self.client.clone(),
            self.bucket.clone(),
            upload_id.to_string().into(),
            object_key,
            s3_key,
            self.runtime.clone(),
            uploaded_parts,
            next_part_number.try_into()?,
        )?);
        s3_upload.complete().await
    }

    async fn signed_url(&self, key: ObjectKey, expires_in: Duration) -> anyhow::Result<String> {
        let timer = sign_url_timer();
        let s3_key = self.s3_key(&key);
        let presigning_config = PresigningConfig::builder().expires_in(expires_in).build()?;
        let presigned_request = self
            .client
            .get_object()
            .bucket(self.bucket.clone())
            .key(&s3_key.0)
            .presigned(presigning_config)
            .await?;
        timer.finish();
        Ok(presigned_request.uri().to_owned())
    }

    async fn presigned_upload_url(
        &self,
        expires_in: Duration,
    ) -> anyhow::Result<(ObjectKey, String)> {
        let key: ObjectKey = self.runtime.new_uuid_v4().to_string().try_into()?;
        let s3_key = self.s3_key(&key);
        let presigning_config = PresigningConfig::builder().expires_in(expires_in).build()?;
        // TODO(CX-4921): figure out how to add SSE/checksums here
        let presigned_request = self
            .client
            .put_object()
            .bucket(self.bucket.clone())
            .key(&s3_key.0)
            .presigned(presigning_config)
            .await?;
        Ok((key, presigned_request.uri().to_owned()))
    }

    fn cache_key(&self, key: &ObjectKey) -> StorageCacheKey {
        StorageCacheKey::new(self.key_prefix.clone() + key)
    }

    fn fully_qualified_key(&self, key: &ObjectKey) -> FullyQualifiedObjectKey {
        format!("{}/{}{}", self.bucket, self.key_prefix, &**key).into()
    }

    fn test_only_decompose_fully_qualified_key(
        &self,
        _key: FullyQualifiedObjectKey,
    ) -> anyhow::Result<ObjectKey> {
        unimplemented!();
    }

    fn get_small_range(
        &self,
        key: &FullyQualifiedObjectKey,
        bytes_range: std::ops::Range<u64>,
    ) -> BoxFuture<'static, anyhow::Result<StorageGetStream>> {
        if bytes_range.start >= bytes_range.end {
            return async {
                Ok(StorageGetStream {
                    content_length: 0,
                    stream: Box::pin(stream::empty()),
                })
            }
            .boxed();
        }
        self.get_small_range_internal(key, bytes_range)
            .map(|r| Ok(r?.context("No such key")?.0))
            .boxed()
    }

    fn get_small_range_and_total_size(
        &self,
        key: &FullyQualifiedObjectKey,
        bytes_range: std::ops::Range<u64>,
    ) -> Option<BoxFuture<'static, anyhow::Result<Option<(StorageGetStream, u64)>>>> {
        if is_range_prefetch_disabled() {
            return None;
        }
        Some(
            self.get_small_range_internal(key, bytes_range)
                .map(|r| {
                    let Some((stream, content_range)) = r? else {
                        return Ok(None);
                    };
                    // header looks like:
                    //   Content-Range: bytes 0-39/12345678
                    let total_size: u64 = try {
                        content_range
                            .as_deref()?
                            .strip_prefix("bytes ")?
                            .split_once('/')?
                            .1
                            .parse()
                            .ok()?
                    }
                    .with_context(|| {
                        format!(
                            "invalid Content-Range header: {content_range:?}. If your \
                             S3-compatible storage provider doesn't return this header, set \
                             AWS_S3_DISABLE_RANGE_PREFETCH=true"
                        )
                    })?;
                    Ok(Some((stream, total_size)))
                })
                .boxed(),
        )
    }

    async fn get_fq_object_attributes(
        &self,
        key: &FullyQualifiedObjectKey,
    ) -> anyhow::Result<Option<ObjectAttributes>> {
        let (bucket, s3_key) = key
            .as_str()
            .split_once('/')
            .with_context(|| format!("Invalid fully qualified S3 key {key:?}"))?;
        let result: Result<HeadObjectOutput, aws_sdk_s3::error::SdkError<HeadObjectError>> = self
            .client
            .head_object()
            .bucket(bucket)
            .key(s3_key)
            .send()
            .await;
        match result {
            Ok(head_attributes) => {
                let size = head_attributes
                    .content_length
                    .context("Object is missing size")? as u64;
                Ok(Some(ObjectAttributes { size }))
            },
            Err(aws_sdk_s3::error::SdkError::ServiceError(err)) => match err.err() {
                HeadObjectError::NotFound(_) => Ok(None),
                // Other service errors from S3
                _ => Err(err.into_err().into()),
            },
            // Unable to get a response from S3 (e.g. timeout error)
            Err(err) => Err(err.into()),
        }
    }

    fn storage_type_proto(&self) -> pb::searchlight::StorageType {
        let prefix = self.key_prefix.clone();
        let bucket = self.bucket.clone();
        pb::searchlight::StorageType {
            storage_type: Some(StorageType::S3(pb::searchlight::S3Storage {
                prefix,
                bucket,
            })),
        }
    }

    async fn delete_object(&self, key: &ObjectKey) -> anyhow::Result<()> {
        let s3_key = self.s3_key(key);
        self.client
            .delete_object()
            .bucket(self.bucket.clone())
            .key(&s3_key.0)
            .send()
            .await
            .context(format!("Failed to delete object {key:?}"))?;
        Ok(())
    }

    async fn put_object(&self, key: ObjectKey, bytes: Bytes) -> anyhow::Result<()> {
        self.client
            .put_object()
            .bucket(self.bucket.clone())
            .key(&self.s3_key(&key).0)
            .body(ByteStream::from(bytes))
            .set_server_side_encryption(server_side_encryption())
            .set_checksum_algorithm(checksum_algorithm())
            .send()
            .await
            .with_context(|| format!("Failed to put object {key:?}"))?;
        Ok(())
    }

    async fn list_objects(&self, key_prefix: &str) -> anyhow::Result<Vec<ObjectListing>> {
        let s3_prefix = format!("{}{}", self.key_prefix, key_prefix);
        let mut paginator = self
            .client
            .list_objects_v2()
            .bucket(self.bucket.clone())
            .prefix(&s3_prefix)
            .into_paginator()
            .send();

        let mut objects = Vec::new();
        while let Some(page) = paginator.next().await {
            let page =
                page.context(format!("Failed to list objects with prefix {key_prefix:?}"))?;
            for object in page.contents.unwrap_or_default() {
                let Some(s3_key) = object.key.as_ref() else {
                    continue;
                };
                let relative_key = s3_key
                    .strip_prefix(&self.key_prefix)
                    .with_context(|| format!("S3 key {s3_key:?} missing storage prefix"))?;
                let key = ObjectKey::try_from(relative_key)?;
                let last_modified = *object
                    .last_modified()
                    .context("S3 object missing last_modified")?;
                let last_modified = SystemTime::try_from(last_modified)
                    .context("S3 last_modified isn't valid SystemTime")?;
                objects.push(ObjectListing { key, last_modified });
            }
        }
        Ok(objects)
    }
}

impl<RT: Runtime> S3Storage<RT> {
    /// Also returns the raw `Content-Range` response header, which carries
    /// the object's total size.
    fn get_small_range_internal(
        &self,
        key: &FullyQualifiedObjectKey,
        bytes_range: std::ops::Range<u64>,
    ) -> impl Future<Output = anyhow::Result<Option<(StorageGetStream, Option<String>)>>> + use<RT>
    {
        let get_object = self.client.get_object();
        let key = key.clone();
        async move {
            let (bucket, s3_key) = key
                .as_str()
                .split_once('/')
                .with_context(|| format!("Invalid fully qualified S3 key {key:?}"))?;
            anyhow::ensure!(bytes_range.start < bytes_range.end);
            let output = match get_object
                .bucket(bucket)
                .key(s3_key)
                .range(format!(
                    "bytes={}-{}",
                    bytes_range.start,
                    bytes_range.end - 1
                ))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) if e.as_service_error().is_some_and(|e| e.is_no_such_key()) => {
                    return Ok(None)
                },
                Err(e) if e.as_service_error().and_then(|e| e.code()) == Some("InvalidRange") => {
                    anyhow::bail!(InvalidGetRangeError);
                },
                Err(e) => return Err(e.into()),
            };
            let content_range = output.content_range().map(String::from);
            Ok(Some((
                StorageGetStream {
                    content_length: output
                        .content_length()
                        .context("Missing content length for object")?,
                    stream: output.body.into_stream().boxed(),
                },
                content_range,
            )))
        }
    }
}

/// Optional server-side encryption header to configure upload builders, for
/// S3 compatibility with non-AWS services.
fn server_side_encryption() -> Option<ServerSideEncryption> {
    (!is_sse_disabled()).then_some(ServerSideEncryption::Aes256)
}

/// Optional checksum algorithm header to configure upload builders, for S3
/// compatibility with non-AWS services.
fn checksum_algorithm() -> Option<ChecksumAlgorithm> {
    (!are_checksums_disabled()).then_some(ChecksumAlgorithm::Crc32)
}

struct S3Key(String);

pub struct S3Upload<RT: Runtime> {
    client: Client,
    bucket: String,
    upload_id: UploadId,
    key: ObjectKey,
    s3_key: S3Key,
    uploaded_parts: Vec<ObjectPart>,
    next_part_number: Option<PartNumber>,
    /// Initialized to true - set to fault if cleanly completed or cleanly
    /// aborted explicitly. Aborting helps save space by cleaning out
    /// incomplete multipart uploads.
    needs_abort_on_drop: bool,
    runtime: RT,
}

impl<RT: Runtime> S3Upload<RT> {
    async fn new(
        client: Client,
        bucket: String,
        upload_id: UploadId,
        key: ObjectKey,
        s3_key: S3Key,
        runtime: RT,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client,
            bucket,
            upload_id,
            key,
            s3_key,
            uploaded_parts: vec![],
            next_part_number: Some(1.try_into()?),
            needs_abort_on_drop: true,
            runtime,
        })
    }

    fn new_client_driven(
        client: Client,
        bucket: String,
        upload_id: UploadId,
        key: ObjectKey,
        s3_key: S3Key,
        runtime: RT,
        uploaded_parts: Vec<ObjectPart>,
        next_part_number: PartNumber,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client,
            bucket,
            upload_id,
            key,
            s3_key,
            uploaded_parts,
            next_part_number: Some(next_part_number),
            needs_abort_on_drop: false,
            runtime,
        })
    }

    fn next_part_number(&mut self) -> anyhow::Result<PartNumber> {
        let part_number = self
            .next_part_number
            .context("S3 multipart part limit reached")?;
        self.next_part_number = u16::from(part_number)
            .checked_add(1)
            .and_then(|next| PartNumber::try_from(next).ok());
        Ok(part_number)
    }

    fn start_write(&mut self, data: Bytes) -> anyhow::Result<UploadPart> {
        let part_number = self.next_part_number()?;
        crate::metrics::log_aws_s3_part_upload_size_bytes(data.len());

        let builder = self
            .client
            .upload_part()
            .body(ByteStream::from(data))
            .bucket(self.bucket.clone())
            .key(&self.s3_key.0)
            .part_number(Into::<u16>::into(part_number) as i32)
            .upload_id(self.upload_id.to_string())
            .set_checksum_algorithm(checksum_algorithm());

        Ok(UploadPart {
            part_number,
            builder,
        })
    }
}

struct UploadPart {
    part_number: PartNumber,
    builder: UploadPartFluentBuilder,
}

impl UploadPart {
    async fn upload(self, size: u64) -> anyhow::Result<ObjectPart> {
        let output = self.builder.send().await?;
        ObjectPart::new(self.part_number, size, output)
    }
}

#[async_trait]
impl<RT: Runtime> Upload for S3Upload<RT> {
    #[fastrace::trace]
    async fn try_write_parallel<'a>(
        &'a mut self,
        receiver: &mut Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send + 'a>>,
    ) -> anyhow::Result<()> {
        let mut uploaded_parts = receiver
            .map(|result| {
                let size = match &result {
                    Ok(buf) => buf.len() as u64,
                    Err(_) => 0,
                };
                match result.and_then(|buf| self.start_write(buf)) {
                    Ok(upload) => Either::Left(upload.upload(size)),
                    Err(e) => Either::Right(future::err(e)),
                }
            })
            .buffer_unordered(MAXIMUM_PARALLEL_UPLOADS)
            .try_collect::<Vec<_>>()
            .await?;
        self.uploaded_parts.append(&mut uploaded_parts);

        Ok(())
    }

    async fn write(&mut self, data: Bytes) -> anyhow::Result<()> {
        let size = data.len() as u64;
        let upload_part = self.start_write(data)?;
        let object_part = upload_part.upload(size).await?;
        self.uploaded_parts.push(object_part);
        Ok(())
    }

    async fn abort(mut self: Box<Self>) -> anyhow::Result<()> {
        self._abort().await?;
        self.needs_abort_on_drop = false;
        Ok(())
    }

    #[fastrace::trace]
    async fn complete(mut self: Box<Self>) -> anyhow::Result<ObjectKey> {
        let key = self.key.clone();
        let mut completed_parts = Vec::new();
        for part in &self.uploaded_parts {
            let mut builder = CompletedPart::builder()
                .part_number(Into::<u16>::into(part.part_number()) as i32)
                .e_tag(part.etag());

            if !are_checksums_disabled() {
                builder = builder.checksum_crc32(part.checksum());
            }

            let part = builder.build();
            completed_parts.push(part);
        }
        // parallel_writes will write out of order.
        completed_parts.sort_by_key(|part| part.part_number());
        let completed_multipart_upload = CompletedMultipartUpload::builder()
            .set_parts(Some(completed_parts))
            .build();
        self.client
            .complete_multipart_upload()
            .bucket(self.bucket.clone())
            .key(&self.s3_key.0)
            .upload_id(self.upload_id.to_string())
            .multipart_upload(completed_multipart_upload)
            .send()
            .await?;
        self.needs_abort_on_drop = false;
        Ok(key)
    }
}

impl<RT: Runtime> S3Upload<RT> {
    fn _abort(&mut self) -> impl Future<Output = anyhow::Result<()>> + use<RT> {
        let client = self.client.clone();
        let bucket = self.bucket.clone();
        let upload_id = self.upload_id.to_string();
        let s3_key = self.s3_key.0.clone();
        async move {
            client
                .abort_multipart_upload()
                .bucket(bucket)
                .upload_id(upload_id)
                .key(s3_key)
                .send()
                .await?;
            Ok(())
        }
    }
}

impl<RT: Runtime> Drop for S3Upload<RT> {
    fn drop(&mut self) {
        if self.needs_abort_on_drop {
            let fut = self._abort();
            self.runtime
                .spawn_background("abort_multipart_upload", async move {
                    if let Err(e) = fut.await {
                        // abort-multipart-upload is idempotent. It has the following properties.
                        //
                        // abort after a successful abort - succeeds
                        // abort after a successful complete - succeeds
                        // complete after a successful abort - fails with a descriptive error.
                        report_error(
                            &mut anyhow::anyhow!(e)
                                .context("Couldn't async abort multipart upload"),
                        )
                        .await;
                    }
                });
        }
    }
}

pub fn s3_bucket_name(use_case: &StorageUseCase) -> anyhow::Result<String> {
    let env_var_name = format!("S3_STORAGE_{}_BUCKET", use_case.to_string().to_uppercase());
    env::var(&env_var_name).context(format!(
        "{env_var_name} env variable is required when using S3 storage"
    ))
}

// Test below only works if you have AWS environment variables set