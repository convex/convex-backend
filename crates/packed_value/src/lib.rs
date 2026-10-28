#![feature(impl_trait_in_assoc_type)]
use std::{
    collections::BTreeMap,
    ops::Deref,
};

use flexbuffers::{
    Blob,
    Buffer,
    Builder,
    FlexBufferType,
    MapReader,
    Reader,
    VectorReader,
};
use value::{
    heap_size::HeapSize,
    serde::ConvexSerializable,
    ConvexObject,
    ConvexValue,
    FieldPath,
};

mod buffer;
mod debug;
mod flexbuilder;
mod walk;

pub use self::buffer::{
    ByteBuffer,
    StringBuffer,
};
use self::flexbuilder::FlexBuilder;
use crate::buffer::PARENT_BUFFER;

#[derive(PartialEq, Eq)]
pub struct PackedValue<B: Buffer>
where
    B::BufferString: Clone,
{
    buf: B,
}

impl<B: Buffer> Clone for PackedValue<B>
where
    B::BufferString: Clone,
{
    fn clone(&self) -> Self {
        Self {
            buf: self.buf.shallow_copy(),
        }
    }
}

impl<B: Buffer> PackedValue<B>
where
    B::BufferString: Clone,
{
    pub fn new(buf: B) -> Self {
        Self { buf }
    }

    pub fn open(self) -> anyhow::Result<OpenedValue<B>> {
        OpenedValue::new(Reader::get_root(self.buf)?)
    }

    pub fn size(&self) -> usize {
        self.buf.len()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Get a shared reference to the PackedValue, so it can be opened multiple
    /// times without cloning the underlying buffer.
    pub fn as_ref(&self) -> PackedValue<&[u8]> {
        PackedValue::new(&self.buf)
    }

    pub fn open_path(self, field_path: &FieldPath) -> Option<OpenedValue<B>> {
        field_path.fields().first()?; // return None if empty path
        let mut v = self.open().expect("failed to open packed value");
        for field in field_path.fields().iter() {
            match v {
                OpenedValue::Object(o) => {
                    v = o.get(field).expect("failed to open packed object")?;
                },
                _ => return None,
            }
        }
        Some(v)
    }

    /// Same behavior as Value::get_path but doesn't fully unpack.
    pub fn get_path(&self, field_path: &FieldPath) -> Option<ConvexValue> {
        self.as_ref()
            .open_path(field_path)
            .map(ConvexValue::try_from)
            .transpose()
            .expect("failed to unpack opened value")
    }
}

impl PackedValue<ByteBuffer> {
    pub fn parse<T: ConvexSerializable>(&self) -> anyhow::Result<T> {
        // make the `Bytes` reference available to `StringBuffer`'s deserializer
        // via the PARENT_BUFFER thread-local; this is a bit hacky
        PARENT_BUFFER.set(&self.buf.inner, || {
            value::serde::from_value::<_, T::Serialized>(self.as_ref().open()?)?
                .try_into()
                .map_err(Into::<anyhow::Error>::into)
        })
    }
}

impl<B: Buffer> HeapSize for PackedValue<B>
where
    B::BufferString: Clone,
{
    fn heap_size(&self) -> usize {
        self.buf.len()
    }
}

impl PackedValue<ByteBuffer> {
    pub fn shrink(self) -> Self {
        Self {
            buf: self.buf.shrink(),
        }
    }

    pub fn pack(value: &ConvexValue) -> Self {
        let mut builder = Builder::default();
        Self::_pack(value, &mut builder);
        Self {
            buf: builder.take_buffer().into(),
        }
    }

    pub fn pack_object(value: &ConvexObject) -> Self {
        let mut builder = Builder::default();
        Self::_pack_object(value, &mut builder);
        Self {
            buf: builder.take_buffer().into(),
        }
    }

    fn _pack(value: &ConvexValue, builder: &mut impl FlexBuilder) {
        match value {
            ConvexValue::Null => {
                builder.push(());
            },
            ConvexValue::Int64(i) => {
                builder.push(*i);
            },
            ConvexValue::Float64(f) => {
                builder.push(*f);
            },
            ConvexValue::Boolean(b) => {
                builder.push(*b);
            },
            ConvexValue::String(s) => {
                builder.push(&s[..]);
            },
            ConvexValue::Bytes(b) => {
                builder.push(Blob(&b[..]));
            },
            ConvexValue::Array(values) => {
                let mut vector = builder.start_vector();
                for value in values {
                    Self::_pack(value, &mut vector);
                }
                vector.end_vector();
            },

            ConvexValue::Object(fields) => {
                Self::_pack_object(fields, builder);
            },
        }
    }

    fn _pack_object(object: &ConvexObject, builder: &mut impl FlexBuilder) {
        let mut map = builder.start_map();
        for (field, value) in object.iter() {
            let mut builder = (&field[..], &mut map);
            Self::_pack(value, &mut builder);
        }
        map.end_map();
    }
}

pub enum OpenedValue<B: Buffer = ByteBuffer>
where
    B::BufferString: Clone,
{
    Null,
    Int64(i64),
    Float64(f64),
    Boolean(bool),
    String(OpenedString<B>),
    Bytes(OpenedBytes<B>),
    Array(OpenedArray<B>),
    Object(OpenedObject<B>),
}

impl<B: Buffer> Clone for OpenedValue<B>
where
    B::BufferString: Clone,
{
    fn clone(&self) -> Self {
        match self {
            OpenedValue::Null => OpenedValue::Null,
            OpenedValue::Int64(i) => OpenedValue::Int64(*i),
            OpenedValue::Float64(f) => OpenedValue::Float64(*f),
            OpenedValue::Boolean(b) => OpenedValue::Boolean(*b),
            OpenedValue::String(s) => OpenedValue::String(s.clone()),
            OpenedValue::Bytes(b) => OpenedValue::Bytes(b.clone()),
            OpenedValue::Array(a) => OpenedValue::Array(a.clone()),
            OpenedValue::Object(o) => OpenedValue::Object(o.clone()),
        }
    }
}

impl<B: Buffer> OpenedValue<B>
where
    B::BufferString: Clone,
{
    fn new(reader: Reader<B>) -> anyhow::Result<Self> {
        let result = match reader.flexbuffer_type() {
            FlexBufferType::Null => OpenedValue::Null,
            FlexBufferType::Int | FlexBufferType::IndirectInt => {
                OpenedValue::Int64(reader.get_i64()?)
            },
            FlexBufferType::Float | FlexBufferType::IndirectFloat => {
                OpenedValue::Float64(reader.get_f64()?)
            },
            FlexBufferType::Bool => OpenedValue::Boolean(reader.get_bool()?),
            FlexBufferType::String => OpenedValue::String(OpenedString {
                buf: reader.get_str()?,
            }),
            FlexBufferType::Blob => OpenedValue::Bytes(OpenedBytes {
                buf: reader.get_blob()?.0,
            }),
            FlexBufferType::Map => {
                let reader = reader.get_map()?;
                OpenedValue::Object(OpenedObject { reader })
            },
            // NB: Maps also satisfy `is_vector`, so be sure to check those first above.
            ty if ty.is_vector() => OpenedValue::Array(OpenedArray {
                reader: reader.get_vector()?,
            }),
            ty => anyhow::bail!("Unexpected buffer type: {ty:?}"),
        };
        Ok(result)
    }
}

pub struct OpenedString<B: Buffer>
where
    B::BufferString: Clone,
{
    buf: B::BufferString,
}

impl<B: Buffer> Clone for OpenedString<B>
where
    B::BufferString: Clone,
{
    fn clone(&self) -> Self {
        Self {
            buf: self.buf.clone(),
        }
    }
}

impl<B: Buffer> Deref for OpenedString<B>
where
    B::BufferString: Clone,
{
    type Target = str;

    fn deref(&self) -> &str {
        &self.buf[..]
    }
}

pub struct OpenedBytes<B: Buffer>
where
    B::BufferString: Clone,
{
    buf: B,
}

impl<B: Buffer> Clone for OpenedBytes<B>
where
    B::BufferString: Clone,
{
    fn clone(&self) -> Self {
        Self {
            buf: self.buf.shallow_copy(),
        }
    }
}

impl<B: Buffer> Deref for OpenedBytes<B>
where
    B::BufferString: Clone,
{
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.buf[..]
    }
}

pub struct OpenedArray<B: Buffer>
where
    B::BufferString: Clone,
{
    reader: VectorReader<B>,
}

impl<B: Buffer> Clone for OpenedArray<B>
where
    B::BufferString: Clone,
{
    fn clone(&self) -> Self {
        Self {
            reader: self.reader.clone(),
        }
    }
}

impl<B: Buffer> OpenedArray<B>
where
    B::BufferString: Clone,
{
    pub fn len(&self) -> usize {
        self.reader.len()
    }

    pub fn is_empty(&self) -> bool {
        self.reader.is_empty()
    }

    pub fn index(&self, i: usize) -> anyhow::Result<OpenedValue<B>> {
        OpenedValue::new(self.reader.index(i)?)
    }

    pub fn iter(&self) -> impl Iterator<Item = anyhow::Result<OpenedValue<B>>> + '_ {
        (0..self.reader.len()).map(|i| OpenedValue::new(self.reader.index(i)?))
    }
}

pub struct OpenedObject<B: Buffer>
where
    B::BufferString: Clone,
{
    reader: MapReader<B>,
}

impl<B: Buffer> Clone for OpenedObject<B>
where
    B::BufferString: Clone,
{
    fn clone(&self) -> Self {
        Self {
            reader: self.reader.clone(),
        }
    }
}

impl<B: Buffer> OpenedObject<B>
where
    B::BufferString: Clone,
{
    pub fn len(&self) -> usize {
        self.reader.len()
    }

    pub fn is_empty(&self) -> bool {
        self.reader.is_empty()
    }

    pub fn get(&self, field: &str) -> anyhow::Result<Option<OpenedValue<B>>> {
        let Some(i) = self.reader.index_key(field) else {
            return Ok(None);
        };
        let reader = self.reader.index(i)?;
        Ok(Some(OpenedValue::new(reader)?))
    }

    pub fn iter(
        &self,
    ) -> impl Iterator<Item = anyhow::Result<(B::BufferString, OpenedValue<B>)>> + use<B> {
        self.reader
            .iter_keys()
            .zip(self.reader.iter_values())
            .map(|(key, value)| Ok((key, OpenedValue::new(value)?)))
    }
}

impl<B: Buffer> TryFrom<PackedValue<B>> for ConvexValue
where
    B::BufferString: Clone,
{
    type Error = anyhow::Error;

    fn try_from(value: PackedValue<B>) -> anyhow::Result<Self> {
        value.open()?.try_into()
    }
}

impl<B: Buffer> TryFrom<OpenedValue<B>> for ConvexValue
where
    B::BufferString: Clone,
{
    type Error = anyhow::Error;

    fn try_from(value: OpenedValue<B>) -> anyhow::Result<Self> {
        let result = match value {
            OpenedValue::Null => Self::Null,
            OpenedValue::Int64(i) => Self::from(i),
            OpenedValue::Float64(f) => Self::from(f),
            OpenedValue::Boolean(b) => Self::from(b),
            OpenedValue::String(s) => Self::try_from(s[..].to_owned())?,
            OpenedValue::Bytes(b) => Self::try_from(b[..].to_owned())?,
            OpenedValue::Array(packed_values) => {
                let values = packed_values
                    .iter()
                    .map(|r| Self::try_from(r?))
                    .collect::<anyhow::Result<Vec<_>>>()?;
                Self::Array(values.try_into()?)
            },
            OpenedValue::Object(packed_values) => {
                let values = packed_values
                    .iter()
                    .map(|r| {
                        let (k, v) = r?;
                        Ok((k.parse()?, Self::try_from(v)?))
                    })
                    .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
                Self::Object(values.try_into()?)
            },
        };
        Ok(result)
    }
}
