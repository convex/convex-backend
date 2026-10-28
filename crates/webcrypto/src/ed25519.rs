use anyhow::Context as _;
use aws_lc_rs::{
    encoding::AsBigEndian,
    signature::{
        self,
        Ed25519KeyPair,
        KeyPair,
        UnparsedPublicKey,
    },
};
use indexmap::{
    indexset,
    IndexSet,
};
use serde::Serialize;
use spki::der::{
    asn1::BitStringRef,
    AnyRef,
    Decode as _,
    Encode as _,
};

use super::{
    check_usages_subset,
    ensure,
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

// id-Ed25519 OBJECT IDENTIFIER ::= { 1 3 101 112 }
const ED25519_OID: const_oid::ObjectIdentifier =
    const_oid::ObjectIdentifier::new_unwrap("1.3.101.112");

#[derive(Serialize)]
#[serde(tag = "name")]
#[serde(rename = "Ed25519")]
pub struct Ed25519Algorithm {}

pub struct Ed25519PrivateKey {
    keypair: Ed25519KeyPair,
}
pub struct Ed25519PublicKey {
    x: [u8; 32],
}

pub fn generate_keypair(
    _rng: &CryptoRng,
    extractable: bool,
    usages: IndexSet<KeyUsage>,
) -> Result<CryptoKeyPair> {
    let keypair = Ed25519KeyPair::generate().context("failed to generate ed25519 keypair")?;
    let public_key = <[u8; 32]>::try_from(keypair.public_key().as_ref())?;
    check_usages_subset(&usages, &[KeyUsage::Sign, KeyUsage::Verify])?;
    Ok(CryptoKeyPair {
        private_key: CryptoKey {
            kind: CryptoKeyKind::Ed25519Private {
                algorithm: Ed25519Algorithm {},
                key: Ed25519PrivateKey { keypair },
            },
            r#type: KeyType::Private,
            extractable,
            usages: usages
                .intersection(&indexset![KeyUsage::Sign])
                .copied()
                .collect(),
        },
        public_key: CryptoKey {
            kind: CryptoKeyKind::Ed25519Public {
                algorithm: Ed25519Algorithm {},
                key: Ed25519PublicKey { x: public_key },
            },
            r#type: KeyType::Public,
            extractable: true, // N.B.: public key is always extractable
            usages: usages
                .intersection(&indexset![KeyUsage::Verify])
                .copied()
                .collect(),
        },
    })
}

pub fn import_key(
    format: ImportKeyInput,
    extractable: bool,
    usages: IndexSet<KeyUsage>,
) -> Result<CryptoKey> {
    match format {
        ImportKeyInput::Raw(raw) => {
            check_usages_subset(&usages, &[KeyUsage::Verify])?;
            let raw = <[u8; 32]>::try_from(raw).map_err(|_| {
                Error::dom(
                    "Ed25519 public key must be 256 bits",
                    DOMExceptionName::DataError,
                )
            })?;
            Ok(CryptoKey {
                kind: CryptoKeyKind::Ed25519Public {
                    algorithm: Ed25519Algorithm {},
                    key: Ed25519PublicKey { x: raw },
                },
                r#type: KeyType::Public,
                extractable,
                usages,
            })
        },
        ImportKeyInput::Pkcs8(der) => {
            check_usages_subset(&usages, &[KeyUsage::Sign])?;
            let keypair = Ed25519KeyPair::from_pkcs8(&der).map_err(|_| {
                Error::dom("Invalid Ed25519 private key", DOMExceptionName::DataError)
            })?;
            Ok(CryptoKey {
                kind: CryptoKeyKind::Ed25519Private {
                    algorithm: Ed25519Algorithm {},
                    key: Ed25519PrivateKey { keypair },
                },
                r#type: KeyType::Private,
                extractable,
                usages,
            })
        },
        ImportKeyInput::Spki(der) => {
            check_usages_subset(&usages, &[KeyUsage::Verify])?;
            let spki = spki::SubjectPublicKeyInfo::<AnyRef, BitStringRef<'_>>::from_der(&der)
                .map_err(|_| {
                    Error::dom(
                        "invalid SubjectPublicKeyInfo document",
                        DOMExceptionName::DataError,
                    )
                })?;
            ensure!(
                spki.algorithm.oid == ED25519_OID,
                Error::dom(
                    "SubjectPublicKeyInfo algorithm is not id-Ed25519",
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
                kind: CryptoKeyKind::Ed25519Public {
                    algorithm: Ed25519Algorithm {},
                    key: Ed25519PublicKey { x },
                },
                r#type: KeyType::Public,
                extractable,
                usages,
            })
        },
        ImportKeyInput::Jwk(jwk) => {
            if jwk.d.is_some() {
                check_usages_subset(&usages, &[KeyUsage::Sign])?;
            } else {
                check_usages_subset(&usages, &[KeyUsage::Verify])?;
            }
            jwk.check_kty("OKP")?;
            jwk.check_crv("Ed25519")?;
            jwk.check_alg_oneof(&["Ed25519", "EdDSA"])?;
            jwk.check_key_ops_and_use(&usages, "sig")?;
            jwk.check_ext(extractable)?;
            let x = jwk
                .x
                .as_ref()
                .and_then(|k| base64::decode_config(k, URL_SAFE_FORGIVING).ok())
                .and_then(|x| <[u8; 32]>::try_from(x).ok())
                .ok_or_else(|| Error::dom("invalid key `x`", DOMExceptionName::DataError))?;
            if let Some(d) = jwk.d {
                let d = base64::decode_config(&d, URL_SAFE_FORGIVING)
                    .ok()
                    .and_then(|d| <[u8; 32]>::try_from(d).ok())
                    .ok_or_else(|| Error::dom("invalid key `d`", DOMExceptionName::DataError))?;
                let keypair = Ed25519KeyPair::from_seed_and_public_key(&d, &x).map_err(|_| {
                    Error::dom("JWT `d` and `x` do not match", DOMExceptionName::DataError)
                })?;
                Ok(CryptoKey {
                    kind: CryptoKeyKind::Ed25519Private {
                        algorithm: Ed25519Algorithm {},
                        key: Ed25519PrivateKey { keypair },
                    },
                    r#type: KeyType::Private,
                    extractable,
                    usages,
                })
            } else {
                Ok(CryptoKey {
                    kind: CryptoKeyKind::Ed25519Public {
                        algorithm: Ed25519Algorithm {},
                        key: Ed25519PublicKey { x },
                    },
                    r#type: KeyType::Public,
                    extractable,
                    usages,
                })
            }
        },
    }
}

impl Ed25519PrivateKey {
    pub fn export_key(&self, format: KeyFormat) -> Result<KeyData> {
        match format {
            KeyFormat::Pkcs8 => {
                // N.B.: WebCrypto spec calls for version 1:
                // > Let data be an instance of the PrivateKeyInfo ASN.1 structure defined in [RFC5208] with the following properties:
                // > - Set the version field to 0.
                // > ...
                Ok(KeyData::Raw(self.keypair.to_pkcs8v1()?.as_ref().to_vec()))
            },
            KeyFormat::Jwk => {
                let jwk = JsonWebKey {
                    kty: Some("OKP".to_owned()),
                    alg: Some("Ed25519".to_owned()),
                    crv: Some("Ed25519".to_owned()),
                    x: Some(base64::encode_config(
                        self.keypair.public_key().as_ref(),
                        base64::URL_SAFE_NO_PAD,
                    )),
                    d: Some(base64::encode_config(
                        self.keypair.seed()?.as_be_bytes()?.as_ref(),
                        base64::URL_SAFE_NO_PAD,
                    )),
                    ..Default::default()
                };
                Ok(KeyData::Jwk(jwk))
            },
            KeyFormat::Raw | KeyFormat::Spki => Err(Error::dom(
                "invalid export format for Ed25519 private key",
                DOMExceptionName::InvalidAccessError,
            )),
        }
    }

    pub fn sign(&self, data: &[u8]) -> Vec<u8> {
        self.keypair.sign(data).as_ref().to_vec()
    }
}

impl Ed25519PublicKey {
    pub fn export_key(&self, format: KeyFormat) -> Result<KeyData> {
        match format {
            KeyFormat::Spki => {
                Ok(KeyData::Raw(
                    spki::SubjectPublicKeyInfo {
                        algorithm: spki::AlgorithmIdentifierOwned {
                            // id-Ed25519
                            oid: ED25519_OID,
                            parameters: None,
                        },
                        subject_public_key: BitStringRef::from_bytes(&self.x)?,
                    }
                    .to_der()?,
                ))
            },
            KeyFormat::Jwk => {
                let jwk = JsonWebKey {
                    kty: Some("OKP".to_owned()),
                    alg: Some("Ed25519".to_owned()),
                    crv: Some("Ed25519".to_owned()),
                    x: Some(base64::encode_config(self.x, base64::URL_SAFE_NO_PAD)),
                    ..Default::default()
                };
                Ok(KeyData::Jwk(jwk))
            },
            KeyFormat::Raw => Ok(KeyData::Raw(self.x.to_vec())),
            KeyFormat::Pkcs8 => Err(Error::dom(
                "invalid export format for Ed25519 public key",
                DOMExceptionName::InvalidAccessError,
            )),
        }
    }

    pub fn verify(&self, data: &[u8], signature: &[u8]) -> bool {
        UnparsedPublicKey::new(&signature::ED25519, self.x)
            .verify(data, signature)
            .is_ok()
    }
}
