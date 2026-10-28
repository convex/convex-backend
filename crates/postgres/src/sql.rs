use std::{
    collections::HashMap,
    fmt::Write,
    sync::LazyLock,
};

use common::query::Order;
use const_format::formatcp;
use itertools::iproduct;

macro_rules! as_table {
    ([$param:ident $(, $rest:ident)*], $e: expr) => {{
        [{
            #[allow(non_upper_case_globals)]
            const $param: bool = false;
            as_table!([$($rest),*], $e)
        }, {
            #[allow(non_upper_case_globals)]
            const $param: bool = true;
            as_table!([$($rest),*], $e)
        }]
    }};
    ([], $e: expr) => { $e }
}
/// Returns the appropriate expression based on the parameter value.
macro_rules! tableify {
    ([$($param:ident),+], $e: expr) => {{
        as_table!([$($param),*], $e)
            $(
                [$param as usize]
            )*
    }};
    ($param:ident, $e: expr) => {
        tableify!([$param], $e)
    };
}

pub(crate) const CHECK_SCHEMA_SQL: &str =
    r"SELECT 1 FROM information_schema.schemata WHERE schema_name = $1";

pub(crate) const CREATE_SCHEMA_SQL: &str = r"CREATE SCHEMA IF NOT EXISTS @db_name;";

// This runs (currently) every time a PostgresPersistence is created, so it
// needs to not only be idempotent but not to affect any already-resident data.
// IF NOT EXISTS and ON CONFLICT are helpful.
// Despite the idempotence of IF NOT EXISTS, we still use a conditional check to
// see if we can avoid running that statement, as it acquires an `ACCESS
// EXCLUSIVE` lock across the database.
pub const fn init_sql(multitenant: bool) -> &'static [(&'static str, bool /* is CREATE INDEX */)] {
    tableify!(
        multitenant,
        &[
            (
                formatcp!(
                    r#"
DO $$
BEGIN
    IF to_regclass('@db_name.documents') IS NULL THEN
        CREATE TABLE IF NOT EXISTS @db_name.documents (
            {instance_col}
            id BYTEA NOT NULL,
            ts BIGINT NOT NULL,

            table_id BYTEA NOT NULL,

            json_value BYTEA NOT NULL,
            deleted BOOLEAN DEFAULT false,

            prev_ts BIGINT
        );
    END IF;
END $$;
"#,
                    instance_col = if multitenant {
                        "instance_name TEXT NOT NULL,"
                    } else {
                        ""
                    }
                ),
                false,
            ),
            (
                formatcp!(
                    r#"
DO $$
BEGIN
    IF to_regclass('@db_name.documents_pkey') IS NULL THEN
        ALTER TABLE @db_name.documents ADD PRIMARY KEY ({instance_col} ts, table_id, id);
    END IF;
    IF to_regclass('@db_name.documents_by_table_and_id') IS NULL THEN
        CREATE INDEX IF NOT EXISTS documents_by_table_and_id ON @db_name.documents (
            {instance_col} table_id, id, ts
        );
    END IF;
    IF to_regclass('@db_name.documents_by_table_ts_and_id') IS NULL THEN
        CREATE INDEX IF NOT EXISTS documents_by_table_ts_and_id ON @db_name.documents (
            {instance_col} table_id, ts, id
        );
    END IF;
END $$;
"#,
                    instance_col = if multitenant { "instance_name," } else { "" }
                ),
                true,
            ),
            (
                formatcp!(
                    r#"
DO $$
BEGIN
    IF to_regclass('@db_name.indexes') IS NULL THEN
        CREATE TABLE IF NOT EXISTS @db_name.indexes (
            {instance_col}
            /* ids should be serialized as bytes but we keep it compatible with documents */
            index_id BYTEA NOT NULL,
            ts BIGINT NOT NULL,
            /*
            Postgres maximum primary key length is 2730 bytes, which
            is why we split up the key. The first 2500 bytes are stored in key_prefix,
            and the remaining ones are stored in key suffix if applicable.
            NOTE: The key_prefix + key_suffix is store all values of IndexKey including
            the id.
            */
            key_prefix BYTEA NOT NULL,
            key_suffix BYTEA NULL,

            /* key_sha256 of the full key, used in primary key to avoid duplicates in case
            of key_prefix collision. */
            key_sha256 BYTEA NOT NULL,

            deleted BOOLEAN,

            /* table_id should be populated iff deleted is false. */
            table_id BYTEA NULL,
            /* document_id should be populated iff deleted is false. */
            document_id BYTEA NULL
        );
    END IF;
END $$;
"#,
                    instance_col = if multitenant {
                        "instance_name TEXT NOT NULL,"
                    } else {
                        ""
                    }
                ),
                false,
            ),
            (
                formatcp!(
                    r#"
DO $$
BEGIN
    IF to_regclass('@db_name.indexes_pkey') IS NULL THEN
        ALTER TABLE @db_name.indexes ADD PRIMARY KEY ({instance_col} index_id, key_sha256, ts);
    END IF;
    /* We only want this index created for new instances; existing ones already have `indexes_by_index_id_key_prefix_key_sha256_ts` */
    IF to_regclass('@db_name.indexes_by_index_id_key_prefix_key_sha256_ts') IS NULL AND to_regclass('@db_name.indexes_by_index_id_key_prefix_key_sha256') IS NULL THEN
        CREATE INDEX IF NOT EXISTS indexes_by_index_id_key_prefix_key_sha256 ON @db_name.indexes (
            {instance_col}
            index_id,
            key_prefix,
            key_sha256
        );
    END IF;
END $$;
"#,
                    instance_col = if multitenant { "instance_name," } else { "" }
                ),
                true,
            ),
            (
                formatcp!(
                    r#"
DO $$
BEGIN
    IF to_regclass('@db_name.leases') IS NULL THEN
        CREATE TABLE IF NOT EXISTS @db_name.leases (
            {id_col},
            ts BIGINT NOT NULL,

            PRIMARY KEY ({pk})
        );
    END IF;
END $$;
"#,
                    id_col = if multitenant {
                        "instance_name TEXT NOT NULL"
                    } else {
                        "id BIGINT NOT NULL"
                    },
                    pk = if multitenant { "instance_name" } else { "id" }
                ),
                false,
            ),
            (
                formatcp!(
                    r#"
DO $$
BEGIN
    IF to_regclass('@db_name.read_only') IS NULL THEN
        CREATE TABLE IF NOT EXISTS @db_name.read_only (
            {id_col},

            PRIMARY KEY ({pk})
        );
    END IF;
END $$;
"#,
                    id_col = if multitenant {
                        "instance_name TEXT NOT NULL"
                    } else {
                        "id BIGINT NOT NULL"
                    },
                    pk = if multitenant { "instance_name" } else { "id" }
                ),
                false,
            ),
            (
                formatcp!(
                    r#"
DO $$
BEGIN
    IF to_regclass('@db_name.persistence_globals') IS NULL THEN
        CREATE TABLE IF NOT EXISTS @db_name.persistence_globals (
            {instance_col}
            key TEXT NOT NULL,
            json_value BYTEA NOT NULL,
            PRIMARY KEY ({instance_pk} key)
            );
    END IF;
END $$;
"#,
                    instance_col = if multitenant {
                        "instance_name TEXT NOT NULL,"
                    } else {
                        ""
                    },
                    instance_pk = if multitenant { "instance_name," } else { "" }
                ),
                false,
            ),
            (
                formatcp!(
                    r#"
        INSERT INTO @db_name.leases ({id_col}, ts) VALUES ({id_val}, 0) ON CONFLICT DO NOTHING;
    "#,
                    id_col = if multitenant { "instance_name" } else { "id" },
                    id_val = if multitenant { "@instance_name" } else { "1" }
                ),
                false,
            ),
        ]
    )
}

pub(crate) const TABLES: &[&str] = &[
    "documents",
    "indexes",
    "leases",
    "read_only",
    "persistence_globals",
];

/// Load a page of documents in ascending order.
///
/// N.B.: it's important to provide only one bound on each side of the index
/// range - otherwise postgres may choose the wrong bounds for its index scan
pub const fn load_docs_by_ts_page_asc(
    multitenant: bool,
    tablet_filter: bool,
    include_prev_rev: bool,
) -> &'static str {
    tableify!([multitenant, tablet_filter, include_prev_rev], {
        const TABLET_ID_FILTER: &str = if tablet_filter {
            "AND D.table_id = $6"
        } else {
            ""
        };
        const INSTANCE_NAME_FILTER: &str = if multitenant {
            formatcp!(
                "AND D.instance_name = ${}",
                if tablet_filter { 7i32 } else { 6 }
            )
        } else {
            ""
        };
        formatcp!(
            r#"
/*+
    Set(enable_seqscan OFF)
    Set(enable_sort OFF)
    Set(enable_incremental_sort OFF)
    Set(enable_hashjoin OFF)
    Set(enable_mergejoin OFF)
    Set(enable_material OFF)
    Set(plan_cache_mode force_generic_plan)
*/
SELECT D.id, D.ts, D.table_id, D.json_value, D.deleted, D.prev_ts
    {prev_rev_col}
    FROM @db_name.documents D
    {prev_rev_join}
    WHERE (D.ts, D.table_id, D.id) > ($1, $2, $3)
    AND D.ts < $4
    {tablet_filter}
    {instance_name_filter}
    ORDER BY D.ts ASC, D.table_id ASC, D.id ASC
    LIMIT $5
"#,
            prev_rev_col = if include_prev_rev {
                ", P.json_value"
            } else {
                ""
            },
            prev_rev_join = if include_prev_rev {
                formatcp!(
                    "LEFT JOIN @db_name.documents P
                    ON P.table_id = D.table_id
                    AND P.id = D.id
                    AND P.ts = D.prev_ts
                    {}",
                    if multitenant {
                        "AND P.instance_name = D.instance_name"
                    } else {
                        ""
                    }
                )
            } else {
                ""
            },
            tablet_filter = TABLET_ID_FILTER,
            instance_name_filter = INSTANCE_NAME_FILTER,
        )
    })
}

/// Load a page of documents in descending order.
/// Note that the parameters are different than LOAD_DOCS_BY_TS_PAGE_ASC.
pub const fn load_docs_by_ts_page_desc(
    multitenant: bool,
    tablet_filter: bool,
    include_prev_rev: bool,
) -> &'static str {
    tableify!([multitenant, tablet_filter, include_prev_rev], {
        const TABLET_ID_FILTER: &str = if tablet_filter {
            "AND D.table_id = $6"
        } else {
            ""
        };
        const INSTANCE_NAME_FILTER: &str = if multitenant {
            formatcp!(
                "AND D.instance_name = ${}",
                if tablet_filter { 7i32 } else { 6 }
            )
        } else {
            ""
        };
        formatcp!(
            r#"
/*+
    Set(enable_seqscan OFF)
    Set(enable_sort OFF)
    Set(enable_incremental_sort OFF)
    Set(enable_hashjoin OFF)
    Set(enable_mergejoin OFF)
    Set(enable_material OFF)
    Set(plan_cache_mode force_generic_plan)
*/
SELECT D.id, D.ts, D.table_id, D.json_value, D.deleted, D.prev_ts
    {prev_rev_col}
    FROM @db_name.documents D
    {prev_rev_join}
    WHERE D.ts >= $1
    AND (D.ts, D.table_id, D.id) < ($2, $3, $4)
    {tablet_filter}
    {instance_name_filter}
    ORDER BY D.ts DESC, D.table_id DESC, D.id DESC
    LIMIT $5
"#,
            prev_rev_col = if include_prev_rev {
                ", P.json_value"
            } else {
                ""
            },
            prev_rev_join = if include_prev_rev {
                formatcp!(
                    "LEFT JOIN @db_name.documents P
        ON P.table_id = D.table_id
        AND P.id = D.id
        AND P.ts = D.prev_ts
        {}",
                    if multitenant {
                        "AND P.instance_name = D.instance_name"
                    } else {
                        ""
                    }
                )
            } else {
                ""
            },
            tablet_filter = TABLET_ID_FILTER,
            instance_name_filter = INSTANCE_NAME_FILTER,
        )
    })
}

pub const fn insert_document(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"INSERT INTO @db_name.documents
    ({instance_col} id, ts, table_id, json_value, deleted, prev_ts)
    SELECT {select_clause} FROM UNNEST(
        $1::BYTEA[],
        $2::BIGINT[],
        $3::BYTEA[],
        $4::BYTEA[],
        $5::BOOLEAN[],
        $6::BIGINT[]
    )
"#,
            instance_col = if multitenant { "instance_name," } else { "" },
            select_clause = if multitenant { "$7, *" } else { "*" }
        )
    )
}

pub const fn insert_overwrite_document(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"INSERT INTO @db_name.documents
    ({instance_col} id, ts, table_id, json_value, deleted, prev_ts)
    SELECT {select_clause} FROM UNNEST(
        $1::BYTEA[],
        $2::BIGINT[],
        $3::BYTEA[],
        $4::BYTEA[],
        $5::BOOLEAN[],
        $6::BIGINT[]
    )
    ON CONFLICT ON CONSTRAINT documents_pkey DO UPDATE
    SET deleted = excluded.deleted, json_value = excluded.json_value
"#,
            instance_col = if multitenant { "instance_name," } else { "" },
            select_clause = if multitenant { "$7, *" } else { "*" }
        )
    )
}

pub const fn has_index_entries(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            "SELECT 1 FROM @db_name.indexes{where_clause} LIMIT 1",
            where_clause = if multitenant {
                " WHERE instance_name = $1"
            } else {
                ""
            }
        )
    )
}

pub const fn insert_index(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"INSERT INTO @db_name.indexes
    ({instance_col} index_id, ts, key_prefix, key_suffix, key_sha256, deleted, table_id, document_id)
    SELECT {select_clause} FROM UNNEST(
        $1::BYTEA[],
        $2::BIGINT[],
        $3::BYTEA[],
        $4::BYTEA[],
        $5::BYTEA[],
        $6::BOOLEAN[],
        $7::BYTEA[],
        $8::BYTEA[]
    )
"#,
            instance_col = if multitenant { "instance_name," } else { "" },
            select_clause = if multitenant { "$9, *" } else { "*" }
        )
    )
}

// Note that on conflict, there's no need to update any of the columns that are
// part of the primary key, nor `key_suffix` as `key_sha256` is derived from the
// prefix and suffix.
// Only the fields that could have actually changed need to be updated.
pub const fn insert_overwrite_index(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"INSERT INTO @db_name.indexes
    ({instance_col} index_id, ts, key_prefix, key_suffix, key_sha256, deleted, table_id, document_id)
    SELECT {select_clause} FROM UNNEST(
        $1::BYTEA[],
        $2::BIGINT[],
        $3::BYTEA[],
        $4::BYTEA[],
        $5::BYTEA[],
        $6::BOOLEAN[],
        $7::BYTEA[],
        $8::BYTEA[]
    )
    ON CONFLICT ON CONSTRAINT indexes_pkey DO UPDATE
    SET deleted = excluded.deleted, table_id = excluded.table_id, document_id = excluded.document_id
"#,
            instance_col = if multitenant { "instance_name," } else { "" },
            select_clause = if multitenant { "$9, *" } else { "*" }
        )
    )
}

pub const fn delete_index(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"
/*+
    Set(enable_seqscan OFF)
    Set(plan_cache_mode force_generic_plan)
*/
DELETE FROM @db_name.indexes WHERE
    (index_id = $1 AND key_prefix = $2 AND key_sha256 = $3 AND ts <= $4{instance_clause})
"#,
            instance_clause = if multitenant {
                " AND instance_name = $5"
            } else {
                ""
            }
        )
    )
}

pub const fn delete_document(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"
/*+
    Set(enable_seqscan OFF)
    Set(plan_cache_mode force_generic_plan)
*/
DELETE FROM @db_name.documents WHERE
    (table_id = $1 AND id = $2 AND ts <= $3{instance_clause})
"#,
            instance_clause = if multitenant {
                " AND instance_name = $4"
            } else {
                ""
            }
        )
    )
}

pub const fn delete_index_chunk(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"
/*+
    Set(enable_seqscan OFF)
    Set(plan_cache_mode force_generic_plan)
*/
DELETE FROM @db_name.indexes WHERE
    (index_id = $1 AND key_prefix = $2 AND key_sha256 = $3 AND ts <= $4{instance_clause}) OR
    (index_id = $5 AND key_prefix = $6 AND key_sha256 = $7 AND ts <= $8{instance_clause}) OR
    (index_id = $9 AND key_prefix = $10 AND key_sha256 = $11 AND ts <= $12{instance_clause}) OR
    (index_id = $13 AND key_prefix = $14 AND key_sha256 = $15 AND ts <= $16{instance_clause}) OR
    (index_id = $17 AND key_prefix = $18 AND key_sha256 = $19 AND ts <= $20{instance_clause}) OR
    (index_id = $21 AND key_prefix = $22 AND key_sha256 = $23 AND ts <= $24{instance_clause}) OR
    (index_id = $25 AND key_prefix = $26 AND key_sha256 = $27 AND ts <= $28{instance_clause}) OR
    (index_id = $29 AND key_prefix = $30 AND key_sha256 = $31 AND ts <= $32{instance_clause})
"#,
            instance_clause = if multitenant {
                " AND instance_name = $33"
            } else {
                ""
            }
        )
    )
}

pub const fn delete_document_chunk(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"
/*+
    Set(enable_seqscan OFF)
    Set(plan_cache_mode force_generic_plan)
*/
DELETE FROM @db_name.documents WHERE
    (table_id = $1 AND id = $2 AND ts <= $3{instance_clause}) OR
    (table_id = $4 AND id = $5 AND ts <= $6{instance_clause}) OR
    (table_id = $7 AND id = $8 AND ts <= $9{instance_clause}) OR
    (table_id = $10 AND id = $11 AND ts <= $12{instance_clause}) OR
    (table_id = $13 AND id = $14 AND ts <= $15{instance_clause}) OR
    (table_id = $16 AND id = $17 AND ts <= $18{instance_clause}) OR
    (table_id = $19 AND id = $20 AND ts <= $21{instance_clause}) OR
    (table_id = $22 AND id = $23 AND ts <= $24{instance_clause})
"#,
            instance_clause = if multitenant {
                " AND instance_name = $25"
            } else {
                ""
            }
        )
    )
}

pub const fn delete_tablet_chunk(multitenant: bool) -> &'static str {
    tableify!(multitenant, {
        const LIMIT_PARAM: i32 = if multitenant { 3 } else { 2 };
        formatcp!(
            r#"DELETE FROM @db_name.documents WHERE table_id = $1 AND id IN (
            SELECT id FROM @db_name.documents
            WHERE table_id = $1{instance_clause}
            LIMIT ${limit_param}
        ){instance_clause}
"#,
            instance_clause = if multitenant {
                " AND instance_name = $2"
            } else {
                ""
            },
            limit_param = LIMIT_PARAM,
        )
    })
}

pub const fn write_persistence_global(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"INSERT INTO @db_name.persistence_globals
    ({instance_col} key, json_value)
    VALUES ({values_clause} $1, $2)
    ON CONFLICT ON CONSTRAINT persistence_globals_pkey DO UPDATE
    SET json_value = excluded.json_value
"#,
            instance_col = if multitenant { "instance_name," } else { "" },
            values_clause = if multitenant { "$3," } else { "" }
        )
    )
}

pub const fn get_persistence_global(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            "SELECT json_value FROM @db_name.persistence_globals WHERE key = $1{instance_clause}",
            instance_clause = if multitenant {
                " AND instance_name = $2"
            } else {
                ""
            }
        )
    )
}

// Gross: after initialization, the first thing database does is insert metadata
// documents.
pub const fn check_newly_created(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            "SELECT 1 FROM @db_name.documents{where_clause} LIMIT 1",
            where_clause = if multitenant {
                " WHERE instance_name = $1"
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
            "INSERT INTO @db_name.read_only ({column}) VALUES ({value})",
            column = if multitenant { "instance_name" } else { "id" },
            value = if multitenant { "$1" } else { "1" }
        )
    )
}

pub const fn unset_read_only(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            "DELETE FROM @db_name.read_only WHERE {column} = {value}",
            column = if multitenant { "instance_name" } else { "id" },
            value = if multitenant { "$1" } else { "1" }
        )
    )
}

// If this query returns a result, the lease is still valid and will remain so
// until the end of the transaction.
pub const fn lease_precond(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            "SELECT 1 FROM @db_name.leases WHERE {column}={value} AND ts=$1 FOR SHARE",
            column = if multitenant { "instance_name" } else { "id" },
            value = if multitenant { "$2" } else { "1" }
        )
    )
}

// Checks if we still hold the lease without blocking another instance from
// stealing it.
pub const fn advisory_lease_check(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            "SELECT 1 FROM @db_name.leases WHERE {column}={value} AND ts=$1",
            column = if multitenant { "instance_name" } else { "id" },
            value = if multitenant { "$2" } else { "1" }
        )
    )
}

// Acquire the lease unless acquire by someone with a higher timestamp.
pub const fn lease_acquire(multitenant: bool, allow_read_only: bool) -> &'static str {
    tableify!(
        [multitenant, allow_read_only],
        formatcp!(
            "UPDATE @db_name.leases SET ts=$1 WHERE {column}={value} AND ts<$1 {ro_cond}",
            column = if multitenant { "instance_name" } else { "id" },
            value = if multitenant { "$2" } else { "1" },
            ro_cond = if allow_read_only {
                ""
            } else if multitenant {
                "AND NOT EXISTS (SELECT 1 FROM @db_name.read_only WHERE instance_name = $2)"
            } else {
                "AND NOT EXISTS (SELECT 1 FROM @db_name.read_only WHERE id = 1)"
            },
        )
    )
}

#[derive(PartialEq, Eq, Hash, Copy, Clone)]
pub(crate) enum BoundType {
    Unbounded,
    Included,
    Excluded,
}

// Pre-build queries with various parameters for both single-tenant and
// multitenant modes.
pub(crate) static INDEX_QUERIES: LazyLock<[HashMap<(BoundType, BoundType, Order), String>; 2]> =
    LazyLock::new(|| {
        let mut single_tenant_queries = HashMap::new();
        let mut multitenant_queries = HashMap::new();

        let bounds = [
            BoundType::Unbounded,
            BoundType::Included,
            BoundType::Excluded,
        ];
        let orders = [Order::Asc, Order::Desc];
        let multitenant_options = [false, true];

        // Note, we always paginate using (key_prefix, key_sha256), which doesn't
        // necessary give us the order we need for long keys that have
        // key_suffix.
        for (lower, upper, order, multitenant) in iproduct!(
            bounds.iter(),
            bounds.iter(),
            orders.iter(),
            multitenant_options.iter()
        ) {
            // Construct the where clause imperatively.
            let mut current_arg = 1..;
            let mut next_arg = || current_arg.next().unwrap();

            let mut where_clause = String::new();
            write!(where_clause, "index_id = ${}", next_arg()).unwrap();
            let ts_arg = next_arg();
            write!(where_clause, " AND ts <= ${ts_arg}").unwrap();
            match lower {
                BoundType::Unbounded => {},
                BoundType::Included => {
                    write!(
                        where_clause,
                        " AND (key_prefix, key_sha256) >= (${}, ${})",
                        next_arg(),
                        next_arg(),
                    )
                    .unwrap();
                },
                BoundType::Excluded => {
                    write!(
                        where_clause,
                        " AND (key_prefix, key_sha256) > (${}, ${})",
                        next_arg(),
                        next_arg(),
                    )
                    .unwrap();
                },
            };
            match upper {
                BoundType::Unbounded => {},
                BoundType::Included => {
                    write!(
                        where_clause,
                        " AND (key_prefix, key_sha256) <= (${}, ${})",
                        next_arg(),
                        next_arg(),
                    )
                    .unwrap();
                },
                BoundType::Excluded => {
                    write!(
                        where_clause,
                        " AND (key_prefix, key_sha256) < (${}, ${})",
                        next_arg(),
                        next_arg(),
                    )
                    .unwrap();
                },
            };

            let limit_arg = next_arg();

            // Add instance_name clauses for multitenant
            let (indexes_instance_clause, documents_instance_clause) = if *multitenant {
                let instance_arg = next_arg();
                (
                    format!(" AND instance_name = ${instance_arg}"),
                    format!(" AND D.instance_name = ${instance_arg}"),
                )
            } else {
                ("".to_string(), "".to_string())
            };

            let order_str = match order {
                Order::Asc => "ASC",
                Order::Desc => "DESC",
            };
            let query = format!(
                r#"
/*+
    Set(enable_seqscan OFF)
    Set(enable_bitmapscan OFF)
    Set(plan_cache_mode force_generic_plan)
    IndexScan(indexes indexes_index_id_key_prefix_key_sha256)
    NestLoop(a d)
    IndexScan(d documents_pkey)
*/
SELECT
    A.index_id,
    A.key_prefix,
    A.key_sha256,
    A.key_suffix,
    A.ts,
    A.deleted,
    A.document_id,
    D.table_id,
    D.json_value,
    D.prev_ts
FROM (
    SELECT DISTINCT ON (key_prefix, key_sha256)
        index_id,
        key_prefix,
        key_sha256,
        key_suffix,
        ts,
        deleted,
        document_id,
        table_id
    FROM @db_name.indexes
    WHERE {where_clause}{indexes_instance_clause}
    ORDER BY key_prefix {order_str}, key_sha256 {order_str}, ts DESC
    LIMIT ${limit_arg}
) A
LEFT JOIN @db_name.documents D
    ON  D.ts          = A.ts
    AND D.table_id    = A.table_id
    AND D.id          = A.document_id{documents_instance_clause}
ORDER BY key_prefix {order_str}, key_sha256 {order_str}
"#,
            );

            // Insert into appropriate HashMap
            if *multitenant {
                multitenant_queries.insert((*lower, *upper, *order), query);
            } else {
                single_tenant_queries.insert((*lower, *upper, *order), query);
            }
        }

        [single_tenant_queries, multitenant_queries]
    });

pub fn index_queries(multitenant: bool) -> &'static HashMap<(BoundType, BoundType, Order), String> {
    &INDEX_QUERIES[multitenant as usize]
}

pub const fn prev_rev_chunk(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"
/*+
    Set(enable_seqscan OFF)
    Set(enable_sort OFF)
    Set(enable_incremental_sort OFF)
    Set(enable_hashjoin OFF)
    Set(enable_mergejoin OFF)
    Set(enable_material OFF)
    Set(plan_cache_mode force_generic_plan)
*/
WITH
    q1 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $3::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $1 AND id = $2 and ts < $3{instance_clause} ORDER BY ts DESC LIMIT 1),
    q2 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $6::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $4 AND id = $5 and ts < $6{instance_clause} ORDER BY ts DESC LIMIT 1),
    q3 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $9::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $7 AND id = $8 and ts < $9{instance_clause} ORDER BY ts DESC LIMIT 1),
    q4 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $12::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $10 AND id = $11 and ts < $12{instance_clause} ORDER BY ts DESC LIMIT 1),
    q5 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $15::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $13 AND id = $14 and ts < $15{instance_clause} ORDER BY ts DESC LIMIT 1),
    q6 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $18::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $16 AND id = $17 and ts < $18{instance_clause} ORDER BY ts DESC LIMIT 1),
    q7 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $21::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $19 AND id = $20 and ts < $21{instance_clause} ORDER BY ts DESC LIMIT 1),
    q8 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $24::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $22 AND id = $23 and ts < $24{instance_clause} ORDER BY ts DESC LIMIT 1)
SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q1
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q2
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q3
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q4
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q5
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q6
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q7
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q8;
"#,
            instance_clause = if multitenant {
                " AND instance_name = $25"
            } else {
                ""
            }
        )
    )
}

pub const fn prev_rev(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"
/*+
    Set(enable_seqscan OFF)
    Set(enable_sort OFF)
    Set(enable_incremental_sort OFF)
    Set(enable_hashjoin OFF)
    Set(enable_mergejoin OFF)
    Set(enable_material OFF)
    Set(plan_cache_mode force_generic_plan)
*/
SELECT id, ts, table_id, json_value, deleted, prev_ts, $3::BIGINT as query_ts
FROM @db_name.documents
WHERE
    table_id = $1 AND
    id = $2 AND
    ts < $3{instance_clause}
ORDER BY ts desc
LIMIT 1
"#,
            instance_clause = if multitenant {
                " AND instance_name = $4"
            } else {
                ""
            }
        )
    )
}

pub const fn exact_rev_chunk(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"
/*+
    Set(enable_seqscan OFF)
    Set(enable_sort OFF)
    Set(enable_incremental_sort OFF)
    Set(enable_hashjoin OFF)
    Set(enable_mergejoin OFF)
    Set(enable_material OFF)
    Set(plan_cache_mode force_generic_plan)
*/
WITH
    q1 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $4::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $1 AND id = $2 and ts = $3{instance_clause}),
    q2 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $8::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $5 AND id = $6 and ts = $7{instance_clause}),
    q3 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $12::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $9 AND id = $10 and ts = $11{instance_clause}),
    q4 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $16::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $13 AND id = $14 and ts = $15{instance_clause}),
    q5 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $20::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $17 AND id = $18 and ts = $19{instance_clause}),
    q6 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $24::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $21 AND id = $22 and ts = $23{instance_clause}),
    q7 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $28::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $25 AND id = $26 and ts = $27{instance_clause}),
    q8 AS (SELECT id, ts, table_id, json_value, deleted, prev_ts, $32::BIGINT as query_ts FROM @db_name.documents WHERE table_id = $29 AND id = $30 and ts = $31{instance_clause})
SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q1
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q2
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q3
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q4
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q5
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q6
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q7
UNION ALL SELECT id, ts, table_id, json_value, deleted, prev_ts, query_ts FROM q8;
"#,
            instance_clause = if multitenant {
                " AND instance_name = $33"
            } else {
                ""
            }
        )
    )
}

pub const fn exact_rev(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"
/*+
    Set(enable_seqscan OFF)
    Set(enable_sort OFF)
    Set(enable_incremental_sort OFF)
    Set(enable_hashjoin OFF)
    Set(enable_mergejoin OFF)
    Set(enable_material OFF)
    Set(plan_cache_mode force_generic_plan)
*/
SELECT id, ts, table_id, json_value, deleted, prev_ts, $4::BIGINT as query_ts
FROM @db_name.documents
WHERE
    table_id = $1 AND
    id = $2 AND
    ts = $3{instance_clause}
"#,
            instance_clause = if multitenant {
                " AND instance_name = $5"
            } else {
                ""
            }
        )
    )
}

// N.B.: tokio-postgres doesn't know how to create regclass values
pub(crate) const TABLE_SIZE_QUERY: &str = r"SELECT
pg_table_size($1::text::regclass),
pg_indexes_size($1::text::regclass),
(SELECT reltuples::bigint FROM pg_class WHERE oid = $1::text::regclass)";

pub const fn import_documents_batch(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            "COPY @db_name.documents ({instance_col} id, ts, table_id, json_value, deleted, \
             prev_ts) FROM STDIN BINARY",
            instance_col = if multitenant { "instance_name," } else { "" }
        )
    )
}

pub const fn import_indexes_batch(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            "COPY @db_name.indexes ({instance_col} index_id, ts, key_prefix, key_suffix, \
             key_sha256, deleted, table_id, document_id) FROM STDIN BINARY",
            instance_col = if multitenant { "instance_name," } else { "" }
        )
    )
}
