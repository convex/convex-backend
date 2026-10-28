#![feature(try_blocks_heterogeneous)]
#![feature(impl_trait_in_fn_trait_return)]

use std::sync::{
    Arc,
    LazyLock,
};

use bytes::BufMut;
use common::{
    http::fetch::build_proxied_reqwest_client,
    knobs::HTTP_CACHE_SIZE,
    try_anyhow,
};
use futures::Future;
use http::StatusCode;
use http_body_util::BodyExt;
use http_cache::{
    MokaCache,
    XCACHE,
};
use http_cache_reqwest::{
    Cache,
    CacheMode,
    HttpCache,
    MokaManager,
};
use metrics::log_http_response;
use openidconnect::{
    http,
    HttpRequest,
    HttpResponse,
};
use reqwest::Url;
use reqwest_middleware::ClientBuilder;
use thiserror::Error;

mod metrics;

#[derive(Error, Debug)]
#[error(transparent)]
pub struct AsStdError(#[from] anyhow::Error);

static CACHE: LazyLock<MokaManager> = LazyLock::new(|| {
    MokaManager::new(
        MokaCache::builder()
            .max_capacity(*HTTP_CACHE_SIZE)
            .weigher(|k: &String, v: &Arc<Vec<u8>>| {
                u32::try_from(k.len() + v.len()).unwrap_or(u32::MAX)
            })
            .build(),
    )
});
/// Just for metrics labeling
#[derive(Copy, Clone, Eq, PartialEq, Debug, strum::IntoStaticStr)]
pub enum ClientPurpose {
    ProviderMetadata,
    Jwks,
    UserInfo,
    WorkOSProvisioning,
}

/// A cached HTTP client that routes requests through a proxy for SSRF
/// protection. All external HTTP requests should use this client.
#[derive(Clone)]
pub struct CachedHttpClient {
    client: reqwest_middleware::ClientWithMiddleware,
}

impl CachedHttpClient {
    /// Creates a new cached HTTP client with proxy support.
    /// If proxy_url is Some, all requests will be routed through the proxy
    /// (Smokescreen) for SSRF protection.
    /// The client_id is used for proxy authentication.
    pub fn new(
        proxy_url: Option<Url>,
        client_id: String,
        redirect_policy: reqwest::redirect::Policy,
    ) -> Self {
        let client = build_proxied_reqwest_client(proxy_url, client_id, redirect_policy);
        let client_with_middleware = ClientBuilder::new(client)
            .with(Cache(HttpCache {
                mode: CacheMode::Default,
                manager: CACHE.clone(),
                options: Default::default(),
            }))
            .build();
        Self {
            client: client_with_middleware,
        }
    }

    /// Returns a closure that can be used with openidconnect's discover_async.
    pub fn for_purpose(
        self,
        purpose: ClientPurpose,
    ) -> impl Fn(HttpRequest) -> (impl Future<Output = Result<HttpResponse, AsStdError>> + 'static)
    {
        move |request: HttpRequest| {
            let client = self.client.clone();
            cached_http_client_inner(client, request, purpose)
        }
    }
}

/// HTTP fetch function that caches responses in memory based on the
/// `Cache-Control` headers in the response. Also checks for SSRF mitigation
/// responses from the proxy.
#[fastrace::trace]
async fn cached_http_client_inner(
    client: reqwest_middleware::ClientWithMiddleware,
    request: HttpRequest,
    purpose: ClientPurpose,
) -> Result<HttpResponse, AsStdError> {
    let uri_string = request.uri().to_string();
    // Error handling shenanigans because `anyhow::Error` doesn't implement
    // `std::error::Error` (required by openidconnect), but the function body
    // returns multiple error types that are easiest to unify under
    // `anyhow::Error`. We can collect the result as an `anyhow::Error`, then
    // convert it to a `AsStdError` which does implement `std::error::Error
    let res: Result<HttpResponse, anyhow::Error> = try_anyhow!({
        let (parts, body) = request.into_parts();
        let mut request_builder = client
            .request(parts.method.as_str().parse()?, parts.uri.to_string())
            .body(body);
        for (name, value) in &parts.headers {
            request_builder = request_builder.header(name.as_str(), value.as_bytes());
        }
        let response = request_builder.send().await?;

        // Check for SSRF mitigation response from proxy
        if response.status() == StatusCode::PROXY_AUTHENTICATION_REQUIRED {
            Err(anyhow::anyhow!("Request to {} forbidden", uri_string))?
        }

        let cache_hit = response
            .headers()
            .get(XCACHE)
            .map(|header| header.as_bytes() == "HIT".as_bytes())
            .unwrap_or_default();

        log_http_response(purpose, cache_hit);

        // `openidconnect` requires that the response be a single `Vec<u8>`
        // chunk, so read the entire response body here
        let (parts, body) = http::Response::from(response).into_parts();
        let body = body.collect().await?;
        let mut vec_body = vec![];
        vec_body.put(body.aggregate());
        HttpResponse::from_parts(parts, vec_body)
    });
    res.map_err(AsStdError)
}
