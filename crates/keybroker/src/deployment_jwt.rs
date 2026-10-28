//! ES256 JWTs signed by a key derived from the deployment's secret.
//!
//! Every token's `iss` identifies its signer. The app signs under the
//! deployment's origin and a component under `{origin}/components/{path}`.
//!
//! The deployment publishes the public keys at `/.well-known/jwks.json`.
//! `customJwt` auth providers as well as third-parties may use it to verify
//! the signatures of issued JWTs.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{
        Duration,
        SystemTime,
    },
};

use anyhow::Context;
use aws_lc_rs::{
    agreement,
    digest,
    rand::SystemRandom,
    signature::{
        EcdsaKeyPair,
        KeyPair,
        ECDSA_P256_SHA256_FIXED_SIGNING,
    },
};
use biscuit::{
    jwa::{
        Algorithm,
        SignatureAlgorithm,
    },
    jwk::{
        AlgorithmParameters,
        CommonParameters,
        EllipticCurve,
        EllipticCurveKeyParameters,
        EllipticCurveKeyType,
        JWKSet,
        PublicKeyUse,
        JWK,
    },
    Empty,
};
use common::{
    components::ComponentPath,
    knobs::DEPLOYMENT_JWT_MAX_LIFETIME,
};
use errors::ErrorMetadata;
use parking_lot::Mutex;
use serde_json::{
    Map as JsonMap,
    Value as JsonValue,
};

use crate::{
    encryptor::derive_bytes,
    secret::Secret,
};

/// The header `typ` on every token, as RFC 7519 recommends.
const TYP: &str = "JWT";

/// Path under the deployment origin that component issuers live at.
const COMPONENT_ISSUER_PREFIX: &str = "/components/";

/// Label for the root every generation's signing key is derived from.
const ROOT_LABEL: &[u8] = b"deployment jwt signing root";

/// Label for one generation's key, followed by the generation and an attempt
/// counter, both big-endian.
const KEY_LABEL: &[u8] = b"deployment jwt signing key";

/// Claim-name prefixes reserved for the framework, compared ASCII
/// case-insensitively so no casing of them can be squatted.
const RESERVED_CLAIM_PREFIXES: &[&str] = &["convex", "cvx"];

/// The generation that signs tokens and that the JWKS publishes.
const SIGNING_GENERATION: u32 = 0;

/// Largest encoded token, in bytes, that `ctx.auth.signJwt` may produce. Keeps
/// tokens within what clients can send as an auth header.
const DEPLOYMENT_JWT_MAX_SIZE: usize = 8000;

/// What a caller asks to be signed.
///
/// The `claims` and `expirse_in` values come from user code.
///
/// Convex sets `iss`, `iat` and `exp`, so `claims` must not contain them, and
/// claim names starting with `convex` or `cvx` are reserved for the framework.
/// Both get enforced at runtime and user-facing APIs should discourage/prevent
/// them from being passed in.
pub struct DeploymentJwtRequest {
    pub origin: String,
    pub signer: ComponentPath,
    pub claims: JsonMap<String, JsonValue>,
    pub expires_in: Duration,
}

/// Every generation of the deployment's signing key, derived on demand from one
/// root, so changing generations stores no key material.
#[derive(Clone)]
pub struct DeploymentJwtKeys {
    root: Secret,
    cache: Arc<Mutex<BTreeMap<u32, DeploymentJwtKey>>>,
}

impl DeploymentJwtKeys {
    pub fn derive_from_secret(secret: &Secret) -> anyhow::Result<Self> {
        let mut root = [0; 32];
        derive_bytes(secret, ROOT_LABEL, &mut root)?;
        Ok(Self::from_root_key_bytes(root))
    }

    /// Rebuilds the keys from [`Self::root_key_bytes`], e.g. in a function
    /// runner that never sees the deployment secret. The root can sign as any
    /// generation.
    pub fn from_root_key_bytes(root: [u8; 32]) -> Self {
        Self {
            root: Secret::from(root),
            cache: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn root_key_bytes(&self) -> [u8; 32] {
        *self.root.as_bytes()
    }

    /// The key for one generation.
    pub fn generation(&self, generation: u32) -> anyhow::Result<DeploymentJwtKey> {
        let mut cache = self.cache.lock();
        if let Some(key) = cache.get(&generation) {
            return Ok(key.clone());
        }
        let key = DeploymentJwtKey::derive(&self.root, generation)?;
        cache.insert(generation, key.clone());
        Ok(key)
    }

    pub fn mint(&self, request: DeploymentJwtRequest, now: SystemTime) -> anyhow::Result<String> {
        self.generation(SIGNING_GENERATION)?.mint(request, now)
    }

    /// Public keys that verifiers, including the deployment's own `customJwt`
    /// providers, check tokens against.
    pub fn jwks(&self) -> anyhow::Result<JWKSet<Empty>> {
        Ok((*self.generation(SIGNING_GENERATION)?.jwks).clone())
    }
}

/// One generation of the deployment's signing key.
#[derive(Clone)]
pub struct DeploymentJwtKey {
    key_pair: Arc<EcdsaKeyPair>,
    kid: String,
    jwks: Arc<JWKSet<Empty>>,
}

impl DeploymentJwtKey {
    fn derive(root: &Secret, generation: u32) -> anyhow::Result<Self> {
        // About 1 in 2^32 derived scalars is out of range for P-256, so retry
        // with a counter in the label.
        for attempt in 0..=u8::MAX {
            let mut label = KEY_LABEL.to_vec();
            label.extend_from_slice(&generation.to_be_bytes());
            label.push(attempt);
            let mut private_key = [0; 32];
            derive_bytes(root, &label, &mut private_key)?;
            if let Ok(key) = Self::from_private_key_bytes(private_key) {
                return Ok(key);
            }
        }
        anyhow::bail!("Could not derive a valid P-256 key")
    }

    fn from_private_key_bytes(private_key: [u8; 32]) -> anyhow::Result<Self> {
        // `EcdsaKeyPair` loads a raw scalar only alongside its public point, which
        // ECDH computes from the scalar: both are the same P-256 key.
        let public_key =
            agreement::PrivateKey::from_private_key(&agreement::ECDH_P256, &private_key)
                .map_err(|e| anyhow::anyhow!("Invalid P-256 private key: {e}"))?
                .compute_public_key()
                .map_err(|_| anyhow::anyhow!("Could not compute P-256 public key"))?;
        let key_pair = EcdsaKeyPair::from_private_key_and_public_key(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            &private_key,
            public_key.as_ref(),
        )
        .map_err(|e| anyhow::anyhow!("Invalid P-256 private key: {e}"))?;

        // Uncompressed SEC1 point: 0x04 || x || y.
        let public_key = key_pair.public_key().as_ref();
        anyhow::ensure!(public_key.len() == 65 && public_key[0] == 0x04);
        let x = public_key[1..33].to_vec();
        let y = public_key[33..65].to_vec();
        let kid = jwk_thumbprint(&x, &y);
        let jwk = JWK {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                algorithm: Some(Algorithm::Signature(SignatureAlgorithm::ES256)),
                key_id: Some(kid.clone()),
                ..Default::default()
            },
            algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
                key_type: EllipticCurveKeyType::EC,
                curve: EllipticCurve::P256,
                x,
                y,
                d: None,
            }),
            additional: Empty {},
        };
        Ok(Self {
            key_pair: Arc::new(key_pair),
            kid,
            jwks: Arc::new(JWKSet { keys: vec![jwk] }),
        })
    }

    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// Public keys that verify tokens from this deployment.
    pub fn jwks(&self) -> &JWKSet<Empty> {
        &self.jwks
    }

    /// Return a signed JWT based on the given `request`.
    ///
    /// This function will populate the `iss`, `iat` and `exp` claims.
    ///
    /// The `iss` in particular allows verifying code to know that code at a
    /// certain component path generated the JWT. That value can't be passed
    /// in from the outside.
    pub fn mint(&self, request: DeploymentJwtRequest, now: SystemTime) -> anyhow::Result<String> {
        let DeploymentJwtRequest {
            origin,
            signer,
            mut claims,
            expires_in,
        } = request;
        for reserved in ["iss", "iat", "exp"] {
            if claims.contains_key(reserved) {
                anyhow::bail!(invalid_request(format!(
                    "The '{reserved}' claim is set by Convex and can't be provided"
                )));
            }
        }
        if let Some(name) = claims.keys().find(|name| {
            RESERVED_CLAIM_PREFIXES.iter().any(|prefix| {
                // Compare bytes: slicing the str could panic mid-character.
                name.as_bytes()
                    .get(..prefix.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
            })
        }) {
            anyhow::bail!(invalid_request(format!(
                "The '{name}' claim can't be provided: claim names starting with 'convex' or \
                 'cvx' are reserved"
            )));
        }
        // RFC 7519 §4.1.2 makes `sub` a string, and `customJwt` providers reject
        // tokens whose `sub` isn't one.
        if let Some(sub) = claims.get("sub")
            && !sub.as_str().is_some_and(|sub| !sub.is_empty())
        {
            anyhow::bail!(invalid_request(
                "The 'sub' claim must be a non-empty string"
            ));
        }

        // The `nbf` claim is a timestamp related to `iat` and `exp` which we set
        // internally. Rather than allowing it and validating it against the
        // values we set, we just disallow it at this point.
        if claims.contains_key("nbf") {
            anyhow::bail!(invalid_request(
                "The 'nbf' claim is reserved by Convex and can't be provided"
            ));
        }
        if expires_in.is_zero() || expires_in > *DEPLOYMENT_JWT_MAX_LIFETIME {
            anyhow::bail!(invalid_request(format!(
                "expiresIn must be between 1 second and {} seconds",
                DEPLOYMENT_JWT_MAX_LIFETIME.as_secs()
            )));
        }
        let issued_at = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("System time before Unix epoch")?
            .as_secs();
        claims.insert(
            "iss".to_string(),
            deployment_jwt_issuer(&origin, &signer).into(),
        );
        claims.insert("iat".to_string(), issued_at.into());
        claims.insert("exp".to_string(), (issued_at + expires_in.as_secs()).into());

        let header = serde_json::json!({ "alg": "ES256", "typ": TYP, "kid": self.kid });
        let signing_input = format!(
            "{}.{}",
            base64::encode_config(serde_json::to_vec(&header)?, base64::URL_SAFE_NO_PAD),
            base64::encode_config(serde_json::to_vec(&claims)?, base64::URL_SAFE_NO_PAD),
        );
        let signature = self
            .key_pair
            .sign(&SystemRandom::new(), signing_input.as_bytes())
            .map_err(|_| anyhow::anyhow!("ES256 signing failed"))?;
        let token = format!(
            "{signing_input}.{}",
            base64::encode_config(signature.as_ref(), base64::URL_SAFE_NO_PAD)
        );
        if token.len() > DEPLOYMENT_JWT_MAX_SIZE {
            anyhow::bail!(invalid_request(format!(
                "The token is {} bytes, over the {} byte limit",
                token.len(),
                DEPLOYMENT_JWT_MAX_SIZE
            )));
        }
        Ok(token)
    }
}

/// The `iss` for tokens signed by `signer`, under the deployment's
/// `CONVEX_CLOUD_URL`.
pub fn deployment_jwt_issuer(origin: &str, signer: &ComponentPath) -> String {
    let origin = origin.trim_end_matches('/');
    if signer.is_root() {
        origin.to_string()
    } else {
        format!("{origin}{COMPONENT_ISSUER_PREFIX}{signer}")
    }
}

/// RFC 7638 thumbprint of a P-256 public key.
fn jwk_thumbprint(x: &[u8], y: &[u8]) -> String {
    // Members in lexicographic order with no whitespace, as the RFC requires.
    let canonical = format!(
        r#"{{"crv":"P-256","kty":"EC","x":"{}","y":"{}"}}"#,
        base64::encode_config(x, base64::URL_SAFE_NO_PAD),
        base64::encode_config(y, base64::URL_SAFE_NO_PAD),
    );
    let hash = digest::digest(&digest::SHA256, canonical.as_bytes());
    base64::encode_config(hash.as_ref(), base64::URL_SAFE_NO_PAD)
}

fn invalid_request(msg: impl Into<std::borrow::Cow<'static, str>>) -> ErrorMetadata {
    ErrorMetadata::bad_request("InvalidJwtRequest", msg)
}
