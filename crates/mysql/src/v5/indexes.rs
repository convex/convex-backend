//! The single-table index format: one `indexes` table holding every revision
//! (`ts` in the primary key), cleaned up by retention deletes.

use common::{
    index::{
        IndexEntry,
        SplitKey,
    },
    persistence::{
        ConflictStrategy,
        PersistenceIndexEntry,
    },
    runtime::Runtime,
    sha256::Sha256,
};

pub(crate) use super::sql::{
    index_point_query,
    index_query,
    to_sql_bounds,
    SqlKey,
};
use super::{
    internal_doc_id_param,
    internal_id_param,
    sql,
    Persistence,
    Reader,
};
use crate::{
    chunks::smart_chunks,
    connection::MySqlTransaction,
};

/// Writes one chunk of index entries: a single insert into the `indexes` table.
pub(crate) async fn write_index_chunk(
    tx: &mut MySqlTransaction<'_>,
    chunk: &[PersistenceIndexEntry],
    conflict_strategy: ConflictStrategy,
    multitenant: bool,
    instance_name: &mysql_async::Value,
) -> anyhow::Result<()> {
    let insert_chunk_query = sql::insert_index_chunk(chunk.len(), multitenant);
    let insert_overwrite_chunk_query = sql::insert_overwrite_index_chunk(chunk.len(), multitenant);
    let insert_index_chunk = match conflict_strategy {
        ConflictStrategy::Error => &insert_chunk_query,
        ConflictStrategy::Overwrite => &insert_overwrite_chunk_query,
    };
    let mut insert_index_chunk_params =
        Vec::with_capacity(chunk.len() * (sql::INSERT_INDEX_COLUMN_COUNT + (multitenant as usize)));
    for update in chunk {
        if multitenant {
            insert_index_chunk_params.push(instance_name.clone());
        }
        index_params(&mut insert_index_chunk_params, update);
    }
    tx.exec_drop(insert_index_chunk, insert_index_chunk_params)
        .await?;
    Ok(())
}

/// Whether any index entries have been written yet.
pub(crate) async fn has_index_entries<RT: Runtime>(p: &Persistence<RT>) -> anyhow::Result<bool> {
    let mut client = p.read_pool.acquire("has_index_entries", &p.db_name).await?;
    let mut params = vec![];
    if p.multitenant {
        params.push(p.instance_name.to_string().into());
    }
    Ok(client
        .query_optional(sql::has_index_entries(p.multitenant), params)
        .await?
        .is_some())
}

/// Deletes the given expired index entries.
pub(crate) async fn delete_index_rows<RT: Runtime>(
    p: &Persistence<RT>,
    mut expired_entries: Vec<IndexEntry>,
) -> anyhow::Result<usize> {
    let multitenant = p.multitenant;
    let instance_name = mysql_async::Value::from(&p.instance_name.raw);
    expired_entries.sort_unstable_by(|a, b| {
        Ord::cmp(
            &(a.index_id, &a.key_prefix, &a.key_sha256),
            &(b.index_id, &b.key_prefix, &b.key_sha256),
        )
    });
    // We implicitly delete all timestamps less than `ts`, so just keep the
    // highest `ts` for each index key.
    expired_entries.dedup_by(|a, b| {
        if (a.index_id, &a.key_prefix, &a.key_sha256) == (b.index_id, &b.key_prefix, &b.key_sha256)
        {
            // N.B.: returning `true` to dedup_by deletes `a`, so update `b`.
            b.ts = b.ts.max(a.ts);
            true
        } else {
            false
        }
    });
    p.lease
        .transact_read_committed(async move |tx| {
            let mut deleted_count = 0;
            for chunk in smart_chunks(&expired_entries) {
                let mut params = Vec::with_capacity(
                    chunk.len() * (sql::DELETE_INDEX_COLUMN_COUNT + (multitenant as usize)),
                );
                for index_entry in chunk.iter() {
                    Reader::<RT>::_index_delete_params(&mut params, index_entry);
                    if multitenant {
                        params.push(instance_name.clone());
                    }
                }
                deleted_count += tx
                    .exec_iter(sql::delete_index_chunk(chunk.len(), multitenant), params)
                    .await?;
            }
            Ok(deleted_count as usize)
        })
        .await
}

fn index_params(query: &mut Vec<mysql_async::Value>, update: &PersistenceIndexEntry) {
    let key: Vec<u8> = update.key.to_vec();
    let key_sha256 = Sha256::hash(&key);
    let key = SplitKey::new(key);

    let (deleted, tablet_id, doc_id) = match &update.value {
        None => (true, None, None),
        Some(doc_id) => (
            false,
            Some(internal_id_param(doc_id.table().0)),
            Some(internal_doc_id_param(*doc_id)),
        ),
    };
    query.push(internal_id_param(update.index.id().0).into());
    query.push(i64::from(update.ts).into());
    query.push(key.prefix.into());
    query.push(
        match key.suffix {
            Some(key_suffix) => Some(key_suffix),
            None => None,
        }
        .into(),
    );
    query.push(key_sha256.to_vec().into());
    query.push(deleted.into());
    query.push(tablet_id.into());
    query.push(doc_id.into());
}
