use std::{
    fmt,
    ops::Deref,
};

use super::size::Size;
use crate::{
    heap_size::HeapSize,
    size::check_system_size,
};

/// Wrapper on `String` that enforces size limits.
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord, derive_more::Display)]
pub struct ConvexString(String);

impl TryFrom<String> for ConvexString {
    type Error = anyhow::Error;

    fn try_from(s: String) -> anyhow::Result<Self> {
        let size = 1 + s.len() + 1;
        check_system_size(size)?;
        Ok(ConvexString(s))
    }
}

impl<'a> TryFrom<&'a str> for ConvexString {
    type Error = anyhow::Error;

    fn try_from(s: &'a str) -> anyhow::Result<Self> {
        s.to_owned().try_into()
    }
}

impl From<ConvexString> for String {
    fn from(string: ConvexString) -> Self {
        string.0
    }
}

impl Deref for ConvexString {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0[..]
    }
}

impl AsRef<str> for ConvexString {
    fn as_ref(&self) -> &str {
        &self.0[..]
    }
}

impl fmt::Debug for ConvexString {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl Size for ConvexString {
    fn size(&self) -> usize {
        1 + self.0.len() + 1
    }

    fn nesting(&self) -> usize {
        0
    }
}

impl HeapSize for ConvexString {
    fn heap_size(&self) -> usize {
        self.0.heap_size()
    }
}
