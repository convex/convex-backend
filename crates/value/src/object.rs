//! Object type used for a document's top-level value.

use std::{
    borrow::Borrow,
    collections::BTreeMap,
    fmt,
    hash::Hash,
    ops::Deref,
};

use errors::ErrorMetadata;

use super::size::{
    check_nesting,
    Size,
};
use crate::{
    field_path::FieldPath,
    size::{
        check_system_size,
        object_size,
    },
    utils::display_map,
    ConvexValue,
    FieldName,
    Namespace,
    MAX_NESTING,
    MAX_SIZE,
};

pub const MAX_OBJECT_FIELDS: usize = 1024;

/// A mapping of field name to [`ConvexValue`] that's used as the contents of a
/// Convex Document.
///
/// To mutate an object, convert it to a `BTreeMap` using `into()`, mutate the
/// map, and then use `Object::try_from` to convert it back to an object. This
/// ensures that we check the object invariants after the modifications.
#[derive(Clone)]
#[repr(C)]
pub struct ConvexObject {
    // Enables niche optimization for ConvexValue, allowing `size_of::<ConvexValue>() ==
    // size_of::<ConvexObject>()`
    _niche: Niche,

    // Precomputed 1 + max(nesting(v1), ..., nesting(vN))
    nesting: u8,
    // Precomputed 1 + (len(field1) + 1) + size(v1) + ... + (len(fieldN) + 1) + size(vN) + 1
    size: u32,

    fields: BTreeMap<FieldName, ConvexValue>,
}

impl fmt::Debug for ConvexObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConvexObject")
            .field("nesting", &self.nesting)
            .field("size", &self.size)
            .field("fields", &self.fields)
            .finish()
    }
}

// A u8 with only one valid value. The value 7 is chosen to line up with the
// discriminant of the `Object` variant of `ConvexValue`
#[derive(Copy, Clone)]
#[repr(u8)]
enum Niche {
    Niche = 7,
}

impl PartialEq for ConvexObject {
    fn eq(&self, other: &Self) -> bool {
        if self.fields == other.fields {
            // We're just comparing based on fields but make sure the derived data always
            // matches.
            assert_eq!(self.size, other.size);
            assert_eq!(self.nesting, other.nesting);
            return true;
        }
        false
    }
}

impl Eq for ConvexObject {}

pub(crate) fn check_field_count(num_fields: usize) -> anyhow::Result<()> {
    if num_fields > MAX_OBJECT_FIELDS {
        anyhow::bail!(ErrorMetadata::bad_request(
            "TooManyFieldsError",
            format!(
                "Object has too many fields ({num_fields} > maximum number {MAX_OBJECT_FIELDS})"
            )
        ));
    }
    Ok(())
}

impl TryFrom<BTreeMap<FieldName, ConvexValue>> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(fields: BTreeMap<FieldName, ConvexValue>) -> anyhow::Result<Self> {
        check_field_count(fields.len())?;
        let size = object_size(fields.iter());
        check_system_size(size)?;
        let nesting = 1 + fields.values().map(|v| v.nesting()).max().unwrap_or(0);
        check_nesting(nesting)?;
        const {
            assert!(MAX_SIZE <= u32::MAX as usize);
            assert!(MAX_NESTING <= u8::MAX as usize);
        }
        Ok(Self {
            _niche: Niche::Niche,
            size: size as u32,
            nesting: nesting as u8,
            fields,
        })
    }
}

impl ConvexObject {
    /// Create an empty object.
    pub fn empty() -> Self {
        Self {
            _niche: Niche::Niche,
            size: 1 + 1,
            nesting: 1,
            fields: BTreeMap::default(),
        }
    }

    /// Generate an object with given key from the value.
    pub fn for_value(key: FieldName, value: ConvexValue) -> anyhow::Result<Self> {
        let mut fields = BTreeMap::new();
        fields.insert(key, value);
        fields.try_into()
    }

    /// Get a value at a given field name.
    pub fn get<Q>(&self, field_name: &Q) -> Option<&ConvexValue>
    where
        FieldName: Borrow<Q>,
        Q: ?Sized + Eq + Ord,
    {
        self.fields.get(field_name)
    }

    /// Does the object have a given field?
    pub fn contains_field(&self, field_name: &FieldName) -> bool {
        self.get(field_name).is_some()
    }

    /// Iterate over an object's fields and values.
    pub fn iter(&self) -> impl Iterator<Item = (&FieldName, &ConvexValue)> {
        self.fields.iter()
    }

    /// Iterate over an object's keys
    pub fn keys(&self) -> impl Iterator<Item = &FieldName> {
        self.fields.keys()
    }

    /// Extract a value below a field path in an object.
    pub fn get_path(&self, field_path: &FieldPath) -> Option<&ConvexValue> {
        let first = field_path.fields().first()?; // return None if empty path
        let mut v = self.fields.get::<str>(first.borrow())?;
        for field in field_path.fields().iter().skip(1) {
            match v {
                ConvexValue::Object(o) => v = o.fields.get::<str>(field.borrow())?,
                _ => return None,
            }
        }
        Some(v)
    }

    /// Inserts the provided key-value pair, replacing any existing field.
    /// Returns an error if the resulting object would exceed size limits.
    pub fn insert(self, key: FieldName, value: ConvexValue) -> anyhow::Result<Self> {
        let mut fields = self.fields;
        fields.insert(key, value);
        fields.try_into()
    }

    /// Returns a new object with only the fields that match the filter.
    pub fn filter_fields(self, filter: impl Fn(&FieldName) -> bool) -> Self {
        let filtered_fields: BTreeMap<_, _> =
            self.fields.into_iter().filter(|(k, _)| filter(k)).collect();
        Self::try_from(filtered_fields)
            .expect("Filtering an object should always produce a smaller, thus valid object")
    }

    /// Returns a new object with the system fields removed.
    pub fn filter_system_fields(self) -> Self {
        self.filter_fields(|k| !k.is_system())
    }
}

impl IntoIterator for ConvexObject {
    type IntoIter = std::collections::btree_map::IntoIter<FieldName, ConvexValue>;
    type Item = (FieldName, ConvexValue);

    fn into_iter(self) -> Self::IntoIter {
        self.fields.into_iter()
    }
}

impl fmt::Display for ConvexObject {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        display_map(f, ["{", "}"], self.iter())
    }
}

impl From<ConvexObject> for BTreeMap<FieldName, ConvexValue> {
    fn from(o: ConvexObject) -> Self {
        o.fields
    }
}

// Helpers for parsing ConvexObject.
pub fn remove_string(
    fields: &mut BTreeMap<FieldName, ConvexValue>,
    field: &str,
) -> anyhow::Result<String> {
    match fields.remove(field) {
        Some(ConvexValue::String(s)) => Ok(s.into()),
        v => anyhow::bail!("expected string for {field}, got {v:?}"),
    }
}
pub fn remove_nullable_string(
    fields: &mut BTreeMap<FieldName, ConvexValue>,
    field: &str,
) -> anyhow::Result<Option<String>> {
    match fields.remove(field) {
        Some(ConvexValue::String(s)) => Ok(Some(s.into())),
        Some(ConvexValue::Null) => Ok(None),
        None => Ok(None),
        v => anyhow::bail!("expected string for {field}, got {v:?}"),
    }
}
pub fn remove_boolean(
    fields: &mut BTreeMap<FieldName, ConvexValue>,
    field: &str,
) -> anyhow::Result<bool> {
    match fields.remove(field) {
        Some(ConvexValue::Boolean(s)) => Ok(s),
        v => anyhow::bail!("expected boolean for {field}, got {v:?}"),
    }
}

pub fn remove_int64(
    fields: &mut BTreeMap<FieldName, ConvexValue>,
    field: &str,
) -> anyhow::Result<i64> {
    match fields.remove(field) {
        Some(ConvexValue::Int64(i)) => Ok(i),
        v => anyhow::bail!("expected int for {field}, got {v:?}"),
    }
}

pub fn remove_nullable_int64(
    fields: &mut BTreeMap<FieldName, ConvexValue>,
    field: &str,
) -> anyhow::Result<Option<i64>> {
    match fields.remove(field) {
        Some(ConvexValue::Int64(i)) => Ok(Some(i)),
        Some(ConvexValue::Null) => Ok(None),
        None => Ok(None),
        v => anyhow::bail!("expected int or null for {field}, got {v:?}"),
    }
}

pub fn remove_object<E, T: TryFrom<ConvexObject, Error = E>>(
    fields: &mut BTreeMap<FieldName, ConvexValue>,
    field: &str,
) -> anyhow::Result<T>
where
    anyhow::Error: From<E>,
{
    match fields.remove(field) {
        Some(ConvexValue::Object(o)) => Ok(o.try_into()?),
        v => anyhow::bail!("expected object for {field}, got {v:?}"),
    }
}
pub fn remove_nullable_object<E, T: TryFrom<ConvexObject, Error = E>>(
    fields: &mut BTreeMap<FieldName, ConvexValue>,
    field: &str,
) -> anyhow::Result<Option<T>>
where
    anyhow::Error: From<E>,
{
    match fields.remove(field) {
        Some(ConvexValue::Object(o)) => Ok(Some(o.try_into()?)),
        Some(ConvexValue::Null) => Ok(None),
        None => Ok(None),
        v => anyhow::bail!("expected object or null for {field}, got {v:?}"),
    }
}
pub fn remove_vec(
    fields: &mut BTreeMap<FieldName, ConvexValue>,
    field: &str,
) -> anyhow::Result<Vec<ConvexValue>> {
    match fields.remove(field) {
        Some(ConvexValue::Array(a)) => Ok(a.into()),
        v => anyhow::bail!("expected array for {field}, got {v:?}"),
    }
}
pub fn remove_vec_of_strings(
    fields: &mut BTreeMap<FieldName, ConvexValue>,
    field: &str,
) -> anyhow::Result<Vec<String>> {
    let values = remove_vec(fields, field)?;
    values
        .into_iter()
        .map(|value| match value {
            ConvexValue::String(s) => anyhow::Ok(s.into()),
            v => anyhow::bail!("expected string in array at {field}, got {v:?}"),
        })
        .try_collect()
}

pub fn remove_nullable_vec(
    fields: &mut BTreeMap<FieldName, ConvexValue>,
    field: &str,
) -> anyhow::Result<Option<Vec<ConvexValue>>> {
    match fields.remove(field) {
        Some(ConvexValue::Array(a)) => Ok(Some(a.into())),
        None => Ok(None),
        v => anyhow::bail!("expected array for {field}, got {v:?}"),
    }
}
pub fn remove_nullable_vec_of_strings(
    fields: &mut BTreeMap<FieldName, ConvexValue>,
    field: &str,
) -> anyhow::Result<Option<Vec<String>>> {
    let Some(values) = remove_nullable_vec(fields, field)? else {
        return Ok(None);
    };
    Ok(Some(
        values
            .into_iter()
            .map(|value| match value {
                ConvexValue::String(s) => anyhow::Ok(s.into()),
                v => anyhow::bail!("expected string in array at {field}, got {v:?}"),
            })
            .try_collect()?,
    ))
}

impl Size for ConvexObject {
    fn size(&self) -> usize {
        self.size as usize
    }

    fn nesting(&self) -> usize {
        self.nesting as usize
    }
}

impl Deref for ConvexObject {
    type Target = BTreeMap<FieldName, ConvexValue>;

    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

impl Hash for ConvexObject {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.fields.hash(state);
    }
}
