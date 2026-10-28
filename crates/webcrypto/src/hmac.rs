use std::num::NonZeroUsize;

use aws_lc_rs::hmac;
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
    CryptoRng,
    DOMExceptionName,
    Error,
    ImportKeyInput,
    JsonWebKey,
    KeyData,
    KeyFormat,
    KeyType,
    KeyUsage,
    Result,
    URL_SAFE_FORGIVING,
};

#[derive(Deserialize, Debug)]
pub struct HmacImportParams {
    /// The hash member represents the inner hash function to use.
    #[serde(with = "super::nullary_algorithm")]
    pub hash: CryptoHash,
    /// The length member represent the length (in bits) of the key.
    pub length: Option<u32>,
}

#[derive(Serialize)]
#[serde(tag = "name")]
#[serde(rename = "HMAC")]
pub struct HmacKeyAlgorithm {
    /// The hash member represents the inner hash function to use.
    #[serde(with = "super::nullary_algorithm")]
    pub hash: CryptoHash,
    /// The length member represent the length (in bits) of the key.
    pub length: u32,
}
fn jwk_alg(hash: &CryptoHash) -> &'static str {
    match hash {
        CryptoHash::Sha1 => "HS1",
        CryptoHash::Sha256 => "HS256",
        CryptoHash::Sha384 => "HS384",
        CryptoHash::Sha512 => "HS512",
    }
}

pub type HmacKeyGenParams = HmacImportParams;

pub struct HmacKey {
    data: Vec<u8>,
    key: hmac::Key,
}

pub fn generate_key(
    algorithm: HmacKeyGenParams,
    _rng: &CryptoRng,
    extractable: bool,
    usages: IndexSet<KeyUsage>,
) -> Result<CryptoKey> {
    check_usages_subset(&usages, &[KeyUsage::Sign, KeyUsage::Verify])?;
    let length = algorithm
        .key_length()?
        .ok_or_else(|| Error::dom("length must not be zero", DOMExceptionName::OperationError))?
        .get();
    let mut key_bytes = vec![0u8; length.div_ceil(8)];
    aws_lc_rs::rand::fill(&mut key_bytes)?;
    Ok(CryptoKey {
        kind: CryptoKeyKind::Hmac {
            algorithm: HmacKeyAlgorithm {
                hash: algorithm.hash,
                length: length as u32,
            },
            key: HmacKey {
                key: hmac::Key::new(hmac_algorithm(algorithm.hash), &key_bytes),
                data: key_bytes,
            },
        },
        r#type: KeyType::Secret,
        extractable,
        usages,
    })
}

pub fn import_key(
    input: ImportKeyInput,
    algorithm: HmacImportParams,
    extractable: bool,
    usages: IndexSet<KeyUsage>,
) -> Result<CryptoKey> {
    check_usages_subset(&usages, &[KeyUsage::Sign, KeyUsage::Verify])?;
    let data = match input {
        ImportKeyInput::Raw(data) => data,
        ImportKeyInput::Jwk(jwk) => {
            jwk.check_kty("oct")?;
            let data = jwk
                .k
                .as_ref()
                .and_then(|k| base64::decode_config(k, URL_SAFE_FORGIVING).ok())
                .ok_or_else(|| Error::dom("invalid key data", DOMExceptionName::DataError))?;
            jwk.check_alg(jwk_alg(&algorithm.hash))?;
            jwk.check_key_ops_and_use(&usages, "sig")?;
            jwk.check_ext(extractable)?;
            data
        },
        ImportKeyInput::Pkcs8(_) | ImportKeyInput::Spki(_) => {
            return Err(Error::dom(
                "unsupported import format",
                DOMExceptionName::NotSupportedError,
            ))
        },
    };
    let mut length = data.len() * 8;
    ensure!(
        length > 0,
        Error::dom("provided HMAC key is empty", DOMExceptionName::DataError)
    );
    if algorithm.length.is_some() {
        let requested_len = algorithm
            .key_length()?
            .ok_or_else(|| Error::dom("length must not be zero", DOMExceptionName::DataError))?
            .get();
        ensure!(
            requested_len <= length,
            Error::dom(
                "provided HMAC key is shorter than requested length",
                DOMExceptionName::DataError
            )
        );
        ensure!(
            requested_len > length - 8,
            Error::dom(
                "provided HMAC key is longer than requested length",
                DOMExceptionName::DataError
            )
        );
        length = requested_len;
    }
    Ok(CryptoKey {
        kind: CryptoKeyKind::Hmac {
            algorithm: HmacKeyAlgorithm {
                hash: algorithm.hash,
                length: length as u32,
            },
            key: HmacKey {
                key: hmac::Key::new(hmac_algorithm(algorithm.hash), &data),
                data,
            },
        },
        r#type: KeyType::Secret,
        extractable,
        usages,
    })
}

impl HmacKey {
    pub fn export_key(&self, algorithm: &HmacKeyAlgorithm, format: KeyFormat) -> Result<KeyData> {
        match format {
            KeyFormat::Raw => Ok(KeyData::Raw(self.data.clone())),
            KeyFormat::Jwk => {
                let jwk = JsonWebKey {
                    kty: Some("oct".to_owned()),
                    k: Some(base64::encode_config(&self.data, base64::URL_SAFE_NO_PAD)),
                    alg: Some(jwk_alg(&algorithm.hash).into()),
                    ..Default::default()
                };
                Ok(KeyData::Jwk(jwk))
            },
            KeyFormat::Pkcs8 | KeyFormat::Spki => Err(Error::dom(
                "unsupported export format",
                DOMExceptionName::NotSupportedError,
            )),
        }
    }
}

impl HmacImportParams {
    /// The spec's "get key length" operation, which reports a zero `length` as
    /// a `TypeError`.
    pub fn get_key_length(&self) -> Result<NonZeroUsize> {
        self.key_length()?
            .ok_or_else(|| Error::type_error("length must not be zero"))
    }

    fn key_length(&self) -> Result<Option<NonZeroUsize>> {
        let Some(length) = self.length else {
            return Ok(Some(self.hash.block_size_bits()));
        };
        let Some(length) = NonZeroUsize::new(length as usize) else {
            return Ok(None);
        };
        // The spec allows any bit length, but node.js, Deno, Bun, Firefox, and Safari
        // don't implement fractional byte lengths; only Chrome does.
        // Node raises a TypeError.
        ensure!(
            length.get() % 8 == 0,
            Error::type_error("length must be a multiple of 8")
        );
        Ok(Some(length))
    }
}

fn hmac_algorithm(hash: CryptoHash) -> hmac::Algorithm {
    match hash {
        CryptoHash::Sha1 => hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
        CryptoHash::Sha256 => hmac::HMAC_SHA256,
        CryptoHash::Sha384 => hmac::HMAC_SHA384,
        CryptoHash::Sha512 => hmac::HMAC_SHA512,
    }
}

impl HmacKey {
    pub fn sign(&self, _algorithm: &HmacKeyAlgorithm, data: &[u8]) -> Vec<u8> {
        hmac::sign(&self.key, data).as_ref().to_vec()
    }

    pub fn verify(&self, _algorithm: &HmacKeyAlgorithm, data: &[u8], signature: &[u8]) -> bool {
        hmac::verify(&self.key, data, signature).is_ok()
    }
}
