/// Represents access to an RNG suitable for cryptographic operations like key
/// generation, i.e. system randomness.
///
/// This is unavailable in deterministic UDFs (i.e. queries/mutations).
pub use webcrypto::CryptoRng;
