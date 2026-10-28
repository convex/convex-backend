//! An implementation of the WebCrypto `SubtleCrypto` API.
//!
//! Algorithm parameters are plain serde-deserializable types, so embedders can
//! construct them from whatever JS engine (or other source) they like.

use std::{
    num::NonZeroUsize,
    str::FromStr,
};

use aws_lc_rs::digest;
use indexmap::IndexSet;
use serde::{
    Deserialize,
    Serialize,
};
use strum::EnumString;

mod crypto_key;
mod crypto_rng;
mod errors;
mod jwk;
mod serde_helpers;

pub mod aes;
pub mod ec;
pub mod ed25519;
pub mod hkdf;
pub mod hmac;
pub mod pbkdf2;
pub mod rsa;
pub mod x25519;

pub use crate::{
    crypto_key::{
        CryptoKey,
        CryptoKeyKind,
        CryptoKeyOrPair,
        CryptoKeyPair,
    },
    crypto_rng::CryptoRng,
    errors::{
        DOMExceptionName,
        Error,
        Result,
    },
    jwk::JsonWebKey,
    serde_helpers::nullary_algorithm,
};
pub(crate) use crate::{
    errors::ensure,
    serde_helpers::algorithm_name,
};

const DERIVE_BITS_MAX: usize = 8 * (1 << 16); // 64KiB in bits
const URL_SAFE_FORGIVING: base64::Config = base64::URL_SAFE_NO_PAD.decode_allow_trailing_bits(true);

#[derive(Deserialize, Serialize, Copy, Clone, Eq, PartialEq, Debug, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum CryptoHash {
    #[serde(rename = "SHA-1")]
    #[strum(serialize = "SHA-1")]
    Sha1,
    #[serde(rename = "SHA-256")]
    #[strum(serialize = "SHA-256")]
    Sha256,
    #[serde(rename = "SHA-384")]
    #[strum(serialize = "SHA-384")]
    Sha384,
    #[serde(rename = "SHA-512")]
    #[strum(serialize = "SHA-512")]
    Sha512,
}
impl CryptoHash {
    fn block_size_bits(&self) -> NonZeroUsize {
        match self {
            CryptoHash::Sha1 => const { NonZeroUsize::new(512).unwrap() },
            CryptoHash::Sha256 => const { NonZeroUsize::new(512).unwrap() },
            CryptoHash::Sha384 => const { NonZeroUsize::new(1024).unwrap() },
            CryptoHash::Sha512 => const { NonZeroUsize::new(1024).unwrap() },
        }
    }

    fn openssl_message_digest(&self) -> openssl_aws_lc::hash::MessageDigest {
        match self {
            CryptoHash::Sha1 => openssl_aws_lc::hash::MessageDigest::sha1(),
            CryptoHash::Sha256 => openssl_aws_lc::hash::MessageDigest::sha256(),
            CryptoHash::Sha384 => openssl_aws_lc::hash::MessageDigest::sha384(),
            CryptoHash::Sha512 => openssl_aws_lc::hash::MessageDigest::sha512(),
        }
    }
}

#[derive(Deserialize, Serialize, Copy, Clone, Eq, PartialEq, Debug, Hash)]
#[serde(rename_all = "camelCase")]
pub enum KeyUsage {
    Encrypt,
    Decrypt,
    Sign,
    Verify,
    DeriveKey,
    DeriveBits,
    WrapKey,
    UnwrapKey,
}

impl FromStr for KeyUsage {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        serde_json::from_value(serde_json::Value::String(s.to_owned())).map_err(|_| ())
    }
}

impl KeyUsage {
    #[allow(clippy::inherent_to_string)]
    pub(crate) fn to_string(self) -> String {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::String(s)) => s,
            _ => unreachable!(),
        }
    }
}

#[derive(Deserialize, Serialize, Copy, Clone, Eq, PartialEq, Debug, Hash)]
#[serde(rename_all = "camelCase")]
pub enum KeyType {
    Secret,
    Private,
    Public,
}

#[derive(Deserialize, Debug, Copy, Clone)]
pub enum KeyFormat {
    #[serde(rename = "raw")]
    Raw,
    #[serde(rename = "pkcs8")]
    Pkcs8,
    #[serde(rename = "spki")]
    Spki,
    #[serde(rename = "jwk")]
    Jwk,
}

#[derive(Debug)]
pub enum ImportKeyAlgorithm {
    Rsa(rsa::RsaHashedImportParams),
    Ec(ec::EcKeyImportParams),
    Hmac(hmac::HmacImportParams),
    Aes(aes::AesAlgorithm),
    AesKw,
    Pbkdf2,
    Hkdf,
    Ed25519,
    X25519,
}

pub enum KeyData {
    Raw(Vec<u8>),
    Jwk(JsonWebKey),
}

pub enum ImportKeyInput {
    Raw(Vec<u8>),
    Pkcs8(Vec<u8>),
    Spki(Vec<u8>),
    Jwk(JsonWebKey),
}

pub fn import_key(
    format: KeyFormat,
    key_data: KeyData,
    algorithm: ImportKeyAlgorithm,
    extractable: bool,
    key_usages: IndexSet<KeyUsage>,
) -> Result<CryptoKey> {
    let input = match (format, key_data) {
        (KeyFormat::Raw, KeyData::Raw(data)) => ImportKeyInput::Raw(data),
        (KeyFormat::Pkcs8, KeyData::Raw(data)) => ImportKeyInput::Pkcs8(data),
        (KeyFormat::Spki, KeyData::Raw(data)) => ImportKeyInput::Spki(data),
        (KeyFormat::Jwk, KeyData::Jwk(jwk)) => ImportKeyInput::Jwk(jwk),
        (format, _) => {
            return Err(Error::type_error(format!(
                "wrong keyData for format {format:?}"
            )))
        },
    };
    let key = match algorithm {
        ImportKeyAlgorithm::Rsa(algorithm) => {
            rsa::import_key(input, algorithm, extractable, key_usages)?
        },
        ImportKeyAlgorithm::Ec(algorithm) => {
            ec::import_key(input, algorithm, extractable, key_usages)?
        },
        ImportKeyAlgorithm::Hmac(algorithm) => {
            hmac::import_key(input, algorithm, extractable, key_usages)?
        },
        ImportKeyAlgorithm::Aes(algorithm) => {
            aes::import_key(input, algorithm, extractable, key_usages)?
        },
        ImportKeyAlgorithm::AesKw => {
            return Err(Error::NotImplemented {
                operation: "importKey",
                algorithm: "AES-KW",
            })
        },
        ImportKeyAlgorithm::Pbkdf2 => pbkdf2::import_key(input, extractable, key_usages)?,
        ImportKeyAlgorithm::Hkdf => hkdf::import_key(input, extractable, key_usages)?,
        ImportKeyAlgorithm::Ed25519 => ed25519::import_key(input, extractable, key_usages)?,
        ImportKeyAlgorithm::X25519 => x25519::import_key(input, extractable, key_usages)?,
    };
    key.check_useless()?;
    Ok(key)
}

pub enum KeyDeriveParams<K> {
    Pbkdf2(pbkdf2::Pbkdf2Params),
    Ecdh(ec::EcdhKeyDeriveParams<K>),
    Hkdf(hkdf::HkdfParams),
    /// X25519 key agreement takes the same dictionary as ECDH.
    X25519(ec::EcdhKeyDeriveParams<K>),
}

/// deriveBits step 6 and deriveKey step 10: an algorithm that is not the one
/// the base key was made for is reported before the key's usages are checked.
fn check_derive_algorithm_matches_key(
    algorithm: &KeyDeriveParams<impl AsRef<CryptoKey>>,
    key: &CryptoKey,
) -> Result<()> {
    let matches = match algorithm {
        KeyDeriveParams::Pbkdf2(_) => matches!(key.kind, CryptoKeyKind::Pbkdf2 { .. }),
        KeyDeriveParams::Hkdf(_) => matches!(key.kind, CryptoKeyKind::Hkdf { .. }),
        KeyDeriveParams::Ecdh(_) => matches!(key.kind, CryptoKeyKind::EcPrivate { .. }),
        KeyDeriveParams::X25519(_) => matches!(key.kind, CryptoKeyKind::X25519Private { .. }),
    };
    ensure!(
        matches,
        Error::dom(
            "invalid algorithm for key",
            DOMExceptionName::InvalidAccessError
        )
    );
    Ok(())
}

fn derive_bits_inner(
    algorithm: KeyDeriveParams<impl AsRef<CryptoKey>>,
    key: &CryptoKey,
    length: Option<usize>,
) -> Result<Vec<u8>> {
    match algorithm {
        KeyDeriveParams::Pbkdf2(algorithm) => pbkdf2::derive_bits(algorithm, key, length),
        KeyDeriveParams::Ecdh(params) => ec::derive_bits(params, key, length),
        KeyDeriveParams::Hkdf(params) => hkdf::derive_bits(params, key, length),
        KeyDeriveParams::X25519(params) => x25519::derive_bits(params, key, length),
    }
}

/// The first `length` bits of a key agreement's shared secret, which is what
/// the ECDH and X25519 derive bits operations return.
pub(crate) fn truncate_shared_secret(
    mut secret: Vec<u8>,
    length: Option<usize>,
) -> Result<Vec<u8>> {
    let Some(length) = length else {
        return Ok(secret);
    };
    let secret_bits = secret.len() * 8;
    ensure!(
        length % 8 == 0,
        Error::dom(
            "length must be a multiple of 8",
            DOMExceptionName::OperationError
        )
    );
    ensure!(
        length <= secret_bits,
        Error::dom(
            format!("requested length {length} exceeds shared secret length {secret_bits}"),
            DOMExceptionName::OperationError
        )
    );
    secret.truncate(length / 8);
    Ok(secret)
}

pub fn derive_bits(
    algorithm: KeyDeriveParams<impl AsRef<CryptoKey>>,
    key: &CryptoKey,
    length: Option<usize>,
) -> Result<Vec<u8>> {
    check_derive_algorithm_matches_key(&algorithm, key)?;
    key.check_usage(KeyUsage::DeriveBits)?;
    derive_bits_inner(algorithm, key, length)
}

pub enum DerivedKeyAlgorithm {
    Hmac(hmac::HmacImportParams),
    Aes(aes::AesDerivedKeyParams),
    AesKw,
}

pub fn derive_key(
    algorithm: KeyDeriveParams<impl AsRef<CryptoKey>>,
    base_key: &CryptoKey,
    derived_key_type: DerivedKeyAlgorithm,
    extractable: bool,
    key_usages: IndexSet<KeyUsage>,
) -> Result<CryptoKey> {
    check_derive_algorithm_matches_key(&algorithm, base_key)?;
    base_key.check_usage(KeyUsage::DeriveKey)?;
    let length = match &derived_key_type {
        DerivedKeyAlgorithm::Hmac(alg) => alg.get_key_length()?.get(),
        DerivedKeyAlgorithm::Aes(alg) => alg.get_key_length()?,
        DerivedKeyAlgorithm::AesKw => {
            return Err(Error::NotImplemented {
                operation: "deriveKey",
                algorithm: "AES-KW",
            })
        },
    };
    let key_bits = derive_bits_inner(algorithm, base_key, Some(length))?;
    let key_input = ImportKeyInput::Raw(key_bits);
    let key = match derived_key_type {
        DerivedKeyAlgorithm::Hmac(alg) => {
            hmac::import_key(key_input, alg, extractable, key_usages)?
        },
        DerivedKeyAlgorithm::Aes(alg) => {
            aes::import_key(key_input, alg.name, extractable, key_usages)?
        },
        DerivedKeyAlgorithm::AesKw => unreachable!(),
    };
    key.check_useless()?;
    Ok(key)
}

pub enum KeyGenParams {
    Rsa(rsa::RsaHashedKeyGenParams),
    Ec(ec::EcKeyGenParams),
    Hmac(hmac::HmacKeyGenParams),
    Aes(aes::AesKeyGenParams),
    AesKw,
    Ed25519,
    X25519,
}

pub fn generate_key(
    algorithm: KeyGenParams,
    rng: &CryptoRng,
    extractable: bool,
    key_usages: IndexSet<KeyUsage>,
) -> Result<CryptoKeyOrPair> {
    let result = match algorithm {
        KeyGenParams::Rsa(algorithm) => {
            rsa::generate_keypair(algorithm, rng, extractable, key_usages)?.into()
        },
        KeyGenParams::Ec(algorithm) => {
            ec::generate_keypair(algorithm, rng, extractable, key_usages)?.into()
        },
        KeyGenParams::Hmac(algorithm) => {
            hmac::generate_key(algorithm, rng, extractable, key_usages)?.into()
        },
        KeyGenParams::Aes(algorithm) => {
            aes::generate_key(algorithm, rng, extractable, key_usages)?.into()
        },
        KeyGenParams::AesKw => {
            return Err(Error::NotImplemented {
                operation: "generateKey",
                algorithm: "AES-KW",
            })
        },
        KeyGenParams::Ed25519 => ed25519::generate_keypair(rng, extractable, key_usages)?.into(),
        KeyGenParams::X25519 => x25519::generate_keypair(rng, extractable, key_usages)?.into(),
    };
    match &result {
        CryptoKeyOrPair::Symmetric(key) => key.check_useless()?,
        CryptoKeyOrPair::Asymmetric(keypair) => keypair.private_key.check_useless()?,
    }
    Ok(result)
}

pub fn export_key(format: KeyFormat, key: &CryptoKey) -> Result<KeyData> {
    // An algorithm that registers no export key operation is reported before
    // the key's extractability is looked at.
    let unexportable = match &key.kind {
        CryptoKeyKind::Pbkdf2 { .. } => Some("PBKDF2"),
        CryptoKeyKind::Hkdf { .. } => Some("HKDF"),
        CryptoKeyKind::Hmac { .. }
        | CryptoKeyKind::Aes { .. }
        | CryptoKeyKind::RsaPrivate { .. }
        | CryptoKeyKind::RsaPublic { .. }
        | CryptoKeyKind::EcPrivate { .. }
        | CryptoKeyKind::EcPublic { .. }
        | CryptoKeyKind::Ed25519Private { .. }
        | CryptoKeyKind::Ed25519Public { .. }
        | CryptoKeyKind::X25519Private { .. }
        | CryptoKeyKind::X25519Public { .. } => None,
    };
    if let Some(algorithm) = unexportable {
        return Err(Error::dom(
            format!("{algorithm} keys are not exportable"),
            DOMExceptionName::NotSupportedError,
        ));
    }
    ensure!(
        key.extractable,
        Error::dom(
            "key is not extractable",
            DOMExceptionName::InvalidAccessError
        )
    );
    let mut exported = match &key.kind {
        CryptoKeyKind::Pbkdf2 { .. } | CryptoKeyKind::Hkdf { .. } => {
            unreachable!("rejected above as unexportable")
        },
        CryptoKeyKind::Hmac { algorithm, key } => key.export_key(algorithm, format)?,
        CryptoKeyKind::Aes { algorithm, key } => key.export_key(algorithm, format)?,
        CryptoKeyKind::RsaPrivate { algorithm, key } => key.export_key(algorithm, format)?,
        CryptoKeyKind::RsaPublic { algorithm, key } => key.export_key(algorithm, format)?,
        CryptoKeyKind::EcPrivate { algorithm, key } => key.export_key(algorithm, format)?,
        CryptoKeyKind::EcPublic { algorithm, key } => key.export_key(algorithm, format)?,
        CryptoKeyKind::Ed25519Private { algorithm: _, key } => key.export_key(format)?,
        CryptoKeyKind::Ed25519Public { algorithm: _, key } => key.export_key(format)?,
        CryptoKeyKind::X25519Private { algorithm: _, key } => key.export_key(format)?,
        CryptoKeyKind::X25519Public { algorithm: _, key } => key.export_key(format)?,
    };
    if let KeyData::Jwk(jwk) = &mut exported {
        jwk.key_ops = Some(key.usages.iter().map(|x| x.to_string()).collect());
        jwk.ext = Some(key.extractable);
    }
    Ok(exported)
}

pub enum EncryptDecryptAlgorithm {
    RsaOaep(rsa::RsaOaepParams),
    AesCtr(aes::AesCtrParams),
    AesCbc(aes::AesCbcParams),
    AesGcm(aes::AesGcmParams),
}

pub fn decrypt(
    algorithm: EncryptDecryptAlgorithm,
    key: &CryptoKey,
    data: Vec<u8>,
) -> Result<Vec<u8>> {
    key.check_usage(KeyUsage::Decrypt)?;
    let plaintext = match (algorithm, &key.kind) {
        (
            EncryptDecryptAlgorithm::RsaOaep(params),
            CryptoKeyKind::RsaPrivate { algorithm, key },
        ) => key.decrypt_oaep(params, algorithm, &data)?,
        (EncryptDecryptAlgorithm::AesCtr(params), CryptoKeyKind::Aes { algorithm, key }) => {
            key.crypt_ctr(params, algorithm, data)?
        },
        (EncryptDecryptAlgorithm::AesCbc(params), CryptoKeyKind::Aes { algorithm, key }) => {
            key.decrypt_cbc(params, algorithm, data)?
        },
        (EncryptDecryptAlgorithm::AesGcm(params), CryptoKeyKind::Aes { algorithm, key }) => {
            key.decrypt_gcm(params, algorithm, &data)?
        },
        _ => {
            return Err(Error::dom(
                "invalid algorithm for key",
                DOMExceptionName::InvalidAccessError,
            ))
        },
    };
    Ok(plaintext)
}

pub fn encrypt(
    algorithm: EncryptDecryptAlgorithm,
    key: &CryptoKey,
    rng: impl FnOnce() -> Result<CryptoRng>,
    data: Vec<u8>,
) -> Result<Vec<u8>> {
    key.check_usage(KeyUsage::Encrypt)?;
    let ciphertext = match (algorithm, &key.kind) {
        (EncryptDecryptAlgorithm::RsaOaep(params), CryptoKeyKind::RsaPublic { algorithm, key }) => {
            key.encrypt_oaep(params, algorithm, &rng()?, &data)?
        },
        (EncryptDecryptAlgorithm::AesCtr(params), CryptoKeyKind::Aes { algorithm, key }) => {
            key.crypt_ctr(params, algorithm, data)?
        },
        (EncryptDecryptAlgorithm::AesCbc(params), CryptoKeyKind::Aes { algorithm, key }) => {
            key.encrypt_cbc(params, algorithm, data)?
        },
        (EncryptDecryptAlgorithm::AesGcm(params), CryptoKeyKind::Aes { algorithm, key }) => {
            key.encrypt_gcm(params, algorithm, &data)?
        },
        _ => {
            return Err(Error::dom(
                "invalid algorithm for key",
                DOMExceptionName::InvalidAccessError,
            ))
        },
    };
    Ok(ciphertext)
}

pub fn digest(algorithm: CryptoHash, data: &[u8]) -> Result<Vec<u8>> {
    let algo = match algorithm {
        CryptoHash::Sha1 => &digest::SHA1_FOR_LEGACY_USE_ONLY,
        CryptoHash::Sha256 => &digest::SHA256,
        CryptoHash::Sha384 => &digest::SHA384,
        CryptoHash::Sha512 => &digest::SHA512,
    };
    Ok(digest::digest(algo, data).as_ref().to_vec())
}

pub enum SignVerifyAlgorithm {
    Rsa(rsa::RsaParams),
    Ecdsa(ec::EcdsaParams),
    Hmac,
    Ed25519,
}

pub fn sign(
    algorithm: SignVerifyAlgorithm,
    key: &CryptoKey,
    rng: impl FnOnce() -> Result<CryptoRng>,
    data: &[u8],
) -> Result<Vec<u8>> {
    key.check_usage(KeyUsage::Sign)?;
    let signature = match (algorithm, &key.kind) {
        (SignVerifyAlgorithm::Rsa(params), CryptoKeyKind::RsaPrivate { algorithm, key }) => {
            key.sign(params, algorithm, rng, data)?
        },
        (SignVerifyAlgorithm::Ecdsa(params), CryptoKeyKind::EcPrivate { algorithm, key }) => {
            key.sign(params, algorithm, &rng()?, data)?
        },
        (SignVerifyAlgorithm::Hmac, CryptoKeyKind::Hmac { algorithm, key }) => {
            key.sign(algorithm, data)
        },
        (SignVerifyAlgorithm::Ed25519, CryptoKeyKind::Ed25519Private { algorithm: _, key }) => {
            key.sign(data)
        },
        _ => {
            return Err(Error::dom(
                "invalid algorithm for key",
                DOMExceptionName::InvalidAccessError,
            ))
        },
    };
    Ok(signature)
}

pub fn verify(
    algorithm: SignVerifyAlgorithm,
    key: &CryptoKey,
    signature: &[u8],
    data: &[u8],
) -> Result<bool> {
    key.check_usage(KeyUsage::Verify)?;
    match (algorithm, &key.kind) {
        (SignVerifyAlgorithm::Rsa(params), CryptoKeyKind::RsaPublic { algorithm, key }) => {
            Ok(key.verify(params, algorithm, data, signature)?)
        },
        (SignVerifyAlgorithm::Ecdsa(params), CryptoKeyKind::EcPublic { algorithm, key }) => {
            Ok(key.verify(params, algorithm, data, signature)?)
        },
        (SignVerifyAlgorithm::Hmac, CryptoKeyKind::Hmac { algorithm, key }) => {
            Ok(key.verify(algorithm, data, signature))
        },
        (SignVerifyAlgorithm::Ed25519, CryptoKeyKind::Ed25519Public { algorithm: _, key }) => {
            Ok(key.verify(data, signature))
        },
        _ => Err(Error::dom(
            "invalid algorithm for key",
            DOMExceptionName::InvalidAccessError,
        )),
    }
}

#[derive(Deserialize)]
pub enum WrapKeyAlgorithm {}

fn check_usages_subset(usages: &IndexSet<KeyUsage>, possible_usages: &[KeyUsage]) -> Result<()> {
    ensure!(
        usages.iter().all(|usage| possible_usages.contains(usage)),
        Error::dom("invalid key_usages", DOMExceptionName::SyntaxError)
    );
    Ok(())
}
