#![feature(portable_simd)]
#![feature(btree_cursors)]
#![feature(try_blocks_heterogeneous)]
#![feature(never_type)]
#![feature(iterator_try_collect)]
#![feature(impl_trait_in_assoc_type)]

mod array;
pub mod base32;
pub mod base64;
mod bytes;
mod document_id;
pub mod export;
mod field_name;
mod field_path;
pub mod id_v6;
mod json;
pub mod numeric;
mod object;
mod pending;
pub mod serde;
pub mod serialized_args_ext;
pub mod sha256;
mod size;
pub mod sorting;
mod string;
mod table_mapping;
mod table_name;
pub mod walk;

// Helper modules we'll eventually factor out.
pub mod heap_size;
pub mod utils;

mod macros;
use std::{
    collections::BTreeMap,
    fmt::{
        self,
        Display,
    },
    hash::{
        Hash,
        Hasher,
    },
};

use anyhow::{
    bail,
    Error,
};
pub use paste::paste;
pub use sync_types::identifier;
use walk::ConvexValueWalker;

pub use crate::{
    array::ConvexArray,
    bytes::ConvexBytes,
    document_id::{
        DeveloperDocumentId,
        InternalDocumentId,
        InternalId,
        ResolvedDocumentId,
    },
    field_name::{
        FieldName,
        FieldType,
        IdentifierFieldName,
    },
    field_path::FieldPath,
    json::{
        bytes::JsonBytes,
        float::JsonFloat,
        integer::JsonInteger,
        json_deserialize,
        json_deserialize_bytes,
        json_packed_value::JsonPackedValue,
        object as json_object,
        value as json_value,
    },
    object::{
        remove_boolean,
        remove_int64,
        remove_nullable_int64,
        remove_nullable_object,
        remove_nullable_string,
        remove_nullable_vec,
        remove_nullable_vec_of_strings,
        remove_object,
        remove_string,
        remove_vec,
        remove_vec_of_strings,
        ConvexObject,
        MAX_OBJECT_FIELDS,
    },
    pending::{
        PendingValue,
        COMMIT_TS_FIELD,
        MAX_COMMIT_TS,
    },
    size::{
        array_size,
        Size,
        MAX_NESTING,
        MAX_SIZE,
        VALUE_TOO_LARGE_SHORT_MSG,
    },
    sorting::values_to_bytes,
    string::ConvexString,
    table_mapping::{
        NamespacedTableMapping,
        TableMapping,
        TableMappingValue,
        TableNamespace,
    },
    table_name::{
        TableName,
        TableNumber,
        TableType,
        TabletId,
        TabletIdAndTableNumber,
        METADATA_PREFIX,
    },
};

/// The various types that can be stored as a field in a [`ConvexObject`].
#[derive(Clone, Debug)]
pub enum ConvexValue {
    /// Sentinel `Null` value.
    Null,

    /// 64-bit signed integer.
    Int64(i64),

    /// IEEE754 double-precision floating point number with NaNs, negative zero,
    /// and subnormal numbers supported.
    Float64(f64),

    /// Boolean value.
    Boolean(bool),

    /// Text strings, represented as a sequence of Unicode scalar values. Scalar
    /// values must be encoded as codepoints without surrogate pairs: The
    /// sequence of codepoints may not contain *any* surrogate pairs, even
    /// if they are correctly paired up.
    String(ConvexString),

    /// Arbitrary binary data.
    Bytes(ConvexBytes),

    /// Arrays of (potentially heterogeneous) [`ConvexValue`]s.
    Array(ConvexArray),

    /// Nested object with [`FieldName`] keys and (potentially heterogenous)
    /// values.
    Object(ConvexObject),
}

impl ConvexValue {
    /// Returns a string description of the type of this Value.
    pub fn type_name(&self) -> &'static str {
        let Ok(ty) = self.walk();
        ty.type_name()
    }
}

impl From<ConvexObject> for ConvexValue {
    fn from(o: ConvexObject) -> ConvexValue {
        ConvexValue::Object(o)
    }
}

impl From<ResolvedDocumentId> for ConvexValue {
    fn from(value: ResolvedDocumentId) -> Self {
        DeveloperDocumentId::from(value).into()
    }
}

impl From<DeveloperDocumentId> for ConvexValue {
    fn from(value: DeveloperDocumentId) -> Self {
        ConvexValue::String(
            value
                .encode()
                .try_into()
                .expect("Could not create Value::String for ID string"),
        )
    }
}

impl TryFrom<ConvexValue> for DeveloperDocumentId {
    type Error = anyhow::Error;

    fn try_from(value: ConvexValue) -> Result<Self, Self::Error> {
        if let ConvexValue::String(s) = value {
            DeveloperDocumentId::decode(&s).map_err(|e| anyhow::anyhow!(e))
        } else {
            Err(anyhow::anyhow!("Value is not an ID"))
        }
    }
}

impl From<i64> for ConvexValue {
    fn from(i: i64) -> Self {
        Self::Int64(i)
    }
}

impl From<f64> for ConvexValue {
    fn from(i: f64) -> Self {
        Self::Float64(i)
    }
}

impl From<bool> for ConvexValue {
    fn from(i: bool) -> Self {
        Self::Boolean(i)
    }
}

impl TryFrom<String> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(i: String) -> anyhow::Result<Self> {
        Ok(Self::String(ConvexString::try_from(i)?))
    }
}

impl<T: TryInto<ConvexValue, Error = anyhow::Error>> TryFrom<Option<T>> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(v: Option<T>) -> anyhow::Result<Self> {
        Ok(match v {
            None => ConvexValue::Null,
            Some(v) => v.try_into()?,
        })
    }
}

impl<'a> TryFrom<&'a str> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(i: &'a str) -> anyhow::Result<Self> {
        Ok(Self::String(i.to_owned().try_into()?))
    }
}

impl TryFrom<Vec<u8>> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(i: Vec<u8>) -> anyhow::Result<Self> {
        Ok(Self::Bytes(ConvexBytes::try_from(i)?))
    }
}

impl TryFrom<Vec<ConvexValue>> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(i: Vec<ConvexValue>) -> anyhow::Result<Self> {
        Ok(Self::Array(i.try_into()?))
    }
}

impl TryFrom<BTreeMap<FieldName, ConvexValue>> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(i: BTreeMap<FieldName, ConvexValue>) -> anyhow::Result<Self> {
        Ok(Self::Object(i.try_into()?))
    }
}

impl TryFrom<ConvexValue> for bool {
    type Error = Error;

    fn try_from(v: ConvexValue) -> anyhow::Result<Self> {
        match v {
            ConvexValue::Boolean(b) => Ok(b),
            _ => bail!("Value must be a Boolean"),
        }
    }
}

impl TryFrom<ConvexValue> for i64 {
    type Error = Error;

    fn try_from(v: ConvexValue) -> anyhow::Result<Self> {
        match v {
            ConvexValue::Int64(i) => Ok(i),
            _ => bail!("Value must be an integer"),
        }
    }
}

impl TryFrom<ConvexValue> for ConvexString {
    type Error = Error;

    fn try_from(v: ConvexValue) -> anyhow::Result<Self> {
        match v {
            ConvexValue::String(s) => Ok(s),
            _ => bail!("Value must be a string"),
        }
    }
}

impl TryFrom<ConvexValue> for String {
    type Error = Error;

    fn try_from(v: ConvexValue) -> anyhow::Result<Self> {
        Ok(ConvexString::try_from(v)?.into())
    }
}

impl TryFrom<ConvexValue> for ConvexArray {
    type Error = Error;

    fn try_from(v: ConvexValue) -> anyhow::Result<Self> {
        match v {
            ConvexValue::Array(a) => Ok(a),
            _ => bail!("Value must be an Array"),
        }
    }
}

impl TryFrom<ConvexValue> for ConvexObject {
    type Error = Error;

    fn try_from(v: ConvexValue) -> anyhow::Result<Self> {
        match v {
            ConvexValue::Object(o) => Ok(o),
            _ => bail!("Value must be an Object"),
        }
    }
}

impl Display for ConvexValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConvexValue::Null => write!(f, "null"),
            ConvexValue::Int64(n) => write!(f, "{n}"),
            ConvexValue::Float64(n) => write!(f, "{n:?}"),
            ConvexValue::Boolean(b) => write!(f, "{b:?}"),
            ConvexValue::String(s) => write!(f, "{s:?}"),
            ConvexValue::Bytes(b) => write!(f, "{b}"),
            ConvexValue::Array(arr) => write!(f, "{arr}"),
            ConvexValue::Object(m) => write!(f, "{m}"),
        }
    }
}

impl Size for ConvexValue {
    fn size(&self) -> usize {
        match self {
            ConvexValue::Null => 1,
            ConvexValue::Int64(_) => 1 + 8,
            ConvexValue::Float64(_) => 1 + 8,
            ConvexValue::Boolean(_) => 1,
            ConvexValue::String(s) => s.size(),
            ConvexValue::Bytes(b) => b.size(),
            ConvexValue::Array(arr) => arr.size(),
            ConvexValue::Object(m) => m.size(),
        }
    }

    fn nesting(&self) -> usize {
        match self {
            ConvexValue::Null => 0,
            ConvexValue::Int64(_) => 0,
            ConvexValue::Float64(_) => 0,
            ConvexValue::Boolean(_) => 0,
            ConvexValue::String(_) => 0,
            ConvexValue::Bytes(_) => 0,
            ConvexValue::Array(arr) => arr.nesting(),
            ConvexValue::Object(m) => m.nesting(),
        }
    }
}

/// f64 doesn't implement `hash` so we need to manually implement `hash` for
/// `ConvexValue`. Must be compatible with our manual implementation of `cmp`.
impl Hash for ConvexValue {
    fn hash<H: Hasher>(&self, h: &mut H) {
        match self {
            ConvexValue::Null => {
                h.write_u8(2);
            },
            ConvexValue::Int64(i) => {
                h.write_u8(3);
                i.hash(h);
            },
            ConvexValue::Float64(f) => {
                h.write_u8(4);
                f.to_le_bytes().hash(h);
            },
            ConvexValue::Boolean(b) => {
                h.write_u8(5);
                b.hash(h);
            },
            ConvexValue::String(s) => {
                h.write_u8(6);
                s.hash(h);
            },
            ConvexValue::Bytes(b) => {
                h.write_u8(7);
                b.hash(h);
            },
            ConvexValue::Array(a) => {
                h.write_u8(8);
                a.hash(h);
            },
            ConvexValue::Object(o) => {
                h.write_u8(11);
                o.hash(h);
            },
        }
    }
}

pub trait Namespace {
    fn is_system(&self) -> bool;
}
