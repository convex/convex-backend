use common::{
    document::{
        ParseDocument,
        ParsedDocument,
    },
    runtime::Runtime,
};
use database::{
    system_tables::SystemIndex,
    SystemMetadataModel,
    Transaction,
};
use value::TableNamespace;

use crate::migr_125::backend_state::{
    types::{
        BackendState,
        OldBackendState,
        PersistedBackendState,
        SystemStopState,
        UserStopState,
    },
    BackendStateTable,
};

mod backend_state;

pub async fn run_migration<RT: Runtime>(tx: &mut Transaction<RT>) -> anyhow::Result<()> {
    let row = tx
        .query_system(
            TableNamespace::Global,
            &SystemIndex::<BackendStateTable>::by_id(),
        )?
        .unique()
        .await?
        .ok_or_else(|| anyhow::anyhow!("Backend must have a state."))?;
    let parsed: ParsedDocument<PersistedBackendState> = (*row).clone().parse()?;
    let (id, persisted) = parsed.into_id_and_value();

    match persisted {
        PersistedBackendState::New(_) => {
            // Already migrated, no-op
        },
        PersistedBackendState::Old(old) => {
            let new_state = match old {
                OldBackendState::Disabled => BackendState {
                    system: SystemStopState::Disabled,
                    user: UserStopState::None,
                },
                OldBackendState::Paused => BackendState {
                    system: SystemStopState::None,
                    user: UserStopState::Paused,
                },
                OldBackendState::Running => BackendState {
                    system: SystemStopState::None,
                    user: UserStopState::None,
                },
                OldBackendState::Suspended => BackendState {
                    system: SystemStopState::Suspended,
                    user: UserStopState::None,
                },
            };
            SystemMetadataModel::new_global(tx)
                .replace(id, PersistedBackendState::New(new_state).try_into()?)
                .await?;
        },
    }
    Ok(())
}
