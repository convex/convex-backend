use std::collections::BTreeMap;

use anyhow::Context;
use serde::{
    Deserialize,
    Serialize,
};
use value::{
    ConvexValue,
    FieldName,
};

use super::VectorDimensions;
use crate::types::ObjectKey;

/// A qdrant Segment that's split into three separate parts, the qdrant Segment
/// which depends on an IdTracker implementation, which depends on a deleted
/// bitset.
///
/// Each file is stored independently, but they're composed to form a queryable
/// segment. The deleted bitset can be written to independently. The id tracker
/// can be queried independently. Using the segment requires all three files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentedVectorSegment {
    pub segment_key: ObjectKey,
    pub id_tracker_key: ObjectKey,
    pub deleted_bitset_key: ObjectKey,
    pub num_vectors: u32,
    pub num_deleted: u32,
    // A random UUID that can be used to identify a segment to determine if the
    // segment has changed during non-transactional index changes (compaction).
    pub id: String,
}

impl FragmentedVectorSegment {
    pub fn extract_key(
        object_fields: &mut BTreeMap<FieldName, ConvexValue>,
        serialized_field_name: &str,
    ) -> anyhow::Result<ObjectKey> {
        match object_fields.remove(serialized_field_name) {
            Some(ConvexValue::String(s)) => s.try_into(),
            _ => anyhow::bail!(
                "Invalid or missing `{serialized_field_name}` field for VectorMultiPartData",
            ),
        }
    }

    pub fn non_deleted_vectors(&self) -> anyhow::Result<u64> {
        let total_vectors = if self.num_vectors < self.num_deleted {
            // Some early segments have been created with num_vectors sent to the initially
            // available point count, which excluded deletes. If sufficient vectors are
            // deleted, that can result in num_deleted exceeding the initial num_vectors.
            // That doesn't strictly mean the segment is empty, but it should be close
            // enough and a backfill to fix these segments is complex.
            Ok(0)
        } else {
            self.num_vectors
                .checked_sub(self.num_deleted)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Failed to subtract {} from {}",
                        self.num_deleted,
                        self.num_vectors
                    )
                })
        };
        total_vectors.map(|value| value as u64)
    }

    /// The estimated size bytes based only on the non-deleted vectors in the
    /// segment.
    ///
    /// The actual size of the segment in s3 will be bigger due both to deleted
    /// vectors excluded from this size estimation and also overhead from the
    /// HNSW index (if present). Index overhead is larger as a percentage for
    /// small dimensional vectors than large dimensional vectors.
    pub fn non_deleted_size_bytes(&self, dimensions: VectorDimensions) -> anyhow::Result<u64> {
        Self::size_bytes(self.non_deleted_vectors()?, dimensions)
    }

    /// The estimated size bytes based on both deleted and non-deleted vectors
    /// in the segment.
    ///
    /// The actual size of the segment in s3 will be bigger due to the overhead
    /// from the HNSW index (if present). Index overhead is larger as a
    /// percentage for small dimensional vectors than large dimensional
    /// vectors.
    pub fn total_size_bytes(&self, dimensions: VectorDimensions) -> anyhow::Result<u64> {
        Self::size_bytes(self.num_vectors as u64, dimensions)
    }

    fn size_bytes(estimated_vectors: u64, dimensions: VectorDimensions) -> anyhow::Result<u64> {
        // A little extra paranoia since all of these numbers are not originally u64 and
        // can overflow u32.
        (estimated_vectors)
            .checked_mul(u32::from(dimensions) as u64)
            .and_then(|value| value.checked_mul(4_u64))
            .context("Overflowed size calculation!")
    }

    pub fn to_paths_proto(self) -> anyhow::Result<pb::searchlight::FragmentedVectorSegmentPaths> {
        Ok(pb::searchlight::FragmentedVectorSegmentPaths {
            segment: Some(pb::searchlight::StorageKey {
                storage_key: self.segment_key.into(),
            }),
            id_tracker: Some(pb::searchlight::StorageKey {
                storage_key: self.id_tracker_key.into(),
            }),
            deleted_bitset: Some(pb::searchlight::StorageKey {
                storage_key: self.deleted_bitset_key.into(),
            }),
        })
    }
}

impl From<FragmentedVectorSegment> for pb::searchlight::FragmentedVectorSegment {
    fn from(value: FragmentedVectorSegment) -> Self {
        Self {
            segment_key: value.segment_key.into(),
            id_tracker_key: value.id_tracker_key.into(),
            deleted_bitset_key: value.deleted_bitset_key.into(),
            num_vectors: value.num_vectors,
            num_deleted: value.num_deleted,
            id: value.id,
        }
    }
}

impl TryFrom<pb::searchlight::FragmentedVectorSegment> for FragmentedVectorSegment {
    type Error = anyhow::Error;

    fn try_from(value: pb::searchlight::FragmentedVectorSegment) -> Result<Self, Self::Error> {
        Ok(Self {
            segment_key: value.segment_key.try_into()?,
            id_tracker_key: value.id_tracker_key.try_into()?,
            deleted_bitset_key: value.deleted_bitset_key.try_into()?,
            num_vectors: value.num_vectors,
            num_deleted: value.num_deleted,
            id: value.id,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct SerializedFragmentedVectorSegment {
    pub segment_key: String,
    pub id_tracker_key: String,
    pub deleted_bitset_key: String,
    pub num_vectors: i64,
    pub num_deleted: i64,
    pub id: String,
}

impl TryFrom<FragmentedVectorSegment> for SerializedFragmentedVectorSegment {
    type Error = anyhow::Error;

    fn try_from(value: FragmentedVectorSegment) -> Result<Self, Self::Error> {
        Ok(Self {
            segment_key: value.segment_key.to_string(),
            id_tracker_key: value.id_tracker_key.to_string(),
            deleted_bitset_key: value.deleted_bitset_key.to_string(),
            num_vectors: value.num_vectors as i64,
            num_deleted: value.num_deleted as i64,
            id: value.id,
        })
    }
}

impl TryFrom<SerializedFragmentedVectorSegment> for FragmentedVectorSegment {
    type Error = anyhow::Error;

    fn try_from(value: SerializedFragmentedVectorSegment) -> Result<Self, Self::Error> {
        Ok(Self {
            segment_key: value.segment_key.try_into()?,
            id_tracker_key: value.id_tracker_key.try_into()?,
            deleted_bitset_key: value.deleted_bitset_key.try_into()?,
            num_vectors: value.num_vectors.try_into()?,
            num_deleted: value.num_deleted.try_into()?,
            id: value.id,
        })
    }
}
