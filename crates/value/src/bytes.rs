use std::{
    fmt,
    ops::Deref,
};

use super::size::Size;
use crate::{
    heap_size::HeapSize,
    size::check_system_size,
};

/// Wrapper on `Vec<u8>` that enforces size limits.
#[derive(Clone, Debug, Hash)]
pub struct ConvexBytes(Vec<u8>);

impl TryFrom<Vec<u8>> for ConvexBytes {
    type Error = anyhow::Error;

    fn try_from(v: Vec<u8>) -> anyhow::Result<Self> {
        let size = 1 + v.len() + 1;
        check_system_size(size)?;
        Ok(ConvexBytes(v))
    }
}

impl From<ConvexBytes> for Vec<u8> {
    fn from(bytes: ConvexBytes) -> Self {
        bytes.0
    }
}

impl AsRef<[u8]> for ConvexBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl Deref for ConvexBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl fmt::Display for ConvexBytes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let encoded = base64::encode(&self.0);
        write!(f, "b{encoded:?}")
    }
}

impl Size for ConvexBytes {
    fn size(&self) -> usize {
        1 + self.0.len() + 1
    }

    fn nesting(&self) -> usize {
        0
    }
}

impl HeapSize for ConvexBytes {
    fn heap_size(&self) -> usize {
        self.0.heap_size()
    }
}
