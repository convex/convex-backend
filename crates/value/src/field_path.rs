//! Types for representing paths to fields.

use std::{
    fmt,
    fmt::{
        Debug,
        Display,
    },
    str::FromStr,
};

use crate::{
    heap_size::{
        HeapSize,
        WithHeapSize,
    },
    ConvexValue,
    IdentifierFieldName,
};

/// A path to a field within an object type. Eventually this may become more
/// complicated as we want to be able to point to fields nested under arrays,
/// but for now, only allowing traversing objects is okay.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldPath {
    /// List of field names to the desired field. A top-level field is just a
    /// single element.
    ///
    /// Field paths should contain valid identifiers. Field paths are used when
    /// in indexes and filters -- `q.eq(q.field("foo.bar.baz"), true)`
    ///
    /// While this means a developer could end up with documents with properties
    /// that they can't index on / filter on in queries, it keeps the invariant
    /// that all filters can become indexes (and is also something we can relax
    /// later).
    fields: WithHeapSize<Vec<IdentifierFieldName>>,
}

impl FieldPath {
    pub fn new(fields: Vec<IdentifierFieldName>) -> anyhow::Result<Self> {
        anyhow::ensure!(!fields.is_empty());
        Ok(Self {
            fields: fields.into(),
        })
    }

    pub fn for_root_field(field: IdentifierFieldName) -> Self {
        Self {
            fields: vec![field].into(),
        }
    }

    pub fn fields(&self) -> &[IdentifierFieldName] {
        &self.fields
    }

    pub fn last(&self) -> &IdentifierFieldName {
        self.fields.last().expect("Empty FieldPath?")
    }
}

impl HeapSize for FieldPath {
    fn heap_size(&self) -> usize {
        self.fields.heap_size()
    }
}

impl FromStr for FieldPath {
    type Err = anyhow::Error;

    /// Extract a [`FieldPath`] from a string.
    /// A field path is a `.` separated list of field names.
    fn from_str(path: &str) -> anyhow::Result<Self> {
        let trimmed_path = path.trim_matches('.');
        if trimmed_path.is_empty() {
            anyhow::bail!("Empty path {}", path);
        }
        let fields = trimmed_path
            .split('.')
            .map(|s| s.parse())
            .collect::<anyhow::Result<Vec<IdentifierFieldName>>>()?;
        Self::new(fields)
    }
}

impl Debug for FieldPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl Display for FieldPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{}\"", self.fields.join("."))
    }
}

impl From<FieldPath> for Vec<IdentifierFieldName> {
    fn from(f: FieldPath) -> Self {
        f.fields.into()
    }
}

impl From<FieldPath> for String {
    fn from(f: FieldPath) -> Self {
        f.fields.join(".")
    }
}

impl TryFrom<FieldPath> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(f: FieldPath) -> anyhow::Result<Self> {
        ConvexValue::try_from(String::from(f))
    }
}

impl TryFrom<ConvexValue> for FieldPath {
    type Error = anyhow::Error;

    fn try_from(value: ConvexValue) -> Result<Self, Self::Error> {
        if let ConvexValue::String(s) = value {
            return s.parse();
        }
        anyhow::bail!("Invalid field name: {:?}", value);
    }
}

impl TryFrom<Vec<FieldPath>> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(v: Vec<FieldPath>) -> anyhow::Result<Self> {
        Ok(ConvexValue::Array(
            v.into_iter()
                .map(|f| f.try_into())
                .collect::<anyhow::Result<Vec<_>>>()?
                .try_into()?,
        ))
    }
}

impl TryFrom<ConvexValue> for Vec<FieldPath> {
    type Error = anyhow::Error;

    fn try_from(value: ConvexValue) -> Result<Self, Self::Error> {
        if let ConvexValue::Array(a) = value {
            return a
                .into_iter()
                .map(FieldPath::try_from)
                .collect::<Result<Vec<_>, _>>();
        }
        anyhow::bail!("Invalid field list: {:?}", value)
    }
}
