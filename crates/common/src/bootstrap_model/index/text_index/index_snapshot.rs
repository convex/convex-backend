use anyhow::Context;
use serde::{
    Deserialize,
    Serialize,
};
use value::{
    serde::WithUnknown,
    ConvexObject,
};

use crate::types::{
    ObjectKey,
    Timestamp,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextIndexSnapshot {
    pub data: TextIndexSnapshotData,
    pub ts: Timestamp,
    pub version: TextSnapshotVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextIndexSnapshotData {
    /// Multi-segment format that supports incremental builds.
    MultiSegment(Vec<FragmentedTextSegment>),
    /// An unrecognized format that can be round tripped without being modified.
    /// Same as a proto with unknown fields.
    /// Used because we don't want to delete / recreate index metadata
    /// unintentionally when changing versions and rolling services
    /// backwards/forwards.
    Unknown(ConvexObject),
}

impl From<FragmentedTextSegment> for pb::searchlight::FragmentedTextSegment {
    fn from(value: FragmentedTextSegment) -> Self {
        fn storage_key_from_object_key(
            object_key: ObjectKey,
        ) -> Option<pb::searchlight::StorageKey> {
            Some(pb::searchlight::StorageKey {
                storage_key: object_key.into(),
            })
        }

        Self {
            segment: storage_key_from_object_key(value.segment_key),
            id_tracker: storage_key_from_object_key(value.id_tracker_key),
            deleted_terms_table: storage_key_from_object_key(value.deleted_terms_table_key),
            alive_bitset: storage_key_from_object_key(value.alive_bitset_key),
            num_indexed_documents: Some(value.num_indexed_documents),
            num_deleted_documents: Some(value.num_deleted_documents),
            size_bytes_total: Some(value.size_bytes_total),
            id: Some(value.id),
        }
    }
}

impl TryFrom<pb::searchlight::FragmentedTextSegment> for FragmentedTextSegment {
    type Error = anyhow::Error;

    fn try_from(value: pb::searchlight::FragmentedTextSegment) -> Result<Self, Self::Error> {
        Ok(Self {
            segment_key: value
                .segment
                .context("Missing segment")?
                .storage_key
                .try_into()?,
            id_tracker_key: value
                .id_tracker
                .context("Missing id tracker")?
                .storage_key
                .try_into()?,
            deleted_terms_table_key: value
                .deleted_terms_table
                .context("Missing deleted terms")?
                .storage_key
                .try_into()?,
            alive_bitset_key: value
                .alive_bitset
                .context("Missing alive bitset")?
                .storage_key
                .try_into()?,
            num_indexed_documents: value.num_indexed_documents.context("Missing num indexed")?,
            num_deleted_documents: value.num_deleted_documents.context("Missing num deleted")?,
            size_bytes_total: value.size_bytes_total.context("Missing size bytes total")?,
            id: value.id.context("Missing id")?,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "data_type", rename_all = "PascalCase")]
enum SerializedTextIndexSnapshotData {
    MultiSegment {
        segments: Vec<SerializedFragmentedTextSegment>,
    },
}

impl TryFrom<WithUnknown<SerializedTextIndexSnapshotData>> for TextIndexSnapshotData {
    type Error = anyhow::Error;

    fn try_from(value: WithUnknown<SerializedTextIndexSnapshotData>) -> Result<Self, Self::Error> {
        match value {
            WithUnknown::Known(SerializedTextIndexSnapshotData::MultiSegment {
                segments: serialized_segments,
            }) => {
                let segments: Vec<FragmentedTextSegment> = serialized_segments
                    .into_iter()
                    .map(FragmentedTextSegment::try_from)
                    .collect::<anyhow::Result<Vec<_>>>()?;
                Ok(TextIndexSnapshotData::MultiSegment(segments))
            },
            WithUnknown::Unknown(unknown) => Ok(TextIndexSnapshotData::Unknown(unknown)),
        }
    }
}

impl TryFrom<TextIndexSnapshotData> for WithUnknown<SerializedTextIndexSnapshotData> {
    type Error = anyhow::Error;

    fn try_from(value: TextIndexSnapshotData) -> Result<Self, Self::Error> {
        match value {
            TextIndexSnapshotData::MultiSegment(segments) => {
                let serialized_segments: Vec<SerializedFragmentedTextSegment> = segments
                    .into_iter()
                    .map(SerializedFragmentedTextSegment::try_from)
                    .collect::<anyhow::Result<Vec<_>>>()?;
                Ok(WithUnknown::Known(
                    SerializedTextIndexSnapshotData::MultiSegment {
                        segments: serialized_segments,
                    },
                ))
            },
            TextIndexSnapshotData::Unknown(unknown) => Ok(WithUnknown::Unknown(unknown)),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct SerializedFragmentedTextSegment {
    pub segment_key: String,
    pub id_tracker_key: String,
    pub deleted_terms_table_key: String,
    pub alive_bitset_key: String,
    pub num_indexed_documents: u64,
    pub num_deleted_documents: u64,
    pub size_bytes_total: u64,
    pub id: String,
}

impl TryFrom<FragmentedTextSegment> for SerializedFragmentedTextSegment {
    type Error = anyhow::Error;

    fn try_from(value: FragmentedTextSegment) -> anyhow::Result<Self> {
        Ok(Self {
            segment_key: value.segment_key.to_string(),
            id_tracker_key: value.id_tracker_key.to_string(),
            deleted_terms_table_key: value.deleted_terms_table_key.to_string(),
            alive_bitset_key: value.alive_bitset_key.to_string(),
            num_indexed_documents: value.num_indexed_documents,
            num_deleted_documents: value.num_deleted_documents,
            size_bytes_total: value.size_bytes_total,
            id: value.id,
        })
    }
}

impl TryFrom<SerializedFragmentedTextSegment> for FragmentedTextSegment {
    type Error = anyhow::Error;

    fn try_from(value: SerializedFragmentedTextSegment) -> Result<Self, Self::Error> {
        Ok(Self {
            segment_key: value.segment_key.try_into()?,
            id_tracker_key: value.id_tracker_key.try_into()?,
            deleted_terms_table_key: value.deleted_terms_table_key.try_into()?,
            alive_bitset_key: value.alive_bitset_key.try_into()?,
            num_indexed_documents: value.num_indexed_documents,
            num_deleted_documents: value.num_deleted_documents,
            size_bytes_total: value.size_bytes_total,
            id: value.id,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentedTextSegment {
    pub segment_key: ObjectKey,
    pub id_tracker_key: ObjectKey,
    pub deleted_terms_table_key: ObjectKey,
    pub alive_bitset_key: ObjectKey,
    // 2^63 ~= 9.2 * 10^18. We only support i64 in Convex.
    pub num_indexed_documents: u64,
    // 2^63 ~= 9.2 * 10^18. We only support i64 in Convex.
    pub num_deleted_documents: u64,
    /// The total size of all files in the segment when the segment was first
    /// built. We assume that deletions do not substantially modify the Convex
    /// segment's size (the tantivy index size is constant, but some deleted
    /// term metadata will expand), so this remains reasonably accurate
    /// throughout the life of the segment.
    ///
    /// This is the size of the segment on disk and may not match the size in s3
    /// due to compression at upload time.
    // 2^63 ~= 9.2 * 10^18. We only support i64 in Convex.
    pub size_bytes_total: u64,
    // A random UUID that can be used to identify a segment to determine if the
    // segment has changed during non-transactional index changes (compaction).
    pub id: String,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum TextSnapshotVersion {
    /// V0 is the original version for search snapshots.
    /// In particular, it interprets missing fields as null.
    V0,
    /// V1 interprets missing fields as undefined.
    V1MissingAsUndefined,
    /// V2 uses string IDs
    V2UseStringIds,
}

impl TextSnapshotVersion {
    pub fn current() -> Self {
        Self::V2UseStringIds
    }

    pub fn to_code(&self) -> i64 {
        match self {
            Self::V0 => 0,
            Self::V1MissingAsUndefined => 1,
            Self::V2UseStringIds => 2,
        }
    }

    pub fn from_code(code: i64) -> anyhow::Result<Self> {
        match code {
            0 => Ok(Self::V0),
            1 => Ok(Self::V1MissingAsUndefined),
            2 => Ok(Self::V2UseStringIds),
            _ => anyhow::bail!("unrecognized search snapshot version {code:?}"),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedTextIndexSnapshot {
    data: WithUnknown<SerializedTextIndexSnapshotData>,
    ts: i64,
    version: i64,
}

impl TryFrom<TextIndexSnapshot> for SerializedTextIndexSnapshot {
    type Error = anyhow::Error;

    fn try_from(snapshot: TextIndexSnapshot) -> Result<Self, Self::Error> {
        let data = snapshot.data.try_into()?;
        Ok(Self {
            data,
            ts: snapshot.ts.into(),
            version: snapshot.version.to_code(),
        })
    }
}

impl TryFrom<SerializedTextIndexSnapshot> for TextIndexSnapshot {
    type Error = anyhow::Error;

    fn try_from(serialized: SerializedTextIndexSnapshot) -> Result<Self, Self::Error> {
        let data = TextIndexSnapshotData::try_from(serialized.data)?;
        Ok(Self {
            data,
            ts: serialized.ts.try_into()?,
            version: TextSnapshotVersion::from_code(serialized.version)?,
        })
    }
}
