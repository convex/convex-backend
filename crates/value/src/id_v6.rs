//! We encode DocumentIds in two steps. First, we encode them into binary:
//! ```text
//! document_id = [ VInt(table_number) ] [ internal ID ] [ footer ]
//! ```
//! We use VInt encoding for the table number, which uses between one and five
//! bytes. Then, we write the 16 bytes of the internal ID as is. Finally, the
//! footer is a checksum of the ID so far XOR'd with the version number.
//! ```text
//! footer = fletcher16( [ VInt(table_number) ] [ internal ID ] ) ^ version
//! ```
use std::{
    array::from_fn,
    simd::{
        num::SimdUint as _,
        u8x16,
    },
    str::FromStr,
};

use thiserror::Error;

pub use crate::document_id::DeveloperDocumentId;
use crate::{
    base32::{
        self,
        InvalidBase32Error,
    },
    table_name::TableNumber,
    ResolvedDocumentId,
    TabletId,
};

// The table number is encoded in one to five bytes with VInt encoding.
const MIN_TABLE_NUMBER_LEN: usize = 1;
const MAX_TABLE_NUMBER_LEN: usize = 5;

// The internal ID is always 16 bytes.
const INTERNAL_ID_LEN: usize = 16;

// The footer is always two bytes and includes a Fletcher16 checksum of the rest
// of the ID XOR'd with the version number.
const FOOTER_LEN: usize = 2;
const VERSION: u16 = 0;

const MIN_BINARY_LEN: usize = MIN_TABLE_NUMBER_LEN + INTERNAL_ID_LEN + FOOTER_LEN;
const MIN_BASE32_LEN: usize = base32::encoded_len(MIN_BINARY_LEN);

const MAX_BINARY_LEN: usize = MAX_TABLE_NUMBER_LEN + INTERNAL_ID_LEN + FOOTER_LEN;
const MAX_BASE32_LEN: usize = base32::encoded_len(MAX_BINARY_LEN);

#[derive(Debug, Error)]
pub enum IdDecodeError {
    #[error("Unable to decode ID: ID wasn't valid base32")]
    InvalidBase32(#[from] InvalidBase32Error),
    #[error("Unable to decode ID: Invalid ID length {0}")]
    InvalidLength(usize),
    #[error("Unable to decode ID: Invalid table number")]
    InvalidTableNumber(#[from] VintDecodeError),
    #[error("Unable to decode ID: Invalid table number")]
    ZeroTableNumber,
    #[error("Unable to decode ID: Invalid ID version {0} (expected {1})")]
    InvalidIdVersion(u16, u16),
}

impl DeveloperDocumentId {
    pub fn encoded_len(&self) -> usize {
        let byte_length = vint_len(self.table().into()) + 16 + 2;
        base32::encoded_len(byte_length)
    }

    pub fn encode_into<'a>(&self, out: &'a mut EncodedDocumentIdBuffer) -> &'a str {
        let mut buf = [0; 28];

        let mut pos = 0;

        pos += vint_encode(self.table().into(), &mut buf[pos..]);
        let f1 = fletcher16(&buf[..pos]);

        buf[pos..(pos + 16)].copy_from_slice(&self.internal_id());
        let f2 = fletcher16_simd(&self.internal_id().0);
        pos += 16;

        let footer = fletcher16_combine(f1, f2, 16) ^ VERSION;
        buf[pos..(pos + 2)].copy_from_slice(&footer.to_le_bytes());
        pos += 2;

        base32::encode_into::<true>(&mut out.0, &buf, pos);

        unsafe { std::str::from_utf8_unchecked(&out.0[..base32::encoded_len(pos)]) }
    }

    pub fn encode(&self) -> String {
        self.encode_into(&mut Default::default()).to_owned()
    }

    /// Is the given string an ID that's not in its canonical encoding?
    pub fn is_noncanonical_id(s: &str) -> bool {
        let Ok(id) = Self::decode(s) else {
            return false;
        };
        s != id.encode()
    }

    pub fn decode(s: &str) -> Result<Self, IdDecodeError> {
        // NB: We want error paths to be as quick as possible, even if `s` is very long.
        // So, be sure to do the length check before decoding the base32.
        if s.len() < MIN_BASE32_LEN || MAX_BASE32_LEN < s.len() {
            return Err(IdDecodeError::InvalidLength(s.len()));
        }

        let buf = base32::decode(s)?;

        let mut pos = 0;

        let (table_number, bytes_read) = vint_decode(&buf[pos..])?;
        pos += bytes_read;
        let Ok(table_number) = TableNumber::try_from(table_number) else {
            return Err(IdDecodeError::ZeroTableNumber);
        };

        let internal_id = buf
            .get(pos..(pos + 16))
            .ok_or(IdDecodeError::InvalidLength(s.len()))?
            .try_into()
            .expect("Slice wasn't length 16?");
        pos += 16;

        let expected_footer = fletcher16(&buf[..pos]) ^ VERSION;

        let footer_bytes = buf
            .get(pos..(pos + 2))
            .ok_or(IdDecodeError::InvalidLength(s.len()))?
            .try_into()
            .expect("Slice wasn't length 2?");
        let footer = u16::from_le_bytes(footer_bytes);
        pos += 2;

        if expected_footer != footer {
            return Err(IdDecodeError::InvalidIdVersion(footer, expected_footer));
        }

        // Sanity check that we used all of our input bytes.
        if pos != buf.len() {
            return Err(IdDecodeError::InvalidLength(s.len()));
        }

        let id = DeveloperDocumentId::new(table_number, internal_id);

        // Check that decoding was one-to-one.
        // TODO: Checking base32 decoding above alone isn't sufficient, see
        // `test_id_decoding_one_to_one` below for a counterexample if we only check
        // that `base32::decode` is one-to-one.
        if id.encode_into(&mut Default::default()) != s {
            return Err(IdDecodeError::InvalidLength(s.len()));
        }

        Ok(id)
    }

    pub fn to_resolved(
        &self,
        f: impl Fn(TableNumber) -> anyhow::Result<TabletId>,
    ) -> anyhow::Result<ResolvedDocumentId> {
        Ok(ResolvedDocumentId {
            tablet_id: f(self.table())?,
            developer_id: *self,
        })
    }
}

impl From<ResolvedDocumentId> for DeveloperDocumentId {
    fn from(document_id: ResolvedDocumentId) -> Self {
        document_id.developer_id
    }
}

impl FromStr for DeveloperDocumentId {
    type Err = IdDecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        DeveloperDocumentId::decode(s)
    }
}

pub struct EncodedDocumentIdBuffer([u8; base32::encoded_buffer_len(MAX_BINARY_LEN)]);
impl Default for EncodedDocumentIdBuffer {
    fn default() -> Self {
        Self([0; base32::encoded_buffer_len(MAX_BINARY_LEN)])
    }
}

// Encode `n` with VInt encoding to `out`, returning the number of bytes
// written.
fn vint_encode(mut n: u32, out: &mut [u8]) -> usize {
    let mut pos = 0;
    loop {
        // If `n` has seven or fewer bits, we're done.
        if n < 0b1000_0000 {
            out[pos] = n as u8;
            pos += 1;
            break;
        }
        // Otherwise, emit the lowest seven bits with the continuation bit set.
        else {
            out[pos] = ((n & 0b0111_1111) | 0b1000_0000) as u8;
            pos += 1;
            n >>= 7;
        }
    }
    pos
}

// Compute the number of encoded bytes for `n` upfront.
fn vint_len(n: u32) -> usize {
    const ONE_BYTE_MAX: u32 = 1 << 7;
    const TWO_BYTE_MAX: u32 = 1 << 14;
    const THREE_BYTE_MAX: u32 = 1 << 21;
    const FOUR_BYTE_MAX: u32 = 1 << 28;

    match n {
        0..ONE_BYTE_MAX => 1,
        ONE_BYTE_MAX..TWO_BYTE_MAX => 2,
        TWO_BYTE_MAX..THREE_BYTE_MAX => 3,
        THREE_BYTE_MAX..FOUR_BYTE_MAX => 4,
        FOUR_BYTE_MAX.. => 5,
    }
}

#[derive(Debug, Error)]
pub enum VintDecodeError {
    #[error("Integer is too large")]
    TooLarge,
    #[error("Input truncated")]
    Truncated,
}

// Decode a single VInt from `buf`, returning the integer and number of bytes
// read.
fn vint_decode(buf: &[u8]) -> Result<(u32, usize), VintDecodeError> {
    let mut pos = 0;
    let mut n = 0;

    for i in 0.. {
        // If we've consumed more than five bytes, we won't fit in a u32.
        if i >= 5 {
            return Err(VintDecodeError::TooLarge);
        }
        let byte = buf
            .get(pos)
            .map(|b| *b as u32)
            .ok_or(VintDecodeError::Truncated)?;
        pos += 1;

        // Fold in the low seven bits, shifted to their final position.
        n |= (byte & 0b0111_1111) << (i * 7);

        // Stop if the continutation bit isn't set.
        if byte < 0b1000_0000 {
            break;
        }
    }
    Ok((n, pos))
}

// Compute the Fletcher-16 checksum with modulus 256 of `buf`.
//
// [1] Appendix I in https://www.ietf.org/rfc/rfc1145.txt
// [2] https://en.wikipedia.org/wiki/Fletcher%27s_checksum#Fletcher-16
fn fletcher16(buf: &[u8]) -> u16 {
    let mut c0 = 0u8;
    let mut c1 = 0u8;
    for byte in buf {
        c0 = c0.wrapping_add(*byte);
        c1 = c1.wrapping_add(c0);
    }
    ((c1 as u16) << 8) | (c0 as u16)
}
// Specialized version of fletcher16
fn fletcher16_simd(buf: &[u8; 16]) -> u16 {
    let data = u8x16::from_array(*buf);
    let c0 = data.reduce_sum();
    let c1 = (data * u8x16::from_array(from_fn(|i| 16 - i as u8))).reduce_sum();
    ((c1 as u16) << 8) | (c0 as u16)
}
// Combine two fletcher16 checksums:
// `fletcher16_combine(fletcher16(buf1), fletcher16(buf2), buf2.len())`
// equals `fletcher16([buf1, buf2].concat())`.
fn fletcher16_combine(b1: u16, b2: u16, b2len: usize) -> u16 {
    let [b10, b11] = b1.to_le_bytes();
    let [b20, b21] = b2.to_le_bytes();
    u16::from_le_bytes([
        b10.wrapping_add(b20),
        b11.wrapping_add(b21)
            .wrapping_add(b10.wrapping_mul(b2len as u8)),
    ])
}
