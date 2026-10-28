use rand::Rng;
use serde_bytes::ByteBuf;

use super::OpProvider;
use crate::convert_v8::TypeError;

#[convex_macro::op]
pub fn op_crypto_random_uuid<P: OpProvider>(provider: &mut P) -> anyhow::Result<String> {
    let rng = provider.rng()?;
    let uuid = uuid::Builder::from_random_bytes(rng.random()).into_uuid();
    Ok(uuid.to_string())
}

#[convex_macro::op]
pub fn op_crypto_get_random_values<P: OpProvider>(
    provider: &mut P,
    byte_length: u32,
) -> anyhow::Result<ByteBuf> {
    let rng = provider.rng()?;
    let max_byte_length = 65536;
    anyhow::ensure!(
        byte_length <= max_byte_length,
        TypeError::new(format!(
            "Byte length ({byte_length}) exceeds the number of bytes of entropy available via \
             this API ({max_byte_length})"
        ))
    );
    let byte_length = byte_length as usize;
    let mut bytes = vec![0u8; byte_length];
    rng.fill(&mut bytes[..]);
    Ok(ByteBuf::from(bytes))
}
