use std::{
    io,
    ops::Deref,
    pin::Pin,
    sync::Arc,
    task::{
        ready,
        Context,
        Poll,
    },
};

use anyhow::Context as _;
use bytes::Bytes;
use common::types::{
    FullyQualifiedObjectKey,
    ObjectKey,
};
use errors::ErrorMetadata;
use futures::Stream;
pub use rc_zip;
use rc_zip::{
    fsm::{
        ArchiveFsm,
        EntryFsm,
        FsmResult,
    },
    parse::{
        Archive,
        Entry,
    },
};
use storage::{
    Storage,
    StorageExt as _,
};
use tokio::io::{
    AsyncRead,
    AsyncReadExt,
    ReadBuf,
};
use tokio_util::io::StreamReader;

pub struct StorageZipArchive {
    storage: Arc<dyn Storage>,
    object_key: FullyQualifiedObjectKey,
    archive: Archive,
}

impl Deref for StorageZipArchive {
    type Target = Archive;

    fn deref(&self) -> &Self::Target {
        &self.archive
    }
}

impl StorageZipArchive {
    /// Reads the central directory of the given zip file in object storage.
    pub async fn open(storage: Arc<dyn Storage>, object_key: &ObjectKey) -> anyhow::Result<Self> {
        let fq_key = storage.fully_qualified_key(object_key);
        Self::open_fq(storage, fq_key).await
    }

    pub async fn open_fq(
        storage: Arc<dyn Storage>,
        object_key: FullyQualifiedObjectKey,
    ) -> anyhow::Result<Self> {
        let attributes = storage
            .get_fq_object_attributes(&object_key)
            .await?
            .with_context(|| format!("Could not find object with key {object_key:?}"))?;
        let mut fsm = ArchiveFsm::new(attributes.size);
        let mut read_position = u64::MAX; // arbitrary value that would never be used
        let mut read_stream: Option<StreamReader<_, _>> = None;
        loop {
            if let Some(offset) = fsm.wants_read() {
                if offset == read_position
                    && let Some(reader) = &mut read_stream
                {
                    // Continue reading
                    anyhow::ensure!(!fsm.space().is_empty(), "wants read but no buffer?");
                    let read_bytes = reader.read(fsm.space()).await?;
                    fsm.fill(read_bytes);
                    read_position += read_bytes as u64;
                    if read_bytes == 0 {
                        read_stream = None;
                        continue;
                    }
                } else {
                    let stream = if read_position == offset {
                        // If we are continuing a sequential read, then assume
                        // that we're reading the central directory; read more
                        // data at once
                        storage
                            .get_fq_object_exact_range(&object_key, offset..attributes.size)
                            .stream
                    } else {
                        let read_len = fsm.space().len() as u64;
                        let end = attributes.size.min(offset + read_len);
                        storage
                            .get_small_range_with_retries(&object_key, offset..end)
                            .await?
                            .stream
                    };
                    read_position = offset;
                    read_stream = Some(StreamReader::new(stream));
                }
            }
            match fsm
                .process()
                .context(ErrorMetadata::bad_request("InvalidZip", "invalid zip file"))?
            {
                FsmResult::Continue(next) => fsm = next,
                FsmResult::Done(archive) => {
                    return Ok(Self {
                        storage,
                        object_key,
                        archive,
                    })
                },
            }
        }
    }

    /// Creates a reader for an entry in the archive.
    /// To get an `Entry`, use [`Archive::entries`] via `StorageZipArchive`'s
    /// `Deref` impl.
    pub fn read_entry(&self, entry: Entry) -> StorageZipEntryReader {
        let start = entry.header_offset;
        // The absolute max amount of data that could be read includes the local
        // file header, compressed data, and data descriptor. The local file
        // header is variable-size but could contain up to 2 64KiB fields (file
        // name & extra fields), and then we add 1KiB for the remaining
        // fixed-size stuff.
        const MAX_HEADER_SIZE: u64 = (1 << 16) * 2 + 1024;
        let end = self
            .archive
            .size()
            .min(start + entry.compressed_size + MAX_HEADER_SIZE);
        let read_stream = StreamReader::new(
            self.storage
                .get_fq_object_exact_range(&self.object_key, start..end)
                .stream,
        );
        StorageZipEntryReader {
            read_stream,
            entry_fsm: Some(EntryFsm::new(Some(entry), None)),
        }
    }
}

/// Reads the content of a single file in a zip archive in storage.
pub struct StorageZipEntryReader {
    read_stream:
        StreamReader<Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send + 'static>>, Bytes>,
    entry_fsm: Option<EntryFsm>,
}

impl AsyncRead for StorageZipEntryReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;
        loop {
            let Some(fsm) = &mut this.entry_fsm else {
                // we previously hit EOF or an error
                return Poll::Ready(Ok(()));
            };
            let mut read_stream_eof = false;
            if fsm.wants_read() {
                let mut read_buf = ReadBuf::new(fsm.space());
                ready!(Pin::new(&mut this.read_stream).poll_read(cx, &mut read_buf))?;
                let read_bytes = read_buf.filled().len();
                fsm.fill(read_bytes);
                if read_bytes == 0 {
                    read_stream_eof = true;
                }
            }
            if buf.remaining() == 0 {
                // Defensive check; this is mostly invalid but we should not
                // infinite loop here
                return Poll::Ready(Ok(()));
            }
            let fsm = this.entry_fsm.take().unwrap();
            // N.B.: use block_in_place because decompression is happening here
            match common::runtime::block_in_place(|| fsm.process(buf.initialize_unfilled())) {
                Ok(FsmResult::Continue((fsm, outcome))) => {
                    let fsm = this.entry_fsm.insert(fsm);
                    buf.advance(outcome.bytes_written);
                    if outcome.bytes_written == 0 && buf.remaining() > 0 {
                        if read_stream_eof {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "Hit EOF while reading zip entry",
                            )));
                        }
                        // This would otherwise signal EOF; try reading again instead.
                        if !fsm.wants_read() {
                            // guard against an infinite loop
                            return Poll::Ready(Err(io::Error::other(
                                "bug: EntryFsm wrote nothing but doesn't want read?",
                            )));
                        }
                        continue;
                    }
                    return Poll::Ready(Ok(()));
                },
                Ok(FsmResult::Done(_buffer)) => return Poll::Ready(Ok(())),
                // zip parse or decompression error
                Err(e) => return Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidData, e))),
            }
        }
    }
}
