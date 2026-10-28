mod de;
mod ser;
mod value;

pub use de::{
    from_object,
    from_value,
    Error as DeError,
};
pub use ser::{
    to_object,
    to_value,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::ConvexObject;

pub trait ConvexSerializable: Sized {
    type Serialized: Serialize
        + for<'de> Deserialize<'de>
        + TryFrom<Self, Error: Into<anyhow::Error>>
        + TryInto<Self, Error: Into<anyhow::Error>>;
}

#[macro_export]
macro_rules! codegen_convex_serialization {
    ($struct:ident, $serialized_struct:ident) => {
        codegen_convex_serialization!($struct, $serialized_struct, test_cases = 256);
    };
    ($struct:ident, $serialized_struct:ident, test_cases = $test_cases:expr) => {
        impl TryFrom<$struct> for value::ConvexObject {
            type Error = anyhow::Error;

            fn try_from(s: $struct) -> anyhow::Result<value::ConvexObject> {
                value::serde::to_object($serialized_struct::try_from(s)?)
            }
        }

        impl TryFrom<$struct> for value::ConvexValue {
            type Error = anyhow::Error;

            fn try_from(s: $struct) -> anyhow::Result<value::ConvexValue> {
                Ok(value::ConvexObject::try_from(s)?.into())
            }
        }

        impl TryFrom<value::ConvexObject> for $struct {
            type Error = anyhow::Error;

            fn try_from(s: value::ConvexObject) -> anyhow::Result<$struct> {
                let r = value::serde::from_object::<$serialized_struct>(s)?.try_into()?;
                Ok(r)
            }
        }

        impl TryFrom<value::ConvexValue> for $struct {
            type Error = anyhow::Error;

            fn try_from(s: value::ConvexValue) -> anyhow::Result<$struct> {
                value::ConvexObject::try_from(s)?.try_into()
            }
        }

        impl $crate::serde::ConvexSerializable for $struct {
            type Serialized = $serialized_struct;
        }

        $crate::paste! {
        }
    };
}

/// For forwards compatibility on enums, it's often useful to preserve an
/// unknown variant as a raw `ConvexObject`. To do so, wrap your enum in this
/// struct:
/// ```ignore,rust
/// #[derive(Serialize, Deserialize)]
/// struct SerializedStruct {
///     state: WithUnknown<SerializedEnum>,
///     another_field: String,
/// }
///
/// #[derive(Serialize, Deserialize)]
/// #[serde(tag = "type")]
/// enum SerializedEnum {
///     Variant1 {
///        field: i32,
///     },
///     Variant2 {
///        another_field: String,
///     },
/// }
/// ```
/// With this setup, `state` will be `WithUnknown::Unknown` when
/// `SerializedEnum` fails to deserialize, so we can preserve an unknown variant
/// for forwards compatibility.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum WithUnknown<T> {
    Known(T),
    Unknown(ConvexObject),
}
