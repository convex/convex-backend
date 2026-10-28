//! Decoding configuration files into Rust types
use std::{
    collections::HashMap,
    marker::PhantomData,
    str::FromStr,
};

use prost_reflect::{
    prost::Message,
    DynamicMessage,
    MessageDescriptor,
};

/// Trait representing the decoding of a config file's contents to a generic
/// output type.
pub trait ConfigDecoder: Clone {
    type Output: PartialEq + Clone + Send + Sync + 'static;
    fn decode(&self, contents: Vec<u8>) -> anyhow::Result<Self::Output>;
}

/// Simple decoder for getting the value of the config file as a UTF-8 string.
#[derive(Copy, Clone)]
pub struct TextDecoder;

impl ConfigDecoder for TextDecoder {
    type Output = String;

    fn decode(&self, contents: Vec<u8>) -> anyhow::Result<String> {
        Ok(String::from_utf8(contents)?)
    }
}

#[derive(Copy, Clone)]
pub struct FromStrDecoder<T> {
    trim: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<T> FromStrDecoder<T> {
    pub fn new() -> Self {
        Self {
            trim: false,
            _marker: PhantomData,
        }
    }

    /// Trim leading/trailing whitespace from the file contents before parsing.
    /// Useful for files with a trailing newline.
    pub fn trimmed(mut self) -> Self {
        self.trim = true;
        self
    }
}

impl<T: FromStr + PartialEq + Clone + Send + Sync + 'static> ConfigDecoder for FromStrDecoder<T>
where
    anyhow::Error: From<T::Err>,
{
    type Output = T;

    fn decode(&self, contents: Vec<u8>) -> anyhow::Result<T> {
        let s = String::from_utf8(contents)?;
        let slice = if self.trim { s.trim() } else { s.as_str() };
        Ok(slice.parse()?)
    }
}

#[derive(Copy, Clone)]
pub struct DelimitedKeyValueDecoder<K, V> {
    delimiter: char,
    _marker: PhantomData<(K, V)>,
}

impl<K: FromStr, V: FromStr> DelimitedKeyValueDecoder<K, V> {
    pub fn new(delimiter: char) -> Self {
        Self {
            delimiter,
            _marker: PhantomData,
        }
    }
}

impl<K, V> ConfigDecoder for DelimitedKeyValueDecoder<K, V>
where
    K: FromStr + Eq + std::hash::Hash + Clone + Send + Sync + 'static,
    V: FromStr + Eq + Clone + Send + Sync + 'static,
    <K as FromStr>::Err: std::error::Error + Send + Sync + 'static,
    <V as FromStr>::Err: std::error::Error + Send + Sync + 'static,
{
    type Output = HashMap<K, V>;

    fn decode(&self, contents: Vec<u8>) -> anyhow::Result<Self::Output> {
        let contents = std::str::from_utf8(&contents)?;
        let mut map = HashMap::new();
        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let (key, value) = trimmed.split_once(self.delimiter).ok_or_else(|| {
                anyhow::anyhow!(
                    "failed to find delimiter `{}` in line: `{}`",
                    self.delimiter,
                    line
                )
            })?;
            map.insert(key.parse()?, value.parse()?);
        }
        Ok(map)
    }
}

/// No-op decoder for getting the raw bytes of the config file.
#[derive(Copy, Clone)]
pub struct BytesDecoder;
impl ConfigDecoder for BytesDecoder {
    type Output = Vec<u8>;

    fn decode(&self, contents: Vec<u8>) -> anyhow::Result<Self::Output> {
        Ok(contents)
    }
}

pub trait ProstMessage = Message + Clone + Default + PartialEq + 'static;

/// Decodes textproto format to a Prost message. You need the
/// [`MessageDescriptor`] for the proto you intend to decode; this can be
/// obtained from the `FILE_DESCRIPTOR_BYTES` constant in every generated
/// protobuf crate.
#[derive(Clone)]
pub struct TextProtoDecoder<T: ProstMessage> {
    descriptor: MessageDescriptor,
    _type: PhantomData<T>,
}

impl<T: ProstMessage> TextProtoDecoder<T> {
    pub const fn new(descriptor: MessageDescriptor) -> Self {
        Self {
            descriptor,
            _type: PhantomData,
        }
    }
}

impl<T: ProstMessage> ConfigDecoder for TextProtoDecoder<T> {
    type Output = T;

    fn decode(&self, contents: Vec<u8>) -> anyhow::Result<T> {
        let contents = std::str::from_utf8(&contents)?;
        let message = DynamicMessage::parse_text_format(self.descriptor.clone(), contents)?;
        Ok(message.transcode_to()?)
    }
}

/// Decodes a base64-encoded binary Prost message: standard-alphabet base64 of
/// the message's wire-format bytes, with any surrounding whitespace trimmed.
/// Empty or all-whitespace input decodes to the message's default value.
#[derive(Clone)]
pub struct Base64ProtoDecoder<T> {
    _type: PhantomData<fn() -> T>,
}

impl<T> Base64ProtoDecoder<T> {
    pub const fn new() -> Self {
        Self { _type: PhantomData }
    }
}

impl<T> Default for Base64ProtoDecoder<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ProstMessage> ConfigDecoder for Base64ProtoDecoder<T> {
    type Output = T;

    fn decode(&self, contents: Vec<u8>) -> anyhow::Result<T> {
        let bytes = base64::decode(contents.trim_ascii())?;
        Ok(T::decode(&bytes[..])?)
    }
}
