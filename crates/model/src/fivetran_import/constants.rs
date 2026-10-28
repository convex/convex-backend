use std::sync::LazyLock;

use common::{
    bootstrap_model::index::database_index::IndexedFields,
    document::CREATION_TIME_FIELD_PATH,
    types::IndexDescriptor,
    value::{
        FieldPath,
        IdentifierFieldName,
    },
};

/// The name of the field used in Convex tables to store the Fivetran metadata.
pub const METADATA_CONVEX_FIELD_NAME: IdentifierFieldName =
    IdentifierFieldName::const_new("fivetran");

pub const SYNCED_CONVEX_FIELD_NAME: IdentifierFieldName = IdentifierFieldName::const_new("synced");
pub const SOFT_DELETE_CONVEX_FIELD_NAME: IdentifierFieldName =
    IdentifierFieldName::const_new("deleted");

pub static SOFT_DELETE_FIELD_PATH: LazyLock<FieldPath> = LazyLock::new(|| {
    FieldPath::new(vec![
        METADATA_CONVEX_FIELD_NAME.clone(),
        SOFT_DELETE_CONVEX_FIELD_NAME.clone(),
    ])
    .expect("Invalid field path")
});

pub static SYNCED_FIELD_PATH: LazyLock<FieldPath> = LazyLock::new(|| {
    FieldPath::new(vec![
        METADATA_CONVEX_FIELD_NAME.clone(),
        SYNCED_CONVEX_FIELD_NAME.clone(),
    ])
    .expect("Invalid field path")
});

pub static FIVETRAN_SYNC_INDEX_WITHOUT_SOFT_DELETE_FIELDS: LazyLock<IndexedFields> =
    LazyLock::new(|| {
        IndexedFields::try_from(vec![
            SYNCED_FIELD_PATH.clone(),
            CREATION_TIME_FIELD_PATH.clone(),
        ])
        .expect("Invalid IndexedFields")
    });

pub static FIVETRAN_SYNC_INDEX_WITH_SOFT_DELETE_FIELDS: LazyLock<IndexedFields> =
    LazyLock::new(|| {
        IndexedFields::try_from(vec![
            SOFT_DELETE_FIELD_PATH.clone(),
            SYNCED_FIELD_PATH.clone(),
            CREATION_TIME_FIELD_PATH.clone(),
        ])
        .expect("Invalid IndexedFields")
    });

pub static FIVETRAN_SYNCED_INDEX_DESCRIPTOR: IndexDescriptor =
    IndexDescriptor::const_new("_fivetran_by_synced");

pub static FIVETRAN_PRIMARY_KEY_INDEX_DESCRIPTOR: IndexDescriptor =
    IndexDescriptor::const_new("_fivetran_by_primary_key");
