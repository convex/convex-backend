use std::sync::LazyLock;

use common::{
    document::{
        ParseDocument,
        ParsedDocument,
    },
    query::{
        IndexRange,
        IndexRangeExpression,
        Order,
        Query,
    },
    runtime::Runtime,
    types::WriteTimestamp,
};
use database::{
    unauthorized_error,
    ResolvedQuery,
    SystemMetadataModel,
    Transaction,
};
use keybroker::Identity;
use sync_types::Timestamp;
use value::{
    ConvexValue,
    FieldPath,
    TableName,
    TableNamespace,
};

pub mod types;

use types::{
    SessionRequestIdentifier,
    SessionRequestOutcome,
    SessionRequestRecord,
};

use crate::{
    SystemIndex,
    SystemTable,
};

/// Table name for the sync protocol requests.
/// This is used to make session requests idempotent
pub const SESSION_REQUESTS_TABLE: TableName = TableName::const_new("_session_requests");

static SESSION_ID_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "sessionId".parse().expect("Invalid built-in field"));

static REQUEST_ID_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "requestId".parse().expect("Invalid built-in field"));

pub static SESSION_REQUESTS_INDEX: LazyLock<SystemIndex<SessionRequestsTable>> =
    LazyLock::new(|| {
        SystemIndex::new(
            "by_session_id_and_request_id",
            [&SESSION_ID_FIELD, &REQUEST_ID_FIELD],
        )
        .unwrap()
    });

pub struct SessionRequestsTable;
impl SystemTable for SessionRequestsTable {
    type Metadata = SessionRequestRecord;

    const TABLE_NAME: TableName = SESSION_REQUESTS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![SESSION_REQUESTS_INDEX.clone()]
    }
}

pub struct SessionRequestModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> SessionRequestModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    pub async fn get_session_request_record(
        &mut self,
        request_identifier: &SessionRequestIdentifier,
        identity: Identity,
    ) -> anyhow::Result<Option<(Timestamp, SessionRequestOutcome)>> {
        // We only expect this function to be called by the framework as part
        // of a mutation UDF. We require passing in a system identity to confirm
        // that the caller isn't letting a user call this directly.
        if !identity.is_system() {
            anyhow::bail!(unauthorized_error("get_session_request_record"))
        }

        // Query whether this request has been seen by the system already. It's
        // important we scan over the session request index and include it
        // in our read set so we can ensure the request happens exactly once.
        let index_range_query = IndexRange {
            index_name: SESSION_REQUESTS_INDEX.name(),
            range: vec![
                IndexRangeExpression::Eq(
                    SESSION_ID_FIELD.clone(),
                    ConvexValue::try_from(request_identifier.session_id.to_string())?.into(),
                ),
                IndexRangeExpression::Eq(
                    REQUEST_ID_FIELD.clone(),
                    ConvexValue::from(request_identifier.request_id as i64).into(),
                ),
            ],
            order: Order::Asc,
        };
        let query = Query::index_range(index_range_query);
        let (doc, ts): (ParsedDocument<SessionRequestRecord>, Timestamp) = {
            // Get the timestamp of when the record was committed.
            // document by ID.
            // Note that this does not check the
            // transaction's outstanding writes. This is ok since we know we didn't
            // commit in the current transaction.
            let mut query_stream = ResolvedQuery::new(self.tx, TableNamespace::Global, query)?;
            let Some((doc, ts)) = query_stream.next_with_ts(self.tx, None).await? else {
                return Ok(None);
            };
            anyhow::ensure!(
                query_stream.next(self.tx, Some(1)).await?.is_none(),
                "Expected at most one session request record."
            );
            let WriteTimestamp::Committed(ts) = ts else {
                anyhow::bail!(
                    "Wrote a session request record in the same transaction as the get? Not \
                     supported."
                );
            };
            (doc.parse()?, ts)
        };

        let outcome = doc.into_value().outcome;

        Ok(Some((ts, outcome)))
    }

    pub async fn record_session_request(
        &mut self,
        record: SessionRequestRecord,
        identity: Identity,
    ) -> anyhow::Result<()> {
        // We only expect this function to be called by the framework as part
        // of executing a sync protocol session UDF. We require passing in a system
        // identity to confirm that the caller isn't letting a user call this
        // directly
        if !identity.is_system() {
            anyhow::bail!(unauthorized_error("record_session_request"))
        }

        SystemMetadataModel::new_global(self.tx)
            .insert_metadata(&SESSION_REQUESTS_TABLE, record.try_into()?)
            .await?;
        Ok(())
    }
}
