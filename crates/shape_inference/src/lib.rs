#![feature(coroutines)]
#![feature(str_as_str)]
#![feature(iter_from_coroutine)]
#![feature(iterator_try_collect)]
#![feature(impl_trait_in_assoc_type)]
#![feature(try_blocks)]
#![feature(try_blocks_heterogeneous)]
// TODO
// [ ] Add IdAnyTable?
// [ ] add tuple types
// [ ] benchmark different configuration parameters + find hotspots

mod array;
mod config;
mod float64;
mod object;
mod string;
mod union;

mod contains;
mod json;
mod overlaps;
mod subtype;
mod supertype;
pub mod export_context;
pub mod pretty;
use std::{
    collections::BTreeMap,
    sync::Arc,
};

use float64::Float64Shape;
use object::ObjectField;
pub use union::{
    UnionBuilder,
    UnionShape,
};
use value::{
    id_v6::DeveloperDocumentId,
    ConvexObject,
    ConvexValue,
    FieldName,
    IdentifierFieldName,
    TableNumber,
};

pub use self::config::{
    ProdConfig,
    ShapeConfig,
};
use self::{
    array::ArrayShape,
    object::{
        ObjectShape,
        RecordShape,
    },
    string::StringLiteralShape,
};

/// This struct defines a type system (we call them "shapes" to distinguish from
/// existing programming language types that we codegen) that can be
/// automatically inferred from a set of `Value`s in the database. These shapes
/// allow us to infer a schema that we can then suggest to the user, allowing
/// the user to smoothly progress from using Convex as a schemaless database to
/// a more rigorous, schema-driven one.
///
/// We define an empty shape `Shape::empty()` for an empty table, and then the
/// database layer can update the shape as values are inserted and removed with
/// `Shape::insert` and `Shape::remove`. These two methods are the only public
/// interface for updating a shape.
///
/// Types in programming languages can be thought of as specifications of *sets
/// of values*. That is, the shape `ShapeEnum::Int64` represents the set of all
/// `Value::Int64`s, and a value is in a shape if it's a member of this set.
/// Our shapes have an additional twist in that they represent *multisets*,
/// where a single value can be repeated. This is necessary since a table may
/// contain multiple copies of the same value.
///
/// This interpretation of a type as multiset also leads to a natural
/// definition of *subtyping*, where one shape is a subtype of
/// another if it's a subset of the other (ignoring the multiset counters). See
/// the algorithms in `subtype.rs` for more detail.
///
/// For further reading, see this survey [1] on set-theoretic types and
/// "semantic subtyping". Languages like XDuce [2] and subsequently CDuce [3]
/// have explored these ideas in more depth, extending the type system to
/// include function types, etc. More recently, Elixir is adding a (sound) type
/// system very similar to TypeScript's that's also based on set-theoretic types
/// [4].
///
/// [1] https://www.irif.fr/~gc/papers/set-theoretic-types-2022.pdf
/// [2] https://www.cis.upenn.edu/~bcpierce/papers/xduce-toit.pdf
/// [3] https://www.cduce.org/papers.html
/// [4] https://elixir-lang.org/blog/2023/06/22/type-system-updates-research-dev/
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Shape<C: ShapeConfig, S: ShapeCounter> {
    variant: Arc<ShapeEnum<C, S>>,
    num_values: S,
}

pub trait ShapeCounter: Clone + Copy + std::fmt::Debug + Eq + Ord {}

impl ShapeCounter for u64 {}
impl ShapeCounter for () {}

pub type CountedShape<C> = Shape<C, u64>;
pub type StructuralShape<C> = Shape<C, ()>;
pub type CountedShapeEnum<C> = ShapeEnum<C, u64>;
pub type StructuralShapeEnum<C> = ShapeEnum<C, ()>;

/// We get to define our shapes in a way that's convenient for inference and
/// useful for schema generation: Not all sets of `Value`s are valid shapes.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ShapeEnum<C: ShapeConfig, S: ShapeCounter> {
    /// The empty shape, corresponding to the empty set of values. This
    /// corresponds to `never` in TypeScript (and generally "bottom" in type
    /// theory).
    Never,

    /// The singleton set of just `Value::Null`.
    Null,

    /// The set of all `Value::Int64`s.
    Int64,

    /// Special `Value::Float64`s
    NegativeInf,
    PositiveInf,
    NegativeZero,
    NaN,
    /// Non-special `Value::Float64`s (e.g. `{3, 1.6}`)
    NormalFloat64,

    /// The set of all `Value::Float64`s.
    Float64,

    /// The set of all `Value::Boolean`s (i.e. `{true, false}`).
    Boolean,

    /// The singleton set of just a single string literal (e.g. `{"hi!"}`). Not
    /// all `Value::String`s may be valid string literal shapes, and this
    /// behavior is configured by [`ShapeConfig::is_valid_string_literal`].
    StringLiteral(StringLiteralShape<C>),

    /// The set of all strings that are a valid `Id` for a particular table.
    Id(TableNumber),

    // The set of all valid `FieldName`s.
    FieldName,

    /// The set of all `Value::String`s.
    String,

    /// The set of all `Value::Bytes`s.
    Bytes,

    /// The set of all `Value::Array`s with elements within a particular shape.
    /// Note that there are two multisets involved here: This shape
    /// represents a multiset of arrays, where the inner element shape
    /// tracks the multiset of all of the arrays' elements. For example,
    /// creating a shape with two arrays `[1, 2, 3]` and `["four", "five"]`
    /// will have an outer shape `array<..> with two elements and an inner
    /// shape `int64 | string` with five elements.
    Array(ArrayShape<C, S>),

    /// The set of all objects with a set of statically known fields. Not all
    /// objects can become a valid object shape: This behavior is configured
    /// by [`ShapeConfig::MAX_OBJECT_FIELDS`] and
    /// [`ShapeConfig::is_valid_object_field`]. Note that we do not have
    /// structural subtyping in our type system: creating a shape with `{a:
    /// 1}` and `{a: 1, b: 2}` will create a union `{a: int64} | {a: int64,
    /// b: int64}`.
    Object(ObjectShape<C, S>),

    /// The set of all objects with fields in a single field shape and values in
    /// a single value shape. Objects naturally subtype records if the union
    /// all of their fields is a subtype of the record's field shape and the
    /// union of all their values is a subtype of the record's value shape.
    Record(RecordShape<C, S>),

    /// The union of other shapes. Accurately inferring union shapes when the
    /// user inserts and removes values from the database requires
    /// restrictions here that do not apply to our validator type system.
    /// See [`UnionShape`]'s documentation for more details.
    Union(UnionShape<C, S>),

    /// The set of all `Value`s. All shapes are a subtype of `Unknown`. This
    /// corresponds to TypeScript's `unknown` type (and generally "top" in type
    /// theory) but not its `any` type.
    Unknown,
}

impl<C: ShapeConfig, S: ShapeCounter> Shape<C, S> {
    pub fn variant(&self) -> &ShapeEnum<C, S> {
        &self.variant
    }
}

impl<C: ShapeConfig> StructuralShape<C> {
    pub fn new(variant: StructuralShapeEnum<C>) -> Self {
        Self {
            variant: Arc::new(variant),
            num_values: (),
        }
    }

    pub fn structural_shape_of(value: &ConvexValue) -> Self {
        let counted = CountedShape::shape_of(value);
        Self::from(&counted)
    }
}

impl<C: ShapeConfig> CountedShape<C> {
    /// Create a new shape from a [`ShapeEnum`] and number of values in the
    /// multiset.
    pub fn new(variant: CountedShapeEnum<C>, num_values: u64) -> Self {
        // Allow only the top or bottom shapes when `num_values == 0`. The top shape is
        // useful when computing the supertypes of covariant shapes like `array<never>`
        // (i.e. `shape_of([])`).
        if num_values == 0 {
            assert!(matches!(variant, ShapeEnum::Never | ShapeEnum::Unknown));
        }

        if let ShapeEnum::Object(object_shape) = &variant {
            object_shape.validate_value_counts(num_values);
        }

        Self {
            variant: Arc::new(variant),
            num_values,
        }
    }

    /// Create the empty shape.
    pub fn empty() -> Self {
        Self::new(ShapeEnum::Never, 0)
    }

    /// Is the shape empty?
    pub fn is_empty(&self) -> bool {
        self.num_values == 0
    }

    /// Compute the shape of a value. Note that this transformation isn't
    /// entirely determined by the input value: Based on the [`ShapeConfig`], we
    /// may take a value's "natural" shape to a supertype. For example, we may
    /// either generate [`ShapeEnum::StringLiteral`] or [`ShapeEnum::String`]
    /// based on [`ShapeConfig::is_valid_string_literal`].
    pub fn shape_of(value: &ConvexValue) -> Self {
        let variant = match value {
            ConvexValue::Null => ShapeEnum::Null,
            ConvexValue::Int64(..) => ShapeEnum::Int64,
            ConvexValue::Float64(f) => Float64Shape::shape_of(*f),
            ConvexValue::Boolean(..) => ShapeEnum::Boolean,
            ConvexValue::String(s) => StringLiteralShape::shape_of(s),
            ConvexValue::Bytes(..) => ShapeEnum::Bytes,
            ConvexValue::Array(array) => ArrayShape::shape_of(array),
            ConvexValue::Object(object) => return Self::shape_of_object(object),
        };
        Self::new(variant, 1)
    }

    fn shape_of_object(object: &ConvexObject) -> Self {
        Self::new(ObjectShape::shape_of(object), 1)
    }

    /// Insert a value into a shape, returning the updated shape.
    pub fn insert_value(&self, value: &ConvexValue) -> Self {
        let union_builder = match &*self.variant {
            ShapeEnum::Union(union) => union.clone().into_builder(),
            _ => UnionBuilder::new().push(self.clone()),
        };
        union_builder.push(Self::shape_of(value)).build()
    }

    /// Insert an object into a shape, returning the updated shape.
    pub fn insert(&self, object: &ConvexObject) -> Self {
        let union_builder = match &*self.variant {
            ShapeEnum::Union(union) => union.clone().into_builder(),
            _ => UnionBuilder::new().push(self.clone()),
        };
        union_builder.push(Self::shape_of_object(object)).build()
    }

    /// Remove a value from a shape, returning the updated shape. The value must
    /// have been previously inserted into the shape, and this function may
    /// return `Err` if it wasn't. Since there may be false successes, it's
    /// usually not safe to recover from this `Err`.
    pub fn remove_value(&self, value: &ConvexValue) -> anyhow::Result<Self> {
        self._remove(value)
            .ok_or_else(|| anyhow::anyhow!("Value {value:?} not in {self:?}"))
    }

    /// Remove an object from a shape, returning the updated shape. The value
    /// must have been previously inserted into the shape, and this function
    /// may return `Err` if it wasn't. Since there may be false successes,
    /// it's usually not safe to recover from this `Err`.
    pub fn remove(&self, object: &ConvexObject) -> anyhow::Result<Self> {
        self._remove_object(object).ok_or_else(|| {
            anyhow::anyhow!(
                "Object with shape {:?} and id {:?} not in {self:?}",
                Self::shape_of_object(object),
                object.get("_id"),
            )
        })
    }

    fn _remove(&self, value: &ConvexValue) -> Option<Self> {
        if self.num_values == 0 {
            return None;
        }
        let mut new_variant = match (value, &*self.variant) {
            (ConvexValue::Object(object), _) => return self._remove_object(object),
            (ConvexValue::Null, ShapeEnum::Null) => ShapeEnum::Null,
            (ConvexValue::Int64(..), ShapeEnum::Int64) => ShapeEnum::Int64,
            (ConvexValue::Float64(..), ShapeEnum::Float64) => ShapeEnum::Float64,
            (ConvexValue::Float64(f), ShapeEnum::NegativeInf) => {
                if Float64Shape::<C>::shape_of(*f) == ShapeEnum::NegativeInf {
                    ShapeEnum::NegativeInf
                } else {
                    return None;
                }
            },
            (ConvexValue::Float64(f), ShapeEnum::PositiveInf) => {
                if Float64Shape::<C>::shape_of(*f) == ShapeEnum::PositiveInf {
                    ShapeEnum::PositiveInf
                } else {
                    return None;
                }
            },
            (ConvexValue::Float64(f), ShapeEnum::NegativeZero) => {
                if Float64Shape::<C>::shape_of(*f) == ShapeEnum::NegativeZero {
                    ShapeEnum::NegativeZero
                } else {
                    return None;
                }
            },
            (ConvexValue::Float64(f), ShapeEnum::NaN) => {
                if Float64Shape::<C>::shape_of(*f) == ShapeEnum::NaN {
                    ShapeEnum::NaN
                } else {
                    return None;
                }
            },
            (ConvexValue::Float64(f), ShapeEnum::NormalFloat64) => {
                if Float64Shape::<C>::shape_of(*f) == ShapeEnum::NormalFloat64 {
                    ShapeEnum::NormalFloat64
                } else {
                    return None;
                }
            },
            (ConvexValue::Boolean(..), ShapeEnum::Boolean) => ShapeEnum::Boolean,
            (ConvexValue::String(s1), ShapeEnum::StringLiteral(s2)) if s1[..] == s2[..] => {
                ShapeEnum::StringLiteral(s2.clone())
            },
            (ConvexValue::String(s), ShapeEnum::Id(table_number)) => {
                if let Ok(id) = DeveloperDocumentId::decode(s)
                    && id.table() == *table_number
                {
                    ShapeEnum::Id(*table_number)
                } else {
                    return None;
                }
            },
            (ConvexValue::String(s), ShapeEnum::FieldName) => {
                if s.parse::<FieldName>().is_err() {
                    return None;
                }
                ShapeEnum::FieldName
            },
            (ConvexValue::String(..), ShapeEnum::String) => ShapeEnum::String,
            (ConvexValue::Bytes(..), ShapeEnum::Bytes) => ShapeEnum::Bytes,
            (ConvexValue::Array(array), ShapeEnum::Array(array_shape)) => {
                let mut element_shape = array_shape.element().clone();
                for value in array {
                    element_shape = element_shape._remove(value)?;
                }
                ShapeEnum::Array(ArrayShape::new(element_shape))
            },
            (value, ShapeEnum::Union(union_shape)) => {
                let (existing_shape, removed) = union_shape
                    .iter()
                    .filter_map(|shape| shape._remove(value).map(|removed| (shape, removed)))
                    .next()?;
                let mut builder = union_shape.clone().into_builder();
                assert!(builder.remove(existing_shape));
                builder = builder.push(removed);
                return Some(builder.build());
            },
            (_, ShapeEnum::Unknown) => ShapeEnum::Unknown,
            _ => return None,
        };
        if self.num_values == 1 {
            new_variant = ShapeEnum::Never;
        }
        Some(Self::new(new_variant, self.num_values - 1))
    }

    fn _remove_object(&self, object: &ConvexObject) -> Option<Self> {
        if self.num_values == 0 {
            return None;
        }
        let new_num_values = self.num_values - 1;
        let mut new_variant = match &*self.variant {
            ShapeEnum::Object(object_shape) => {
                let mut fields = BTreeMap::new();
                // Go through all the fields that are in `object` (which must also be in
                // `object_shape`) and compute the new value shape after removing
                // the field value.
                for (field_name, field_value) in object.iter() {
                    let field_name = IdentifierFieldName::try_from(field_name.clone()).ok()?;
                    let field = object_shape.get(&field_name)?;
                    let new_value_shape = field.value_shape._remove(field_value)?;
                    if new_value_shape.is_empty() {
                        continue;
                    }

                    let new_field = ObjectField {
                        value_shape: new_value_shape,
                        optional: field.optional,
                    };
                    fields.insert(field_name.clone(), new_field);
                }
                // Go through the fields in `object_shape` but not `object` and insert them into
                // `fields`. These fields should all be optional since they weren't in `object`.
                for (field_name, field) in object_shape.iter() {
                    if !object.contains_key(&field_name[..]) {
                        if !field.optional {
                            return None;
                        }
                        if field.value_shape.num_values == new_num_values {
                            // If this optional field has a value for every remaining object, mark
                            // it as non-optional
                            fields.insert(
                                field_name.clone(),
                                ObjectField {
                                    value_shape: field.value_shape.clone(),
                                    optional: false,
                                },
                            );
                        } else {
                            fields.insert(field_name.clone(), field.clone());
                        }
                    }
                }
                ShapeEnum::Object(ObjectShape::<C, u64>::new(fields))
            },
            ShapeEnum::Record(record_shape) => {
                let mut field_shape = record_shape.field().clone();
                let mut value_shape = record_shape.value().clone();
                for (field, value) in object.iter() {
                    let field_value = ConvexValue::String(field[..].try_into().unwrap());
                    field_shape = field_shape._remove(&field_value)?;
                    value_shape = value_shape._remove(value)?;
                }
                ShapeEnum::Record(RecordShape::new(field_shape, value_shape))
            },
            ShapeEnum::Union(union_shape) => {
                let (existing_shape, removed) = union_shape
                    .iter()
                    .filter_map(|shape| {
                        shape._remove_object(object).map(|removed| (shape, removed))
                    })
                    .next()?;
                let mut builder = union_shape.clone().into_builder();
                assert!(builder.remove(existing_shape));
                builder = builder.push(removed);
                return Some(builder.build());
            },
            ShapeEnum::Unknown => ShapeEnum::Unknown,
            _ => return None,
        };
        if new_num_values == 0 {
            new_variant = ShapeEnum::Never;
        }
        Some(Self::new(new_variant, new_num_values))
    }

    pub fn num_values(&self) -> &u64 {
        &self.num_values
    }
}

impl<C: ShapeConfig, S: ShapeCounter> ShapeEnum<C, S> {
    pub fn is_empty(&self) -> bool {
        matches!(self, ShapeEnum::Never)
    }

    pub fn is_string_subtype_with_string_literal(&self) -> bool {
        match self {
            ShapeEnum::Never
            | ShapeEnum::Null
            | ShapeEnum::Int64
            | ShapeEnum::Float64
            | ShapeEnum::NegativeInf
            | ShapeEnum::PositiveInf
            | ShapeEnum::NegativeZero
            | ShapeEnum::NaN
            | ShapeEnum::NormalFloat64
            | ShapeEnum::Boolean
            | ShapeEnum::Id(_)
            | ShapeEnum::FieldName
            | ShapeEnum::String
            | ShapeEnum::Bytes
            | ShapeEnum::Array(_)
            | ShapeEnum::Object(_)
            | ShapeEnum::Record(_)
            | ShapeEnum::Unknown => false,

            ShapeEnum::StringLiteral(_) => true,
            ShapeEnum::Union(union_shape) => union_shape
                .iter()
                .any(|t| t.variant.is_string_subtype_with_string_literal()),
        }
    }
}

impl<C: ShapeConfig> From<&CountedShape<C>> for StructuralShape<C> {
    fn from(value: &CountedShape<C>) -> Self {
        Self {
            variant: Arc::new(value.variant.as_ref().into()),
            num_values: (),
        }
    }
}

impl<C: ShapeConfig> From<&CountedShapeEnum<C>> for StructuralShapeEnum<C> {
    fn from(value: &CountedShapeEnum<C>) -> Self {
        match value {
            ShapeEnum::Never => Self::Never,
            ShapeEnum::Null => Self::Null,
            ShapeEnum::Int64 => Self::Int64,
            ShapeEnum::NegativeInf => Self::NegativeInf,
            ShapeEnum::PositiveInf => Self::PositiveInf,
            ShapeEnum::NegativeZero => Self::NegativeZero,
            ShapeEnum::NaN => Self::NaN,
            ShapeEnum::NormalFloat64 => Self::NormalFloat64,
            ShapeEnum::Float64 => Self::Float64,
            ShapeEnum::Boolean => Self::Boolean,
            ShapeEnum::StringLiteral(literal) => Self::StringLiteral(literal.clone()),
            ShapeEnum::Id(table_number) => Self::Id(*table_number),
            ShapeEnum::FieldName => Self::FieldName,
            ShapeEnum::String => Self::String,
            ShapeEnum::Bytes => Self::Bytes,
            ShapeEnum::Array(array) => Self::Array(ArrayShape::new(array.element().into())),
            ShapeEnum::Object(object) => Self::Object(ObjectShape::<C, ()>::new(
                object
                    .iter()
                    .map(|(field, value)| {
                        (
                            field.clone(),
                            ObjectField {
                                value_shape: StructuralShape::from(&value.value_shape),
                                optional: value.optional,
                            },
                        )
                    })
                    .collect(),
            )),
            ShapeEnum::Record(record) => Self::Record(RecordShape::new(
                record.field().into(),
                record.value().into(),
            )),
            ShapeEnum::Union(union) => Self::Union(UnionShape::from_parts(
                union.iter().map(StructuralShape::from).collect(),
            )),
            ShapeEnum::Unknown => Self::Unknown,
        }
    }
}
