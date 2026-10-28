use errors::ErrorMetadata;
use humansize::{
    FormatSize,
    BINARY,
};

pub const MAX_SIZE: usize = 1 << 25; // 32 MB
pub const MAX_NESTING: usize = 64;
pub const VALUE_TOO_LARGE_SHORT_MSG: &str = "ValueTooLargeError";

/// Trait for enforcing different notions of "size" for values.
pub trait Size {
    /// How "large" is the given value? We define a (somewhat) arbitrary notion
    /// of size on `Value`s to bound the amount of memory our system uses
    /// per value.
    fn size(&self) -> usize;

    /// How "nested" is the given value? Primitive types, like integers, have
    /// nesting 0, while a list of integers has nesting 1, a list of list of
    /// integers has nesting 2, and so on.
    fn nesting(&self) -> usize;
}

/// The size of an array-like sequence: one byte for each delimiter plus the
/// elements' sizes.
pub fn array_size<S: Size>(items: &[S]) -> usize {
    1 + items.iter().map(Size::size).sum::<usize>() + 1
}

/// The size of an object-like mapping: one byte for each delimiter plus, for
/// each field, its name, a separator byte, and its value's size.
pub fn object_size<'a, S: Size + 'a>(
    fields: impl Iterator<Item = (&'a crate::FieldName, &'a S)>,
) -> usize {
    1 + fields
        .map(|(name, value)| name.len() + 1 + value.size())
        .sum::<usize>()
        + 1
}

pub fn check_system_size(size: usize) -> anyhow::Result<()> {
    if size > MAX_SIZE {
        // TODO CX-4516 - differentiate this from the check_user_size
        anyhow::bail!(ErrorMetadata::bad_request(
            VALUE_TOO_LARGE_SHORT_MSG,
            format!(
                "Value is too large ({} > maximum size {})",
                size.format_size(BINARY),
                MAX_SIZE.format_size(BINARY),
            )
        ));
    }
    Ok(())
}

pub fn check_nesting(nesting: usize) -> anyhow::Result<()> {
    if nesting > MAX_NESTING {
        anyhow::bail!(ErrorMetadata::bad_request(
            "TooNestedError",
            format!(
                "Value is too nested (nested {nesting} levels deep > maximum nesting \
                 {MAX_NESTING})"
            )
        ))
    }
    Ok(())
}
