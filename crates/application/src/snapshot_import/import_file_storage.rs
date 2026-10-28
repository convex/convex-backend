use std::{
    collections::BTreeMap,
    str::FromStr,
};

use anyhow::Context;
use common::{
    components::ComponentPath,
    document::{
        CreationTime,
        CREATION_TIME_FIELD,
        ID_FIELD,
    },
    runtime::Runtime,
    types::StorageUuid,
};
use database::{
    Database,
    ImportFacingModel,
};
use errors::ErrorMetadata;
use exports::FileStorageZipMetadata;
use file_storage::FileStorage;
use futures::{
    Stream,
    TryStreamExt,
};
use headers::{
    ContentLength,
    ContentType,
};
use keybroker::Identity;
use model::{
    file_storage::{
        FILE_STORAGE_TABLE,
        FILE_STORAGE_VIRTUAL_TABLE,
    },
    snapshot_imports::types::ImportRequestor,
};
use serde_json::Value as JsonValue;
use thousands::Separable;
use usage_tracking::{
    FunctionUsageTracker,
    StorageCallTracker,
    StorageUsageTracker,
};
use value::{
    sha256::Sha256Digest,
    val,
    ConvexObject,
    DeveloperDocumentId,
    TableMapping,
    TabletIdAndTableNumber,
};

use crate::snapshot_import::{
    import_error::ImportError,
    parse::ImportStorageFileStream,
    progress::{
        add_checkpoint_message,
        best_effort_update_progress_message,
    },
};

pub async fn import_storage_table<RT: Runtime>(
    database: &Database<RT>,
    file_storage: &FileStorage<RT>,
    identity: &Identity,
    table_id: TabletIdAndTableNumber,
    component_path: &ComponentPath,
    mut documents: impl Stream<Item = anyhow::Result<JsonValue>> + Unpin,
    storage_files: Vec<(DeveloperDocumentId, ImportStorageFileStream)>,
    usage: &FunctionUsageTracker,
    import_id: Option<DeveloperDocumentId>,
    num_to_skip: u64,
    requestor: ImportRequestor,
    table_mapping_for_schema: &TableMapping,
) -> anyhow::Result<()> {
    let snapshot = database.latest_snapshot()?;
    let virtual_table_number = snapshot.table_mapping().tablet_number(table_id.tablet_id)?;
    let mut lineno = 0;
    let mut storage_metadata = BTreeMap::new();
    while let Some(exported_value) = documents.try_next().await? {
        lineno += 1;
        let metadata: FileStorageZipMetadata = serde_json::from_value(exported_value)
            .map_err(|e| ImportError::InvalidConvexValue(lineno, e.into()))?;
        let id = DeveloperDocumentId::decode(&metadata.id)
            .map_err(|e| ImportError::InvalidConvexValue(lineno, e.into()))?;
        anyhow::ensure!(
            id.table() == virtual_table_number,
            ErrorMetadata::bad_request(
                "InvalidId",
                format!(
                    "_storage entry has invalid ID {id} ({:?} != {:?})",
                    id.table(),
                    virtual_table_number
                )
            )
        );
        let content_length = metadata.size.map(|size| ContentLength(size as u64));
        let content_type = metadata
            .content_type
            .map(|content_type| anyhow::Ok(ContentType::from_str(&content_type)?))
            .transpose()
            .map_err(|e| ImportError::InvalidConvexValue(lineno, e))?;
        let sha256 = metadata
            .sha256
            .map(|sha256| Sha256Digest::from_base64(&sha256))
            .transpose()
            .map_err(|e| ImportError::InvalidConvexValue(lineno, e))?;
        let storage_id = metadata
            .internal_id
            .map(|storage_id| {
                StorageUuid::from_str(&storage_id).context("Couldn't parse storage_id")
            })
            .transpose()
            .map_err(|e| ImportError::InvalidConvexValue(lineno, e))?;
        let creation_time = metadata
            .creation_time
            .map(CreationTime::try_from)
            .transpose()
            .map_err(|e| ImportError::InvalidConvexValue(lineno, e))?;

        storage_metadata.insert(
            id,
            (
                content_length,
                content_type,
                sha256,
                storage_id,
                creation_time,
            ),
        );
    }
    let total_num_files = storage_metadata.len();
    let mut num_files = 0;
    for (id, file_chunks) in storage_files {
        // The or_default means a storage file with a valid id will be imported
        // even if it has been explicitly removed from _storage/documents.jsonl,
        // to be robust to manual modifications.
        let (content_length, content_type, expected_sha256, storage_id, creation_time) =
            storage_metadata.remove(&id).unwrap_or_default();
        let mut entry = file_storage
            .transactional_file_storage
            .upload_file(content_length, content_type, file_chunks(), expected_sha256)
            .await?;
        if let Some(storage_id) = storage_id {
            entry.storage_id = storage_id;
        }
        if num_files < num_to_skip {
            num_files += 1;
            continue;
        }
        let file_size = entry.size as u64;
        database
            .execute_with_overloaded_and_ratelimited_retries(
                identity.clone(),
                FunctionUsageTracker::new(),
                "snapshot_import_storage_table",
                |tx| {
                    async {
                        let mut entry_object_map =
                            BTreeMap::from(ConvexObject::try_from(entry.clone())?);
                        entry_object_map.insert(ID_FIELD.clone().into(), val!(id));
                        if let Some(creation_time) = creation_time {
                            entry_object_map.insert(
                                CREATION_TIME_FIELD.clone().into(),
                                val!(f64::from(creation_time)),
                            );
                        }
                        let entry_object = ConvexObject::try_from(entry_object_map)?;
                        ImportFacingModel::new(tx)
                            .insert(
                                table_id,
                                &FILE_STORAGE_TABLE,
                                entry_object,
                                table_mapping_for_schema,
                            )
                            .await?;
                        Ok(())
                    }
                    .into()
                },
            )
            .await?;
        let content_type = entry
            .content_type
            .as_ref()
            .map(|ct| ct.parse())
            .transpose()?;
        usage
            .track_storage_call(component_path.clone(), requestor.usage_tag(), content_type)
            .await;
        usage
            .track_storage_ingress(
                component_path.clone(),
                requestor.usage_tag().to_string(),
                file_size,
            )
            .await;
        num_files += 1;
        if let Some(import_id) = import_id {
            best_effort_update_progress_message(
                database,
                identity,
                import_id,
                format!(
                    "Importing \"_storage\" ({}/{} files)",
                    num_files.separate_with_commas(),
                    total_num_files.separate_with_commas()
                ),
                component_path,
                &FILE_STORAGE_VIRTUAL_TABLE,
                num_files as i64,
            )
            .await;
        }
    }
    if let Some(import_id) = import_id {
        add_checkpoint_message(
            database,
            identity,
            import_id,
            format!(
                "Imported \"_storage\"{} ({} files)",
                component_path.in_component_str(),
                num_files.separate_with_commas()
            ),
            component_path,
            &FILE_STORAGE_VIRTUAL_TABLE,
            num_files as i64,
        )
        .await?;
    }
    Ok(())
}
