//! Frozen copy of the persistence index ID allocator as of migration 130.
//!
//! Only the singleton type/table definition and `allocate` are frozen; the
//! read/advance/write still runs against the live `database` infrastructure.

pub mod types;

use std::sync::Arc;

use anyhow::Context;
use common::{
    runtime::Runtime,
    types::PersistenceIndexId,
};
use database::{
    system_tables::{
        SystemIndex,
        SystemTable,
    },
    SystemMetadataModel,
    Transaction,
};
use value::{
    TableName,
    TableNamespace,
};

use self::types::NextPersistenceIndexIdMetadata;

pub struct NextPersistenceIndexIdTable;

impl SystemTable for NextPersistenceIndexIdTable {
    type Metadata = NextPersistenceIndexIdMetadata;

    const FOR_MIGRATION: bool = true;
    const TABLE_NAME: TableName = TableName::const_new("_next_persistence_index_id");

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub struct NextPersistenceIndexIdModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> NextPersistenceIndexIdModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    /// Reserves the next `count` persistence index IDs by advancing the
    /// singleton allocator, returning the reserved block.
    pub async fn allocate(&mut self, count: usize) -> anyhow::Result<Vec<PersistenceIndexId>> {
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
