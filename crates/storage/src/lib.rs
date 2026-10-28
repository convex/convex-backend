#![feature(try_blocks_heterogeneous)]
#![feature(coroutines)]

use std::{
    cmp,
    env,
    fmt::{
        Debug,
        Display,
    },
    fs::{
        self,
        File,
        OpenOptions,
    },
    io::{
        self,
        Read,
        Seek,
        SeekFrom,
        Write,
    },
    mem,
    ops::Range,
    path::{
        Path,
        PathBuf,
    },
    pin::Pin,
    sync::Arc,
    task::{
        Context,
        Poll,
    },
    time::{
        Duration,
        SystemTime,
    },
};

use anyhow::Context as _;
use async_trait::async_trait;
use bytes::Bytes;
use common::{
    runtime::Runtime,
    try_anyhow,
    types::{
        FullyQualifiedObjectKey,
        ObjectKey,
    },
};
use futures::{
    future::BoxFuture,
    io::{
        Error as IoError,
        ErrorKind as IoErrorKind,
    },
    pin_mut,
    ready,
    select_biased,
    stream::{
        self,
        BoxStream,
        IntoAsyncRead,
    },
    FutureExt,
    Stream,
    StreamExt,
    TryStreamExt,
};
use futures_async_stream::try_stream;
use serde_json::{
    json,
    Value as JsonValue,
};
use tempfile::TempDir;
use tokio::{
    io::AsyncWrite,
    sync::mpsc,
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::{
    io::StreamReader,
    sync::PollSender,
};
use url::Url;
use value::sha256::{
    Sha256,
    Sha256Digest,
};

pub const LOCAL_DIR_MIN_PART_SIZE: usize = 5 * (1 << 20);
pub const LOCAL_DIR_MAX_PART_SIZE: usize = 8 * (1 << 30);
pub const MAX_NUM_PARTS: usize = 10000;
pub const MAXIMUM_PARALLEL_UPLOADS: usize = 8;

#[derive(Clone, Eq, PartialEq, PartialOrd, Ord, derive_more::Display)]
pub struct UploadId(String);

impl From<String> for UploadId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

pub struct StorageGetStream {
    pub content_length: i64,
    pub stream: BoxStream<'static, io::Result<Bytes>>,
}

impl StorageGetStream {
    pub fn into_reader(self) -> IntoAsyncRead<BoxStream<'static, io::Result<Bytes>>> {
        self.stream.into_async_read()
    }

    pub fn into_tokio_reader(self) -> StreamReader<BoxStream<'static, io::Result<Bytes>>, Bytes> {
        StreamReader::new(self.stream)
    }
}

#[derive(Clone, Hash, PartialEq, Eq, Debug, derive_more::Constructor, derive_more::Into)]
pub struct StorageCacheKey(String);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClientDrivenUploadToken(pub String);
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClientDrivenUploadPartToken(pub String);

pub const DOWNLOAD_CHUNK_SIZE: u64 = 8 * (1 << 20);
pub const MAX_CONCURRENT_CHUNK_DOWNLOADS: usize = 16;

#[async_trait]
pub trait Storage: Send + Sync + Debug {
    /// Start a new upload for generated data where no authoritative hash is
    /// present to be verified and no hash is desired.
    ///
    /// Storage may choose to implement some checksuming strategy for uploads,
    /// but it's opaque to callers.
    async fn start_upload(&self) -> anyhow::Result<Box<BufferedUpload>>;

    /// A multi-part upload where the client uploads parts one at a time.
    async fn start_client_driven_upload(&self) -> anyhow::Result<ClientDrivenUploadToken>;
    async fn upload_part(
        &self,
        token: ClientDrivenUploadToken,
        part_number: u16,
        part: Bytes,
    ) -> anyhow::Result<ClientDrivenUploadPartToken>;
    async fn finish_client_driven_upload(
        &self,
        token: ClientDrivenUploadToken,
        part_tokens: Vec<ClientDrivenUploadPartToken>,
    ) -> anyhow::Result<ObjectKey>;

    /// Gets a URL that can be used to fetch an object.
    async fn signed_url(&self, key: ObjectKey, expires_in: Duration) -> anyhow::Result<String>;
    /// Creates a presigned url for uploading an object.
    async fn presigned_upload_url(
        &self,
        expires_in: Duration,
    ) -> anyhow::Result<(ObjectKey, String)>;
    async fn get_object_attributes(
        &self,
        key: &ObjectKey,
    ) -> anyhow::Result<Option<ObjectAttributes>> {
        self.get_fq_object_attributes(&self.fully_qualified_key(key))
            .await
    }
    // TODO: FullyQualifiedObjectKey is an abstraction violation as it possibly
    // reads from another `Storage` instance; get rid of it
    async fn get_fq_object_attributes(
        &self,
        key: &FullyQualifiedObjectKey,
    ) -> anyhow::Result<Option<ObjectAttributes>>;
    /// Not intended to be called directly.
    /// Use get_range() or get() instead.
    fn get_small_range(
        &self,
        key: &FullyQualifiedObjectKey,
        bytes_range: Range<u64>,
    ) -> BoxFuture<'static, anyhow::Result<StorageGetStream>>;
    /// Returns None immediately if the caller should use
    /// `get_fq_object_attributes` instead. The returned future yields None
    /// if the object doesn't exist, and yields an InvalidGetRangeError if the
    /// requested range starts after the object's size.
    fn get_small_range_and_total_size(
        &self,
        _key: &FullyQualifiedObjectKey,
        _bytes_range: Range<u64>,
    ) -> Option<BoxFuture<'static, anyhow::Result<Option<(StorageGetStream, u64)>>>> {
        None
    }
    fn storage_type_proto(&self) -> pb::searchlight::StorageType;
    /// Return a cache key suitable for the given ObjectKey, even in
    /// a multi-tenant cache.
    fn cache_key(&self, key: &ObjectKey) -> StorageCacheKey;
    /// Return a fully qualified key, including info on bucket name
    /// and suitable for access in multi-tenant scenario
    fn fully_qualified_key(&self, key: &ObjectKey) -> FullyQualifiedObjectKey;
    fn test_only_decompose_fully_qualified_key(
        &self,
        key: FullyQualifiedObjectKey,
    ) -> anyhow::Result<ObjectKey>;
    /// Delete the given object.
    async fn delete_object(&self, key: &ObjectKey) -> anyhow::Result<()>;

    /// Upload `bytes` to `key`, overwriting any object already at that key.
    ///
    /// This uploads the object in a single request. Objects above a size limit
    /// can fail. Use `start_upload` for large objects.
    async fn put_object(&self, key: ObjectKey, bytes: Bytes) -> anyhow::Result<()>;

    /// List all objects whose key begins with `key_prefix`.
    async fn list_objects(&self, key_prefix: &str) -> anyhow::Result<Vec<ObjectListing>>;
}

pub struct ObjectAttributes {
    pub size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectListing {
    pub key: ObjectKey,
    pub last_modified: SystemTime,
}

#[derive(thiserror::Error, Debug)]
#[error("Requested get range exceeds object's size")]
pub struct InvalidGetRangeError;

pub struct SizeAndHash {
    sha256: Sha256,
    size: usize,
    err: Option<anyhow::Error>,
}

impl SizeAndHash {
    fn new() -> Self {
        Self {
            sha256: Sha256::new(),
            size: 0,
            err: None,
        }
    }

    fn update(&mut self, result: &anyhow::Result<Bytes>) {
        if self.err.is_some() {
            return;
        }
        match result {
            Ok(bytes) => {
                self.sha256.update(bytes);
                self.size += bytes.len();
            },
            Err(e) => {
                self.err = Some(anyhow::anyhow!(
                    "At least one byte value was a failure {e:?}"
                ))
            },
        }
    }

    fn finish(self) -> anyhow::Result<(usize, Sha256Digest)> {
        if let Some(err) = self.err {
            anyhow::bail!(err);
        }
        Ok((self.size, self.sha256.finalize()))
    }
}

#[async_trait]
pub trait Upload: Send + Sync {
    /// Writes data to specified object. The `data` argument must be at most
    /// 5GB, and if it isn't the final chunk of the file, it must be at
    /// least 5MB. There may be at most 10000 writes in a single upload.
    /// Must be followed by `complete` to complete the object.
    async fn write(&mut self, data: Bytes) -> anyhow::Result<()>;

    /// Writes data to the specified object in parts where multiple parts may be
    /// uploaded in parallel. The stream must contain the bytes in the same
    /// order as they are present in the file.
    /// Must be follwed by 'complete' to complete the object.
    ///
    /// See UploadExt for some easier to use variants or if you need to verify
    /// a checksum.
    async fn try_write_parallel<'a>(
        &'a mut self,
        stream: &mut Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send + 'a>>,
    ) -> anyhow::Result<()>;

    /// Abort the multipart upload. Must call either abort or complete to avoid
    /// being charged for incomplete objects.
    async fn abort(self: Box<Self>) -> anyhow::Result<()>;

    /// Completes the multipart object.
    async fn complete(self: Box<Self>) -> anyhow::Result<ObjectKey>;
}

/// Helper functions for working with uploads for functions that have generic
/// types that would otherwise prevent Upload from being "object safe":
/// https://doc.rust-lang.org/reference/items/traits.html#object-safety.
#[async_trait]
pub trait UploadExt {
    /// Similar to try_write_parallel, but without support for per item Results
    async fn write_parallel(
        &mut self,
        stream: impl Stream<Item = Bytes> + Send + 'static,
    ) -> anyhow::Result<()>;

    /// Calculates a linear sha256 digest from the stream, then uploads the
    /// parts in parallel.
    async fn try_write_parallel_and_hash(
        &mut self,
        stream: impl Stream<Item = anyhow::Result<Bytes>> + Send,
    ) -> anyhow::Result<(usize, Sha256Digest)>;
}

#[async_trait]
impl<T: Upload> UploadExt for T {
    async fn write_parallel(
        &mut self,
        stream: impl Stream<Item = Bytes> + Send,
    ) -> anyhow::Result<()> {
        let mut boxed = stream.map(Ok).boxed();
        self.try_write_parallel(&mut boxed).await
    }

    async fn try_write_parallel_and_hash(
        &mut self,
        stream: impl Stream<Item = anyhow::Result<Bytes>> + Send,
    ) -> anyhow::Result<(usize, Sha256Digest)> {
        let mut size_and_hash = SizeAndHash::new();
        let mut boxed = stream
            .map(|value| {
                size_and_hash.update(&value);
                value
            })
            .boxed();
        self.try_write_parallel(&mut boxed).await?;
        drop(boxed);
        size_and_hash.finish()
    }
}

#[must_use]
pub struct BufferedUpload {
    upload: Box<dyn Upload>,
    buffer: Vec<u8>,
    max_intermediate_part_size: usize,
    target_intermediate_part_size: usize,
}

impl BufferedUpload {
    pub fn new(
        upload: impl Upload + 'static,
        min_intermediate_part_size: usize,
        max_intermediate_part_size: usize,
    ) -> Self {
        let buffer = Vec::with_capacity(min_intermediate_part_size);
        Self {
            upload: Box::new(upload),
            buffer,
            max_intermediate_part_size,
            target_intermediate_part_size: min_intermediate_part_size,
        }
    }

    fn update_buffer_and_get_next(&mut self, data: Bytes) -> Option<Bytes> {
        Self::_update_buffer_and_get_next(
            &mut self.buffer,
            &mut self.target_intermediate_part_size,
            self.max_intermediate_part_size,
            data,
        )
    }

    // Hack around wanting to use this when `upload` is borrowed. Rust can't
    // otherwise tell that we're using different and not previously borrowed
    // parts of self.
    fn _update_buffer_and_get_next(
        buffer: &mut Vec<u8>,
        target_intermediate_part_size: &mut usize,
        max_intermediate_part_size: usize,
        data: Bytes,
    ) -> Option<Bytes> {
        // Fast path, ship the buffer without copying.
        if buffer.is_empty()
            && data.len() >= *target_intermediate_part_size
            && data.len() <= max_intermediate_part_size
        {
            *target_intermediate_part_size = cmp::min(
                *target_intermediate_part_size * 2,
                max_intermediate_part_size,
            );
            return Some(data);
        }
        buffer.extend_from_slice(&data);
        if buffer.len() >= *target_intermediate_part_size {
            *target_intermediate_part_size = cmp::min(
                *target_intermediate_part_size * 2,
                max_intermediate_part_size,
            );
            let ready = if buffer.len() > max_intermediate_part_size {
                let remainder = buffer.split_off(max_intermediate_part_size);
                mem::replace(buffer, remainder)
            } else {
                mem::replace(buffer, Vec::with_capacity(*target_intermediate_part_size))
            };
            Some(ready.into())
        } else {
            None
        }
    }
}

#[async_trait]
impl Upload for BufferedUpload {
    async fn write(&mut self, data: Bytes) -> anyhow::Result<()> {
        if let Some(buf) = self.update_buffer_and_get_next(data) {
            self.upload.write(buf).await?;
        }
        Ok(())
    }

    async fn try_write_parallel<'a>(
        &'a mut self,
        stream: &mut Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send + 'a>>,
    ) -> anyhow::Result<()> {
        // Try to keep some buffered data ready in the channel, but not too much.
        let (tx, rx) = mpsc::channel(MAXIMUM_PARALLEL_UPLOADS / 2);

        let mut boxed_rx = ReceiverStream::new(rx).boxed();
        let mut upload = self.upload.try_write_parallel(&mut boxed_rx).fuse();

        let buffer_bytes = async {
            let result: anyhow::Result<()> = try_anyhow!({
                while let Some(result) = stream.next().await {
                    match result {
                        Err(e) => tx.send(Err(e)).await?,
                        Ok(buf) => {
                            if let Some(buf) = Self::_update_buffer_and_get_next(
                                &mut self.buffer,
                                &mut self.target_intermediate_part_size,
                                self.max_intermediate_part_size,
                                buf,
                            ) {
                                tx.send(Ok(buf)).await?;
                            }
                        },
                    }
                }
            });
            drop(tx);
            result
        }
        .fuse();
        pin_mut!(buffer_bytes);

        // We do loop, clippy is confused by select_biased!
        #[allow(clippy::never_loop)]
        loop {
            select_biased! {
                upload_result = upload => {
                    return upload_result;
                }
                bytes_result = buffer_bytes => {
                    bytes_result?;
                }
            }
        }
    }

    async fn abort(mut self: Box<Self>) -> anyhow::Result<()> {
        self.upload.abort().await
    }

    async fn complete(mut self: Box<Self>) -> anyhow::Result<ObjectKey> {
        let Self {
            buffer: ready,
            mut upload,
            ..
        } = *self;
        upload.write(ready.into()).await?;
        upload.complete().await
    }
}

pub struct ChannelWriter {
    parts: PollSender<Bytes>,
    current_part: Vec<u8>,
    part_size: usize,
}

impl ChannelWriter {
    pub fn new(sender: mpsc::Sender<Bytes>, part_size: usize) -> Self {
        Self {
            parts: PollSender::new(sender),
            current_part: Vec::with_capacity(part_size),
            part_size,
        }
    }
}

impl AsyncWrite for ChannelWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, IoError>> {
        let self_ = self.get_mut();
        loop {
            if self_.current_part.len() < self_.part_size {
                let n = cmp::min(buf.len(), self_.part_size - self_.current_part.len());
                self_.current_part.extend_from_slice(&buf[..n]);
                return Poll::Ready(Ok(n));
            }
            ready!(self_
                .parts
                .poll_reserve(cx)
                .map_err(|_| IoError::new(IoErrorKind::BrokenPipe, "Channel closed")))?;
            let next_buf = Vec::with_capacity(self_.part_size);
            let buf = mem::replace(&mut self_.current_part, next_buf);
            self_
                .parts
                .send_item(buf.into())
                .map_err(|_| IoError::new(IoErrorKind::BrokenPipe, "Channel closed"))?;
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        // We want to control the part size, so don't do anything on flush.
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        let self_ = self.get_mut();
        if !self_.current_part.is_empty() {
            ready!(self_
                .parts
                .poll_reserve(cx)
                .map_err(|_| IoError::new(IoErrorKind::BrokenPipe, "Channel closed")))?;
            let next_buf = vec![];
            let buf = mem::replace(&mut self_.current_part, next_buf);
            self_
                .parts
                .send_item(buf.into())
                .map_err(|_| IoError::new(IoErrorKind::BrokenPipe, "Channel closed"))?;
        }
        Poll::Ready(Ok(()))
    }
}

#[async_trait]
pub trait StorageExt {
    /// Gets a previously stored object.
    async fn get(&self, key: &ObjectKey) -> anyhow::Result<Option<StorageGetStream>>;
    /// Gets a stream for a range of a previously stored object.
    async fn get_range(
        &self,
        key: &ObjectKey,
        bytes_range: (std::ops::Bound<u64>, std::ops::Bound<u64>),
    ) -> anyhow::Result<Option<StorageGetStream>>;

    // Equivalents for the above taking fully qualified objects
    // TODO: FullyQualifiedObjectKey is an abstraction violation as it possibly
    // reads from another `Storage` instance; get rid of it
    async fn get_fq_object(
        &self,
        object_key: &FullyQualifiedObjectKey,
    ) -> anyhow::Result<Option<StorageGetStream>>;
    async fn get_fq_object_range(
        &self,
        key: &FullyQualifiedObjectKey,
        bytes_range: (std::ops::Bound<u64>, std::ops::Bound<u64>),
    ) -> anyhow::Result<Option<StorageGetStream>>;
    /// Requires that `byte_range` be in-bounds for the object
    fn get_fq_object_exact_range(
        &self,
        key: &FullyQualifiedObjectKey,
        byte_range: Range<u64>,
    ) -> StorageGetStream;

    // Implementation detail
    async fn get_small_range_with_retries(
        &self,
        key: &FullyQualifiedObjectKey,
        small_byte_range: Range<u64>,
    ) -> anyhow::Result<StorageGetStream>;
}

#[async_trait]
impl StorageExt for Arc<dyn Storage> {
    async fn get(&self, key: &ObjectKey) -> anyhow::Result<Option<StorageGetStream>> {
        self.get_range(
            key,
            (std::ops::Bound::Unbounded, std::ops::Bound::Unbounded),
        )
        .await
    }

    async fn get_range(
        &self,
        key: &ObjectKey,
        bytes_range: (std::ops::Bound<u64>, std::ops::Bound<u64>),
    ) -> anyhow::Result<Option<StorageGetStream>> {
        self.get_fq_object_range(&self.fully_qualified_key(key), bytes_range)
            .await
    }

    async fn get_fq_object(
        &self,
        key: &FullyQualifiedObjectKey,
    ) -> anyhow::Result<Option<StorageGetStream>> {
        self.get_fq_object_range(
            key,
            (std::ops::Bound::Unbounded, std::ops::Bound::Unbounded),
        )
        .await
    }

    async fn get_fq_object_range(
        &self,
        key: &FullyQualifiedObjectKey,
        bytes_range: (std::ops::Bound<u64>, std::ops::Bound<u64>),
    ) -> anyhow::Result<Option<StorageGetStream>> {
        let start_byte = match bytes_range.0 {
            std::ops::Bound::Included(bound) => bound,
            std::ops::Bound::Excluded(bound) => bound.saturating_add(1),
            std::ops::Bound::Unbounded => 0,
        };
        let end_byte_bound = match bytes_range.1 {
            std::ops::Bound::Included(bound) => bound.saturating_add(1),
            std::ops::Bound::Excluded(bound) => bound,
            std::ops::Bound::Unbounded => u64::MAX, // this will be corrected later
        };
        let prefetch_end = start_byte
            .saturating_add(DOWNLOAD_CHUNK_SIZE)
            .min(end_byte_bound);
        let (first_chunk, total_size) = if start_byte < prefetch_end
            && let Some(f) = self.get_small_range_and_total_size(key, start_byte..prefetch_end)
        {
            // Fetch the first chunk and total size at the same time.
            // This is faster for small files than calling `get_fq_object_attributes`.
            match f.await {
                Ok(Some((first_chunk, total_size))) => (
                    Some(StorageGetStream {
                        content_length: first_chunk.content_length,
                        // wrap the returned stream in a retrying one
                        stream: Box::pin(stream_object_with_retries(
                            first_chunk.stream,
                            self.clone(),
                            key.clone(),
                            start_byte..(start_byte + first_chunk.content_length as u64),
                            STORAGE_GET_RETRIES,
                        )),
                    }),
                    total_size,
                ),
                Ok(None) => return Ok(None),
                // This function's contract is to return only the matching bytes, which is nothing
                // in this case
                Err(e) if e.is::<InvalidGetRangeError>() => {
                    return Ok(Some(StorageGetStream {
                        content_length: 0,
                        stream: Box::pin(stream::empty()),
                    }))
                },
                Err(e) => return Err(e),
            }
        } else {
            let Some(attributes) = self.get_fq_object_attributes(key).await? else {
                return Ok(None);
            };
            (None, attributes.size)
        };
        let start_byte = cmp::min(
            start_byte
                + first_chunk
                    .as_ref()
                    .map_or(0, |chunk| chunk.content_length as u64),
            total_size,
        );
        let end_byte_bound = cmp::min(end_byte_bound, total_size);
        let remaining_bytes = self.get_fq_object_exact_range(key, start_byte..end_byte_bound);
        if let Some(first_chunk) = first_chunk {
            Ok(Some(StorageGetStream {
                content_length: first_chunk.content_length + remaining_bytes.content_length,
                stream: first_chunk.stream.chain(remaining_bytes.stream).boxed(),
            }))
        } else {
            Ok(Some(remaining_bytes))
        }
    }

    fn get_fq_object_exact_range(
        &self,
        key: &FullyQualifiedObjectKey,
        Range {
            start: start_byte,
            end: end_byte_bound,
        }: Range<u64>,
    ) -> StorageGetStream {
        if start_byte >= end_byte_bound {
            return StorageGetStream {
                content_length: 0,
                stream: Box::pin(stream::empty()),
            };
        }
        let num_chunks = (end_byte_bound - start_byte).div_ceil(DOWNLOAD_CHUNK_SIZE);
        // A list of futures, each of which resolves to a stream.
        let mut chunk_futures = vec![];
        for idx in 0..num_chunks {
            let chunk_start = start_byte + DOWNLOAD_CHUNK_SIZE * idx;
            let chunk_end = if idx == num_chunks - 1 {
                end_byte_bound
            } else {
                start_byte + DOWNLOAD_CHUNK_SIZE * (idx + 1)
            };
            let self_ = self.clone();
            let key_ = key.clone();
            let stream_fut = async move {
                self_
                    .get_small_range_with_retries(&key_, chunk_start..chunk_end)
                    .await
                    .map_err(
                        // Mapping everything to `io::ErrorKind::Other` feels bad, but it's what
                        // the AWS library does internally.
                        io::Error::other,
                    )
                    .map(|storage_get_stream| storage_get_stream.stream)
            };
            chunk_futures.push(stream_fut);
        }
        // Convert the list of futures into a stream, where each item is the resolved
        // output of the future (i.e. a `ByteStream`)
        let byte_stream = futures::stream::iter(chunk_futures)
            // Limit the concurrency of the chunk downloads
            .buffered(MAX_CONCURRENT_CHUNK_DOWNLOADS)
            // Flatten the `Stream<Item = io::Result<Stream<Item = io::Result<Bytes>>>>` into a single `Stream<Item = io::Result<Bytes>>`
            .try_flatten();
        StorageGetStream {
            content_length: (end_byte_bound - start_byte) as i64,
            stream: Box::pin(byte_stream),
        }
    }

    #[fastrace::trace]
    async fn get_small_range_with_retries(
        &self,
        key: &FullyQualifiedObjectKey,
        small_byte_range: Range<u64>,
    ) -> anyhow::Result<StorageGetStream> {
        let output = self.get_small_range(key, small_byte_range.clone()).await?;
        let content_length = output.content_length;
        let initial_stream = output.stream;
        Ok(StorageGetStream {
            content_length,
            stream: Box::pin(stream_object_with_retries(
                initial_stream,
                self.clone(),
                key.clone(),
                small_byte_range,
                STORAGE_GET_RETRIES,
            )),
        })
    }
}

const STORAGE_GET_RETRIES: usize = 5;

#[allow(clippy::blocks_in_conditions)]
#[try_stream(ok = Bytes, error = io::Error)]
async fn stream_object_with_retries(
    mut stream: BoxStream<'static, io::Result<Bytes>>,
    storage: Arc<dyn Storage>,
    key: FullyQualifiedObjectKey,
    small_byte_range: Range<u64>,
    mut retries_remaining: usize,
) {
    let mut bytes_yielded = 0;
    loop {
        match stream.try_next().await {
            Ok(Some(chunk)) => {
                bytes_yielded += chunk.len();
                yield chunk;
                if small_byte_range.start + bytes_yielded as u64 >= small_byte_range.end {
                    // In case there's a later error, we don't want to retry and fetch zero bytes,
                    // so just end here.
                    return Ok(());
                }
            },
            Ok(None) => return Ok(()),
            Err(e) if retries_remaining == 0 => {
                return Err(e);
            },
            Err(e) => {
                tracing::warn!(
                    ?key,
                    retries_remaining,
                    bytes_yielded,
                    error = ?e,
                    "Retrying storage download after stream error"
                );
                let new_range =
                    (small_byte_range.start + bytes_yielded as u64)..small_byte_range.end;
                let output = storage
                    .get_small_range(&key, new_range)
                    .await
                    .map_err(io::Error::other)?;
                stream = output.stream;
                retries_remaining -= 1;
            },
        }
    }
}

#[derive(Clone)]
pub struct LocalDirStorage<RT: Runtime> {
    rt: RT,
    dir: PathBuf,
    _temp_dir: Option<Arc<TempDir>>,
}

impl<RT: Runtime> std::fmt::Debug for LocalDirStorage<RT> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalDirStorage")
            .field("dir", &self.dir)
            .finish()
    }
}

impl<RT: Runtime> LocalDirStorage<RT> {
    // Creates local storage using a temporary directory. The directory
    // is deleted when the object is dropped.
    pub fn new(rt: RT) -> anyhow::Result<Self> {
        let temp_dir = TempDir::new()?;
        let storage = Self {
            rt,
            dir: temp_dir.path().to_owned(),
            _temp_dir: Some(Arc::new(temp_dir)),
        };
        Ok(storage)
    }

    /// Create storage at the provided directory
    pub fn new_at_path(rt: RT, dir: PathBuf) -> anyhow::Result<Self> {
        let dir = if dir.is_absolute() {
            dir
        } else {
            env::current_dir()?.join(dir)
        };
        fs::create_dir_all(&dir)?;
        let storage = Self {
            rt,
            dir,
            _temp_dir: None,
        };
        Ok(storage)
    }

    /// Returns the path to the storage directory.
    pub fn path(&self) -> &PathBuf {
        &self.dir
    }

    fn filename_for_key(&self, key: ObjectKey) -> String {
        String::from(key) + ".blob"
    }

    pub fn for_use_case(rt: RT, dir: &str, use_case: StorageUseCase) -> anyhow::Result<Self> {
        let use_case_str = use_case.to_string();
        anyhow::ensure!(!dir.is_empty());
        let storage = LocalDirStorage::new_at_path(rt, PathBuf::from(dir).join(use_case_str))?;
        Ok(storage)
    }
}

struct ClientDrivenUpload {
    object_key: ObjectKey,
    filepath: PathBuf,
}

impl TryFrom<ClientDrivenUpload> for ClientDrivenUploadToken {
    type Error = anyhow::Error;

    fn try_from(value: ClientDrivenUpload) -> Result<Self, Self::Error> {
        let v = json!({
            "objectKey": value.object_key.to_string(),
            "filepath": value.filepath.to_str(),
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
        let filepath = v
            .get("filepath")
            .context("missing filepath")?
            .as_str()
            .context("filepath should be str")?
            .parse()?;
        Ok(Self {
            object_key,
            filepath,
        })
    }
}

/// Removes the file backing a `LocalDirStorage` object. An already-absent file
/// is a success, matching S3 DeleteObject semantics. A missing storage root is
/// an error: treating it as success would let the caller record a deletion
/// that never durably happened.
fn remove_local_object(storage_root: &Path, path: &Path) -> anyhow::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound && storage_root.exists() => Ok(()),
        Err(e) => Err(e).with_context(|| format!("Failed to delete {}", path.display())),
    }
}

#[async_trait]
impl<RT: Runtime> Storage for LocalDirStorage<RT> {
    async fn start_upload(&self) -> anyhow::Result<Box<BufferedUpload>> {
        let object_key: ObjectKey = self.rt.new_uuid_v4().to_string().try_into()?;
        let key = self.filename_for_key(object_key.clone());
        let filepath = self.dir.join(key);

        // The filename constraints on the local file system are a bit stricter than S3,
        // so these might fail. If it fails, let's think about what kinds of paths we're
        // passing in and figure out if we want to expand LocalDirStorage, or constrain
        // the keys.
        //
        // Note that "/" (forward slash) is allowed in the key and is reinterpreted onto
        // the local file system as a directory.
        fs::create_dir_all(filepath.parent().expect("Must have parent")).context(
            "LocalDirStorage file creation failed. Perhaps the storage object key isn't valid?",
        )?;
        let file = File::create(filepath).context(
            "LocalDirStorage file creation failed. Perhaps the storage object key isn't valid?",
        )?;

        let upload = LocalDirUpload {
            object_key,
            file: Some(file),
            num_parts: 0,
        };
        let upload = BufferedUpload::new(upload, LOCAL_DIR_MIN_PART_SIZE, LOCAL_DIR_MAX_PART_SIZE);
        Ok(Box::new(upload))
    }

    async fn start_client_driven_upload(&self) -> anyhow::Result<ClientDrivenUploadToken> {
        let object_key: ObjectKey = self.rt.new_uuid_v4().to_string().try_into()?;
        let key = self.filename_for_key(object_key.clone());
        let filepath = self.dir.join(key);

        // The filename constraints on the local file system are a bit stricter than S3,
        // so these might fail. If it fails, let's think about what kinds of paths we're
        // passing in and figure out if we want to expand LocalDirStorage, or constrain
        // the keys.
        //
        // Note that "/" (forward slash) is allowed in the key and is reinterpreted onto
        // the local file system as a directory.
        fs::create_dir_all(filepath.parent().expect("Must have parent")).context(
            "LocalDirStorage file creation failed. Perhaps the storage object key isn't valid?",
        )?;
        let _file = File::create(filepath.clone()).context(
            "LocalDirStorage file creation failed. Perhaps the storage object key isn't valid?",
        )?;

        ClientDrivenUpload {
            object_key,
            filepath,
        }
        .try_into()
    }

    async fn upload_part(
        &self,
        token: ClientDrivenUploadToken,
        _part_number: u16,
        part: Bytes,
    ) -> anyhow::Result<ClientDrivenUploadPartToken> {
        let ClientDrivenUpload {
            object_key,
            filepath,
        } = token.try_into()?;
        let file = OpenOptions::new().append(true).open(filepath)?;
        let mut upload = LocalDirUpload {
            object_key,
            file: Some(file),
            num_parts: 0, // unused
        };
        upload.write(part).await?;
        Ok(ClientDrivenUploadPartToken(String::new()))
    }

    async fn finish_client_driven_upload(
        &self,
        token: ClientDrivenUploadToken,
        _part_tokens: Vec<ClientDrivenUploadPartToken>,
    ) -> anyhow::Result<ObjectKey> {
        let ClientDrivenUpload {
            object_key,
            filepath: _,
        } = token.try_into()?;
        Ok(object_key)
    }

    async fn signed_url(&self, key: ObjectKey, _expires_in: Duration) -> anyhow::Result<String> {
        let key = self.filename_for_key(key);
        let path = self.dir.join(key);
        let url = Url::from_file_path(&path)
            .map_err(|()| anyhow::anyhow!("Dir isn't valid UTF8: {:?}", self.dir))?;
        Ok(url.into())
    }

    async fn presigned_upload_url(
        &self,
        expires_in: Duration,
    ) -> anyhow::Result<(ObjectKey, String)> {
        let object_key: ObjectKey = self.rt.new_uuid_v4().to_string().try_into()?;
        Ok((
            object_key.clone(),
            self.signed_url(object_key, expires_in).await?,
        ))
    }

    fn cache_key(&self, key: &ObjectKey) -> StorageCacheKey {
        let key = self.filename_for_key(key.clone());
        let path = self.dir.join(key);
        StorageCacheKey(path.to_string_lossy().to_string())
    }

    fn fully_qualified_key(&self, key: &ObjectKey) -> FullyQualifiedObjectKey {
        let key = self.filename_for_key(key.clone());
        let path = self.dir.join(key);
        path.to_string_lossy().to_string().into()
    }

    fn test_only_decompose_fully_qualified_key(
        &self,
        key: FullyQualifiedObjectKey,
    ) -> anyhow::Result<ObjectKey> {
        let key: String = key.into();
        let path = Path::new(&key);
        let remaining = path.strip_prefix(&self.dir)?.to_string_lossy();
        remaining
            .strip_suffix(".blob")
            .context("Doesn't end with .blob")?
            .try_into()
    }

    fn get_small_range(
        &self,
        key: &FullyQualifiedObjectKey,
        bytes_range: Range<u64>,
    ) -> BoxFuture<'static, anyhow::Result<StorageGetStream>> {
        let path = Path::new(key.as_str()).to_owned();
        async move {
            let mut buf = vec![0; (bytes_range.end - bytes_range.start) as usize];
            let mut file = File::open(path.clone()).context(format!(
                "Local dir storage couldn't open {}",
                path.display()
            ))?;
            file.seek(SeekFrom::Start(bytes_range.start))?;
            file.read_exact(&mut buf)?;
            Ok(StorageGetStream {
                content_length: (bytes_range.end - bytes_range.start) as i64,
                stream: stream::once(async move { Ok(buf.into()) }).boxed(),
            })
        }
        .boxed()
    }

    async fn get_fq_object_attributes(
        &self,
        key: &FullyQualifiedObjectKey,
    ) -> anyhow::Result<Option<ObjectAttributes>> {
        let path = Path::new(key.as_str());
        let mut buf = vec![];
        let result = File::open(path);
        if result.is_err() {
            return Ok(None);
        }
        let mut file = result.unwrap();
        file.read_to_end(&mut buf)?;
        Ok(Some(ObjectAttributes {
            size: buf.len() as u64,
        }))
    }

    fn storage_type_proto(&self) -> pb::searchlight::StorageType {
        pb::searchlight::StorageType {
            storage_type: Some(pb::searchlight::storage_type::StorageType::Local(
                pb::searchlight::LocalStorage {
                    path: self.dir.to_string_lossy().into_owned(),
                },
            )),
        }
    }

    async fn delete_object(&self, key: &ObjectKey) -> anyhow::Result<()> {
        let key = self.filename_for_key(key.clone());
        let path = self.dir.join(key);
        remove_local_object(&self.dir, &path)
    }

    async fn put_object(&self, key: ObjectKey, bytes: Bytes) -> anyhow::Result<()> {
        let filename = self.filename_for_key(key);
        let filepath = self.dir.join(filename);
        fs::create_dir_all(filepath.parent().expect("Must have parent")).context(
            "LocalDirStorage file creation failed. Perhaps the storage object key isn't valid?",
        )?;
        let mut file = File::create(filepath)?;
        file.write_all(&bytes)?;
        Ok(())
    }

    async fn list_objects(&self, key_prefix: &str) -> anyhow::Result<Vec<ObjectListing>> {
        fn walk(dir: &Path, out: &mut Vec<(PathBuf, SystemTime)>) -> anyhow::Result<()> {
            if !dir.exists() {
                return Ok(());
            }
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out)?;
                } else {
                    out.push((path, entry.metadata()?.modified()?));
                }
            }
            Ok(())
        }

        let mut files = Vec::new();
        walk(&self.dir, &mut files)?;
        let mut objects = Vec::new();
        for (path, last_modified) in files {
            let relative = path.strip_prefix(&self.dir)?.to_string_lossy();
            let Some(key) = relative.strip_suffix(".blob") else {
                continue;
            };
            if !key.starts_with(key_prefix) {
                continue;
            }
            objects.push(ObjectListing {
                key: ObjectKey::try_from(key)?,
                last_modified,
            });
        }
        Ok(objects)
    }
}

pub struct LocalDirUpload {
    object_key: ObjectKey,
    file: Option<File>,
    num_parts: usize,
}

#[async_trait]
impl Upload for LocalDirUpload {
    async fn write(&mut self, data: Bytes) -> anyhow::Result<()> {
        anyhow::ensure!(self.num_parts < MAX_NUM_PARTS);
        anyhow::ensure!(data.len() <= LOCAL_DIR_MAX_PART_SIZE);
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Upload not active"))?;

        file.write_all(&data)?;

        self.num_parts += 1;

        Ok(())
    }

    async fn try_write_parallel<'a>(
        &'a mut self,
        stream: &mut Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send + 'a>>,
    ) -> anyhow::Result<()> {
        while let Some(value) = stream.next().await {
            self.write(value?).await?;
        }
        Ok(())
    }

    async fn abort(mut self: Box<Self>) -> anyhow::Result<()> {
        anyhow::ensure!(self.file.is_some());
        self.file.take();
        Ok(())
    }

    async fn complete(mut self: Box<Self>) -> anyhow::Result<ObjectKey> {
        let object_key = self.object_key;

        let file = self.file.take().context("Completing inactive file")?;
        file.sync_all()?;
        Ok(object_key)
    }
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub enum StorageUseCase {
    /// Snapshot Exports
    Exports,
    /// Snapshot Imports, stored temporarily so we can process multiple times
    /// without holding in memory.
    SnapshotImports,
    /// Our module cache
    Modules,
    /// User/customer facing storage
    Files,
    /// Search index snapshots
    SearchIndexes,
}

impl Display for StorageUseCase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageUseCase::Exports => write!(f, "exports"),
            StorageUseCase::SnapshotImports => write!(f, "snapshot_imports"),
            StorageUseCase::Modules => write!(f, "modules"),
            StorageUseCase::Files => write!(f, "files"),
            StorageUseCase::SearchIndexes => write!(f, "search"),
        }
    }
}
