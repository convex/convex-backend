use std::{
    ops::Deref,
    time::Duration,
};

use anyhow::Context;
use pb::common::RepeatableTimestamp as RepeatableTimestampProto;
use sync_types::Timestamp;

/// WARNING: constructors of this struct must validate the timestamp is
/// repeatable -- according to the commit protocol -- in the constructor.
#[derive(Clone, Copy, Debug, derive_more::Display, Ord, PartialOrd, Eq, PartialEq)]
pub struct RepeatableTimestamp(Timestamp);

pub enum RepeatableReason {
    /// ts <= max_repeatable_ts from persistence globals.
    MaxRepeatableTsPersistence,
    /// ts = SnapshotManager.latest_ts()
    SnapshotManagerLatest,
    /// ts = TableSummarySnapshot.ts from persistence globals.
    TableSummarySnapshot,
    /// ts <= max_ts from persistence, and no Committer is running
    IdleMaxTs,
    /// ts <= min_index_snapshot_ts or min_document_snapshot_ts from persistence
    /// globals
    MinSnapshotTsPersistence,
    /// ts <= some other RepeatableTimestamp
    InductiveRepeatableTimestamp,
    /// only in db-info tool, and only when
    /// non-repeatable reads are directly requested.
    DbInfoManuallyRequested,
    /// RepeatableTimestamp serialized as a funrun::RepeatableTimestamp proto.
    /// The [FunctionRunner] requires a [`RepeatableTimestamp`] to create a
    /// [`DatabaseSnapshot`]. For now, the server trusts that the timestamp
    /// created in the client was repeatable. In the future, [FunctionRunner]
    /// can verify this.
    RepeatableTimestampProto,
}

impl RepeatableTimestamp {
    // Zero is always a valid RepeatableTimestamp.
    pub const MIN: RepeatableTimestamp = RepeatableTimestamp(Timestamp::MIN);

    /// Only call this constructor if you have validated the timestamp is
    /// repeatable. There should be very few callers of this function
    /// directly -- most should go through specialized constructors like
    /// new_static_repeatable_recent, unchecked_repeatable_ts, etc.
    ///
    /// Example of correct call-site:
    /// new_static_repeatable_recent reads the max_repeatable_ts persistence
    /// global, so the timestamp is guaranteed repeatable.
    ///
    /// Example of incorrect call-site:
    /// A timestamp is read from a cursor on IndexMetadata backfill state.
    /// Even though the cursor was probably valid when it was written, we
    /// should either
    /// (1) revalidate, or
    /// (2) pass through validation in the type system.
    /// To avoid issues where a non-repeatable Timestamp is
    /// serialized to u64 and deserialized as RepeatableTimestamp.
    pub fn new_validated(ts: Timestamp, _reason: RepeatableReason) -> Self {
        Self(ts)
    }

    pub fn prior_ts(&self, ts: Timestamp) -> anyhow::Result<Self> {
        anyhow::ensure!(ts <= *self);
        Ok(RepeatableTimestamp::new_validated(
            ts,
            RepeatableReason::InductiveRepeatableTimestamp,
        ))
    }

    pub fn sub(&self, duration: Duration) -> anyhow::Result<Self> {
        self.prior_ts((**self).sub(duration)?)
    }

    pub fn pred(&self) -> anyhow::Result<Self> {
        self.prior_ts((**self).pred()?)
    }
}

impl Deref for RepeatableTimestamp {
    type Target = Timestamp;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// RepeatableTimestampProto should never be constructed directly. Always use
/// From<RepeatableTimestamp> to guarantee it's repeatable.
impl From<RepeatableTimestamp> for RepeatableTimestampProto {
    fn from(value: RepeatableTimestamp) -> Self {
        Self {
            ts: Some((*value).into()),
        }
    }
}

/// RepeatableTimestamps can be serialized and deserialized to proto.
/// When deserializing, we assume that the proto was originally serialized from
/// RepeatableTimestamp.
/// This should be a last resort. If possible, pass the timestamp through as a
/// plain u64 and re-validate when needed.
impl TryFrom<RepeatableTimestampProto> for RepeatableTimestamp {
    type Error = anyhow::Error;

    fn try_from(value: RepeatableTimestampProto) -> anyhow::Result<Self> {
        let ts = value.ts.context("RepeatableTimestampProto missing ts")?;
        Ok(RepeatableTimestamp::new_validated(
            ts.try_into()?,
            RepeatableReason::RepeatableTimestampProto,
        ))
    }
}

/// In some places, like indexing, it's useful to assign a "timestamp" to
/// uncommitted writes in a transaction. `WriteTimestamp` provides a safe way to
/// do so without risking confusing an uncommitted write with a committed one.
/// The `Pending` timestamp sorts greater than any committed timestamp.
#[derive(Clone, Copy, Ord, Eq, Debug, PartialEq, PartialOrd)]
pub enum WriteTimestamp {
    Committed(Timestamp),
    Pending,
}
