use anyhow::Context;
use convex_sync_types::{
    AuthenticationToken,
    UserIdentityAttributes,
};

use crate::convex_identity::authentication_token::Identity as AuthenticationTokenProto;

impl TryFrom<crate::convex_identity::AuthenticationToken> for AuthenticationToken {
    type Error = anyhow::Error;

    fn try_from(
        message: crate::convex_identity::AuthenticationToken,
    ) -> anyhow::Result<AuthenticationToken> {
        let identity = message.identity.context("Missing `identity` field")?;
        let token = match identity {
            AuthenticationTokenProto::Admin(token) => {
                let key = token.key.context("Missing `key` field")?;
                let acting_as = token
                    .acting_as
                    .map(|attributes| attributes.try_into())
                    .transpose()?;
                AuthenticationToken::Admin(key, acting_as)
            },
            AuthenticationTokenProto::User(token) => AuthenticationToken::User(token),
            AuthenticationTokenProto::None(_) => AuthenticationToken::None,
        };
        Ok(token)
    }
}

impl From<AuthenticationToken> for crate::convex_identity::AuthenticationToken {
    fn from(token: AuthenticationToken) -> Self {
        let identity = match token {
            AuthenticationToken::Admin(key, acting_as) => {
                let acting_as = acting_as.map(|attributes| attributes.into());
                AuthenticationTokenProto::Admin(crate::convex_identity::AdminAuthenticationToken {
                    key: Some(key),
                    acting_as,
                })
            },
            AuthenticationToken::User(token) => AuthenticationTokenProto::User(token),
            AuthenticationToken::None => AuthenticationTokenProto::None(()),
        };
        Self {
            identity: Some(identity),
        }
    }
}

impl From<(String, Option<UserIdentityAttributes>)>
    for crate::convex_identity::AdminAuthenticationToken
{
    fn from(token: (String, Option<UserIdentityAttributes>)) -> Self {
        let acting_as = token.1.map(|user_attributes| user_attributes.into());
        Self {
            key: Some(token.0),
            acting_as,
        }
    }
}
