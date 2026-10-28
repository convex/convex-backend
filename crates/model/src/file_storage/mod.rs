use std::{
    collections::BTreeMap,
    str::FromStr,
    sync::{
        Arc,
        LazyLock,
    },
};

use anyhow::Context;
use common::{
    document::{
        ParseDocument,
        ParsedDocument,
    },
    errors::report_error,
    knobs::FILE_STORAGE_SIZE_MIN_DOCUMENTS_TO_RESUME,
    query::{
        IndexRange,
        IndexRangeExpression,
        Order,
        Query,
    },
    runtime::Runtime,
    types::{
        IndexName,
        IndexRef,
        StorageUuid,
    },
    virtual_system_mapping::AssociatedVirtualTable,
};
use database::{
    query::{
        resolved_query_batch_next,
        TableFilter,
    },
    unauthorized_error,
    DataSyncCursor,
    DataSyncIterator,
    DataSyncStatus,
    DatabaseSnapshot,
    IndexModel,
    ResolvedQuery,
    SearchNotEnabled,
    SystemMetadataModel,
    TableModel,
    Transaction,
};
use errors::ErrorMetadata;
use imbl::ordmap;
use keybroker::Identity;
use maplit::btreemap;
use pb::storage::{
    file_storage_id::StorageIdType as FileStorageIdTypeProto,
    FileStorageId as FileStorageIdProto,
};
use usage_tracking::FunctionUsageTracker;
use value::{
    id_v6::DeveloperDocumentId,
    ConvexValue,
    FieldPath,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
    TabletId,
};

use self::virtual_table::FileStorageDocMapper;
use crate::{
    file_storage::types::FileStorageEntry,
    virtual_system_mapping,
    SystemIndex,
    SystemTable,
};

pub mod types;
pub mod virtual_table;

pub type BatchKey = usize;

pub const FILE_STORAGE_TABLE: TableName = TableName::const_new("_file_storage");
pub const FILE_STORAGE_VIRTUAL_TABLE: TableName = TableName::const_new("_storage");

const FILE_STORAGE_INDEX_BY_ID: IndexName = IndexName::by_id(FILE_STORAGE_TABLE);
const FILE_STORAGE_INDEX_BY_CREATION_TIME: IndexName =
    IndexName::by_creation_time(FILE_STORAGE_TABLE);
const FILE_STORAGE_VIRTUAL_INDEX_BY_ID: IndexName = IndexName::by_id(FILE_STORAGE_VIRTUAL_TABLE);
const FILE_STORAGE_VIRTUAL_INDEX_BY_CREATION_TIME: IndexName =
    IndexName::by_creation_time(FILE_STORAGE_VIRTUAL_TABLE);

static FILE_STORAGE_ID_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "storageId".parse().expect("invalid storageId field"));
pub static FILE_STORAGE_ID_INDEX: LazyLock<SystemIndex<FileStorageTable>> =
    LazyLock::new(|| SystemIndex::new("by_storage_id", [&FILE_STORAGE_ID_FIELD]).unwrap());

pub struct FileStorageTable;
impl SystemTable for FileStorageTable {
    type Metadata = FileStorageEntry;

    const TABLE_NAME: TableName = FILE_STORAGE_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![FILE_STORAGE_ID_INDEX.clone()]
    }

    fn virtual_table() -> Option<AssociatedVirtualTable> {
        Some(AssociatedVirtualTable::Primary {
            virtual_table_name: FILE_STORAGE_VIRTUAL_TABLE,
            virtual_to_system_indexes: ordmap! {
                FILE_STORAGE_VIRTUAL_INDEX_BY_CREATION_TIME =>
                    FILE_STORAGE_INDEX_BY_CREATION_TIME,
                FILE_STORAGE_VIRTUAL_INDEX_BY_ID =>
                    FILE_STORAGE_INDEX_BY_ID
            },
            doc_mapper: Arc::new(FileStorageDocMapper),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, derive_more::Display)]
pub enum FileStorageId {
    LegacyStorageId(StorageUuid),
    DocumentId(DeveloperDocumentId),
}

impl FromStr for FileStorageId {
    type Err = anyhow::Error;

    fn from_str(storage_id: &str) -> Result<Self, Self::Err> {
        let decoded_id = DeveloperDocumentId::decode(storage_id);
        match decoded_id {
            Ok(decoded_id) => Ok(FileStorageId::DocumentId(decoded_id)),
            Err(_) => Ok(FileStorageId::LegacyStorageId(storage_id.parse().context(
                ErrorMetadata::bad_request(
                    "InvalidStorageId",
                    format!(
                        "Invalid storage ID: \"{storage_id}\". Storage ID should be an
                    Id of '_storage' table, or a UUID string.",
                    ),
                ),
            )?)),
        }
    }
}

impl TryFrom<FileStorageIdProto> for FileStorageId {
    type Error = anyhow::Error;

    fn try_from(storage_id: FileStorageIdProto) -> Result<Self, Self::Error> {
        let storage_id = match storage_id.storage_id_type {
            Some(FileStorageIdTypeProto::LegacyStorageId(storage_id)) => {
                FileStorageId::LegacyStorageId(storage_id.parse()?)
            },
            Some(FileStorageIdTypeProto::DocumentId(storage_id)) => {
                FileStorageId::DocumentId(DeveloperDocumentId::decode(storage_id.as_str())?)
            },
            None => anyhow::bail!("Missing `storage_id_type` field"),
        };
        Ok(storage_id)
    }
}

impl From<FileStorageId> for FileStorageIdProto {
    fn from(storage_id: FileStorageId) -> Self {
        let storage_id_type = match storage_id {
            FileStorageId::LegacyStorageId(storage_id) => {
                FileStorageIdTypeProto::LegacyStorageId(storage_id.to_string())
            },
            FileStorageId::DocumentId(storage_id) => {
                FileStorageIdTypeProto::DocumentId(storage_id.encode())
            },
        };
        FileStorageIdProto {
            storage_id_type: Some(storage_id_type),
        }
    }
}

pub struct FileStorageModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
    namespace: TableNamespace,
}

impl<'a, RT: Runtime> FileStorageModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>, namespace: TableNamespace) -> Self {
        Self { tx, namespace }
    }

    pub async fn store_file(
        &mut self,
        entry: FileStorageEntry,
    ) -> anyhow::Result<ResolvedDocumentId> {
        // Call insert_metadata rather than insert because we already
        // did access check on `identity` rather than `self.identity`
        SystemMetadataModel::new(self.tx, self.namespace)
            .insert_metadata(&FILE_STORAGE_TABLE, entry.try_into()?)
            .await
    }

    pub async fn get_file(
        &mut self,
        storage_id: FileStorageId,
    ) -> anyhow::Result<Option<ParsedDocument<FileStorageEntry>>> {
        self.get_file_batch(btreemap! {0 => storage_id})
            .await
            .remove(&0)
            .context("batch_key missing")?
    }

    pub async fn get_file_batch(
        &mut self,
        storage_ids: BTreeMap<BatchKey, FileStorageId>,
    ) -> BTreeMap<BatchKey, anyhow::Result<Option<ParsedDocument<FileStorageEntry>>>> {
        let batch_size = storage_ids.len();
        let mut results = BTreeMap::new();
        let mut queries = BTreeMap::new();
        for (batch_key, storage_id) in storage_ids {
            match self.query_for_storage_id(storage_id) {
                Ok(query) => {
                    queries.insert(batch_key, query);
                },
                Err(e) => {
                    results.insert(batch_key, Err(e));
                },
            }
        }
        let queries_to_fetch = queries
            .iter_mut()
            .map(|(batch_key, query)| (*batch_key, (query, Some(1))))
            .collect();
        for (batch_key, fetch_result) in resolved_query_batch_next(queries_to_fetch, self.tx).await
        {
            let parsed_result = match fetch_result {
                Err(e) => Err(e),
                Ok(None) => Ok(None),
                Ok(Some((doc, _))) => ParseDocument::parse(doc).map(Some),
            };
            results.insert(batch_key, parsed_result);
        }
        assert_eq!(results.len(), batch_size);
        results
    }

    fn query_for_storage_id(
        &mut self,
        storage_id: FileStorageId,
    ) -> anyhow::Result<ResolvedQuery<RT>> {
        let index_query = match storage_id {
            FileStorageId::LegacyStorageId(storage_id) => Query::index_range(IndexRange {
                index_name: FILE_STORAGE_ID_INDEX.name(),
                range: vec![IndexRangeExpression::Eq(
                    FILE_STORAGE_ID_FIELD.clone(),
                    ConvexValue::try_from(storage_id)?.into(),
                )],
                order: Order::Asc,
            }),
            FileStorageId::DocumentId(document_id) => {
                let table_name = self
                    .tx
                    .resolve_idv6(
                        document_id,
                        self.namespace,
                        TableFilter::ExcludePrivateSystemTables,
                    )
                    .context(ErrorMetadata::bad_request(
                        "InvalidArgument",
                        "Invalid storage ID. Storage ID cannot be an ID on any table other than \
                         '_storage'."
                            .to_string(),
                    ))?;
                anyhow::ensure!(
                    table_name == FILE_STORAGE_VIRTUAL_TABLE,
                    ErrorMetadata::bad_request(
                        "InvalidArgument",
                        "Invalid storage ID. Storage ID cannot be an ID on any table other than \
                         '_storage'."
                            .to_string(),
                    )
                );
                let table_mapping = self.tx.table_mapping().clone();
                let document_id = self
                    .tx
                    .virtual_system_mapping()
                    .virtual_id_v6_to_system_resolved_doc_id(
                        self.namespace,
                        &document_id,
                        &table_mapping,
                    )?;
                Query::get(FILE_STORAGE_TABLE.clone(), document_id.into())
            },
        };
        ResolvedQuery::new(self.tx, self.namespace, index_query)
    }

    pub async fn delete_file(
        &mut self,
        storage_id: FileStorageId,
        identity: Identity,
    ) -> anyhow::Result<Option<ParsedDocument<FileStorageEntry>>> {
        // We only expect this function to be called by the framework as part
        // of a storage syscall. We require passing in a system identity to confirm
        // that the caller isn't letting a user call this directly
        if !identity.is_system() {
            anyhow::bail!(unauthorized_error("delete_file"))
        }
        let Some(entry) = self.get_file(storage_id).await? else {
            return Ok(None);
        };
        SystemMetadataModel::new(self.tx, self.namespace)
            .delete(entry.id())
            .await?;
        Ok(Some(entry))
    }

    pub async fn get_total_storage_count(&mut self) -> anyhow::Result<u64> {
        TableModel::new(self.tx)
            .must_count(self.namespace, &FILE_STORAGE_TABLE.clone())
            .await
    }
}

/// The `_file_storage` tablets to iterate, mapped to their `by_id` index id.
async fn file_storage_target_tables<RT: Runtime>(
    identity: &Identity,
    snapshot: &DatabaseSnapshot<RT>,
) -> anyhow::Result<BTreeMap<TabletId, IndexRef>> {
    let mut tx = snapshot.begin_tx(
        identity.clone(),
        Arc::new(SearchNotEnabled),
        FunctionUsageTracker::new(),
        virtual_system_mapping().clone(),
    )?;
    let by_id_indexes = IndexModel::new(&mut tx).by_id_indexes().await?;
    let table_mapping = tx.table_mapping();
    table_mapping
        .iter()
        .filter(|(tablet_id, _, _, table_name)| {
            **table_name == FILE_STORAGE_TABLE && table_mapping.is_active(*tablet_id)
        })
        .map(|(tablet_id, ..)| {
            anyhow::Ok((
                tablet_id,
                *by_id_indexes
                    .get(&tablet_id)
                    .context("_file_storage by_id index not found")?,
            ))
        })
        .try_collect()
}

/// The deployment's total `_file_storage` document size, maintained
/// incrementally across calls to [`Self::total_size`]. The first call syncs
/// every `_file_storage` document; later ones resume from the retained cursor
/// and read only the document log written since, unless the storage tables are
/// small enough ([`FILE_STORAGE_SIZE_MIN_DOCUMENTS_TO_RESUME`]) that syncing
/// them again is cheaper.
pub struct FileStorageSizeTracker<RT: Runtime> {
    iterator: DataSyncIterator<RT>,
    synced: Option<SyncedTotals>,
    min_documents_to_resume: i64,
}

/// A completed sync: the totals cover every document captured by `cursor`,
/// which is at a consistent snapshot.
struct SyncedTotals {
    cursor: DataSyncCursor,
    /// Accumulated based on deltas to the table, hence signed.
    size: i64,
    /// Documents in the synced tables, accumulated the same way.
    num_documents: i64,
}

impl<RT: Runtime> FileStorageSizeTracker<RT> {
    pub fn new(iterator: DataSyncIterator<RT>) -> Self {
        Self {
            iterator,
            synced: None,
            min_documents_to_resume: *FILE_STORAGE_SIZE_MIN_DOCUMENTS_TO_RESUME,
        }
    }

    /// Total size of all `_file_storage` documents across the deployment, at
    /// the consistent snapshot the iterator picks at the end of the sync.
    /// `snapshot` supplies the set of `_file_storage` tablets to sync, not the
    /// timestamp read at.
    #[fastrace::trace]
    pub async fn total_size(
        &mut self,
        identity: &Identity,
        snapshot: DatabaseSnapshot<RT>,
    ) -> anyhow::Result<u64> {
        let target_tables = file_storage_target_tables(identity, &snapshot).await?;
        drop(snapshot);
        // Only resume when every table counted in the totals is still a target:
        // a dropped table's documents are baked into them, and the iterator
        // will never emit them again for us to subtract back out. Added tables
        // are fine — the iterator walks them in the `by_id` dimension and we
        // add their documents.
        let resumable = self.synced.take().filter(|synced| {
            synced.num_documents >= self.min_documents_to_resume
                && synced
                    .cursor
                    .synced_tables()
                    .iter()
                    .all(|tablet| target_tables.contains_key(tablet))
        });
        let synced = match resumable {
            None => sync_totals(&self.iterator, None, &target_tables).await?,
            Some(resumable) => {
                match sync_totals(&self.iterator, Some(resumable), &target_tables).await {
                    Ok(synced) => synced,
                    Err(e) => {
                        // A retained cursor has its own failure modes, notably
                        // `synced_ts` falling out of document retention. Start
                        // over rather than failing every subsequent call too.
                        report_error(&mut e.context("Resuming file storage size sync failed"))
                            .await;
                        sync_totals(&self.iterator, None, &target_tables).await?
                    },
                }
            },
        };
        // A negative size is not possible. `self.synced` stays empty, so the
        // next call starts over.
        let size = u64::try_from(synced.size).map_err(|_| {
            anyhow::anyhow!(
                "DataSyncIterator returned negative file storage total {}",
                synced.size
            )
        })?;
        self.synced = Some(synced);
        Ok(size)
    }

}

/// Drive the iterator until it is caught up, accumulating deltas: add each
/// emitted revision and subtract its predecessor, which the iterator supplies
/// for every re-emitted document. Memory stays constant rather than
/// materializing a per-document size map.
async fn sync_totals<RT: Runtime>(
    iterator: &DataSyncIterator<RT>,
    resume: Option<SyncedTotals>,
    target_tables: &BTreeMap<TabletId, IndexRef>,
) -> anyhow::Result<SyncedTotals> {
    let (mut cursor, mut size, mut num_documents) = match resume {
        Some(SyncedTotals {
            cursor,
            size,
            num_documents,
        }) => (Some(cursor), size, num_documents),
        None => (None, 0, 0),
    };
    let cursor = loop {
        let page = iterator
            .next_page_with_prev_revs(cursor, target_tables)
            .await?;
        for entry in page.entries {
            if let Some(value) = entry.log_entry.value {
                let storage_entry: ParsedDocument<FileStorageEntry> = value.parse()?;
                size += storage_entry.size;
                num_documents += 1;
            }
            if let Some(prev_rev) = entry.prev_rev {
                let prev_entry: ParsedDocument<FileStorageEntry> = prev_rev.parse()?;
                size -= prev_entry.size;
                num_documents -= 1;
            }
        }
        match page.status {
            DataSyncStatus::UpToDate { .. } => break page.cursor,
            // `Stale` is a consistent snapshot too, but behind the latest
            // repeatable timestamp — keep paging so the totals don't fall
            // further behind on every call.
            DataSyncStatus::Stale { .. } | DataSyncStatus::Snapshotting { .. } => {},
        }
        cursor = Some(page.cursor);
    };
    Ok(SyncedTotals {
        cursor,
        size,
        num_documents,
    })
}
