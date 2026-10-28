use std::{
    fmt,
    ops::{
        Deref,
        Range,
    },
    str::{
        self,
        Utf8Error,
    },
};

use bytes::Bytes;
use flexbuffers::Buffer;
use scoped_tls::scoped_thread_local;
use serde::{
    de::{
        Error,
        Visitor,
    },
    Deserialize,
    Serialize,
    Serializer,
};
use value::heap_size::HeapSize;

scoped_thread_local!(pub(crate) static PARENT_BUFFER: Bytes);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ByteBuffer {
    pub(crate) inner: Bytes,
}

impl<B> From<B> for ByteBuffer
where
    Bytes: From<B>,
{
    fn from(buf: B) -> Self {
        Self::new(Bytes::from(buf))
    }
}

impl ByteBuffer {
    pub fn new(inner: Bytes) -> Self {
        Self { inner }
    }

    pub fn shrink(self) -> Self {
        let mut buf = Vec::from(self.inner);
        buf.shrink_to_fit();
        Self::new(buf.into())
    }
}

impl Deref for ByteBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.inner.deref()
    }
}

impl Buffer for ByteBuffer {
    type BufferString = StringBuffer;

    fn slice(&self, range: Range<usize>) -> Option<Self> {
        if range.start > range.end || range.end >= self.inner.len() {
            return None;
        }
        Some(Self {
            inner: self.inner.slice(range),
        })
    }

    fn empty() -> Self {
        Self {
            inner: Bytes::new(),
        }
    }

    fn buffer_str(&self) -> Result<Self::BufferString, Utf8Error> {
        str::from_utf8(&self.inner[..])?;
        Ok(StringBuffer {
            inner: self.inner.clone(),
        })
    }

    fn shallow_copy(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }

    fn empty_str() -> Self::BufferString {
        StringBuffer {
            inner: Bytes::new(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StringBuffer {
    inner: Bytes,
}

impl fmt::Debug for StringBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl Deref for StringBuffer {
    type Target = str;

    fn deref(&self) -> &str {
        unsafe { str::from_utf8_unchecked(&self.inner[..]) }
    }
}

impl Serialize for StringBuffer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        (**self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StringBuffer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visit;
        impl<'de> Visitor<'de> for Visit {
            type Value = StringBuffer;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                // If `v` is a subslice of `PARENT_BUFFER` (i.e. the buffer that
                // is currently being parsed with
                // `PackedValue<ByteBuffer>::parse`), then return a subslice of
                // that buffer instead of copying it
                if PARENT_BUFFER.is_set()
                    && let Some(v) = PARENT_BUFFER.with(|parent| {
                        if let Some(range) = parent.subslice_range(v.as_bytes()) {
                            Some(parent.slice(range))
                        } else {
                            None
                        }
                    })
                {
                    return Ok(StringBuffer { inner: v });
                }
                Ok(StringBuffer {
                    inner: v.as_bytes().to_owned().into(),
                })
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(StringBuffer::new(v))
            }
        }
        deserializer.deserialize_str(Visit)
    }
}

impl StringBuffer {
    pub fn new(s: String) -> StringBuffer {
        StringBuffer {
            inner: s.into_bytes().into(),
        }
    }
}

impl HeapSize for StringBuffer {
    fn heap_size(&self) -> usize {
        self.inner.heap_size()
    }
}
