#![feature(impl_trait_in_assoc_type)]

mod broker;
mod deployment_jwt;
mod encryptor;
mod legacy_encryptor;
mod metrics;
mod operations;
mod secret;
pub use pb::convex_identity::DeploymentOperation;
pub use sync_types::UserIdentityAttributes;

pub use self::{
    broker::{
        AdminIdentity,
        AdminIdentityPrincipal,
        CoreIdTokenWithCustomClaims,
        CustomClaims,
        ExportDownloadActor,
        FunctionRunnerKeyBroker,
        GetFileAuthorization,
        Identity,
        IdentityValidity,
        KeyBroker,
        StoreFileAuthorization,
        SystemKey,
        UserIdentity,
        UNATTRIBUTED_ADMIN_KEY_MEMBER_ID,
    },
    deployment_jwt::{
        deployment_jwt_issuer,
        DeploymentJwtKey,
        DeploymentJwtKeys,
        DeploymentJwtRequest,
    },
    encryptor::{
        Encryptor,
        RandomEncryptor,
    },
    legacy_encryptor::LegacyEncryptor,
    operations::{
        bad_admin_key_error,
        legacy_admin_key_disabled_error,
        operations_for_deploy_key,
        read_only_operations,
        DeploymentOp,
    },
    secret::{
        DeploymentSecret,
        Secret,
    },
};

pub const DEV_INSTANCE_NAME: &str = include_str!("../dev/instance_name.txt");
