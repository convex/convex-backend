use std::sync::Arc;
use anyhow::Context;
use common::runtime::Runtime;
use errors::ErrorMetadata;
use keybroker::{
    Identity,
    KeyBroker,
};
use crate::{
    access_token_auth::AccessTokenAuth,
    metrics::{
        log_deploy_key_use,
        DeployKeyType,
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
