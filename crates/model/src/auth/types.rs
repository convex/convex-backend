use std::collections::{
    BTreeMap,
    BTreeSet,
};

use common::auth::AuthInfo;
use openidconnect::IssuerUrl;
use serde::{
    Deserialize,
    Serialize,
};
use value::{
    obj,
    ConvexObject,
    ConvexValue,
};

/// Persisted version of AuthInfo that impls try_from to ConvexObject
/// Ideally this remains local to this crate (has to be pub for db-info)
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AuthInfoPersisted(pub AuthInfo);

impl TryFrom<ConvexObject> for AuthInfoPersisted {
    type Error = anyhow::Error;

    fn try_from(o: ConvexObject) -> Result<Self, Self::Error> {
        let mut fields: BTreeMap<_, _> = o.into();

        let is_oidc = match fields.remove("type") {
            // Default to OIDC
            None => true,
            Some(ConvexValue::String(s)) if &s[..] == "oidc" => true,
            Some(ConvexValue::String(s)) if &s[..] == "customJwt" => false,
            f => anyhow::bail!("Missing or invalid type field for AuthInfo: {f:?}"),
        };
        let result = if is_oidc {
            let application_id = match fields.remove("applicationID") {
                Some(ConvexValue::String(s)) => s.into(),
                _ => anyhow::bail!("Missing or invalid applicationID field for AuthInfo"),
            };
            let domain = match fields.remove("domain") {
                Some(ConvexValue::String(s)) => IssuerUrl::new(s.into())?,
                _ => anyhow::bail!("Missing or invalid domain field for AuthInfo"),
            };
            AuthInfo::Oidc {
                application_id,
                domain,
            }
        } else {
            let application_id = match fields.remove("applicationID") {
                Some(ConvexValue::String(s)) => Some(s.into()),
                Some(ConvexValue::Null) | None => None,
                v => anyhow::bail!("Invalid applicationID field for AuthInfo: {v:?}"),
            };
            let issuer = match fields.remove("issuer") {
                Some(ConvexValue::String(s)) => IssuerUrl::new(s.into())?,
                _ => anyhow::bail!("Missing or invalid issuer field for AuthInfo"),
            };
            let jwks = match fields.remove("jwks") {
                Some(ConvexValue::String(s)) => s.into(),
                _ => anyhow::bail!("Missing or invalid jwks field for AuthInfo"),
            };
            let algorithm = match fields.remove("algorithm") {
                Some(ConvexValue::String(s)) => s.parse()?,
                _ => anyhow::bail!("Missing or invalid algorithm field for AuthInfo"),
            };
            AuthInfo::CustomJwt {
                application_id,
                issuer,
                jwks,
                algorithm,
            }
        };
        Ok(Self(result))
    }
}

impl TryFrom<AuthInfoPersisted> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(info: AuthInfoPersisted) -> Result<Self, Self::Error> {
        let result = match info.0 {
            AuthInfo::Oidc {
                application_id,
                domain,
            } => obj!(
                "applicationID" => application_id,
                "domain" => domain.to_string(),
            )?,
            AuthInfo::CustomJwt {
                application_id,
                issuer,
                jwks,
                algorithm,
            } => obj!(
                "type" => "customJwt",
                "applicationID" => application_id,
                "issuer" => issuer.to_string(),
                "jwks" => jwks,
                "algorithm" => String::from(algorithm),
            )?,
        };
        Ok(result)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl AuthDiff {
    pub fn new(added: BTreeSet<AuthInfo>, removed: BTreeSet<AuthInfo>) -> anyhow::Result<Self> {
        let added_strings = added
            .into_iter()
            .map(|auth_info| {
                let auth_info_obj = ConvexObject::try_from(AuthInfoPersisted(auth_info))?;
                let auth_info_json = auth_info_obj.json_serialize()?;
                anyhow::Ok(auth_info_json)
            })
            .try_collect::<Vec<String>>()?;

        let removed_strings = removed
            .into_iter()
            .map(|auth_info| {
                let auth_info_obj = ConvexObject::try_from(AuthInfoPersisted(auth_info))?;
                let auth_info_json = auth_info_obj.json_serialize()?;
                anyhow::Ok(auth_info_json)
            })
            .try_collect::<Vec<String>>()?;

        Ok(Self {
            added: added_strings,
            removed: removed_strings,
        })
    }
}
