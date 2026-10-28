//! V5 document-table SQL.

use std::{
    collections::HashMap,
    iter,
    sync::LazyLock,
};

use const_format::formatcp;
use itertools::Itertools;

use crate::{
    chunks::smart_chunk_sizes,
    sql::common::{
        as_table,
        tableify,
    },
};

/// Load a page of documents, where timestamps are bounded by [$1, $2),
/// and ($3, $4, $5) is the (ts, table_id, id) from the last document read.
pub const fn load_docs_by_ts_page_asc(
    multitenant: bool,
    tablet_filter: bool,
    include_prev_rev: bool,
) -> &'static str {
    tableify!([multitenant, tablet_filter, include_prev_rev], {
        formatcp!(
            r#"SELECT D.id, D.ts, D.table_id, D.json_value, D.deleted, D.prev_ts
    {prev_rev_col}
    FROM @db_name.documents D
    FORCE INDEX FOR ORDER BY (PRIMARY)
    {prev_rev_join}
    WHERE D.ts >= ?
    AND D.ts < ?
    AND (D.ts > ? OR (D.ts = ? AND (D.table_id > ? OR (D.table_id = ? AND D.id > ?))))
    {tablet_filter}
    {instance_name_filter}
    ORDER BY D.ts ASC, D.table_id ASC, D.id ASC
    LIMIT ?
"#,
            prev_rev_col = if include_prev_rev {
                ", P.json_value"
            } else {
                ""
            },
            prev_rev_join = if include_prev_rev {
                formatcp!(
                    "LEFT JOIN @db_name.documents P
                    FORCE INDEX FOR JOIN (PRIMARY)
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
            tablet_filter = if tablet_filter {
                "AND D.table_id = ?"
            } else {
                ""
            },
            instance_name_filter = if multitenant {
                "AND D.instance_name = ?"
            } else {
                ""
            },
        )
    })
}

pub const fn load_docs_by_ts_page_desc(
    multitenant: bool,
    tablet_filter: bool,
    include_prev_rev: bool,
) -> &'static str {
    tableify!([multitenant, tablet_filter, include_prev_rev], {
        formatcp!(
            r#"SELECT D.id, D.ts, D.table_id, D.json_value, D.deleted, D.prev_ts
    {prev_rev_col}
    FROM @db_name.documents D
    FORCE INDEX FOR ORDER BY (PRIMARY)
    {prev_rev_join}
    WHERE D.ts >= ?
    AND D.ts < ?
    AND (D.ts < ? OR (D.ts = ? AND (D.table_id < ? OR (D.table_id = ? AND D.id < ?))))
    {tablet_filter}
    {instance_name_filter}
    ORDER BY D.ts DESC, D.table_id DESC, D.id DESC
    LIMIT ?
"#,
            prev_rev_col = if include_prev_rev {
                ", P.json_value"
            } else {
                ""
            },
            prev_rev_join = if include_prev_rev {
                formatcp!(
                    "LEFT JOIN @db_name.documents P
                    FORCE INDEX FOR JOIN (PRIMARY)
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
            tablet_filter = if tablet_filter {
                "AND D.table_id = ?"
            } else {
                ""
            },
            instance_name_filter = if multitenant {
                "AND D.instance_name = ?"
            } else {
                ""
            },
        )
    })
}

pub const INSERT_DOCUMENT_COLUMN_COUNT: usize = 6;

static INSERT_DOCUMENT_CHUNK_QUERIES: LazyLock<HashMap<(usize, bool), String>> =
    LazyLock::new(|| {
        smart_chunk_sizes()
            .flat_map(|chunk_size| {
                [false, true].into_iter().map(move |multitenant| {
                    let query = if multitenant {
                        let values = (1..=chunk_size)
                            .map(|_| "(?, ?, ?, ?, ?, ?, ?)".to_string())
                            .join(", ");
                        format!(
                            r#"INSERT INTO @db_name.documents
    (instance_name, id, ts, table_id, json_value, deleted, prev_ts)
    VALUES {values}"#
                        )
                    } else {
                        let values = (1..=chunk_size)
                            .map(|_| "(?, ?, ?, ?, ?, ?)".to_string())
                            .join(", ");
                        format!(
                            r#"INSERT INTO @db_name.documents
    (id, ts, table_id, json_value, deleted, prev_ts)
    VALUES {values}"#
                        )
                    };
                    ((chunk_size, multitenant), query)
                })
            })
            .collect()
    });

pub fn insert_document_chunk(chunk_size: usize, multitenant: bool) -> &'static str {
    INSERT_DOCUMENT_CHUNK_QUERIES
        .get(&(chunk_size, multitenant))
        .unwrap()
}

static INSERT_OVERWRITE_DOCUMENT_CHUNK_QUERIES: LazyLock<HashMap<(usize, bool), String>> =
    LazyLock::new(|| {
        smart_chunk_sizes()
            .flat_map(|chunk_size| {
                [false, true].into_iter().map(move |multitenant| {
                    let query = if multitenant {
                        let values = (1..=chunk_size)
                            .map(|_| "(?, ?, ?, ?, ?, ?, ?)".to_string())
                            .join(", ");
                        format!(
                            r#"REPLACE INTO @db_name.documents
    (instance_name, id, ts, table_id, json_value, deleted, prev_ts)
    VALUES {values}"#
                        )
                    } else {
                        let values = (1..=chunk_size)
                            .map(|_| "(?, ?, ?, ?, ?, ?)".to_string())
                            .join(", ");
                        format!(
                            r#"REPLACE INTO @db_name.documents
    (id, ts, table_id, json_value, deleted, prev_ts)
    VALUES {values}"#
                        )
                    };
                    ((chunk_size, multitenant), query)
                })
            })
            .collect()
    });

pub fn insert_overwrite_document_chunk(chunk_size: usize, multitenant: bool) -> &'static str {
    INSERT_OVERWRITE_DOCUMENT_CHUNK_QUERIES
        .get(&(chunk_size, multitenant))
        .unwrap()
}

pub const DELETE_DOCUMENT_COLUMN_COUNT: usize = 3;
static DELETE_DOCUMENT_CHUNK_QUERIES: LazyLock<HashMap<(usize, bool), String>> =
    LazyLock::new(|| {
        smart_chunk_sizes()
            .flat_map(|chunk_size| {
                [false, true].into_iter().map(move |multitenant| {
                    let where_clauses = (1..=chunk_size)
                        .map(|_| {
                            if multitenant {
                                "(table_id = ? AND id = ? AND ts <= ? AND instance_name = ?)"
                            } else {
                                "(table_id = ? AND id = ? AND ts <= ?)"
                            }
                        })
                        .join(" OR ");
                    (
                        (chunk_size, multitenant),
                        // Note the use of "multi-table DELETE syntax" (`DELETE table
                        // FROM table WHERE ...`) which MySQL requires for FORCE INDEX
                        // syntax
                        format!(
                            "DELETE @db_name.documents FROM @db_name.documents FORCE INDEX \
                             (documents_by_table_and_id) WHERE {where_clauses}"
                        ),
                    )
                })
            })
            .collect()
    });

pub fn delete_document_chunk(chunk_size: usize, multitenant: bool) -> &'static str {
    DELETE_DOCUMENT_CHUNK_QUERIES
        .get(&(chunk_size, multitenant))
        .unwrap()
}

pub const DELETE_TABLE_COLUMN_COUNT: usize = 2;
static DELETE_TABLET_CHUNK_QUERIES: LazyLock<HashMap<bool, String>> = LazyLock::new(|| {
    [false, true]
        .into_iter()
        .map(move |multitenant| {
            let where_clause = if multitenant {
                "(table_id = ? AND instance_name = ?)"
            } else {
                "(table_id = ?)"
            };
            (
                multitenant,
                format!(
                    "DELETE /*+ INDEX(documents documents_by_table_and_id) */ FROM \
                     @db_name.documents WHERE {where_clause} LIMIT ?",
                ),
            )
        })
        .collect()
});

pub fn delete_tablet_chunk(multitenant: bool) -> &'static str {
    DELETE_TABLET_CHUNK_QUERIES.get(&multitenant).unwrap()
}

// Gross: after initialization, the first thing database does is insert metadata
// documents.
pub const fn check_newly_created(multitenant: bool) -> &'static str {
    tableify!(
        multitenant,
        formatcp!(
            r#"SELECT 1 FROM @db_name.documents {instance_clause} LIMIT 1"#,
            instance_clause = if multitenant {
                "WHERE instance_name = ?"
            } else {
                ""
            }
        )
    )
}

// Parameter count for exact_rev_chunk queries: table_id, id, ts,
// [instance_name]
pub const EXACT_REV_CHUNK_PARAMS: usize = 3;

static EXACT_REV_CHUNK_QUERIES: LazyLock<HashMap<(usize, bool), String>> = LazyLock::new(|| {
    smart_chunk_sizes()
        .flat_map(|chunk_size| {
            [false, true].into_iter().map(move |multitenant| {
                let where_clause = if multitenant {
                    std::iter::repeat_n(
                        "(table_id = ? AND id = ? AND ts = ? AND instance_name = ?)",
                        chunk_size,
                    )
                    .join(" OR ")
                } else {
                    std::iter::repeat_n("(table_id = ? AND id = ? AND ts = ?)", chunk_size)
                        .join(" OR ")
                };
                (
                    (chunk_size, multitenant),
                    format!(
                        "SELECT id, ts, table_id, json_value, deleted, prev_ts
FROM @db_name.documents FORCE INDEX (PRIMARY)
WHERE {where_clause}
ORDER BY ts ASC, table_id ASC, id ASC"
                    ),
                )
            })
        })
        .collect()
});

pub fn exact_rev_chunk(chunk_size: usize, multitenant: bool) -> &'static str {
    EXACT_REV_CHUNK_QUERIES
        .get(&(chunk_size, multitenant))
        .unwrap()
}

// Parameter count for prev_rev_chunk queries: query_ts, table_id, id, ts,
// [instance_name]
pub const PREV_REV_CHUNK_PARAMS: usize = 4;

static PREV_REV_CHUNK_QUERIES: LazyLock<HashMap<(usize, bool), String>> = LazyLock::new(|| {
    smart_chunk_sizes()
        .flat_map(|chunk_size| {
            [false, true].into_iter().map(move |multitenant| {
                let select = if multitenant {
                    r#"
SELECT id, ts, table_id, json_value, deleted, prev_ts, ? as query_ts
FROM @db_name.documents FORCE INDEX (documents_by_table_and_id)
WHERE table_id = ? AND id = ? and ts < ? AND instance_name = ?
ORDER BY table_id DESC, id DESC, ts DESC LIMIT 1
"#
                } else {
                    r#"
SELECT id, ts, table_id, json_value, deleted, prev_ts, ? as query_ts
FROM @db_name.documents FORCE INDEX (documents_by_table_and_id)
WHERE table_id = ? AND id = ? and ts < ?
ORDER BY table_id DESC, id DESC, ts DESC LIMIT 1
"#
                };
                let queries =
                    iter::repeat_n(&format!("({select})"), chunk_size).join(" UNION ALL ");
                ((chunk_size, multitenant), queries)
            })
        })
        .collect()
});

pub fn prev_rev_chunk(chunk_size: usize, multitenant: bool) -> &'static str {
    PREV_REV_CHUNK_QUERIES
        .get(&(chunk_size, multitenant))
        .unwrap()
}
