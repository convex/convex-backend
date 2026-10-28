use std::{
    self,
    fmt,
    hash::{
        Hash,
        Hasher,
    },
    ops::Deref,
};

use errors::ErrorMetadata;
use sync_types::types::SerializedArgs;

use super::size::Size;
use crate::{
    size::{
        array_size,
        check_nesting,
        check_system_size,
    },
    utils::display_sequence,
    walk::ConvexValueType,
    ConvexValue,
    MAX_NESTING,
    MAX_SIZE,
};

const MAX_ARRAY_LEN: usize = 8192;

/// Wrapper on `Vec<ConvexValue>` that enforces size limits.
#[derive(Clone)]
pub struct ConvexArray {
    // Precomputed `1 + size(v1) + ... + size(vN) + 1`
    size: u32,
    // Precomputed `1 + max(nesting(v1), ..., nesting(vN))`.
    nesting: u8,

    items: Box<[ConvexValue]>,
}

impl ConvexArray {
    pub fn empty() -> Self {
        Self {
            size: 2,
            nesting: 1,
            items: Box::new([]),
        }
    }

    pub fn into_serialized_args(&self) -> anyhow::Result<SerializedArgs> {
        let serialized = serde_json::value::to_raw_value(&crate::json_value::SerializeValue::new(
            ConvexValueType::<&ConvexValue>::Array(self),
        ))?;
        Ok(SerializedArgs::from_raw(serialized))
    }
}

impl IntoIterator for ConvexArray {
    type IntoIter = std::vec::IntoIter<ConvexValue>;
    type Item = ConvexValue;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a> IntoIterator for &'a ConvexArray {
    type IntoIter = std::slice::Iter<'a, ConvexValue>;
    type Item = &'a ConvexValue;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

pub(crate) fn check_array_len(len: usize) -> anyhow::Result<()> {
    if len > MAX_ARRAY_LEN {
        anyhow::bail!(ErrorMetadata::bad_request(
            "ArrayTooLong",
            format!("Array length is too long ({len} > maximum length {MAX_ARRAY_LEN})"),
        ));
    }
    Ok(())
}

impl TryFrom<Vec<ConvexValue>> for ConvexArray {
    type Error = anyhow::Error;

    fn try_from(items: Vec<ConvexValue>) -> anyhow::Result<Self> {
        check_array_len(items.len())?;
        let size = array_size(&items);
        check_system_size(size)?;
        let nesting = 1 + items.iter().map(|v| v.nesting()).max().unwrap_or(0);
        check_nesting(nesting)?;
        const {
            assert!(MAX_SIZE <= u32::MAX as usize);
            assert!(MAX_NESTING <= u8::MAX as usize);
        }
        Ok(Self {
            size: size as u32,
            nesting: nesting as u8,
            items: items.into_boxed_slice(),
        })
    }
}

impl From<ConvexArray> for Vec<ConvexValue> {
    fn from(array: ConvexArray) -> Self {
        array.items.into_vec()
    }
}

impl Deref for ConvexArray {
    type Target = [ConvexValue];

    fn deref(&self) -> &[ConvexValue] {
        &self.items[..]
    }
}

impl fmt::Debug for ConvexArray {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.items, f)
    }
}

impl fmt::Display for ConvexArray {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        display_sequence(f, ["[", "]"], self.items.iter())
    }
}

// We only compute the hash on the non-derived state but have an assert in
// `PartialEq` to make sure the derived state always matches.
impl Hash for ConvexArray {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.items.hash(hasher)
    }
}

impl PartialEq for ConvexArray {
    fn eq(&self, other: &Self) -> bool {
        if self.items == other.items {
            // We're just comparing based on items but make sure the derived data always
            // matches.
            assert_eq!(self.size, other.size);
            assert_eq!(self.nesting, other.nesting);
            return true;
        }
        false
    }
}

impl Eq for ConvexArray {}

impl Size for ConvexArray {
    fn size(&self) -> usize {
        self.size as usize
    }

    fn nesting(&self) -> usize {
        self.nesting as usize
    }
}

#[macro_export]
/// Create an array.
///
/// Uses the same syntax as vec![].
macro_rules! array {
    () => (
        $crate::ConvexArray::empty()
    );
    ($($x:expr),+ $(,)?) => (
        $crate::ConvexArray::try_from(
            vec![$($x),+]
        )
    );
}
