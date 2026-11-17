use anyhow::anyhow;
use serde::Serialize;
use serde_json::Value as JsonValue;

/// Helper for encoding `Int64`s as `String`s.
pub struct JsonInteger(pub i64);

impl JsonInteger {
    /// Decode an integer from a string.
    pub fn decode(s: String) -> anyhow::Result<i64> {
        let bytes: [u8; 8] = base64::decode(s.as_bytes())?
            .try_into()
            .map_err(|_| anyhow!("Int64 must be exactly eight bytes"))?;
        Ok(i64::from_le_bytes(bytes))
    }
}

impl Serialize for JsonInteger {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut buf = [0u8; 12];
        let bytes = base64::encode_config_slice(self.0.to_le_bytes(), base64::STANDARD, &mut buf);
        assert_eq!(bytes, 12);
        // SAFETY: base64 encoding is always valid UTF-8
        serializer.serialize_str(unsafe { str::from_utf8_unchecked(&buf) })
    }
}

impl From<JsonInteger> for JsonValue {
    fn from(value: JsonInteger) -> JsonValue {
        serde_json::to_value(value).unwrap()
    }
}
