use std::{
    fmt,
    str::FromStr,
};

use anyhow::Context;
use rand::Rng;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct Secret {
    key: [u8; 32],
}

impl TryFrom<&str> for Secret {
    type Error = anyhow::Error;

    fn try_from(s: &str) -> anyhow::Result<Self> {
        let key: [u8; 32] = const_hex::decode(s)
            .context("Couldn't hexdecode key")?
            .try_into()
            .map_err(|e: Vec<u8>| {
                anyhow::anyhow!("Hex-decoded key was {} bytes, not 32", e.len())
            })?;
        Ok(Self { key })
    }
}

impl From<[u8; 32]> for Secret {
    fn from(key: [u8; 32]) -> Self {
        Self { key }
    }
}

impl TryFrom<Vec<u8>> for Secret {
    type Error = anyhow::Error;

    fn try_from(v: Vec<u8>) -> anyhow::Result<Self> {
        let key: [u8; 32] = v
            .try_into()
            .map_err(|e: Vec<u8>| anyhow::anyhow!("Key was {} bytes, not 32", e.len()))?;
        Ok(Self { key })
    }
}

impl Secret {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }

    pub fn random() -> Self {
        Self {
            key: rand::rng().random(),
        }
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", const_hex::encode(self.key))
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secret")
            .field("key", &const_hex::encode(self.key))
            .finish()
    }
}

impl FromStr for Secret {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

pub type DeploymentSecret = Secret;
