//! RS256 signing and verification. Callers supply claim-validation policy.

use std::{
    collections::HashSet,
    sync::Arc,
    time::SystemTime,
};

use anyhow::{
    bail,
    ensure,
    Context,
};
use biscuit::{
    jwa::{
        Algorithm,
        SignatureAlgorithm,
    },
    jwk::{
        AlgorithmParameters,
        JWKSet,
        PublicKeyUse,
        JWK,
    },
    jws::{
        Header,
        RegisteredHeader,
    },
    ClaimsSet,
    Compact,
    Empty,
    ValidationOptions,
    JWT,
};
use chrono::{
    DateTime,
    Utc,
};
use rsa::{
    pkcs1v15::SigningKey,
    signature::{
        SignatureEncoding,
        Signer,
    },
    BigUint,
    RsaPrivateKey,
    RsaPublicKey,
};
use serde::{
    de::DeserializeOwned,
    Serialize,
};
use sha2::Sha256;

/// Limits decoding work on untrusted tokens before parsing.
pub const MAX_JWT_SIZE: usize = 8 * 1024;

const MIN_RSA_KEY_BITS: u64 = 2048;
const MAX_KEY_ID_LENGTH: usize = 128;
pub(crate) const MAX_VERIFICATION_KEYS: usize = 16;

/// A credential; access its contents explicitly through `as_str` or
/// `into_string`.
pub struct Jwt(String);

impl Jwt {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum JwtError {
    #[error("could not sign JWT")]
    SigningFailed,
    #[error("JWT exceeds the maximum size")]
    TokenTooLarge,
    #[error("invalid JWT")]
    InvalidToken,
}

/// Reads a JWT's registered expiration without verifying its signature.
/// This is suitable for cache freshness only; authentication must use a
/// [`JwtVerifier`].
pub fn unverified_expiration(token: &str) -> Result<SystemTime, JwtError> {
    if token.len() > MAX_JWT_SIZE {
        return Err(JwtError::TokenTooLarge);
    }
    let token = JWT::<Empty, Empty>::new_encoded(token);
    token
        .unverified_header()
        .map_err(|_| JwtError::InvalidToken)?;
    token.signature().map_err(|_| JwtError::InvalidToken)?;
    let payload = token
        .unverified_payload()
        .map_err(|_| JwtError::InvalidToken)?;
    let expiration: DateTime<Utc> = payload
        .registered
        .expiry
        .ok_or(JwtError::InvalidToken)?
        .into();
    Ok(expiration.into())
}

/// Owns private key material, which must be kept out of logs.
#[derive(Clone)]
pub struct JwtSigner {
    key_id: String,
    signing_key: Arc<SigningKey<Sha256>>,
}

impl PartialEq for JwtSigner {
    fn eq(&self, other: &Self) -> bool {
        // Matching modulus and exponent identify the same RSA key, so the public
        // half detects a rotated key. `SigningKey` has no comparison of its own.
        fn public_half(signing_key: &SigningKey<Sha256>) -> RsaPublicKey {
            AsRef::<RsaPrivateKey>::as_ref(signing_key).to_public_key()
        }
        self.key_id == other.key_id
            && public_half(&self.signing_key) == public_half(&other.signing_key)
    }
}

impl JwtSigner {
    /// Loads one RSA signing key from a private JWK.
    ///
    /// The JWK must declare `alg=RS256`, a signature purpose, a bounded `kid`,
    /// and at least a 2048-bit modulus.
    pub fn new(private_jwk: JWK<Empty>) -> anyhow::Result<Self> {
        let key_id = rs256_signature_key_id(&private_jwk)
            .context("invalid JWT signing key")?
            .to_owned();
        let AlgorithmParameters::RSA(parameters) = private_jwk.algorithm else {
            bail!("invalid JWT signing key");
        };
        ensure!(
            parameters.n.bits() >= MIN_RSA_KEY_BITS && parameters.other_primes_info.is_none(),
            "invalid JWT signing key"
        );
        let (Some(d), Some(p), Some(q)) = (parameters.d, parameters.p, parameters.q) else {
            bail!("invalid JWT signing key");
        };
        let private_key = RsaPrivateKey::from_components(
            BigUint::from_bytes_be(&parameters.n.to_bytes_be()),
            BigUint::from_bytes_be(&parameters.e.to_bytes_be()),
            BigUint::from_bytes_be(&d.to_bytes_be()),
            vec![
                BigUint::from_bytes_be(&p.to_bytes_be()),
                BigUint::from_bytes_be(&q.to_bytes_be()),
            ],
        )
        .context("invalid JWT signing key")?;
        private_key.validate().context("invalid JWT signing key")?;
        Ok(Self {
            key_id,
            signing_key: Arc::new(SigningKey::new(private_key)),
        })
    }

    /// Signs a claim set the caller has already assembled.
    ///
    /// `DeserializeOwned` is required because Biscuit's compact representation
    /// is bidirectional, even though signing only writes.
    pub fn sign<C: Serialize + DeserializeOwned>(
        &self,
        claims: &ClaimsSet<C>,
    ) -> Result<Jwt, JwtError> {
        let header = Header::from_registered_header(RegisteredHeader {
            algorithm: SignatureAlgorithm::RS256,
            key_id: Some(self.key_id.clone()),
            ..Default::default()
        });
        let mut compact = Compact::with_capacity(3);
        compact.push(&header).map_err(|_| JwtError::SigningFailed)?;
        compact.push(claims).map_err(|_| JwtError::SigningFailed)?;
        let signing_input = compact.encode();
        let signature = self
            .signing_key
            .try_sign(signing_input.as_bytes())
            .map_err(|_| JwtError::SigningFailed)?;
        compact
            .push(&signature.to_vec())
            .map_err(|_| JwtError::SigningFailed)?;
        Ok(Jwt(compact.encode()))
    }
}

/// Verifies untrusted tokens using public keys only.
///
/// Equality compares the public key sets, letting `ConfigLoader` skip
/// publishing reloads whose keys are unchanged.
#[derive(Clone, PartialEq)]
pub struct JwtVerifier {
    public_keys: JWKSet<Empty>,
}

impl JwtVerifier {
    /// Loads a bounded public-key set.
    ///
    /// Multiple unique key IDs allow a current and previous key to overlap
    /// during a deployment-driven rotation.
    pub fn new(public_keys: JWKSet<Empty>) -> anyhow::Result<Self> {
        ensure!(
            !public_keys.keys.is_empty(),
            "JWT verifier requires at least one verification key"
        );
        ensure!(
            public_keys.keys.len() <= MAX_VERIFICATION_KEYS,
            "JWT verifier received {} verification keys; maximum is {MAX_VERIFICATION_KEYS}",
            public_keys.keys.len()
        );
        let mut key_ids = HashSet::with_capacity(public_keys.keys.len());
        for key in &public_keys.keys {
            let key_id = validate_public_verification_key(key)?;
            ensure!(
                key_ids.insert(key_id),
                "duplicate JWT verification key ID: {key_id}"
            );
        }
        Ok(Self { public_keys })
    }

    /// Checks the signature and the supplied validation options, then hands the
    /// claims back for the caller to apply its own policy to.
    pub fn verify<C>(
        &self,
        token: &str,
        options: ValidationOptions,
    ) -> Result<ClaimsSet<C>, JwtError>
    where
        C: Serialize + DeserializeOwned,
    {
        if token.len() > MAX_JWT_SIZE {
            return Err(JwtError::TokenTooLarge);
        }
        let token = JWT::<C, Empty>::new_encoded(token);
        let decoded = token
            .decode_with_jwks(&self.public_keys, Some(SignatureAlgorithm::RS256))
            .map_err(|_| JwtError::InvalidToken)?;
        decoded
            .validate(options)
            .map_err(|_| JwtError::InvalidToken)?;
        let JWT::Decoded { payload, .. } = decoded else {
            return Err(JwtError::InvalidToken);
        };
        Ok(payload)
    }
}

fn rs256_signature_key_id(jwk: &JWK<Empty>) -> Option<&str> {
    let has_required_purpose = jwk.common.public_key_use == Some(PublicKeyUse::Signature)
        && jwk.common.key_operations.is_none();
    if jwk.common.algorithm != Some(Algorithm::Signature(SignatureAlgorithm::RS256))
        || !has_required_purpose
    {
        return None;
    }
    jwk.common
        .key_id
        .as_deref()
        .filter(|key_id| !key_id.is_empty() && key_id.len() <= MAX_KEY_ID_LENGTH)
}

fn validate_public_verification_key(key: &JWK<Empty>) -> anyhow::Result<&str> {
    let key_id = rs256_signature_key_id(key).context("invalid JWT verification key")?;
    let AlgorithmParameters::RSA(parameters) = &key.algorithm else {
        bail!("invalid JWT verification key");
    };
    ensure!(
        parameters.n.bits() >= MIN_RSA_KEY_BITS
            && parameters.d.is_none()
            && parameters.p.is_none()
            && parameters.q.is_none()
            && parameters.dp.is_none()
            && parameters.dq.is_none()
            && parameters.qi.is_none()
            && parameters.other_primes_info.is_none(),
        "invalid JWT verification key"
    );
    RsaPublicKey::new(
        BigUint::from_bytes_be(&parameters.n.to_bytes_be()),
        BigUint::from_bytes_be(&parameters.e.to_bytes_be()),
    )
    .context("invalid JWT verification key")?;
    Ok(key_id)
}
