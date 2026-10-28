use std::collections::BTreeMap;

use serde::{
    de::Error as DeError,
    ser::{
        SerializeMap,
        SerializeSeq,
    },
    Deserialize,
    Serialize,
};

use crate::{
    ConvexArray,
    ConvexObject,
    ConvexValue,
    FieldName,
};

impl Serialize for ConvexValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ConvexValue::Null => serializer.serialize_unit(),
            ConvexValue::Int64(n) => serializer.serialize_i64(*n),
            ConvexValue::Float64(n) => serializer.serialize_f64(*n),
            ConvexValue::Boolean(b) => serializer.serialize_bool(*b),
            ConvexValue::String(s) => serializer.serialize_str(s),
            ConvexValue::Bytes(b) => serializer.serialize_bytes(b),
            ConvexValue::Array(a) => a.serialize(serializer),
            ConvexValue::Object(o) => o.serialize(serializer),
        }
    }
}

impl Serialize for ConvexObject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut serializer = serializer.serialize_map(Some(self.len()))?;
        for (key, value) in self.iter() {
            serializer.serialize_entry(key, value)?;
        }
        serializer.end()
    }
}

impl Serialize for ConvexArray {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut serializer = serializer.serialize_seq(Some(self.len()))?;
        for element in self {
            serializer.serialize_element(element)?;
        }
        serializer.end()
    }
}

impl Serialize for FieldName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self[..].serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ConvexValue {
    fn deserialize<D>(deserializer: D) -> Result<ConvexValue, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ConvexValueVisitor;

        impl<'de> serde::de::Visitor<'de> for ConvexValueVisitor {
            type Value = ConvexValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a ConvexValue")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(ConvexValue::Null)
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(ConvexValue::Int64(v))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(ConvexValue::Float64(v))
            }

            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(ConvexValue::Boolean(v))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(ConvexValue::String(v.try_into().map_err(E::custom)?))
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(ConvexValue::Bytes(
                    v.to_vec().try_into().map_err(E::custom)?,
                ))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut vec = Vec::new();
                while let Some(value) = seq.next_element()? {
                    vec.push(value);
                }
                Ok(ConvexValue::Array(
                    vec.try_into().map_err(A::Error::custom)?,
                ))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut m = BTreeMap::<FieldName, ConvexValue>::new();
                while let Some((key, value)) = map.next_entry()? {
                    m.insert(key, value);
                }
                Ok(ConvexValue::Object(m.try_into().map_err(A::Error::custom)?))
            }
        }

        deserializer.deserialize_any(ConvexValueVisitor)
    }
}

impl<'de> Deserialize<'de> for ConvexObject {
    fn deserialize<D>(deserializer: D) -> Result<ConvexObject, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let m: BTreeMap<FieldName, ConvexValue> = Deserialize::deserialize(deserializer)?;
        m.try_into().map_err(D::Error::custom)
    }
}

impl<'de> Deserialize<'de> for ConvexArray {
    fn deserialize<D>(deserializer: D) -> Result<ConvexArray, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v: Vec<ConvexValue> = Deserialize::deserialize(deserializer)?;
        v.try_into().map_err(D::Error::custom)
    }
}

impl<'de> Deserialize<'de> for FieldName {
    fn deserialize<D>(deserializer: D) -> Result<FieldName, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<FieldName>().map_err(D::Error::custom)
    }
}
