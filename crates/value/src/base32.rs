// Forked from https://github.com/andreasots/base32 @ 58909ac.
//
// Copyright (c) 2015 The base32 Developers - MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
use thiserror::Error;

// Crockford's Base32 alphabet (https://www.crockford.com/base32.html) with lowercase
// alphabetical characters. We also don't decode permissively.
const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Lookup table for decoding base32 characters
/// Maps ASCII byte values to 5-bit indices (0-31)
/// Invalid characters are marked with 0xFF
const DECODE_TABLE: [u8; 256] = {
    let mut table = [0xFFu8; 256];
    let mut i = 0;
    while i < 32 {
        table[ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    table
};

pub const fn encoded_len(len: usize) -> usize {
    let last_chunk = match len % 5 {
        0 => 0,
        1 => 2,
        2 => 4,
        3 => 5,
        4 => 7,
        _ => unreachable!(),
    };
    (len / 5) * 8 + last_chunk
}

pub const fn encoded_buffer_len(len: usize) -> usize {
    len.div_ceil(5) * 8
}

/// Writes the base32-encoding of `data[..len]` into `out`, which should have
/// length at least `encoded_buffer_len(data.len())`. Only the first
/// `encoded_len(data.len())` bytes of `out` should be used.
///
/// If `MAY_OVERREAD` is true, then assumes that we can read past `len` at least
/// 3 bytes past the last chunk & that `data[len..]` is all zeroes
#[inline]
pub fn encode_into<const MAY_OVERREAD: bool>(out: &mut [u8], data: &[u8], len: usize) {
    cfg_select! {
        all(
            target_arch = "aarch64",
            target_endian = "little",
            target_feature = "neon"
        ) => {
            // SAFETY: this block is gated behind `target_feature = "neon"`.
            unsafe {
                encode_into_neon::<MAY_OVERREAD>(out, data, len)
            }
        }
        _ => {
            encode_into_inner::<MAY_OVERREAD>(out, data, len)
        }
    }
}

#[inline]
#[cfg(any(
    test,
    not(all(
        target_arch = "aarch64",
        target_endian = "little",
        target_feature = "neon"
    ))
))]
fn encode_into_inner<const MAY_OVERREAD: bool>(out: &mut [u8], data: &[u8], len: usize) {
    // Process the input in chunks of length 5 (i.e 40 bits).
    for (i, out_chunk) in (0..len).step_by(5).zip(out.as_chunks_mut::<8>().0) {
        let block = if MAY_OVERREAD {
            // Read 8 bytes at a time for performance
            u64::from_be_bytes(*data[i..].first_chunk::<8>().unwrap())
        } else {
            let chunk = &data[i..len.min(i + 5)];
            let mut buf = [0; 8];
            buf[..chunk.len()].copy_from_slice(chunk);
            u64::from_be_bytes(buf)
        };
        // Turn our block of 5 groups of 8 bits into 8 groups of 5 bits.
        out_chunk[0] = ALPHABET[((block >> 59) & 0x1f) as usize];
        out_chunk[1] = ALPHABET[((block >> 54) & 0x1f) as usize];
        out_chunk[2] = ALPHABET[((block >> 49) & 0x1f) as usize];
        out_chunk[3] = ALPHABET[((block >> 44) & 0x1f) as usize];
        out_chunk[4] = ALPHABET[((block >> 39) & 0x1f) as usize];
        out_chunk[5] = ALPHABET[((block >> 34) & 0x1f) as usize];
        out_chunk[6] = ALPHABET[((block >> 29) & 0x1f) as usize];
        out_chunk[7] = ALPHABET[((block >> 24) & 0x1f) as usize];
    }
}

#[cfg(all(
    target_arch = "aarch64",
    target_endian = "little",
    target_feature = "neon"
))]
#[target_feature(enable = "neon")]
#[inline]
fn encode_into_neon<const MAY_OVERREAD: bool>(out: &mut [u8], data: &[u8], len: usize) {
    use std::{
        arch::aarch64::{
            int32x4_t,
            uint8x16_t,
            uint8x16x2_t,
            vand_u8,
            vdup_n_u8,
            vdupq_n_u32,
            vmovn_high_u32,
            vmovn_u16,
            vmovn_u32,
            vqtbl2_u8,
            vshlq_u32,
        },
        simd::u8x8,
    };
    // Process the input in chunks of length 5 (i.e 40 bits).
    for (i, out_chunk) in (0..len).step_by(5).zip(out.as_chunks_mut::<8>().0) {
        let block = if MAY_OVERREAD {
            // Read 8 bytes at a time for performance
            u64::from_be_bytes(*data[i..].first_chunk::<8>().unwrap())
        } else {
            let chunk = &data[i..len.min(i + 5)];
            let mut buf = [0; 8];
            buf[..chunk.len()].copy_from_slice(chunk);
            u64::from_be_bytes(buf)
        };
        // Extract bits 24..64 in eight 5-bit chunks. This is basically a
        // deposit_bits operation (it would be PDEP on Intel) but NEON
        // doesn't have that as an intrinsic, so do it manually by moving
        // groups of bits in parallel.
        // First split the block into two u32s so that we can splat them into two
        // u32x4s, then shift out the bits in parallel on each half.
        let (left, right) = ((block >> 44) as u32, (block >> 24) as u32);
        const SHIFT: int32x4_t =
            unsafe { std::mem::transmute::<[i32; 4], int32x4_t>([-15, -10, -5, 0]) };
        let left = vshlq_u32(vdupq_n_u32(left), SHIFT);
        let right = vshlq_u32(vdupq_n_u32(right), SHIFT);
        // Collapse the block back into a single u8x8 and mask off the bits
        // we want so we can do the table lookup.
        let block = vmovn_high_u32(vmovn_u32(left), right);
        let block = vand_u8(vmovn_u16(block), vdup_n_u8(0x1f));
        // Then use the lookup table.
        const ALPHABET_TABLE: uint8x16x2_t = unsafe {
            let &[tbl0, tbl1] = ALPHABET.as_chunks::<16>().0 else {
                panic!()
            };
            uint8x16x2_t(
                std::mem::transmute::<[u8; 16], uint8x16_t>(tbl0),
                std::mem::transmute::<[u8; 16], uint8x16_t>(tbl1),
            )
        };
        let block = vqtbl2_u8(ALPHABET_TABLE, block);
        *out_chunk = u8x8::from(block).to_array();
    }
}

pub fn encode(data: &[u8]) -> String {
    let mut out = vec![0; encoded_buffer_len(data.len())];
    encode_into::<false>(&mut out, data, data.len());
    // Truncate any extra zeros we added on the last block.
    out.truncate(encoded_len(data.len()));
    String::from_utf8(out).unwrap()
}

#[derive(Debug, Error, PartialEq)]
#[error("Invalid character {character} at position {position} in {string}")]
pub struct InvalidBase32Error {
    pub character: char,
    pub position: usize,
    pub string: String,
}

pub fn decode(data: &str) -> Result<Vec<u8>, InvalidBase32Error> {
    let data_bytes = data.as_bytes();
    let out_length = data_bytes.len() * 5 / 8;
    let mut out = Vec::with_capacity(out_length.div_ceil(5) * 5);

    // Process the data in 8 byte chunks
    for chunk in data_bytes.chunks(8) {
        let mut indexes = [0u8; 8];
        for (i, byte) in chunk.iter().enumerate() {
            // Safe, bounds-checked, and (after inlining) O(1) lookup
            let index = DECODE_TABLE[*byte as usize];

            if index == 0xFF {
                // Invalid character found
                let position = i + chunk.as_ptr().addr() - data_bytes.as_ptr().addr();
                return Err(InvalidBase32Error {
                    character: data[position..].chars().next().unwrap_or_else(|| {
                        panic!("Checked characters 0..{position} in {data} were one-byte")
                    }),
                    position,
                    string: data.to_string(),
                });
            }
            indexes[i] = index;
        }

        // Regroup our block of 8 5-bit indexes into 5 output bytes.
        out.push((indexes[0] << 3) | (indexes[1] >> 2));
        out.push((indexes[1] << 6) | (indexes[2] << 1) | (indexes[3] >> 4));
        out.push((indexes[3] << 4) | (indexes[4] >> 1));
        out.push((indexes[4] << 7) | (indexes[5] << 2) | (indexes[6] >> 3));
        out.push((indexes[6] << 5) | indexes[7]);
    }

    // Truncate any extra output from our last chunk.
    out.truncate(out_length);
    Ok(out)
}
