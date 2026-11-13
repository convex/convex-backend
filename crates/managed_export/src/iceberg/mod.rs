use std::collections::HashMap;

use anyhow::Result;
use common::knobs::S3_EXPORT_RETAINED_SNAPSHOTS;
use fastrace::{
    future::FutureExt as _,
    local::LocalSpan,
    Span,
};
use iceberg::{
    spec::DataFileFormat,
    table::Table,
    transaction::{
        ApplyTransactionAction,
        Transaction,
    },
    writer::{
        base_writer::data_file_writer::DataFileWriterBuilder,
        file_writer::{
            location_generator::{
                DefaultFileNameGenerator,
                DefaultLocationGenerator,
            },
            rolling_writer::RollingFileWriterBuilder,
            ParquetWriterBuilder,
        },
        IcebergWriter,
        IcebergWriterBuilder,
    },
    Catalog,
};
use parquet_iceberg::file::properties::WriterProperties;
use value::ConvexObject;

pub mod columns;
mod names;
mod writer;
pub use writer::{
    IcebergChangeWriter,
    S3Destination,
    SourceTable,
    TableState,
};

/// One document revision in a table's change log. A `None` document records a
/// deletion.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub id: String,
    pub ts: u64,
    pub document: Option<ConvexObject>,
}

/// Appends `changes` to `table` as one snapshot, adding columns as needed.
/// Returns the table and the Parquet bytes written. One row per revision;
/// readers take the latest `_ts` per `_id` and skip deletes. A retried page
/// can repeat rows, so readers dedupe on (`_id`, `_ts`).
#[fastrace::trace]
pub(crate) async fn append(
    catalog: &dyn Catalog,
    table: &Table,
    changes: &[Change],
    snapshot_properties: &HashMap<String, String>,
) -> Result<(Table, u64)> {
    let table = &columns::add_columns(catalog, table, changes).await?;
    let batch = {
        let _span = LocalSpan::enter_with_local_parent("append::encode");
        columns::batch(table.metadata().current_schema(), changes)?
    };
    let schema = table.metadata().current_schema().clone();
    let mut writer =
        DataFileWriterBuilder::new(RollingFileWriterBuilder::new_with_default_file_size(
            ParquetWriterBuilder::new(WriterProperties::default(), schema),
            table.file_io().clone(),
            DefaultLocationGenerator::new(table.metadata())?,
            // The generator numbers files from zero per writer, so the prefix keeps
            // file names unique across appends.
            DefaultFileNameGenerator::new(
                uuid::Uuid::new_v4().to_string(),
                None,
                DataFileFormat::Parquet,
            ),
        ))
        .build(None)
        .await?;
    let files = async {
        writer.write(batch).await?;
        writer.close().await
    }
    .in_span(Span::enter_with_local_parent("append::write_data_file"))
    .await?;
    let bytes_written = files.iter().map(|file| file.file_size_in_bytes()).sum();
    let tx = Transaction::new(table);
    // Expire before appending, so the expiry's ref check sees the table as
    // loaded. The far-future cutoff leaves the count as the only limit. Expiry
    // rewrites metadata only; old manifest lists stay for readers on them.
    let tx = tx
        .expire_snapshots()
        .expire_older_than_ms(i64::MAX)
        .retain_last(*S3_EXPORT_RETAINED_SNAPSHOTS - 1)
        .apply(tx)?;
    // File names are UUID-prefixed, so the duplicate check, which reads every
    // manifest, can never fire.
    let tx = tx
        .fast_append()
        .with_check_duplicate(false)
        .set_snapshot_properties(snapshot_properties.clone())
        .add_data_files(files)
        .apply(tx)?;
    let table = tx
        .commit(catalog)
        .in_span(Span::enter_with_local_parent("append::commit"))
        .await?;
    Ok((table, bytes_written))
}
