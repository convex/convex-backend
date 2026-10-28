/// Represents access to an RNG suitable for cryptographic operations like key
/// generation, i.e. system randomness.
///
/// Callers that run in deterministic contexts can withhold this token to make
/// operations requiring randomness fail.
pub struct CryptoRng(());

impl CryptoRng {
    pub fn new() -> Self {
        CryptoRng(())
    }

    /// Returns a `aws_lc_rs`-compatible random number generator
    pub fn aws_lc(&self) -> aws_lc_rs::rand::SystemRandom {
        aws_lc_rs::rand::SystemRandom::new()
    }
}

impl Default for CryptoRng {
    fn default() -> Self {
        Self::new()
    }
}
