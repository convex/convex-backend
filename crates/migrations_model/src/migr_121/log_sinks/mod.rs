use common::{
    self,
    document::ParsedDocument,
    runtime::Runtime,
};
use database::{
    system_tables::{
        SystemIndex,
        SystemTable,
    },
    Transaction,
};
use value::{
    TableName,
    TableNamespace,
};

pub mod types;
use types::LogSinksRow;

pub const LOG_SINKS_TABLE: TableName = TableName::const_new("_log_sinks");

pub struct LogSinksTable;
impl SystemTable for LogSinksTable {
    type Metadata = LogSinksRow;

    const TABLE_NAME: TableName = LOG_SINKS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![]
    }
}

pub struct LogSinksModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> LogSinksModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    pub async fn get_all(&mut self) -> anyhow::Result<Vec<ParsedDocument<LogSinksRow>>> {
        let result = self
            .tx
            .query_system(
                TableNamespace::Global,
                &SystemIndex::<LogSinksTable>::by_id(),
            )?
            .all()
            .await?
            .into_iter()
            .map(|arc_row| (*arc_row).clone())
            .collect();
        Ok(result)
    }
}
