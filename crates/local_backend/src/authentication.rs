//! Code for handling authentication between the CLI user / dashboard and the
//! backend.

use anyhow::{
    anyhow,
    Context,
};
use authentication::extract_bearer_token;
use axum::{
    extract::FromRequestParts,
    RequestPartsExt,
};
use common::{
    http::{
        extract::{
            FromMtState,
            Query,
        },
        ExtractRequestId,
        ExtractRequestMetadata,
        ExtractResolvedHostname,
        HttpResponseError,
    },
    runtime::Runtime,
    types::remove_type_prefix_from_admin_key,
    RequestContext,
};
use errors::ErrorMetadata;
use keybroker::Identity;
use serde::Deserialize;
use sync_types::{
    AuthenticationToken,
    UserIdentityAttributes,
};

use crate::{
    LocalAppState,
    RouterState,
};

pub struct ExtractAuthenticationToken(pub AuthenticationToken);

impl<T: Sync> FromRequestParts<T> for ExtractAuthenticationToken {
    type Rejection = HttpResponseError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _st: &T,
    ) -> Result<Self, Self::Rejection> {
        // First, try extracting from headers
        if let Some(h) = parts.headers.get(http::header::AUTHORIZATION) {
            let h_str = h.to_str().context(ErrorMetadata::bad_request(
                "HeaderParseFailure",
                format!("Failed to parse header {h:?}"),
            ))?;
            let is_admin_key = h_str
                .get(..7)
                .ok_or_else(|| anyhow!("Invalid Header"))
                .context(ErrorMetadata::bad_request(
                    "InvalidHeaderFailure",
                    "Invalid authentication header".to_string(),
                ))?
                .eq_ignore_ascii_case("convex ");

            return if is_admin_key {
                // This is an admin key, not an OIDC bearer token. These are sent from the
                // dashboard in lieu of our old cookie-based auth.
                Ok(Self(extract_admin_key(h_str)?))
            } else {
                let auth: String = extract_bearer_token(Some(h_str.to_string()))
                    .await
                    .map_err(|_| {
                        anyhow::anyhow!(ErrorMetadata::bad_request(
                            "InvalidAdminKey",
                            "Invalid admin key",
                        ))
                    })?
                    .unwrap();
                Ok(Self(AuthenticationToken::User(auth)))
            };
        }

        // If no header is provided, also allow extracting admin key from query param.
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct QueryParams {
            admin_key: Option<String>,
        }
        if let Query(QueryParams {
            admin_key: Some(admin_key),
        }) = parts.extract().await?
        {
            return Ok(Self(AuthenticationToken::Admin(admin_key, None)));
        }

        Ok(Self(AuthenticationToken::None))
    }
}

impl From<ExtractAuthenticationToken> for AuthenticationToken {
    fn from(token: ExtractAuthenticationToken) -> Self {
        token.0
    }
}

pub struct ExtractIdentity(pub Identity);

impl<S> FromRequestParts<S> for ExtractIdentity
where
    LocalAppState: FromMtState<S>,
    S: Send + Sync + Clone + 'static,
{
    type Rejection = HttpResponseError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        st: &S,
    ) -> Result<Self, Self::Rejection> {
        let token: AuthenticationToken =
            parts.extract::<ExtractAuthenticationToken>().await?.into();
        let st = LocalAppState::from_request_parts(parts, st).await?;

        Ok(Self(
            st.application
                .authenticate(token, st.application.runtime().system_time())
                .await?,
        ))
    }
}

impl From<ExtractIdentity> for Identity {
    fn from(identity: ExtractIdentity) -> Self {
        identity.0
    }
}

pub struct TryExtractIdentity(pub anyhow::Result<Identity>);

impl FromRequestParts<RouterState> for TryExtractIdentity {
    type Rejection = HttpResponseError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        st: &RouterState,
    ) -> Result<Self, Self::Rejection> {
        let token = match parts.extract::<ExtractAuthenticationToken>().await {
            Ok(t) => t.into(),
            Err(e) => return Ok(Self(Err(e.into()))),
        };

        let Ok(ExtractResolvedHostname(host)) = parts.extract::<ExtractResolvedHostname>().await;

        let request_id = match parts.extract::<ExtractRequestId>().await {
            Ok(id) => id,
            Err(e) => return Ok(Self(Err(e.into()))),
        };
        let request_metadata = match parts.extract::<ExtractRequestMetadata>().await {
            Ok(m) => m.0,
            Err(e) => return Ok(Self(Err(e.into()))),
        };
        let request_context = RequestContext::new(request_id.0, request_metadata);
        Ok(Self(
            st.api.authenticate(&host, request_context, token).await,
        ))
    }
}

fn extract_admin_key(header: &str) -> anyhow::Result<AuthenticationToken> {
    let key = strip_prefix_ignore_case(header, "convex ")
        .context("Called extract_admin_key with a non-admin authorization header.")?;
    // We need to strip the unencrypted deployment type prefix ending in ':'
    // which clashes with the user impersonation logic below.
    // So theoretically this method accepts a key in the format:
    // "prod:some-depl-name123|sa67asd6a5da6d5:sd6f5sdf76dsf4ds6f4s68fd"
    // where the last part is the `acting_user_b64`.
    let key_without_prefix = remove_type_prefix_from_admin_key(key);
    // Looks for two parts split by a colon -- the first part always being the admin
    // key, and the second part being an optional base64 encoded
    // user to act as.
    match key_without_prefix.split_once(':') {
        // An admin acting as a user
        Some((key, acting_user_b64)) => {
            let attributes_s = base64::decode(acting_user_b64).context(
                ErrorMetadata::bad_request("HeaderParseFailure", "Malformed Authorization header."),
            )?;
            let attributes: UserIdentityAttributes =
                serde_json::from_slice::<serde_json::Value>(&attributes_s)
                    .context(ErrorMetadata::bad_request(
                        "HeaderParseFailure",
                        "Malformed Authorization header.",
                    ))?
                    .try_into()
                    .context(ErrorMetadata::bad_request(
                        "HeaderParseFailure",
                        "Malformed Authorization header.",
                    ))?;
            Ok(AuthenticationToken::Admin(
                key.to_string(),
                Some(attributes),
            ))
        },
        // Just an admin
        None => Ok(AuthenticationToken::Admin(key_without_prefix, None)),
    }
}

// Like `str::strip_prefix`, but ignores casing.
fn strip_prefix_ignore_case<'a>(string: &'a str, prefix: &str) -> Option<&'a str> {
    if string.len() >= prefix.len() && string[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&string[prefix.len()..])
    } else {
        None
    }
}
