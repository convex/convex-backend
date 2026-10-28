use authentication::application_auth::ApplicationAuth;
use common::runtime::Runtime;
use errors::ErrorMetadataAnyhowExt;
use keybroker::{
    bad_admin_key_error,
    AdminIdentityPrincipal,
    Identity,
};

pub async fn must_be_admin_from_key<RT: Runtime>(
    app_auth: &ApplicationAuth<RT>,
    instance_name: String,
    admin_key: String,
) -> anyhow::Result<Identity> {
    let identity = app_auth.check_key(admin_key).await.map_err(|e| {
        if e.is_forbidden() {
            // This attaches a second ErrorMetadata to the error, but this
            // message is more helpful
            e.context(bad_admin_key_error(Some(instance_name)))
        } else {
            // This is a server error of some kind, not an issue with the
            // key itself
            e
        }
    })?;
    Ok(identity)
}

pub fn must_be_admin(identity: &Identity) -> anyhow::Result<AdminIdentityPrincipal> {
    let admin_identity = match identity {
        Identity::DeploymentAdmin(admin_identity) => admin_identity,
        Identity::ActingUser(admin_identity, _user_identity_attributes) => admin_identity,
        Identity::System(_) | Identity::User(_) | Identity::Unknown(_) => {
            return Err(bad_admin_key_error(identity.instance_name()).into());
        },
    };
    Ok(admin_identity.principal().clone())
}
