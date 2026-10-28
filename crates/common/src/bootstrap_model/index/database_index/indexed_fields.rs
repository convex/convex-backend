use std::{
    collections::HashSet,
    fmt::Display,
    iter,
    ops::Deref,
};

use pb::common::FieldPath as FieldPathProto;
use value::{
    heap_size::{
        HeapSize,
        WithHeapSize,
    },
    utils::display_sequence,
    ConvexValue,
};

use crate::{
    bootstrap_model::index::{
        index_validation_error,
        MAX_INDEX_FIELDS_SIZE,
    },
    document::{
        CREATION_TIME_FIELD,
        ID_FIELD_PATH,
    },
    paths::FieldPath,
};

/// Ordered list of fields in a multi-column index. This list only contains
/// the user-specified indexes: the system adds the `_id` column at the
/// end to guarantee uniqueness, but this trailing `_id` field isn't
/// included in this type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IndexedFields(WithHeapSize<Vec<FieldPath>>);

impl IndexedFields {
    pub const fn by_id() -> Self {
        IndexedFields(WithHeapSize::new_vec())
    }

    pub fn creation_time() -> Self {
        let field_path = FieldPath::new(vec![CREATION_TIME_FIELD.to_owned()])
            .expect("Invalid _creationTime field path");
        IndexedFields(vec![field_path].into())
    }

    pub fn iter_with_id(&self) -> impl Iterator<Item = &FieldPath> {
        self.iter().chain(iter::once(&*ID_FIELD_PATH))
    }
}

impl HeapSize for IndexedFields {
    fn heap_size(&self) -> usize {
        self.0.heap_size()
    }
}

impl Display for IndexedFields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        display_sequence(f, ["[", "]"], self.0.iter())
    }
}

impl Deref for IndexedFields {
    type Target = Vec<FieldPath>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<Vec<FieldPath>> for IndexedFields {
    type Error = anyhow::Error;

    fn try_from(fields: Vec<FieldPath>) -> anyhow::Result<Self> {
        if fields.len() > MAX_INDEX_FIELDS_SIZE {
            anyhow::bail!(index_validation_error::too_many_fields(
                MAX_INDEX_FIELDS_SIZE
            ));
        }

        if fields.contains(&ID_FIELD_PATH) {
            anyhow::bail!(index_validation_error::fields_contain_id())
        }

        let mut seen: HashSet<_> = HashSet::new();
        for field in fields.iter() {
            if !seen.insert(field) {
                anyhow::bail!(index_validation_error::fields_not_unique_within_index(
                    field
                ));
            }
        }
        Ok(Self(fields.into()))
    }
}

impl IntoIterator for IndexedFields {
    type IntoIter = std::vec::IntoIter<Self::Item>;
    type Item = FieldPath;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl From<IndexedFields> for Vec<FieldPath> {
    fn from(fields: IndexedFields) -> Self {
        fields.0.into()
    }
}

impl TryFrom<IndexedFields> for ConvexValue {
    type Error = anyhow::Error;

    fn try_from(fields: IndexedFields) -> anyhow::Result<Self> {
        let vec: Vec<_> = fields.0.into();
        vec.try_into()
    }
}

impl TryFrom<ConvexValue> for IndexedFields {
    type Error = anyhow::Error;

    fn try_from(val: ConvexValue) -> anyhow::Result<Self> {
        if let ConvexValue::Array(arr) = val {
            let fields: Vec<FieldPath> = arr
                .iter()
                .cloned()
                .map(FieldPath::try_from)
                .collect::<anyhow::Result<Vec<_>>>()?;
            Ok(IndexedFields(fields.into()))
        } else {
            anyhow::bail!("Invalid value for IndexedFields")
        }
    }
}

impl From<IndexedFields> for Vec<FieldPathProto> {
    fn from(fields: IndexedFields) -> Self {
        Vec::<FieldPath>::from(fields)
            .into_iter()
            .map(|f| f.into())
            .collect()
    }
}
