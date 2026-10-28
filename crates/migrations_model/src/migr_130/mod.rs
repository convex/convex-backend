use std::{
    ops::Bound,
    sync::Arc,
};

use common::runtime::Runtime;
use database::{
    system_tables::SystemIndex,
    SystemMetadataModel,
    Transaction,
};
use value::{
    DeveloperDocumentId,
    TableNamespace,
};

use self::{
    index::{
        IndexTable,
        SerializedIndexConfig,
    },
    next_persistence_index_id::NextPersistenceIndexIdModel,
};

mod index;
mod next_persistence_index_id;

/// Max `_index` documents scanned per batch. This bounds both reads and writes
/// per transaction: at most this many documents are read, and since only a
/// subset are patched, at most this many system writes occur — keeping every
/// batch under `TRANSACTION_MAX_READ_SIZE_ROWS` (32000) and
/// `TRANSACTION_MAX_SYSTEM_NUM_WRITES` (40000).
const BATCH_SIZE: usize = 1000;

/// Progress from one [`run_migration`] batch.
pub struct BatchProgress {
    /// How many indexes this batch patched (0 if the batch only scanned
    /// non-database or already-assigned indexes).
    pub patched: usize,
    /// The last `_index` document scanned, to resume the next batch after — or
    /// `None` once the scan reaches the end of `_index` (migration complete).
    pub cursor: Option<DeveloperDocumentId>,
}

/// Scans up to [`BATCH_SIZE`] `_index` documents starting after `cursor`,
/// assigning a persistence index ID to each database index still missing one.
///
/// The migration arm reruns this — committing between batches and threading the
/// returned cursor back in — so `_index` is scanned exactly once (rather than
/// re-scanned from the start each batch) and every transaction stays bounded.
pub async fn run_migration<RT: Runtime>(
    tx: &mut Transaction<RT>,
    cursor: Option<DeveloperDocumentId>,
) -> anyhow::Result<BatchProgress> {
    // Scan `_index` by id (ascending, deterministic), resuming strictly after
    // `cursor` so documents from earlier batches aren't reread.
    let by_id = SystemIndex::<IndexTable>::by_id();
    let mut indexes = match cursor {
        Some(cursor) => tx
            .query_system(TableNamespace::Global, &by_id)?
            .range((
                Bound::Excluded([cursor.encode_into(&mut Default::default())]),
                Bound::Unbounded,
            ))?
            .build(),
        None => tx.query_system(TableNamespace::Global, &by_id)?.build(),
    };

    let mut to_patch = Vec::new();
    let mut next_cursor = None;
    let mut scanned = 0;
    while let Some(document) = indexes.next().await? {
        let (id, metadata) = Arc::unwrap_or_clone(document).into_id_and_value();
        let dev_id = id.developer_id;
        if matches!(
            metadata.config,
            SerializedIndexConfig::Database {
                persistence_index_id: None,
                ..
            }
        ) {
            to_patch.push((id, metadata));
        }
        scanned += 1;
        if scanned >= BATCH_SIZE {
            next_cursor = Some(dev_id);
            break;
        }
    }

    let patched = to_patch.len();
    let persistence_index_ids = NextPersistenceIndexIdModel::new(tx)
        .allocate(patched)
        .await?;
    for ((id, mut metadata), persistence_index_id) in
        to_patch.into_iter().zip(persistence_index_ids)
    {
        let SerializedIndexConfig::Database {
            persistence_index_id: slot,
            ..
        } = &mut metadata.config
        else {
            unreachable!("only database indexes missing an ID are queued for patching");
        };
        *slot = Some(i64::from(persistence_index_id.value()));
        SystemMetadataModel::new_global(tx)
            .replace(id, metadata.try_into()?)
            .await?;
    }
    Ok(BatchProgress {
        patched,
        cursor: next_cursor,
    })
}
