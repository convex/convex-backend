use std::collections::HashMap;

use anyhow::Result;
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
use serde::{
    Deserialize,
    Serialize,
};
use strum::{
    Display,
    EnumString,
    IntoStaticStr,
};
use value::ConvexObject;

pub mod columns;
mod json;
mod names;
mod writer;
pub use writer::{
    IcebergChangeWriter,
    S3Destination,
    SourceTable,
    TableState,
};

/// How a table lays out a document. An export keeps the format it started
/// with, so its stream never pauses for a format change. New exports start in
/// `Typed`. The string form is the `convex.porter.encoding` table property and
/// the `format` field of the export's state.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Display, EnumString, IntoStaticStr, Serialize, Deserialize,
)]
#[serde(into = "&'static str", try_from = "String")]
pub enum Format {
    /// `_id`, `ts`, `deleted`, and the whole document in a JSON `payload`.
    #[strum(serialize = "convex-change-log-json-v1")]
    Json,
    /// Metadata columns, then one typed column per top-level document field.
    #[strum(serialize = "convex-change-log-typed-v1")]
    Typed,
}

impl TryFrom<String> for Format {
    type Error = strum::ParseError;

    fn try_from(encoding: String) -> Result<Self, Self::Error> {
        encoding.parse()
    }
}

/// One document revision in a table's change log. A `None` document records a
/// deletion.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub id: String,
    pub ts: u64,
    pub document: Option<ConvexObject>,
}

/// Appends `changes` to `table` as one snapshot, after adding the columns they
/// need in the typed format, and returns the table with the bytes of Parquet
/// written. Every revision is a row; readers take the latest timestamp per
/// `_id` and skip deleted rows. A retried page can repeat rows, so readers
/// deduplicate on (`_id`, timestamp).
#[fastrace::trace]
pub(crate) async fn append(
    catalog: &dyn Catalog,
    table: &Table,
    changes: &[Change],
    snapshot_properties: &HashMap<String, String>,
    format: Format,
) -> Result<(Table, u64)> {
    let (table, batch) = match format {
        Format::Json => (table.clone(), json::batch(changes)?),
        Format::Typed => {
            let table = columns::add_columns(catalog, table, changes).await?;
            let batch = columns::batch(table.metadata().current_schema(), changes)?;
            (table, batch)
        },
    };
    let table = &table;
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
    writer.write(batch).await?;
    let files = writer.close().await?;
    let bytes_written = files.iter().map(|file| file.file_size_in_bytes()).sum();
    let tx = Transaction::new(table);
    // File names are UUID-prefixed, so the duplicate check, which reads every
    // manifest, can never fire.
    let tx = tx
        .fast_append()
        .with_check_duplicate(false)
        .set_snapshot_properties(snapshot_properties.clone())
        .add_data_files(files)
        .apply(tx)?;
    Ok((tx.commit(catalog).await?, bytes_written))
}
