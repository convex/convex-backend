use std::collections::BTreeMap;

use common::{
    types::MaybeValue,
    value::FieldName,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use value::{
    ConvexObject,
    PendingValue,
};

/// A object used in patch. Similar to GenericObject but also allows top level
/// undefined fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchValue {
    fields: BTreeMap<FieldName, Option<PendingValue>>,
}

impl PatchValue {
    /// Merge into the original document body, which may itself contain
    /// unresolved commit timestamps that the patch leaves in place.
    pub fn apply(self, original: PendingValue) -> anyhow::Result<PendingValue> {
        let mut original_fields = original.into_object_fields()?;

        for (field, maybe_value) in self.fields {
            match maybe_value {
                Some(value) => {
                    original_fields.insert(field, value);
                },
                None => {
                    original_fields.remove(&field);
                },
            }
        }
        PendingValue::object(original_fields)
    }

    /// Parse internal JSON, additionally accepting the `{"$commitTs": null}`
    /// token in field values (see [`PendingValue::from_uncommitted_json`]) and
    /// `{"$undefined": null}` as a top-level field value to remove the field.
    pub fn from_uncommitted_json(json: JsonValue) -> anyhow::Result<Self> {
        match json {
            JsonValue::Object(map) => {
                let mut fields = BTreeMap::new();
                for (key, value) in map {
                    let value = if value == json!({ "$undefined": null }) {
                        None
                    } else {
                        Some(PendingValue::from_uncommitted_json(value)?)
                    };
                    fields.insert(key.parse()?, value);
                }
                Ok(Self { fields })
            },
            _ => {
                anyhow::bail!("Value must be an Object");
            },
        }
    }
}

impl From<BTreeMap<FieldName, MaybeValue>> for PatchValue {
    fn from(fields: BTreeMap<FieldName, MaybeValue>) -> Self {
        Self {
            fields: fields
                .into_iter()
                .map(|(field, value)| (field, value.0.map(PendingValue::from)))
                .collect(),
        }
    }
}

impl From<ConvexObject> for PatchValue {
    fn from(obj: ConvexObject) -> Self {
        Self {
            fields: obj
                .into_iter()
                .map(|(field, value)| (field, Some(PendingValue::from(value))))
                .collect(),
        }
    }
}

#[macro_export]
/// Create an patch object from field/value pairs.
macro_rules! patch_value {
    ($($field:expr => $val:expr),* $(,)?) => {
        {
            use common::types::MaybeValue;
            use $crate::PatchValue;
            use std::collections::BTreeMap;
            #[allow(unused)]
            let mut fields = BTreeMap::new();
            {
                $(
                    fields.insert($field.parse()?, MaybeValue($val));
                )*
            }
            PatchValue::try_from(fields)
        }
    };
}
