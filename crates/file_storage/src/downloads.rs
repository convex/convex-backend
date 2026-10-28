use std::{
    collections::BTreeMap,
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use common::types::ObjectKey;
use parking_lot::Mutex;

/// All files downloaded by and the total download time elapsed in a mutation,
/// including in nested mutations
#[derive(Clone, Debug, Default)]
pub struct FileDownloads {
    inner: Arc<Mutex<FileDownloadsInner>>,
}

#[derive(Debug, Default)]
struct FileDownloadsInner {
    files: BTreeMap<ObjectKey, Bytes>,
    download_time: Duration,
}

impl FileDownloads {
    pub fn get(&self, storage_key: &ObjectKey) -> Option<Bytes> {
        self.inner.lock().files.get(storage_key).cloned()
    }

    pub(crate) fn download_time(&self) -> Duration {
        self.inner.lock().download_time
    }

    pub(crate) fn add_download_time(&self, download_time: Duration) {
        self.inner.lock().download_time += download_time;
    }

    pub(crate) fn insert(&self, files: impl IntoIterator<Item = (ObjectKey, Bytes)>) {
        self.inner.lock().files.extend(files);
    }
}
