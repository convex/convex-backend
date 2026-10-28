use std::{
    collections::HashMap,
    sync::Arc,
};

use anyhow::Result;
use arrow_iceberg::{
    ArrayRef,
    BooleanArray,
    Int64Array,
    RecordBatch,
    StringArray,
};
use iceberg::{
    arrow::schema_to_arrow_schema,
    spec::{
        DataFileFormat,
        NestedField,
        PrimitiveType,
        Schema,
        Type,
    },
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

#[path = "iceberg_writer.rs"]
mod writer;
pub use writer::{
    IcebergChangeWriter,
    S3Destination,
    SourceTable,
};

/// One document revision in a table's change log. A `None` payload records a
/// deletion.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub id: String,
    pub ts: u64,
    pub payload: Option<String>,
}

/// Every revision is a row; readers take the latest `ts` per `_id` and skip
/// deleted rows.
pub(crate) fn change_log_schema() -> Result<Schema> {
    Ok(Schema::builder()
        .with_fields(vec![
            NestedField::required(1, "_id", Type::Primitive(PrimitiveType::String)).into(),
            NestedField::required(2, "ts", Type::Primitive(PrimitiveType::Long)).into(),
            NestedField::required(3, "deleted", Type::Primitive(PrimitiveType::Boolean)).into(),
            NestedField::optional(4, "payload", Type::Primitive(PrimitiveType::String)).into(),
        ])
        .build()?)
}

/// Appends `changes` to `table` as one snapshot and returns the table with the
/// bytes of Parquet written. On a conflict the commit reloads the table and
/// reapplies the append, so concurrent writers both land and a retried page
/// can repeat rows; readers deduplicate on (`_id`, `ts`).
#[fastrace::trace]
pub(crate) async fn append(
    catalog: &dyn Catalog,
    table: &Table,
    changes: &[Change],
    snapshot_properties: &HashMap<String, String>,
) -> Result<(Table, u64)> {
    let schema = table.metadata().current_schema().clone();
    let batch = RecordBatch::try_new(
        Arc::new(schema_to_arrow_schema(&schema)?),
        vec![
            Arc::new(StringArray::from_iter_values(changes.iter().map(|c| &c.id))) as ArrayRef,
            Arc::new(Int64Array::from(
                changes
                    .iter()
                    .map(|c| i64::try_from(c.ts))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Arc::new(BooleanArray::from_iter(
                changes.iter().map(|c| Some(c.payload.is_none())),
            )),
            Arc::new(StringArray::from_iter(
                changes.iter().map(|c| c.payload.as_deref()),
            )),
        ],
    )?;
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
