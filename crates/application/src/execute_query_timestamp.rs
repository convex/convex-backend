use pb::common::{
    execute_query_timestamp,
    ExecuteQueryTimestamp as ExecuteQueryTimestampProto,
};

use crate::api::ExecuteQueryTimestamp;

impl From<ExecuteQueryTimestamp> for ExecuteQueryTimestampProto {
    fn from(ts: ExecuteQueryTimestamp) -> Self {
        let ts = match ts {
            ExecuteQueryTimestamp::Latest => execute_query_timestamp::Ts::Latest(()),
            ExecuteQueryTimestamp::At(ts) => execute_query_timestamp::Ts::At(ts.into()),
        };
        Self { ts: Some(ts) }
    }
}

impl TryFrom<ExecuteQueryTimestampProto> for ExecuteQueryTimestamp {
    type Error = anyhow::Error;

    fn try_from(msg: ExecuteQueryTimestampProto) -> anyhow::Result<Self> {
        let ts = match msg.ts {
            Some(execute_query_timestamp::Ts::Latest(())) => ExecuteQueryTimestamp::Latest,
            Some(execute_query_timestamp::Ts::At(ts)) => ExecuteQueryTimestamp::At(ts.try_into()?),
            None => anyhow::bail!("Missing `ts` field"),
        };
        Ok(ts)
    }
}
