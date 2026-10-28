use std::{
    fmt,
    io,
    mem,
    ops::Deref,
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use anyhow::Context;
use async_lru::async_lru::{
    AsyncLru,
    SizedValue,
};
use bytesize::ByteSize;
use common::{
    bounded_thread_pool::BoundedThreadPool,
    knobs::ARCHIVE_FETCH_TIMEOUT_SECONDS,
    runtime::{
        Runtime,
        SpawnHandle,
    },
    types::{
        ObjectKey,
        SearchIndexMetricLabels,
    },
};
use futures::{
    pin_mut,
    select_biased,
    FutureExt,
};
use storage::{
    Storage,
    StorageCacheKey,
    StorageExt,
};
use tokio::{
    fs,
    io::{
        AsyncWriteExt,
        BufReader,
    },
    sync::mpsc,
};
use uuid::Uuid;
use vector::qdrant_segments::restore_segment_from_tar;

use super::{
    extract_zip,
    metrics::{
        self,
        archive_untar_timer,
    },
};
use crate::SearchFileType;

/// Owns the directory a fetch extracts into, from the moment the fetch starts
/// until the cache entry built from it is dropped. Dropping it deletes the
/// directory. `size` is 0 until extraction finishes, so a fetch that fails or
/// times out has no bytes to subtract from the accounting.
struct IndexTempDir {
    dir: PathBuf,
    cleaner: CacheCleaner,
    search_file_type: SearchFileType,
    metric_labels: SearchIndexMetricLabels<'static>,
    size: u64,
}

impl Drop for IndexTempDir {
    fn drop(&mut self) {
        let _ = self.cleaner.attempt_cleanup(
            mem::take(&mut self.dir),
            self.search_file_type,
            self.size,
        );
        metrics::adjust_archive_bytes_for_index(
            -(self.size as i64),
            self.search_file_type,
            self.metric_labels.clone(),
        );
    }
}

struct IndexMeta {
    /// A path under `tempdir.dir`; may not be the directory itself
    path: PathBuf,
    tempdir: IndexTempDir,
}

impl SizedValue for IndexMeta {
    fn size(&self) -> u64 {
        self.tempdir.size
    }
}

/// A handle to an archive extracted by an [`ArchiveCacheManager`].
///
/// The extracted files stay on disk for as long as any handle to them exists:
/// evicting the entry from the manager only drops the manager's own reference.
/// Anything that opens files under `path` (a tantivy reader, a qdrant segment,
/// an mmap) must therefore keep the handle alive for as long as it uses them.
#[derive(Clone)]
pub struct CachedArchive {
    path: PathBuf,
    _meta: Arc<IndexMeta>,
}

impl CachedArchive {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Deref for CachedArchive {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for CachedArchive {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl fmt::Debug for CachedArchive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.path.fmt(f)
    }
}

/// A specialized LRU cache for storing archives of Tantivy and Qdrant indexes.
/// The manager is constructed with a storage directory and a max size.
///
/// The entry point to the manager is the `get()` method which checks to see if
/// the key exists in the disk cache, and if not, fetches it from the provided
/// [`Storage`] implementation. Multiple calls to `get()` the same key will be
/// queued, such that only the head of the queue performs the remote fetching
/// and unarchiving; subsequent requests wait on a channel to receive the
/// result. This logic is implemented by the wrapped `AsyncLru`
///
/// The manager asynchronously prunes old entries when the cache is "over
/// quota". As this pruning is performed after archives are added to the cache,
/// the manager will transiently exceed the configured `max_size`.
///
/// `get()` returns a [`CachedArchive`] handle, and an extracted directory is
/// only deleted once the manager has evicted it *and* every handle to it has
/// been dropped. `max_size` therefore bounds the bytes the manager itself
/// retains; directories pinned by outstanding handles (in practice, the entries
/// of the in-memory segment caches) can add to that.
///
/// In the interest of hot-path performance, any deletion or pruning operations
/// are best-effort and are spawned to the thread pool rather than occurring in
/// the calling task. For now, errors in these spawned tasks will panic the
/// entire process.
pub struct ArchiveCacheManager<RT: Runtime> {
    path: PathBuf,
    max_size: u64,
    cleaner: CacheCleaner,
    blocking_thread_pool: BoundedThreadPool<RT>,
    cache: AsyncLru<RT, Key, IndexMeta>,
    rt: RT,
}

impl<RT: Runtime> Clone for ArchiveCacheManager<RT> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            max_size: self.max_size,
            cleaner: self.cleaner.clone(),
            blocking_thread_pool: self.blocking_thread_pool.clone(),
            cache: self.cache.clone(),
            rt: self.rt.clone(),
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
struct Key {
    path: StorageCacheKey,
    search_file_type: SearchFileType,
}

#[derive(Clone)]
struct ArchiveFetcher<RT: Runtime> {
    cache_path: PathBuf,
    rt: RT,
    blocking_thread_pool: BoundedThreadPool<RT>,
    cleaner: CacheCleaner,
}

impl<RT: Runtime> ArchiveFetcher<RT> {
    /// Fetch the archive with the given key from storage, and extract it into a
    /// new directory owned by this manager. The caller is responsible for
    /// updating internal state to account for this new addition.
    ///
    /// This function is intentionally private, and should only be called via
    /// `get` by the task that was first to request this key.
    async fn fetch(
        self,
        search_storage: Arc<dyn Storage>,
        key: ObjectKey,
        search_file_type: SearchFileType,
        mut destination: IndexTempDir,
    ) -> anyhow::Result<IndexMeta> {
        let timer = metrics::archive_fetch_timer();
        let archive = search_storage
            .get(&key)
            .await?
            .context(format!("{key:?} not found in search storage"))?
            .into_tokio_reader();
        let extract_archive_timer = metrics::extract_archive_timer();
        let extract_archive_result = self
            .extract_archive(search_file_type, destination.dir.clone(), archive)
            .await;
        extract_archive_timer.finish();

        let (bytes_used, path) = extract_archive_result?;
        if is_immutable(search_file_type) {
            set_readonly(&path, true).await?;
        }
        metrics::add_bytes_by_file_type(search_file_type, bytes_used);
        metrics::finish_archive_fetch(timer, bytes_used, search_file_type);
        metrics::adjust_archive_bytes_for_index(
            bytes_used as i64,
            search_file_type,
            destination.metric_labels.clone(),
        );
        destination.size = bytes_used;
        Ok(IndexMeta {
            tempdir: destination,
            path,
        })
    }

    async fn extract_archive(
        &self,
        search_file_type: SearchFileType,
        output_directory: PathBuf,
        archive: impl tokio::io::AsyncRead + Send + 'static + Unpin,
    ) -> anyhow::Result<(u64, PathBuf)> {
        if search_file_type == SearchFileType::FragmentedVectorSegment {
            self.extract_segment(&output_directory, archive).await
        } else {
            let bytes_copied = extract_zip(&output_directory, archive).await?;
            // Generic ZIPs are always extracted to the output directory.
            Ok((bytes_copied, output_directory))
        }
    }

    async fn extract_segment(
        &self,
        output_directory: &PathBuf,
        archive: impl tokio::io::AsyncRead + Send + 'static + Unpin,
    ) -> anyhow::Result<(u64, PathBuf)> {
        fs::create_dir(output_directory).await?;
        let output_file = output_directory.join("segment.tar");
        let bytes_copied = {
            let mut reader = BufReader::with_capacity(2 << 16, archive);
            let mut file = fs::File::create(&output_file).await?;
            let bytes_copied = tokio::io::copy_buf(&mut reader, &mut file).await?;
            file.flush().await?;
            bytes_copied
        };

        // We're expecting that the uncompressed tar and its contents are roughly the
        // same size. There is some file moving / copying going on in
        // this method, but hopefully it's small enough to be a rounding
        // error relative to the overall segment size.
        let path = Self::unpack_fragmented_segment_tar(output_file).await?;

        Ok((bytes_copied, path))
    }

    async fn unpack_fragmented_segment_tar(tar_path: PathBuf) -> anyhow::Result<PathBuf> {
        let timer = archive_untar_timer();
        let restored_path = restore_segment_from_tar(&tar_path).await?;
        fs::remove_file(tar_path).await?;
        timer.finish();
        Ok(restored_path)
    }
}

impl<RT: Runtime> ArchiveFetcher<RT> {
    async fn generate_value(
        self,
        search_storage: Arc<dyn Storage>,
        key: ObjectKey,
        search_file_type: SearchFileType,
        metric_labels: SearchIndexMetricLabels<'static>,
    ) -> anyhow::Result<IndexMeta> {
        let mut timeout_fut = self.rt.wait(*ARCHIVE_FETCH_TIMEOUT_SECONDS).fuse();
        let destination = self.cache_path.join(Uuid::new_v4().simple().to_string());

        // Create this right away so its Drop impl (which deletes the path) runs
        // even on failure
        let tempdir = IndexTempDir {
            cleaner: self.cleaner.clone(),
            dir: destination.clone(),
            search_file_type,
            metric_labels,
            size: 0,
        };
        let new_self = self.clone();
        let new_key = key.clone();
        // Many parts of the fetch perform blocking operations. To avoid blocking the
        // calling thread's scheduling, punt all fetches to a separate OS thread.
        let fetch_fut = self
            .blocking_thread_pool
            .execute_async(move || {
                new_self.fetch(search_storage, new_key, search_file_type, tempdir)
            })
            .fuse();
        pin_mut!(fetch_fut);
        select_biased! {
            meta = fetch_fut => {
                meta?
            },
            _ = timeout_fut => {
                metrics::log_cache_fetch_timeout();
                tracing::error!("Timed out fetching archive for key {key:?}");
                Err(anyhow::anyhow!("Timed out"))
            }
        }
    }
}

impl<RT: Runtime> ArchiveCacheManager<RT> {
    /// Create a new [ArchiveCacheManager] with the specified `max_size` bytes
    /// used on disk. The manager will attempt to create a directory under
    /// `storage_path` where archives will be extracted and
    /// cached. The manager expects to have sole custody over this
    /// directory, and will not observe any external changes that could
    /// affect the space usage of the cache.
    ///
    /// When callers query the cache with `.get()` and the key is not yet
    /// present on disk, the first caller will fetch the file from the provided
    /// [Storage] implementation, while all subsequent callers will queue behind
    /// this task and await a notification that the result is ready.
    ///
    /// Returns an error if the manager is unable to create a directory under
    /// `storage_path`, or if `storage_path` doesn't already exist.
    pub fn new<P: AsRef<Path>>(
        local_storage_path: P,
        max_size: u64,
        blocking_thread_pool: BoundedThreadPool<RT>,
        max_concurrent_fetches: usize,
        rt: RT,
    ) -> anyhow::Result<Self> {
        let cleaner = CacheCleaner::new(rt.clone());
        let cache = AsyncLru::new(rt.clone(), max_size, max_concurrent_fetches, 200, "cache");
        let this = Self {
            path: local_storage_path.as_ref().to_owned(),
            max_size,
            blocking_thread_pool,
            cache,
            cleaner,
            rt,
        };
        Ok(this)
    }

    /// Returns the configured max capacity for this manager.
    #[expect(dead_code)]
    pub fn max_size(&self) -> u64 {
        self.max_size
    }

    /// Get a handle to the extracted directory for a given key. Fetches the
    /// archive from storage if it doesn't already exist on disk.
    #[fastrace::trace]
    pub async fn get(
        &self,
        search_storage: Arc<dyn Storage>,
        key: &ObjectKey,
        search_file_type: SearchFileType,
        metric_labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<CachedArchive> {
        let timer = metrics::archive_get_timer(search_file_type);
        let result = self
            .get_logged(search_storage, key, search_file_type, metric_labels)
            .await;
        timer.finish(result.is_ok());
        result
    }

    #[fastrace::trace]
    pub async fn get_single_file(
        &self,
        search_storage: Arc<dyn Storage>,
        storage_path: &ObjectKey,
        file_type: SearchFileType,
        metric_labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<CachedArchive> {
        // The archive cache always dumps things into directories, but we want a
        // specific file path.
        let parent_dir = self
            .get(search_storage, storage_path, file_type, metric_labels)
            .await?;
        let mut read_dir = fs::read_dir(parent_dir.path()).await?;
        let mut paths = Vec::with_capacity(1);
        while let Some(entry) = read_dir.next_entry().await? {
            paths.push(entry.path());
        }
        anyhow::ensure!(
            paths.len() == 1,
            "Expected one file but found multiple paths: {:?}",
            paths,
        );
        Ok(CachedArchive {
            path: paths.pop().expect("checked above"),
            _meta: parent_dir._meta,
        })
    }

    async fn get_logged(
        &self,
        search_storage: Arc<dyn Storage>,
        key: &ObjectKey,
        search_file_type: SearchFileType,
        metric_labels: SearchIndexMetricLabels<'_>,
    ) -> anyhow::Result<CachedArchive> {
        let archive_fetcher = ArchiveFetcher {
            cache_path: self.path.clone(),
            rt: self.rt.clone(),
            blocking_thread_pool: self.blocking_thread_pool.clone(),
            cleaner: self.cleaner.clone(),
        };
        let cache_key = Key {
            path: search_storage.cache_key(key),
            search_file_type,
        };
        let result = self
            .cache
            .get(&cache_key, || {
                archive_fetcher.generate_value(
                    search_storage.clone(),
                    key.clone(),
                    search_file_type,
                    metric_labels.to_owned(),
                )
            })
            .await
            .with_context(|| {
                format!("Failed to get cache_key {cache_key:?} in {search_storage:?}")
            })?;

        let path = result.path.clone();
        let current_size = self.cache.size();
        tracing::debug!(
            "Finished fetching archive for key {key:?}, cached path: {}(space used: {} / {})",
            path.display(),
            ByteSize(current_size),
            ByteSize(self.max_size)
        );
        metrics::log_bytes_used(current_size, self.max_size);

        Ok(CachedArchive {
            path,
            _meta: result,
        })
    }
}

fn is_immutable(search_file_type: SearchFileType) -> bool {
    match search_file_type {
        // At least one rocksdb instance used by the qdrant Segment is not
        // opened in read only mode.
        SearchFileType::VectorSegment => false,
        SearchFileType::FragmentedVectorSegment => true,
        SearchFileType::VectorDeletedBitset => true,
        SearchFileType::VectorIdTracker => true,
        // Text indexes do not appear to be read in readonly mode.
        SearchFileType::Text => false,
        SearchFileType::TextIdTracker => true,
        SearchFileType::TextAliveBitset => true,
        SearchFileType::TextDeletedTerms => true,
    }
}

async fn set_readonly(path: &Path, readonly: bool) -> io::Result<()> {
    let metadata = fs::metadata(path).await?;
    let mut permissions = metadata.permissions();
    permissions.set_readonly(readonly);
    fs::set_permissions(path, permissions).await?;
    Ok(())
}

/// Clears the readonly bit on `path` and everything beneath it.
///
/// `fetch` marks the directory an immutable archive was extracted into as
/// readonly, and for a `FragmentedVectorSegment` that directory is the nested
/// `<entry>/segment`, not the entry itself. A non-root process cannot unlink
/// files inside a directory without the write bit, so `remove_dir_all` on the
/// entry needs the bit cleared throughout the tree. Symlinks are skipped so the
/// walk never leaves the cache directory; paths that disappear mid-walk are
/// tolerated.
async fn clear_readonly_recursive(path: &Path) -> io::Result<()> {
    let mut stack = vec![path.to_owned()];
    while let Some(current) = stack.pop() {
        let Some(metadata) = ignore_not_found(fs::symlink_metadata(&current).await)? else {
            continue;
        };
        if metadata.is_symlink() {
            continue;
        }
        if metadata.permissions().readonly() {
            ignore_not_found(set_readonly(&current, false).await)?;
        }
        if metadata.is_dir() {
            let Some(mut entries) = ignore_not_found(fs::read_dir(&current).await)? else {
                continue;
            };
            while let Some(entry) = ignore_not_found(entries.next_entry().await)?.flatten() {
                stack.push(entry.path());
            }
        }
    }
    Ok(())
}

/// Maps a `NotFound` error to `Ok(None)`, for filesystem steps that are moot
/// once the path they target has disappeared.
fn ignore_not_found<T>(result: io::Result<T>) -> io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[derive(Clone)]
struct CacheCleaner {
    cleanup_tx: mpsc::UnboundedSender<(PathBuf, SearchFileType, u64)>,
    _cleanup_handle: Arc<Box<dyn SpawnHandle>>,
}

impl CacheCleaner {
    fn new<RT: Runtime>(rt: RT) -> Self {
        let (cleanup_tx, cleanup_rx) = mpsc::unbounded_channel();
        let cleanup_handle =
            Arc::new(rt.spawn_thread("search_cache_cleaner", || cleanup_thread(cleanup_rx)));
        Self {
            cleanup_tx,
            _cleanup_handle: cleanup_handle,
        }
    }

    fn attempt_cleanup(
        &self,
        path: PathBuf,
        search_file_type: SearchFileType,
        size: u64,
    ) -> anyhow::Result<()> {
        Ok(self.cleanup_tx.send((path, search_file_type, size))?)
    }
}

/// Runs on a separate thread to delete archives that have been removed from the
/// in-memory cache.
/// Using a separate thread for this is just an optimization, recognizing that a
/// recursive deletion doesn't need to be in the critical path and may block the
/// for a meaningful amount of time as opposed to our other filesystem ops which
/// should be quite fast.
async fn cleanup_thread(mut rx: mpsc::UnboundedReceiver<(PathBuf, SearchFileType, u64)>) {
    while let Some((path, search_file_type, size)) = rx.recv().await {
        // Yes, we'll panic and restart here. If we actually see panics in
        // production here, we should investigate further but for now, it's simpler
        // to disallow inconsistent filesystem state.
        tracing::debug!("Removing path {} from disk", path.display());
        let result: io::Result<()> = try {
            clear_readonly_recursive(&path).await?;
            fs::remove_dir_all(&path).await?;
        };
        match result {
            Ok(()) => {
                metrics::subtract_bytes_by_file_type(search_file_type, size);
            },
            // Can happen if the path to clean up was never created
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => panic!("ArchiveCacheManager failed to clean up archive directory: {e:?}"),
        }
    }
}
