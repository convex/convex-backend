use std::fmt::Formatter;

use headers::Authorization;
use serde::{
    Deserialize,
    Serialize,
};
use sync_types::headers::ConvexAdminAuthorization;
use utoipa::ToSchema;

/// Encrypted system key
#[derive(Clone)]
pub struct SystemKey(String);

impl SystemKey {
    // We're not using `Display` to avoid accidentally printing the key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SystemKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str("SystemKey(*****)")
    }
}

/// Raw credential supplied for deployment authorization.
///
/// This can be a legacy admin key, an internal system key, a WorkOS session
/// token, or a Convex access token such as a deploy key or a personal, team,
/// or project token.
#[derive(Serialize, Deserialize, Clone)]
pub struct RawDeploymentCredential(String);

impl RawDeploymentCredential {
    pub fn new(credential: String) -> Self {
        Self(credential)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for RawDeploymentCredential {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str("RawDeploymentCredential(*****)")
    }
}

/// A credential classified for deployment authorization. Each variant still
/// requires the corresponding authorization check.
#[derive(Debug)]
pub enum DeploymentCredential {
    LegacyAdminKey(LegacyAdminKey),
    SystemKey(SystemKey),
    AccessToken(DeploymentAccessToken),
    DeployKey(DeployKey),
}

/// An encrypted key for a deployment admin identity.
#[derive(Clone, Serialize, ToSchema)]
#[schema(deprecated)]
pub struct LegacyAdminKey(String);

impl LegacyAdminKey {
    pub fn new(key: String) -> Self {
        Self(key)
    }

    pub fn as_header(&self) -> anyhow::Result<Authorization<ConvexAdminAuthorization>> {
        Ok(Authorization(ConvexAdminAuthorization::from_admin_key(
            &self.0,
        )?))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_string(&self) -> String {
        self.0.clone()
    }
}

impl std::fmt::Debug for LegacyAdminKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str("LegacyAdminKey(*****)")
    }
}

/// An access token with a deployment or project routing prefix.
#[derive(Serialize, ToSchema)]
pub struct DeployKey(String);

impl DeployKey {
    pub fn new(key: String) -> Self {
        Self(key)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_string(&self) -> String {
        self.0.clone()
    }
}

impl std::fmt::Debug for DeployKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str("DeployKey(*****)")
    }
}

/// An unprefixed token presented for deployment authorization.
pub struct DeploymentAccessToken(String);

impl DeploymentAccessToken {
    pub fn new(token: String) -> Self {
        Self(token)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for DeploymentAccessToken {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str("DeploymentAccessToken(*****)")
    }
}

impl SystemKey {
    pub fn new(key: String) -> Self {
        Self(key)
    }

    pub fn as_header(&self) -> anyhow::Result<Authorization<ConvexAdminAuthorization>> {
        Ok(Authorization(ConvexAdminAuthorization::from_admin_key(
            &self.0,
        )?))
    }
}

impl From<&str> for AdminKeyParts {
    fn from(value: &str) -> Self {
        split_admin_key(value)
            .map(|(deployment_name, encrypted_part)| {
                let (deployment_type_prefix, deployment_name) = deployment_name
                    .split_once(':')
                    .map(|(deployment_type_prefix, name)| (Some(deployment_type_prefix), name))
                    .unwrap_or((None, deployment_name));
                AdminKeyParts {
                    deployment_type_prefix: deployment_type_prefix.map(|d| d.to_string()),
                    deployment_name: Some(deployment_name.to_string()),
                    encrypted_part: encrypted_part.to_string(),
                }
            })
            .unwrap_or(AdminKeyParts {
                deployment_type_prefix: None,
                deployment_name: None,
                encrypted_part: value.to_string(),
            })
    }
}

impl TryFrom<AdminKeyParts> for RawDeploymentCredential {
    type Error = anyhow::Error;

    fn try_from(value: AdminKeyParts) -> Result<Self, Self::Error> {
        let encrypted_part = value.encrypted_part;
        let key = match (value.deployment_type_prefix, value.deployment_name) {
            (None, None) => encrypted_part,
            (None, Some(instance_identifier)) => format!("{instance_identifier}|{encrypted_part}"),
            (Some(_), None) => anyhow::bail!("Invalid admin key parts"),
            (Some(deployment_type_prefix), Some(instance_identifier)) => {
                format!("{deployment_type_prefix}:{instance_identifier}|{encrypted_part}")
            },
        };
        Ok(RawDeploymentCredential::new(key))
    }
}

/// The different parts of 'prod:happy-animal-123|restofkey'
pub struct AdminKeyParts {
    pub deployment_type_prefix: Option<String>,
    pub deployment_name: Option<String>,
    // N.B.: for a device token, this is not actually encrypted - it's just an encoded UUID
    pub encrypted_part: String,
}

pub fn split_admin_key(admin_key: &str) -> Option<(&str, &str)> {
    admin_key.split_once('|')
}

pub fn format_admin_key(deployment_name: &str, encrypted_part: &str) -> String {
    format!("{deployment_name}|{encrypted_part}")
}

pub fn remove_type_prefix_from_admin_key(admin_key: &str) -> String {
    // check if key has an instance prefix
    let Some((instance_prefix, key_part)) = admin_key.split_once('|') else {
        return admin_key.to_string();
    };

    // check if instance prefix has a type prefix
    let Some((instance_type, instance_info)) = instance_prefix.split_once(':') else {
        return admin_key.to_string();
    };

    // if instance type is "preview" or "project" - return just the key part
    if instance_type.eq_ignore_ascii_case("preview")
        || instance_type.eq_ignore_ascii_case("project")
    {
        return key_part.to_string();
    }

    // return instance info and key part
    format!("{instance_info}|{key_part}")
}

// Dashboard adds a superficial prod: or dev: prefix
// for user's visibility to the admin key's instance name.
// CLI also adds this prefix to CONVEX_DEPLOYMENT env var.
// This method strips the prefix.
pub fn remove_type_prefix_from_deployment_name(deployment_name: &str) -> &str {
    deployment_name
        .split_once(':')
        .map(|(_, name)| name)
        .unwrap_or(deployment_name)
}
