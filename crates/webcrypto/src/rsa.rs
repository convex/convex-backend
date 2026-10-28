use std::ops::RangeInclusive;

use indexmap::{
    indexset,
    IndexSet,
};
use openssl_aws_lc::{
    bn::{
        BigNum,
        BigNumRef,
    },
    encrypt::{
        Decrypter,
        Encrypter,
    },
    pkey::{
        HasPublic,
        PKey,
        Private,
        Public,
    },
    rsa::{
        Padding,
        Rsa,
        RsaPrivateKeyBuilder,
    },
    sign::{
        RsaPssSaltlen,
        Signer,
        Verifier,
    },
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_bytes::ByteBuf;
use spki::der::{
    asn1::BitStringRef,
    AnyRef,
    Decode,
};
use strum::EnumString;

use super::{
    check_usages_subset,
    ensure,
    CryptoHash,
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

// pkcs-1 OBJECT IDENTIFIER ::= { iso(1) member-body(2) us(840) rsadsi(113549)
// pkcs(1) 1 } rsaEncryption OBJECT IDENTIFIER ::=  { pkcs-1 1}
const RSA_OID: const_oid::ObjectIdentifier =
    const_oid::ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");

#[derive(Deserialize, Serialize, Copy, Clone, PartialEq, Debug, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum RsaAlgorithm {
    #[serde(rename = "RSASSA-PKCS1-v1_5")]
    #[strum(serialize = "RSASSA-PKCS1-v1_5")]
    RsaSsaPkcs1v15,
    #[serde(rename = "RSA-PSS")]
    #[strum(serialize = "RSA-PSS")]
    RsaPss,
    #[serde(rename = "RSA-OAEP")]
    #[strum(serialize = "RSA-OAEP")]
    RsaOaep,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RsaHashedKeyGenParams {
    #[serde(deserialize_with = "super::algorithm_name::deserialize")]
    pub name: RsaAlgorithm,
    pub modulus_length: u32,
    pub public_exponent: ByteBuf,
    #[serde(with = "super::nullary_algorithm")]
    pub hash: CryptoHash,
}

pub type RsaHashedKeyAlgorithm = RsaHashedKeyGenParams;

#[derive(Deserialize, Debug)]
pub struct RsaHashedImportParams {
    #[serde(deserialize_with = "super::algorithm_name::deserialize")]
    pub name: RsaAlgorithm,
    #[serde(with = "super::nullary_algorithm")]
    pub hash: CryptoHash,
}

#[derive(Deserialize)]
pub struct RsaOaepParams {
    pub label: Option<ByteBuf>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RsaPssParams {
    pub salt_length: u32,
}

pub enum RsaParams {
    RsaSsaPkcs1v15,
    RsaPss(RsaPssParams),
}

const ALLOWED_MODULUS_LENGTHS: RangeInclusive<u32> = 512..=8192;

pub struct RsaPrivateKey {
    private_key: PKey<Private>,
}
pub struct RsaPublicKey {
    public_key: PKey<Public>,
}

fn jwk_alg(name: RsaAlgorithm, hash: CryptoHash) -> &'static str {
    match (name, hash) {
        (RsaAlgorithm::RsaSsaPkcs1v15, CryptoHash::Sha1) => "RS1",
        (RsaAlgorithm::RsaSsaPkcs1v15, CryptoHash::Sha256) => "RS256",
        (RsaAlgorithm::RsaSsaPkcs1v15, CryptoHash::Sha384) => "RS384",
        (RsaAlgorithm::RsaSsaPkcs1v15, CryptoHash::Sha512) => "RS512",
        (RsaAlgorithm::RsaPss, CryptoHash::Sha1) => "PS1",
        (RsaAlgorithm::RsaPss, CryptoHash::Sha256) => "PS256",
        (RsaAlgorithm::RsaPss, CryptoHash::Sha384) => "PS384",
        (RsaAlgorithm::RsaPss, CryptoHash::Sha512) => "PS512",
        (RsaAlgorithm::RsaOaep, CryptoHash::Sha1) => "RSA-OAEP",
        (RsaAlgorithm::RsaOaep, CryptoHash::Sha256) => "RSA-OAEP-256",
        (RsaAlgorithm::RsaOaep, CryptoHash::Sha384) => "RSA-OAEP-384",
        (RsaAlgorithm::RsaOaep, CryptoHash::Sha512) => "RSA-OAEP-512",
    }
}

pub fn generate_keypair(
    algorithm: RsaHashedKeyGenParams,
    _rng: &CryptoRng,
    extractable: bool,
    usages: IndexSet<KeyUsage>,
) -> Result<CryptoKeyPair> {
    let (private_usages, public_usages) = match algorithm.name {
        RsaAlgorithm::RsaSsaPkcs1v15 | RsaAlgorithm::RsaPss => {
            check_usages_subset(&usages, &[KeyUsage::Sign, KeyUsage::Verify])?;
            (
                usages
                    .intersection(&indexset![KeyUsage::Sign])
                    .copied()
                    .collect(),
                usages
                    .intersection(&indexset![KeyUsage::Verify])
                    .copied()
                    .collect(),
            )
        },

        RsaAlgorithm::RsaOaep => {
            check_usages_subset(
                &usages,
                &[
                    KeyUsage::Encrypt,
                    KeyUsage::Decrypt,
                    KeyUsage::WrapKey,
                    KeyUsage::UnwrapKey,
                ],
            )?;
            (
                usages
                    .intersection(&indexset![KeyUsage::Decrypt, KeyUsage::UnwrapKey])
                    .copied()
                    .collect(),
                usages
                    .intersection(&indexset![KeyUsage::Encrypt, KeyUsage::WrapKey])
                    .copied()
                    .collect(),
            )
        },
    };
    ensure!(
        ALLOWED_MODULUS_LENGTHS.contains(&algorithm.modulus_length),
        Error::dom(
            "unsupported RSA modulus length",
            DOMExceptionName::OperationError
        )
    );
    let exp = BigNum::from_slice(&algorithm.public_exponent).map_err(|_| {
        Error::dom(
            "invalid RSA public exponent",
            DOMExceptionName::OperationError,
        )
    })?;
    let private_key = Rsa::generate_with_e(algorithm.modulus_length, &exp).map_err(|_| {
        Error::dom(
            "failed to generate RSA key",
            DOMExceptionName::OperationError,
        )
    })?;
    let public_key =
        Rsa::from_public_components(private_key.n().to_owned()?, private_key.e().to_owned()?)?;
    Ok(CryptoKeyPair {
        private_key: CryptoKey {
            kind: CryptoKeyKind::RsaPrivate {
                algorithm: algorithm.clone(),
                key: RsaPrivateKey {
                    private_key: PKey::from_rsa(private_key)?,
                },
            },
            r#type: KeyType::Private,
            extractable,
            usages: private_usages,
        },
        public_key: CryptoKey {
            kind: CryptoKeyKind::RsaPublic {
                algorithm,
                key: RsaPublicKey {
                    public_key: PKey::from_rsa(public_key)?,
                },
            },
            r#type: KeyType::Public,
            extractable: true, // N.B.: public key is always extractable
            usages: public_usages,
        },
    })
}

fn key_info<T: HasPublic>(
    algorithm: RsaHashedImportParams,
    rsa: &Rsa<T>,
) -> Result<RsaHashedKeyAlgorithm> {
    let modulus_length = rsa.n().num_bits() as u32;
    let public_exponent = ByteBuf::from(rsa.e().to_vec());
    Ok(RsaHashedKeyAlgorithm {
        name: algorithm.name,
        modulus_length,
        public_exponent,
        hash: algorithm.hash,
    })
}

pub fn import_key(
    input: ImportKeyInput,
    algorithm: RsaHashedImportParams,
    extractable: bool,
    usages: IndexSet<KeyUsage>,
) -> Result<CryptoKey> {
    let (private_usages, public_usages, jwk_use) = match algorithm.name {
        RsaAlgorithm::RsaSsaPkcs1v15 | RsaAlgorithm::RsaPss => {
            (&[KeyUsage::Sign][..], &[KeyUsage::Verify][..], "sig")
        },
        RsaAlgorithm::RsaOaep => (
            &[KeyUsage::Decrypt, KeyUsage::UnwrapKey][..],
            &[KeyUsage::Encrypt, KeyUsage::WrapKey][..],
            "enc",
        ),
    };
    match input {
        ImportKeyInput::Spki(der) => {
            check_usages_subset(&usages, public_usages)?;
            let spki = spki::SubjectPublicKeyInfo::<AnyRef, BitStringRef<'_>>::from_der(&der)
                .map_err(|_| {
                    Error::dom(
                        "invalid RSA SubjectPublicKeyInfo",
                        DOMExceptionName::DataError,
                    )
                })?;
            ensure!(
                spki.algorithm.oid == RSA_OID,
                Error::dom(
                    "SubjectPublicKeyInfo algorithm is not rsaEncryption",
                    DOMExceptionName::DataError,
                )
            );
            let public_key = Rsa::public_key_from_der_pkcs1(spki.subject_public_key.raw_bytes())
                .map_err(|_| {
                    Error::dom("invalid RSA SubjectPublicKey", DOMExceptionName::DataError)
                })?;
            Ok(CryptoKey {
                kind: CryptoKeyKind::RsaPublic {
                    algorithm: key_info(algorithm, &public_key)?,
                    key: RsaPublicKey {
                        public_key: PKey::from_rsa(public_key)?,
                    },
                },
                r#type: KeyType::Public,
                extractable,
                usages,
            })
        },
        ImportKeyInput::Pkcs8(der) => {
            check_usages_subset(&usages, private_usages)?;
            let pki = pkcs8::PrivateKeyInfo::from_der(&der).map_err(|_| {
                Error::dom("invalid RSA PrivateKeyInfo", DOMExceptionName::DataError)
            })?;
            ensure!(
                pki.algorithm.oid == RSA_OID,
                Error::dom(
                    "PrivateKeyInfo algorithm is not rsaEncryption",
                    DOMExceptionName::DataError,
                )
            );
            let private_key = Rsa::private_key_from_der(pki.private_key)
                .map_err(|_| Error::dom("invalid RSA PrivateKey", DOMExceptionName::DataError))?;
            Ok(CryptoKey {
                kind: CryptoKeyKind::RsaPrivate {
                    algorithm: key_info(algorithm, &private_key)?,
                    key: RsaPrivateKey {
                        private_key: PKey::from_rsa(private_key)?,
                    },
                },
                r#type: KeyType::Private,
                extractable,
                usages,
            })
        },
        ImportKeyInput::Jwk(jwk) => {
            if jwk.d.is_some() {
                check_usages_subset(&usages, private_usages)?;
            } else {
                check_usages_subset(&usages, public_usages)?;
            }
            jwk.check_kty("RSA")?;
            jwk.check_key_ops_and_use(&usages, jwk_use)?;
            jwk.check_ext(extractable)?;
            jwk.check_alg(jwk_alg(algorithm.name, algorithm.hash))?;
            let decode_number = |a: &Option<String>| {
                a.as_ref()
                    .and_then(|a| base64::decode_config(a, URL_SAFE_FORGIVING).ok())
                    .and_then(|a| BigNum::from_slice(&a).ok())
            };
            let data_error = || Error::dom("invalid RSA parameters", DOMExceptionName::DataError);
            let n = decode_number(&jwk.n).ok_or_else(data_error)?;
            let e = decode_number(&jwk.e).ok_or_else(data_error)?;
            if let Some(d) = jwk.d {
                // RFC 7518 6.3.2.7 admits "oth" only for a key with more than
                // two prime factors, which this implementation cannot
                // represent.
                ensure!(
                    jwk.oth.is_none(),
                    Error::dom(
                        "multi-prime RSA keys are not supported",
                        DOMExceptionName::DataError
                    )
                );
                let d = decode_number(&Some(d)).ok_or_else(data_error)?;
                let mut builder = RsaPrivateKeyBuilder::new(n, e, d).map_err(|_| data_error())?;
                match (jwk.p, jwk.q, jwk.dp, jwk.dq, jwk.qi) {
                    (None, None, None, None, None) => {},
                    (Some(p), Some(q), Some(dmp1), Some(dmq1), Some(iqmp)) => {
                        let p = decode_number(&Some(p)).ok_or_else(data_error)?;
                        let q = decode_number(&Some(q)).ok_or_else(data_error)?;
                        let dmp1 = decode_number(&Some(dmp1)).ok_or_else(data_error)?;
                        let dmq1 = decode_number(&Some(dmq1)).ok_or_else(data_error)?;
                        let iqmp = decode_number(&Some(iqmp)).ok_or_else(data_error)?;
                        builder = builder.set_factors(p, q).map_err(|_| data_error())?;
                        builder = builder
                            .set_crt_params(dmp1, dmq1, iqmp)
                            .map_err(|_| data_error())?;
                    },
                    // RFC 7518 6.3.2 requires the second prime factor and the
                    // CRT parameters to accompany the first, or be absent
                    // together.
                    _ => {
                        return Err(Error::dom(
                            "incomplete RSA private key parameters",
                            DOMExceptionName::DataError,
                        ))
                    },
                }
                let private_key = builder.build();
                ensure!(
                    private_key.check_key().unwrap_or(false),
                    Error::dom("inconsistent RSA private key", DOMExceptionName::DataError)
                );
                Ok(CryptoKey {
                    kind: CryptoKeyKind::RsaPrivate {
                        algorithm: key_info(algorithm, &private_key)?,
                        key: RsaPrivateKey {
                            private_key: PKey::from_rsa(private_key)?,
                        },
                    },
                    r#type: KeyType::Private,
                    extractable,
                    usages,
                })
            } else {
                let public_key = Rsa::from_public_components(n, e).map_err(|_| data_error())?;
                Ok(CryptoKey {
                    kind: CryptoKeyKind::RsaPublic {
                        algorithm: key_info(algorithm, &public_key)?,
                        key: RsaPublicKey {
                            public_key: PKey::from_rsa(public_key)?,
                        },
                    },
                    r#type: KeyType::Public,
                    extractable,
                    usages,
                })
            }
        },
        ImportKeyInput::Raw(_) => Err(Error::dom(
            "unsupported import format",
            DOMExceptionName::NotSupportedError,
        )),
    }
}

impl RsaPrivateKey {
    pub fn export_key(
        &self,
        algorithm: &RsaHashedKeyAlgorithm,
        format: KeyFormat,
    ) -> Result<KeyData> {
        match format {
            KeyFormat::Pkcs8 => Ok(KeyData::Raw(self.private_key.private_key_to_pkcs8()?)),
            KeyFormat::Jwk => {
                let rsa = self.private_key.rsa()?;
                let maybe_b64 = |bn: Option<&BigNumRef>| {
                    bn.map(|bn| base64::encode_config(bn.to_vec(), base64::URL_SAFE_NO_PAD))
                };
                let b64 = |bn: &BigNumRef| maybe_b64(Some(bn));
                let jwk = JsonWebKey {
                    kty: Some("RSA".to_owned()),
                    alg: Some(jwk_alg(algorithm.name, algorithm.hash).to_owned()),
                    n: b64(rsa.n()),
                    e: b64(rsa.e()),
                    d: b64(rsa.d()),
                    p: maybe_b64(rsa.p()),
                    q: maybe_b64(rsa.q()),
                    dp: maybe_b64(rsa.dmp1()),
                    dq: maybe_b64(rsa.dmq1()),
                    qi: maybe_b64(rsa.iqmp()),
                    ..Default::default()
                };
                Ok(KeyData::Jwk(jwk))
            },
            KeyFormat::Spki => Err(Error::dom(
                "invalid export format for RSA private key",
                DOMExceptionName::InvalidAccessError,
            )),
            KeyFormat::Raw => Err(Error::dom(
                "invalid export format for RSA",
                DOMExceptionName::NotSupportedError,
            )),
        }
    }

    pub fn sign(
        &self,
        params: RsaParams,
        algorithm: &RsaHashedKeyAlgorithm,
        rng: impl FnOnce() -> Result<CryptoRng>,
        data: &[u8],
    ) -> Result<Vec<u8>> {
        let hash_algorithm = algorithm.hash.openssl_message_digest();
        let mut signer = Signer::new(hash_algorithm, &self.private_key)?;
        match (algorithm.name, params) {
            (RsaAlgorithm::RsaSsaPkcs1v15, RsaParams::RsaSsaPkcs1v15) => {
                signer.set_rsa_padding(Padding::PKCS1)?;
            },
            (RsaAlgorithm::RsaPss, RsaParams::RsaPss(params)) => {
                // RSA-PSS uses randomized padding; this requires the
                // crypto RNG (although openssl-rs doesn't ask for it
                // explicitly)
                _ = rng()?;
                signer.set_rsa_padding(Padding::PKCS1_PSS)?;
                signer.set_rsa_pss_saltlen(RsaPssSaltlen::custom(
                    params.salt_length.try_into().map_err(|_| {
                        Error::dom("invalid saltLength", DOMExceptionName::OperationError)
                    })?,
                ))?;
            },
            _ => {
                return Err(Error::dom(
                    "invalid algorithm for key",
                    DOMExceptionName::InvalidAccessError,
                ))
            },
        };
        signer
            .len()
            .and_then(|len| {
                let mut out = vec![0; len];
                let len = signer.sign_oneshot(&mut out, data)?;
                out.truncate(len);
                Ok(out)
            })
            .map_err(|_| Error::dom("RSA signing failed", DOMExceptionName::OperationError))
    }

    pub fn decrypt_oaep(
        &self,
        params: RsaOaepParams,
        algorithm: &RsaHashedKeyAlgorithm,
        data: &[u8],
    ) -> Result<Vec<u8>> {
        ensure!(
            algorithm.name == RsaAlgorithm::RsaOaep,
            Error::dom(
                "invalid algorithm for key",
                DOMExceptionName::InvalidAccessError
            )
        );
        let mut decrypter = Decrypter::new(&self.private_key)?;
        decrypter.set_rsa_padding(Padding::PKCS1_OAEP)?;
        let md = algorithm.hash.openssl_message_digest();
        decrypter.set_rsa_oaep_md(md)?;
        decrypter.set_rsa_mgf1_md(md)?;
        if let Some(label) = &params.label {
            decrypter.set_rsa_oaep_label(label)?;
        }
        decrypter
            .decrypt_len(data)
            .and_then(|len| {
                let mut out = vec![0; len];
                let len = decrypter.decrypt(data, &mut out)?;
                out.truncate(len);
                Ok(out)
            })
            .map_err(|_| Error::dom("OAEP decryption failed", DOMExceptionName::OperationError))
    }
}

impl RsaPublicKey {
    pub fn export_key(
        &self,
        algorithm: &RsaHashedKeyAlgorithm,
        format: KeyFormat,
    ) -> Result<KeyData> {
        match format {
            KeyFormat::Spki => Ok(KeyData::Raw(self.public_key.public_key_to_der()?)),
            KeyFormat::Jwk => {
                let rsa = self.public_key.rsa()?;
                let b64 = |bn: &BigNumRef| {
                    Some(base64::encode_config(bn.to_vec(), base64::URL_SAFE_NO_PAD))
                };
                let jwk = JsonWebKey {
                    kty: Some("RSA".to_owned()),
                    alg: Some(jwk_alg(algorithm.name, algorithm.hash).to_owned()),
                    n: b64(rsa.n()),
                    e: b64(rsa.e()),
                    ..Default::default()
                };
                Ok(KeyData::Jwk(jwk))
            },
            KeyFormat::Pkcs8 => Err(Error::dom(
                "invalid export format for RSA public key",
                DOMExceptionName::InvalidAccessError,
            )),
            KeyFormat::Raw => Err(Error::dom(
                "invalid export format for RSA",
                DOMExceptionName::NotSupportedError,
            )),
        }
    }

    pub fn verify(
        &self,
        params: RsaParams,
        algorithm: &RsaHashedKeyAlgorithm,
        data: &[u8],
        signature: &[u8],
    ) -> Result<bool> {
        let hash_algorithm = algorithm.hash.openssl_message_digest();
        let mut verifier = Verifier::new(hash_algorithm, &self.public_key)?;
        match (algorithm.name, params) {
            (RsaAlgorithm::RsaSsaPkcs1v15, RsaParams::RsaSsaPkcs1v15) => {
                verifier.set_rsa_padding(Padding::PKCS1)?;
            },
            (RsaAlgorithm::RsaPss, RsaParams::RsaPss(params)) => {
                verifier.set_rsa_padding(Padding::PKCS1_PSS)?;
                verifier.set_rsa_pss_saltlen(RsaPssSaltlen::custom(
                    params.salt_length.try_into().map_err(|_| {
                        Error::dom("invalid saltLength", DOMExceptionName::OperationError)
                    })?,
                ))?;
            },
            _ => {
                return Err(Error::dom(
                    "invalid algorithm for key",
                    DOMExceptionName::InvalidAccessError,
                ))
            },
        };
        verifier
            .verify_oneshot(signature, data)
            .map_err(|_| Error::dom("RSA verification failed", DOMExceptionName::OperationError))
    }

    pub fn encrypt_oaep(
        &self,
        params: RsaOaepParams,
        algorithm: &RsaHashedKeyAlgorithm,
        _rng: &CryptoRng,
        data: &[u8],
    ) -> Result<Vec<u8>> {
        ensure!(
            algorithm.name == RsaAlgorithm::RsaOaep,
            Error::dom(
                "invalid algorithm for key",
                DOMExceptionName::InvalidAccessError
            )
        );
        let mut encrypter = Encrypter::new(&self.public_key)?;
        encrypter.set_rsa_padding(Padding::PKCS1_OAEP)?;
        let md = algorithm.hash.openssl_message_digest();
        encrypter.set_rsa_oaep_md(md)?;
        encrypter.set_rsa_mgf1_md(md)?;
        if let Some(label) = &params.label {
            encrypter.set_rsa_oaep_label(label)?;
        }
        encrypter
            .encrypt_len(data)
            .and_then(|len| {
                let mut out = vec![0; len];
                let len = encrypter.encrypt(data, &mut out)?;
                out.truncate(len);
                Ok(out)
            })
            .map_err(|_| Error::dom("OAEP encryption failed", DOMExceptionName::OperationError))
    }
}
