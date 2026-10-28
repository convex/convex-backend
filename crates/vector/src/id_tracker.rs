use std::{
    io::Write,
    iter,
    ops::RangeFrom,
};

use bitvec::slice::BitSlice;
use common::{
    deleted_bitset::DeletedBitset,
    id_tracker::{
        MemoryIdTracker,
        StaticIdTracker,
    },
};
use qdrant_common::types::PointOffsetType;
use qdrant_segment::{
    common::{
        operation_error::{
            OperationError,
            OperationResult,
        },
        Flusher,
    },
    id_tracker::IdTracker,
    types::{
        ExtendedPointId,
        PointIdType,
        SeqNumberType,
    },
};
use uuid::Uuid;

/// Qdrant has a notion of "operation number" that it uses for tracking
/// mutations. Since we only use it as a static index, require that all points'
/// operation numbers are always 1.
pub const OP_NUM: SeqNumberType = 1;

/// Restricted implementation of `IdTracker` that assumes...
///
/// 1. All operation numbers are OP_NUM.
/// 2. The application only uses UUID point IDs.
/// 3. The set of offsets used is dense (i.e. `0..self.len()`).
pub struct VectorMemoryIdTracker {
    memory_id_tracker: MemoryIdTracker,

    // We don't actually support deletes here but keep this empty bitset around
    // to use in `deleted_point_bitslice`.
    deleted: DeletedBitset,
}

impl VectorMemoryIdTracker {
    pub fn new() -> Self {
        Self {
            memory_id_tracker: MemoryIdTracker::default(),
            deleted: DeletedBitset::new(0),
        }
    }

    fn _internal_to_external_id(&self, internal_id: PointOffsetType) -> Option<PointIdType> {
        self.memory_id_tracker
            .convex_id(internal_id)
            .map(|bytes| PointIdType::Uuid(Uuid::from_slice(&bytes).unwrap()))
    }
}

impl IdTracker for VectorMemoryIdTracker {
    fn internal_version(&self, internal_id: PointOffsetType) -> Option<SeqNumberType> {
        self.external_id(internal_id).map(|_| OP_NUM)
    }

    fn set_internal_version(
        &mut self,
        _internal_id: PointOffsetType,
        version: SeqNumberType,
    ) -> OperationResult<()> {
        if version != OP_NUM {
            return Err(OperationError::service_error(format!(
                "Invalid version: {version}"
            )));
        }
        Ok(())
    }

    fn internal_id(&self, external_id: PointIdType) -> Option<PointOffsetType> {
        let PointIdType::Uuid(uuid) = external_id else {
            panic!("Invalid external ID: {external_id}");
        };
        self.memory_id_tracker
            .index_id(*uuid.as_bytes())
            .map(|ix| ix as PointOffsetType)
    }

    fn external_id(&self, internal_id: PointOffsetType) -> Option<PointIdType> {
        if self.is_deleted_point(internal_id) {
            return None;
        }
        self._internal_to_external_id(internal_id)
    }

    fn set_link(
        &mut self,
        external_id: PointIdType,
        internal_id: PointOffsetType,
    ) -> OperationResult<()> {
        let PointIdType::Uuid(uuid) = external_id else {
            panic!("Invalid external ID: {external_id}");
        };
        self.memory_id_tracker.insert(internal_id, *uuid.as_bytes());
        self.deleted.resize(internal_id as usize + 1);
        Ok(())
    }

    fn drop(&mut self, external_id: PointIdType) -> OperationResult<()> {
        let Some(internal_id) = self.internal_id(external_id) else {
            panic!("Unrecognized external id: {external_id}");
        };
        self.deleted
            .delete(internal_id)
            .map_err(|e| OperationError::InconsistentStorage {
                description: e.to_string(),
            })?;
        Ok(())
    }

    fn iter_external(&self) -> Box<dyn Iterator<Item = PointIdType> + '_> {
        Box::new(
            self.memory_id_tracker
                .by_index_id
                .iter()
                .filter(|(key, _)| !self.is_deleted_point(**key))
                .map(|(_, uuid)| PointIdType::Uuid(Uuid::from_slice(uuid).unwrap())),
        )
    }

    fn iter_internal(&self) -> Box<dyn Iterator<Item = PointOffsetType> + '_> {
        Box::new(self.memory_id_tracker.by_index_id.keys().copied())
    }

    fn iter_from(
        &self,
        external_id: Option<PointIdType>,
    ) -> Box<dyn Iterator<Item = (PointIdType, PointOffsetType)> + '_> {
        // All `NumId`s sort before all `Uuid`s in qdrant's order, so effectively ignore
        // a `NumId` lower bound.
        let Some(minimum) = self.memory_id_tracker.by_convex_id.keys().next().copied() else {
            return Box::new(iter::empty());
        };
        let lower_bound = external_id
            .and_then(|id| match id {
                ExtendedPointId::NumId(..) => None,
                ExtendedPointId::Uuid(uuid) => Some(uuid.into_bytes()),
            })
            .unwrap_or(minimum);
        let iter = self
            .memory_id_tracker
            .by_convex_id
            .range::<[u8; 16], RangeFrom<&[u8; 16]>>(&lower_bound..)
            .map(|(k, v)| {
                (
                    PointIdType::Uuid(Uuid::from_slice(k).unwrap()),
                    *v as PointOffsetType,
                )
            });
        Box::new(iter)
    }

    fn iter_ids(&self) -> Box<dyn Iterator<Item = PointOffsetType> + '_> {
        Box::new(
            self.memory_id_tracker
                .by_index_id
                .keys()
                .filter(|internal_id| !self.deleted.is_deleted(**internal_id))
                .copied(),
        )
    }

    fn mapping_flusher(&self) -> Flusher {
        // Do nothing when Qdrant asks us to flush the mapping to disk. We fully
        // manage ID mappings ourselves in separate index files that Qdrant isn't
        // aware of, so there's nothing to do here.
        Box::new(|| OperationResult::Ok(()))
    }

    fn versions_flusher(&self) -> Flusher {
        // We don't allow mutating points' versions (see `set_internal_version`) to
        // anything other than `OP_NUM`, so there's nothing to flush here either.
        Box::new(|| OperationResult::Ok(()))
    }

    fn total_point_count(&self) -> usize {
        self.memory_id_tracker.by_index_id.len()
    }

    fn deleted_point_count(&self) -> usize {
        self.deleted.num_deleted()
    }

    fn deleted_point_bitslice(&self) -> &BitSlice {
        self.deleted.deleted_id_bitslice()
    }

    fn is_deleted_point(&self, internal_id: PointOffsetType) -> bool {
        self.deleted.is_deleted(internal_id)
    }
}

impl VectorMemoryIdTracker {
    pub fn write_uuids(&mut self, out: impl Write) -> anyhow::Result<()> {
        self.memory_id_tracker.write_id_tracker(out)
    }

    pub fn write_deleted_bitset(&mut self, out: impl Write) -> anyhow::Result<()> {
        self.deleted.write(out)
    }
}

pub struct VectorStaticIdTracker {
    pub id_tracker: StaticIdTracker,
    pub deleted_bitset: DeletedBitset,
}

impl VectorStaticIdTracker {
    fn get_uuid(&self, offset: usize) -> Option<Uuid> {
        self.id_tracker
            .get_convex_id(offset)
            .map(|v| Uuid::from_slice(&v).unwrap())
    }
}

impl IdTracker for VectorStaticIdTracker {
    fn internal_version(&self, internal_id: PointOffsetType) -> Option<SeqNumberType> {
        self.external_id(internal_id).map(|_| OP_NUM)
    }

    fn set_internal_version(
        &mut self,
        _internal_id: PointOffsetType,
        _version: SeqNumberType,
    ) -> OperationResult<()> {
        panic!("set_internal_version() unsupported");
    }

    fn internal_id(&self, external_id: PointIdType) -> Option<PointOffsetType> {
        let PointIdType::Uuid(uuid) = external_id else {
            panic!("Invalid external ID: {external_id}");
        };
        self.id_tracker.lookup(*uuid.as_bytes())
    }

    fn external_id(&self, internal_id: PointOffsetType) -> Option<PointIdType> {
        self.get_uuid(internal_id as usize)
            .map(ExtendedPointId::Uuid)
    }

    fn set_link(
        &mut self,
        _external_id: PointIdType,
        _internal_id: PointOffsetType,
    ) -> OperationResult<()> {
        panic!("set_link() unsupported")
    }

    /// Marks a point deleted in the deleted bitset. The id mapping itself is
    /// immutable (loaded from disk), so this is a soft delete: the point is
    /// excluded from `iter_ids` and so from queries and from any subsequent
    /// segment merge.
    fn drop(&mut self, external_id: PointIdType) -> OperationResult<()> {
        let Some(internal_id) = self.internal_id(external_id) else {
            panic!("Unrecognized external id: {external_id}");
        };
        if self.deleted_bitset.is_deleted(internal_id) {
            return Ok(());
        }
        self.deleted_bitset.delete(internal_id).map_err(|e| {
            OperationError::InconsistentStorage {
                description: e.to_string(),
            }
        })?;
        Ok(())
    }

    fn iter_external(&self) -> Box<dyn Iterator<Item = PointIdType> + '_> {
        panic!("iter_external() unsupported")
    }

    fn iter_internal(&self) -> Box<dyn Iterator<Item = PointOffsetType> + '_> {
        Box::new(0..self.id_tracker.count() as u32)
    }

    fn iter_from(
        &self,
        _external_id: Option<PointIdType>,
    ) -> Box<dyn Iterator<Item = (PointIdType, PointOffsetType)> + '_> {
        panic!("iter_from() unsupported")
    }

    fn iter_ids(&self) -> Box<dyn Iterator<Item = PointOffsetType> + '_> {
        Box::new(
            (0..self.id_tracker.count())
                .map(|value| value as u32)
                .filter(|internal_id| !self.deleted_bitset.is_deleted(*internal_id)),
        )
    }

    fn mapping_flusher(&self) -> Flusher {
        panic!("mapping_flusher() unsupported")
    }

    fn versions_flusher(&self) -> Flusher {
        panic!("versions_flusher() unsupported")
    }

    fn total_point_count(&self) -> usize {
        self.id_tracker.count()
    }

    fn deleted_point_count(&self) -> usize {
        self.deleted_bitset.num_deleted()
    }

    fn deleted_point_bitslice(&self) -> &BitSlice {
        self.deleted_bitset.deleted_id_bitslice()
    }

    fn is_deleted_point(&self, internal_id: PointOffsetType) -> bool {
        self.deleted_bitset.is_deleted(internal_id)
    }
}
