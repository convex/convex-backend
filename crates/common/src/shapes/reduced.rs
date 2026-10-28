use std::collections::{
    BTreeMap,
    BTreeSet,
};

use serde::Deserialize;
use serde_json::{
    json,
    Value as JsonValue,
};
use shape_inference::{
    CountedShape,
    ShapeConfig,
    ShapeEnum,
    UnionBuilder,
    UnionShape,
};
use value::{
    id_v6::DeveloperDocumentId,
    FieldName,
    IdentifierFieldName,
    TableNumber,
};

/// ReducedShapes are like Shapes but less precise.
/// In particular, objects have optional fields and there are no unions at the
/// top object level.
/// So a Shape {a: string} | {b: string} will be reduced to the ReducedShape
/// {a?: string, b?: string}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReducedShape {
    /// See TypeEnum docstrings.
    Unknown,
    Never,
    Id(TableNumber),
    Null,
    Int64,
    Float64(ReducedFloatRange),
    Boolean,
    String,
    Bytes,
    Object(BTreeMap<FieldName, ReducedField>),
    Array(Box<ReducedShape>),
    Record {
        key_shape: Box<ReducedShape>,
        value_shape: Box<ReducedField>,
    },
    Union(BTreeSet<ReducedShape>),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReducedField {
    pub optional: bool,
    pub shape: ReducedShape,
}

impl ReducedShape {
    pub fn from_type<C: ShapeConfig>(
        value: &CountedShape<C>,
        table_exists: &impl Fn(TableNumber) -> bool,
    ) -> Self {
        match value.variant() {
            ShapeEnum::Never => ReducedShape::Never,
            ShapeEnum::Null => ReducedShape::Null,
            ShapeEnum::Int64 => ReducedShape::Int64,
            ShapeEnum::Float64 => ReducedShape::Float64(ReducedFloatRange {
                has_special_values: true,
            }),
            ShapeEnum::NegativeInf => ReducedShape::Float64(ReducedFloatRange {
                has_special_values: true,
            }),
            ShapeEnum::PositiveInf => ReducedShape::Float64(ReducedFloatRange {
                has_special_values: true,
            }),
            ShapeEnum::NegativeZero => ReducedShape::Float64(ReducedFloatRange {
                has_special_values: true,
            }),
            ShapeEnum::NaN => ReducedShape::Float64(ReducedFloatRange {
                has_special_values: true,
            }),
            ShapeEnum::NormalFloat64 => ReducedShape::Float64(ReducedFloatRange {
                has_special_values: false,
            }),
            ShapeEnum::Boolean => ReducedShape::Boolean,
            ShapeEnum::StringLiteral(s) => {
                if let Ok(id) = DeveloperDocumentId::decode(s)
                    && table_exists(id.table())
                {
                    ReducedShape::Id(id.table())
                } else {
                    ReducedShape::String
                }
            },
            ShapeEnum::Id(table_number) => {
                if table_exists(*table_number) {
                    ReducedShape::Id(*table_number)
                } else {
                    ReducedShape::String
                }
            },
            ShapeEnum::FieldName => ReducedShape::String,
            ShapeEnum::String => ReducedShape::String,
            ShapeEnum::Bytes => ReducedShape::Bytes,
            ShapeEnum::Array(array_type) => ReducedShape::Array(Box::new(ReducedShape::from_type(
                array_type.element(),
                table_exists,
            ))),
            ShapeEnum::Object(object_type) => {
                let reduced_fields = object_type
                    .fields()
                    .iter()
                    .map(|(field_name, shape)| {
                        (
                            FieldName::from(field_name.clone()),
                            ReducedField {
                                optional: shape.optional,
                                shape: ReducedShape::from_type(&shape.value_shape, table_exists),
                            },
                        )
                    })
                    .collect();
                ReducedShape::Object(reduced_fields)
            },
            ShapeEnum::Record(record_type) => {
                let key_shape = ReducedShape::from_type(record_type.field(), table_exists);
                let value_shape = ReducedShape::from_type(record_type.value(), table_exists);
                let value_optional = record_type
                    .field()
                    .variant()
                    .is_string_subtype_with_string_literal();

                ReducedShape::Record {
                    key_shape: Box::new(key_shape),
                    value_shape: Box::new(ReducedField {
                        optional: value_optional,
                        shape: value_shape,
                    }),
                }
            },
            ShapeEnum::Union(union_type) => Self::reduce_union_type(union_type, table_exists),
            ShapeEnum::Unknown => ReducedShape::Unknown,
        }
    }

    fn reduce_union_type<C: ShapeConfig>(
        union_type: &UnionShape<C, u64>,
        table_exists: &impl Fn(TableNumber) -> bool,
    ) -> Self {
        let mut object_fields: BTreeMap<IdentifierFieldName, UnionBuilder<C>> = BTreeMap::new();
        let mut object_optional_fields = BTreeSet::new();
        let mut object_count = 0;

        let mut float_range: Option<ReducedFloatRange> = None;

        let mut reduced = BTreeSet::new();

        for t in union_type.iter() {
            if let ShapeEnum::Object(object_type) = t.variant() {
                let fields = object_type.fields();
                for (field_name, field_type) in fields {
                    if field_type.optional {
                        object_optional_fields.insert(field_name.clone());
                    }
                    let existing_shapes = match object_fields.remove(field_name) {
                        None => {
                            // New field that didn't exist in previous object shapes => make the
                            // field optional.
                            if object_count > 0 {
                                object_optional_fields.insert(field_name.clone());
                            }
                            UnionBuilder::new()
                        },
                        Some(existing_shapes) => existing_shapes,
                    };
                    let builder = existing_shapes.push(field_type.value_shape.clone());
                    object_fields.insert(field_name.clone(), builder);
                }
                for field_name in object_fields.keys() {
                    if fields.contains_key(field_name) {
                        continue;
                    }
                    object_optional_fields.insert(field_name.clone());
                }
                object_count += t.num_values();
            } else {
                let reduced_shape = Self::from_type(t, table_exists);
                if let ReducedShape::Float64(f) = reduced_shape {
                    float_range = match float_range {
                        Some(range) => Some(ReducedFloatRange {
                            has_special_values: range.has_special_values || f.has_special_values,
                        }),
                        None => Some(f),
                    }
                } else {
                    reduced.insert(Self::from_type(t, table_exists));
                }
            }
        }

        if object_count > 0 {
            let fields = object_fields
                .into_iter()
                .map(|(k, v)| {
                    let optional = object_optional_fields.contains(&k);
                    (
                        k.into(),
                        ReducedField {
                            optional,
                            shape: Self::from_type(&v.build(), table_exists),
                        },
                    )
                })
                .collect();
            let object_shape = ReducedShape::Object(fields);
            assert!(reduced.insert(object_shape));
        }
        if let Some(range) = float_range {
            assert!(reduced.insert(ReducedShape::Float64(range)));
        }
        if reduced.is_empty() {
            ReducedShape::Never
        } else if reduced.len() == 1 {
            reduced.pop_first().unwrap()
        } else {
            ReducedShape::Union(reduced)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReducedFloatRange {
    // -inf, inf, -0.0, NaN
    pub has_special_values: bool,
}

impl ReducedFloatRange {
}

impl TryFrom<JsonValue> for ReducedFloatRange {
    type Error = anyhow::Error;

    fn try_from(json_value: JsonValue) -> anyhow::Result<Self> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ReducedFloatRangeJson {
            has_special_values: bool,
        }
        let range_json: ReducedFloatRangeJson = serde_json::from_value(json_value)?;
        Ok(ReducedFloatRange {
            has_special_values: range_json.has_special_values,
        })
    }
}

impl From<&ReducedFloatRange> for JsonValue {
    fn from(range: &ReducedFloatRange) -> Self {
        json!({
            "hasSpecialValues": range.has_special_values,
        })
    }
}
