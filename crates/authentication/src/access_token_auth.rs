use async_trait::async_trait;
use common::types::{
    DeploymentCredential,
    LegacyAdminKey,
};
use errors::ErrorMetadata;
use keybroker::Identity;

pub enum LegacyAdminKeyVerdict {
    Accepted(Identity),
    /// The deployment's team opted out of legacy admin keys.
    DisabledForTeam,
    /// Legacy admin keys are no longer available to the deployment's team.
    Obsolete,
    CheckLocally,
}

/// Logic to check authorization based on Access Token
#[async_trait]
pub trait AccessTokenAuth: Send + Sync {
    async fn is_authorized(&self, access_token: &DeploymentCredential) -> anyhow::Result<Identity>;

    /// Only called for keys that already decrypt to a member admin identity
    /// for this deployment. An error rejects the key.
    async fn check_legacy_admin_key(
        &self,
        _admin_key: &LegacyAdminKey,
    ) -> anyhow::Result<LegacyAdminKeyVerdict> {
        Ok(LegacyAdminKeyVerdict::CheckLocally)
    }
}
pub struct NullAccessTokenAuth;

#[async_trait]
impl AccessTokenAuth for NullAccessTokenAuth {
    async fn is_authorized(
        &self,
        _access_token: &DeploymentCredential,
    ) -> anyhow::Result<Identity> {
        anyhow::bail!(ErrorMetadata::unauthenticated(
            "BadAdminKey",
            "The provided admin key was invalid for this instance",
        ))
    }
}
