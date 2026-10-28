use std::{
    collections::BTreeMap,
    str::FromStr,
};

use anyhow::Context;
use chrono::{
    DateTime,
    Utc,
};
use common::{
    bootstrap_model::index::{
        database_index::DatabaseIndexSpec,
        DeveloperIndexMetadata,
        IndexConfig,
    },
    document::{
        ResolvedDocument,
        CREATION_TIME_FIELD_PATH,
    },
    query::{
        IndexRange,
        IndexRangeExpression,
        Order,
        Query,
    },
    runtime::Runtime,
};
use database::{
    IndexModel,
    PatchValue,
    ResolvedQuery,
    Transaction,
    UserFacingModel,
};
use errors::ErrorMetadata;
use serde::{
    Deserialize,
    Serialize,
};
use value::{
    ConvexObject,
    ConvexValue,
    FieldName,
    TableName,
    TableNamespace,
};

use self::constants::{
    FIVETRAN_PRIMARY_KEY_INDEX_DESCRIPTOR,
    FIVETRAN_SYNCED_INDEX_DESCRIPTOR,
    FIVETRAN_SYNC_INDEX_WITHOUT_SOFT_DELETE_FIELDS,
    FIVETRAN_SYNC_INDEX_WITH_SOFT_DELETE_FIELDS,
    METADATA_CONVEX_FIELD_NAME,
    SOFT_DELETE_CONVEX_FIELD_NAME,
    SOFT_DELETE_FIELD_PATH,
    SYNCED_FIELD_PATH,
};

pub mod constants;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum BatchWriteOperation {
    Upsert,
    Update,
    HardDelete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BatchWriteRow {
    pub table: String,
    pub operation: BatchWriteOperation,
    #[serde(with = "common::value::json_object")]
    pub row: ConvexObject,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum DeleteType {
    SoftDelete,
    HardDelete,
}

pub struct FivetranImportModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
}

impl<'a, RT: Runtime> FivetranImportModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>) -> Self {
        Self { tx }
    }

    pub async fn apply_operation(&mut self, row: BatchWriteRow) -> anyhow::Result<()> {
        let table_name = TableName::from_str(&row.table)?;
        let existing_document = self
            .primary_key_query(&table_name, &row.row)
            .await?
            .expect_at_most_one(self.tx)
            .await?;

        match row.operation {
            BatchWriteOperation::Upsert => {
                if let Some(existing_document) = existing_document {
                    UserFacingModel::new(self.tx, TableNamespace::Global)
                        .replace(existing_document.developer_id(), row.row)
                        .await?;
                } else {
                    UserFacingModel::new(self.tx, TableNamespace::Global)
                        .insert(table_name.clone(), row.row)
                        .await?;
                }
            },
            BatchWriteOperation::Update => {
                let Some(existing_document) = existing_document else {
                    anyhow::bail!(ErrorMetadata::not_found(
                        "FivetranMissingUpdatedRow",
                        "Fivetran is trying to update a row that doesn’t exist in the Convex \
                         destination."
                            .to_string(),
                    ));
                };

                UserFacingModel::new(self.tx, TableNamespace::Global)
                    .patch(
                        existing_document.developer_id(),
                        fivetran_patch_value(existing_document.into_value().into_value(), row.row),
                    )
                    .await?;
            },
            BatchWriteOperation::HardDelete => {
                if let Some(existing_document) = existing_document {
                    UserFacingModel::new(self.tx, TableNamespace::Global)
                        .delete(existing_document.developer_id())
                        .await?;
                }
            },
        }

        Ok(())
    }

    pub async fn truncate_document(
        &mut self,
        doc: ResolvedDocument,
        delete_type: DeleteType,
    ) -> anyhow::Result<()> {
        match delete_type {
            DeleteType::HardDelete => {
                UserFacingModel::new(self.tx, TableNamespace::Global)
                    .delete(doc.developer_id())
                    .await?;
            },
            DeleteType::SoftDelete => {
                UserFacingModel::new(self.tx, TableNamespace::Global)
                    .replace(
                        doc.developer_id(),
                        mark_as_soft_deleted(doc.into_value().0)?,
                    )
                    .await?;
            },
        }
        Ok(())
    }

    async fn primary_key_query(
        &mut self,
        table_name: &TableName,
        object: &ConvexObject,
    ) -> anyhow::Result<ResolvedQuery<RT>> {
        let mut model = IndexModel::new(self.tx);
        let indexes = model
            .get_system_indexes(TableNamespace::root_component())
            .await?;
        let index = indexes
            .into_iter()
            .map(|index| index.into_value())
            .find(|index: &DeveloperIndexMetadata| {
                index.name.table() == table_name
                    && *index.name.descriptor() == FIVETRAN_PRIMARY_KEY_INDEX_DESCRIPTOR
                    && index.is_database_index()
            })
            .ok_or_else(|| {
                ErrorMetadata::bad_request(
                    "MissingFivetranPrimaryKeyIndex",
                    format!(
                        "The `{}` index on the `{table_name}` table is missing. Please edit your \
                         `schema.ts` file and add the following index to the `{table_name}` \
                         table: .index(\"{}\", [/* …attributes in the primary key… */]) (or \
                         .index(\"sync_index\", [\"fivetran.deleted\", /* …attributes in the \
                         primary key… */]).",
                        *FIVETRAN_PRIMARY_KEY_INDEX_DESCRIPTOR,
                        *FIVETRAN_PRIMARY_KEY_INDEX_DESCRIPTOR,
                    ),
                )
            })?;

        let IndexConfig::Database {
            spec: DatabaseIndexSpec { fields },
            ..
        } = index.config
        else {
            anyhow::bail!("Unexpected index type");
        };

        let mut range: Vec<IndexRangeExpression> = Vec::new();
        for field_path in fields.iter() {
            range.push(if *field_path == *SOFT_DELETE_FIELD_PATH {
                IndexRangeExpression::Eq(field_path.clone(), ConvexValue::Boolean(false).into())
            } else if *field_path == *CREATION_TIME_FIELD_PATH {
                continue;
            } else {
                let value = object
                    .get_path(field_path)
                    .context(ErrorMetadata::bad_request(
                        "MissingValueForFivetranPrimaryKeyIndex",
                        format!(
                            "Missing value for field {field_path:?} of primary key index {}.",
                            index.name
                        ),
                    ))?;
                IndexRangeExpression::Eq(field_path.clone(), value.clone().into())
            })
        }

        ResolvedQuery::new(
            self.tx,
            TableNamespace::Global,
            Query::index_range(IndexRange {
                index_name: index.name,
                range,
                order: Order::Asc,
            }),
        )
    }

    /// Queries all the documents in the given table that were not soft deleted.
    ///
    /// If `delete_before` is set, only returns documents that have a
    /// `fivetran.synced` field smaller than `delete_before`.
    pub async fn synced_query(
        &mut self,
        table_name: &TableName,
        delete_before: &Option<DateTime<Utc>>,
    ) -> anyhow::Result<ResolvedQuery<RT>> {
        let mut model = IndexModel::new(self.tx);
        let indexes = model
            .get_system_indexes(TableNamespace::root_component())
            .await?;

        let index = indexes
            .into_iter()
            .map(|index| index.into_value())
            .find(|index| {
                *index.name.table() == *table_name
                    && *index.name.descriptor() == FIVETRAN_SYNCED_INDEX_DESCRIPTOR
            })
            .ok_or_else(|| {
                ErrorMetadata::bad_request(
                    "MissingFivetranSyncedIndex",
                    format!(
                        "The Fivetran synchronization index on the `{table_name}` table is \
                         missing. Something went wrong with the Fivetran sync.",
                    ),
                )
            })?;

        if !index.is_database_index() {
            anyhow::bail!("Unexpected index type");
        }

        let IndexConfig::Database {
            spec: DatabaseIndexSpec { fields },
            ..
        } = index.config
        else {
            anyhow::bail!("Unexpected index type");
        };

        if fields != *FIVETRAN_SYNC_INDEX_WITH_SOFT_DELETE_FIELDS
            && fields != *FIVETRAN_SYNC_INDEX_WITHOUT_SOFT_DELETE_FIELDS
        {
            anyhow::bail!(ErrorMetadata::bad_request(
                "WrongFieldsInFivetranSyncedIndex",
                format!(
                    "The Fivetran synchronization index on the `{table_name}` has the wrong \
                     fields {fields:?}. Expected {} or {} Something went wrong with the Fivetran \
                     sync.",
                    *FIVETRAN_SYNC_INDEX_WITH_SOFT_DELETE_FIELDS,
                    *FIVETRAN_SYNC_INDEX_WITHOUT_SOFT_DELETE_FIELDS,
                ),
            ))
        }

        let mut range: Vec<IndexRangeExpression> = Vec::new();
        for field_path in fields.iter() {
            if *field_path == *SYNCED_FIELD_PATH {
                if let Some(delete_before) = delete_before {
                    range.push(IndexRangeExpression::Lt(
                        field_path.clone(),
                        ConvexValue::Float64(delete_before.timestamp_millis() as f64).into(),
                    ));
                }
            } else if *field_path == *SOFT_DELETE_FIELD_PATH {
                range.push(IndexRangeExpression::Eq(
                    field_path.clone(),
                    ConvexValue::Boolean(false).into(),
                ));
            } else if *field_path == *CREATION_TIME_FIELD_PATH {
                continue;
            } else {
                anyhow::bail!("Unexpected field in sync index");
            }
        }

        ResolvedQuery::new(
            self.tx,
            TableNamespace::Global,
            Query::index_range(IndexRange {
                index_name: index.name,
                range,
                order: Order::Asc,
            }),
        )
    }
}

/// Converts `patch` to the corresponding `PatchValue`, including the
/// preexisting Fivetran metadata fields if they exist and are not overridden.
///
/// We need this because patching a document will replace all fields with their
/// new value. But for Fivetran’s point of view, the metadata fields are all
/// separate, which means we must make sure that we’re not overriding the entire
/// `fivetran` field if it contains attributes that are not overridden.
fn fivetran_patch_value(existing_document: ConvexObject, patch: ConvexObject) -> PatchValue {
    let metadata_field_name = FieldName::from(METADATA_CONVEX_FIELD_NAME);

    let mut existing_document: BTreeMap<FieldName, ConvexValue> = existing_document.into();
    let Some(ConvexValue::Object(existing_metadata)) =
        existing_document.remove(&metadata_field_name)
    else {
        return PatchValue::from(patch);
    };

    let mut patch_contents: BTreeMap<FieldName, ConvexValue> = patch.clone().into();
    let Some(ConvexValue::Object(new_metadata)) = patch_contents.remove(&metadata_field_name)
    else {
        return PatchValue::from(patch);
    };

    // Merge `new_metadata` into `existing_metadata` field-by-field
    let mut merged_metadata: BTreeMap<_, _> = existing_metadata.into();
    for (field, value) in new_metadata.into_iter() {
        merged_metadata.insert(field, value);
    }

    PatchValue::from(
        patch
            .insert(
                METADATA_CONVEX_FIELD_NAME.into(),
                merged_metadata.try_into().expect(
                    "The number of metadata fields should always be under the field count limit",
                ),
            )
            .expect(
                "Adding a field to an object that previously had the same field keeps the object \
                 under the field count limit",
            ),
    )
}

fn mark_as_soft_deleted(object: ConvexObject) -> anyhow::Result<ConvexObject> {
    let metadata_key = FieldName::from(METADATA_CONVEX_FIELD_NAME);

    let mut new_value: BTreeMap<FieldName, ConvexValue> = object.into();
    let metadata_object = match new_value.remove(&metadata_key) {
        Some(ConvexValue::Object(object)) => object,
        _ => ConvexObject::empty(),
    };

    new_value.insert(
        metadata_key,
        ConvexValue::Object(
            metadata_object.insert(SOFT_DELETE_CONVEX_FIELD_NAME.into(), true.into())?,
        ),
    );
    new_value.try_into()
}
