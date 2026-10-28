//! The `ctx.auth.signJwt` syscall, available in mutations.

use std::{
    collections::BTreeMap,
    time::{
        Duration,
        SystemTime,
    },
};

use anyhow::Context;
use common::{
    components::ComponentPath,
    runtime::Runtime,
    types::{
        EnvVarName,
        EnvVarValue,
    },
};
use database::Transaction;
use errors::ErrorMetadata;
use keybroker::{
    DeploymentJwtKeys,
    DeploymentJwtRequest,
};
use serde::Deserialize;
use serde_json::{
    Map as JsonMap,
    Value as JsonValue,
};

use crate::environment::{
    system_env_var_overrides,
    CONVEX_ORIGIN,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignArgs {
    #[serde(default)]
    claims: JsonMap<String, JsonValue>,
    expires_in_seconds: f64,
}

/// The origin issuers are built from: `CONVEX_CLOUD_URL` as functions and
/// `auth.config` see it, so a canonical URL overrides the `.convex.cloud`
/// default.
pub async fn deployment_origin<RT: Runtime>(
    tx: &mut Transaction<RT>,
    default_system_env_vars: &BTreeMap<EnvVarName, EnvVarValue>,
) -> anyhow::Result<String> {
    if let Some(url) = system_env_var_overrides(tx).await?.remove(&*CONVEX_ORIGIN) {
        return Ok(url.into());
    }
    Ok(default_system_env_vars
        .get(&*CONVEX_ORIGIN)
        .context("Missing CONVEX_CLOUD_URL")?
        .as_ref()
        .to_owned())
}

/// Mints a new signed JWT.
pub fn mint(
    keys: &DeploymentJwtKeys,
    origin: &str,
    signer: ComponentPath,
    args: JsonValue,
    now: SystemTime,
) -> anyhow::Result<String> {
    let SignArgs {
        claims,
        expires_in_seconds,
    } = serde_json::from_value(args).map_err(|e| invalid_args("auth.signJwt", e))?;
    if !expires_in_seconds.is_finite() || expires_in_seconds < 1.0 {
        anyhow::bail!(invalid_args(
            "auth.signJwt",
            "expiresInSeconds must be a number of seconds, at least 1"
        ));
    }
    keys.mint(
        DeploymentJwtRequest {
            origin: origin.to_string(),
            signer,
            claims,
            expires_in: Duration::from_secs(expires_in_seconds as u64),
        },
        now,
    )
}

fn invalid_args(api: &str, e: impl std::fmt::Display) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "InvalidArguments",
        format!("Invalid arguments for {api}: {e}"),
    )
}
