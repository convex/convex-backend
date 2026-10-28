use std::{
    str::FromStr,
    sync::Arc,
};

use json_trait::JsonForm;
use serde::Deserialize;
use serde_json::{
    json,
    Value as JsonValue,
};
use value::{
    IdentifierFieldName,
    TableNumber,
};

use crate::{
    array::ArrayShape,
    object::{
        ObjectField,
        ObjectShape,
        RecordShape,
    },
    string::StringLiteralShape,
    CountedShape,
    CountedShapeEnum,
    Shape,
    ShapeConfig,
    ShapeEnum,
    UnionBuilder,
};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ShapeJson {
    num_values: u64,
    variant: Box<ShapeEnumJson>,
}

impl<C: ShapeConfig> JsonForm for CountedShape<C> {
    type Json = ShapeJson;
}

impl<C: ShapeConfig> TryFrom<ShapeJson> for CountedShape<C> {
    type Error = anyhow::Error;

    fn try_from(shape: ShapeJson) -> Result<Self, Self::Error> {
        Ok(Shape {
            num_values: shape.num_values,
            variant: Arc::new(ShapeEnum::try_from(*shape.variant)?),
        })
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FieldPair {
    field_name: String,
    r#type: ObjectFieldJson,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ObjectFieldJson {
    r#type: ShapeJson,
    optional: bool,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum ShapeEnumJson {
    Never,
    Null,
    Int64,
    Float64,
    NegativeInf,
    PositiveInf,
    NegativeZero,
    NaN,
    NormalFloat64,
    Boolean,
    StringLiteral {
        literal: String,
    },
    #[serde(rename_all = "camelCase")]
    Id {
        table_number: u32,
    },
    FieldName,
    String,
    Bytes,
    #[serde(rename_all = "camelCase")]
    Array {
        element_type: ShapeJson,
    },
    #[serde(rename_all = "camelCase")]
    Object {
        fields: Vec<FieldPair>,
    },
    #[serde(rename_all = "camelCase")]
    Record {
        field_type: ShapeJson,
        value_type: ShapeJson,
    },
    Union {
        types: Vec<ShapeJson>,
    },
    Unknown,
}

impl<C: ShapeConfig> JsonForm for CountedShapeEnum<C> {
    type Json = ShapeEnumJson;
}
impl<C: ShapeConfig> TryFrom<ShapeEnumJson> for CountedShapeEnum<C> {
    type Error = anyhow::Error;

    fn try_from(shape_enum: ShapeEnumJson) -> Result<Self, Self::Error> {
        let result = match shape_enum {
            ShapeEnumJson::Never => ShapeEnum::Never,
            ShapeEnumJson::Null => ShapeEnum::Null,
            ShapeEnumJson::Int64 => ShapeEnum::Int64,
            ShapeEnumJson::Float64 => ShapeEnum::Float64,
            ShapeEnumJson::NegativeInf => ShapeEnum::NegativeInf,
            ShapeEnumJson::PositiveInf => ShapeEnum::PositiveInf,
            ShapeEnumJson::NegativeZero => ShapeEnum::NegativeZero,
            ShapeEnumJson::NaN => ShapeEnum::NaN,
            ShapeEnumJson::NormalFloat64 => ShapeEnum::NormalFloat64,
            ShapeEnumJson::Boolean => ShapeEnum::Boolean,
            ShapeEnumJson::StringLiteral { literal } => {
                let t = StringLiteralShape::shape_of(&literal);
                anyhow::ensure!(matches!(t, ShapeEnum::StringLiteral(_)));
                t
            },
            ShapeEnumJson::Id { table_number } => {
                ShapeEnum::Id(TableNumber::try_from(table_number)?)
            },
            ShapeEnumJson::FieldName => ShapeEnum::FieldName,
            ShapeEnumJson::String => ShapeEnum::String,
            ShapeEnumJson::Bytes => ShapeEnum::Bytes,
            ShapeEnumJson::Array { element_type } => {
                ShapeEnum::Array(ArrayShape::new(Shape::try_from(element_type)?))
            },
            ShapeEnumJson::Object { fields } => ShapeEnum::Object(ObjectShape::<C, u64>::new(
                fields
                    .into_iter()
                    .map(|f| {
                        let field_name = IdentifierFieldName::from_str(&f.field_name)?;
                        let object_field = ObjectField {
                            value_shape: Shape::try_from(f.r#type.r#type)?,
                            optional: f.r#type.optional,
                        };
                        anyhow::Ok((field_name, object_field))
                    })
                    .try_collect()?,
            )),
            ShapeEnumJson::Record {
                field_type,
                value_type,
            } => ShapeEnum::Record(RecordShape::new(
                Shape::try_from(field_type)?,
                Shape::try_from(value_type)?,
            )),
            ShapeEnumJson::Union { types } => {
                let mut builder = UnionBuilder::new();
                for t in types {
                    builder = builder.push(Shape::try_from(t)?);
                }

                let union_shape = builder.build();
                anyhow::ensure!(matches![union_shape.variant(), ShapeEnum::Union(..)]);
                union_shape.variant().clone()
            },

            ShapeEnumJson::Unknown => ShapeEnum::Unknown,
        };
        Ok(result)
    }
}

impl<C: ShapeConfig> From<&CountedShape<C>> for JsonValue {
    fn from(value: &CountedShape<C>) -> Self {
        value.to_json(true)
    }
}

impl<C: ShapeConfig> From<&CountedShapeEnum<C>> for JsonValue {
    fn from(value: &CountedShapeEnum<C>) -> Self {
        value.to_json(true)
    }
}

impl<C: ShapeConfig> CountedShape<C> {
    pub fn to_json(&self, include_pii: bool) -> JsonValue {
        let mut object = serde_json::Map::new();
        object.insert("numValues".into(), (*self.num_values()).into());
        object.insert("variant".into(), self.variant().to_json(include_pii));
        object.into()
    }
}

impl<C: ShapeConfig> CountedShapeEnum<C> {
    pub fn to_json(&self, include_pii: bool) -> JsonValue {
        match self {
            ShapeEnum::Never => json!({"kind": "Never"}),
            ShapeEnum::Null => json!({"kind": "Null"}),
            ShapeEnum::Int64 => json!({"kind": "Int64"}),
            ShapeEnum::Float64 => json!({"kind": "Float64"}),
            ShapeEnum::NegativeInf => json!({"kind": "NegativeInf"}),
            ShapeEnum::PositiveInf => json!({"kind": "PositiveInf"}),
            ShapeEnum::NegativeZero => json!({"kind": "NegativeZero"}),
            ShapeEnum::NaN => json!({"kind": "NaN"}),
            ShapeEnum::NormalFloat64 => json!({"kind": "NormalFloat64"}),
            ShapeEnum::Boolean => json!({"kind": "Boolean"}),
            ShapeEnum::StringLiteral(s) => {
                if include_pii {
                    json!({"kind": "StringLiteral", "literal": s.as_str() })
                } else {
                    json!({"kind": "StringLiteral" })
                }
            },
            ShapeEnum::Id(table_number) => {
                json!({"kind": "Id", "tableNumber": u32::from(*table_number) })
            },
            ShapeEnum::FieldName => json!({"kind": "FieldName"}),
            ShapeEnum::String => json!({"kind": "String"}),
            ShapeEnum::Bytes => json!({"kind": "Bytes"}),
            ShapeEnum::Array(array_shape) => {
                let mut object = serde_json::Map::new();
                object.insert("kind".into(), "Array".into());
                object.insert(
                    "elementType".into(),
                    array_shape.element().to_json(include_pii),
                );
                object.into()
            },
            ShapeEnum::Object(object_shape) => {
                let field_json = object_shape
                    .iter()
                    .map(|(field_name, field)| {
                        let shape_json = serde_json::Value::Object({
                            let mut object = serde_json::Map::new();
                            object.insert("type".into(), field.value_shape.to_json(include_pii));
                            object.insert("optional".into(), field.optional.into());
                            object
                        });
                        let mut object = serde_json::Map::new();
                        object.insert("fieldName".into(), field_name.as_str().into());
                        object.insert("type".into(), shape_json);
                        object.into()
                    })
                    .collect::<Vec<JsonValue>>();
                let mut object = serde_json::Map::new();
                object.insert("kind".into(), "Object".into());
                object.insert("fields".into(), field_json.into());
                object.into()
            },
            ShapeEnum::Record(record_shape) => {
                let mut object = serde_json::Map::new();
                object.insert("kind".into(), "Record".into());
                object.insert(
                    "fieldType".into(),
                    record_shape.field().to_json(include_pii),
                );
                object.insert(
                    "valueType".into(),
                    record_shape.value().to_json(include_pii),
                );
                object.into()
            },
            ShapeEnum::Union(union_shape) => {
                let mut object = serde_json::Map::new();
                object.insert("kind".into(), "Union".into());
                object.insert(
                    "types".into(),
                    union_shape
                        .iter()
                        .map(|t| t.to_json(include_pii))
                        .collect::<Vec<_>>()
                        .into(),
                );
                object.into()
            },
            ShapeEnum::Unknown => json!({"kind": "Unknown"}),
        }
    }
}
