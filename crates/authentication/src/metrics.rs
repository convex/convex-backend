use metrics::{
    log_counter_with_labels,
    log_distribution_with_labels,
    register_convex_counter,
    register_convex_histogram,
    StaticMetricLabel,
};

pub(crate) const OPENIDCONNECT: &str = "openidconnect";
pub(crate) const CUSTOMJWT: &str = "customjwt";

register_convex_histogram!(
    pub(crate) JWKS_RESPONSE_SIZE_BYTES,
    "Size in bytes of JWKS response bodies, including decoded data URLs",
    &["provider"]
);
register_convex_histogram!(
    pub(crate) JWKS_KEYS_TOTAL,
    "Number of deserialized keys in JWKS responses",
    &["provider"]
);
register_convex_counter!(
    pub(crate) JWKS_ALGORITHM_TOTAL,
    "Count of recognized algorithms advertised by deserialized JWKS keys",
    &["provider", "algorithm"]
);
register_convex_counter!(
    pub(crate) JWT_ALGORITHM_TOTAL,
    "Count of recognized algorithms in deserialized JWT headers",
    &["provider", "algorithm"]
);

// Bound label cardinality as the verification libraries add algorithm variants.
const RECOGNIZED_ALGORITHMS: &[&str] = &[
    "HS256", "HS384", "HS512", "RS256", "RS384", "RS512", "ES256", "ES384", "ES512", "ES256K",
    "PS256", "PS384", "PS512", "EdDSA", "Ed25519", "Ed448", "none",
];

pub(crate) fn log_jwks_response_size(provider: &'static str, size: usize) {
    log_distribution_with_labels(
        &JWKS_RESPONSE_SIZE_BYTES,
        size as f64,
        vec![StaticMetricLabel::new("provider", provider)],
    );
}

pub(crate) fn log_jwks_keys(provider: &'static str, keys: usize) {
    log_distribution_with_labels(
        &JWKS_KEYS_TOTAL,
        keys as f64,
        vec![StaticMetricLabel::new("provider", provider)],
    );
}

pub(crate) fn log_jwks_algorithm(provider: &'static str, algorithm: &impl serde::Serialize) {
    log_algorithm(&JWKS_ALGORITHM_TOTAL, provider, algorithm);
}

pub(crate) fn log_jwt_algorithm(provider: &'static str, algorithm: &impl serde::Serialize) {
    log_algorithm(&JWT_ALGORITHM_TOTAL, provider, algorithm);
}

fn log_algorithm(
    counter: &metrics::prometheus::IntCounterVec,
    provider: &'static str,
    algorithm: &impl serde::Serialize,
) {
    // Serde preserves the JOSE wire name for both libraries' algorithm enums.
    let Ok(serde_json::Value::String(algorithm)) = serde_json::to_value(algorithm) else {
        return;
    };
    if let Some(algorithm) = RECOGNIZED_ALGORITHMS
        .iter()
        .copied()
        .find(|&recognized| recognized == algorithm)
    {
        log_counter_with_labels(
            counter,
            1,
            vec![
                StaticMetricLabel::new("provider", provider),
                StaticMetricLabel::new("algorithm", algorithm),
            ],
        );
    }
}

register_convex_counter!(pub DEPLOY_KEY_USE_TOTAL, "Count of deploy key uses", &["key_type"]);

pub enum DeployKeyType {
    Legacy,
    System,
    Unknown,
    AccessToken,
}

pub fn log_deploy_key_use(key_type: DeployKeyType) {
    let key_type_label = match key_type {
        DeployKeyType::Legacy => "legacy",
        DeployKeyType::System => "system",
        DeployKeyType::AccessToken => "access_token",
        DeployKeyType::Unknown => "unknown",
    };
    log_counter_with_labels(
        &DEPLOY_KEY_USE_TOTAL,
        1,
        vec![StaticMetricLabel::new("key_type", key_type_label)],
    );
}
