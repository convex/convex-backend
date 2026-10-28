use std::collections::BTreeMap;

use anyhow::Context;
use fivetran_common::fivetran_sdk::value_type::Inner as FivetranValue;
use prost_types::Timestamp;
use serde_json::Value as JsonValue;
use value::{
    export::ValueFormat,
    ConvexValue,
};

fn timestamp_from_ms(ms_since_unix_epoch: f64) -> Timestamp {
    let ms_in_s = 1000.0;
    let ns_in_ms = 1_000_000.0;

    Timestamp {
        seconds: (ms_since_unix_epoch / ms_in_s) as i64,
        nanos: ((ms_since_unix_epoch % ms_in_s) * ns_in_ms) as i32,
    }
}

fn to_fivetran_value(value: ConvexValue) -> FivetranValue {
    match value {
        ConvexValue::Null => FivetranValue::Null(true),
        ConvexValue::Int64(value) => FivetranValue::Long(value),
        ConvexValue::Float64(value) => FivetranValue::Double(value),
        ConvexValue::Boolean(value) => FivetranValue::Bool(value),
        ConvexValue::String(value) => FivetranValue::String(value.into()),
        ConvexValue::Bytes(value) => FivetranValue::Binary(value.into()),
        value @ (ConvexValue::Array(_) | ConvexValue::Object(_)) => {
            FivetranValue::Json(value.export(ValueFormat::ConvexCleanJSON).to_string())
        },
    }
}

/// Converts a Convex document field to a Fivetran field.
/// Returns None if the field is skipped in Fivetran.
fn to_fivetran_field(
    (field_name, field_value): (String, JsonValue),
) -> anyhow::Result<Option<(String, FivetranValue)>> {
    let result =
        // Skip most system fields
        if field_name.starts_with('_') && field_name != "_id" && field_name != "_creationTime" {
            None
        } else {
            let fivetran_value: FivetranValue = if field_name == "_creationTime" {
                let JsonValue::Number(milliseconds) = field_value else {
                    anyhow::bail!("Unexpected _creationTime value: {:?}", field_value);
                };
                let milliseconds = milliseconds.as_f64().context(
                    "Unexpected arbitrary-precision floating-point number found in _creationTime"
                )?;
                FivetranValue::UtcDatetime(timestamp_from_ms(milliseconds))
            } else {
                // The data sync API emits `ValueFormat::ConvexExportJSON`, of
                // which this is the inverse.
                let convex_value = ConvexValue::from_clean_lossless(field_value)
                    .context("Invalid Convex value")?;
                to_fivetran_value(convex_value)
            };

            Some((field_name, fivetran_value))
        };
    anyhow::Result::Ok(result)
}

pub fn to_fivetran_row(
    convex_document: BTreeMap<String, JsonValue>,
) -> anyhow::Result<BTreeMap<String, FivetranValue>> {
    let possible_object_entries: Vec<Option<(String, FivetranValue)>> = convex_document
        .into_iter()
        .map(to_fivetran_field)
        .try_collect()?;
    Ok(possible_object_entries.into_iter().flatten().collect())
}
