#![feature(try_blocks)]
#![feature(try_blocks_heterogeneous)]

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::Instant,
};

use anyhow::Context as _;
use bytes::Bytes;
use common::{
    async_compat::TokioAsyncWriteCompatExt,
    bootstrap_model::tables::TABLES_TABLE,
    components::{
        ComponentId,
        ComponentPath,
    },
    fastrace_helpers::get_sampled_span,
    knobs::{
        EXPORT_PROGRESS_UPDATE_INTERVAL,
        EXPORT_WORKER_PAGE_SIZE,
    },
    persistence::LatestDocument,
    runtime::Runtime,
    types::{
        IndexRef,
        ObjectKey,
        TableName,
    },
};
use database::{
    DatabaseSnapshot,
    IndexModel,
    MultiTableIterator,
    SearchNotEnabled,
    TableCount,
    COMPONENTS_TABLE,
};
use errors::ErrorMetadata;
use fastrace::future::FutureExt;
use futures::{
    pin_mut,
    try_join,
    AsyncWriteExt,
    Future,
    StreamExt,
    TryStreamExt,
};
use humansize::{
    FormatSize,
    BINARY,
};
use itertools::Itertools;
use keybroker::Identity;
use model::{
    exports::types::{
        ExportFormat,
        ExportRequestor,
    },
    file_storage::FILE_STORAGE_TABLE,
    virtual_system_mapping,
};
use serde_json::json;
use storage::{
    ChannelWriter,
    Storage,
    Upload,
    UploadExt,
};
use thousands::Separable;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use usage_tracking::FunctionUsageTracker;
use value::{
    TableNamespace,
    TableNumber,
    TabletId,
};

use self::{
    export_storage::write_storage_table,
    zip_uploader::ZipSnapshotUpload,
};

mod export_storage;
pub mod interface;
mod metrics;
mod zip_uploader;

use crate::metrics::export_timer;
pub use crate::{
    export_storage::FileStorageZipMetadata,
    zip_uploader::README_MD_CONTENTS,
};

pub const MAX_FILE_STORAGE_EXPORT_SIZE_BYTES: u64 = 1 << 40;
pub const FILE_STORAGE_EXPORT_TOO_LARGE_SHORT_MSG: &str = "ExportFileStorageTooLarge";

pub fn ensure_file_storage_export_size(size: u64) -> anyhow::Result<()> {
    anyhow::ensure!(
        size <= MAX_FILE_STORAGE_EXPORT_SIZE_BYTES,
        ErrorMetadata::bad_request(
            FILE_STORAGE_EXPORT_TOO_LARGE_SHORT_MSG,
            format!(
                "File storage is too large to include in this backup ({} > maximum size {}). You \
                 can still create a tables-only backup. Restoring it replaces table data while \
                 leaving the target deployment's current file storage unchanged.",
                size.format_size(BINARY),
                MAX_FILE_STORAGE_EXPORT_SIZE_BYTES.format_size(BINARY),
            ),
        )
    );
    Ok(())
}

pub struct ExportComponents<RT: Runtime> {
    pub runtime: RT,
    pub database: DatabaseSnapshot<RT>,
    pub exports_storage: Arc<dyn Storage>,
    pub file_storage: Arc<dyn Storage>,
    pub deployment_name: String,
}

/// Uploads an export to exports_storage at the returned `ObjectKey`.
/// The export is current as of the `DatabaseSnapshot`'s timestamp.
pub async fn export_inner<F, Fut, RT: Runtime>(
    components: &ExportComponents<RT>,
    format: ExportFormat,
    requestor: ExportRequestor,
    update_progress: F,
) -> anyhow::Result<(ObjectKey, FunctionUsageTracker)>
where
    F: Fn(String) -> Fut + Send + Copy,
    Fut: Future<Output = anyhow::Result<()>> + Send,
{
    let timer = export_timer(&components.deployment_name);
    let exports_storage = &components.exports_storage;
    update_progress("Beginning backup".to_string()).await?;
    let (tables, component_ids_to_paths, by_id_indexes, system_tables, storage_table_counts) = {
        let mut tx = components.database.begin_tx(
            Identity::system(),
            Arc::new(SearchNotEnabled),
            // only used for system reads
            FunctionUsageTracker::new(),
            virtual_system_mapping().clone(),
        )?;
        let by_id_indexes = IndexModel::new(&mut tx).by_id_indexes().await?;
        drop(tx);
        let mut database = components.database.clone();
        if database.snapshot.table_counts.is_none() {
            database.load_table_summaries().await?;
        }
        let snapshot = &database.snapshot;
        let table_counts = snapshot.must_table_counts()?;
        let tables: BTreeMap<_, _> = snapshot
            .table_registry
            .iter_active_user_tables()
            .map(|(tablet_id, table_namespace, table_number, table_name)| {
                (
                    tablet_id,
                    (
                        table_namespace,
                        table_number,
                        table_name.clone(),
                        table_counts.tablet_count(&tablet_id),
                    ),
                )
            })
            .collect();
        let component_ids_to_paths = snapshot.component_ids_to_paths();
        let system_tables: BTreeMap<_, _> = snapshot
            .table_registry
            .iter_active_system_tables()
            .map(|(id, namespace, _, name)| ((namespace, name.clone()), id))
            .collect();
        let storage_table_counts: BTreeMap<TableNamespace, u64> = system_tables
            .iter()
            .filter(|((_, name), _)| *name == FILE_STORAGE_TABLE)
            .map(|((ns, _), id)| (*ns, table_counts.tablet_count(id).num_values()))
            .collect();
        (
            tables,
            component_ids_to_paths,
            by_id_indexes,
            system_tables,
            storage_table_counts,
        )
    };
    let export = match format {
        ExportFormat::Zip { include_storage } => {
            // Start upload.
            let mut upload = exports_storage.start_upload().await?;
            let (sender, receiver) = mpsc::channel::<Bytes>(1);
            let uploader =
                upload.try_write_parallel_and_hash(ReceiverStream::new(receiver).map(Ok));
            let writer = ChannelWriter::new(sender, 5 * (1 << 20));
            let usage = FunctionUsageTracker::new();

            let mut tablet_ids: Vec<_> = tables.keys().copied().collect();
            if include_storage {
                for &component_id in component_ids_to_paths.keys() {
                    tablet_ids.push(
                        *system_tables
                            .get(&(component_id.into(), FILE_STORAGE_TABLE.clone()))
                            .context("_file_storage does not exist")?,
                    );
                }
            }
            let table_iterator = components
                .database
                .table_iterator()
                .with_page_size(*EXPORT_WORKER_PAGE_SIZE)
                .multi(tablet_ids);

            let zipper = construct_zip_snapshot(
                components,
                writer,
                tables,
                table_iterator,
                component_ids_to_paths,
                by_id_indexes,
                system_tables,
                storage_table_counts,
                include_storage,
                usage.clone(),
                requestor,
                update_progress,
            );
            let (_, ()) = try_join!(uploader, zipper)?;
            let zip_object_key = upload.complete().await?;
            (zip_object_key, usage)
        },
    };
    timer.finish();
    Ok(export)
}

async fn write_tables_table<'a, 'b: 'a>(
    path_prefix: &str,
    zip_snapshot_upload: &'a mut ZipSnapshotUpload<'b>,
    namespace: TableNamespace,
    tables: &'a BTreeMap<TabletId, (TableNamespace, TableNumber, TableName, TableCount)>,
) -> anyhow::Result<()> {
    // _tables
    let mut table_upload = zip_snapshot_upload
        .start_system_table(path_prefix, TABLES_TABLE.clone())
        .await?;

    // Write documents from stream to table uploads, in table number order.
    // This includes all user tables present in the export.
    let mut user_table_numbers_and_names: Vec<_> = tables
        .iter()
        .filter(|(_, (ns, ..))| *ns == namespace)
        .map(|(_, (_, table_number, table_name, _))| (table_number, table_name))
        .collect();
    user_table_numbers_and_names.sort();
    for (table_number, table_name) in user_table_numbers_and_names {
        table_upload
            .write_json_line(json!({
                "name": table_name.clone(),
                "id": *table_number,
            }))
            .await?;
    }
    table_upload.complete().await?;
    Ok(())
}

pub async fn write_table<'a, 'b: 'a, F, Fut, RT: Runtime>(
    path_prefix: &str,
    zip_snapshot_upload: &'a mut ZipSnapshotUpload<'b>,
    table_iterator: &mut MultiTableIterator<RT>,
    component_path: &ComponentPath,
    tablet_id: &TabletId,
    table_name: TableName,
    by_id: &IndexRef,
    usage: &FunctionUsageTracker,
    update_progress: &F,
    table_total_docs: u64,
    in_component_str: &str,
) -> anyhow::Result<()>
where
    F: Fn(String) -> Fut + Send,
    Fut: Future<Output = anyhow::Result<()>> + Send,
{
    let mut table_upload = zip_snapshot_upload
        .start_table(path_prefix, table_name.clone())
        .await?;

    let stream = table_iterator.stream_documents_in_table(*tablet_id, *by_id, None);
    pin_mut!(stream);

    // Write documents from stream to table uploads
    let mut num_documents: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut last_log_time = Instant::now();
    while let Some(LatestDocument { value: doc, .. }) = stream.try_next().await? {
        let doc_size = doc.size() as u64;
        usage.track_database_egress(component_path.clone(), &table_name, doc_size, false);
        usage.track_database_egress_v2(component_path.clone(), &table_name, doc_size, false);
        table_upload.write(doc).await?;
        num_documents += 1;
        total_bytes += doc_size;
        if last_log_time.elapsed() >= *EXPORT_PROGRESS_UPDATE_INTERVAL {
            tracing::info!(
                "Export table {table_name} in progress: {num_documents} documents, {total_bytes} \
                 bytes written so far",
            );
            update_progress(format!(
                "Backing up {table_name}{in_component_str}: {} / {} documents",
                num_documents.separate_with_commas(),
                table_total_docs.separate_with_commas(),
            ))
            .await?;
            last_log_time = Instant::now();
        }
    }
    tracing::info!(
        "Export table {table_name} complete: {num_documents} documents, {total_bytes} bytes",
    );

    table_upload.complete().await?;
    zip_snapshot_upload
        .write_legacy_generated_schema(path_prefix, &table_name)
        .await?;
    Ok(())
}

async fn construct_zip_snapshot<F, Fut, RT: Runtime>(
    components: &ExportComponents<RT>,
    mut writer: ChannelWriter,
    tables: BTreeMap<TabletId, (TableNamespace, TableNumber, TableName, TableCount)>,
    mut table_iterator: MultiTableIterator<RT>,
    component_ids_to_paths: BTreeMap<ComponentId, ComponentPath>,
    by_id_indexes: BTreeMap<TabletId, IndexRef>,
    system_tables: BTreeMap<(TableNamespace, TableName), TabletId>,
    storage_table_counts: BTreeMap<TableNamespace, u64>,
    include_storage: bool,
    usage: FunctionUsageTracker,
    requestor: ExportRequestor,
    update_progress: F,
) -> anyhow::Result<()>
where
    F: Fn(String) -> Fut + Send + Copy,
    Fut: Future<Output = anyhow::Result<()>> + Send,
{
    let mut zip_snapshot_upload = ZipSnapshotUpload::new(&mut writer).await?;

    // Aim to write things in fast -> slow order in the zip snapshot. This is
    // helpful, because TableIterator has an overhead proportional to the time
    // since `snapshot_ts`. We create many TableIterator while constructing a
    // zip snapshot, so it is helpful to do this.

    // Backup all the tables-tables. These are generally small.
    for (component_id, component_path) in component_ids_to_paths.iter() {
        let namespace: TableNamespace = (*component_id).into();
        let path_prefix = get_export_path_prefix(component_path);
        let in_component_str = component_path.in_component_str();

        update_progress(format!("Backing up _tables{in_component_str}")).await?;
        let root = get_sampled_span(
            &components.deployment_name,
            "export_worker/write_table",
            &mut components.runtime.rng(),
        )
        .with_properties(|| {
            [
                ("dev.convex.component_path", component_path.to_string()),
                ("dev.convex.table_name", "_tables".to_string()),
            ]
        });
        write_tables_table(&path_prefix, &mut zip_snapshot_upload, namespace, &tables)
            .in_span(root)
            .await?;
    }

    // sort tables small to large, and write them to the zip.
    let mut sorted_tables: Vec<_> = tables.iter().collect();
    sorted_tables.sort_by_key(|(_, (_, _, _, table_count))| table_count.total_size());
    for (tablet_id, (namespace, _, table_name, table_count)) in sorted_tables {
        let component_id: ComponentId = (*namespace).into();
        let Some(component_path) = component_ids_to_paths.get(&component_id) else {
            tracing::info!(
                "Table {table_name} in namespace {namespace:?} has no component. Skipping."
            );
            continue;
        };
        let in_component_str = component_path.in_component_str();
        let path_prefix = get_export_path_prefix(component_path);
        let by_id = by_id_indexes
            .get(tablet_id)
            .ok_or_else(|| anyhow::anyhow!("no by_id index for {} found", tablet_id))?;

        let root = get_sampled_span(
            &components.deployment_name,
            "export_worker/write_table",
            &mut components.runtime.rng(),
        )
        .with_properties(|| {
            [
                ("dev.convex.component_path", component_path.to_string()),
                ("dev.convex.table_name", table_name.to_string()),
            ]
        });

        update_progress(format!("Backing up {table_name}{in_component_str}")).await?;

        write_table(
            &path_prefix,
            &mut zip_snapshot_upload,
            &mut table_iterator,
            component_path,
            tablet_id,
            table_name.clone(),
            by_id,
            &usage,
            &update_progress,
            table_count.num_values(),
            &in_component_str,
        )
        .in_span(root)
        .await?;

        table_iterator.unregister_table(*tablet_id)?;
    }

    // Backup the storage tables last - since the upload/download can be slower
    if include_storage {
        let mut file_storage_size = 0;
        for (component_id, component_path) in component_ids_to_paths {
            let namespace: TableNamespace = component_id.into();
            let path_prefix = get_export_path_prefix(&component_path);
            let in_component_str = component_path.in_component_str();
            update_progress(format!("Backing up _storage{in_component_str}")).await?;

            let root = get_sampled_span(
                &components.deployment_name,
                "export_worker/write_table",
                &mut components.runtime.rng(),
            )
            .with_properties(|| {
                [
                    ("dev.convex.component_path", component_path.to_string()),
                    ("dev.convex.table_name", "_storage".to_string()),
                ]
            });
            let storage_total_entries = storage_table_counts.get(&namespace).copied().unwrap_or(0);
            write_storage_table(
                components,
                &path_prefix,
                &mut zip_snapshot_upload,
                namespace,
                &component_path,
                &mut table_iterator,
                &by_id_indexes,
                &system_tables,
                &usage,
                requestor,
                &update_progress,
                &in_component_str,
                storage_total_entries,
                &mut file_storage_size,
            )
            .in_span(root)
            .await?;
        }
    }

    // Complete upload.
    zip_snapshot_upload.complete().await?;
    writer.compat_write().close().await?;
    Ok(())
}

fn get_export_path_prefix(component_path: &ComponentPath) -> String {
    component_path
        .iter()
        .map(|parent_name| {
            format!(
                "{}/{}/",
                &*COMPONENTS_TABLE,
                String::from(parent_name.clone())
            )
        })
        .join("")
}
