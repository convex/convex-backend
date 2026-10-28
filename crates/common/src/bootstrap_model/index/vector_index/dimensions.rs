use std::ops::Deref;

use errors::ErrorMetadata;

pub const MIN_VECTOR_DIMENSIONS: u32 = 2;
pub const MAX_VECTOR_DIMENSIONS: u32 = 4096;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct VectorDimensions(
    u32,
);

impl From<VectorDimensions> for usize {
    fn from(value: VectorDimensions) -> Self {
        value.0 as usize
    }
}

impl From<VectorDimensions> for u32 {
    fn from(value: VectorDimensions) -> Self {
        value.0
    }
}

impl Deref for VectorDimensions {
    type Target = u32;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<u32> for VectorDimensions {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        anyhow::ensure!(
            (MIN_VECTOR_DIMENSIONS..=MAX_VECTOR_DIMENSIONS).contains(&value),
            ErrorMetadata::bad_request(
                "InvalidVectorDimensionError",
                format!(
                    "Dimensions {value} must be between {MIN_VECTOR_DIMENSIONS} and \
                     {MAX_VECTOR_DIMENSIONS}."
                )
            )
        );
        Ok(Self(value))
    }
}
