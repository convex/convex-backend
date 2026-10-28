use std::collections::HashSet;

use indexmap::IndexSet;
use itertools::Itertools as _;
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    ensure,
    DOMExceptionName,
    Error,
    KeyUsage,
    Result,
};

#[derive(Deserialize, Serialize, Default)]
pub struct JsonWebKey {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#use: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_ops: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dq: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qi: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oth: Option<Vec<RsaOtherPrimesInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct RsaOtherPrimesInfo {
    // The following fields are defined in Section 6.3.2.7 of JSON Web Algorithms
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
}

impl JsonWebKey {
    pub fn check_kty(&self, kty: &str) -> Result<()> {
        ensure!(
            self.kty.as_deref() == Some(kty),
            Error::dom(
                format!("JWK \"kty\" must be {kty:?}"),
                DOMExceptionName::DataError
            ),
        );
        Ok(())
    }

    pub fn check_crv(&self, crv: &str) -> Result<()> {
        ensure!(
            self.crv.as_deref() == Some(crv),
            Error::dom(
                format!("JWK \"crv\" must be {crv:?}"),
                DOMExceptionName::DataError
            ),
        );
        Ok(())
    }

    pub fn check_ext(&self, extractable: bool) -> Result<()> {
        if let Some(false) = self.ext
            && extractable
        {
            return Err(Error::dom(
                "JWK \"ext\" must be true",
                DOMExceptionName::DataError,
            ));
        }
        Ok(())
    }

    pub fn check_key_ops_and_use(
        &self,
        key_usages: &IndexSet<KeyUsage>,
        expected_use: &str,
    ) -> Result<()> {
        if !key_usages.is_empty()
            && let Some(r#use) = &self.r#use
        {
            ensure!(
                r#use == expected_use,
                Error::dom(
                    format!("JWK \"use\" must be {expected_use:?}"),
                    DOMExceptionName::DataError
                ),
            );
        }
        if let Some(key_ops) = &self.key_ops {
            ensure!(
                key_ops.iter().all_unique(),
                Error::dom(
                    "JWK \"key_ops\" must not contain duplicates",
                    DOMExceptionName::DataError
                ),
            );
            let allowed_usages: HashSet<_> = key_ops
                .iter()
                .filter_map(|s| s.parse::<KeyUsage>().ok())
                .collect();
            for usage in key_usages {
                ensure!(
                    allowed_usages.contains(usage),
                    Error::dom(
                        format!("JWK \"key_ops\" does not contain {usage:?}"),
                        DOMExceptionName::DataError
                    ),
                );
            }
        }
        Ok(())
    }

    pub fn check_alg_oneof(&self, expected: &[&str]) -> Result<()> {
        if let Some(alg) = &self.alg {
            ensure!(
                expected.contains(&alg.as_str()),
                Error::dom(
                    format!(
                        "JWK \"alg\" must be {}",
                        expected.iter().map(|e| format!("{e:?}")).join(" or ")
                    ),
                    DOMExceptionName::DataError
                ),
            );
        }
        Ok(())
    }

    pub fn check_alg(&self, expected: &str) -> Result<()> {
        self.check_alg_oneof(&[expected])
    }
}
