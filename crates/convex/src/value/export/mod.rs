use serde_json::{
    json,
    Value as JsonValue,
};

use crate::Value;

impl Value {
    /// Converts this value to a JSON value in the `json` export format.
    /// <https://docs.convex.dev/database/types>
    ///
    /// It is possible for distinct Convex values to be serialized to the same
    /// JSON value by this method. For instance, strings and binary values are
    /// both exported as JSON strings. However, it is possible to convert the
    /// exported value back to a unique Convex value if you also have the `Type`
    /// value associated with the original Convex value (see `roundtrip.rs`).
    ///
    /// # Example
    /// ```
    /// use convex::Value;
    /// use serde_json::{
    ///     json,
    ///     Value as JsonValue,
    /// };
    ///
    /// let value = Value::Bytes(vec![0b00000000, 0b00010000, 0b10000011]);
    /// assert_eq!(JsonValue::from(value.clone()), json!({ "$bytes": "ABCD" }));
    /// assert_eq!(value.export(), json!("ABCD"));
    /// ```
    pub fn export(self) -> JsonValue {
        match self {
            Value::Null => JsonValue::Null,
            Value::Int64(value) => JsonValue::String(value.to_string()),
            Value::Float64(value) => {
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
            Value::Boolean(value) => JsonValue::Bool(value),
            Value::String(value) => JsonValue::String(value),
            Value::Bytes(value) => JsonValue::String(base64::encode(value)),
            Value::Array(values) => {
                JsonValue::Array(values.into_iter().map(|x| x.export()).collect())
            },
            Value::Object(map) => JsonValue::Object(
                map.into_iter()
                    .map(|(key, value)| (key, value.export()))
                    .collect(),
            ),
        }
    }
}
