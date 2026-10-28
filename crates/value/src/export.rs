use std::{
    collections::BTreeMap,
    str::FromStr,
};

use errors::ErrorMetadata;
use serde_json::{
    json,
    Value as JsonValue,
};

use crate::{
    ConvexObject,
    ConvexValue,
    FieldName,
};

/// There are multiple ways a client may want their return Values represented.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueFormat {
    /// The default, representing values with invertible encoding.
    /// The websocket protocol uses this encoding.
    /// e.g. integers are represented as {"$int": "base64-encoded-int"}
    ConvexEncodedJSON,
    /// Representing values with a lossy, but more human readable encoding.
    /// e.g. integers are represented as stringified "33"
    ///
    /// On import, we'll use the same algorithm as `ConvexEncodedJSON` (hence
    /// lossyness)
    ///
    /// On export, we'll use the clean export algorithm: implemented as
    /// `.export_clean()` in `Value`
    ///
    /// <https://www.notion.so/convex-dev/Clean-Export-serialization-c02508b390f54bdfa1cfdf06f9b7f71e>.
    ConvexCleanJSON,
    /// Representing values with a lossless(*) but human-readable encoding.
    ///
    /// (*): lossless to Rust; int64s larger than Number.MAX_SAFE_VALUE will
    /// lose precision if parsed in JS, and other languages may have trouble
    /// distinguishing between int64 and float.
    ConvexExportJSON,
}

impl FromStr for ValueFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "convex_encoded_json" => Ok(Self::ConvexEncodedJSON),
            "convex_json" => Ok(Self::ConvexEncodedJSON), // Legacy alias
            "json" => Ok(Self::ConvexCleanJSON),
            "convex_clean_json" => Ok(Self::ConvexCleanJSON), // Legacy alias
            "export_json" => Ok(Self::ConvexExportJSON),
            _ => Err(anyhow::anyhow!("unrecognized value format {s:?}").context(
                ErrorMetadata::bad_request(
                    "BadFormat",
                    format!("format param must be one of [`json`]. Got {s}"),
                ),
            )),
        }
    }
}

impl ValueFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ValueFormat::ConvexEncodedJSON => "convex_encoded_json",
            ValueFormat::ConvexCleanJSON => "json",
            ValueFormat::ConvexExportJSON => "export_json",
        }
    }
}

impl ConvexObject {
    pub fn export(self, value_format: ValueFormat) -> JsonValue {
        let v: serde_json::Map<_, _> = self
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.export(value_format)))
            .collect();
        JsonValue::Object(v)
    }
}

impl ConvexValue {
    pub fn export(self, value_format: ValueFormat) -> JsonValue {
        match value_format {
            ValueFormat::ConvexEncodedJSON => self.to_internal_json(),
            ValueFormat::ConvexCleanJSON => self.export_clean(),
            ValueFormat::ConvexExportJSON => self.export_clean_lossless(),
        }
    }

    /// Converts this value to a JSON value that is more convenient to work with
    /// than the internal representation.
    ///
    /// It is possible for distinct Convex values to be serialized to the same
    /// JSON value by this method. For instance, strings and binary values are
    /// both exported as JSON strings. However, it is possible to convert the
    /// exported value back to a unique Convex value if you also have the `Type`
    /// value associated with the original Convex value (see `roundtrip.rs`).
    ///
    /// # Example
    /// ```
    /// use crate::ConvexValue;
    /// use serde_json::{
    ///     json,
    ///     Value as JsonValue,
    /// };
    ///
    /// let value = ConvexValue::Bytes(vec![0b00000000, 0b00010000, 0b10000011]);
    /// assert_eq!(JsonValue::from(value.clone()), json!({ "$bytes": "ABCD" }));
    /// assert_eq!(value.export(), json!("ABCD"));
    /// ```
    fn export_clean(self) -> JsonValue {
        match self {
            ConvexValue::Null => JsonValue::Null,
            ConvexValue::Int64(value) => JsonValue::String(value.to_string()),
            ConvexValue::Float64(value) => {
                if value.is_nan() {
                    json!("NaN")
                } else if value.is_infinite() {
                    if value.is_sign_positive() {
                        json!("Infinity")
                    } else {
                        json!("-Infinity")
                    }
                } else {
                    value.into()
                }
            },
            ConvexValue::Boolean(value) => JsonValue::Bool(value),
            ConvexValue::String(value) => JsonValue::String(value.into()),
            ConvexValue::Bytes(value) => {
                let bytes: Vec<u8> = value.into();
                JsonValue::String(base64::encode(bytes))
            },
            ConvexValue::Array(values) => {
                JsonValue::Array(values.into_iter().map(|x| x.export_clean()).collect())
            },
            ConvexValue::Object(map) => JsonValue::Object(
                map.into_iter()
                    .map(|(key, value)| (key.into(), value.export_clean()))
                    .collect(),
            ),
        }
    }

    /// See [`ValueFormat::ConvexExportJSON`].
    fn export_clean_lossless(self) -> JsonValue {
        match self {
            ConvexValue::Null => JsonValue::Null,
            ConvexValue::Int64(value) => {
                // serializes as a decimal number, e.g. "123"
                value.into()
            },
            ConvexValue::Float64(value) => {
                if value.is_finite() {
                    // serializes as a decimal number with a dot, e.g "123.0" or "-0.0"
                    value.into()
                } else {
                    // use the `{"$integer":"<base64>"}` encoding
                    self.to_internal_json()
                }
            },
            ConvexValue::Boolean(value) => JsonValue::Bool(value),
            ConvexValue::String(value) => JsonValue::String(value.into()),
            this @ ConvexValue::Bytes(_) => {
                // use the `{"$bytes":"<base64>"}` encoding
                this.to_internal_json()
            },
            ConvexValue::Array(values) => JsonValue::Array(
                values
                    .into_iter()
                    .map(|x| x.export_clean_lossless())
                    .collect(),
            ),
            ConvexValue::Object(map) => JsonValue::Object(
                map.into_iter()
                    .map(|(key, value)| (key.into(), value.export_clean_lossless()))
                    .collect(),
            ),
        }
    }

    /// See [`ValueFormat::ConvexExportJSON`].
    pub fn from_clean_lossless(j: JsonValue) -> anyhow::Result<Self> {
        Ok(match j {
            JsonValue::Null => Self::Null,
            JsonValue::Bool(b) => Self::Boolean(b),
            JsonValue::Number(number) => {
                if let Some(number) = number.as_i64() {
                    Self::Int64(number)
                } else if let Some(number) = number.as_f64() {
                    Self::Float64(number)
                } else {
                    anyhow::bail!("number is neither i64 nor f64: {number:?}");
                }
            },
            JsonValue::String(s) => Self::String(s.try_into()?),
            JsonValue::Array(a) => Self::Array(
                a.into_iter()
                    .map(Self::from_clean_lossless)
                    .collect::<anyhow::Result<Vec<Self>>>()?
                    .try_into()?,
            ),
            JsonValue::Object(m) => {
                if m.keys().next().is_some_and(|s| s.starts_with("$")) {
                    // parse as internal JSON
                    Self::try_from(JsonValue::Object(m))?
                } else {
                    Self::Object(
                        m.into_iter()
                            .map(|(k, v)| {
                                anyhow::Ok((FieldName::try_from(k)?, Self::from_clean_lossless(v)?))
                            })
                            .collect::<anyhow::Result<BTreeMap<_, _>>>()?
                            .try_into()?,
                    )
                }
            },
        })
    }
}
