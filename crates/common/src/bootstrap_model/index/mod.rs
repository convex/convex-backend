pub mod database_index;
mod index_config;
mod index_metadata;
pub mod index_validation_error;
pub mod search_index;
pub mod text_index;
pub mod vector_index;

use std::sync::LazyLock;

use value::IdentifierFieldName;

pub use self::{
    index_config::IndexConfig,
    index_metadata::{
        index_metadata_serialize_tablet_id,
        DeveloperIndexMetadata,
        IndexMetadata,
        TabletIndexMetadata,
    },
};
use crate::{
    paths::FieldPath,
    types::{
        IndexDescriptor,
        TableName,
    },
};

/// Table name for Index data.
pub const INDEX_TABLE: TableName = TableName::const_new("_index");
/// Field for an indexed table's name in `IndexMetadata`.
pub const TABLE_ID_FIELD_NAME: IdentifierFieldName = IdentifierFieldName::const_new("table_id");
pub static TABLE_ID_FIELD_PATH: LazyLock<FieldPath> =
    LazyLock::new(|| FieldPath::new(vec![TABLE_ID_FIELD_NAME.clone()]).unwrap());

/// See `record_reads_for_writes` in `database::writes`
pub static INDEX_BY_TABLE_ID_VIRTUAL_INDEX_DESCRIPTOR: IndexDescriptor =
    IndexDescriptor::const_new("by_table_id");

pub const MAX_INDEX_FIELDS_SIZE: usize = 16;
pub const MAX_TEXT_INDEX_FILTER_FIELDS_SIZE: usize = 16;
pub const MAX_VECTOR_INDEX_FILTER_FIELDS_SIZE: usize = 16;
