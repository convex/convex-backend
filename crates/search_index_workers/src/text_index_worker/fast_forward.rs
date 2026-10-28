use async_trait::async_trait;
use common::{
    bootstrap_model::index::{
        text_index::{
            TextIndexSnapshot,
            TextIndexState,
            TextSnapshotVersion,
        },
        IndexConfig,
    },
    document::ParsedDocument,
    runtime::Runtime,
    types::IndexId,
};
use database::{
    IndexWorkerMetadataModel,
    IndexWorkerMetadataRecord,
    Snapshot,
    Transaction,
};
use sync_types::Timestamp;

use crate::fast_forward::IndexFastForward;

pub struct TextFastForward;

#[async_trait]
impl<RT: Runtime> IndexFastForward<RT, TextSnapshotVersion> for TextFastForward {
    fn current_version(_tx: &mut Transaction<RT>) -> TextSnapshotVersion {
        TextSnapshotVersion::current()
    }

    fn snapshot_info(config: &IndexConfig) -> Option<(Timestamp, TextSnapshotVersion)> {
        let IndexConfig::Text { on_disk_state, .. } = config else {
            return None;
        };
        let TextIndexSnapshot { ts, version, .. } = match on_disk_state {
            TextIndexState::SnapshottedAt(snapshot)
            | TextIndexState::Backfilled { snapshot, .. } => snapshot,
            TextIndexState::Backfilling(_) => return None,
        };
        Some((*ts, *version))
    }

    async fn get_or_create_worker_meta(
        mut model: IndexWorkerMetadataModel<'_, RT>,
        id: IndexId,
    ) -> anyhow::Result<ParsedDocument<IndexWorkerMetadataRecord>> {
        model.get_or_create_text_search(id).await
    }

    fn num_transactions(snapshot: Snapshot, index_id: IndexId) -> anyhow::Result<Option<usize>> {
        snapshot.text_indexes.num_transactions(index_id)
    }
}
