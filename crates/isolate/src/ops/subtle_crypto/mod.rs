//! V8 glue for the WebCrypto `SubtleCrypto` API: parsing of polymorphic
//! algorithm arguments, the CryptoKey object store, and the ops themselves.
//! The actual cryptographic implementation lives in the `webcrypto` crate.

mod crypto_key;

use std::{
    rc::Rc,
    str::FromStr,
};

use deno_core::v8;
use indexmap::IndexSet;
use serde_bytes::ByteBuf;
use webcrypto::{
    aes,
    ec,
    hkdf,
    hmac,
    pbkdf2,
    rsa,
    CryptoHash,
    CryptoKey,
    DerivedKeyAlgorithm,
    EncryptDecryptAlgorithm,
    ImportKeyAlgorithm,
    JsonWebKey,
    KeyData,
    KeyDeriveParams,
    KeyFormat,
    KeyGenParams,
    KeyUsage,
    SignVerifyAlgorithm,
    WrapKeyAlgorithm,
};

use self::crypto_key::{
    JsCryptoKey,
    JsCryptoKeyOrPair,
};
use crate::{
    convert_v8::{
        ArrayBuffer,
        DOMException,
        DOMExceptionName,
        FromV8,
        JsException,
        ToV8,
        TypeError,
    },
    environment::UncatchableDeveloperError,
    ops::V8OpProvider,
    strings,
};

const USE_NODE_SUGGESTION: &str = "Consider calling an action defined in Node.js instead (https://docs.convex.dev/functions/actions).";

fn get_name<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    input: v8::Local<'s, v8::Value>,
) -> anyhow::Result<String> {
    let v8_string = if let Ok(s) = input.try_cast::<v8::String>() {
        s
    } else if let Ok(object) = input.try_cast::<v8::Object>() {
        let name_str = strings::name.create(scope)?;
        let name = object
            .get(scope, name_str.into())
            .filter(|name| {
                // WebIDL reads an `undefined` dictionary member as absent
                !name.is_undefined()
            })
            .ok_or_else(|| TypeError::new("'name' missing in algorithm"))?;
        name.to_string(scope)
            .ok_or_else(|| anyhow::anyhow!("[TODO: propagate exception]"))?
    } else {
        anyhow::bail!(DOMException::new(
            "Unrecognized or invalid algorithm",
            DOMExceptionName::NotSupportedError
        ))
    };
    let mut string = v8_string.to_rust_string_lossy(scope);
    string.make_ascii_lowercase();
    Ok(string)
}

// newtypes needed to avoid coherence issues
pub(super) struct JsImportKeyAlgorithm;

impl FromV8 for JsImportKeyAlgorithm {
    type Output = ImportKeyAlgorithm;

    fn from_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        input: v8::Local<'s, v8::Value>,
    ) -> anyhow::Result<Self::Output> {
        match get_name(scope, input)?.as_str() {
            "rsassa-pkcs1-v1_5" | "rsa-pss" | "rsa-oaep" => Ok(ImportKeyAlgorithm::Rsa(
                rsa::RsaHashedImportParams::from_v8(scope, input)?,
            )),
            "ecdsa" | "ecdh" => Ok(ImportKeyAlgorithm::Ec(ec::EcKeyAlgorithm::from_v8(
                scope, input,
            )?)),
            "hmac" => Ok(ImportKeyAlgorithm::Hmac(hmac::HmacImportParams::from_v8(
                scope, input,
            )?)),
            "aes-cbc" => Ok(ImportKeyAlgorithm::Aes(aes::AesAlgorithm::AesCbc)),
            "aes-ctr" => Ok(ImportKeyAlgorithm::Aes(aes::AesAlgorithm::AesCtr)),
            "aes-gcm" => Ok(ImportKeyAlgorithm::Aes(aes::AesAlgorithm::AesGcm)),
            "aes-kw" => Ok(ImportKeyAlgorithm::AesKw),
            "pbkdf2" => Ok(ImportKeyAlgorithm::Pbkdf2),
            "hkdf" => Ok(ImportKeyAlgorithm::Hkdf),
            "ed25519" => Ok(ImportKeyAlgorithm::Ed25519),
            "x25519" => Ok(ImportKeyAlgorithm::X25519),
            name => anyhow::bail!(DOMException::new(
                format!("Unrecognized or invalid algorithm {name}"),
                DOMExceptionName::NotSupportedError
            )),
        }
    }
}

pub(super) struct JsKeyData(pub KeyData);

// Accepts either raw bytes or an object
impl FromV8 for JsKeyData {
    type Output = KeyData;

    fn from_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        input: v8::Local<'s, v8::Value>,
    ) -> anyhow::Result<Self::Output> {
        // TODO: does data view actually work?
        if input.is_array_buffer() || input.is_typed_array() || input.is_data_view() {
            <serde_bytes::ByteBuf>::from_v8(scope, input).map(|x| KeyData::Raw(x.into_vec()))
        } else {
            JsonWebKey::from_v8(scope, input).map(KeyData::Jwk)
        }
    }
}
impl ToV8 for JsKeyData {
    fn to_v8<'s>(
        self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> anyhow::Result<v8::Local<'s, v8::Value>> {
        match self.0 {
            KeyData::Raw(bytes) => ArrayBuffer(bytes).to_v8(scope),
            KeyData::Jwk(jwk) => jwk.to_v8(scope),
        }
    }
}

/// Used by both ECDH and X25519
fn key_agreement_params_from_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    input: v8::Local<'s, v8::Value>,
) -> anyhow::Result<ec::EcdhKeyDeriveParams<Rc<CryptoKey>>> {
    let Ok(object) = input.try_cast::<v8::Object>() else {
        anyhow::bail!(TypeError::new("algorithm requires 'public' parameter"));
    };
    let public_str = strings::public.create(scope)?;
    let public_key_value = object
        .get(scope, public_str.into())
        .ok_or_else(|| anyhow::anyhow!(TypeError::new("algorithm requires 'public' parameter")))?;
    let public_key = JsCryptoKey::from_v8(scope, public_key_value)?;
    Ok(ec::EcdhKeyDeriveParams { public_key })
}

pub(super) struct JsKeyDeriveParams;

impl FromV8 for JsKeyDeriveParams {
    type Output = KeyDeriveParams<Rc<CryptoKey>>;

    fn from_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        input: v8::Local<'s, v8::Value>,
    ) -> anyhow::Result<Self::Output> {
        match get_name(scope, input)?.as_str() {
            "pbkdf2" => Ok(KeyDeriveParams::Pbkdf2(pbkdf2::Pbkdf2Params::from_v8(
                scope, input,
            )?)),
            "ecdh" => Ok(KeyDeriveParams::Ecdh(key_agreement_params_from_v8(
                scope, input,
            )?)),
            "hkdf" => Ok(KeyDeriveParams::Hkdf(hkdf::HkdfParams::from_v8(
                scope, input,
            )?)),
            "x25519" => Ok(KeyDeriveParams::X25519(key_agreement_params_from_v8(
                scope, input,
            )?)),
            name => anyhow::bail!(DOMException::new(
                format!("Unrecognized or invalid algorithm {name}"),
                DOMExceptionName::NotSupportedError
            )),
        }
    }
}

fn unimplemented(operation: &'static str, algorithm: &'static str) -> anyhow::Error {
    UncatchableDeveloperError {
        message: format!(
            "Not implemented: crypto.subtle.{operation} for {algorithm}. {USE_NODE_SUGGESTION}"
        ),
    }
    .into()
}

fn flatten_error(error: webcrypto::Error) -> anyhow::Error {
    match error {
        webcrypto::Error::Dom { name, message } => JsException::DOMException(DOMException {
            name: name.into(),
            message,
        })
        .into(),
        webcrypto::Error::Type(message) => JsException::TypeError(TypeError { message }).into(),
        webcrypto::Error::NotImplemented {
            operation,
            algorithm,
        } => unimplemented(operation, algorithm),
        webcrypto::Error::Other(e) => e,
    }
}

#[convex_macro::v8_op]
pub(crate) fn op_crypto_subtle_import_key<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    format: KeyFormat,
    key_data: JsKeyData,
    algorithm: JsImportKeyAlgorithm,
    extractable: bool,
    key_usages: IndexSet<KeyUsage>,
) -> anyhow::Result<JsCryptoKey> {
    webcrypto::import_key(format, key_data, algorithm, extractable, key_usages)
        .map_err(flatten_error)
        .map(JsCryptoKey)
}

#[convex_macro::v8_op]
pub(crate) fn op_crypto_subtle_derive_bits<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    algorithm: JsKeyDeriveParams,
    key: JsCryptoKey,
    length: Option<usize>,
) -> anyhow::Result<ArrayBuffer> {
    webcrypto::derive_bits(algorithm, &key, length)
        .map_err(flatten_error)
        .map(ArrayBuffer)
}

pub(super) struct JsDerivedKeyAlgorithm;

impl FromV8 for JsDerivedKeyAlgorithm {
    type Output = DerivedKeyAlgorithm;

    fn from_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        input: v8::Local<'s, v8::Value>,
    ) -> anyhow::Result<Self::Output> {
        match get_name(scope, input)?.as_str() {
            "hmac" => Ok(DerivedKeyAlgorithm::Hmac(hmac::HmacImportParams::from_v8(
                scope, input,
            )?)),
            "aes-ctr" | "aes-cbc" | "aes-gcm" => Ok(DerivedKeyAlgorithm::Aes(
                aes::AesKeyGenParams::from_v8(scope, input)?,
            )),
            "aes-kw" => Ok(DerivedKeyAlgorithm::AesKw),
            name => anyhow::bail!(DOMException::new(
                format!("Unrecognized or invalid algorithm {name}"),
                DOMExceptionName::NotSupportedError
            )),
        }
    }
}

#[convex_macro::v8_op]
pub(crate) fn op_crypto_subtle_derive_key<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    algorithm: JsKeyDeriveParams,
    base_key: JsCryptoKey,
    derived_key_type: JsDerivedKeyAlgorithm,
    extractable: bool,
    key_usages: IndexSet<KeyUsage>,
) -> anyhow::Result<JsCryptoKey> {
    webcrypto::derive_key(
        algorithm,
        &base_key,
        derived_key_type,
        extractable,
        key_usages,
    )
    .map_err(flatten_error)
    .map(JsCryptoKey)
}

pub(super) struct JsKeyGenParams;

impl FromV8 for JsKeyGenParams {
    type Output = KeyGenParams;

    fn from_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        input: v8::Local<'s, v8::Value>,
    ) -> anyhow::Result<Self::Output> {
        match get_name(scope, input)?.as_str() {
            "rsassa-pkcs1-v1_5" | "rsa-pss" | "rsa-oaep" => Ok(KeyGenParams::Rsa(
                rsa::RsaHashedKeyGenParams::from_v8(scope, input)?,
            )),
            "ecdsa" | "ecdh" => Ok(KeyGenParams::Ec(ec::EcKeyGenParams::from_v8(scope, input)?)),
            "hmac" => Ok(KeyGenParams::Hmac(hmac::HmacKeyGenParams::from_v8(
                scope, input,
            )?)),
            "aes-ctr" | "aes-cbc" | "aes-gcm" => Ok(KeyGenParams::Aes(
                aes::AesKeyGenParams::from_v8(scope, input)?,
            )),
            "aes-kw" => Ok(KeyGenParams::AesKw),
            "ed25519" => Ok(KeyGenParams::Ed25519),
            "x25519" => Ok(KeyGenParams::X25519),
            name => anyhow::bail!(DOMException::new(
                format!("Unrecognized or invalid algorithm {name}"),
                DOMExceptionName::NotSupportedError
            )),
        }
    }
}

#[convex_macro::v8_op]
pub(crate) fn op_crypto_subtle_generate_key<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    algorithm: JsKeyGenParams,
    extractable: bool,
    key_usages: IndexSet<KeyUsage>,
) -> anyhow::Result<JsCryptoKeyOrPair> {
    let rng = provider.crypto_rng()?;
    webcrypto::generate_key(algorithm, &rng, extractable, key_usages)
        .map_err(flatten_error)
        .map(JsCryptoKeyOrPair)
}

#[convex_macro::v8_op]
pub(crate) fn op_crypto_subtle_export_key<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    format: KeyFormat,
    key: JsCryptoKey,
) -> anyhow::Result<JsKeyData> {
    webcrypto::export_key(format, &key)
        .map_err(flatten_error)
        .map(JsKeyData)
}

pub(super) struct JsEncryptDecryptAlgorithm;

impl FromV8 for JsEncryptDecryptAlgorithm {
    type Output = EncryptDecryptAlgorithm;

    fn from_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        input: v8::Local<'s, v8::Value>,
    ) -> anyhow::Result<Self::Output> {
        match get_name(scope, input)?.as_str() {
            "rsa-oaep" => Ok(EncryptDecryptAlgorithm::RsaOaep(
                rsa::RsaOaepParams::from_v8(scope, input)?,
            )),
            "aes-ctr" => Ok(EncryptDecryptAlgorithm::AesCtr(aes::AesCtrParams::from_v8(
                scope, input,
            )?)),
            "aes-cbc" => Ok(EncryptDecryptAlgorithm::AesCbc(aes::AesCbcParams::from_v8(
                scope, input,
            )?)),
            "aes-gcm" => Ok(EncryptDecryptAlgorithm::AesGcm(aes::AesGcmParams::from_v8(
                scope, input,
            )?)),
            _ => anyhow::bail!(DOMException::new(
                "invalid algorithm for key",
                DOMExceptionName::InvalidAccessError
            )),
        }
    }
}

#[convex_macro::v8_op]
pub(crate) fn op_crypto_subtle_decrypt<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    algorithm: JsEncryptDecryptAlgorithm,
    key: JsCryptoKey,
    data: ByteBuf,
) -> anyhow::Result<ArrayBuffer> {
    webcrypto::decrypt(algorithm, &key, data.into_vec())
        .map_err(flatten_error)
        .map(ArrayBuffer)
}

#[convex_macro::v8_op]
pub(crate) fn op_crypto_subtle_encrypt<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    algorithm: JsEncryptDecryptAlgorithm,
    key: JsCryptoKey,
    data: ByteBuf,
) -> anyhow::Result<ArrayBuffer> {
    let result = webcrypto::encrypt(
        algorithm,
        &key,
        || Ok(provider.crypto_rng()?),
        data.into_vec(),
    );
    result.map_err(flatten_error).map(ArrayBuffer)
}

pub(super) struct JsDigestAlgorithm;

impl FromV8 for JsDigestAlgorithm {
    type Output = CryptoHash;

    fn from_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        input: v8::Local<'s, v8::Value>,
    ) -> anyhow::Result<Self::Output> {
        let name = get_name(scope, input)?;
        let Ok(hash) = CryptoHash::from_str(&name) else {
            anyhow::bail!(DOMException::new(
                format!("Unrecognized or invalid algorithm {name}"),
                DOMExceptionName::NotSupportedError
            ));
        };
        Ok(hash)
    }
}

#[convex_macro::v8_op]
pub(crate) fn op_crypto_subtle_digest<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    algorithm: JsDigestAlgorithm,
    data: ByteBuf,
) -> anyhow::Result<ArrayBuffer> {
    webcrypto::digest(algorithm, &data)
        .map_err(flatten_error)
        .map(ArrayBuffer)
}

pub(super) struct JsSignVerifyAlgorithm;

impl FromV8 for JsSignVerifyAlgorithm {
    type Output = SignVerifyAlgorithm;

    fn from_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        input: v8::Local<'s, v8::Value>,
    ) -> anyhow::Result<Self::Output> {
        match get_name(scope, input)?.as_str() {
            "rsassa-pkcs1-v1_5" => Ok(SignVerifyAlgorithm::Rsa(rsa::RsaParams::RsaSsaPkcs1v15)),
            "rsa-pss" => Ok(SignVerifyAlgorithm::Rsa(rsa::RsaParams::RsaPss(
                rsa::RsaPssParams::from_v8(scope, input)?,
            ))),
            "ecdsa" => Ok(SignVerifyAlgorithm::Ecdsa(ec::EcdsaParams::from_v8(
                scope, input,
            )?)),
            "hmac" => Ok(SignVerifyAlgorithm::Hmac),
            "ed25519" => Ok(SignVerifyAlgorithm::Ed25519),
            _ => anyhow::bail!(DOMException::new(
                "invalid algorithm for key",
                DOMExceptionName::InvalidAccessError
            )),
        }
    }
}

#[convex_macro::v8_op]
pub(crate) fn op_crypto_subtle_sign<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    algorithm: JsSignVerifyAlgorithm,
    key: JsCryptoKey,
    data: ByteBuf,
) -> anyhow::Result<ArrayBuffer> {
    let result = webcrypto::sign(algorithm, &key, || Ok(provider.crypto_rng()?), &data);
    result.map_err(flatten_error).map(ArrayBuffer)
}

#[convex_macro::v8_op]
pub(crate) fn op_crypto_subtle_verify<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    algorithm: JsSignVerifyAlgorithm,
    key: JsCryptoKey,
    signature: ByteBuf,
    data: ByteBuf,
) -> anyhow::Result<bool> {
    webcrypto::verify(algorithm, &key, &signature, &data).map_err(flatten_error)
}

/// Note: this op is never called, JS raises an error directly
#[convex_macro::v8_op]
pub(crate) fn op_crypto_subtle_wrap_key<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    _format: KeyFormat,
    _key: JsCryptoKey,
    _wrapping_key: JsCryptoKey,
    _wrapping_algorithm: WrapKeyAlgorithm,
) -> anyhow::Result<ByteBuf> {
    anyhow::bail!(DOMException::new(
        "wrapKey not implemented",
        DOMExceptionName::NotSupportedError
    ));
}

/// Note: this op is never called, JS raises an error directly
#[convex_macro::v8_op]
pub(crate) fn op_crypto_subtle_unwrap_key<'b, P: V8OpProvider<'b>>(
    provider: &mut P,
    _format: KeyFormat,
    _wrapped_key: ByteBuf,
    _unwrapping_key: JsCryptoKey,
    _wrapping_algorithm: WrapKeyAlgorithm,
    _unwrapped_key_algorithm: JsImportKeyAlgorithm, // should be something else
) -> anyhow::Result<JsCryptoKey> {
    anyhow::bail!(DOMException::new(
        "unwrapKey not implemented",
        DOMExceptionName::NotSupportedError
    ));
}
