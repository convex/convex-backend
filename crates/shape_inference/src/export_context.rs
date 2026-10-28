/// ConvexValue::export creates human-readable JSON from a ConvexValue,
/// which might lose some information like make String and Bytes ambiguous.
/// This module allows ConvexValue to round trip.
///
/// Take a document D which exports as JSON J, in a table with Shape T.
/// there is an export context ExportContext::of(D, T)
/// such that D can be recovered from J, T, and ExportContext::of(D, T).
/// Which means that a document can round-trip through the export JSON,
/// as long as the Shape of its table, and possibly some document-specific
/// context are passed along.
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    io::Cursor,
    iter,
};

use anyhow::Context;
use itertools::Itertools;
use maplit::btreeset;
use serde_json::Value as JsonValue;
use value::{
    id_v6::DeveloperDocumentId,
    ConvexValue,
    FieldName,
    IdentifierFieldName,
};

use crate::{
    Shape,
    ShapeConfig,
    ShapeCounter,
    ShapeEnum,
    StructuralShape,
};

/// Shape hint associated with a Convex value. This allows us to uniquely
/// convert the exported value back to the original Convex value.
///
/// For example, the document {foo: int64(5)} is exported as {"foo":"5"}, while
/// a hypothetical document {foo: string("5")} would have the same export.
/// There are two possibilites when encoding {foo: int64(5)}:
/// 1. the table's shape is {foo: int64} (or Record<string, int64>, etc). In
///    this case we know "5" should be decoded as int64, so we don't need any
///    special ExportContext. This is represented by ExportContext::Infer and
///    encoded as the document having no overrides.
/// 2. the table's shape is {foo: int64 | string} (or Unknown, etc). In this
///    case we don't know that "5" should be decoded as int64, so we use
///    ExportContext::Object({foo: ExportContext::Int64}) to allow decoding to
///    round-trip.
///
/// Now consider what happens when decoding the exported {"foo":"5"}.
/// 1. If the shape is {foo: int64}, we decode "5" as int64(5)
/// 2. If the ExportContext is Object({foo: Int64}), decode "5" as int64(5)
/// 3. If the shape is ambiguous *and* ExportContext is Infer, decode "5" as
///    string("5").
#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone)]
pub enum ExportContext {
    /// As described above, Infer can mean
    /// 1. the shape gives enough information to decode
    /// 2. if the shape is ambiguous, decode with the canonical format, i.e.
    ///    string.
    Infer,
    Int64,
    Float64NaN {
        // Store the f64 value in the export context when it is NaN, because the export format
        // assumes a single NaN value. This ensures that we can fully roundtrip values.
        nan_le_bytes: [u8; 8],
    },
    Float64Inf,
    Bytes,
    Array(Vec<ExportContext>),
    Object(BTreeMap<FieldName, ExportContext>),
}

impl<C: ShapeConfig, S: ShapeCounter> Shape<C, S> {
    fn union_options(&self) -> impl Iterator<Item = &Self> {
        // There are no nested unions, so this does not need to be recursive.
        iter::from_coroutine(
            #[coroutine]
            || {
                if let ShapeEnum::Union(options) = self.variant() {
                    for option in options.iter() {
                        yield option;
                    }
                } else {
                    yield self;
                }
            },
        )
    }

    fn array_element(&self) -> BTreeSet<&Self> {
        match self.variant() {
            ShapeEnum::Array(array) => btreeset!(array.element()),
            ShapeEnum::Union(options) => options
                .iter()
                .flat_map(|option| option.array_element())
                .collect(),
            ShapeEnum::Unknown => btreeset!(self),
            _ => btreeset!(),
        }
    }

    fn object_field(&self, field: &FieldName) -> BTreeSet<&Self> {
        match self.variant() {
            ShapeEnum::Object(object) => match IdentifierFieldName::try_from(field.clone()) {
                Ok(field) => object
                    .get(&field)
                    .into_iter()
                    .map(|object_field| &object_field.value_shape)
                    .collect(),
                Err(_) => btreeset!(),
            },
            ShapeEnum::Union(options) => options
                .iter()
                .flat_map(|option| option.object_field(field))
                .collect(),
            ShapeEnum::Record(record) => btreeset!(record.value()),
            ShapeEnum::Unknown => btreeset!(self),
            _ => btreeset!(),
        }
    }

}

impl ExportContext {
    pub fn is_infer(&self) -> bool {
        matches!(self, Self::Infer)
    }

    /// If the given shapes can export a String in exactly one way, return that
    /// way. If it's ambiguous, returns None.
    fn inferred_context_for_string<C: ShapeConfig, S: ShapeCounter>(
        shape_options: &BTreeSet<&Shape<C, S>>,
    ) -> Option<Self> {
        let mut possibilities = Self::possible_contexts_for_string(shape_options);
        let first = possibilities.next()?;
        match possibilities.next() {
            None => Some(first),
            Some(_) => None,
        }
    }

    /// Given a set of possible shapes, knowing that the exported value is a
    /// string, what are the possible ExportContexts?
    /// e.g. if the shapes are Union(Array(null), Int64, String), the possible
    /// export contexts are Int64 and Infer.
    fn possible_contexts_for_string<'a, C: ShapeConfig, S: ShapeCounter>(
        shape_options: &'a BTreeSet<&'a Shape<C, S>>,
    ) -> impl Iterator<Item = Self> + 'a {
        iter::from_coroutine(
            #[coroutine]
            move || {
                for shape in shape_options
                    .iter()
                    .flat_map(|option| option.union_options())
                {
                    match shape.variant() {
                        ShapeEnum::Int64 => yield ExportContext::Int64,
                        ShapeEnum::NegativeInf | ShapeEnum::PositiveInf => {
                            yield ExportContext::Float64Inf
                        },
                        ShapeEnum::NaN | ShapeEnum::Float64 => {
                            // We care about whether this function yields multiple distinct results,
                            // to see if we can use Infer. There are
                            // multiple values of NaN, and multiple
                            // kinds of floats, so we mark NaN as not
                            // inferrable by returning multiple distinct results.
                            yield ExportContext::Float64NaN {
                                nan_le_bytes: f64::NAN.to_le_bytes(),
                            };
                            yield ExportContext::Float64Inf;
                        },
                        ShapeEnum::StringLiteral(_)
                        | ShapeEnum::Id(_)
                        | ShapeEnum::FieldName
                        | ShapeEnum::String => yield ExportContext::Infer,
                        ShapeEnum::Bytes => yield ExportContext::Bytes,
                        // Unknown could have any ExportContext that can be a string.
                        ShapeEnum::Unknown => {
                            yield ExportContext::Infer;
                            yield ExportContext::Float64Inf;
                            yield ExportContext::Float64NaN {
                                nan_le_bytes: f64::NAN.to_le_bytes(),
                            };
                            yield ExportContext::Int64;
                            yield ExportContext::Bytes;
                        },
                        // coroutine cannot be recursive, so unions are already handled by
                        // union_options() above.
                        ShapeEnum::Union(_) => unreachable!(),
                        // Never exported as strings.
                        ShapeEnum::Never
                        | ShapeEnum::Null
                        | ShapeEnum::NegativeZero
                        | ShapeEnum::NormalFloat64
                        | ShapeEnum::Boolean
                        | ShapeEnum::Array(_)
                        | ShapeEnum::Object(_)
                        | ShapeEnum::Record(_) => {},
                    }
                }
            },
        )
        .dedup()
    }

    pub fn apply<C: ShapeConfig, S: ShapeCounter>(
        self,
        exported_value: JsonValue,
        shape: &Shape<C, S>,
    ) -> anyhow::Result<ConvexValue> {
        let shape_options = btreeset!(shape);
        self.apply_inner(exported_value, &shape_options)
    }

    fn apply_inner<C: ShapeConfig, S: ShapeCounter>(
        self,
        exported_value: JsonValue,
        shape: &BTreeSet<&Shape<C, S>>,
    ) -> anyhow::Result<ConvexValue> {
        match exported_value {
            JsonValue::Null => Ok(ConvexValue::Null),
            JsonValue::Bool(value) => Ok(value.into()),
            JsonValue::Number(n) => n
                .as_f64()
                .map(ConvexValue::from)
                .context("Unexpected number for i64"),
            JsonValue::String(value) => {
                let inferred_context = if self.is_infer() {
                    if let Some(inferred) = Self::inferred_context_for_string(shape) {
                        inferred
                    } else {
                        self
                    }
                } else {
                    self
                };
                match inferred_context {
                    Self::Infer => value.try_into(),
                    Self::Int64 => value
                        .parse::<i64>()
                        .map(ConvexValue::from)
                        .context("Unexpected string for i64"),
                    Self::Float64NaN { nan_le_bytes } => {
                        let nan_value = f64::from_le_bytes(nan_le_bytes);
                        if !nan_value.is_nan() {
                            anyhow::bail!("Unexpected non-NaN value in the export context");
                        }

                        if &value != "NaN" {
                            anyhow::bail!("Unexpected serialization of a NaN value");
                        }

                        Ok(nan_value.into())
                    },
                    Self::Float64Inf => match value.as_ref() {
                        "Infinity" => Ok(f64::INFINITY.into()),
                        "-Infinity" => Ok(f64::NEG_INFINITY.into()),
                        _ => anyhow::bail!("Unexpected string for f64"),
                    },
                    Self::Bytes => ConvexValue::try_from(base64::decode(value)?),
                    Self::Array(_) | Self::Object(_) => {
                        anyhow::bail!("unexpected shape hint for string")
                    },
                }
            },
            JsonValue::Array(exported_values) => match self {
                Self::Infer => {
                    let mut values = vec![];
                    let element_shape = shape
                        .iter()
                        .flat_map(|shape| shape.array_element())
                        .collect();
                    for exported_value in exported_values {
                        values.push(Self::Infer.apply_inner(exported_value, &element_shape)?);
                    }
                    ConvexValue::try_from(values)
                },
                Self::Array(shape_hints) => {
                    if exported_values.len() != shape_hints.len() {
                        anyhow::bail!("Array lengths do not match");
                    }

                    let mut values = vec![];
                    let element_shape = shape
                        .iter()
                        .flat_map(|shape| shape.array_element())
                        .collect();
                    for (exported_value, shape_hint) in exported_values.into_iter().zip(shape_hints)
                    {
                        values.push(shape_hint.apply_inner(exported_value, &element_shape)?);
                    }
                    ConvexValue::try_from(values)
                },
                Self::Bytes
                | Self::Float64NaN { .. }
                | Self::Float64Inf
                | Self::Int64
                | Self::Object(_) => anyhow::bail!("unsupported shape hint for array value"),
            },
            JsonValue::Object(exported_values) => match self {
                Self::Infer => {
                    let entries: BTreeMap<FieldName, ConvexValue> = exported_values
                        .into_iter()
                        .map(|(key, value)| {
                            let field: FieldName = key.parse()?;
                            let field_shape = shape
                                .iter()
                                .flat_map(|shape| shape.object_field(&field))
                                .collect();
                            anyhow::Ok((field, Self::Infer.apply_inner(value, &field_shape)?))
                        })
                        .try_collect()?;

                    Ok(ConvexValue::Object(entries.try_into()?))
                },
                Self::Object(mut shape_hints) => {
                    let entries: BTreeMap<FieldName, ConvexValue> = exported_values
                        .into_iter()
                        .map(|(key, value)| {
                            let field: FieldName = key.parse()?;
                            let field_shape = shape
                                .iter()
                                .flat_map(|shape| shape.object_field(&field))
                                .collect();
                            let shape_hint =
                                shape_hints.remove(&field).unwrap_or(ExportContext::Infer);
                            anyhow::Ok((field, shape_hint.apply_inner(value, &field_shape)?))
                        })
                        .try_collect()?;

                    Ok(ConvexValue::Object(entries.try_into()?))
                },
                Self::Int64
                | Self::Float64NaN { .. }
                | Self::Float64Inf
                | Self::Bytes
                | Self::Array(_) => anyhow::bail!("unsupported shape hint for object value"),
            },
        }
    }
}

impl TryFrom<JsonValue> for ExportContext {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        let export_context = match value {
            JsonValue::String(s) => match &*s {
                "infer" => Self::Infer,
                "int64" => Self::Int64,
                "float64inf" => Self::Float64Inf,
                "bytes" => Self::Bytes,
                _ => anyhow::bail!("invalid export context {s}"),
            },
            JsonValue::Array(array) => ExportContext::Array(
                array
                    .into_iter()
                    .map(ExportContext::try_from)
                    .try_collect()?,
            ),
            JsonValue::Object(mut object) => {
                if let Some(nan_value) = object.remove("$float64NaN")
                    && let JsonValue::String(nan_value_str) = nan_value
                {
                    let nan_le_bytes: [u8; 8] = base64::decode(nan_value_str.as_bytes())?
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("Float64 must be exactly eight bytes"))?;
                    ExportContext::Float64NaN { nan_le_bytes }
                } else {
                    ExportContext::Object(
                        object
                            .into_iter()
                            .map(|(k, v)| anyhow::Ok((k.parse()?, ExportContext::try_from(v)?)))
                            .try_collect()?,
                    )
                }
            },
            _ => anyhow::bail!("invalid export context {value}"),
        };
        Ok(export_context)
    }
}

/// GeneratedSchema stores sidecar data necessary to round-trip an entire table.
#[derive(Debug, Clone)]
pub enum GeneratedSchema<T: ShapeConfig> {
    /// Stores a desired shape & per-document overrides to resolve ambiguities.
    /// This format is deprecated and only exists to support importing old
    /// zip exports.
    LegacyInferred {
        inferred_shape: StructuralShape<T>,
        overrides: BTreeMap<DeveloperDocumentId, ExportContext>,
    },
    /// Indicates that values are encoded using a uniform encoding (i.e.
    /// [`value::export::ValueFormat::ConvexExportJSON`]).
    Uniform,
}

impl<T: ShapeConfig> GeneratedSchema<T> {
    // legacy code, no longer used in production
    pub fn apply(
        schema: Option<&mut Self>,
        exported_value: JsonValue,
    ) -> anyhow::Result<ConvexValue> {
        let Some(exported_object) = exported_value.as_object() else {
            let mut buf = [0u8; 100];
            let mut writer = Cursor::new(&mut buf[..]);
            let truncated = serde_json::to_writer(&mut writer, &exported_value).is_err();
            let len = writer.position() as usize;
            anyhow::bail!(
                "expected object, received {}{}",
                String::from_utf8_lossy(&buf[..len]),
                if truncated { "..." } else { "" }
            );
        };
        match schema {
            Some(GeneratedSchema::Uniform) => ConvexValue::from_clean_lossless(exported_value),
            Some(GeneratedSchema::LegacyInferred {
                inferred_shape,
                overrides,
            }) => {
                let export_context =
                    if let Some(JsonValue::String(id_str)) = exported_object.get("_id") {
                        let id = DeveloperDocumentId::decode(id_str)?;
                        overrides.remove(&id).unwrap_or(ExportContext::Infer)
                    } else {
                        ExportContext::Infer
                    };
                export_context.apply(exported_value, inferred_shape)
            },
            None => ExportContext::Infer.apply(
                exported_value,
                &StructuralShape::<T>::new(ShapeEnum::Unknown),
            ),
        }
    }
}
