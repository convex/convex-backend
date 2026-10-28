use common::virtual_system_mapping::AssociatedVirtualTable;
use database::system_tables::{
    SystemIndex,
    SystemTable,
};
use value::TableName;

use crate::scheduled_jobs::{
    types::ScheduledJobArgs,
    SCHEDULED_JOBS_VIRTUAL_TABLE,
};

pub const SCHEDULED_JOBS_ARGS_TABLE: TableName = TableName::const_new("_scheduled_job_args");

pub struct ScheduledJobArgsTable;

impl SystemTable for ScheduledJobArgsTable {
    type Metadata = ScheduledJobArgs;

    const TABLE_NAME: TableName = SCHEDULED_JOBS_ARGS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }

    fn virtual_table() -> Option<AssociatedVirtualTable> {
        Some(AssociatedVirtualTable::Secondary(
            SCHEDULED_JOBS_VIRTUAL_TABLE.clone(),
        ))
    }
}
