use std::sync::Arc;
use anyhow::Context;
use common::runtime::Runtime;
use errors::ErrorMetadata;
use keybroker::{
    legacy_admin_key_disabled_error,
    AdminIdentityPrincipal,
    Identity,
    KeyBroker,
    UNATTRIBUTED_ADMIN_KEY_MEMBER_ID,
};
use crate::{
    access_token_auth::{
        AccessTokenAuth,
        LegacyAdminKeyVerdict,
    },
    metrics::{
        log_deploy_key_use,
        log_legacy_admin_key_check,
        DeployKeyType,
        LegacyAdminKeyCheckOutcome,
    },
};

pub struct ApplicationAuth<RT: Runtime> {
    key_broker: KeyBroker,
    access_token_auth: Arc<dyn AccessTokenAuth>,
    rt: RT,

}

// Encapsulates auth logic supporting both legacy Deploy Keys and new Convex
// Access tokens
impl<RT: Runtime> ApplicationAuth<RT> {
    pub fn new(key_broker: KeyBroker, access_token_auth: Arc<dyn AccessTokenAuth>, rt: RT) -> Self {
        Self {
            key_broker,
            access_token_auth,
            rt,
        }
    }

    pub async fn check_key(&self, admin_key_or_access_token: String) -> anyhow::Result<Identity> {
        let now = self.rt.system_time();
        tracing::debug!("Checking admin_key/access token at {now:?}");

        if self
            .key_broker
            .is_encrypted_admin_key(&admin_key_or_access_token)
        {
            // assume this is a legacy Deploy Key
            let result = self
                .key_broker
                .check_admin_key(&admin_key_or_access_token, self.rt.system_time())
                .context(ErrorMetadata::unauthenticated(
                    "BadAdminKey",
                    "The provided admin key was invalid for this instance",
                ));
            match &result {
                Ok(Identity::DeploymentAdmin(_)) => {
                    log_deploy_key_use(DeployKeyType::Legacy);
                },
                Ok(Identity::System(_)) => {
                    log_deploy_key_use(DeployKeyType::System);
                },
                _ => {
                    log_deploy_key_use(DeployKeyType::Unknown);
                },
            }
            if let Ok(Identity::DeploymentAdmin(identity)) = &result
                && let AdminIdentityPrincipal::Member(member_id) = identity.principal()
                && *member_id != UNATTRIBUTED_ADMIN_KEY_MEMBER_ID
            {
                // Big brain enforces team opt-outs of legacy admin keys and
                // records their use, so its answer takes precedence.
                return match self
                    .access_token_auth
                    .check_legacy_admin_key(&admin_key_or_access_token)
                    .await?
                {
                    LegacyAdminKeyVerdict::Accepted(identity) => {
                        log_legacy_admin_key_check(LegacyAdminKeyCheckOutcome::Accepted);
                        Ok(identity)
                    },
                    LegacyAdminKeyVerdict::DisabledForTeam => {
                        log_legacy_admin_key_check(LegacyAdminKeyCheckOutcome::DisabledForTeam);
                        Err(anyhow::anyhow!(legacy_admin_key_disabled_error()))
                    },
                    LegacyAdminKeyVerdict::CheckLocally => {
                        log_legacy_admin_key_check(LegacyAdminKeyCheckOutcome::CheckedLocally);
                        result
                    },
                };
            }
            result
        } else {
            // assume this is an Access Token
            // Access Tokens are base64 encoded strings
            log_deploy_key_use(DeployKeyType::AccessToken);
            self.access_token_auth
                .is_authorized(&admin_key_or_access_token)
                .await
        }
    }

}
