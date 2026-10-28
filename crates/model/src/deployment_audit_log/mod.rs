use std::sync::LazyLock;

use common::{
    document::CREATION_TIME_FIELD_PATH,
    execution_context::RequestMetadata,
    query::{
        Cursor,
        CursorPosition,
        IndexRange,
        IndexRangeExpression,
        Order,
        Query,
    },
    runtime::Runtime,
    types::{
        IndexName,
        MemberId,
    },
};
use database::{
    query::{
        PaginationOptions,
        TableFilter,
    },
    unauthorized_error,
    ResolvedQuery,
    SystemMetadataModel,
    Transaction,
};
use value::{
    ConvexValue,
    FieldPath,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

pub mod developer_index_config;
pub mod integration_config;
pub mod types;

use types::{
    DeploymentAuditLogEntry,
    DeploymentAuditLogEvent,
    DeploymentAuditLogEventKind,
};

use crate::{
    SystemIndex,
    SystemTable,
};

pub const DEPLOYMENT_AUDIT_LOG_TABLE: TableName = TableName::const_new("_deployment_audit_log");

pub static ACTION_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "action".parse().expect("invalid action field"));

/// By action, then creation time. Lets callers that only care about a single
/// action (e.g. the usage-limit worker priming from `usage_limit_exceeded`
/// events) scan just that action's entries instead of every audit log event.
pub static AUDIT_LOG_INDEX_BY_ACTION: LazyLock<SystemIndex<DeploymentAuditLogsTable>> =
    LazyLock::new(|| {
        SystemIndex::new(
            "by_action_and_creation_time",
            [&ACTION_FIELD, &CREATION_TIME_FIELD_PATH],
        )
        .unwrap()
    });

pub struct DeploymentAuditLogsTable;
impl SystemTable for DeploymentAuditLogsTable {
    type Metadata = DeploymentAuditLogEvent;

    const TABLE_NAME: TableName = DEPLOYMENT_AUDIT_LOG_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![AUDIT_LOG_INDEX_BY_ACTION.clone()]
    }
}

pub struct DeploymentAuditLogModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> DeploymentAuditLogModel<'a, RT> {
    /// This fn should never be called directly. Instead use
    /// `commit_with_audit_log_events` in `application` to ensure that audit
    /// logs are streamed.
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    pub async fn insert(
        &mut self,
        events: Vec<DeploymentAuditLogEvent>,
        request_metadata: &RequestMetadata,
    ) -> anyhow::Result<Vec<ResolvedDocumentId>> {
        self.insert_with_member_override(events, None, request_metadata)
            .await
    }

    pub async fn insert_with_member_override(
        &mut self,
        events: Vec<DeploymentAuditLogEvent>,
        member_id_override: Option<MemberId>,
        request_metadata: &RequestMetadata,
    ) -> anyhow::Result<Vec<ResolvedDocumentId>> {
        if !(self.tx.identity().is_system() || self.tx.identity().is_admin()) {
            anyhow::bail!(unauthorized_error("insert_deployment_audit_log_event"));
        }
        let member_id = member_id_override.or_else(|| self.tx.identity().member_id());
        let member_id_value = member_id
            .map(|member_id| {
                let member_id_u64: u64 = member_id.into();
                i64::try_from(member_id_u64)
            })
            .transpose()?;
        let token_id = self
            .tx
            .identity()
            .token_id()
            .map(|id| i64::try_from(id.0))
            .transpose()?;
        let app_client_id = self.tx.identity().app_client_id();
        let client_ip = request_metadata.ip.as_ref().map(|ip| ip.as_str());
        let client_user_agent = request_metadata
            .user_agent
            .as_ref()
            .map(|user_agent| user_agent.as_str());
        let mut deployment_audit_log_ids = vec![];
        for event in events {
            let event_object = event.into_audit_log_object(
                member_id_value,
                token_id,
                app_client_id.as_deref(),
                client_ip,
                client_user_agent,
            )?;
            let id = SystemMetadataModel::new_global(self.tx)
                .insert_metadata(&DEPLOYMENT_AUDIT_LOG_TABLE, event_object)
                .await?;
            deployment_audit_log_ids.push(id);
        }
        Ok(deployment_audit_log_ids)
    }

    /// List audit log events with creation time >= `from_ts_ms`, in ascending
    /// creation-time order.
    ///
    /// When `action` is set, only entries of that kind are returned, served
    /// from the `by_action_and_creation_time` index so unrelated events aren't
    /// scanned.
    pub async fn list_events_from_time(
        &mut self,
        from_ts_ms: u64,
        action: Option<DeploymentAuditLogEventKind>,
        cursor: Option<Cursor>,
        limit: usize,
    ) -> anyhow::Result<(Vec<DeploymentAuditLogEntry>, Option<Cursor>)> {
        let creation_time_gte =
            IndexRangeExpression::Gte(CREATION_TIME_FIELD_PATH.clone(), (from_ts_ms as f64).into());
        let (index_name, range) = match action {
            Some(action) => (
                AUDIT_LOG_INDEX_BY_ACTION.name(),
                vec![
                    IndexRangeExpression::Eq(
                        ACTION_FIELD.clone(),
                        ConvexValue::try_from(action.action())?.into(),
                    ),
                    creation_time_gte,
                ],
            ),
            None => (
                IndexName::by_creation_time(DEPLOYMENT_AUDIT_LOG_TABLE.clone()),
                vec![creation_time_gte],
            ),
        };
        let query = Query::index_range(IndexRange {
            index_name,
            range,
            order: Order::Asc,
        });
        let mut query_stream = ResolvedQuery::new_bounded(
            self.tx,
            TableNamespace::Global,
            query,
            PaginationOptions::ManualPagination {
                start_cursor: cursor,
                maximum_rows_read: Some(limit),
                maximum_bytes_read: None,
            },
            None,
            TableFilter::IncludePrivateSystemTables,
        )?;

        let mut events = Vec::with_capacity(limit);
        while events.len() < limit
            && let Some(document) = query_stream.next(self.tx, None).await?
        {
            events.push(DeploymentAuditLogEntry::try_from(document)?);
        }

        let next_cursor = match query_stream.cursor() {
            Some(cursor) if !matches!(cursor.position, CursorPosition::End) => Some(cursor),
            _ => None,
        };
        Ok((events, next_cursor))
    }
}
