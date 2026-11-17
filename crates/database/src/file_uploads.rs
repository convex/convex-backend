use std::{
    collections::BTreeMap,
    sync::Arc,
};

use bytes::Bytes;
use common::components::ComponentPath;
use imbl::OrdMap;
use value::ResolvedDocumentId;

use crate::{
    writes::PendingWrites,
    FileStorageWriteSize,
};

/// A file stored in a transaction whose `_file_storage` entry is
/// written but whose contents haven't been uploaded yet.
#[derive(Clone, Debug)]
pub struct PendingFileUpload {
    /// The component the file belongs to, for usage attribution.
    pub component_path: ComponentPath,
    pub bytes: Bytes,
}

/// A file stored in a transaction that has been uploaded.
#[derive(Clone, Debug, PartialEq)]
pub struct UploadedFileUpload {
    /// Size of the file in bytes.
    pub size: usize,
}

/// The files a transaction stored. All files are pending until we upload them
/// after function execution finishes.
#[derive(Clone, Debug)]
pub enum FileUploads {
    /// Files stored while the transaction is running.
    Pending(OrdMap<ResolvedDocumentId, Arc<PendingFileUpload>>),
    /// Uploaded files after the transaction execution has finished.
    Uploaded(BTreeMap<ResolvedDocumentId, UploadedFileUpload>),
}

impl Default for FileUploads {
    fn default() -> Self {
        Self::Pending(OrdMap::new())
    }
}

impl PendingWrites for FileUploads {}

impl FileUploads {
    pub(crate) fn add_pending(
        &mut self,
        id: ResolvedDocumentId,
        upload: PendingFileUpload,
    ) -> anyhow::Result<()> {
        match self {
            Self::Pending(uploads) => {
                uploads.insert(id, Arc::new(upload));
                Ok(())
            },
            Self::Uploaded(_) => anyhow::bail!(
                "Can't store a file in a transaction whose files were already uploaded"
            ),
        }
    }

    /// Remove the pending file with the given `_file_storage` id, if it exists.
    pub(crate) fn remove_pending(&mut self, id: &ResolvedDocumentId) -> anyhow::Result<()> {
        match self {
            Self::Pending(uploads) => {
                uploads.remove(id);
                Ok(())
            },
            Self::Uploaded(_) => anyhow::bail!(
                "Can't remove a file in a transaction whose files were already uploaded"
            ),
        }
    }

    pub(crate) fn num_files(&self) -> usize {
        match self {
            Self::Pending(uploads) => uploads.len(),
            Self::Uploaded(uploads) => uploads.len(),
        }
    }

    pub(crate) fn total_bytes(&self) -> usize {
        match self {
            Self::Pending(uploads) => uploads.values().map(|upload| upload.bytes.len()).sum(),
            Self::Uploaded(uploads) => uploads.values().map(|upload| upload.size).sum(),
        }
    }

    pub(crate) fn size(&self) -> FileStorageWriteSize {
        FileStorageWriteSize {
            num_writes: self.num_files(),
            size: self.total_bytes(),
        }
    }

    pub(crate) fn pending_files(
        &self,
    ) -> anyhow::Result<Vec<(ResolvedDocumentId, PendingFileUpload)>> {
        match self {
            Self::Pending(uploads) => Ok(uploads
                .iter()
                .map(|(id, upload)| (*id, PendingFileUpload::clone(upload)))
                .collect()),
            Self::Uploaded(_) => {
                anyhow::bail!("The transaction's stored files were already uploaded")
            },
        }
    }

    pub(crate) fn pending_file(
        &self,
        id: &ResolvedDocumentId,
    ) -> anyhow::Result<Option<&PendingFileUpload>> {
        match self {
            Self::Pending(uploads) => Ok(uploads.get(id).map(Arc::as_ref)),
            Self::Uploaded(_) => {
                anyhow::bail!("The transaction's stored files were already uploaded")
            },
        }
    }

    pub(crate) fn require_no_pending(&self) -> anyhow::Result<()> {
        if let Self::Pending(uploads) = self {
            anyhow::ensure!(
                uploads.is_empty(),
                "Expected no pending file uploads, but found {}",
                uploads.len()
            );
        }
        Ok(())
    }

    pub(crate) fn mark_uploaded(&mut self) {
        if let Self::Pending(uploads) = self {
            let uploaded = uploads
                .iter()
                .map(|(id, upload)| {
                    (
                        *id,
                        UploadedFileUpload {
                            size: upload.bytes.len(),
                        },
                    )
                })
                .collect();
            *self = Self::Uploaded(uploaded);
        }
    }

    pub(crate) fn discard_pending(&mut self) {
        if let Self::Pending(_) = self {
            *self = Self::Uploaded(BTreeMap::new());
        }
    }

    pub(crate) fn uploaded_files(
        &self,
    ) -> anyhow::Result<BTreeMap<ResolvedDocumentId, UploadedFileUpload>> {
        match self {
            Self::Uploaded(uploads) => Ok(uploads.clone()),
            Self::Pending(_) => {
                anyhow::bail!("The transaction's stored files haven't been uploaded")
            },
        }
    }

    pub(crate) fn add_uploaded(
        &mut self,
        uploaded: BTreeMap<ResolvedDocumentId, UploadedFileUpload>,
    ) -> anyhow::Result<()> {
        if uploaded.is_empty() {
            return Ok(());
        }
        match self {
            Self::Uploaded(uploads) => uploads.extend(uploaded),
            Self::Pending(uploads) => {
                anyhow::ensure!(
                    uploads.is_empty(),
                    "Can't add uploaded files to a transaction with files still pending"
                );
                *self = Self::Uploaded(uploaded);
            },
        }
        Ok(())
    }
}
