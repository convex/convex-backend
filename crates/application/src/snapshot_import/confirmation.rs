use std::collections::{
    BTreeMap,
    BTreeSet,
};

use anyhow::Context;
use common::{
    bootstrap_model::tables::TABLES_TABLE,
    components::ComponentPath,
    document::{
        ParsedDocument,
        ID_FIELD,
    },
    runtime::Runtime,
    types::TableName,
};
use database::TransactionReadSet;
use futures::TryStreamExt;
use itertools::Itertools;
use model::{
    file_storage::{
        FILE_STORAGE_TABLE,
        FILE_STORAGE_VIRTUAL_TABLE,
    },
    snapshot_imports::types::{
        ImportMode,
        ImportTableCheckpoint,
        SnapshotImport,
    },
};

use crate::snapshot_import::{
    import_error::ImportError,
    table_change::{
        render_table_changes,
        TableChange,
    },
    SnapshotImportExecutor,
};

/// Parse the uploaded import file, compare it to existing data, and return
/// a message to display about the import before it begins.
pub async fn info_message_for_import<RT: Runtime>(
    executor: &SnapshotImportExecutor<RT>,
    snapshot_import: ParsedDocument<SnapshotImport>,
) -> anyhow::Result<(String, bool, Vec<ImportTableCheckpoint>)> {
    executor.fail_if_too_old(&snapshot_import)?;
    let mut message_lines = Vec::new();
    let (content_confirmation_messages, require_manual_confirmation, new_checkpoints) =
        messages_to_confirm_replace(executor, snapshot_import).await?;
    message_lines.extend(content_confirmation_messages);
    // Consider adding confirmation messages about bandwidth usage.
    if !message_lines.is_empty() {
        message_lines.insert(0, "Import change summary:".to_string())
    }
    message_lines.push(
        "Once the import has started, it will run in the background.\nInterrupting `npx convex \
         import` will not cancel it."
            .to_string(),
    );
    Ok((
        message_lines.join("\n"),
        require_manual_confirmation,
        new_checkpoints,
    ))
}

async fn messages_to_confirm_replace<RT: Runtime>(
    executor: &SnapshotImportExecutor<RT>,
    snapshot_import: ParsedDocument<SnapshotImport>,
) -> anyhow::Result<(Vec<String>, bool, Vec<ImportTableCheckpoint>)> {
    let mode = snapshot_import.mode;
    let (_, import) = executor
        .parse_import(snapshot_import.developer_id())
        .await?;
    // Find all tables being written to.
    let mut count_by_table: BTreeMap<(ComponentPath, TableName), u64> = BTreeMap::new();
    let mut tables_missing_id_field: BTreeSet<(ComponentPath, TableName)> = BTreeSet::new();
    for (component_path, table_name, mut objects) in import.documents {
        let mut lineno = 0u64;
        let component_table = (component_path, table_name);
        while let Some(exported_value) = objects.try_next().await? {
            lineno += 1;
            if component_table.1 == TABLES_TABLE {
                let exported_object = exported_value
                    .as_object()
                    .with_context(|| ImportError::NotAnObject(lineno))?;
                let entry_table_name = exported_object
                    .get("name")
                    .and_then(|name| name.as_str())
                    .with_context(|| {
                    ImportError::InvalidConvexValue(lineno, anyhow::anyhow!("table requires name"))
                })?;
                let entry_table_name = entry_table_name
                    .parse()
                    .map_err(|e| ImportError::InvalidName(entry_table_name.to_string(), e))?;
                count_by_table
                    .entry((component_table.0.clone(), entry_table_name))
                    .or_default();
            }
            if !tables_missing_id_field.contains(&component_table)
                && exported_value.get(&*ID_FIELD).is_none()
            {
                tables_missing_id_field.insert(component_table.clone());
            }
        }
        *count_by_table.entry(component_table.clone()).or_default() += lineno;
    }

    let db_snapshot = executor.database.latest_snapshot()?;

    // Add to count_by_table all tables that are being replaced that don't appear in
    // the import.
    if mode == ImportMode::ReplaceAll {
        let component_paths = db_snapshot.component_ids_to_paths();
        let table_mapping = db_snapshot.table_mapping();
        for (tablet_id, namespace, _, table_name) in table_mapping.iter() {
            let Some(component_path) = component_paths.get(&namespace.into()) else {
                continue;
            };
            if !table_mapping.is_active(tablet_id) {
                continue;
            }
            count_by_table
                .entry((component_path.clone(), table_name.clone()))
                .or_default();
        }
    }

    let mut table_changes = BTreeMap::new();
    for (component_and_table, count_importing) in count_by_table.iter() {
        let (component_path, table_name) = component_and_table;
        let existing_num_values = db_snapshot
            .component_registry
            .component_path_to_ids(component_path, &mut TransactionReadSet::new())?
            .map(|(_, component_id)| {
                let table_name = if table_name == &FILE_STORAGE_VIRTUAL_TABLE {
                    &FILE_STORAGE_TABLE
                } else {
                    table_name
                };
                let table_count = db_snapshot.must_table_count(component_id.into(), table_name)?;
                anyhow::Ok(table_count.num_values())
            })
            .transpose()?
            .unwrap_or(0);
        if !table_name.is_system() {
            let to_delete = match mode {
                ImportMode::Replace | ImportMode::ReplaceAll => {
                    // Overwriting nonempty user table.
                    existing_num_values
                },
                ImportMode::Append => 0,
                ImportMode::RequireEmpty if existing_num_values > 0 => {
                    anyhow::bail!(ImportError::TableExists(table_name.clone()))
                },
                ImportMode::RequireEmpty => 0,
            };
            table_changes.insert(
                component_and_table.clone(),
                TableChange {
                    added: *count_importing,
                    deleted: to_delete,
                    existing: existing_num_values,
                    unit: "",
                    is_missing_id_field: tables_missing_id_field.contains(component_and_table),
                },
            );
        }
        if table_name == &FILE_STORAGE_VIRTUAL_TABLE {
            let to_delete = match mode {
                ImportMode::Replace | ImportMode::ReplaceAll => {
                    // Overwriting nonempty file storage.
                    existing_num_values
                },
                ImportMode::Append => 0,
                ImportMode::RequireEmpty if existing_num_values > 0 => {
                    anyhow::bail!(ImportError::TableExists(table_name.clone()))
                },
                ImportMode::RequireEmpty => 0,
            };
            table_changes.insert(
                component_and_table.clone(),
                TableChange {
                    added: *count_importing,
                    deleted: to_delete,
                    existing: existing_num_values,
                    unit: " files",
                    is_missing_id_field: tables_missing_id_field.contains(component_and_table),
                },
            );
        }
    }
    let mut require_manual_confirmation = false;
    let mut new_checkpoints = Vec::new();

    for (
        (component_path, table_name),
        TableChange {
            added,
            deleted,
            existing,
            unit: _,
            is_missing_id_field,
        },
    ) in table_changes.iter()
    {
        if *deleted > 0 {
            // Deleting files can be destructive, so require confirmation.
            require_manual_confirmation = true;
        }
        new_checkpoints.push(ImportTableCheckpoint {
            component_path: component_path.clone(),
            display_table_name: table_name.clone(),
            tablet_id: None,
            num_rows_written: 0,
            total_num_rows_to_write: *added as i64,
            existing_rows_to_delete: *deleted as i64,
            existing_rows_in_table: *existing as i64,
            is_missing_id_field: *is_missing_id_field,
        });
    }
    let mut message_lines = Vec::new();
    for (component_path, table_changes) in &table_changes
        .into_iter()
        .chunk_by(|((component_path, _), _)| component_path.clone())
    {
        if !component_path.is_root() {
            message_lines.push(format!("Component {}", String::from(component_path)));
        }
        message_lines.extend(render_table_changes(table_changes.collect()));
    }
    Ok((message_lines, require_manual_confirmation, new_checkpoints))
}
