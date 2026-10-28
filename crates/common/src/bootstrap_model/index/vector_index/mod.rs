mod backfill_state;
mod dimensions;
mod index_config;
mod index_snapshot;
mod index_state;
mod segment;

pub use self::{
    backfill_state::VectorIndexBackfillState,
    dimensions::{
        VectorDimensions,
        MAX_VECTOR_DIMENSIONS,
        MIN_VECTOR_DIMENSIONS,
    },
    index_config::{
        SerializedVectorIndexSpec,
        VectorIndexSpec,
    },
    index_snapshot::{
        VectorIndexSnapshot,
        VectorIndexSnapshotData,
    },
    index_state::{
        SerializedVectorIndexState,
        VectorIndexState,
    },
    segment::FragmentedVectorSegment,
};
