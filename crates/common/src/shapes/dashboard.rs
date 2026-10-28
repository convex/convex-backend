use serde_json::{
    json,
    Value as JsonValue,
};
use value::NamespacedTableMapping;

use super::reduced::ReducedShape;
use crate::virtual_system_mapping::{
    all_tables_number_to_name,
    VirtualSystemMapping,
};

pub fn dashboard_shape_json(
    shape: &ReducedShape,
    mapping: &NamespacedTableMapping,
    virtual_mapping: &VirtualSystemMapping,
) -> anyhow::Result<JsonValue> {
    let result = match shape {
        ReducedShape::Unknown => json!({"type": "Unknown"}),
        ReducedShape::Never => json!({"type": "Never"}),
        ReducedShape::Id(table_number) => {
            match all_tables_number_to_name(mapping, virtual_mapping)(*table_number) {
                Ok(table_name) => {
                    json!({
                        "type": "Id",
                        "tableName": table_name[..],
                    })
                },
                Err(_) => json!({ "type": "String" }),
            }
        },
        ReducedShape::Null => json!({"type": "Null"}),
        ReducedShape::Int64 => {
            json!({"type": "Int64" })
        },
        ReducedShape::Float64(range) => {
            json!({"type": "Float64", "float64Range": JsonValue::from(range)})
        },
        ReducedShape::Boolean => json!({"type": "Boolean"}),
        ReducedShape::String => json!({"type": "String"}),
        ReducedShape::Bytes => json!({"type": "Bytes"}),
        ReducedShape::Object(fields) => {
            let field_json = fields
                .iter()
                .map(|(field_name, field_shape)| {
                    let result = json!({
                        "fieldName": String::from(field_name.clone()),
                        "optional": field_shape.optional,
                        "shape": dashboard_shape_json(&field_shape.shape, mapping, virtual_mapping)?,
                    });
                    anyhow::Ok(result)
                })
                .try_collect::<Vec<_>>()?;
            json!({"type": "Object", "fields": field_json})
        },
        ReducedShape::Record {
            key_shape,
            value_shape,
        } => {
            json!({
                "type": "Record",
                "keyShape": dashboard_shape_json(key_shape.as_ref(), mapping, virtual_mapping)?,
                "valueShape": json!({
                    "optional": value_shape.as_ref().optional,
                    "shape": dashboard_shape_json(&value_shape.as_ref().shape, mapping, virtual_mapping)?,
                })
            })
        },
        ReducedShape::Array(shape) => {
            json!({"type": "Array", "shape": dashboard_shape_json(shape.as_ref(), mapping, virtual_mapping)?})
        },
        ReducedShape::Union(shapes) => {
            json!({
                "type": "Union",
                "shapes": shapes
                    .iter()
                    .map(|s| dashboard_shape_json(s, mapping, virtual_mapping))
                    .try_collect::<Vec<_>>()?,
            })
        },
    };
    Ok(result)
}
