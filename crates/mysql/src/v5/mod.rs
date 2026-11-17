//! V5 persistence.

pub(crate) mod documents;
pub(crate) mod indexes;
mod persistence;
mod sql;

use std::sync::Arc;

use common::{
    persistence::{
        Persistence as PersistenceTrait,
        PersistenceReader,
    },
    runtime::Runtime,
    shutdown::ShutdownSignal,
};
use const_format::formatcp;
#[allow(
    unused_imports,
    reason = "Consumed by the opt-in shadow implementation in the follow-up PR"
)]
pub(crate) use persistence::Lease;
pub(super) use persistence::{
    internal_doc_id_param,
    internal_id_param,
};
pub use persistence::{
    Persistence,
    Reader,
};
pub(crate) use sql::{
    get_persistence_global,
    lease_acquire,
    lease_precond,
    set_read_only,
    unset_read_only,
    write_persistence_global,
    GET_TABLE_COUNT,
    TABLE_SIZE_QUERY,
};

use crate::{
    ConvexMySqlPool,
    MySqlOptions,
    MySqlReaderOptions,
};

#[derive(PartialEq, Eq, Hash, Copy, Clone)]
pub(super) enum BoundType {
    Unbounded,
    Included,
    Excluded,
}

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
    Ok(Arc::new(Persistence::new_reader(pool, db_name, options)))
}

pub(crate) async fn set_persistence_read_only<RT: Runtime>(
    pool: Arc<ConvexMySqlPool<RT>>,
    db_name: String,
    options: MySqlOptions,
    read_only: bool,
) -> anyhow::Result<()> {
    Persistence::set_read_only(pool, db_name, options, read_only).await
}

use crate::sql::common::{
    as_table,
    tableify,
};

pub(crate) const EXPECTED_TABLE_COUNT: usize = 5;

// This runs (currently) every time a Persistence is created, so it
// needs to not only be idempotent but not to affect any already-resident data.
// IF NOT EXISTS and ON CONFLICT are helpful.
pub(crate) const fn init_sql(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"
        CREATE TABLE IF NOT EXISTS @db_name.documents (
            {instance_col_def}
            id BINARY(16) NOT NULL,
            ts BIGINT NOT NULL,

            table_id BINARY(16) NOT NULL,

            json_value LONGBLOB NOT NULL,
            deleted BOOLEAN DEFAULT false,

            prev_ts BIGINT,

            PRIMARY KEY ({instance_col} ts, table_id, id),
            INDEX documents_by_table_and_id ({instance_col} table_id, id, ts)
        ) ROW_FORMAT=DYNAMIC;

        CREATE TABLE IF NOT EXISTS @db_name.indexes (
            {instance_col_def}
            /* ids should be serialized as bytes but we keep it compatible with documents */
            index_id BINARY(16) NOT NULL,
            ts BIGINT NOT NULL,

            /*
            MySQL maximum primary key length is 3072 bytes with DYNAMIC row format,
            which is why we split up the key. The first 2500 bytes are stored in key_prefix,
            and the remaining ones are stored in key suffix if applicable.
            NOTE: The key_prefix + key_suffix is store all values of IndexKey including
            the id.
            */
            key_prefix VARBINARY(2500) NOT NULL,
            key_suffix LONGBLOB NULL,

            /* key_sha256 of the full key, used in primary key to avoid duplicates in case
            of key_prefix collision. */
            key_sha256 BINARY(32) NOT NULL,

            deleted BOOLEAN,
            /* table_id and document_id should be populated iff deleted is false. */
            table_id BINARY(16) NULL,
            document_id BINARY(16) NULL,

            PRIMARY KEY ({instance_col} index_id, key_prefix, key_sha256, ts)
        ) ROW_FORMAT=DYNAMIC;
        CREATE TABLE IF NOT EXISTS @db_name.leases (
            {lease_col_def},
            ts BIGINT NOT NULL,

            PRIMARY KEY ({lease_pk})
        ) ROW_FORMAT=DYNAMIC;
        CREATE TABLE IF NOT EXISTS @db_name.read_only (
            {read_only_col_def},

            PRIMARY KEY ({read_only_pk})
        ) ROW_FORMAT=DYNAMIC;
        CREATE TABLE IF NOT EXISTS @db_name.persistence_globals (
            {instance_col_def}
            `key` VARCHAR(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin NOT NULL,
            json_value LONGBLOB NOT NULL,

            PRIMARY KEY ({instance_col} `key`)
        ) ROW_FORMAT=DYNAMIC;"#,
            instance_col_def = if multitenant {
                "instance_name VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin NOT NULL,"
            } else {
                ""
            },
            instance_col = if multitenant { "instance_name," } else { "" },
            lease_col_def = if multitenant {
                "instance_name VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin NOT NULL"
            } else {
                "id BIGINT NOT NULL"
            },
            lease_pk = if multitenant { "instance_name" } else { "id" },
            read_only_col_def = if multitenant {
                "instance_name VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin NOT NULL"
            } else {
                "id BIGINT NOT NULL"
            },
            read_only_pk = if multitenant { "instance_name" } else { "id" }
        )
    )
}

pub(crate) const fn init_lease(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        // Note the no-op `ON DUPLICATE` expression to "do nothing" if there's a duplicate.
        // INSERT IGNORE ignores *all* errors, so this is considered best practice..
        formatcp!(
            "INSERT INTO @db_name.leases ({lease_col_def}, ts) VALUES ({lease_val}, 0) ON \
             DUPLICATE KEY UPDATE {lease_col_def} = {lease_col_def};",
            lease_col_def = if multitenant { "instance_name" } else { "id" },
            lease_val = if multitenant { "?" } else { "1" }
        )
    )
}
