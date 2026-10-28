use axum::response::IntoResponse;
use common::{
    http::{
        extract::{
            Json,
            MtState,
        },
        HttpResponseError,
    },
    knobs::DEPLOYMENT_JWKS_MAX_AGE,
};
use http::header::CACHE_CONTROL;

use crate::LocalAppState;

/// Public keys for the deployment's JWT signing key. Verifiers, including this
/// deployment's own `customJwt` providers, fetch it to check tokens from
/// `ctx.auth.signJwt`.
pub async fn deployment_jwks(
    MtState(st): MtState<LocalAppState>,
) -> Result<impl IntoResponse, HttpResponseError> {
    let jwks = st.application.deployment_jwks().await?;
    Ok((
        [(
            CACHE_CONTROL,
            format!("public, max-age={}", DEPLOYMENT_JWKS_MAX_AGE.as_secs()),
        )],
        Json(jwks),
    ))
}
