use std::{
    path::Path,
    sync::Arc,
    time::SystemTime,
};

use anyhow::Context;
use async_zip_0_0_9::{
    read::stream::ZipFileReader,
    write::ZipFileWriter,
    Compression,
    ZipEntryBuilder,
    ZipEntryBuilderExt,
};
use bytes::Bytes;
use common::{
    bootstrap_model::index::{
        text_index::FragmentedTextSegment,
        vector_index::FragmentedVectorSegment,
    },
    runtime::{
        tokio_spawn_blocking,
        Runtime,
    },
    types::ObjectKey,
};
use storage::{
    ChannelWriter,
    Storage,
    StorageExt,
    Upload,
    UploadExt,
};
use tantivy::{
    Index,
    IndexReader,
    ReloadPolicy,
};
use tokio::{
    fs,
    io::{
        AsyncBufRead,
        AsyncWrite,
        AsyncWriteExt,
        BufReader,
        BufWriter,
    },
    sync::mpsc,
};
use tokio_stream::wrappers::ReceiverStream;
use vector::qdrant_segments::{
    VectorDiskSegmentPaths,
    VectorDiskSegmentValues,
};
use walkdir::WalkDir;

use crate::{
    constants::CONVEX_EN_TOKENIZER,
    convex_en,
    metrics::{
        self,
    },
    NewTextSegment,
    SearchFileType,
    TextSegmentPaths,
};

#[fastrace::trace]
pub async fn index_reader_for_directory<P: AsRef<Path>>(
    directory: P,
) -> anyhow::Result<IndexReader> {
    let timer = metrics::index_reader_for_directory_timer();
    let directory = directory.as_ref().to_path_buf();
    let index =
        tokio_spawn_blocking("disk_index_open", move || Index::open_in_dir(directory)).await??;
    index
        .tokenizers()
        .register(CONVEX_EN_TOKENIZER, convex_en());
    // These directories hold an immutable snapshot of one committed segment
    // that this process never writes to, and every caller takes
    // `reader.searcher()` immediately and never reloads. The default
    // `ReloadPolicy::OnCommit` spawns a thread per reader that polls
    // `meta.json` every 500ms for the reader's whole lifetime, and once the
    // archive cache deletes the directory that thread warns about the missing
    // file twice a second forever.
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()?;
    timer.finish();
    Ok(reader)
}

pub async fn download_single_file_zip<P: AsRef<Path>>(
    key: &ObjectKey,
    path: P,
    storage: Arc<dyn Storage>,
) -> anyhow::Result<()> {
    // Start the file download
    let stream = storage
        .get(key)
        .await?
        .context(format!("Failed to find stored file! {key:?}"))?
        .into_tokio_reader();

    // Open the target file
    let file = fs::File::create(path).await?;
    let mut file = BufWriter::new(file);

    // Require the stream to be a zip containing a single file, extract the data for
    // that single file and write it to our target path.
    let mut reader = ZipFileReader::new(stream);
    let mut is_written = false;
    while !reader.finished() {
        // Some entries may just be blank, so we skip them.
        if let Some(entry) = reader.entry_reader().await? {
            // Some entries may be directories, which we don't care about.
            if entry.entry().filename().ends_with('/') {
                continue;
            }
            // If it is a file, make sure we haven't already read one (we're expecting
            // exactly one!)
            if is_written {
                anyhow::bail!(
                    "ZIP contained multiple files! latest: {:?}",
                    entry.entry().filename()
                );
            }
            is_written = true;
            entry.copy_to_end_crc(&mut file, 2 << 15).await?;

            // Keep reading to make sure we don't get something unexpected (like
            // multiple files)
        }
    }
    file.flush().await?;
    Ok(())
}

pub async fn upload_text_segment<RT: Runtime>(
    rt: &RT,
    storage: Arc<dyn Storage>,
    new_segment: NewTextSegment,
) -> anyhow::Result<FragmentedTextSegment> {
    let TextSegmentPaths {
        index_path,
        id_tracker_path,
        alive_bit_set_path,
        deleted_terms_path,
    } = new_segment.paths;
    let upload_index =
        upload_index_archive_from_path(index_path, storage.clone(), SearchFileType::Text);
    let upload_id_tracker = upload_single_file_from_path(
        id_tracker_path,
        storage.clone(),
        SearchFileType::TextIdTracker,
    );
    let upload_bitset = upload_single_file_from_path(
        alive_bit_set_path,
        storage.clone(),
        SearchFileType::TextAliveBitset,
    );
    let upload_deleted_terms = upload_single_file_from_path(
        deleted_terms_path,
        storage.clone(),
        SearchFileType::TextDeletedTerms,
    );
    let result = futures::try_join!(
        upload_index,
        upload_id_tracker,
        upload_bitset,
        upload_deleted_terms
    )?;
    let (segment_key, id_tracker_key, alive_bitset_key, deleted_terms_table_key) = result;
    Ok(FragmentedTextSegment {
        segment_key,
        id_tracker_key,
        deleted_terms_table_key,
        alive_bitset_key,
        num_indexed_documents: new_segment.num_indexed_documents,
        // Brand-new text segments will never have any deleted documents. Deleted documents will
        // instead have just been excluded from the segment.
        num_deleted_documents: 0,
        size_bytes_total: new_segment.size_bytes_total,
        id: rt.new_uuid_v4().to_string(),
    })
}

pub async fn upload_vector_segment<RT: Runtime>(
    rt: &RT,
    storage: Arc<dyn Storage>,
    new_segment: VectorDiskSegmentValues,
) -> anyhow::Result<FragmentedVectorSegment> {
    let VectorDiskSegmentPaths {
        segment,
        uuids,
        deleted_bitset,
    } = new_segment.paths;
    let upload_segment = upload_single_file_from_path(
        segment,
        storage.clone(),
        SearchFileType::FragmentedVectorSegment,
    );
    let upload_id_tracker =
        upload_single_file_from_path(uuids, storage.clone(), SearchFileType::VectorIdTracker);
    let upload_bitset = upload_single_file_from_path(
        deleted_bitset,
        storage.clone(),
        SearchFileType::VectorDeletedBitset,
    );
    let (segment_key, id_tracker_key, deleted_bitset_key) =
        futures::try_join!(upload_segment, upload_id_tracker, upload_bitset)?;

    Ok(FragmentedVectorSegment {
        segment_key,
        id_tracker_key,
        deleted_bitset_key,
        num_vectors: new_segment.num_vectors,
        num_deleted: new_segment.num_deleted,
        id: rt.new_uuid_v4().to_string(),
    })
}

pub async fn upload_single_file_from_path<P: AsRef<Path>>(
    path: P,
    storage: Arc<dyn Storage>,
    upload_type: SearchFileType,
) -> anyhow::Result<ObjectKey> {
    let filename = path
        .as_ref()
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("invalid path: {:?}", path.as_ref()))?
        .to_string();

    let file = fs::File::open(path).await?;
    let mut file = BufReader::new(file);
    upload_single_file(&mut file, filename, storage, upload_type).await
}

pub async fn upload_single_file<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    filename: String,
    storage: Arc<dyn Storage>,
    upload_type: SearchFileType,
) -> anyhow::Result<ObjectKey> {
    let timer = metrics::upload_archive_timer(upload_type);
    let (sender, receiver) = mpsc::channel::<Bytes>(1);
    let mut upload = storage.start_upload().await?;
    let uploader = upload.write_parallel(ReceiverStream::new(receiver));
    let writer = ChannelWriter::new(sender, 5 * (1 << 20));
    // FragmentedVectorSegment files are tarballs already. Compression provides a
    // relatively small improvement in file size. Extracting a zip and then a
    // second tarball is expensive. TODO(CX-5191): We should think about
    // compressing the tar files as they're created.
    let file_type = if upload_type == SearchFileType::FragmentedVectorSegment {
        SingleFileFormat::ORIGINAL
    } else {
        SingleFileFormat::ZIP
    };
    let archiver = write_single_file(reader, filename, writer, file_type);
    tokio::try_join!(archiver, uploader)?;
    let key = upload.complete().await?;
    timer.finish();
    Ok(key)
}

#[derive(PartialEq)]
enum SingleFileFormat {
    /// Zip the file during upload with compression
    ZIP,
    /// Just upload the original file without any additional processing or
    /// compression.
    ORIGINAL,
}

async fn write_single_file<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    filename: String,
    mut out: W,
    format: SingleFileFormat,
) -> anyhow::Result<()> {
    if format == SingleFileFormat::ZIP {
        let mut writer = ZipFileWriter::new(&mut out);
        zip_single_file(reader, filename, &mut writer).await?;
        writer.close().await?;
    } else {
        raw_single_file(reader, &mut out).await?;
    }
    out.shutdown().await?;
    Ok(())
}

pub async fn upload_index_archive_from_path<P: AsRef<Path>>(
    directory: P,
    storage: Arc<dyn Storage>,
    upload_type: SearchFileType,
) -> anyhow::Result<ObjectKey> {
    let timer = metrics::upload_archive_timer(upload_type);
    let (sender, receiver) = mpsc::channel::<Bytes>(1);
    let mut upload = storage.start_upload().await?;
    let uploader = upload.write_parallel(ReceiverStream::new(receiver));
    let writer = ChannelWriter::new(sender, 5 * (1 << 20));
    let archiver = write_index_archive(directory, writer);
    let ((), ()) = futures::try_join!(archiver, uploader)?;
    let key = upload.complete().await?;
    timer.finish();
    Ok(key)
}

async fn write_index_archive<P: AsRef<Path>>(
    directory: P,
    mut out: impl AsyncWrite + Send + Unpin,
) -> anyhow::Result<()> {
    let mut writer = ZipFileWriter::new(&mut out);
    for entry in WalkDir::new(&directory).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }

        let filename = entry
            .path()
            .strip_prefix(&directory)?
            .to_str()
            .map(|s| s.to_owned())
            .context("Invalid path inside directory")?;
        let file = fs::File::open(entry.path()).await?;
        let mut file = BufReader::new(file);
        zip_single_file(&mut file, filename, &mut writer).await?;
    }
    writer.close().await?;
    out.shutdown().await?;
    Ok(())
}

async fn raw_single_file<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
) -> anyhow::Result<()> {
    let bytes_written = tokio::io::copy_buf(reader, writer).await?;
    tracing::trace!("Copied {bytes_written} bytes");
    Ok(())
}

async fn zip_single_file<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    filename: String,
    writer: &mut ZipFileWriter<W>,
) -> anyhow::Result<()> {
    let entry = ZipEntryBuilder::new(filename, Compression::Zstd)
        .unix_permissions(0o644)
        // Pin the mtime to prevent flakes in CI, where we've observed the mtime incrementing by
        // one when traversing the test directory multiple times.
        .last_modification_date(SystemTime::UNIX_EPOCH.into())
        .build();
    let mut stream = writer.write_entry_stream(entry).await?;
    tokio::io::copy_buf(reader, &mut stream).await?;
    stream.close().await?;
    Ok(())
}
