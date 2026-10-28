use std::ops::{
    Deref,
    DerefMut,
};

use value::heap_size::HeapSize;

use crate::index::IndexKeyBytes;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BinaryKey {
    key: Vec<u8>,
}

impl HeapSize for BinaryKey {
    fn heap_size(&self) -> usize {
        self.key.heap_size()
    }
}

impl From<IndexKeyBytes> for BinaryKey {
    fn from(key: IndexKeyBytes) -> Self {
        key.0.into()
    }
}

impl From<Vec<u8>> for BinaryKey {
    fn from(key: Vec<u8>) -> Self {
        Self { key }
    }
}

impl From<BinaryKey> for IndexKeyBytes {
    fn from(b: BinaryKey) -> Self {
        IndexKeyBytes(b.into())
    }
}

impl From<BinaryKey> for Vec<u8> {
    fn from(b: BinaryKey) -> Self {
        b.key
    }
}

impl Deref for BinaryKey {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.key[..]
    }
}

impl DerefMut for BinaryKey {
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.key[..]
    }
}

impl BinaryKey {
    pub const fn min() -> Self {
        Self { key: Vec::new() }
    }

    pub const fn as_slice(&self) -> &[u8] {
        self.key.as_slice()
    }

    /// For any key `k`, `increment(k)` is the minimum key such that for
    /// all keys `s` where `k.is_prefix(s)`, we have `s < increment(k)`.
    pub fn increment(&self) -> Option<Self> {
        let mut incremented = self.clone();
        while let Some(byte) = incremented.last_mut() {
            if *byte < 255 {
                *byte += 1;
                return Some(incremented);
            }
            incremented.key.pop();
        }
        None
    }
}
