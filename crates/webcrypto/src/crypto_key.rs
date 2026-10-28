use indexmap::IndexSet;

use crate::{
    aes,
    ec,
    ed25519,
    hkdf,
    hmac,
    pbkdf2,
    rsa,
    x25519,
    DOMExceptionName,
    Error,
    KeyType,
    KeyUsage,
    Result,
};

pub enum CryptoKeyKind {
    Pbkdf2 {
        algorithm: pbkdf2::Pbkdf2Algorithm,
        key: pbkdf2::Pbkdf2Key,
    },
    Hkdf {
        algorithm: hkdf::HkdfAlgorithm,
        key: hkdf::HkdfKey,
    },
    Hmac {
        algorithm: hmac::HmacKeyAlgorithm,
        key: hmac::HmacKey,
    },
    Aes {
        algorithm: aes::AesKeyAlgorithm,
        key: aes::AesKey,
    },
    RsaPrivate {
        algorithm: rsa::RsaHashedKeyAlgorithm,
        key: rsa::RsaPrivateKey,
    },
    RsaPublic {
        algorithm: rsa::RsaHashedKeyAlgorithm,
        key: rsa::RsaPublicKey,
    },
    EcPrivate {
        algorithm: ec::EcKeyAlgorithm,
        key: ec::EcPrivateKey,
    },
    EcPublic {
        algorithm: ec::EcKeyAlgorithm,
        key: ec::EcPublicKey,
    },
    Ed25519Private {
        algorithm: ed25519::Ed25519Algorithm,
        key: ed25519::Ed25519PrivateKey,
    },
    Ed25519Public {
        algorithm: ed25519::Ed25519Algorithm,
        key: ed25519::Ed25519PublicKey,
    },
    X25519Private {
        algorithm: x25519::X25519Algorithm,
        key: x25519::X25519PrivateKey,
    },
    X25519Public {
        algorithm: x25519::X25519Algorithm,
        key: x25519::X25519PublicKey,
    },
}

pub struct CryptoKey {
    pub kind: CryptoKeyKind,
    pub r#type: KeyType,
    pub extractable: bool,
    pub usages: IndexSet<KeyUsage>,
}

impl CryptoKey {
    pub fn check_usage(&self, usage: KeyUsage) -> Result<()> {
        if !self.usages.contains(&usage) {
            return Err(Error::dom(
                format!("CryptoKey does not have {:?} usage", usage.to_string()),
                DOMExceptionName::InvalidAccessError,
            ));
        }
        Ok(())
    }

    /// If the [[type]] internal slot of result is "secret" or "private" and
    /// usages is empty, then throw a SyntaxError.
    pub fn check_useless(&self) -> Result<()> {
        if [KeyType::Secret, KeyType::Private].contains(&self.r#type) && self.usages.is_empty() {
            return Err(Error::dom(
                "invalid key usages",
                DOMExceptionName::SyntaxError,
            ));
        }
        Ok(())
    }
}

impl AsRef<CryptoKey> for CryptoKey {
    fn as_ref(&self) -> &CryptoKey {
        self
    }
}

pub struct CryptoKeyPair {
    pub private_key: CryptoKey,
    pub public_key: CryptoKey,
}

pub enum CryptoKeyOrPair {
    Symmetric(CryptoKey),
    Asymmetric(CryptoKeyPair),
}

impl From<CryptoKeyPair> for CryptoKeyOrPair {
    fn from(v: CryptoKeyPair) -> Self {
        Self::Asymmetric(v)
    }
}

impl From<CryptoKey> for CryptoKeyOrPair {
    fn from(v: CryptoKey) -> Self {
        Self::Symmetric(v)
    }
}
