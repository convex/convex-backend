use std::{
    collections::{
        BTreeMap,
        HashMap,
    },
    pin::Pin,
    sync::{
        atomic::{
            AtomicU64,
            Ordering,
        },
        Arc,
        LazyLock,
    },
};

use async_trait::async_trait;
use bytes::Bytes;
use futures::{
    future::BoxFuture,
    Stream,
    StreamExt,
};
use futures_async_stream::try_stream;
use http::StatusCode;
use reqwest::{
    Body,
    Proxy,
    Url,
};
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::select;

use crate::http::{
    HttpRequestStream,
    HttpResponseStream,
};

/// Http client used for fetch syscall.
#[async_trait]
pub trait FetchClient: Send + Sync {
    async fn fetch(&self, request: HttpRequestStream) -> anyhow::Result<HttpResponseStream>;
}

/// The request's `AbortSignal` fired before the fetch (or the read of its
/// response body) completed. Layers closer to JS translate this into a
/// `DOMException` named `AbortError`, which is what the fetch spec requires and
/// what SDKs check for.
#[derive(Debug, thiserror::Error)]
#[error("The signal has been aborted")]
pub struct FetchAborted;

pub struct ProxiedFetchClient {
    http_client:
        LazyLock<reqwest::Client, Box<dyn FnOnce() -> reqwest::Client + Send + Sync + 'static>>,
    uses_proxy: bool,
}

// Share the underlying rustls::ClientConfig between ProxiedFetchClients
static TLS_CONNECTOR: LazyLock<rustls::ClientConfig> = LazyLock::new(|| {
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("invalid default protocol versions?")
    .with_platform_verifier()
    .expect("Failed to create rustls_platform_verifier")
    .with_no_client_auth();
    // reqwest prefers HTTP2 by default, preserve that
    config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];
    config
});

/// Creates a reqwest client configured with an optional proxy.
/// The client_id is set to the instance name for logging.
/// The redirect_policy dictates how redirects are handled.
///
/// This function is shared between `ProxiedFetchClient` (for fetch syscalls)
/// and `CachedHttpClient` in the `http_client` crate (for OIDC
/// discovery).
pub fn build_proxied_reqwest_client(
    proxy_url: Option<Url>,
    client_id: String,
    redirect_policy: reqwest::redirect::Policy,
) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .redirect(redirect_policy)
        .http2_keep_alive_interval(*crate::knobs::HTTP2_CLIENT_KEEPALIVE_INTERVAL)
        .http2_keep_alive_timeout(*crate::knobs::HTTP2_CLIENT_KEEPALIVE_TIMEOUT);
    // It's okay to panic on these errors, as they indicate a serious programming
    // error -- building the reqwest client is expected to be infallible.
    if let Some(proxy_url) = proxy_url {
        let proxy = Proxy::all(proxy_url)
            .expect("Infallible conversion from URL type to URL type")
            .custom_http_auth(
                client_id
                    .try_into()
                    .expect("Backend name is not valid ASCII?"),
            );
        builder = builder.proxy(proxy);
    }
    builder = builder
        .user_agent("Convex/1.0")
        .use_preconfigured_tls(TLS_CONNECTOR.clone());
    builder.build().expect("Failed to build reqwest client")
}

impl ProxiedFetchClient {
    pub fn new(
        proxy_url: Option<Url>,
        client_id: String,
        redirect_policy: reqwest::redirect::Policy,
    ) -> Self {
        Self {
            uses_proxy: proxy_url.is_some(),
            http_client: LazyLock::new(Box::new(move || {
                build_proxied_reqwest_client(proxy_url, client_id, redirect_policy)
            })),
        }
    }
}

#[async_trait]
impl FetchClient for ProxiedFetchClient {
    async fn fetch(&self, mut request: HttpRequestStream) -> anyhow::Result<HttpResponseStream> {
        let mut request_builder = self
            .http_client
            .request(request.method, request.url.as_str());
        let request_size = Arc::new(AtomicU64::new(0));
        // Only attach a body when the request has one. `Body::wrap_stream` (used
        // by `streaming_body`) reports `is_end_stream() == false`, so hyper omits
        // END_STREAM from the HTTP/2 HEADERS frame and closes the stream with a
        // trailing empty DATA frame -- which strict servers reject for a body-less
        // GET (https://github.com/convex/convex-backend/issues/497). Omitting
        // the body uses `Body::empty()` (`is_end_stream() == true`), so hyper sets
        // END_STREAM on HEADERS and sends no DATA frame.
        if let Some(body) = request.body {
            request_builder = request_builder.body(streaming_body(body, request_size.clone()));
        }
        for (name, value) in &request.headers {
            // `Proxy-Authorization` is hop-by-hop and comes from the client's proxy
            // configuration.
            if name == http::header::PROXY_AUTHORIZATION {
                continue;
            }
            request_builder = request_builder.header(name.as_str(), value.as_bytes());
        }
        let raw_request = request_builder.build()?;
        let raw_response = select! {
            response = self.http_client.execute(raw_request) => {
                // Through our egress proxy, the chain below reqwest's message
                // names the proxy ("tunnel error"), so keep only that message.
                response.map_err(|e| {
                    if self.uses_proxy {
                        anyhow::anyhow!("{e}")
                    } else {
                        e.into()
                    }
                })?
            },
            _ = &mut request.signal => {
                anyhow::bail!(FetchAborted);
            },
        };
        if raw_response.status() == StatusCode::PROXY_AUTHENTICATION_REQUIRED {
            // SSRF mitigated -- our proxy blocked this request because it was
            // directed at a non-public IP range. Don't send back the raw HTTP response as
            // it leaks internal implementation details in the response headers.
            anyhow::bail!("Request to {} forbidden", request.url);
        }
        let status = raw_response.status();
        let headers = raw_response.headers().to_owned();
        let response = HttpResponseStream {
            status,
            headers,
            url: Some(request.url),
            body: Some(cancelable_body_stream(
                raw_response.bytes_stream(),
                request.signal,
            )),
            request_size,
        };
        Ok(response)
    }
}

/// Wraps a request body stream into a reqwest [`Body`], counting the bytes that
/// flow through it into `request_size`.
///
/// This is intentionally a lazy, non-awaiting wrap: the body stream may be a
/// full-duplex body whose first chunk is only produced after the response
/// headers arrive, so we must not poll it before the request is sent.
fn streaming_body(
    body: Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Sync + Send>>,
    request_size: Arc<AtomicU64>,
) -> Body {
    Body::wrap_stream(body.inspect(move |b| {
        if let Ok(b) = b {
            request_size.fetch_add(b.len() as u64, Ordering::Relaxed);
        }
    }))
}

#[try_stream(boxed, ok = Bytes, error = anyhow::Error)]
async fn cancelable_body_stream<E: Into<anyhow::Error>>(
    stream: impl futures::stream::Stream<Item = Result<Bytes, E>> + Send + 'static,
    mut signal: BoxFuture<'static, ()>,
) {
    let mut stream = Box::pin(stream);
    loop {
        let result = async {
            select! {
                item = stream.next() => {
                    item.transpose().map_err(Into::<anyhow::Error>::into)
                },
                _ = &mut signal => {
                    Err(anyhow::anyhow!(FetchAborted))
                },
            }
        };
        match result.await? {
            Some(item) => {
                yield item;
            },
            None => {
                break;
            },
        }
    }
}

type HandlerFn = Box<
    dyn Fn(HttpRequestStream) -> BoxFuture<'static, anyhow::Result<HttpResponseStream>>
        + Send
        + Sync
        + 'static,
>;

pub struct StaticFetchClient {
    router: BTreeMap<url::Url, HashMap<http::Method, HandlerFn>>,
    num_calls: AtomicU64,
}

impl StaticFetchClient {
    pub fn new() -> Self {
        Self {
            router: BTreeMap::new(),
            num_calls: AtomicU64::new(0),
        }
    }

    pub fn register_http_route<F>(&mut self, url: url::Url, method: http::Method, handler: F)
    where
        F: Fn(HttpRequestStream) -> BoxFuture<'static, anyhow::Result<HttpResponseStream>>
            + Send
            + Sync
            + 'static,
    {
        self.router
            .entry(url)
            .or_default()
            .insert(method, Box::new(handler));
    }

    /// Returns how many times a fetch client has been called
    pub fn num_calls(&self) -> u64 {
        self.num_calls.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl FetchClient for StaticFetchClient {
    async fn fetch(&self, request: HttpRequestStream) -> anyhow::Result<HttpResponseStream> {
        self.num_calls.fetch_add(1, Ordering::Relaxed);
        let handler = self
            .router
            .get(&request.url)
            .and_then(|methods| methods.get(&request.method))
            .unwrap_or_else(|| {
                panic!(
                    "could not find route {} with method {}",
                    request.url, request.method
                )
            });
        handler(request).await
    }
}
