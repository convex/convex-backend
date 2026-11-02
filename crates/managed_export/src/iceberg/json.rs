//! The format before typed columns: `_id`, `ts`, `deleted`, and the whole
//! document in one JSON `payload` column. See [`super::Format`].

use std::sync::Arc;

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
        NestedField,
        PrimitiveType,
        Schema,
        Type,
    },
};
use value::export::ValueFormat;

use super::Change;

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

pub(crate) fn batch(changes: &[Change]) -> Result<RecordBatch> {
    let payloads = changes
        .iter()
        .map(|c| {
            c.document
                .clone()
                .map(|document| {
                    serde_json::to_string(&document.export(ValueFormat::ConvexEncodedJSON))
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RecordBatch::try_new(
        Arc::new(schema_to_arrow_schema(&change_log_schema()?)?),
        vec![
            Arc::new(StringArray::from_iter_values(changes.iter().map(|c| &c.id))) as ArrayRef,
            Arc::new(Int64Array::from(
                changes
                    .iter()
                    .map(|c| i64::try_from(c.ts))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Arc::new(BooleanArray::from_iter(
                changes.iter().map(|c| Some(c.document.is_none())),
            )),
            Arc::new(StringArray::from_iter(
                payloads.iter().map(Option::as_deref),
            )),
        ],
    )?)
}
