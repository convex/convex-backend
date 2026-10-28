pub mod types;

use std::sync::Arc;

use anyhow::Context;
use common::{
    runtime::Runtime,
    types::PersistenceIndexId,
};
use value::{
    TableName,
    TableNamespace,
    TableNumber,
};

use self::types::NextPersistenceIndexIdMetadata;
use crate::{
    system_tables::{
        SystemIndex,
        SystemTable,
    },
    SystemMetadataModel,
    Transaction,
};

pub const NEXT_PERSISTENCE_INDEX_ID_TABLE: TableName =
    TableName::const_new("_next_persistence_index_id");

pub struct NextPersistenceIndexIdTable;

impl NextPersistenceIndexIdTable {
    pub async fn initialize<RT: Runtime>(
        tx: &mut Transaction<RT>,
        default_table_number: Option<TableNumber>,
    ) -> anyhow::Result<bool> {
        let is_new = tx
            .create_system_table(
                TableNamespace::Global,
                &NEXT_PERSISTENCE_INDEX_ID_TABLE,
                default_table_number,
            )
            .await?;
        if is_new {
            NextPersistenceIndexIdModel::new(tx).initialize().await?;
        }
        Ok(is_new)
    }
}

impl SystemTable for NextPersistenceIndexIdTable {
    type Metadata = NextPersistenceIndexIdMetadata;

    const TABLE_NAME: TableName = NEXT_PERSISTENCE_INDEX_ID_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub(crate) struct NextPersistenceIndexIdModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> NextPersistenceIndexIdModel<'a, RT> {
    pub(crate) fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    async fn initialize(&mut self) -> anyhow::Result<()> {
        SystemMetadataModel::new_global(self.tx)
            .insert(
                &NEXT_PERSISTENCE_INDEX_ID_TABLE,
                NextPersistenceIndexIdMetadata {
                    next_id: PersistenceIndexId::FIRST,
                }
                .try_into()?,
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn allocate(
        &mut self,
        count: usize,
    ) -> anyhow::Result<Vec<PersistenceIndexId>> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let count = u32::try_from(count).context("too many persistence index IDs requested")?;
        let next_id_document = self
            .tx
            .query_system(
                TableNamespace::Global,
                &SystemIndex::<NextPersistenceIndexIdTable>::by_id(),
            )?
            .unique()
            .await?
            .map(Arc::unwrap_or_clone)
            .context("next persistence index ID is not initialized")?;
        let first_id = next_id_document.next_id;
        let next_id = PersistenceIndexId::new(
            first_id
                .value()
                .checked_add(count)
                .context("exhausted persistence index IDs")?,
        )
        .expect("adding a positive count to a nonzero ID cannot produce zero");
        SystemMetadataModel::new_global(self.tx)
            .replace(
                next_id_document.id(),
                NextPersistenceIndexIdMetadata { next_id }.try_into()?,
            )
            .await?;
        Ok((first_id.value()..next_id.value())
            .map(|id| {
                PersistenceIndexId::new(id)
                    .expect("persistence index ID allocation starts at a nonzero value")
            })
            .collect())
    }
}
