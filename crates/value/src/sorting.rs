//! Implementation of `Ord` for `Value` using *sort keys*. We define a total
//! ordering on `Value`s by mapping them to binary strings and then comparing
//! those lexicographically.
//!
//! This encoding of `Value` is self-delimiting and variable-length. Each value
//! has a tag indicating its value (and some additional length information for
//! integers), with the tags carefully chosen to preserve order.
//!
//! This encoding largely follows FoundationDB's [tuple
//! layer](https://github.com/apple/foundationdb/blob/master/design/tuple.md).
//! 1) Values are always prefixed with a tag.
//! 2) Binary strings are stored with `0x0` as a delimiter. Null bytes within
//!    the string are escaped to `0x0 0xFF`, which implies that `0xFF` must
//!    never be a valid tag.
//! 3) 64-bit signed integers are stored as either 1, 2, 4, or 8 bytes, with the
//!    size stored within the tag. There is a negative variant of the tag that
//!    orders negatives before positives.
//! 4) Floats are stored according to IEEE-754 total ordering. See [FoundationDB's notes](https://github.com/apple/foundationdb/blob/master/design/tuple.md#ieee-binary-floating-point)
//!    for an explanation of the algorithm.
//! 5) Compound types, like arrays, are stored sequentially, with a null
//!    terminator at the end.
use std::cmp::Ordering;

use bytes::BufMut;

use crate::{
    walk::{
        ConvexArrayWalker,
        ConvexBytesWalker,
        ConvexObjectWalker,
        ConvexStringWalker,
        ConvexValueType,
        ConvexValueWalker,
    },
    ConvexValue,
};

const UNDEFINED_TAG: u8 = 0x1;

// Legacy ID format (unused).
// const ID_TAG: u8 = 0x2;
const NULL_TAG: u8 = 0x3;

const NEG_INT64_8_BYTE_TAG: u8 = 0x4;
const NEG_INT64_4_BYTE_TAG: u8 = 0x5;
const NEG_INT64_2_BYTE_TAG: u8 = 0x6;
const NEG_INT64_1_BYTE_TAG: u8 = 0x7;
const ZERO_INT64_TAG: u8 = 0x8;
const POS_INT64_1_BYTE_TAG: u8 = 0x9;
const POS_INT64_2_BYTE_TAG: u8 = 0xA;
const POS_INT64_4_BYTE_TAG: u8 = 0xB;
const POS_INT64_8_BYTE_TAG: u8 = 0xC;

const FLOAT64_TAG: u8 = 0xD;

const FALSE_BOOLEAN_TAG: u8 = 0xE;
const TRUE_BOOLEAN_TAG: u8 = 0xF;

const STRING_TAG: u8 = 0x10;
const BYTES_TAG: u8 = 0x11;
const ARRAY_TAG: u8 = 0x12;
// Deprecated datatypes, now unused.
// const SET_TAG: u8 = 0x13;
// const MAP_TAG: u8 = 0x14;
const OBJECT_TAG: u8 = 0x15;

pub const TERMINATOR_BYTE: u8 = 0x0;
const ESCAPE_BYTE: u8 = 0xFF;

pub fn write_escaped_bytes(buf: &[u8], writer: &mut impl BufMut) {
    let mut last = 0;
    // Insert `ESCAPE_BYTE`s after each `TERMINATOR_BYTE`
    for i in memchr::Memchr::new(TERMINATOR_BYTE, buf) {
        writer.put_slice(&buf[last..=i]);
        writer.put_u8(ESCAPE_BYTE);
        last = i + 1;
    }
    writer.put_slice(&buf[last..]);
    writer.put_u8(TERMINATOR_BYTE);
}

fn write_escaped_string(buf: &str, writer: &mut impl BufMut) {
    write_escaped_bytes(buf.as_bytes(), writer)
}

#[allow(clippy::match_overlapping_arm)]
fn write_tagged_int(n: i64, writer: &mut impl BufMut) {
    // Our integer tag values are chosen such that their distance from the zero tag
    // represents how many bytes they should take.
    let tag_diff = match n {
        -128..=127 => 1,
        -32_768..=32_767 => 2,
        -2_147_483_648..=2147483647 => 3,
        -9_223_372_036_854_775_808..=9_223_372_036_854_775_807 => 4,
    };
    let tag = if n < 0 {
        ZERO_INT64_TAG - tag_diff
    } else {
        ZERO_INT64_TAG + tag_diff
    };
    let num_bytes = 1 << (tag_diff - 1);
    let buf = n.to_be_bytes();
    // Check that all of the bytes we're leaving off aren't used.
    let empty = if n < 0 { 0xFF } else { 0 };
    assert!(buf[..(8 - num_bytes)].iter().all(|&b| b == empty));
    writer.put_u8(tag);
    writer.put(&buf[(8 - num_bytes)..]);
}

/// Generate the sort key for a sequence of `Value`s.
pub fn values_to_bytes(values: &[Option<ConvexValue>]) -> Vec<u8> {
    let mut out = vec![];
    for value in values {
        let Ok(()) = write_sort_key_or_undefined(value.as_ref(), &mut out);
    }
    out
}

pub mod sorting_decode {
    use std::collections::BTreeMap;

    use anyhow::bail;
    use bytes::Buf;

    use super::*;
    use crate::ConvexObject;

    fn read_escaped_string<R: Buf>(reader: &mut R) -> anyhow::Result<String> {
        Ok(String::from_utf8(read_escaped_bytes(reader)?)?)
    }

    fn read_terminated<R: Buf, F>(reader: &mut R, mut read_element: F) -> anyhow::Result<()>
    where
        F: FnMut(&mut R) -> anyhow::Result<()>,
    {
        loop {
            if let Some(&TERMINATOR_BYTE) = reader.chunk().first() {
                reader.get_u8();
                break;
            }
            read_element(reader)?;
        }
        Ok(())
    }

    fn read_tagged_int<R: Buf>(tag: u8, reader: &mut R) -> anyhow::Result<i64> {
        let val = match tag {
            NEG_INT64_1_BYTE_TAG => reader.try_get_i8()? as i64,
            NEG_INT64_2_BYTE_TAG => reader.try_get_i16()? as i64,
            NEG_INT64_4_BYTE_TAG => reader.try_get_i32()? as i64,
            NEG_INT64_8_BYTE_TAG => reader.try_get_i64()?,
            ZERO_INT64_TAG => return Ok(0),
            POS_INT64_1_BYTE_TAG => reader.try_get_i8()? as i64,
            POS_INT64_2_BYTE_TAG => reader.try_get_i16()? as i64,
            POS_INT64_4_BYTE_TAG => reader.try_get_i32()? as i64,
            POS_INT64_8_BYTE_TAG => reader.try_get_i64()?,
            _ => panic!("not a tagged int"),
        };
        // Check that the decoded value is in the expected range for `tag`
        let in_range = match tag {
            NEG_INT64_1_BYTE_TAG => val < 0,
            NEG_INT64_2_BYTE_TAG => val < i8::MIN as i64,
            NEG_INT64_4_BYTE_TAG => val < i16::MIN as i64,
            NEG_INT64_8_BYTE_TAG => val < i32::MIN as i64,
            POS_INT64_1_BYTE_TAG => val > 0,
            POS_INT64_2_BYTE_TAG => val > i8::MAX as i64,
            POS_INT64_4_BYTE_TAG => val > i16::MAX as i64,
            POS_INT64_8_BYTE_TAG => val > i32::MAX as i64,
            _ => unreachable!(),
        };
        anyhow::ensure!(in_range, "non-canonical tagged int {val} with tag {tag}");
        Ok(val)
    }

    /// Parse a `Vec<Option<ConvexValue>>` from its respective sort keys.
    pub fn bytes_to_values<R: Buf>(reader: &mut R) -> anyhow::Result<Vec<Option<ConvexValue>>> {
        let mut values = vec![];
        while reader.has_remaining() {
            if reader.chunk()[0] == UNDEFINED_TAG {
                reader.get_u8();
                values.push(None);
            } else {
                let value = ConvexValue::read_sort_key(reader)?;
                values.push(Some(value));
            }
        }
        Ok(values)
    }

    /// Read an escaped, null-terminated byte string from the input stream.
    pub fn read_escaped_bytes<R: Buf>(reader: &mut R) -> anyhow::Result<Vec<u8>> {
        let mut out = vec![];
        loop {
            anyhow::ensure!(reader.has_remaining(), "unexpected EOF");
            let chunk = reader.chunk();
            if let Some(i) = memchr::memchr(TERMINATOR_BYTE, chunk) {
                out.extend_from_slice(&chunk[..i]);
                reader.advance(i + 1);
                if let Some(&ESCAPE_BYTE) = reader.chunk().first() {
                    reader.advance(1);
                    out.push(TERMINATOR_BYTE);
                } else {
                    break;
                }
            } else {
                out.extend_from_slice(chunk);
                reader.advance(chunk.len());
            }
        }
        Ok(out)
    }

    impl ConvexValue {
        /// Parse a `Value` from a sort key.
        pub fn read_sort_key<R: Buf>(reader: &mut R) -> anyhow::Result<Self> {
            let tag = reader.try_get_u8()?;
            let r = match tag {
                NULL_TAG => Self::Null,

                NEG_INT64_8_BYTE_TAG..=POS_INT64_8_BYTE_TAG => {
                    ConvexValue::from(read_tagged_int(tag, reader)?)
                },
                FLOAT64_TAG => {
                    let mut n = reader.try_get_u64()?;
                    // If the sign bit was set, just turn off the sign bit.
                    if n & (1 << 63) != 0 {
                        n &= !(1 << 63);
                    }
                    // Otherwise, flip all of the bits.
                    else {
                        n = !n;
                    }
                    ConvexValue::from(f64::from_bits(n))
                },

                FALSE_BOOLEAN_TAG => ConvexValue::from(false),
                TRUE_BOOLEAN_TAG => ConvexValue::from(true),

                STRING_TAG => {
                    let s = read_escaped_string(reader)?;
                    ConvexValue::try_from(s)?
                },
                BYTES_TAG => {
                    let b = read_escaped_bytes(reader)?;
                    ConvexValue::try_from(b)?
                },

                ARRAY_TAG => {
                    let mut elements = vec![];
                    read_terminated(reader, |reader| {
                        elements.push(Self::read_sort_key(reader)?);
                        Ok(())
                    })?;
                    ConvexValue::Array(elements.try_into()?)
                },
                OBJECT_TAG => {
                    let mut elements = vec![];
                    loop {
                        let field = if let Some(&TERMINATOR_BYTE) = reader.chunk().first() {
                            reader.get_u8();
                            if let Some(&ESCAPE_BYTE) = reader.chunk().first() {
                                reader.get_u8();
                                "".parse()?
                            } else {
                                break;
                            }
                        } else {
                            read_escaped_string(reader)?.try_into()?
                        };
                        let value = Self::read_sort_key(reader)?;
                        elements.push((field, value));
                    }
                    let len = elements.len();
                    let elements = BTreeMap::from_iter(elements);
                    if elements.len() != len {
                        anyhow::bail!("Duplicate element in encoded object");
                    }
                    ConvexValue::Object(ConvexObject::try_from(elements)?)
                },

                ESCAPE_BYTE => bail!("Escape code used as tag"),
                _ => bail!("Unrecognized tag: {}", tag),
            };
            Ok(r)
        }
    }
}

impl ConvexValue {
    /// Generate the sort key for a given `Value`.
    pub fn sort_key(&self) -> Vec<u8> {
        let mut out = vec![];
        self.write_sort_key(&mut out);
        out
    }

    pub fn write_sort_key(&self, writer: &mut impl BufMut) {
        let Ok(()) = write_sort_key(self, writer);
    }
}

/// Write a `Value`'s sort key out to a writer.
pub fn write_sort_key<V: ConvexValueWalker>(
    value: V,
    writer: &mut impl BufMut,
) -> Result<(), V::Error> {
    match value.walk()? {
        ConvexValueType::Null => {
            writer.put_u8(NULL_TAG);
        },
        ConvexValueType::Int64(0) => {
            writer.put_u8(ZERO_INT64_TAG);
        },
        ConvexValueType::Int64(i) => {
            write_tagged_int(i, writer);
        },
        ConvexValueType::Float64(f) => {
            let mut f = f.to_bits();
            // Flip all of the bits if the sign bit is set.
            if f & (1 << 63) != 0 {
                f = !f;
            }
            // Otherwise, just flip the sign bit.
            else {
                f |= 1 << 63;
            }
            writer.put_u8(FLOAT64_TAG);
            writer.put_u64(f); // N.B.: always big-endian
        },
        ConvexValueType::Boolean(false) => {
            writer.put_u8(FALSE_BOOLEAN_TAG);
        },
        ConvexValueType::Boolean(true) => {
            writer.put_u8(TRUE_BOOLEAN_TAG);
        },
        ConvexValueType::String(s) => {
            writer.put_u8(STRING_TAG);
            write_escaped_string(s.as_str(), writer);
        },
        ConvexValueType::Bytes(b) => {
            writer.put_u8(BYTES_TAG);
            write_escaped_bytes(b.as_bytes(), writer);
        },
        ConvexValueType::Array(array) => {
            writer.put_u8(ARRAY_TAG);
            for element in array.walk() {
                write_sort_key(element?, writer)?;
            }
            writer.put_u8(TERMINATOR_BYTE);
        },
        ConvexValueType::Object(object) => {
            writer.put_u8(OBJECT_TAG);
            for pair in object.walk() {
                let (field, value) = pair?;
                let field = field.as_str();
                write_escaped_string(field, writer);
                // Distinguish the case of an empty FieldName from an empty
                // object. Note that this encoding looks like an escaped `\0`
                // byte, but valid FieldNames only contain non-control ASCII
                // characters so this is not ambiguous.
                if field.is_empty() {
                    writer.put_u8(ESCAPE_BYTE);
                }
                write_sort_key(value, writer)?;
            }
            writer.put_u8(TERMINATOR_BYTE);
        },
    }
    Ok(())
}

/// Writes `value`'s sort key, interpreting `None` as `undefined` (which is,
/// notably, a different value than `null`)
pub fn write_sort_key_or_undefined<V: ConvexValueWalker>(
    value: Option<V>,
    writer: &mut impl BufMut,
) -> Result<(), V::Error> {
    match value {
        Some(value) => write_sort_key(value, writer),
        None => {
            writer.put_u8(UNDEFINED_TAG);
            Ok(())
        },
    }
}

// Manual implementation of `Ord` that is proptested to be equivalent to
// comparing sort keys.
impl Ord for ConvexValue {
    fn cmp(&self, other: &Self) -> Ordering {
        // This function is structured to make it hard to add another variant without
        // adding its case here. To that end, we avoid using wildcard matches.
        fn type_tag(v: &ConvexValue) -> usize {
            match v {
                ConvexValue::Null => 1,
                ConvexValue::Int64(..) => 2,
                ConvexValue::Float64(..) => 3,
                ConvexValue::Boolean(..) => 4,
                ConvexValue::String(..) => 5,
                ConvexValue::Bytes(..) => 6,
                ConvexValue::Array(..) => 7,
                ConvexValue::Object(..) => 10,
            }
        }
        let tag_cmp = type_tag(self).cmp(&type_tag(other));
        if !tag_cmp.is_eq() {
            return tag_cmp;
        }
        match self {
            ConvexValue::Null => {
                let ConvexValue::Null = other else {
                    panic!("Invalid value: {other:?}");
                };
                Ordering::Equal
            },
            ConvexValue::Int64(self_) => {
                let ConvexValue::Int64(other_) = other else {
                    panic!("Invalid value: {other:?}");
                };
                self_.cmp(other_)
            },
            ConvexValue::Float64(self_) => {
                let ConvexValue::Float64(other_) = other else {
                    panic!("Invalid value: {other:?}");
                };
                self_.total_cmp(other_)
            },
            ConvexValue::Boolean(self_) => {
                let ConvexValue::Boolean(other_) = other else {
                    panic!("Invalid value: {other:?}");
                };
                self_.cmp(other_)
            },
            ConvexValue::String(self_) => {
                let ConvexValue::String(other_) = other else {
                    panic!("Invalid value: {other:?}");
                };
                self_.cmp(other_)
            },
            ConvexValue::Bytes(self_) => {
                let ConvexValue::Bytes(other_) = other else {
                    panic!("Invalid value: {other:?}");
                };
                self_.cmp(other_)
            },
            ConvexValue::Array(self_) => {
                let ConvexValue::Array(other_) = other else {
                    panic!("Invalid value: {other:?}");
                };
                self_.cmp(other_)
            },
            ConvexValue::Object(self_) => {
                let ConvexValue::Object(other_) = other else {
                    panic!("Invalid value: {other:?}");
                };
                self_.cmp(other_)
            },
        }
    }
}

impl PartialOrd for ConvexValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ConvexValue {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for ConvexValue {}

#[derive(Clone, Debug)]
pub struct TotalOrdF64(f64);

impl Ord for TotalOrdF64 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}
impl PartialOrd for TotalOrdF64 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for TotalOrdF64 {
    fn eq(&self, other: &Self) -> bool {
        matches!(self.cmp(other), Ordering::Equal)
    }
}
impl Eq for TotalOrdF64 {}
impl From<TotalOrdF64> for ConvexValue {
    fn from(f: TotalOrdF64) -> ConvexValue {
        ConvexValue::from(f.0)
    }
}
impl From<f64> for TotalOrdF64 {
    fn from(f: f64) -> Self {
        Self(f)
    }
}

impl From<TotalOrdF64> for f64 {
    fn from(f: TotalOrdF64) -> f64 {
        f.0
    }
}
