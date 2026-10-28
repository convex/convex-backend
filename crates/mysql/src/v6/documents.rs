//! V6 document-table definitions and queries.

use std::iter;

use common::query::Order;
use itertools::Itertools;

/// Loads a page of documents, where timestamps are bounded by [?, ?) and the
/// (ts, table_id, id) of the last document read is the cursor. The cursor
/// follows the primary key after its `deployment_id` prefix, which is fixed for
/// the whole page.
pub(crate) fn load_page(
    order: Order,
    tablet_filter: bool,
    include_previous_revision: bool,
) -> String {
    let (comparison, direction) = match order {
        Order::Asc => (">", "ASC"),
        Order::Desc => ("<", "DESC"),
    };
    let previous_revision_column = if include_previous_revision {
        ", P.json_value"
    } else {
        ""
    };
    let previous_revision_join = if include_previous_revision {
        r#"LEFT JOIN @db_name.documents P FORCE INDEX FOR JOIN (PRIMARY)
    ON P.deployment_id = D.deployment_id
    AND P.table_id = D.table_id
    AND P.id = D.id
    AND P.ts = D.prev_ts"#
    } else {
        ""
    };
    let tablet_filter = if tablet_filter {
        "AND D.table_id = ?"
    } else {
        ""
    };
    format!(
        r#"SELECT D.id, D.ts, D.table_id, D.json_value, D.deleted, D.prev_ts{previous_revision_column}
FROM @db_name.documents D FORCE INDEX FOR ORDER BY (PRIMARY)
{previous_revision_join}
WHERE D.deployment_id = ?
AND D.ts >= ?
AND D.ts < ?
AND (D.ts {comparison} ? OR (D.ts = ? AND (D.table_id {comparison} ? OR (D.table_id = ? AND D.id {comparison} ?))))
{tablet_filter}
ORDER BY D.deployment_id {direction}, D.ts {direction}, D.table_id {direction}, D.id {direction}
LIMIT ?"#
    )
}

fn insert_chunk_with(operation: &str, chunk_size: usize) -> String {
    let values = iter::repeat_n("(?, ?, ?, ?, ?, ?, ?)", chunk_size).join(", ");
    format!(
        r#"{operation} INTO @db_name.documents
    (deployment_id, id, ts, table_id, json_value, deleted, prev_ts)
    VALUES {values}"#
    )
}

pub(crate) fn insert_chunk(chunk_size: usize) -> String {
    insert_chunk_with("INSERT", chunk_size)
}

pub(crate) fn insert_overwrite_chunk(chunk_size: usize) -> String {
    insert_chunk_with("REPLACE", chunk_size)
}

pub(crate) fn delete_chunk(chunk_size: usize) -> String {
    let entries = iter::repeat_n("(table_id = ? AND id = ? AND ts <= ?)", chunk_size).join(" OR ");
    // Note the use of "multi-table DELETE syntax" (`DELETE table FROM table
    // WHERE ...`) which MySQL requires for FORCE INDEX syntax.
    format!(
        "DELETE @db_name.documents FROM @db_name.documents FORCE INDEX \
         (documents_by_table_and_id) WHERE deployment_id = ? AND ({entries})"
    )
}

pub(crate) fn delete_tablet_chunk() -> &'static str {
    r#"DELETE /*+ INDEX(documents documents_by_table_and_id) */ FROM @db_name.documents
WHERE deployment_id = ? AND table_id = ? LIMIT ?"#
}

// Gross: after initialization, the first thing database does is insert metadata
// documents.
pub(crate) fn check_newly_created() -> &'static str {
    "SELECT 1 FROM @db_name.documents WHERE deployment_id = ? LIMIT 1"
}

pub(crate) fn exact_revision_chunk(chunk_size: usize) -> String {
    let entries = iter::repeat_n("(table_id = ? AND id = ? AND ts = ?)", chunk_size).join(" OR ");
    format!(
        r#"SELECT id, ts, table_id, json_value, deleted, prev_ts
FROM @db_name.documents FORCE INDEX (PRIMARY)
WHERE deployment_id = ? AND ({entries})
ORDER BY deployment_id ASC, ts ASC, table_id ASC, id ASC"#
    )
}

pub(crate) fn previous_revision_chunk(chunk_size: usize) -> String {
    let arm = r#"(SELECT id, ts, table_id, json_value, deleted, prev_ts, ? AS query_ts
FROM @db_name.documents FORCE INDEX (documents_by_table_and_id)
WHERE deployment_id = ? AND table_id = ? AND id = ? AND ts < ?
ORDER BY deployment_id DESC, table_id DESC, id DESC, ts DESC LIMIT 1)"#;
    iter::repeat_n(arm, chunk_size).join(" UNION ALL ")
}
