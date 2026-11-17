//! Structural definitions shared by every V6 persistence table.
//!
//! The V6 logical database is tenant-scoped by `deployment_id`.

pub(crate) mod backfill;
pub(crate) mod documents;
pub(crate) mod indexes;
pub mod maintenance;
mod persistence;
pub(crate) mod reader;
pub(crate) mod sql;
use std::sync::Arc;

use anyhow::Context;
use common::{
    persistence::{
        Persistence as PersistenceTrait,
        PersistenceGlobalKey,
        PersistenceReader,
    },
    runtime::Runtime,
    shutdown::ShutdownSignal,
    types::DeploymentId as BigBrainDeploymentId,
};
use const_format::concatcp;
use mysql_async::{
    Row,
    Value,
};
pub(crate) use persistence::Persistence;
use serde::Deserialize;
use serde_json::Value as JsonValue;
pub(crate) use sql::{
    FIND_TABLE,
    INIT_LEASE,
    LEASE_ACQUIRE,
    LEASE_ACQUIRE_ALLOW_READ_ONLY,
    LEASE_PRECONDITION,
    READ_PERSISTENCE_GLOBAL,
    READ_V6_SCOPED_TABLES,
    SET_READ_ONLY,
    UNSET_READ_ONLY,
    WRITE_PERSISTENCE_GLOBAL,
};

use crate::{
    ConvexMySqlPool,
    MySqlOptions,
    MySqlReaderOptions,
};

pub(crate) async fn connect<RT: Runtime>(
    pool: Arc<ConvexMySqlPool<RT>>,
    db_name: String,
    options: MySqlOptions,
    lease_lost_shutdown: ShutdownSignal,
) -> anyhow::Result<Arc<dyn PersistenceTrait>> {
    Ok(Arc::new(
        Persistence::new(pool, db_name, options, lease_lost_shutdown).await?,
    ))
}

pub(crate) fn connect_reader<RT: Runtime>(
    pool: Arc<ConvexMySqlPool<RT>>,
    db_name: String,
    options: MySqlReaderOptions,
) -> anyhow::Result<Arc<dyn PersistenceReader>> {
    Ok(Arc::new(Persistence::new_reader(pool, db_name, options)?))
}

pub(crate) async fn set_persistence_read_only<RT: Runtime>(
    pool: Arc<ConvexMySqlPool<RT>>,
    db_name: String,
    options: MySqlOptions,
    read_only: bool,
) -> anyhow::Result<()> {
    Persistence::set_read_only(pool, db_name, options, read_only).await
}

/// A deployment identifier in V6 persistence tables.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct PersistenceDeploymentId(u32);

impl PersistenceDeploymentId {
}

impl From<PersistenceDeploymentId> for Value {
    fn from(deployment_id: PersistenceDeploymentId) -> Self {
        Value::UInt(u64::from(deployment_id.0))
    }
}

impl TryFrom<BigBrainDeploymentId> for PersistenceDeploymentId {
    type Error = anyhow::Error;

    fn try_from(value: BigBrainDeploymentId) -> Result<Self, Self::Error> {
        let value = u32::try_from(value.0).map_err(|_| {
            anyhow::anyhow!("deployment ID {} does not fit in INT UNSIGNED", value.0)
        })?;
        Ok(Self(value))
    }
}

const DOCUMENTS_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS @db_name.documents (
    deployment_id INT UNSIGNED NOT NULL,
    id BINARY(16) NOT NULL,
    ts BIGINT NOT NULL,
    table_id BINARY(16) NOT NULL,
    json_value LONGBLOB NOT NULL,
    deleted BOOLEAN DEFAULT false,
    prev_ts BIGINT,
    PRIMARY KEY (deployment_id, ts, table_id, id),
    INDEX documents_by_table_and_id (deployment_id, table_id, id, ts)
) ROW_FORMAT=DYNAMIC;
"#;

pub(crate) const INDEXES_LATEST_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS @db_name.indexes_latest (
    deployment_id INT UNSIGNED NOT NULL,
    index_id INT UNSIGNED NOT NULL,
    key_prefix VARBINARY(2500) NOT NULL,
    key_suffix LONGBLOB NULL,
    key_suffix_hash VARBINARY(16) NOT NULL,
    ts BIGINT NOT NULL,
    table_id BINARY(16) NOT NULL,
    document_id BINARY(16) NOT NULL,
    PRIMARY KEY (deployment_id, index_id, key_prefix, key_suffix_hash),
    CHECK (ts >= 0)
) ROW_FORMAT=DYNAMIC;
"#;

/// Deletion markers used to reconcile index backfill rows. Each marker records
/// the latest deletion timestamp for an index key.
pub(crate) const INDEXES_BACKFILL_DELETES_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS @db_name.indexes_backfill_deletes (
    deployment_id INT UNSIGNED NOT NULL,
    index_id INT UNSIGNED NOT NULL,
    key_prefix VARBINARY(2500) NOT NULL,
    key_suffix LONGBLOB NULL,
    key_suffix_hash VARBINARY(16) NOT NULL,
    ts BIGINT NOT NULL,
    PRIMARY KEY (deployment_id, index_id, key_prefix, key_suffix_hash)
) ROW_FORMAT=DYNAMIC;
"#;

const LEASES_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS @db_name.leases (
    deployment_id INT UNSIGNED NOT NULL,
    ts BIGINT NOT NULL,
    PRIMARY KEY (deployment_id)
) ROW_FORMAT=DYNAMIC;
"#;

const READ_ONLY_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS @db_name.read_only (
    deployment_id INT UNSIGNED NOT NULL,
    PRIMARY KEY (deployment_id)
) ROW_FORMAT=DYNAMIC;
"#;

const PERSISTENCE_GLOBALS_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS @db_name.persistence_globals (
    deployment_id INT UNSIGNED NOT NULL,
    `key` VARCHAR(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin NOT NULL,
    json_value LONGBLOB NOT NULL,
    PRIMARY KEY (deployment_id, `key`)
) ROW_FORMAT=DYNAMIC;
"#;

// `indexes_latest` is V6-only and must stay first: its presence distinguishes
// an interrupted V6 initialization from an incompatible V5 database. Every
// statement is idempotent so initialization can resume. The log tables are
// absent because `maintenance` creates them ahead of the writes that need
// them; its state row is seeded here so reads find it as soon as the schema
// exists.
pub(crate) const fn init_sql() -> &'static str {
    concatcp!(
        INDEXES_LATEST_DDL,
        INDEXES_BACKFILL_DELETES_DDL,
        DOCUMENTS_DDL,
        LEASES_DDL,
        READ_ONLY_DDL,
        PERSISTENCE_GLOBALS_DDL,
        maintenance::INIT_SQL,
    )
}

/// Decodes a `persistence_globals.json_value` column. Globals can nest deeply
/// enough to trip serde_json's default recursion limit.
pub(crate) fn decode_persistence_global(
    row: &Row,
    key: PersistenceGlobalKey,
) -> anyhow::Result<JsonValue> {
    let mut deserializer = serde_json::Deserializer::from_slice(column::bytes(row, 0)?);
    deserializer.disable_recursion_limit();
    let value = JsonValue::deserialize(&mut deserializer)
        .with_context(|| format!("Invalid JSON at persistence key {key:?}"))?;
    deserializer.end()?;
    Ok(value)
}

/// Column accessors for V6 rows.
pub(crate) mod column {
    use mysql_async::{
        Row,
        Value,
    };

    pub(crate) fn bytes(row: &Row, column: usize) -> anyhow::Result<&[u8]> {
        match row.as_ref(column) {
            Some(Value::Bytes(bytes)) => Ok(bytes),
            _ => anyhow::bail!("row[{column}] must be bytes"),
        }
    }

    pub(crate) fn maybe_bytes(row: &Row, column: usize) -> anyhow::Result<Option<&[u8]>> {
        match row.as_ref(column) {
            Some(Value::Bytes(bytes)) => Ok(Some(bytes)),
            Some(Value::NULL) => Ok(None),
            _ => anyhow::bail!("row[{column}] must be bytes or NULL"),
        }
    }
}
