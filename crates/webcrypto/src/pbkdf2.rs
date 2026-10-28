use std::num::NonZeroU32;

use aws_lc_rs::pbkdf2;
use indexmap::IndexSet;
use serde::{
    Deserialize,
    Serialize,
};

use super::{
    check_usages_subset,
    ensure,
    CryptoHash,
    CryptoKey,
    CryptoKeyKind,
    DOMExceptionName,
    Error,
    ImportKeyInput,
    KeyType,
    KeyUsage,
    Result,
    DERIVE_BITS_MAX,
};

pub struct Pbkdf2Key {
    secret: Vec<u8>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct ImportKeyAlgorithm;

#[derive(Deserialize, Debug)]
pub struct Pbkdf2Params {
    #[serde(with = "super::nullary_algorithm")]
    pub hash: CryptoHash,
    pub iterations: u32,
    pub salt: serde_bytes::ByteBuf,
}

#[derive(Serialize, Debug)]
#[serde(rename = "PBKDF2")]
#[serde(tag = "name")]
pub struct Pbkdf2Algorithm {}

pub fn derive_bits(
    algorithm: Pbkdf2Params,
    key: &CryptoKey,
    length: Option<usize>,
) -> Result<Vec<u8>> {
    let CryptoKeyKind::Pbkdf2 { key, .. } = &key.kind else {
        return Err(Error::dom(
            "Key algorithm mismatch",
            DOMExceptionName::InvalidAccessError,
        ));
    };
    let Pbkdf2Params {
        hash,
        iterations,
        salt,
    } = algorithm;
    let Some(iterations) = NonZeroU32::new(iterations) else {
        return Err(Error::dom(
            "iterations cannot be zero",
            DOMExceptionName::OperationError,
        ));
    };
    let Some(length) = length else {
        return Err(Error::dom(
            "length cannot be null",
            DOMExceptionName::OperationError,
        ));
    };
    ensure!(
        length % 8 == 0,
        Error::dom(
            "length must be a multiple of 8",
            DOMExceptionName::OperationError
        )
    );
    ensure!(
        length <= DERIVE_BITS_MAX,
        Error::dom(
            format!("cannot generate more than {DERIVE_BITS_MAX} bits"),
            DOMExceptionName::OperationError
        )
    );
    let byte_length = length / 8;
    ensure!(
        // roughly 1s of CPU time in the worst case
        iterations.get() as u64 * byte_length.max(64) as u64 <= 150_000_000,
        Error::dom("too many iterations", DOMExceptionName::OperationError)
    );
    let algorithm = match hash {
        CryptoHash::Sha1 => pbkdf2::PBKDF2_HMAC_SHA1,
        CryptoHash::Sha256 => pbkdf2::PBKDF2_HMAC_SHA256,
        CryptoHash::Sha384 => pbkdf2::PBKDF2_HMAC_SHA384,
        CryptoHash::Sha512 => pbkdf2::PBKDF2_HMAC_SHA512,
    };
    let secret = &key.secret;
    let mut out = vec![0; byte_length];
    pbkdf2::derive(algorithm, iterations, &salt, secret, &mut out);
    Ok(out)
}

pub fn import_key(
    format: ImportKeyInput,
    extractable: bool,
    usages: IndexSet<KeyUsage>,
) -> Result<CryptoKey> {
    let ImportKeyInput::Raw(secret) = format else {
        return Err(Error::dom(
            "unsupported input format",
            DOMExceptionName::NotSupportedError,
        ));
    };
    check_usages_subset(&usages, &[KeyUsage::DeriveKey, KeyUsage::DeriveBits])?;
    ensure!(
        !extractable,
        Error::dom(
            "PBKDF2 keys cannot be extractable",
            DOMExceptionName::SyntaxError
        )
    );
    Ok(CryptoKey {
        kind: CryptoKeyKind::Pbkdf2 {
            algorithm: Pbkdf2Algorithm {},
            key: Pbkdf2Key { secret },
        },
        r#type: KeyType::Secret,
        extractable,
        usages,
    })
}
