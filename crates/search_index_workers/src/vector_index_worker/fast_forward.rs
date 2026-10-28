use async_trait::async_trait;
use common::{
    bootstrap_model::index::{
        vector_index::{
            VectorIndexSnapshot,
            VectorIndexState,
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

pub struct VectorFastForward;

#[async_trait]
impl<RT: Runtime> IndexFastForward<RT, ()> for VectorFastForward {
    // We have exactly one version of vector metadata right now, so there's nothing
    // to compare against.
    fn current_version(_: &mut Transaction<RT>) {}

    fn snapshot_info(config: &IndexConfig) -> Option<(Timestamp, ())> {
        let IndexConfig::Vector { on_disk_state, .. } = config else {
            return None;
        };
        let VectorIndexSnapshot { ts, .. } = match on_disk_state {
            VectorIndexState::SnapshottedAt(snapshot)
            | VectorIndexState::Backfilled { snapshot, .. } => snapshot,
            VectorIndexState::Backfilling(_) => return None,
        };
        Some((*ts, ()))
    }

    async fn get_or_create_worker_meta(
        mut model: IndexWorkerMetadataModel<'_, RT>,
        index_id: IndexId,
    ) -> anyhow::Result<ParsedDocument<IndexWorkerMetadataRecord>> {
        model.get_or_create_vector_search(index_id).await
    }

    fn num_transactions(snapshot: Snapshot, index_id: IndexId) -> anyhow::Result<Option<usize>> {
        snapshot.vector_indexes.num_transactions(index_id)
    }
}
