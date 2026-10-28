use common::http::RequestDestination;
use serde::{
    Deserialize,
    Serialize,
};
use value::codegen_convex_serialization;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalUrl {
    pub request_destination: RequestDestination,
    pub url: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedCanonicalUrl {
    request_destination: String,
    url: String,
}

impl From<CanonicalUrl> for SerializedCanonicalUrl {
    fn from(value: CanonicalUrl) -> Self {
        Self {
            request_destination: value.request_destination.to_string(),
            url: value.url,
        }
    }
}

impl TryFrom<SerializedCanonicalUrl> for CanonicalUrl {
    type Error = anyhow::Error;

    fn try_from(value: SerializedCanonicalUrl) -> Result<Self, Self::Error> {
        Ok(Self {
            request_destination: value.request_destination.parse()?,
            url: value.url,
        })
    }
}

codegen_convex_serialization!(CanonicalUrl, SerializedCanonicalUrl);
