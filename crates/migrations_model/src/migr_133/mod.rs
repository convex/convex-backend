use common::{
    runtime::Runtime,
    types::{
        IndexDescriptor,
        IndexName,
    },
};
use database::{
    system_tables::{
        SystemIndex,
        SystemTable,
    },
    IndexModel,
    SystemMetadataModel,
    Transaction,
};

mod progress;
use progress::ProgressTable;

/// Aggregate counters are disposable; ordinary validation restarts its walks.
/// The `by_schema_id` index that looked them up goes with them.
///
/// A single transaction suffices: the legacy writer kept one aggregate
/// document per schema and deleted it once the schema resolved, and a
/// namespace has at most one `Pending` and one `Validated` schema, so each
/// namespace holds at most a couple of these documents.
pub async fn run_migration<RT: Runtime>(tx: &mut Transaction<RT>) -> anyhow::Result<()> {
    let legacy_index = IndexName::new(
        ProgressTable::TABLE_NAME,
        IndexDescriptor::new("by_schema_id")?,
    )?;
    for namespace in tx
        .table_mapping()
        .namespaces_for_name(&ProgressTable::TABLE_NAME)
    {
        let all_progress_docs = tx
            .query_system(namespace, &SystemIndex::<ProgressTable>::by_id())?
            .all()
            .await?;
        for doc in all_progress_docs {
            if doc.validation_id.is_none() {
                SystemMetadataModel::new(tx, namespace)
                    .delete(doc.id())
                    .await?;
            }
        }
        IndexModel::new(tx)
            .drop_system_index(namespace, legacy_index.clone())
            .await?;
    }
    Ok(())
}
