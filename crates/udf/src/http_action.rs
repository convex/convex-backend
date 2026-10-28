use core::fmt;

use bytes::Bytes;
use common::{
    http::normalize_header_map,
    types::{
        HttpActionRoute,
        RoutableMethod,
    },
};
use futures::stream::BoxStream;
use headers::{
    HeaderMap,
    HeaderValue,
};
use http::{
    header::CONTENT_TYPE,
    Method,
    StatusCode,
};
use pb::common::HttpHeader;
use serde_json::Value as JsonValue;
use tokio::sync::mpsc;
use url::Url;
use value::sha256::{
    Sha256,
    Sha256Digest,
};

pub const HTTP_ACTION_RESPONSE_BODY_LIMIT: usize = 100 << 20;

pub struct HttpActionRequest {
    pub head: HttpActionRequestHead,
    pub body: Option<BoxStream<'static, anyhow::Result<bytes::Bytes>>>,
}

impl fmt::Debug for HttpActionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpActionRequest")
            .field("head", &self.head)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpActionRequestHead {
    pub headers: HeaderMap,
    pub url: Url,
    pub method: Method,
}

impl HttpActionRequestHead {
    // HttpActionRoutes should normally come from the router, but in cases where
    // we fail to do so, we use this to construct a route we can use for the
    // purposes of logging
    pub fn route_for_failure(&self) -> HttpActionRoute {
        let path = self.url.path();
        HttpActionRoute {
            // TODO: we want this to be infallible so we can always log something, so pick `Get`
            // if the method doesn't parse. The better solution is to have a separate struct for
            // logging that allows `method` to be any string.
            method: self
                .method
                .to_string()
                .parse()
                .unwrap_or(RoutableMethod::Get),
            path: path.to_string(),
            matched: false,
        }
    }
}

impl TryFrom<pb::common::HttpActionRequestHead> for HttpActionRequestHead {
    type Error = anyhow::Error;

    fn try_from(
        pb::common::HttpActionRequestHead {
            http_headers,
            url,
            method,
        }: pb::common::HttpActionRequestHead,
    ) -> Result<Self, Self::Error> {
        let headers = http_headers
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<HeaderMap, _>>()?;
        let url = Url::parse(&url)?;
        let method = method.parse()?;
        Ok(Self {
            headers,
            url,
            method,
        })
    }
}

impl From<HttpActionRequestHead> for pb::common::HttpActionRequestHead {
    fn from(
        HttpActionRequestHead {
            headers,
            url,
            method,
        }: HttpActionRequestHead,
    ) -> Self {
        Self {
            http_headers: normalize_header_map(headers)
                .map(HttpHeader::from)
                .collect(),
            url: url.to_string(),
            method: method.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum HttpActionResponsePart {
    Head(HttpActionResponseHead),
    BodyChunk(Bytes),
}

impl HttpActionResponsePart {
    pub fn from_text(status: StatusCode, message: String) -> Vec<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static(mime::TEXT_PLAIN_UTF_8.as_ref()),
        );
        let head = Self::Head(HttpActionResponseHead { status, headers });
        let body = Self::BodyChunk(message.into_bytes().into());
        vec![head, body]
    }

    pub fn from_json(status: StatusCode, body: JsonValue) -> Vec<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static(mime::APPLICATION_JSON.as_ref()),
        );
        let head = Self::Head(HttpActionResponseHead { status, headers });
        let body_chunk = HttpActionResponsePart::BodyChunk(body.to_string().into_bytes().into());
        vec![head, body_chunk]
    }
}

#[derive(Debug, Clone)]
pub struct HttpActionResponseHead {
    pub status: StatusCode,
    pub headers: HeaderMap,
}

impl TryFrom<pb::common::HttpActionResponseHead> for HttpActionResponseHead {
    type Error = anyhow::Error;

    fn try_from(
        pb::common::HttpActionResponseHead {
            status,
            http_headers,
        }: pb::common::HttpActionResponseHead,
    ) -> Result<Self, Self::Error> {
        let status = StatusCode::from_u16(status as u16)?;
        let headers = http_headers
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<HeaderMap, _>>()?;
        Ok(Self { status, headers })
    }
}

impl From<HttpActionResponseHead> for pb::common::HttpActionResponseHead {
    fn from(HttpActionResponseHead { status, headers }: HttpActionResponseHead) -> Self {
        let status = u16::from(status) as u32;
        let http_headers = normalize_header_map(headers)
            .map(HttpHeader::from)
            .collect();
        Self {
            status,
            http_headers,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpActionResponseStreamer {
    head: Option<HttpActionResponseHead>,
    total_bytes_sent: usize,
    sha256: Sha256,
    pub sender: mpsc::UnboundedSender<HttpActionResponsePart>,
}

impl HttpActionResponseStreamer {
    pub fn new(sender: mpsc::UnboundedSender<HttpActionResponsePart>) -> Self {
        Self {
            head: None,
            total_bytes_sent: 0,
            sha256: Sha256::new(),
            sender,
        }
    }

    pub fn has_started(&self) -> bool {
        self.head.is_some()
    }

    pub fn head(&self) -> Option<&HttpActionResponseHead> {
        self.head.as_ref()
    }

    pub fn total_bytes_sent(&self) -> usize {
        self.total_bytes_sent
    }

    fn send_head(
        &mut self,
        head: HttpActionResponseHead,
    ) -> anyhow::Result<Result<(), mpsc::error::SendError<HttpActionResponsePart>>> {
        if self.has_started() {
            anyhow::bail!("Sending HTTP response head after other response parts");
        };
        self.head = Some(head.clone());
        Ok(self.sender.send(HttpActionResponsePart::Head(head)))
    }

    fn send_body(
        &mut self,
        bytes: Bytes,
    ) -> anyhow::Result<Result<(), mpsc::error::SendError<HttpActionResponsePart>>> {
        anyhow::ensure!(
            self.has_started(),
            "Sending response body before response head"
        );
        self.total_bytes_sent += bytes.len();
        self.sha256.update(&bytes);
        Ok(self.sender.send(HttpActionResponsePart::BodyChunk(bytes)))
    }

    pub fn send_part(
        &mut self,
        part: HttpActionResponsePart,
    ) -> anyhow::Result<Result<(), mpsc::error::SendError<HttpActionResponsePart>>> {
        let send_result = match part {
            HttpActionResponsePart::Head(h) => self.send_head(h)?,
            HttpActionResponsePart::BodyChunk(b) => self.send_body(b)?,
        };
        Ok(send_result)
    }

    pub fn complete(self) -> Sha256Digest {
        self.sha256.finalize()
    }
}
