//! The `_udf_config` table has a single row with the global configuration
//! for the UDF runtime.

use anyhow::Context;
use common::runtime::UnixTimestamp;
use semver::Version;
use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

#[derive(Debug, Clone, PartialEq)]
pub struct UdfConfig {
    /// What is the version of `convex` in a developer's
    /// "package.json" when they push their UDFs? We currently allow this to
    /// go forwards and backwards, but we'll eventually want this to only
    /// move forwards. All of the developers pushing to an instance should be on
    /// the same version.
    pub server_version: Version,
    pub import_phase_rng_seed: [u8; 32],
    pub import_phase_unix_timestamp: UnixTimestamp,
}

impl UdfConfig {
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedUdfConfig {
    server_version: String,
    #[serde(with = "serde_bytes")]
    import_phase_rng_seed: [u8; 32],
    import_phase_unix_timestamp: i64,
}

impl TryFrom<UdfConfig> for SerializedUdfConfig {
    type Error = anyhow::Error;

    fn try_from(config: UdfConfig) -> anyhow::Result<Self> {
        Ok(SerializedUdfConfig {
            server_version: config.server_version.to_string(),
            import_phase_rng_seed: config.import_phase_rng_seed,
            import_phase_unix_timestamp: config
                .import_phase_unix_timestamp
                .as_nanos()
                .try_into()
                .context("Unix timestamp past 2262")?,
        })
    }
}

impl TryFrom<SerializedUdfConfig> for UdfConfig {
    type Error = anyhow::Error;

    fn try_from(value: SerializedUdfConfig) -> anyhow::Result<Self> {
        let server_version = Version::parse(&value.server_version)?;
        let import_phase_rng_seed = value.import_phase_rng_seed;
        let import_phase_unix_timestamp = {
            let nanos = value.import_phase_unix_timestamp;
            anyhow::ensure!(nanos >= 0, "UnixTimestamp before the unix epoch");
            UnixTimestamp::from_nanos(nanos as u64)
        };
        Ok(Self {
            server_version,
            import_phase_rng_seed,
            import_phase_unix_timestamp,
        })
    }
}

codegen_convex_serialization!(UdfConfig, SerializedUdfConfig);
