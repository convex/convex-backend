use std::{
    collections::HashMap,
    fmt::Write,
    sync::LazyLock,
};

use common::{
    index::{
        SplitKey,
        MAX_INDEX_KEY_PREFIX_LEN,
    },
    interval::{
        End,
        Interval,
        StartIncluded,
    },
    query::Order,
    types::{
        IndexId,
        Timestamp,
    },
};
use const_format::formatcp;
use itertools::{
    iproduct,
    Itertools,
};

use super::BoundType;
use crate::{
    chunks::smart_chunk_sizes,
    sql::common::{
        as_table,
        tableify,
    },
    MySqlInstanceName,
};

pub const GET_TABLE_COUNT: &str = r#"
    SELECT COUNT(1) FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = ?;
"#;

pub const fn has_index_entries(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            "SELECT 1 FROM @db_name.indexes {instance_clause} LIMIT 1",
            instance_clause = if multitenant {
                "WHERE instance_name = ?"
            } else {
                ""
            }
        )
    )
}

pub const INSERT_INDEX_COLUMN_COUNT: usize = 8;

static INSERT_INDEX_CHUNK_QUERIES: LazyLock<HashMap<(usize, bool), String>> = LazyLock::new(|| {
    smart_chunk_sizes()
        .flat_map(|chunk_size| {
            [false, true].into_iter().map(move |multitenant| {
                let query = if multitenant {
                    let values = (1..=chunk_size)
                        .map(|_| "(?, ?, ?, ?, ?, ?, ?, ?, ?)".to_string())
                        .join(", ");
                    format!(
                        r#"INSERT INTO @db_name.indexes
            (instance_name, index_id, ts, key_prefix, key_suffix, key_sha256, deleted, table_id, document_id)
            VALUES {values}"#
                    )
                } else {
                    let values = (1..=chunk_size)
                        .map(|_| "(?, ?, ?, ?, ?, ?, ?, ?)".to_string())
                        .join(", ");
                    format!(
                        r#"INSERT INTO @db_name.indexes
            (index_id, ts, key_prefix, key_suffix, key_sha256, deleted, table_id, document_id)
            VALUES {values}"#
                    )
                };
                ((chunk_size, multitenant), query)
            })
        })
        .collect()
});

// Note that on conflict, there's no need to update any of the columns that are
// part of the primary key, nor `key_suffix` as `key_sha256` is derived from the
// prefix and suffix.
// Only the fields that could have actually changed need to be updated.
pub fn insert_index_chunk(chunk_size: usize, multitenant: bool) -> &'static str {
    INSERT_INDEX_CHUNK_QUERIES
        .get(&(chunk_size, multitenant))
        .unwrap()
}

static INSERT_OVERWRITE_INDEX_CHUNK_QUERIES: LazyLock<HashMap<(usize, bool), String>> =
    LazyLock::new(|| {
        smart_chunk_sizes()
            .flat_map(|chunk_size| {
                [false, true].into_iter().map(move |multitenant| {
                    let query = if multitenant {
                        let values = (1..=chunk_size)
                            .map(|_| "(?, ?, ?, ?, ?, ?, ?, ?, ?)".to_string())
                            .join(", ");
                        format!(
                            r#"INSERT INTO @db_name.indexes
            (instance_name, index_id, ts, key_prefix, key_suffix, key_sha256, deleted, table_id, document_id)
            VALUES
                {values}
                ON DUPLICATE KEY UPDATE
                deleted = VALUES(deleted),
                table_id = VALUES(table_id),
                document_id = VALUES(document_id)
        "#
                        )
                    } else {
                        let values = (1..=chunk_size)
                            .map(|_| "(?, ?, ?, ?, ?, ?, ?, ?)".to_string())
                            .join(", ");
                        format!(
                            r#"INSERT INTO @db_name.indexes
            (index_id, ts, key_prefix, key_suffix, key_sha256, deleted, table_id, document_id)
            VALUES
                {values}
                ON DUPLICATE KEY UPDATE
                deleted = VALUES(deleted),
                table_id = VALUES(table_id),
                document_id = VALUES(document_id)
        "#
                        )
                    };
                    ((chunk_size, multitenant), query)
                })
            })
            .collect()
    });

pub fn insert_overwrite_index_chunk(chunk_size: usize, multitenant: bool) -> &'static str {
    INSERT_OVERWRITE_INDEX_CHUNK_QUERIES
        .get(&(chunk_size, multitenant))
        .unwrap()
}

pub const DELETE_INDEX_COLUMN_COUNT: usize = 4;
static DELETE_INDEX_CHUNK_QUERIES: LazyLock<HashMap<(usize, bool), String>> = LazyLock::new(|| {
    smart_chunk_sizes()
        .flat_map(|chunk_size| {
            [false, true].into_iter().map(move |multitenant| {
                let where_clauses = (1..=chunk_size)
                    .map(|_| {
                        if multitenant {
                            "(index_id = ? AND key_prefix = ? AND key_sha256 = ? AND ts <= ? AND \
                             instance_name = ?)"
                        } else {
                            "(index_id = ? AND key_prefix = ? AND key_sha256 = ? AND ts <= ?)"
                        }
                    })
                    .join(" OR ");
                (
                    (chunk_size, multitenant),
                    format!("DELETE FROM @db_name.indexes WHERE {where_clauses}"),
                )
            })
        })
        .collect()
});

pub fn delete_index_chunk(chunk_size: usize, multitenant: bool) -> &'static str {
    DELETE_INDEX_CHUNK_QUERIES
        .get(&(chunk_size, multitenant))
        .unwrap()
}

pub const fn write_persistence_global(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"INSERT INTO @db_name.persistence_globals
                ({instance_col} `key`, json_value)
                VALUES ({instance_val} ?, ?)
                ON DUPLICATE KEY UPDATE
                json_value = VALUES(json_value)
            "#,
            instance_col = if multitenant { "instance_name," } else { "" },
            instance_val = if multitenant { "?," } else { "" }
        )
    )
}

pub const fn get_persistence_global(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"SELECT json_value FROM @db_name.persistence_globals FORCE INDEX (PRIMARY) WHERE `key` = ? {instance_clause}"#,
            instance_clause = if multitenant {
                "AND instance_name = ?"
            } else {
                ""
            }
        )
    )
}

pub const fn set_read_only(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            "INSERT INTO @db_name.read_only ({read_only_col}) VALUES ({read_only_val})",
            read_only_col = if multitenant { "instance_name" } else { "id" },
            read_only_val = if multitenant { "?" } else { "1" }
        )
    )
}

pub const fn unset_read_only(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            "DELETE FROM @db_name.read_only WHERE {read_only_col} = {read_only_val}",
            read_only_col = if multitenant { "instance_name" } else { "id" },
            read_only_val = if multitenant { "?" } else { "1" }
        )
    )
}

// If this query returns a result, the lease is still valid and will remain so
// until the end of the transaction.
pub const fn lease_precond(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            "SELECT 1 FROM @db_name.leases FORCE INDEX (PRIMARY) WHERE ts=? AND {lease_cond} FOR \
             SHARE",
            lease_cond = if multitenant {
                "instance_name = ?"
            } else {
                "id = 1"
            }
        )
    )
}

// Acquire the lease unless acquire by someone with a higher timestamp.
pub const fn lease_acquire(multitenant: bool, allow_read_only: bool) -> &'static str {
    tableify!(
        [multitenant, allow_read_only],
        formatcp!(
            "UPDATE @db_name.leases SET ts=? WHERE ts<? AND {lease_cond} {ro_cond}",
            lease_cond = if multitenant {
                "instance_name = ?"
            } else {
                "id = 1"
            },
            ro_cond = if allow_read_only {
                ""
            } else if multitenant {
                "AND NOT EXISTS (SELECT 1 FROM @db_name.read_only WHERE instance_name = ?)"
            } else {
                "AND NOT EXISTS (SELECT 1 FROM @db_name.read_only WHERE id = 1)"
            },
        )
    )
}

// Pre-build queries with various parameters.
//
// Tricks that convince MySQL to choose good query plans:
// 1. All queries are ordered by a prefix of columns in the primary key. If you
//    say `WHERE col1 = 'a' ORDER BY col2 ASC` it might not use the index, but
//    `WHERE col1 = 'a' ORDER BY col1 ASC, col2 ASC` which is completely
//    equivalent, does use the index.
// 2. LEFT JOIN and FORCE INDEX FOR JOIN makes the join use the index for
//    lookups. Despite having all index columns with equality checks, MySQL will
//    do a hash join if you do an INNER JOIN or a plain FORCE INDEX.
// 3. Tuple comparisons `(key_prefix, key_sha256) >= (?, ?)` are required for
//    Postgres to choose the correct query plan, but MySQL requires the other
//    format `(key_prefix > ? OR (key_prefix = ? AND key_sha256 >= ?))`.
//
// Note, we always paginate using (key_prefix, key_sha256), which doesn't
// necessary give us the order we need for long keys that have key_suffix.
static INDEX_QUERIES: LazyLock<[HashMap<(BoundType, BoundType, Order), String>; 2]> = LazyLock::new(
    || {
        let mut single_tenant = HashMap::new();
        let mut multi_tenant = HashMap::new();

        let bounds = [
            BoundType::Unbounded,
            BoundType::Included,
            BoundType::Excluded,
        ];
        let orders = [Order::Asc, Order::Desc];
        let multitenant_options = [false, true];

        for (lower, upper, order, multitenant) in iproduct!(
            bounds.iter(),
            bounds.iter(),
            orders.iter(),
            multitenant_options.iter()
        ) {
            // Build WHERE clause
            let mut where_clause = String::new();
            if *multitenant {
                write!(where_clause, "instance_name = ? AND ").unwrap();
            }
            write!(where_clause, "index_id = ? AND ts <= ?").unwrap();

            // Add bound conditions
            match lower {
                BoundType::Unbounded => {},
                BoundType::Included => {
                    write!(
                        where_clause,
                        " AND (key_prefix > ? OR (key_prefix = ? AND key_sha256 >= ?))",
                    )
                    .unwrap();
                },
                BoundType::Excluded => {
                    write!(
                        where_clause,
                        " AND (key_prefix > ? OR (key_prefix = ? AND key_sha256 > ?))"
                    )
                    .unwrap();
                },
            };
            match upper {
                BoundType::Unbounded => {},
                BoundType::Included => {
                    write!(
                        where_clause,
                        " AND (key_prefix < ? OR (key_prefix = ? AND key_sha256 <= ?))"
                    )
                    .unwrap();
                },
                BoundType::Excluded => {
                    write!(
                        where_clause,
                        " AND (key_prefix < ? OR (key_prefix = ? AND key_sha256 < ?))"
                    )
                    .unwrap();
                },
            };

            let order_str = match order {
                Order::Asc => "ASC",
                Order::Desc => "DESC",
            };

            // Build instance-specific clauses
            let (
                select_instance_col,
                group_by_instance,
                join_instance_i1,
                join_instance_snapshot,
                doc_join_instance_cond,
            ) = if *multitenant {
                (
                    "I1.instance_name, ",
                    "instance_name, ",
                    "I1.instance_name, ",
                    "snapshot.instance_name, ",
                    "D.instance_name = I2.instance_name AND ",
                )
            } else {
                ("", "", "", "", "")
            };

            let query = format!(
                r#"
SELECT I2.index_id, I2.key_prefix, I2.key_sha256, I2.key_suffix, I2.ts, I2.deleted, I2.document_id, D.table_id, D.json_value, D.prev_ts FROM
(
    SELECT
        {select_instance_col}I1.index_id, I1.key_prefix, I1.key_sha256, I1.key_suffix, I1.ts,
        I1.deleted, I1.table_id, I1.document_id
    FROM
    (
        SELECT {group_by_instance}index_id, key_prefix, key_sha256, MAX(ts) as ts_at_snapshot FROM @db_name.indexes
        FORCE INDEX FOR GROUP BY (PRIMARY)
        WHERE {where_clause}
        GROUP BY {group_by_instance}index_id, key_prefix, key_sha256
        ORDER BY index_id {order_str}, key_prefix {order_str}, key_sha256 {order_str}
        LIMIT ?
    ) snapshot
    LEFT JOIN @db_name.indexes I1 FORCE INDEX FOR JOIN (PRIMARY)
    ON
    ({join_instance_i1}I1.index_id, I1.key_prefix, I1.key_sha256, I1.ts) = ({join_instance_snapshot}snapshot.index_id, snapshot.key_prefix, snapshot.key_sha256, snapshot.ts_at_snapshot)
) I2
LEFT JOIN @db_name.documents D FORCE INDEX FOR JOIN (PRIMARY)
ON
{doc_join_instance_cond}D.ts = I2.ts AND D.table_id = I2.table_id AND D.id = I2.document_id AND NOT I2.deleted
ORDER BY I2.key_prefix {order_str}, I2.key_sha256 {order_str}
"#
            );

            if *multitenant {
                multi_tenant.insert((*lower, *upper, *order), query);
            } else {
                single_tenant.insert((*lower, *upper, *order), query);
            }
        }

        [single_tenant, multi_tenant]
    },
);

pub fn index_queries(multitenant: bool) -> &'static HashMap<(BoundType, BoundType, Order), String> {
    &INDEX_QUERIES[multitenant as usize]
}

pub fn index_point_query(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"
SELECT I.ts, I.table_id, D.json_value, D.prev_ts FROM
(
    SELECT {instance_col}ts, deleted, table_id, document_id
    FROM @db_name.indexes
    FORCE INDEX (PRIMARY)
    WHERE index_id = ? AND key_prefix = ? AND key_sha256 = ? AND ts <= ?
    {instance_cond}
    ORDER BY ts DESC
    LIMIT 1
) I
LEFT JOIN @db_name.documents D FORCE INDEX FOR JOIN (PRIMARY)
ON
{instance_join}D.ts = I.ts AND D.table_id = I.table_id AND D.id = I.document_id
WHERE I.deleted = false
"#,
            instance_col = if multitenant { "instance_name, " } else { "" },
            instance_cond = if multitenant {
                "AND instance_name = ?"
            } else {
                ""
            },
            instance_join = if multitenant {
                "D.instance_name = I.instance_name AND "
            } else {
                ""
            }
        )
    )
}

// Multitenant variants of the index queries. Filters by instance_name and
// ensures joins include instance_name so rows from other tenants cannot match.
// (Removed) separate multitenant map; we now use INDEX_QUERIES[bool] with a
// getter.

// TODO: This is now incorrect for multitenant databases.
pub const TABLE_SIZE_QUERY: &str = "
SELECT table_name, data_length, index_length, table_rows
FROM information_schema.tables
WHERE table_schema = ?
";

pub const MIN_SHA256: [u8; 32] = [0; 32];
pub const MAX_SHA256: [u8; 32] = [255; 32];

// The key we use to paginate in SQL, note that we can't use key_suffix since
// it is not part of the primary key. We use key_sha256 instead.
#[derive(Clone)]
pub struct SqlKey {
    pub prefix: Vec<u8>,
    pub sha256: Vec<u8>,
}

impl SqlKey {
    pub fn min_with_same_prefix(key: Vec<u8>) -> Self {
        let key = SplitKey::new(key);
        Self {
            prefix: key.prefix,
            sha256: MIN_SHA256.to_vec(),
        }
    }

    pub fn max_with_same_prefix(key: Vec<u8>) -> Self {
        let key = SplitKey::new(key);
        Self {
            prefix: key.prefix,
            sha256: MAX_SHA256.to_vec(),
        }
    }
}

// Translates a range to a SqlKey bounds we can use to get records in that
// range. Note that because the SqlKey does not sort the same way as IndexKey
// for very long keys, the returned range might contain extra keys that needs to
// be filtered application side.
pub fn to_sql_bounds(interval: Interval) -> (std::ops::Bound<SqlKey>, std::ops::Bound<SqlKey>) {
    use std::ops::Bound;

    let lower = match interval.start {
        StartIncluded(key) => {
            // This can potentially include more results than needed.
            Bound::Included(SqlKey::min_with_same_prefix(key.into()))
        },
    };
    let upper = match interval.end {
        End::Excluded(key) => {
            if key.len() < MAX_INDEX_KEY_PREFIX_LEN {
                Bound::Excluded(SqlKey::min_with_same_prefix(key.into()))
            } else {
                // We can't exclude the bound without potentially excluding other
                // keys that fall within the range.
                Bound::Included(SqlKey::max_with_same_prefix(key.into()))
            }
        },
        End::Unbounded => Bound::Unbounded,
    };
    (lower, upper)
}

pub fn index_query(
    index_id: IndexId,
    read_timestamp: Timestamp,
    lower: std::ops::Bound<SqlKey>,
    upper: std::ops::Bound<SqlKey>,
    order: Order,
    batch_size: usize,
    multitenant: bool,
    instance_name: &MySqlInstanceName,
) -> (&'static str, Vec<mysql_async::Value>) {
    use std::ops::Bound;

    let mut params = vec![];

    fn internal_id_param(id: common::document::InternalId) -> Vec<u8> {
        id.into()
    }

    let mut map_bound = |b: Bound<SqlKey>| -> BoundType {
        match b {
            Bound::Unbounded => BoundType::Unbounded,
            Bound::Excluded(sql_key) => {
                params.push(sql_key.prefix.clone());
                params.push(sql_key.prefix);
                params.push(sql_key.sha256);
                BoundType::Excluded
            },
            Bound::Included(sql_key) => {
                params.push(sql_key.prefix.clone());
                params.push(sql_key.prefix);
                params.push(sql_key.sha256);
                BoundType::Included
            },
        }
    };

    let lt = map_bound(lower);
    let ut = map_bound(upper);

    let query = index_queries(multitenant).get(&(lt, ut, order)).unwrap();
    // Substitutions are {where_clause}, ts, {where_clause}, ts, limit.
    let mut all_params = vec![];
    if multitenant {
        all_params.push((&instance_name.raw).into());
    }
    all_params.push(internal_id_param(index_id.0).into());
    all_params.push(i64::from(read_timestamp).into());
    for param in params {
        all_params.push(param.into());
    }
    all_params.push((batch_size as i64).into());
    (query, all_params)
}
