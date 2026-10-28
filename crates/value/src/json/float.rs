use anyhow::anyhow;
use serde::Serialize;

/// Helper for encoding `f64`s as `String`s.
pub struct JsonFloat(pub f64);

impl JsonFloat {
    /// Decode an `f64` from a string.
    pub fn decode(s: String) -> anyhow::Result<f64> {
        let bytes: [u8; 8] = base64::decode(s.as_bytes())?
            .try_into()
            .map_err(|_| anyhow!("Float64 must be exactly eight bytes"))?;
        Ok(f64::from_le_bytes(bytes))
    }
}

impl Serialize for JsonFloat {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut buf = [0u8; 12];
        assert_eq!(
            base64::encode_config_slice(self.0.to_le_bytes(), base64::STANDARD, &mut buf),
            12
        );
        // SAFETY: base64 encoding is always valid UTF-8
        serializer.serialize_str(unsafe { str::from_utf8_unchecked(&buf) })
    }
}
