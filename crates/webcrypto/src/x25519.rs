use aws_lc_rs::{
    agreement::{
        self,
        X25519,
    },
    encoding::{
        AsBigEndian,
        Curve25519SeedBin,
    },
};
use indexmap::IndexSet;
use serde::Serialize;
use spki::der::{
    asn1::{
        BitStringRef,
        OctetStringRef,
    },
    AnyRef,
    Decode as _,
    Encode,
};

use super::{
    check_usages_subset,
    ec,
    ensure,
    truncate_shared_secret,
    CryptoKey,
    CryptoKeyKind,
    CryptoKeyPair,
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

// id-X25519 OBJECT IDENTIFIER ::= { 1 3 101 110 }
const X25519_OID: const_oid::ObjectIdentifier =
    const_oid::ObjectIdentifier::new_unwrap("1.3.101.110");

#[derive(Serialize)]
#[serde(tag = "name")]
#[serde(rename = "X25519")]
pub struct X25519Algorithm {}

pub struct X25519PrivateKey {
    private_key: agreement::PrivateKey,
}

pub struct X25519PublicKey {
    public_key: agreement::UnparsedPublicKey<[u8; 32]>,
}

pub fn generate_keypair(
    _rng: &CryptoRng,
    extractable: bool,
    usages: IndexSet<KeyUsage>,
) -> Result<CryptoKeyPair> {
    check_usages_subset(&usages, &[KeyUsage::DeriveKey, KeyUsage::DeriveBits])?;
    let private_key = agreement::PrivateKey::generate(&X25519)?;
    let public_key = private_key.compute_public_key()?;
    Ok(CryptoKeyPair {
        private_key: CryptoKey {
            kind: CryptoKeyKind::X25519Private {
                algorithm: X25519Algorithm {},
                key: X25519PrivateKey { private_key },
            },
            r#type: KeyType::Private,
            extractable,
            usages,
        },
        public_key: CryptoKey {
            kind: CryptoKeyKind::X25519Public {
                algorithm: X25519Algorithm {},
                key: X25519PublicKey {
                    public_key: agreement::UnparsedPublicKey::new(
                        &X25519,
                        public_key.as_ref().try_into()?,
                    ),
                },
            },
            r#type: KeyType::Public,
            extractable: true, // N.B.: public key is always extractable
            usages: IndexSet::new(),
        },
    })
}

pub fn import_key(
    format: ImportKeyInput,
    extractable: bool,
    usages: IndexSet<KeyUsage>,
) -> Result<CryptoKey> {
    match format {
        ImportKeyInput::Spki(der) => {
            check_usages_subset(&usages, &[])?;
            let spki = spki::SubjectPublicKeyInfo::<AnyRef, BitStringRef<'_>>::from_der(&der)
                .map_err(|_| {
                    Error::dom(
                        "invalid SubjectPublicKeyInfo document",
                        DOMExceptionName::DataError,
                    )
                })?;
            ensure!(
                spki.algorithm.oid == X25519_OID,
                Error::dom(
                    "SubjectPublicKeyInfo algorithm is not id-X25519",
                    DOMExceptionName::DataError
                )
            );
            ensure!(
                spki.algorithm.parameters.is_none(),
                Error::dom(
                    "SubjectPublicKeyInfo parameters must not be present",
                    DOMExceptionName::DataError
                )
            );
            let x = spki
                .subject_public_key
                .as_bytes()
                .and_then(|x| <[u8; 32]>::try_from(x).ok())
                .ok_or_else(|| {
                    Error::dom(
                        "SubjectPublicKeyInfo public key has wrong length",
                        DOMExceptionName::DataError,
                    )
                })?;
            Ok(CryptoKey {
                kind: CryptoKeyKind::X25519Public {
                    algorithm: X25519Algorithm {},
                    key: X25519PublicKey {
                        public_key: agreement::UnparsedPublicKey::new(&X25519, x),
                    },
                },
                r#type: KeyType::Public,
                extractable,
                usages,
            })
        },
        ImportKeyInput::Pkcs8(der) => {
            check_usages_subset(&usages, &[KeyUsage::DeriveKey, KeyUsage::DeriveBits])?;
            let pki = pkcs8::PrivateKeyInfo::from_der(&der).map_err(|_| {
                Error::dom("invalid X25519 PrivateKeyInfo", DOMExceptionName::DataError)
            })?;
            ensure!(
                pki.algorithm.oid == X25519_OID,
                Error::dom(
                    "PrivateKeyInfo algorithm is not id-X25519",
                    DOMExceptionName::DataError,
                )
            );
            ensure!(
                pki.algorithm.parameters.is_none(),
                Error::dom(
                    "PrivateKeyInfo parameters must not be present",
                    DOMExceptionName::DataError
                )
            );
            // X25519 PKCS#8 PrivateKeyInfo is a CurvePrivateKey, which is an OCTET STRING
            let private_key = OctetStringRef::from_der(pki.private_key)
                .ok()
                .and_then(|pk| agreement::PrivateKey::from_private_key(&X25519, pk.as_bytes()).ok())
                .ok_or_else(|| {
                    Error::dom("invalid X25519 private key", DOMExceptionName::DataError)
                })?;
            Ok(CryptoKey {
                kind: CryptoKeyKind::X25519Private {
                    algorithm: X25519Algorithm {},
                    key: X25519PrivateKey { private_key },
                },
                r#type: KeyType::Private,
                extractable,
                usages,
            })
        },
        ImportKeyInput::Jwk(jwk) => {
            if jwk.d.is_some() {
                check_usages_subset(&usages, &[KeyUsage::DeriveKey, KeyUsage::DeriveBits])?;
            } else {
                check_usages_subset(&usages, &[])?;
            }
            jwk.check_kty("OKP")?;
            jwk.check_crv("X25519")?;
            jwk.check_key_ops_and_use(&usages, "enc")?;
            jwk.check_ext(extractable)?;
            let x = jwk
                .x
                .as_ref()
                .and_then(|k| base64::decode_config(k, URL_SAFE_FORGIVING).ok())
                .and_then(|x| <[u8; 32]>::try_from(x).ok())
                .ok_or_else(|| Error::dom("invalid key `x`", DOMExceptionName::DataError))?;
            if let Some(d) = jwk.d {
                let private_key = base64::decode_config(&d, URL_SAFE_FORGIVING)
                    .ok()
                    .and_then(|d| agreement::PrivateKey::from_private_key(&X25519, &d).ok())
                    .ok_or_else(|| Error::dom("invalid key `d`", DOMExceptionName::DataError))?;
                let public_key = private_key.compute_public_key()?;
                ensure!(
                    x == public_key.as_ref(),
                    Error::dom("JWT `d` and `x` do not match", DOMExceptionName::DataError)
                );
                Ok(CryptoKey {
                    kind: CryptoKeyKind::X25519Private {
                        algorithm: X25519Algorithm {},
                        key: X25519PrivateKey { private_key },
                    },
                    r#type: KeyType::Private,
                    extractable,
                    usages,
                })
            } else {
                Ok(CryptoKey {
                    kind: CryptoKeyKind::X25519Public {
                        algorithm: X25519Algorithm {},
                        key: X25519PublicKey {
                            public_key: agreement::UnparsedPublicKey::new(&X25519, x),
                        },
                    },
                    r#type: KeyType::Public,
                    extractable,
                    usages,
                })
            }
        },
        ImportKeyInput::Raw(raw) => {
            check_usages_subset(&usages, &[])?;
            let raw = <[u8; 32]>::try_from(raw).map_err(|_| {
                Error::dom(
                    "X25519 public key must be 256 bits",
                    DOMExceptionName::DataError,
                )
            })?;
            Ok(CryptoKey {
                kind: CryptoKeyKind::X25519Public {
                    algorithm: X25519Algorithm {},
                    key: X25519PublicKey {
                        public_key: agreement::UnparsedPublicKey::new(&X25519, raw),
                    },
                },
                r#type: KeyType::Public,
                extractable,
                usages,
            })
        },
    }
}

impl X25519PrivateKey {
    pub fn export_key(&self, format: KeyFormat) -> Result<KeyData> {
        match format {
            KeyFormat::Pkcs8 => Ok(KeyData::Raw(
                pkcs8::PrivateKeyInfo {
                    algorithm: spki::AlgorithmIdentifier {
                        oid: X25519_OID,
                        parameters: None,
                    },
                    private_key: &OctetStringRef::new(
                        AsBigEndian::<Curve25519SeedBin>::as_be_bytes(&self.private_key)?.as_ref(),
                    )?
                    .to_der()?,
                    public_key: None,
                }
                .to_der()?,
            )),
            KeyFormat::Jwk => {
                let b64 =
                    |bytes: &[u8]| Some(base64::encode_config(bytes, base64::URL_SAFE_NO_PAD));
                let jwk = JsonWebKey {
                    kty: Some("OKP".to_owned()),
                    crv: Some("X25519".to_owned()),
                    x: b64(self.private_key.compute_public_key()?.as_ref()),
                    d: b64(
                        AsBigEndian::<Curve25519SeedBin>::as_be_bytes(&self.private_key)?.as_ref(),
                    ),
                    ..Default::default()
                };
                Ok(KeyData::Jwk(jwk))
            },
            KeyFormat::Raw | KeyFormat::Spki => Err(Error::dom(
                "invalid export format for X25519 private key",
                DOMExceptionName::InvalidAccessError,
            )),
        }
    }
}

/// Perform X25519 key agreement to compute shared bits. Takes the same
/// parameters as ECDH.
pub fn derive_bits(
    params: ec::EcdhKeyDeriveParams<impl AsRef<CryptoKey>>,
    base_key: &CryptoKey,
    length: Option<usize>,
) -> Result<Vec<u8>> {
    let CryptoKeyKind::X25519Private { key, .. } = &base_key.kind else {
        return Err(Error::dom(
            "Base key must be an X25519 private key",
            DOMExceptionName::InvalidAccessError,
        ));
    };
    let CryptoKeyKind::X25519Public {
        key: public_key, ..
    } = &params.public_key.as_ref().kind
    else {
        return Err(Error::dom(
            "Public key must be an X25519 public key",
            DOMExceptionName::InvalidAccessError,
        ));
    };
    let peer_public_key = &public_key.public_key;
    let shared_secret = agreement::agree(
        &key.private_key,
        peer_public_key,
        // N.B.: this error is returned if the shared secret is all zero, as
        // required by the WebCrypto spec.
        Error::dom(
            "X25519 key derivation failed",
            DOMExceptionName::OperationError,
        ),
        |secret| Ok(secret.to_vec()),
    )?;
    truncate_shared_secret(shared_secret, length)
}

impl X25519PublicKey {
    pub fn export_key(&self, format: KeyFormat) -> Result<KeyData> {
        match format {
            KeyFormat::Spki => Ok(KeyData::Raw(
                spki::SubjectPublicKeyInfo {
                    algorithm: spki::AlgorithmIdentifierOwned {
                        oid: X25519_OID,
                        parameters: None,
                    },
                    subject_public_key: BitStringRef::from_bytes(self.public_key.bytes())?,
                }
                .to_der()?,
            )),
            KeyFormat::Jwk => {
                let b64 =
                    |bytes: &[u8]| Some(base64::encode_config(bytes, base64::URL_SAFE_NO_PAD));
                let jwk = JsonWebKey {
                    kty: Some("OKP".to_owned()),
                    crv: Some("X25519".to_owned()),
                    x: b64(self.public_key.bytes()),
                    ..Default::default()
                };
                Ok(KeyData::Jwk(jwk))
            },
            KeyFormat::Raw => Ok(KeyData::Raw(self.public_key.bytes().to_vec())),
            KeyFormat::Pkcs8 => Err(Error::dom(
                "invalid export format for X25519 public key",
                DOMExceptionName::InvalidAccessError,
            )),
        }
    }
}
