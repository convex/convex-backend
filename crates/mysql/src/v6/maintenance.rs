//! Ahead-of-time creation of V6 `indexes_log_<bucket>` tables.
//!
//! A commit whose bucket table is missing fails, so buckets are created with a
//! wide lookahead and the round publishes how far ahead it got. Reads union
//! every bucket from the published floor through the newest created, so the
//! floor's bucket is created too when it precedes the one taking writes.
//!
//! The tables live in the DB cluster's schema, which every partition on that
//! cluster shares, so several conductors run this against the same tables. A
//! CAS lease on `indexes_maintenance_state` picks one of them per round.

use std::{
    cmp,
    collections::BTreeSet,
    sync::Arc,
    time::Duration,
};

use anyhow::Context as _;
use common::{
    cover,
    knobs::{
        INDEXES_LOG_LOOKAHEAD_BUCKETS,
        INDEX_RETENTION_DELAY,
    },
    runtime::Runtime,
    types::Timestamp,
};
use const_format::concatcp;
use itertools::Itertools as _;
use mysql_async::{
    Row,
    Value,
};

use super::sql::{
    drop_log_ddl,
    log_ddl,
    LogBucket,
    LIST_LOG_TABLES,
};
use crate::{
    connection::MySqlConnection,
    metrics,
    ConvexMySqlPool,
};

pub(crate) const LEASE_TTL: Duration = Duration::from_secs(5 * 60);

const CONNECTION_NAME: &str = "indexes_log_maintenance";

const STATE_TABLE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS @db_name.indexes_maintenance_state (
    id TINYINT UNSIGNED NOT NULL,
    /* exclusive upper bound of the buckets that existed at the last round */
    created_through_ts BIGINT NOT NULL,
    /* the oldest snapshot a read may still ask for; never retreats */
    oldest_kept_ts BIGINT NOT NULL,
    lease_owner VARCHAR(64) NOT NULL,
    lease_expires_ts BIGINT NOT NULL,
    last_run_ts BIGINT NOT NULL,
    last_run_status VARCHAR(16) NOT NULL,
    PRIMARY KEY (id),
    CHECK (id = 1)
) ROW_FORMAT=DYNAMIC;
"#;

const SEED_STATE_ROW: &str = r#"
INSERT INTO @db_name.indexes_maintenance_state
    (id, created_through_ts, oldest_kept_ts, lease_owner, lease_expires_ts, last_run_ts, last_run_status)
    VALUES (1, 0, 0, '', 0, 0, 'never_run')
    ON DUPLICATE KEY UPDATE id = id;
"#;

pub(crate) const INIT_SQL: &str = concatcp!(STATE_TABLE_DDL, SEED_STATE_ROW);

const READ_STATE: &str = r#"
SELECT oldest_kept_ts FROM @db_name.indexes_maintenance_state WHERE id = 1
"#;

const TAKE_LEASE: &str = r#"
UPDATE @db_name.indexes_maintenance_state
SET lease_owner = ?, lease_expires_ts = ?
WHERE id = 1 AND (lease_expires_ts <= ? OR lease_owner = ?)
"#;

// Writers cache the published ceiling, so it must remain monotonic.
const PUBLISH_STATE: &str = r#"
UPDATE @db_name.indexes_maintenance_state
SET created_through_ts = GREATEST(created_through_ts, ?),
    oldest_kept_ts = GREATEST(oldest_kept_ts, ?),
    last_run_ts = ?,
    last_run_status = 'ok'
WHERE id = 1 AND lease_owner = ?
"#;

const MARK_FAILED: &str = r#"
UPDATE @db_name.indexes_maintenance_state
SET last_run_ts = ?, last_run_status = 'failed'
WHERE id = 1 AND lease_owner = ?
"#;

#[derive(Debug)]
pub struct MaintenanceRound {
    pub dropped: Option<String>,
    pub created_through_ts: Timestamp,
    pub oldest_kept_ts: Timestamp,
    pub bucket_count: usize,
}

pub struct IndexesLogMaintenance<RT: Runtime> {
    pool: Arc<ConvexMySqlPool<RT>>,
    db_name: String,
    owner: String,
}

impl<RT: Runtime> IndexesLogMaintenance<RT> {
    pub fn new(pool: Arc<ConvexMySqlPool<RT>>, db_name: String, owner: String) -> Self {
        Self {
            pool,
            db_name,
            owner,
        }
    }

    pub async fn initialize(&self) -> anyhow::Result<()> {
        let mut conn = self.pool.acquire(CONNECTION_NAME, &self.db_name).await?;
        conn.execute_many(INIT_SQL).await
    }

    /// Creates the buckets writes will need, drop at most one that has aged
    /// out, and publish the new ceiling and floor. Returns `None` when
    /// another conductor holds the lease.
    pub async fn run_once(&self, now: Timestamp) -> anyhow::Result<Option<MaintenanceRound>> {
        let mut conn = self.pool.acquire(CONNECTION_NAME, &self.db_name).await?;
        let row = conn
            .query_optional(READ_STATE, vec![])
            .await?
            .context("indexes_maintenance_state has no row; call initialize() first")?;

        // The floor a previous round published. This round may advance it
        // but never walk it back.
        let prev_oldest_kept_ts = ts_column(&row, 0)?;

        if !self.take_lease(&mut conn, now).await? {
            return Ok(None);
        }

        let round = self.maintain(&mut conn, now, prev_oldest_kept_ts).await;
        if round.is_err()
            && let Err(e) = conn
                .exec_iter(
                    MARK_FAILED,
                    vec![Value::Int(i64::from(now)), (&self.owner).into()],
                )
                .await
        {
            tracing::warn!("Could not record indexes_log maintenance failure: {e:#}");
        }
        round.map(Some)
    }

    /// Reports this round's gauges. The created and dropped counters are logged
    /// by the DDL itself.
    pub fn report_metrics(&self, round: &MaintenanceRound, now: Timestamp) {
        metrics::log_indexes_log_maintenance(self.pool.cluster_name(), round, now);
    }

    async fn take_lease(
        &self,
        conn: &mut MySqlConnection<'_, RT>,
        now: Timestamp,
    ) -> anyhow::Result<bool> {
        let expires_ts = now.add(LEASE_TTL)?;
        let affected_rows = conn
            .exec_iter(
                TAKE_LEASE,
                vec![
                    (&self.owner).into(),
                    Value::Int(i64::from(expires_ts)),
                    Value::Int(i64::from(now)),
                    (&self.owner).into(),
                ],
            )
            .await?;
        Ok(affected_rows == 1)
    }

    async fn maintain(
        &self,
        conn: &mut MySqlConnection<'_, RT>,
        now: Timestamp,
        prev_oldest_kept_ts: Timestamp,
    ) -> anyhow::Result<MaintenanceRound> {
        let mut existing: BTreeSet<LogBucket> = conn
            .query_collect(
                LIST_LOG_TABLES,
                vec![(&self.db_name).into()],
                16,
                |row: Row| {
                    let table_name: String = row
                        .get(0)
                        .context("LIST_LOG_TABLES returned zero columns?")?;
                    LogBucket::from_table_name(&table_name)
                },
            )
            .await?
            .into_iter()
            .collect();

        let now_bucket = LogBucket::from_successor_ts(now);
        let retention_start = now.sub(*INDEX_RETENTION_DELAY).unwrap_or(Timestamp::MIN);
        let oldest_kept_ts = cmp::max(
            prev_oldest_kept_ts,
            LogBucket::from_successor_ts(retention_start).start_ts()?,
        );
        let floor_bucket = LogBucket::from_successor_ts(oldest_kept_ts);
        let missing = buckets_to_create(
            floor_bucket.min(now_bucket),
            now_bucket,
            *INDEXES_LOG_LOOKAHEAD_BUCKETS,
            &existing,
        )?;
        // Counted here rather than off the returned round: the DDL is done and
        // committed even if this round later loses the publish fence.
        for bucket in missing {
            conn.execute_many(&log_ddl(bucket)).await?;
            metrics::log_indexes_log_bucket_created(self.pool.cluster_name());
            existing.insert(bucket);
        }

        // Dropping runs against the floor a previous round published, and
        // publishing this round's floor comes after. A reader still holding the
        // older floor can only be pointed at buckets that are still here.
        let dropped = bucket_to_drop(&existing, prev_oldest_kept_ts, now_bucket)?;
        if let Some(bucket) = dropped {
            cover!(super::coverage::BUCKET_DROPPED);
            conn.execute_many(&drop_log_ddl(bucket)).await?;
            metrics::log_indexes_log_bucket_dropped(self.pool.cluster_name());
            existing.remove(&bucket);
        }

        let created_through_ts = existing
            .last()
            .context("no log bucket exists after creating the lookahead")?
            .end_ts()?;
        let published = conn
            .exec_iter(
                PUBLISH_STATE,
                vec![
                    Value::Int(i64::from(created_through_ts)),
                    Value::Int(i64::from(oldest_kept_ts)),
                    Value::Int(i64::from(now)),
                    (&self.owner).into(),
                ],
            )
            .await?;
        anyhow::ensure!(
            published == 1,
            "another conductor took over the indexes_log maintenance lease mid-round"
        );

        Ok(MaintenanceRound {
            dropped: dropped.map(LogBucket::table_name),
            created_through_ts,
            oldest_kept_ts,
            bucket_count: existing.len(),
        })
    }
}

fn ts_column(row: &Row, index: usize) -> anyhow::Result<Timestamp> {
    let nanos: i64 = row
        .get(index)
        .with_context(|| format!("indexes_maintenance_state has no column {index}"))?;
    Timestamp::try_from(nanos)
}

/// A day of lookahead. Past this the knob is a typo rather than a policy, and
/// enumerating the range would cost more than the outage it guards against.
const MAX_LOOKAHEAD_BUCKETS: usize = 24 * 6;

/// The buckets from `first`, the floor's or the one taking writes, through the
/// lookahead past `now_bucket`: every bucket a read unions or a write may land
/// in before maintenance is guaranteed to run again, so all of them must exist
/// before those reads and writes arrive.
fn buckets_to_create(
    first: LogBucket,
    now_bucket: LogBucket,
    lookahead: usize,
    existing: &BTreeSet<LogBucket>,
) -> anyhow::Result<Vec<LogBucket>> {
    anyhow::ensure!(
        lookahead <= MAX_LOOKAHEAD_BUCKETS,
        "INDEXES_LOG_LOOKAHEAD_BUCKETS is {lookahead}, above the cap of {MAX_LOOKAHEAD_BUCKETS}"
    );
    let last = now_bucket
        .offset(i64::try_from(lookahead).context("lookahead does not fit in a bucket offset")?)?;
    (0..=last.value() - first.value())
        .map(|offset| first.offset(offset))
        .filter_ok(|bucket| !existing.contains(bucket))
        .try_collect()
}

fn bucket_to_drop(
    existing: &BTreeSet<LogBucket>,
    prev_oldest_kept_ts: Timestamp,
    now_bucket: LogBucket,
) -> anyhow::Result<Option<LogBucket>> {
    let Some(&bucket) = existing.first() else {
        return Ok(None);
    };
    let past_backstop = bucket.value() < now_bucket.value() - 1;
    Ok((past_backstop && bucket.end_ts()? <= prev_oldest_kept_ts).then_some(bucket))
}
