#![feature(iterator_try_collect)]
#![feature(try_blocks_heterogeneous)]
#![feature(impl_trait_in_assoc_type)]

use std::ops::Deref;

use common::{
    bootstrap_model::index::vector_index::MAX_VECTOR_DIMENSIONS,
    types::IndexName,
};
use errors::ErrorMetadata;
use qdrant_segment::data_types::vectors::{
    QueryVector,
    Vector,
};
use value::FieldPath;

pub mod id_tracker;
mod memory_index;
pub mod metrics;
mod qdrant_index;
pub mod qdrant_segments;
mod query;
pub mod result_merger;
mod searcher;
mod vector_index_manager;

pub use self::{
    memory_index::MemoryVectorIndex,
    metrics::{
        vector_index_type_label,
        VectorIndexType,
        VECTOR_INDEX_TYPE_LABEL,
    },
    qdrant_index::{
        PreviousVectorSegmentsHack,
        QdrantDocument,
        QdrantExternalId,
        QdrantSchema,
        QdrantVectorIndexType,
    },
    query::{
        CompiledVectorSearch,
        InternalVectorSearch,
        PublicVectorSearchQueryResult,
        VectorSearch,
        VectorSearchExpression,
        VectorSearchJson,
        VectorSearchQueryResult,
        VectorSearchRequest,
    },
    searcher::VectorSearcher,
    vector_index_manager::{
        IndexState,
        VectorIndexManager,
    },
};

pub const MAX_VECTOR_RESULTS: usize = 256;
pub const DEFAULT_VECTOR_LIMIT: u32 = 10;
pub const MAX_FILTER_LENGTH: usize = 64;

#[derive(Clone, Debug)]
pub struct IndexedVector(Vec<f32>);

impl From<IndexedVector> for QueryVector {
    fn from(value: IndexedVector) -> Self {
        QueryVector::Nearest(Vector::Dense(value.0))
    }
}

impl Deref for IndexedVector {
    type Target = [f32];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<Vec<f32>> for IndexedVector {
    type Error = anyhow::Error;

    fn try_from(value: Vec<f32>) -> Result<Self, Self::Error> {
        anyhow::ensure!(
            value.len() <= MAX_VECTOR_DIMENSIONS as usize,
            vector_dimensions_mismatch_error(value.len() as u32, MAX_VECTOR_DIMENSIONS)
        );
        Ok(IndexedVector(value))
    }
}

impl From<IndexedVector> for Vec<f32> {
    fn from(value: IndexedVector) -> Self {
        value.0
    }
}

fn vector_dimensions_mismatch_error(dimensions: u32, expected_dimensions: u32) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "VectorDimensionsMismatch",
        format!("Expected a vector with dimensions {expected_dimensions}, received {dimensions}."),
    )
}

fn incorrect_vector_filter_field_error(
    index_name: &IndexName,
    field_path: &FieldPath,
) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "IncorrectVectorFilterField",
        format!(
            "Vector query against {index_name} contains a filter on {field_path:?} but that field \
             isn't indexed for filtering in `filterFields`."
        ),
    )
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VectorIndexWriteSize(pub u64);
