//! Bijection between a subset of JSON values and our [`Value`]s.
//!
//! Notable features:
//!
//! 1) JSON numbers (64-bit floating point) are mapped to `Number`s.
//! 2) Int64 integers are encoded as their little endian representation in
//!    base64: {"$integer": "..."}.
//! 3) Blobs are encoded as base64: {"$binary": "..."}.
//! 4) Objects are not allowed to have keys starting with "$".

pub mod bytes;
pub mod float;
pub mod integer;
pub(crate) mod json_packed_value;

use std::{
    collections::BTreeMap,
    num::FpCategory,
};

use anyhow::{
    anyhow,
    bail,
    Error,
    Result,
};
use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::{
    json::{
        bytes::JsonBytes,
        float::JsonFloat,
        integer::JsonInteger,
    },
    numeric::is_negative_zero,
    object::ConvexObject,
    walk::ConvexValueType,
    ConvexArray,
    ConvexValue,
};

pub mod value {
    use std::{
        cell::Cell,
        num::FpCategory,
    };

    use serde::{
        ser::{
            Error,
            SerializeMap,
            SerializeSeq,
        },
        Serialize,
        Serializer,
    };

    use crate::{
        numeric::is_negative_zero,
        walk::{
            ConvexArrayWalker,
            ConvexBytesWalker,
            ConvexObjectWalker,
            ConvexStringWalker,
            ConvexValueType,
            ConvexValueWalker,
        },
        JsonBytes,
        JsonFloat,
        JsonInteger,
    };

    /// Wrapper for `ConvexValueWalker` that implements `Serialize`.
    ///
    /// Note that `ConvexValueWalker` can only be walked once (consuming the
    /// walker) whereas the `serde::Serialize` trait takes `&self`; to bridge
    /// them, we use a `Cell<Option>` and return an error if the same
    /// `SerializeValue` is serialized more than once.
    pub(crate) struct SerializeValue<V>(Cell<Option<V>>);
    impl<V> SerializeValue<V> {
        pub(crate) fn new(value: V) -> Self {
            Self(Cell::new(Some(value)))
        }
    }
    impl<V: ConvexValueWalker> Serialize for SerializeValue<V> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serialize(
                self.0
                    .take()
                    .ok_or_else(|| Error::custom("cannot serialize value more than once"))?,
                serializer,
            )
        }
    }

    pub fn serialize<V: ConvexValueWalker, S: Serializer>(
        value: V,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value.walk().map_err(Error::custom)? {
            ConvexValueType::Null => serializer.serialize_unit(),
            ConvexValueType::Int64(n) => {
                let mut obj = serializer.serialize_map(Some(1))?;
                obj.serialize_entry("$integer", &JsonInteger(n))?;
                obj.end()
            },
            ConvexValueType::Float64(n) => {
                let mut is_special = is_negative_zero(n);
                is_special |= match n.classify() {
                    FpCategory::Zero | FpCategory::Normal | FpCategory::Subnormal => false,
                    FpCategory::Infinite | FpCategory::Nan => true,
                };
                if is_special {
                    let mut obj = serializer.serialize_map(Some(1))?;
                    obj.serialize_entry("$float", &JsonFloat(n))?;
                    obj.end()
                } else {
                    serializer.serialize_f64(n)
                }
            },
            ConvexValueType::Boolean(b) => serializer.serialize_bool(b),
            ConvexValueType::String(s) => serializer.serialize_str(s.as_str()),
            ConvexValueType::Bytes(b) => {
                let mut obj = serializer.serialize_map(Some(1))?;
                obj.serialize_entry("$bytes", &JsonBytes::encode(b.as_bytes()))?;
                obj.end()
            },
            ConvexValueType::Array(a) => {
                let iter = a.walk();
                let mut seq = serializer.serialize_seq(size_hint(&iter))?;
                for value in iter {
                    let value = value.map_err(Error::custom)?;
                    seq.serialize_element(&SerializeValue::new(value))?;
                }
                seq.end()
            },
            ConvexValueType::Object(o) => {
                let iter = o.walk();
                let mut map = serializer.serialize_map(size_hint(&iter))?;
                for pair in iter {
                    let (key, value) = pair.map_err(Error::custom)?;
                    map.serialize_entry(key.as_str(), &SerializeValue::new(value))?;
                }
                map.end()
            },
        }
    }

    fn size_hint(iter: &impl Iterator) -> Option<usize> {
        let (lo, hi) = iter.size_hint();
        if hi == Some(lo) {
            hi
        } else {
            None
        }
    }
}

pub mod object {
    use serde::{
        Deserialize,
        Deserializer,
        Serializer,
    };
    use serde_json::Value as JsonValue;

    use crate::{
        walk::ConvexValueType,
        ConvexObject,
        ConvexValue,
    };

    pub fn serialize<S: Serializer>(
        object: &ConvexObject,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        super::value::serialize(ConvexValueType::<&ConvexValue>::Object(object), serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<ConvexObject, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = JsonValue::deserialize(deserializer)?;
        ConvexObject::try_from(value).map_err(serde::de::Error::custom)
    }
}

impl TryFrom<JsonValue> for ConvexValue {
    type Error = Error;

    #[allow(clippy::float_cmp)]
    fn try_from(value: JsonValue) -> Result<Self> {
        let r = match value {
            JsonValue::Null => Self::Null,
            JsonValue::Bool(b) => Self::from(b),
            JsonValue::Number(n) => {
                // TODO/WTF: JSON supports arbitrary precision numbers?
                let n = n
                    .as_f64()
                    .ok_or_else(|| anyhow!("Arbitrary precision JSON integers unsupported"))?;
                ConvexValue::from(n)
            },
            JsonValue::String(s) => Self::try_from(s)?,
            JsonValue::Array(arr) => {
                let mut out = Vec::with_capacity(arr.len());
                for a in arr {
                    out.push(ConvexValue::try_from(a)?);
                }
                ConvexValue::Array(out.try_into()?)
            },
            JsonValue::Object(map) => {
                if map.len() == 1 {
                    let (key, value) = map.into_iter().next().unwrap();
                    match &key[..] {
                        "$bytes" => {
                            let i: String = serde_json::from_value(value)?;
                            Self::Bytes(JsonBytes::decode(i)?)
                        },
                        "$integer" => {
                            let i: String = serde_json::from_value(value)?;
                            Self::from(JsonInteger::decode(i)?)
                        },
                        "$float" => {
                            let i: String = serde_json::from_value(value)?;
                            let n = JsonFloat::decode(i)?;
                            // Float64s encoded as a $float object must not fit into a regular
                            // `number`.
                            if !is_negative_zero(n)
                                && let FpCategory::Normal | FpCategory::Subnormal = n.classify()
                            {
                                bail!("Float64 {} should be encoded as a number", n);
                            }
                            Self::from(n)
                        },
                        _ => Self::Object(ConvexObject::for_value(
                            key.parse()?,
                            Self::try_from(value)?,
                        )?),
                    }
                } else {
                    let mut fields = BTreeMap::new();
                    for (key, value) in map {
                        fields.insert(key.parse()?, Self::try_from(value)?);
                    }
                    Self::Object(fields.try_into()?)
                }
            },
        };
        Ok(r)
    }
}

impl TryFrom<JsonValue> for ConvexArray {
    type Error = Error;

    fn try_from(object: JsonValue) -> Result<Self> {
        Self::try_from(ConvexValue::try_from(object)?)
    }
}

impl TryFrom<JsonValue> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(object: JsonValue) -> anyhow::Result<Self> {
        ConvexValue::try_from(object)?.try_into()
    }
}

impl From<ConvexValue> for JsonValue {
    fn from(value: ConvexValue) -> Self {
        value.to_internal_json()
    }
}

impl ConvexValue {
    pub fn to_internal_json(&self) -> JsonValue {
        serde_json::to_value(self.to_internal_json_serializable())
            .expect("Failed to serialize to JsonValue")
    }

    pub fn to_internal_json_serializable(&self) -> impl Serialize + '_ {
        value::SerializeValue::new(self)
    }

    pub fn json_serialize(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(
            &self.to_internal_json_serializable(),
        )?)
    }
}

impl From<ConvexObject> for JsonValue {
    fn from(value: ConvexObject) -> Self {
        value.to_internal_json()
    }
}

impl ConvexObject {
    pub fn to_internal_json(&self) -> JsonValue {
        serde_json::to_value(self.to_internal_json_serializable())
            .expect("Failed to serialize to JsonValue")
    }

    pub fn to_internal_json_serializable(&self) -> impl Serialize + '_ {
        value::SerializeValue::new(ConvexValueType::<&ConvexValue>::Object(self))
    }

    pub fn json_serialize(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(
            &self.to_internal_json_serializable(),
        )?)
    }
}

impl ConvexArray {
    pub fn to_internal_json(&self) -> JsonValue {
        serde_json::to_value(self.to_internal_json_serializable())
            .expect("Failed to serialize to JsonValue")
    }

    pub fn to_internal_json_serializable(&self) -> impl Serialize + '_ {
        value::SerializeValue::new(ConvexValueType::<&ConvexValue>::Array(self))
    }

    pub fn json_serialize(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(
            &self.to_internal_json_serializable(),
        )?)
    }
}

pub fn json_deserialize_bytes(s: &[u8]) -> anyhow::Result<ConvexValue> {
    let v: serde_json::Value = serde_json::from_slice(s)?;
    v.try_into()
}

pub fn json_deserialize(s: &str) -> anyhow::Result<ConvexValue> {
    let v: serde_json::Value = serde_json::from_str(s)?;
    v.try_into()
}
