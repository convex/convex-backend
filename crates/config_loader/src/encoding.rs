//! Encoding Rust types into configuration file formats
use std::marker::PhantomData;

use prost_reflect::{
    text_format::FormatOptions,
    DynamicMessage,
    MessageDescriptor,
};

use super::decoding::ProstMessage;

/// Encodes a Prost message to textproto format. This is the inverse of
/// [`super::decoding::TextProtoDecoder`].
#[derive(Clone)]
pub struct TextProtoEncoder<T: ProstMessage> {
    descriptor: MessageDescriptor,
    _type: PhantomData<T>,
}

impl<T: ProstMessage> TextProtoEncoder<T> {
    pub const fn new(descriptor: MessageDescriptor) -> Self {
        Self {
            descriptor,
            _type: PhantomData,
        }
    }

    pub fn encode(&self, value: &T) -> anyhow::Result<Vec<u8>> {
        let mut message = DynamicMessage::new(self.descriptor.clone());
        message.transcode_from(value)?;
        let text = message.to_text_format_with_options(&FormatOptions::new().pretty(true));
        Ok(text.into_bytes())
    }
}

/// Encodes a Prost message as standard-alphabet base64 of its binary wire
/// format. The inverse of [`super::decoding::Base64ProtoDecoder`].
#[derive(Clone)]
pub struct Base64ProtoEncoder<T> {
    _type: PhantomData<fn() -> T>,
}

impl<T> Base64ProtoEncoder<T> {
    pub const fn new() -> Self {
        Self { _type: PhantomData }
    }
}

impl<T> Default for Base64ProtoEncoder<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ProstMessage> Base64ProtoEncoder<T> {
    pub fn encode(&self, value: &T) -> anyhow::Result<Vec<u8>> {
        Ok(base64::encode(value.encode_to_vec()).into_bytes())
    }
}
