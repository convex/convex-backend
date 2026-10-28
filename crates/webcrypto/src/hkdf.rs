use aws_lc_rs::hkdf;
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
};

pub struct HkdfKey {
    secret: Vec<u8>,
}
#[derive(Deserialize, Debug)]
pub struct HkdfParams {
    #[serde(with = "super::nullary_algorithm")]
    pub hash: CryptoHash,
    pub salt: serde_bytes::ByteBuf,
    pub info: serde_bytes::ByteBuf,
}

#[derive(Serialize, Debug)]
#[serde(rename = "HKDF")]
#[serde(tag = "name")]
pub struct HkdfAlgorithm {}

struct OutputLength(usize);

impl hkdf::KeyType for OutputLength {
    fn len(&self) -> usize {
        self.0
    }
}

pub fn derive_bits(
    algorithm: HkdfParams,
    key: &CryptoKey,
    length: Option<usize>,
) -> Result<Vec<u8>> {
    let CryptoKeyKind::Hkdf { key, .. } = &key.kind else {
        return Err(Error::dom(
            "Key algorithm mismatch",
            DOMExceptionName::InvalidAccessError,
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
    let HkdfParams { hash, salt, info } = algorithm;
    let algorithm = match hash {
        CryptoHash::Sha1 => hkdf::HKDF_SHA1_FOR_LEGACY_USE_ONLY,
        CryptoHash::Sha256 => hkdf::HKDF_SHA256,
        CryptoHash::Sha384 => hkdf::HKDF_SHA384,
        CryptoHash::Sha512 => hkdf::HKDF_SHA512,
    };
    let output_length = OutputLength(length / 8);
    let salt = hkdf::Salt::new(algorithm, &salt);
    let prk = salt.extract(&key.secret);
    let info = [info.as_ref()];
    let okm = prk.expand(&info, output_length).map_err(|_| {
        Error::dom(
            "requested length exceeds the maximum for HKDF",
            DOMExceptionName::OperationError,
        )
    })?;
    let mut out = vec![0; length / 8];
    okm.fill(&mut out)
        .map_err(|_| Error::dom("HKDF derivation failed", DOMExceptionName::OperationError))?;
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
            "HKDF keys cannot be extractable",
            DOMExceptionName::SyntaxError
        )
    );
    Ok(CryptoKey {
        kind: CryptoKeyKind::Hkdf {
            algorithm: HkdfAlgorithm {},
            key: HkdfKey { secret },
        },
        r#type: KeyType::Secret,
        extractable,
        usages,
    })
}
