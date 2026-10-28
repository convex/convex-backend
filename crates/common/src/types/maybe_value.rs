use std::fmt::{
    self,
    Debug,
    Display,
};

use errors::ErrorMetadata;
use serde_json::{
    json,
    Value as JsonValue,
};
use value::ConvexValue;

#[macro_export]
macro_rules! maybe_val {
    ( undefined ) => ({
        $crate::types::MaybeValue(None)
    });
    ( $($tt:tt)+ ) => ({
        $crate::types::MaybeValue(Some( $crate::val!( $($tt)+ ) ))
    });
}

/// Wrapper on `Value` that permits the value to be missing. This missing value
/// corresponds to `undefined` in JavaScript.
///
/// Note that `None` here differs from `Some(Value::Null)`, since `Value::Null`
/// is a valid Convex value, but `undefined` is not. This is why MaybeValue is
/// used only in a limited number of features such as db.patch() and field
/// expressions for a nonexistent fields.
///
/// `None` (or `undefined` in JavaScript) is defined to sort before all other
/// values.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct MaybeValue(pub Option<ConvexValue>);

impl Display for MaybeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(ref v) => write!(f, "{v}"),
            None => write!(f, "undefined"),
        }
    }
}

impl MaybeValue {
    pub fn type_name(&self) -> &'static str {
        match self.0 {
            Some(ref v) => v.type_name(),
            None => "undefined",
        }
    }

    pub fn into_boolean(self) -> anyhow::Result<bool> {
        match &self.0 {
            Some(ConvexValue::Boolean(b)) => Ok(*b),
            _ => anyhow::bail!(ErrorMetadata::bad_request(
                "EvalError",
                format!(
                    "Cannot use value {self} (type {}) as a Boolean",
                    self.type_name()
                )
            )),
        }
    }

    pub fn to_internal_json(&self) -> JsonValue {
        match &self.0 {
            Some(value) => value.to_internal_json(),
            None => json!({ "$undefined": null }),
        }
    }
}

impl From<ConvexValue> for MaybeValue {
    fn from(v: ConvexValue) -> Self {
        Self(Some(v))
    }
}

impl From<i64> for MaybeValue {
    fn from(v: i64) -> Self {
        Self(Some(ConvexValue::from(v)))
    }
}

impl From<f64> for MaybeValue {
    fn from(v: f64) -> Self {
        Self(Some(ConvexValue::from(v)))
    }
}

impl TryFrom<JsonValue> for MaybeValue {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> anyhow::Result<Self> {
        if value == json!({ "$undefined": null }) {
            return Ok(maybe_val!(undefined));
        }
        let v = value.try_into()?;
        Ok(MaybeValue(Some(v)))
    }
}

impl From<MaybeValue> for JsonValue {
    fn from(value: MaybeValue) -> Self {
        value.to_internal_json()
    }
}
