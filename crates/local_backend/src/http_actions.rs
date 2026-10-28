use std::{
    pin::Pin,
    sync::Arc,
};

use anyhow::Context;
use application::api::ApplicationApi;
use axum::{
    body::{
        Body,
        Bytes,
    },
    debug_handler,
    extract::{
        FromRequest,
        State,
    },
    response::{
        IntoResponse,
        Response,
    },
    routing::{
        get,
        MethodRouter,
    },
    RequestExt,
};
use axum_extra::extract::Host;
use common::{
    http::{
        ExtractRequestId,
        ExtractRequestMetadata,
        ExtractResolvedHostname,
        HttpResponseError,
        OriginalHttpUri,
        ResolvedHostname,
    },
    types::FunctionCaller,
    RequestContext,
};
use futures::{
    stream::{
        BoxStream,
        FusedStream,
        Peekable,
    },
    FutureExt,
    StreamExt,
    TryStreamExt,
};
use futures_async_stream::try_stream;
use http::{
    header::FORWARDED,
    HeaderMap,
    Method,
    StatusCode,
};
use keybroker::Identity;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use udf::{
    HttpActionRequest,
    HttpActionRequestHead,
    HttpActionResponsePart,
    HttpActionResponseStreamer,
};
use url::Url;

use crate::{
    authentication::TryExtractIdentity,
    RouterState,
};

pub struct ExtractHttpRequestMetadata(pub HttpActionRequest);

const X_FORWARDED_PROTO: &str = "x-forwarded-proto";

fn parse_forwarded(headers: &HeaderMap) -> Option<&str> {
    // if there are multiple `Forwarded` `HeaderMap::get` will return the first one
    let forwarded_values = headers.get(FORWARDED)?.to_str().ok()?;

    // get the first set of values
    let first_value = forwarded_values.split(',').next()?;

    // find the value of the `host` field
    first_value.split(';').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case("proto")
            .then(|| value.trim().trim_matches('"'))
    })
}

impl FromRequest<RouterState, axum::body::Body> for ExtractHttpRequestMetadata {
    type Rejection = HttpResponseError;

    async fn from_request(
        mut req: axum::http::Request<axum::body::Body>,
        _: &RouterState,
    ) -> Result<Self, Self::Rejection> {
        // Assume HTTP if neither `X-Forwarded-Proto` nor `Forwarded` headers give us a
        // protocol. This is a reasonable default as the only way to get HTTPS
        // with the backend is via a TLS-terminating proxy which should be
        // setting one of these headers.
        let scheme = req
            .headers()
            .get(X_FORWARDED_PROTO)
            .and_then(|h| h.to_str().ok())
            .or_else(|| parse_forwarded(req.headers()))
            .unwrap_or("http")
            .to_owned();
        let host = req
            .extract_parts::<Host>()
            .await
            .context("Host header not present")?
            .0;
        // If the URI has been rewritten to `/http`, present the original URI to the
        // action. Note that this may not be the same as `OriginalUri`,
        // depending on where the rewrite takes place.
        let OriginalHttpUri(uri) = req
            .extensions()
            .get::<OriginalHttpUri>()
            .cloned()
            .unwrap_or_else(|| OriginalHttpUri(req.uri().clone()));
        let headers = req.headers().clone();
        let method = req.method().clone();

        // Construct the URL we provide in the HTTP request object.
        let url = Url::parse(&format!("{scheme}://{host}{uri}")).context("Invalid URL")?;

        if method == Method::GET || method == Method::OPTIONS || method == Method::HEAD {
            return Ok(ExtractHttpRequestMetadata(HttpActionRequest {
                head: HttpActionRequestHead {
                    headers,
                    url,
                    method,
                },
                body: None,
            }));
        }

        let body = req.into_body();

        Ok(ExtractHttpRequestMetadata(HttpActionRequest {
            head: HttpActionRequestHead {
                headers,
                url,
                method,
            },
            body: Some(Box::pin(body.into_data_stream().map_err(|e| e.into()))),
        }))
    }
}

#[fastrace::trace(properties = { "udf_type": "http_action"})]
#[debug_handler]
pub async fn http_any_method(
    State(st): State<RouterState>,
    TryExtractIdentity(identity_result): TryExtractIdentity,
    ExtractRequestId(request_id): ExtractRequestId,
    ExtractRequestMetadata(request_metadata): ExtractRequestMetadata,
    ExtractResolvedHostname(host): ExtractResolvedHostname,
    ExtractHttpRequestMetadata(http_request_metadata): ExtractHttpRequestMetadata,
) -> Result<impl IntoResponse, HttpResponseError> {
    // The `Authorization` header for the request may contain a token corresponding
    // to Convex auth, or it could be something separate managed by the developer.
    // Try to extract the identity based on the Convex auth, but allow the request
    // to go through if the header does not seem to specify Convex auth.
    let identity = identity_result.unwrap_or_else(|e| Identity::Unknown(e.downcast().ok()));

    let mut http_response_stream = stream_http_response(
        host,
        RequestContext::new(request_id, request_metadata),
        http_request_metadata,
        identity,
        st.api.clone(),
    );
    let head = http_response_stream.try_next().await?;
    let Some(HttpActionResponsePart::Head(response_head)) = head else {
        return Err(anyhow::anyhow!("Did not receive HTTP response head first").into());
    };
    let body: BoxStream<'static, _> = Box::pin(http_response_stream.map(|p| match p {
        Ok(HttpActionResponsePart::BodyChunk(bytes)) => Ok(bytes),
        Err(e) => Err(e),
        _ => Err(anyhow::anyhow!(
            "Unexpected element in HTTP response stream"
        )),
    }));

    let mut peek_body = body.peekable();
    let content_length = response_head
        .headers
        .get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse().ok());
    if content_length == Some(0) {
        // In case hyper/axum doesn't poll the body at all, make sure we poll it
        // at least once to do any cleanup.
        Pin::new(&mut peek_body).peek().await;
    }

    Ok(HttpActionResponse {
        status: response_head.status,
        headers: response_head.headers,
        content_length,
        body: peek_body,
    })
}

#[try_stream(ok=HttpActionResponsePart, error=anyhow::Error, boxed)]
async fn stream_http_response(
    host: ResolvedHostname,
    request_context: RequestContext,
    http_request_metadata: HttpActionRequest,
    identity: Identity,
    application: Arc<dyn ApplicationApi>,
) {
    let (http_response_sender, http_response_receiver) = mpsc::unbounded_channel();

    tokio::pin! {
        let run_action_fut = application
            .execute_http_action(
                &host,
                request_context,
                http_request_metadata,
                identity,
                FunctionCaller::HttpEndpoint,
                HttpActionResponseStreamer::new(http_response_sender),
            )
            .fuse();
    }
    let mut response_stream = UnboundedReceiverStream::new(http_response_receiver).fuse();
    loop {
        let next_part = async {
            let v: Option<HttpActionResponsePart> = tokio::select! {
                Some(result) = response_stream.next(), if !response_stream.is_terminated() => {
                    Some(result)
                },
                func_result = &mut run_action_fut => {
                    match func_result {
                        Ok(_) => None,
                        Err(e) => return Err(e)
                    }
                }
            };
            Ok(v)
        };
        match next_part.await? {
            Some(part) => yield part,
            None => break,
        }
    }
    while let Some(part) = response_stream.next().await {
        yield part
    }
}

pub fn http_action_handler() -> MethodRouter<RouterState> {
    get(http_any_method)
        .post(http_any_method)
        .delete(http_any_method)
        .patch(http_any_method)
        .put(http_any_method)
        .options(http_any_method)
}

pub struct HttpActionResponse {
    pub body: Peekable<BoxStream<'static, Result<Bytes, anyhow::Error>>>,
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub content_length: Option<u64>,
}

impl IntoResponse for HttpActionResponse {
    fn into_response(self) -> Response {
        let status = self.status;
        let headers = self.headers;
        let body = Body::from_stream(stream_with_content_length(self.body, self.content_length));
        (status, headers, body).into_response()
    }
}

#[try_stream(ok=Bytes, error=anyhow::Error)]
pub async fn stream_with_content_length(
    mut stream: Peekable<BoxStream<'static, Result<Bytes, anyhow::Error>>>,
    length: Option<u64>,
) {
    let mut length_returned = 0;
    while let Some(chunk) = stream.try_next().await? {
        length_returned += chunk.len() as u64;
        if let Some(length) = length
            && length_returned >= length
        {
            // Once `hyper` receives enough bytes to match 'Content-Length'
            // bytes, it will stop polling this stream, so we need to peek at
            // the underlying stream to ensure that it can do any cleanup it
            // needs to do.
            // Note: this peek will probably return `None`.
            Pin::new(&mut stream).peek().await;
        }
        yield chunk;
    }
}
