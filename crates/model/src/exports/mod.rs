use std::{
    sync::LazyLock,
    time::Duration,
};

use anyhow::Context;
use common::{
    components::ComponentId,
    document::{
        ParseDocument,
        ParsedDocument,
        CREATION_TIME_FIELD_PATH,
    },
    maybe_val,
    query::{
        Expression,
        IndexRange,
        IndexRangeExpression,
        Order,
        Query,
    },
    runtime::Runtime,
    types::ObjectKey,
};
use database::{
    ResolvedQuery,
    SystemMetadataModel,
    Transaction,
};
use sync_types::Timestamp;
use types::{
    ExportFormat,
    ExportRequestor,
};
use value::{
    ConvexValue,
    DeveloperDocumentId,
    FieldPath,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

use self::types::Export;
use crate::{
    SystemIndex,
    SystemTable,
};

pub mod types;

pub const EXPORTS_TABLE: TableName = TableName::const_new("_exports");

pub static EXPORTS_BY_STATE_AND_TS_INDEX: LazyLock<SystemIndex<ExportsTable>> =
    LazyLock::new(|| {
        SystemIndex::new("by_state_and_ts", [&EXPORTS_STATE_FIELD, &EXPORTS_TS_FIELD]).unwrap()
    });

pub static EXPORTS_BY_REQUESTOR: LazyLock<SystemIndex<ExportsTable>> = LazyLock::new(|| {
    SystemIndex::new(
        "by_requestor",
        [&EXPORTS_REQUESTOR_FIELD, &CREATION_TIME_FIELD_PATH],
    )
    .unwrap()
});

pub static EXPORTS_STATE_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "state".parse().expect("Invalid built-in field"));

pub static EXPORTS_TS_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "start_ts".parse().expect("Invalid built-in field"));

pub static EXPORTS_EXPIRATION_TS_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "expiration_ts".parse().expect("Invalid built-in field"));

static EXPORTS_REQUESTOR_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "requestor".parse().expect("Invalid built-in field"));

const DEFAULT_EXPORT_RETENTION: u64 = Duration::from_days(14).as_nanos() as u64;

pub struct ExportsTable;
impl SystemTable for ExportsTable {
    type Metadata = Export;

    const TABLE_NAME: TableName = EXPORTS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![
            EXPORTS_BY_STATE_AND_TS_INDEX.clone(),
            EXPORTS_BY_REQUESTOR.clone(),
        ]
    }
}

pub struct ExportsModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> ExportsModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    pub async fn insert_requested(
        &mut self,
        format: ExportFormat,
        component: ComponentId,
        requestor: ExportRequestor,
        expiration_ts_ns: Option<u64>,
    ) -> anyhow::Result<ResolvedDocumentId> {
        let default_expiration_ts =
            u64::from(*self.tx.begin_timestamp()) + DEFAULT_EXPORT_RETENTION;
        let expiration_ts_ns = expiration_ts_ns.unwrap_or(default_expiration_ts);

        SystemMetadataModel::new_global(self.tx)
            .insert(
                &EXPORTS_TABLE,
                Export::requested(format, component, requestor, expiration_ts_ns).try_into()?,
            )
            .await
    }

    pub async fn list(&mut self) -> anyhow::Result<Vec<ParsedDocument<Export>>> {
        let result = self
            .tx
            .query_system(
                TableNamespace::Global,
                &SystemIndex::<ExportsTable>::by_id(),
            )?
            .order(Order::Asc)
            .all()
            .await?
            .into_iter()
            .map(|doc| (*doc).clone())
            .collect();
        Ok(result)
    }

    pub async fn list_unexpired_cloud_backups(
        &mut self,
    ) -> anyhow::Result<Vec<ParsedDocument<Export>>> {
        let index_range = IndexRange {
            index_name: EXPORTS_BY_REQUESTOR.name(),
            range: vec![IndexRangeExpression::Eq(
                EXPORTS_REQUESTOR_FIELD.clone(),
                ConvexValue::try_from(ExportRequestor::CloudBackup.to_string())?.into(),
            )],
            order: Order::Asc,
        };
        let completed_filter = Expression::Eq(
            Expression::Field(EXPORTS_STATE_FIELD.clone()).into(),
            Expression::Literal(maybe_val!("completed")).into(),
        );
        let expired_filter = Expression::Gt(
            Expression::Field(EXPORTS_EXPIRATION_TS_FIELD.clone()).into(),
            Expression::Literal(maybe_val!(i64::from(*self.tx.begin_timestamp()))).into(),
        );
        let query = Query::index_range(index_range)
            .filter(Expression::And(vec![completed_filter, expired_filter]));
        let mut query_stream = ResolvedQuery::new(self.tx, TableNamespace::Global, query)?;
        let mut result = vec![];
        while let Some(doc) = query_stream.next(self.tx, None).await? {
            let row: ParsedDocument<Export> = doc.parse()?;
            result.push(row);
        }
        Ok(result)
    }

    pub async fn latest_requested(&mut self) -> anyhow::Result<Option<ParsedDocument<Export>>> {
        self.export_in_state("requested").await
    }

    pub async fn latest_in_progress(&mut self) -> anyhow::Result<Option<ParsedDocument<Export>>> {
        self.export_in_state("in_progress").await
    }

    async fn export_in_state(
        &mut self,
        export_state: &str,
    ) -> anyhow::Result<Option<ParsedDocument<Export>>> {
        let export = self
            .tx
            .query_system(TableNamespace::Global, &*EXPORTS_BY_STATE_AND_TS_INDEX)?
            .eq(&[export_state])?
            .unique()
            .await?
            .map(|doc| (*doc).clone());
        Ok(export)
    }

    pub async fn completed_export_at_ts(
        &mut self,
        snapshot_ts: Timestamp,
    ) -> anyhow::Result<Option<ParsedDocument<Export>>> {
        let export = self
            .tx
            .query_system(TableNamespace::Global, &*EXPORTS_BY_STATE_AND_TS_INDEX)?
            .eq(&["completed"])?
            .eq(&[i64::from(snapshot_ts)])?
            .unique()
            .await?
            .map(|doc| (*doc).clone());
        Ok(export)
    }

    pub async fn get(
        &mut self,
        snapshot_id: DeveloperDocumentId,
    ) -> anyhow::Result<Option<ParsedDocument<Export>>> {
        let export = self
            .tx
            .get_system::<ExportsTable>(TableNamespace::Global, snapshot_id)
            .await?
            .map(|doc| (*doc).clone());
        Ok(export)
    }

    pub async fn set_expiration(
        &mut self,
        snapshot_id: DeveloperDocumentId,
        expiration_ts_ns: u64,
    ) -> anyhow::Result<()> {
        let (id, mut export) = self
            .get(snapshot_id)
            .await?
            .context("Snapshot not found")?
            .into_id_and_value();
        let Export::Completed { expiration_ts, .. } = &mut export else {
            anyhow::bail!("Can only set expiration on completed exports");
        };
        *expiration_ts = expiration_ts_ns;
        SystemMetadataModel::new_global(self.tx)
            .replace(id, export.try_into()?)
            .await?;
        Ok(())
    }

    pub async fn cleanup_expired(
        &mut self,
        retention_duration: Duration,
    ) -> anyhow::Result<Vec<ObjectKey>> {
        let delete_before_ts = (*self.tx.begin_timestamp()).sub(retention_duration)?;
        let mut to_delete = vec![];
        for export in self.list().await? {
            let (id, export) = export.into_id_and_value();
            match export {
                Export::Requested { .. } | Export::InProgress { .. } => continue,
                Export::Completed {
                    expiration_ts,
                    zip_object_key,
                    ..
                } => {
                    if expiration_ts < delete_before_ts.into() {
                        to_delete.push(zip_object_key);
                        SystemMetadataModel::new_global(self.tx).delete(id).await?;
                    }
                },
                Export::Failed {
                    failed_ts: last_ts, ..
                }
                | Export::Canceled {
                    canceled_ts: last_ts,
                    ..
                } => {
                    if last_ts < delete_before_ts {
                        SystemMetadataModel::new_global(self.tx).delete(id).await?;
                    }
                },
            }
        }
        Ok(to_delete)
    }

    pub async fn cancel(&mut self, snapshot_id: DeveloperDocumentId) -> anyhow::Result<()> {
        let (id, export) = self
            .get(snapshot_id)
            .await?
            .context("Snapshot not found")?
            .into_id_and_value();
        let export = export.canceled(*self.tx.begin_timestamp())?;
        SystemMetadataModel::new_global(self.tx)
            .replace(id, export.try_into()?)
            .await?;
        Ok(())
    }
}
