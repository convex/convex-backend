use common::{
    document::ParsedDocument,
    runtime::Runtime,
    types::{
        BackendState,
        SystemStopState,
        UsageLimitStopState,
        UserStopState,
    },
};
use database::{
    SystemMetadataModel,
    Transaction,
};
use value::{
    TableName,
    TableNamespace,
};

use crate::{
    SystemIndex,
    SystemTable,
};

pub mod types;

use self::types::PersistedBackendState;

pub const BACKEND_STATE_TABLE: TableName = TableName::const_new("_backend_state");

pub struct BackendStateTable;
impl SystemTable for BackendStateTable {
    type Metadata = PersistedBackendState;

    const TABLE_NAME: TableName = BACKEND_STATE_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub struct BackendStateModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> BackendStateModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    pub async fn initialize(&mut self) -> anyhow::Result<()> {
        SystemMetadataModel::new_global(self.tx)
            .insert(
                &BACKEND_STATE_TABLE,
                PersistedBackendState(BackendState {
                    system: SystemStopState::None,
                    usage_limit: UsageLimitStopState::None,
                    user: UserStopState::None,
                })
                .try_into()?,
            )
            .await?;
        Ok(())
    }

    pub async fn get_backend_state(&mut self) -> anyhow::Result<ParsedDocument<BackendState>> {
        let backend_state = self
            .tx
            .query_system(
                TableNamespace::Global,
                &SystemIndex::<BackendStateTable>::by_id(),
            )?
            .unique()
            .await?
            .ok_or_else(|| anyhow::anyhow!("Backend must have a state."))?;
        (*backend_state).clone().map(|bs| Ok(bs.0))
    }

    pub async fn set_user_stop_state(
        &mut self,
        new_user_state: UserStopState,
    ) -> anyhow::Result<Option<BackendState>> {
        let (id, mut current) = self.get_backend_state().await?.into_id_and_value();
        if current.user == new_user_state {
            return Ok(None);
        }
        let old = current;
        current.user = new_user_state;
        SystemMetadataModel::new_global(self.tx)
            .replace(id, PersistedBackendState(current).try_into()?)
            .await?;
        Ok(Some(old))
    }

    pub async fn set_system_stop_state(
        &mut self,
        new_system_state: SystemStopState,
    ) -> anyhow::Result<Option<BackendState>> {
        let (id, mut current) = self.get_backend_state().await?.into_id_and_value();
        if current.system == new_system_state {
            return Ok(None);
        }
        let old = current;
        current.system = new_system_state;
        SystemMetadataModel::new_global(self.tx)
            .replace(id, PersistedBackendState(current).try_into()?)
            .await?;
        Ok(Some(old))
    }

    pub async fn set_usage_limit_stop_state(
        &mut self,
        new_usage_limit_state: UsageLimitStopState,
    ) -> anyhow::Result<Option<BackendState>> {
        let (id, mut current) = self.get_backend_state().await?.into_id_and_value();
        if current.usage_limit == new_usage_limit_state {
            return Ok(None);
        }
        let old = current;
        current.usage_limit = new_usage_limit_state;
        SystemMetadataModel::new_global(self.tx)
            .replace(id, PersistedBackendState(current).try_into()?)
            .await?;
        Ok(Some(old))
    }

}
