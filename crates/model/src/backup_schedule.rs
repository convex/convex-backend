use std::sync::Arc;

use anyhow::Context;
use common::{
    cron::next_occurrence_after,
    document::ParsedDocument,
    runtime::Runtime,
};
use database::{
    SystemMetadataModel,
    Transaction,
};
use saffron::Cron;
use serde::{
    Deserialize,
    Serialize,
};
use sync_types::Timestamp;
use value::{
    codegen_convex_serialization,
    TableName,
    TableNamespace,
};

use crate::{
    SystemIndex,
    SystemTable,
};

pub const BACKUP_SCHEDULE_TABLE: TableName = TableName::const_new("_backup_schedule");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupSchedule {
    pub cronspec: String,
    pub expiration_delta_seconds: i64,
    pub include_storage: bool,
    pub next_run: Timestamp,
}

impl BackupSchedule {
    pub fn next_run_after(&self, now: Timestamp) -> anyhow::Result<Timestamp> {
        let cron: Cron = self.cronspec.parse().context("Invalid backup cronspec")?;
        next_occurrence_after(&cron, now)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedBackupSchedule {
    cronspec: String,
    expiration_delta_seconds: i64,
    include_storage: bool,
    next_run: i64,
}

impl From<BackupSchedule> for SerializedBackupSchedule {
    fn from(schedule: BackupSchedule) -> Self {
        Self {
            cronspec: schedule.cronspec,
            expiration_delta_seconds: schedule.expiration_delta_seconds,
            include_storage: schedule.include_storage,
            next_run: schedule.next_run.into(),
        }
    }
}

impl TryFrom<SerializedBackupSchedule> for BackupSchedule {
    type Error = anyhow::Error;

    fn try_from(schedule: SerializedBackupSchedule) -> anyhow::Result<Self> {
        Ok(Self {
            cronspec: schedule.cronspec,
            expiration_delta_seconds: schedule.expiration_delta_seconds,
            include_storage: schedule.include_storage,
            next_run: schedule.next_run.try_into()?,
        })
    }
}

codegen_convex_serialization!(BackupSchedule, SerializedBackupSchedule);

pub struct BackupScheduleTable;

impl SystemTable for BackupScheduleTable {
    type Metadata = BackupSchedule;

    const TABLE_NAME: TableName = BACKUP_SCHEDULE_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub struct BackupScheduleModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> BackupScheduleModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    pub async fn get(&mut self) -> anyhow::Result<Option<Arc<ParsedDocument<BackupSchedule>>>> {
        self.tx
            .query_system(
                TableNamespace::Global,
                &SystemIndex::<BackupScheduleTable>::by_id(),
            )?
            .unique()
            .await
    }

    pub async fn set(&mut self, schedule: BackupSchedule) -> anyhow::Result<()> {
        let existing = self.get().await?;
        let mut metadata = SystemMetadataModel::new_global(self.tx);
        match existing {
            Some(document) => {
                metadata
                    .replace(document.id(), schedule.try_into()?)
                    .await?;
            },
            None => {
                metadata
                    .insert(&BACKUP_SCHEDULE_TABLE, schedule.try_into()?)
                    .await?;
            },
        }
        Ok(())
    }

    pub async fn delete(&mut self) -> anyhow::Result<()> {
        if let Some(document) = self.get().await? {
            SystemMetadataModel::new_global(self.tx)
                .delete(document.id())
                .await?;
        }
        Ok(())
    }
}
