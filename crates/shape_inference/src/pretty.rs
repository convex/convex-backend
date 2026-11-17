/// Pretty-print a shape, and parse the pretty-printed string back into a shape,
/// losing num_values but otherwise recovering the structure.
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    fmt,
    str::FromStr,
};

use anyhow::Context as _;
use value::{
    TableNumber,
    MAX_NESTING,
};

use crate::{
    array::ArrayShape,
    object::{
        ObjectField,
        ObjectShape,
        RecordShape,
    },
    string::StringLiteralShape,
    Shape,
    ShapeConfig,
    ShapeCounter,
    ShapeEnum,
    StructuralShape,
    StructuralShapeEnum,
    UnionShape,
};

impl<C: ShapeConfig, S: ShapeCounter> fmt::Display for Shape<C, S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.variant, f)
    }
}

impl<C: ShapeConfig, S: ShapeCounter> fmt::Display for ShapeEnum<C, S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ShapeEnum::Never => write!(f, "never"),
            ShapeEnum::Null => write!(f, "null"),
            ShapeEnum::Int64 => write!(f, "int64"),
            ShapeEnum::Float64 => write!(f, "float64"),
            ShapeEnum::NegativeInf => write!(f, "-inf"),
            ShapeEnum::PositiveInf => write!(f, "inf"),
            ShapeEnum::NegativeZero => write!(f, "-0"),
            ShapeEnum::NaN => write!(f, "NaN"),
            ShapeEnum::NormalFloat64 => write!(f, "normalfloat64"),
            ShapeEnum::Boolean => write!(f, "boolean"),
            ShapeEnum::StringLiteral(s) => write!(f, "{:?}", &s[..]),
            ShapeEnum::Id(table) => write!(f, "id<{table}>"),
            ShapeEnum::FieldName => write!(f, "field_name"),
            ShapeEnum::String => write!(f, "string"),
            ShapeEnum::Bytes => write!(f, "bytes"),
            ShapeEnum::Array(array) => write!(f, "array<{}>", array.element()),

            ShapeEnum::Object(object) => {
                let mut first = true;
                write!(f, "{{")?;
                for (field_name, field) in object.iter() {
                    if first {
                        first = false;
                    } else {
                        write!(f, ", ")?;
                    }
                    let optional_suffix = if field.optional { "?" } else { "" };
                    write!(
                        f,
                        "{:?}{}: {}",
                        &field_name[..],
                        optional_suffix,
                        field.value_shape
                    )?;
                }
                write!(f, "}}")
            },
            ShapeEnum::Record(record) => {
                write!(f, "record<{}, {}>", record.field(), record.value())
            },
            ShapeEnum::Union(union) => {
                let mut first = true;
                for variant in union.iter() {
                    if first {
                        first = false
                    } else {
                        write!(f, " | ")?;
                    }
                    write!(f, "{variant}")?;
                }
                Ok(())
            },
            ShapeEnum::Unknown => write!(f, "unknown"),
        }
    }
}

pub(crate) fn format_shapes<'a, C: ShapeConfig, S: ShapeCounter + 'static>(
    shapes: impl Iterator<Item = &'a Shape<C, S>>,
) -> String {
    let mut first = true;
    let mut out = String::new();
    for t in shapes {
        if first {
            first = false;
        } else {
            out.push_str(", ");
        }
        out.push_str(&format!("{t}"));
    }
    if first {
        out.push_str("<empty>");
    }
    out
}

impl<T: ShapeConfig> FromStr for StructuralShape<T> {
    type Err = anyhow::Error;

    /// Inverse of Display.
    fn from_str(s: &str) -> anyhow::Result<Self> {
        let (t, suffix) = Self::parse_prefix_union(s, MAX_NESTING)?;
        if !suffix.is_empty() {
            anyhow::bail!("unexpected token at '{suffix}'");
        }
        Ok(t)
    }
}

impl<C: ShapeConfig> StructuralShape<C> {
    /// Helper for from_str
    fn parse_prefix_union(s: &str, max_nesting: usize) -> anyhow::Result<(Self, &str)> {
        let mut union_builder = BTreeSet::new();
        let mut suffix = s;
        loop {
            let (part, end) = Self::parse_prefix(suffix, max_nesting)?;
            union_builder.insert(part);
            if let Some(stripped) = end.strip_prefix(" | ") {
                suffix = stripped;
            } else {
                suffix = end;
                break;
            }
        }
        let union = if union_builder.len() == 1 {
            union_builder.pop_first().expect("should have 1")
        } else {
            Self::new(StructuralShapeEnum::Union(UnionShape::from_parts(
                union_builder,
            )))
        };
        Ok((union, suffix))
    }

    fn parse_value_with_terminator(
        suffix: &mut &str,
        terminator: &str,
        max_nesting: usize,
    ) -> anyhow::Result<Self> {
        let (value, end) = Self::parse_prefix_union(suffix, max_nesting)?;
        if !end.starts_with(terminator) {
            anyhow::bail!("unexpected token at '{end}'. expected '{terminator}'");
        }
        *suffix = &end[terminator.len()..];
        Ok(value)
    }

    fn parse_identifier_with_terminator<'a>(
        suffix: &mut &'a str,
        terminator: &str,
    ) -> anyhow::Result<&'a str> {
        let Some(index) = suffix.find(terminator) else {
            anyhow::bail!("unexpected token at '{suffix}'. expected '{terminator}'");
        };
        let identifier = &suffix[..index];
        *suffix = &suffix[index + terminator.len()..];
        Ok(identifier)
    }

    /// Helper for from_str_structural
    fn parse_prefix(s: &str, max_nesting: usize) -> anyhow::Result<(Self, &str)> {
        let units = [
            ("never", ShapeEnum::Never),
            ("null", ShapeEnum::Null),
            ("int64", ShapeEnum::Int64),
            ("float64", ShapeEnum::Float64),
            ("-inf", ShapeEnum::NegativeInf),
            ("inf", ShapeEnum::PositiveInf),
            ("-0", ShapeEnum::NegativeZero),
            ("NaN", ShapeEnum::NaN),
            ("normalfloat64", ShapeEnum::NormalFloat64),
            ("boolean", ShapeEnum::Boolean),
            ("field_name", ShapeEnum::FieldName),
            ("string", ShapeEnum::String),
            ("bytes", ShapeEnum::Bytes),
            ("unknown", ShapeEnum::Unknown),
        ];
        for (unit_str, unit_enum) in units {
            if let Some(suffix) = s.strip_prefix(unit_str) {
                return Ok((Self::new(unit_enum), suffix));
            }
        }
        if let Some(mut suffix) = s.strip_prefix('{') {
            let max_nesting = max_nesting.checked_sub(1).context("shape too nested")?;
            let mut fields = BTreeMap::new();
            while !suffix.starts_with('}') {
                let field = Self::parse_identifier_with_terminator(&mut suffix, ": ")?;
                let (field, optional) = if let Some(field) = field.strip_suffix('?') {
                    (field, true)
                } else {
                    (field, false)
                };
                anyhow::ensure!(
                    field.starts_with('\"') && field.ends_with('\"'),
                    "cannot parse object key '{field}'"
                );
                let field_str = &field[1..field.len() - 1];
                let value_shape = Self::parse_value_with_terminator(&mut suffix, "", max_nesting)?;
                let object_field = ObjectField {
                    value_shape,
                    optional,
                };
                fields.insert(field_str.parse()?, object_field);

                if suffix.starts_with(", ") {
                    suffix = &suffix[2..];
                } else if !suffix.starts_with('}') {
                    anyhow::bail!("unexpected token at '{suffix}'");
                }
            }
            suffix = &suffix[1..];
            let object = Self::new(ShapeEnum::Object(ObjectShape::<C, ()>::new(fields)));
            Ok((object, suffix))
        } else if let Some(mut suffix) = s.strip_prefix("id<") {
            let number = Self::parse_identifier_with_terminator(&mut suffix, ">")?;
            let table_number = TableNumber::try_from(number.parse::<u32>()?)?;
            Ok((Self::new(ShapeEnum::Id(table_number)), suffix))
        } else if let Some(mut suffix) = s.strip_prefix("array<") {
            let value = Self::parse_value_with_terminator(
                &mut suffix,
                ">",
                max_nesting.checked_sub(1).context("shape too nested")?,
            )?;
            Ok((Self::new(ShapeEnum::Array(ArrayShape::new(value))), suffix))
        } else if let Some(mut suffix) = s.strip_prefix("record<") {
            let max_nesting = max_nesting.checked_sub(1).context("shape too nested")?;
            let key = Self::parse_value_with_terminator(&mut suffix, ", ", max_nesting)?;
            let value = Self::parse_value_with_terminator(&mut suffix, ">", max_nesting)?;
            Ok((
                Self::new(ShapeEnum::Record(RecordShape::new(key, value))),
                suffix,
            ))
        } else if let Some(mut suffix) = s.strip_prefix('\"') {
            // Note we don't have to worry about escapes because valid literals
            // have restricted valid characters.
            let literal = Self::parse_identifier_with_terminator(&mut suffix, "\"")?;
            Ok((Self::new(StringLiteralShape::shape_of(literal)), suffix))
        } else {
            anyhow::bail!("unexpected token at '{s}'");
        }
    }
}
