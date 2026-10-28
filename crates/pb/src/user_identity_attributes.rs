use convex_sync_types::{
    UserIdentifier,
    UserIdentityAttributes,
};

use crate::convex_identity::UserIdentityAttributes as UserIdentityAttributesProto;

impl TryFrom<UserIdentityAttributesProto> for UserIdentityAttributes {
    type Error = anyhow::Error;

    fn try_from(
        UserIdentityAttributesProto {
            token_identifier,
            issuer,
            subject,
            name,
            given_name,
            family_name,
            nickname,
            preferred_username,
            profile_url,
            picture_url,
            website_url,
            email,
            email_verified,
            gender,
            birthday,
            timezone,
            language,
            phone_number,
            phone_number_verified,
            address,
            updated_at,
            custom_claims,
        }: UserIdentityAttributesProto,
    ) -> anyhow::Result<UserIdentityAttributes> {
        let token_identifier =
            token_identifier.ok_or_else(|| anyhow::anyhow!("Missing token_identifier"))?;
        let token_identifier = UserIdentifier(token_identifier);
        Ok(UserIdentityAttributes {
            token_identifier,
            issuer,
            subject,
            name,
            given_name,
            family_name,
            nickname,
            preferred_username,
            profile_url,
            picture_url,
            website_url,
            email,
            email_verified,
            gender,
            birthday,
            timezone,
            language,
            phone_number,
            phone_number_verified,
            address,
            updated_at,
            custom_claims: custom_claims.into_iter().collect(),
        })
    }
}

impl From<UserIdentityAttributes> for UserIdentityAttributesProto {
    fn from(
        UserIdentityAttributes {
            token_identifier,
            subject,
            issuer,
            name,
            given_name,
            family_name,
            nickname,
            preferred_username,
            profile_url,
            picture_url,
            website_url,
            email,
            email_verified,
            gender,
            birthday,
            timezone,
            language,
            phone_number,
            phone_number_verified,
            address,
            updated_at,
            custom_claims,
        }: UserIdentityAttributes,
    ) -> UserIdentityAttributesProto {
        UserIdentityAttributesProto {
            token_identifier: Some(token_identifier.to_string()),
            issuer,
            subject,
            name,
            given_name,
            family_name,
            nickname,
            preferred_username,
            profile_url,
            picture_url,
            website_url,
            email,
            email_verified,
            gender,
            birthday,
            timezone,
            language,
            phone_number,
            phone_number_verified,
            address,
            updated_at,
            custom_claims: custom_claims.into_iter().collect(),
        }
    }
}
