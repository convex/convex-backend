use indexmap::{
    indexset,
    IndexSet,
};
use openssl_aws_lc::{
    bn::{
        BigNum,
        BigNumContext,
    },
    derive::Deriver,
    ec::{
        EcGroup,
        EcKey,
        EcPoint,
        PointConversionForm,
    },
    ecdsa::EcdsaSig,
    nid::Nid,
    pkey::{
        HasPublic,
        PKey,
        Private,
        Public,
    },
    sign::{
        Signer,
        Verifier,
    },
};
use serde::{
    Deserialize,
    Serialize,
};
use spki::{
    der::{
        asn1::BitStringRef,
        Decode as _,
    },
    ObjectIdentifier,
};
use strum::EnumString;

use super::{
    check_usages_subset,
    ensure,
    truncate_shared_secret,
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

// id-ecPublicKey OBJECT IDENTIFIER ::= { iso(1) member-body(2) us(840)
// ansi-X9-62(10045) keyType(2) 1 }
const ALGORITHM_OID: pkcs8::ObjectIdentifier =
    pkcs8::ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
const ID_SECP256R1_OID: const_oid::ObjectIdentifier =
    const_oid::ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");
const ID_SECP384R1_OID: const_oid::ObjectIdentifier =
    const_oid::ObjectIdentifier::new_unwrap("1.3.132.0.34");
const ID_SECP521R1_OID: const_oid::ObjectIdentifier =
    const_oid::ObjectIdentifier::new_unwrap("1.3.132.0.35");

#[derive(Serialize, Deserialize, Copy, Clone, PartialEq, Debug, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum EcAlgorithm {
    /// The "ECDSA" algorithm identifier is used to perform signing and
    /// verification using the ECDSA algorithm specified in [RFC6090] and using
    /// the SHA hash functions and elliptic curves defined in this
    /// specification.
    #[serde(rename = "ECDSA")]
    #[strum(serialize = "ECDSA")]
    Ecdsa,
    /// This describes using Elliptic Curve Diffie-Hellman (ECDH) for key
    /// generation and key agreement, as specified by [RFC6090].
    #[serde(rename = "ECDH")]
    #[strum(serialize = "ECDH")]
    Ecdh,
}

/// The NamedCurve type represents named elliptic curves, which are a convenient
/// way to specify the domain parameters of well-known elliptic curves.
#[derive(Serialize, Deserialize, Copy, Clone, PartialEq, Debug)]
pub enum NamedCurve {
    /// NIST recommended curve P-256, also known as secp256r1.
    #[serde(rename = "P-256")]
    P256,
    /// NIST recommended curve P-384, also known as secp384r1.
    #[serde(rename = "P-384")]
    P384,
    /// NIST recommended curve P-521, also known as secp521r1.
    #[serde(rename = "P-521")]
    P521,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EcKeyAlgorithm {
    #[serde(deserialize_with = "super::algorithm_name::deserialize")]
    pub name: EcAlgorithm,
    /// The namedCurve member represents the named curve that the key uses.
    pub named_curve: NamedCurve,
}

pub type EcKeyImportParams = EcKeyAlgorithm;
pub type EcKeyGenParams = EcKeyAlgorithm;

#[derive(Deserialize)]
pub struct EcdsaParams {
    #[serde(with = "super::nullary_algorithm")]
    pub hash: CryptoHash,
}

pub struct EcPrivateKey {
    private_key: PKey<Private>,
}
pub struct EcPublicKey {
    public_key: PKey<Public>,
}

impl NamedCurve {
    fn nid(&self) -> Nid {
        match self {
            NamedCurve::P256 => Nid::X9_62_PRIME256V1,
            NamedCurve::P384 => Nid::SECP384R1,
            NamedCurve::P521 => Nid::SECP521R1,
        }
    }

    // the smallest integer such that n * 8 is greater than the logarithm to
    // base 2 of the order of the base point of the elliptic curve
    fn private_key_size(&self) -> usize {
        match self {
            NamedCurve::P256 => 32,
            NamedCurve::P384 => 48,
            NamedCurve::P521 => 66,
        }
    }
}

pub fn generate_keypair(
    algorithm: EcKeyGenParams,
    _rng: &CryptoRng,
    extractable: bool,
    usages: IndexSet<KeyUsage>,
) -> Result<CryptoKeyPair> {
    let (private_usages, public_usages) = match algorithm.name {
        EcAlgorithm::Ecdsa => {
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
        EcAlgorithm::Ecdh => {
            check_usages_subset(&usages, &[KeyUsage::DeriveKey, KeyUsage::DeriveBits])?;
            (usages, indexset![])
        },
    };
    let group = EcGroup::from_curve_name(algorithm.named_curve.nid())?;
    let private_key = EcKey::generate(&group)?;
    let public_key = EcKey::from_public_key(&group, private_key.public_key())?;
    Ok(CryptoKeyPair {
        private_key: CryptoKey {
            kind: CryptoKeyKind::EcPrivate {
                algorithm: algorithm.clone(),
                key: EcPrivateKey {
                    private_key: PKey::from_ec_key(private_key)?,
                },
            },
            r#type: KeyType::Private,
            extractable,
            usages: private_usages,
        },
        public_key: CryptoKey {
            kind: CryptoKeyKind::EcPublic {
                algorithm,
                key: EcPublicKey {
                    public_key: PKey::from_ec_key(public_key)?,
                },
            },
            r#type: KeyType::Public,
            extractable: true, // N.B.: public key is always extractable
            usages: public_usages,
        },
    })
}

pub fn import_key(
    input: ImportKeyInput,
    algorithm: EcKeyImportParams,
    extractable: bool,
    usages: IndexSet<KeyUsage>,
) -> Result<CryptoKey> {
    let (private_usages, public_usages) = match algorithm.name {
        EcAlgorithm::Ecdsa => (&[KeyUsage::Sign][..], &[KeyUsage::Verify][..]),
        EcAlgorithm::Ecdh => (&[KeyUsage::DeriveKey, KeyUsage::DeriveBits][..], &[][..]),
    };
    match input {
        ImportKeyInput::Spki(der) => {
            check_usages_subset(&usages, public_usages)?;
            let spki =
                spki::SubjectPublicKeyInfo::<ObjectIdentifier, BitStringRef<'_>>::from_der(&der)
                    .map_err(|_| {
                        Error::dom(
                            "invalid SubjectPublicKeyInfo document",
                            DOMExceptionName::DataError,
                        )
                    })?;
            // id-ecPublicKey
            ensure!(
                spki.algorithm.oid == ALGORITHM_OID,
                Error::dom(
                    "algorithm oid is not id-ecPublicKey",
                    DOMExceptionName::DataError,
                )
            );
            let curve = match spki.algorithm.parameters {
                Some(ID_SECP256R1_OID) => NamedCurve::P256,
                Some(ID_SECP384R1_OID) => NamedCurve::P384,
                Some(ID_SECP521R1_OID) => NamedCurve::P521,
                _ => {
                    return Err(Error::dom(
                        "unknown ECParameters",
                        DOMExceptionName::DataError,
                    ))
                },
            };
            ensure!(
                curve == algorithm.named_curve,
                Error::dom("EC curve mismatch", DOMExceptionName::DataError)
            );
            let group = EcGroup::from_curve_name(curve.nid())?;
            let mut ctx = BigNumContext::new()?;
            let public_key =
                EcPoint::from_bytes(&group, spki.subject_public_key.raw_bytes(), &mut ctx)
                    .and_then(|p| EcKey::from_public_key(&group, &p))
                    .map_err(|_| {
                        Error::dom("invalid SubjectPublicKey", DOMExceptionName::DataError)
                    })?;
            Ok(CryptoKey {
                kind: CryptoKeyKind::EcPublic {
                    algorithm,
                    key: EcPublicKey {
                        public_key: PKey::from_ec_key(public_key)?,
                    },
                },
                r#type: KeyType::Public,
                extractable,
                usages,
            })
        },
        ImportKeyInput::Pkcs8(der) => {
            check_usages_subset(&usages, private_usages)?;
            let pki = PKey::private_key_from_pkcs8(&der)
                .map_err(|_| Error::dom("invalid PublicKeyInfo", DOMExceptionName::DataError))?;
            let ec_key = pki
                .ec_key()
                .map_err(|_| Error::dom("not an EC key", DOMExceptionName::DataError))?;
            ensure!(
                ec_key.group().curve_name() == Some(algorithm.named_curve.nid()),
                Error::dom("EC curve mismatch", DOMExceptionName::DataError)
            );
            ec_key
                .check_key()
                .map_err(|_| Error::dom("invalid EC key", DOMExceptionName::DataError))?;
            Ok(CryptoKey {
                kind: CryptoKeyKind::EcPrivate {
                    algorithm,
                    key: EcPrivateKey { private_key: pki },
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
            jwk.check_kty("EC")?;
            let jwk_use = match algorithm.name {
                EcAlgorithm::Ecdsa => "sig",
                EcAlgorithm::Ecdh => "enc",
            };
            jwk.check_key_ops_and_use(&usages, jwk_use)?;
            jwk.check_ext(extractable)?;
            let curve = match jwk.crv.as_deref() {
                Some("P-256") => Some(NamedCurve::P256),
                Some("P-384") => Some(NamedCurve::P384),
                Some("P-521") => Some(NamedCurve::P521),
                _ => None,
            };
            ensure!(
                curve == Some(algorithm.named_curve),
                Error::dom("EC curve mismatch", DOMExceptionName::DataError)
            );
            // Only ECDSA requires "alg"; an ECDH JWK may legitimately carry a
            // JOSE key agreement algorithm such as "ECDH-ES".
            if algorithm.name == EcAlgorithm::Ecdsa {
                jwk.check_alg(match algorithm.named_curve {
                    NamedCurve::P256 => "ES256",
                    NamedCurve::P384 => "ES384",
                    NamedCurve::P521 => "ES512",
                })?;
            }
            let group = EcGroup::from_curve_name(algorithm.named_curve.nid())?;
            let mut ctx = BigNumContext::new()?;
            let (mut p, mut _a, mut _b) = (BigNum::new()?, BigNum::new()?, BigNum::new()?);
            group.components_gfp(&mut p, &mut _a, &mut _b, &mut ctx)?;
            let expected_len = p.num_bytes() as usize;
            let decode_coordinate = |coord: &Option<String>| {
                coord
                    .as_ref()
                    .and_then(|a| base64::decode_config(a, URL_SAFE_FORGIVING).ok())
                    .and_then(|a| {
                        // RFC 7518 6.2.1.2, 6.2.1.3:
                        // The length of this octet string MUST be the full size of a coordinate for
                        // the curve specified in the "crv" parameter.
                        if a.len() != expected_len {
                            return None;
                        }
                        BigNum::from_slice(&a).ok()
                    })
            };
            let coord_error = || {
                Error::dom(
                    "invalid EC public key coordinates",
                    DOMExceptionName::DataError,
                )
            };
            let x = decode_coordinate(&jwk.x).ok_or_else(coord_error)?;
            let y = decode_coordinate(&jwk.y).ok_or_else(coord_error)?;
            // This checks:
            // 1. that coordinates are canonical (in the range [0, p-1])
            // 2. that the cooordinates indeed lie on the curve
            let public_key = EcKey::from_public_key_affine_coordinates(&group, &x, &y)
                .map_err(|_| coord_error())?;
            if let Some(d) = jwk.d {
                let private_key = base64::decode_config(&d, URL_SAFE_FORGIVING)
                    .ok()
                    .and_then(|bytes| {
                        // RFC 7518 6.2.2.1:
                        // The length of this octet string MUST be ceiling(log-base-2(n)/8) octets
                        // (where n is the order of the curve).
                        if bytes.len() != group.order_bits().div_ceil(8) as usize {
                            return None;
                        }
                        BigNum::from_slice(&bytes).ok()
                    })
                    .and_then(|bn| {
                        EcKey::from_private_components(&group, &bn, public_key.public_key()).ok()
                    })
                    .ok_or_else(|| {
                        Error::dom("invalid EC private key", DOMExceptionName::DataError)
                    })?;
                private_key.check_key().map_err(|_| {
                    Error::dom("invalid EC private key", DOMExceptionName::DataError)
                })?;
                Ok(CryptoKey {
                    kind: CryptoKeyKind::EcPrivate {
                        algorithm,
                        key: EcPrivateKey {
                            private_key: PKey::from_ec_key(private_key)?,
                        },
                    },
                    r#type: KeyType::Private,
                    extractable,
                    usages,
                })
            } else {
                Ok(CryptoKey {
                    kind: CryptoKeyKind::EcPublic {
                        algorithm,
                        key: EcPublicKey {
                            public_key: PKey::from_ec_key(public_key)?,
                        },
                    },
                    r#type: KeyType::Public,
                    extractable,
                    usages,
                })
            }
        },
        ImportKeyInput::Raw(bytes) => {
            check_usages_subset(&usages, public_usages)?;
            let group = EcGroup::from_curve_name(algorithm.named_curve.nid())?;
            let mut ctx = BigNumContext::new()?;
            let public_key = EcPoint::from_bytes(&group, &bytes, &mut ctx)
                .and_then(|p| EcKey::from_public_key(&group, &p))
                .map_err(|_| Error::dom("invalid EC public key", DOMExceptionName::DataError))?;
            Ok(CryptoKey {
                kind: CryptoKeyKind::EcPublic {
                    algorithm,
                    key: EcPublicKey {
                        public_key: PKey::from_ec_key(public_key)?,
                    },
                },
                r#type: KeyType::Public,
                extractable,
                usages,
            })
        },
    }
}

fn public_jwt<T: HasPublic>(algorithm: &EcKeyAlgorithm, ec_key: &EcKey<T>) -> Result<JsonWebKey> {
    let group = ec_key.group();
    let mut ctx = BigNumContext::new()?;
    let (mut p, mut _a, mut _b) = (BigNum::new()?, BigNum::new()?, BigNum::new()?);
    group.components_gfp(&mut p, &mut _a, &mut _b, &mut ctx)?;
    let coord_len = p.num_bytes();
    let (mut x, mut y) = (BigNum::new()?, BigNum::new()?);
    ec_key
        .public_key()
        .affine_coordinates(group, &mut x, &mut y, &mut ctx)?;
    Ok(JsonWebKey {
        kty: Some("EC".to_owned()),
        crv: Some(
            match algorithm.named_curve {
                NamedCurve::P256 => "P-256",
                NamedCurve::P384 => "P-384",
                NamedCurve::P521 => "P-521",
            }
            .to_owned(),
        ),
        x: Some(base64::encode_config(
            x.to_vec_padded(coord_len)?,
            base64::URL_SAFE_NO_PAD,
        )),
        y: Some(base64::encode_config(
            y.to_vec_padded(coord_len)?,
            base64::URL_SAFE_NO_PAD,
        )),
        ..Default::default()
    })
}

impl EcPrivateKey {
    pub fn export_key(&self, algorithm: &EcKeyAlgorithm, format: KeyFormat) -> Result<KeyData> {
        match format {
            KeyFormat::Pkcs8 => Ok(KeyData::Raw(self.private_key.private_key_to_pkcs8()?)),
            KeyFormat::Jwk => {
                let ec_key = self.private_key.ec_key()?;
                let d = ec_key.private_key();
                Ok(KeyData::Jwk(JsonWebKey {
                    d: Some(base64::encode_config(
                        d.to_vec_padded(algorithm.named_curve.private_key_size() as i32)?,
                        base64::URL_SAFE_NO_PAD,
                    )),
                    ..public_jwt(algorithm, &ec_key)?
                }))
            },
            KeyFormat::Spki | KeyFormat::Raw => Err(Error::dom(
                "invalid export format for EC private key",
                DOMExceptionName::InvalidAccessError,
            )),
        }
    }

    pub fn sign(
        &self,
        params: EcdsaParams,
        algorithm: &EcKeyAlgorithm,
        _rng: &CryptoRng,
        data: &[u8],
    ) -> Result<Vec<u8>> {
        ensure!(
            algorithm.name == EcAlgorithm::Ecdsa,
            Error::dom(
                "invalid algorithm for key",
                DOMExceptionName::InvalidAccessError
            )
        );
        let mut signer = Signer::new(params.hash.openssl_message_digest(), &self.private_key)?;
        signer
            .len()
            .and_then(|len| {
                let mut der = vec![0; len];
                let len = signer.sign_oneshot(&mut der, data)?;
                der.truncate(len);
                // N.B.: The OpenSSL interface creates ASN.1 (DER) encoded
                // signatures; convert them to the fixed-length form specified
                // by WebCrypto
                let size = algorithm.named_curve.private_key_size();
                let sig = EcdsaSig::from_der(&der)?;
                let mut fixed_sig = Vec::with_capacity(size * 2);
                fixed_sig.extend_from_slice(&sig.r().to_vec_padded(size as i32)?);
                fixed_sig.extend_from_slice(&sig.s().to_vec_padded(size as i32)?);
                Ok(fixed_sig)
            })
            .map_err(|_| Error::dom("ECDSA signing failed", DOMExceptionName::OperationError))
    }
}

impl EcPublicKey {
    pub fn export_key(&self, algorithm: &EcKeyAlgorithm, format: KeyFormat) -> Result<KeyData> {
        match format {
            KeyFormat::Spki => Ok(KeyData::Raw(self.public_key.ec_key()?.public_key_to_der()?)),
            KeyFormat::Jwk => {
                let ec_key = self.public_key.ec_key()?;
                Ok(KeyData::Jwk(public_jwt(algorithm, &ec_key)?))
            },
            KeyFormat::Raw => {
                let ec_key = self.public_key.ec_key()?;
                let group = ec_key.group();
                let mut ctx = BigNumContext::new()?;
                Ok(KeyData::Raw(ec_key.public_key().to_bytes(
                    group,
                    PointConversionForm::UNCOMPRESSED,
                    &mut ctx,
                )?))
            },
            KeyFormat::Pkcs8 => Err(Error::dom(
                "invalid export format for EC public key",
                DOMExceptionName::InvalidAccessError,
            )),
        }
    }

    pub fn verify(
        &self,
        params: EcdsaParams,
        algorithm: &EcKeyAlgorithm,
        data: &[u8],
        signature: &[u8],
    ) -> Result<bool> {
        ensure!(
            algorithm.name == EcAlgorithm::Ecdsa,
            Error::dom(
                "invalid algorithm for key",
                DOMExceptionName::InvalidAccessError
            )
        );
        // Convert the signature back to ASN.1 so that it can be verified
        let size = algorithm.named_curve.private_key_size();
        if signature.len() != size * 2 {
            return Ok(false);
        }
        let (r, s) = signature.split_at(size);
        let sig =
            EcdsaSig::from_private_components(BigNum::from_slice(r)?, BigNum::from_slice(s)?)?;
        let der = sig.to_der()?;
        let mut verifier = Verifier::new(params.hash.openssl_message_digest(), &self.public_key)?;
        verifier.verify_oneshot(&der, data).map_err(|_| {
            Error::dom(
                "ECDSA verification failed",
                DOMExceptionName::OperationError,
            )
        })
    }
}

/// Parameters for ECDH deriveBits/deriveKey operations.
/// Contains the public key of the other party for the key agreement.
pub struct EcdhKeyDeriveParams<K> {
    pub public_key: K,
}

/// Perform ECDH key derivation to compute shared bits.
pub fn derive_bits(
    params: EcdhKeyDeriveParams<impl AsRef<CryptoKey>>,
    base_key: &CryptoKey,
    length: Option<usize>,
) -> Result<Vec<u8>> {
    // Get the private key from base_key
    let CryptoKeyKind::EcPrivate { algorithm, key } = &base_key.kind else {
        return Err(Error::dom(
            "Base key must be an EC private key",
            DOMExceptionName::InvalidAccessError,
        ));
    };

    // Ensure the algorithm is ECDH
    ensure!(
        algorithm.name == EcAlgorithm::Ecdh,
        Error::dom(
            "Base key algorithm must be ECDH",
            DOMExceptionName::InvalidAccessError
        )
    );

    // Get the public key from params
    let CryptoKeyKind::EcPublic {
        algorithm: public_algorithm,
        key: public_key,
    } = &params.public_key.as_ref().kind
    else {
        return Err(Error::dom(
            "Public key must be an EC public key",
            DOMExceptionName::InvalidAccessError,
        ));
    };

    // Ensure the public key algorithm is ECDH
    ensure!(
        public_algorithm.name == EcAlgorithm::Ecdh,
        Error::dom(
            "Public key algorithm must be ECDH",
            DOMExceptionName::InvalidAccessError
        )
    );

    // Ensure both keys use the same curve
    ensure!(
        algorithm.named_curve == public_algorithm.named_curve,
        Error::dom(
            "Private and public keys must use the same curve",
            DOMExceptionName::InvalidAccessError
        )
    );

    // Perform ECDH key agreement
    let mut deriver = Deriver::new(&key.private_key)?;
    deriver.set_peer(&public_key.public_key)?;
    let shared_secret = deriver.derive_to_vec().map_err(|_| {
        Error::dom(
            "ECDH key derivation failed",
            DOMExceptionName::OperationError,
        )
    })?;

    truncate_shared_secret(shared_secret, length)
}
