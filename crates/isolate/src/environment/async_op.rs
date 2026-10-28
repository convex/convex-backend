use std::fmt;

use common::{
    http::HttpRequestStream,
    runtime::UnixTimestamp,
    sync::spsc,
};
use futures::stream::BoxStream;
use serde_json::{
    json,
    Value as JsonValue,
};
use value::sha256::Sha256;

#[derive(strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AsyncOpRequest {
    Fetch {
        request: HttpRequestStream,
        response_body_stream_id: uuid::Uuid,
    },
    ParseMultiPart {
        content_type: String,
        request_stream: BoxStream<'static, anyhow::Result<bytes::Bytes>>,
    },
    Sleep {
        name: String, // setTimeout or setInterval
        until: UnixTimestamp,
    },
    StorageStore {
        body_stream: spsc::UnboundedReceiver<anyhow::Result<bytes::Bytes>>,
        content_type: Option<String>,
        content_length: Option<String>,
        digest: Option<String>,
    },
    StorageGet {
        storage_id: String,
        stream_id: uuid::Uuid,
    },
    SendStream {
        stream: Option<BoxStream<'static, anyhow::Result<bytes::Bytes>>>,
        stream_id: uuid::Uuid,
    },
}

impl AsyncOpRequest {
    pub fn name_for_error(&self) -> &'static str {
        match self {
            Self::Fetch { .. } => "Fetch",
            Self::ParseMultiPart { .. } => "FormParse",
            Self::Sleep { .. } => "Sleep",
            Self::StorageStore { .. } | Self::StorageGet { .. } => "Storage",
            Self::SendStream { .. } => "Stream",
        }
    }

    pub fn description_for_error(&self) -> String {
        match self {
            Self::Fetch { .. } => "fetch()".to_string(),
            Self::ParseMultiPart { .. } => "formData()".to_string(),
            Self::Sleep { name, .. } => name.to_string(),
            Self::StorageStore { .. } => "storage.store()".to_string(),
            Self::StorageGet { .. } => "storage.get()".to_string(),
            Self::SendStream { .. } => "stream".to_string(),
        }
    }

    /// The op's inputs other than its body streams, which a replay compares
    /// against the recorded op's to tell whether it started the same one.
    pub fn non_stream_inputs(&self) -> JsonValue {
        match self {
            Self::Fetch {
                request,
                response_body_stream_id,
            } => json!({
                "url": request.url.as_str(),
                "method": request.method.as_str(),
                // Header values are hashed because they often hold credentials,
                // which recordings and tape mismatch errors mustn't expose.
                "headers": request
                    .headers
                    .iter()
                    .map(|(name, value)| [
                        name.as_str().to_owned(),
                        Sha256::hash(value.as_bytes()).as_hex(),
                    ])
                    .collect::<Vec<_>>(),
                "hasBody": request.body.is_some(),
                "responseBodyStreamId": response_body_stream_id,
            }),
            Self::ParseMultiPart {
                content_type,
                request_stream: _,
            } => json!({ "contentType": content_type }),
            Self::Sleep { name, until } => json!({
                "name": name,
                "untilNanos": until.as_nanos().to_string(),
            }),
            Self::StorageStore {
                body_stream: _,
                content_type,
                content_length,
                digest,
            } => json!({
                "contentType": content_type,
                "contentLength": content_length,
                "digest": digest,
            }),
            Self::StorageGet {
                storage_id,
                stream_id,
            } => json!({ "storageId": storage_id, "streamId": stream_id }),
            Self::SendStream { stream, stream_id } => {
                json!({ "hasStream": stream.is_some(), "streamId": stream_id })
            },
        }
    }
}

impl fmt::Debug for AsyncOpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.name_for_error().fmt(f)
    }
}
