#![feature(coroutines)]
use std::{
    pin::Pin,
    sync::Arc,
    task::{
        Context,
        Poll,
    },
};

use common::{
    runtime::Runtime,
    sha256::Sha256Digest,
    types::ConvexOrigin,
};
use database::Database;
use futures::{
    stream::BoxStream,
    Stream,
};
use headers::{
    ContentLength,
    ContentRange,
    ContentType,
};
use storage::Storage;

mod core;
mod downloads;
mod metrics;
pub use self::downloads::FileDownloads;

pub const UPLOAD_PENDING_FILE: &str = "upload_pending_file";
pub const DOWNLOAD_FILE: &str = "download_file";
pub const FILE_UPLOAD_FAILED_SHORT_MSG: &str = "FileUploadFailed";
pub const FILE_DOWNLOAD_FAILED_SHORT_MSG: &str = "FileDownloadFailed";

pub struct FileStream {
    pub sha256: Option<Sha256Digest>,
    pub content_length: ContentLength,
    /// None if file size is 0, as the RFC doesn't allow range responses on
    /// 0-size files https://datatracker.ietf.org/doc/html/rfc7233#section-4.2
    pub content_range: Option<ContentRange>,
    pub content_type: Option<ContentType>,
    inner: BoxStream<'static, futures::io::Result<bytes::Bytes>>,
    bytes_read_so_far: u64,
    on_complete: Vec<Box<dyn FnOnce(u64) + Send>>,
}

impl FileStream {
    pub fn add_on_complete(&mut self, cb: Box<dyn FnOnce(u64) + Send>) {
        self.on_complete.push(cb);
    }

    fn fire_on_complete(&mut self) {
        for cb in self.on_complete.drain(..) {
            cb(self.bytes_read_so_far);
        }
    }
}

impl Stream for FileStream {
    type Item = futures::io::Result<bytes::Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                this.bytes_read_so_far += bytes.len() as u64;
                Poll::Ready(Some(Ok(bytes)))
            },
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => {
                this.fire_on_complete();
                Poll::Ready(None)
            },
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for FileStream {
    fn drop(&mut self) {
        self.fire_on_complete();
    }
}

#[derive(Clone)]
pub struct FileStorage<RT: Runtime> {
    pub database: Database<RT>,
    pub transactional_file_storage: TransactionalFileStorage<RT>,
}

#[derive(Clone)]
pub struct TransactionalFileStorage<RT: Runtime> {
    rt: RT,
    storage: Arc<dyn Storage>,
    convex_origin: ConvexOrigin,
}
