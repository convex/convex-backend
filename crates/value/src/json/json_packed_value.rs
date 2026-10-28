use std::{
    marker::PhantomData,
    sync::Arc,
};

use anyhow::Context as _;
use serde_json::{
    value::RawValue,
    Value as JsonValue,
};

use crate::{
    heap_size::HeapSize,
    ConvexValue,
    PendingValue,
    COMMIT_TS_FIELD,
};

/// A value packed as its serialized JSON. The type parameter is the value
/// the string parses back into: [`ConvexValue`] (the default) is the
/// ordinary internal JSON, and [`PendingValue`] additionally admits the
/// `{"$commitTs": null}` token, for boundaries that may carry unresolved
/// commit timestamps (mutation return values within their transaction).
#[derive(Clone, Debug)]
pub struct JsonPackedValue<V: PackedValueFormat = ConvexValue>(Arc<str>, PhantomData<V>);

/// A value format that [`JsonPackedValue`] can pack and unpack.
pub trait PackedValueFormat: Sized {
    fn pack(value: Self) -> String;
    fn unpack(json: JsonValue) -> anyhow::Result<Self>;
}

impl PackedValueFormat for ConvexValue {
    fn pack(value: Self) -> String {
        value
            .json_serialize()
            .expect("Failed to serialize to string")
    }

    fn unpack(json: JsonValue) -> anyhow::Result<Self> {
        ConvexValue::try_from(json).context("JsonPackedValue wasn't a valid ConvexValue")
    }
}

impl PackedValueFormat for PendingValue {
    fn pack(value: Self) -> String {
        match value {
            PendingValue::Concrete(value) => ConvexValue::pack(value),
            pending => serde_json::to_string(&pending.to_uncommitted_json_serializable())
                .expect("Failed to serialize to string"),
        }
    }

    fn unpack(json: JsonValue) -> anyhow::Result<Self> {
        PendingValue::from_uncommitted_json(json)
            .context("JsonPackedValue wasn't a valid PendingValue")
    }
}

impl<V: PackedValueFormat> JsonPackedValue<V> {
    pub fn pack(value: V) -> Self {
        Self(V::pack(value).into(), PhantomData)
    }

    /// This should never return an error, but in theory
    /// `JsonPackedValue::from_network` could accept an invalid value
    pub fn unpack(&self) -> anyhow::Result<V> {
        V::unpack(self.json_value())
    }

    pub fn json_value(&self) -> JsonValue {
        serde_json::from_str(&self.0).expect("Failed to deserialize packed JSON value")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn from_network(json: String) -> anyhow::Result<Self> {
        // TODO: consider checking JSON validity here
        Ok(Self(json.into(), PhantomData))
    }

    pub fn to_raw_value(&self) -> anyhow::Result<Box<RawValue>> {
        Ok(serde_json::from_str(&self.0)?)
    }
}

impl JsonPackedValue<PendingValue> {
    /// Replace each unresolved commit timestamp with `Int64(commit_ts)`,
    /// yielding the packed [`ConvexValue`]
    pub fn resolve_commit_ts(&self, commit_ts: i64) -> anyhow::Result<JsonPackedValue> {
        // Can check for commit_ts quickly to skip unpacking and resolving
        if !self.0.contains(COMMIT_TS_FIELD) {
            return Ok(JsonPackedValue(self.0.clone(), PhantomData));
        }
        Ok(JsonPackedValue::pack(
            self.unpack()?.into_resolved(commit_ts)?,
        ))
    }
}
impl From<JsonPackedValue> for JsonPackedValue<PendingValue> {
    fn from(value: JsonPackedValue) -> Self {
        Self(value.0, PhantomData)
    }
}

impl TryFrom<JsonPackedValue<PendingValue>> for JsonPackedValue {
    type Error = anyhow::Error;

    fn try_from(value: JsonPackedValue<PendingValue>) -> anyhow::Result<Self> {
        if value.0.contains(COMMIT_TS_FIELD) {
            anyhow::ensure!(
                !value.unpack()?.is_pending(),
                "Value contains an unresolved commit timestamp"
            );
        }
        Ok(JsonPackedValue(value.0, PhantomData))
    }
}

impl<V: PackedValueFormat> HeapSize for JsonPackedValue<V> {
    fn heap_size(&self) -> usize {
        self.0.len()
    }
}

// TODO: This impl is only needed to serialize sync protocol messages. Ideally
// that should transfer the JSON data as-is without parsing it into an
// intermediate data structure.
impl From<JsonPackedValue> for JsonValue {
    fn from(value: JsonPackedValue) -> Self {
        value.json_value()
    }
}
