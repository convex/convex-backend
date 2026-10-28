#![allow(non_snake_case)]
use std::{
    collections::BTreeMap,
    fmt,
    str::FromStr,
};

use anyhow::Context;
use bytes::Bytes;
use common::{
    audit_log_lines::AuditLogLine,
    bootstrap_model::components::handles::FunctionHandle,
    components::{
        CanonicalizedComponentFunctionPath,
        PublicFunctionPath,
        Reference,
        ResolvedComponentFunctionPath,
        Resource,
    },
    document::DeveloperDocument,
    knobs::{
        COMPONENT_GET_USER_IDENTITY_LOG_SAMPLE_RATIO,
        MAX_REACTOR_CALL_DEPTH,
        MAX_SYSCALL_BATCH_SIZE,
        TRANSACTION_MAX_READ_SIZE_ROWS,
    },
    query::{
        Cursor,
        CursorPosition,
        Query,
    },
    query_journal::QueryJournal,
    runtime::{
        Runtime,
        UnixTimestamp,
    },
    sha256::Sha256,
    try_anyhow,
    types::{
        AllowedVisibility,
        DeploymentClass,
        ObjectKey,
        RegionName,
        StorageUuid,
        UdfType,
        WriteTimestamp,
    },
    value::ConvexValue,
    version::Version,
};
use database::{
    query::{
        query_batch_next,
        PaginationOptions,
        TableFilter,
    },
    soft_data_limit,
    table_summary::table_summary_bootstrapping_error,
    BootstrapComponentsModel,
    DeveloperQuery,
    PatchValue,
    PendingFileUpload,
    Transaction,
    TransactionLimits,
    UserFacingModel,
};
use deno_core::v8;
use errors::{
    ErrorCode,
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use headers::ContentType;
use itertools::Itertools;
use model::{
    components::{
        handles::FunctionHandlesModel,
        ComponentsModel,
    },
    deployment_audit_log::{
        types::DeploymentAuditLogEvent,
        DeploymentAuditLogModel,
    },
    file_storage::{
        types::FileStorageEntry,
        BatchKey,
        FileStorageId,
        FileStorageModel,
    },
    scheduled_jobs::{
        VirtualSchedulerModel,
        MIN_NPM_VERSION_MUTATION_SELF_CANCEL,
    },
    virtual_system_mapping,
};
use rand::Rng;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    value::RawValue,
    Value as JsonValue,
};
use sync_types::{
    types::SerializedArgs,
    AuthenticationToken,
};
use udf::{
    helpers::UdfArgsJson,
    validation::{
        validate_schedule_args,
        PendingArgsPolicy,
        ValidatedPathAndArgs,
    },
    NestedUdfOutcome,
};
use value::{
    heap_size::HeapSize,
    id_v6::DeveloperDocumentId,
    obj,
    serialized_args_ext::SerializedArgsExt,
    sha256::Sha256Digest,
    ConvexArray,
    ConvexObject,
    PendingValue,
    TableName,
    TableNamespace,
};

use super::DatabaseUdfSyscallProvider;
use crate::{
    client::{
        EnvironmentData,
        UdfCallback,
        UdfRequest,
    },
    environment::{
        action::parse_name_or_reference,
        helpers::{
            check_table_name,
            parse_version,
            remove_rejected_before_execution,
            with_argument_error,
            ArgName,
        },
    },
    metrics::{
        async_syscall_timer,
        log_component_get_user_identity,
        log_run_udf,
    },
};

/// A type for UDFs that can be run as subtransactions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestedUdfType {
    Query,
    Mutation,
    SnapshotQuery,
}

impl NestedUdfType {
    /// Returns the underlying UdfType used for execution.
    pub fn execution_type(&self) -> UdfType {
        match self {
            Self::Query | Self::SnapshotQuery => UdfType::Query,
            Self::Mutation => UdfType::Mutation,
        }
    }
}

impl fmt::Display for NestedUdfType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query => write!(f, "Query"),
            Self::Mutation => write!(f, "Mutation"),
            Self::SnapshotQuery => write!(f, "SnapshotQuery"),
        }
    }
}

impl FromStr for NestedUdfType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "snapshotQuery" => Ok(Self::SnapshotQuery),
            _ => {
                let udf_type: UdfType = s.parse()?;
                match udf_type {
                    UdfType::Query => Ok(Self::Query),
                    UdfType::Mutation => Ok(Self::Mutation),
                    _ => anyhow::bail!(
                        "Only queries and mutations can be called as nested UDFs, got {udf_type}"
                    ),
                }
            },
        }
    }
}

pub struct PendingSyscall {
    pub name: String,
    pub args: JsonValue,
    pub resolver: v8::Global<v8::PromiseResolver>,
}

impl HeapSize for PendingSyscall {
    fn heap_size(&self) -> usize {
        self.name.heap_size() + self.args.heap_size()
    }
}

// Checks if the underlying table and the request's expectation for the table
// line up.
pub fn system_table_guard(name: &TableName, expect_system_table: bool) -> anyhow::Result<()> {
    if expect_system_table && !name.is_system() {
        return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
            "SystemTableError",
            "User tables cannot be accessed with db.system."
        )));
    } else if !expect_system_table && name.is_system() {
        return Err(anyhow::anyhow!(ErrorMetadata::bad_request(
            "SystemTableError",
            "System tables can only be accessed with db.system."
        )));
    }
    Ok(())
}

/// A batch of async syscalls that can run "in parallel", where they actually
/// execute in a batch for determinism, but as far as the js promises are
/// concerned, they're running in parallel.
/// This could conceivably run reads
/// (get/queryStreamNext/queryPage/storageGetUrl) all in a single batch.
/// We could also run inserts, deletes, patches, and replaces in a batch
/// together, disjoint from reads, as long as the affected IDs are disjoint.
/// For now, we only allow batches of `db.get`s and `db.query`s.
/// TODO(lee) implement other kinds of batches.
#[derive(Debug, Clone, PartialEq)]
pub enum AsyncSyscallBatch {
    Reads(Vec<AsyncRead>),
    StorageGetUrls(Vec<JsonValue>),
    StorageGets(Vec<JsonValue>),
    Unbatched { name: String, args: JsonValue },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AsyncRead {
    Get(JsonValue),
    QueryStreamNext(JsonValue),
}

impl HeapSize for AsyncSyscallBatch {
    fn heap_size(&self) -> usize {
        match self {
            Self::Reads(reads) => reads.iter().map(HeapSize::heap_size).sum(),
            Self::StorageGetUrls(args) | Self::StorageGets(args) => {
                args.iter().map(HeapSize::heap_size).sum()
            },
            Self::Unbatched { name, args } => name.heap_size() + args.heap_size(),
        }
    }
}

impl HeapSize for AsyncRead {
    fn heap_size(&self) -> usize {
        match self {
            Self::Get(args) | Self::QueryStreamNext(args) => args.heap_size(),
        }
    }
}

impl AsyncSyscallBatch {
    pub fn new(name: String, args: JsonValue) -> Self {
        match &*name {
            "1.0/get" => Self::Reads(vec![AsyncRead::Get(args)]),
            "1.0/queryStreamNext" => Self::Reads(vec![AsyncRead::QueryStreamNext(args)]),
            "1.0/storageGetUrl" => Self::StorageGetUrls(vec![args]),
            "1.0/storageGet" => Self::StorageGets(vec![args]),
            _ => Self::Unbatched { name, args },
        }
    }

    pub fn can_push(&self, name: &str, _args: &JsonValue) -> bool {
        if self.len() >= *MAX_SYSCALL_BATCH_SIZE {
            return false;
        }
        match (self, name) {
            (Self::Reads(_), "1.0/get") => true,
            (Self::Reads(_), "1.0/queryStreamNext") => true,
            (Self::Reads(_), _) => false,
            (Self::StorageGetUrls(_), "1.0/storageGetUrl") => true,
            (Self::StorageGetUrls(_), _) => false,
            (Self::StorageGets(_), "1.0/storageGet") => true,
            (Self::StorageGets(_), _) => false,
            (Self::Unbatched { .. }, _) => false,
        }
    }

    pub fn push(&mut self, name: String, args: JsonValue) -> anyhow::Result<()> {
        match (&mut *self, &*name) {
            (Self::Reads(batch_args), "1.0/get") => batch_args.push(AsyncRead::Get(args)),
            (Self::Reads(batch_args), "1.0/queryStreamNext") => {
                batch_args.push(AsyncRead::QueryStreamNext(args))
            },
            (Self::StorageGetUrls(batch_args), "1.0/storageGetUrl") => {
                batch_args.push(args);
            },
            (Self::StorageGets(batch_args), "1.0/storageGet") => {
                batch_args.push(args);
            },
            _ => anyhow::bail!("cannot push {name} onto {self:?}"),
        }
        Ok(())
    }

    pub fn name(&self) -> &str {
        match self {
            // 1.0/get is grouped in with 1.0/queryStreamNext.
            Self::Reads(_) => "1.0/queryStreamNext",
            Self::StorageGetUrls(_) => "1.0/storageGetUrl",
            Self::StorageGets(_) => "1.0/storageGet",
            Self::Unbatched { name, .. } => name,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Reads(args) => args.len(),
            Self::StorageGetUrls(args) => args.len(),
            Self::StorageGets(args) => args.len(),
            Self::Unbatched { .. } => 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct QueryManager<RT: Runtime> {
    next_id: u32,
    developer_queries: BTreeMap<u32, DeveloperQuery<RT>>,
}

impl<RT: Runtime> QueryManager<RT> {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            developer_queries: BTreeMap::new(),
        }
    }

    pub fn put_developer(&mut self, query: DeveloperQuery<RT>) -> u32 {
        let id = self.next_id;
        self.developer_queries.insert(id, query);
        self.next_id += 1;
        id
    }

    pub fn take_developer(&mut self, id: u32) -> Option<DeveloperQuery<RT>> {
        self.developer_queries.remove(&id)
    }

    pub fn insert_developer(&mut self, id: u32, query: DeveloperQuery<RT>) {
        self.developer_queries.insert(id, query);
    }

    pub fn cleanup_developer(&mut self, id: u32) -> bool {
        self.developer_queries.remove(&id).is_some()
    }
}

pub enum ManagedQuery<RT: Runtime> {
    Pending {
        query: Query,
        version: Option<Version>,
    },
    Active(DeveloperQuery<RT>),
}

pub type QueryId = u32;

impl<RT: Runtime> DatabaseUdfSyscallProvider<RT> {
    fn is_system(&self) -> bool {
        self.path.udf_path.is_system()
    }

    fn table_filter(&self) -> TableFilter {
        if self.path.udf_path.is_system() {
            TableFilter::IncludePrivateSystemTables
        } else {
            TableFilter::ExcludePrivateSystemTables
        }
    }

    async fn validate_schedule_args(
        &mut self,
        path: CanonicalizedComponentFunctionPath,
        args: Vec<JsonValue>,
        scheduled_ts: UnixTimestamp,
    ) -> anyhow::Result<(CanonicalizedComponentFunctionPath, ConvexArray)> {
        validate_schedule_args(
            path,
            args,
            scheduled_ts,
            self.phase.unix_timestamp()?,
            self.phase.tx()?,
        )
        .await
    }

    async fn file_storage_generate_upload_url(&mut self) -> anyhow::Result<String> {
        let issued_ts = self.phase.unix_timestamp()?;
        let component = self.phase.component()?;
        let post_url = self
            .file_storage
            .generate_upload_url(self.phase.tx()?, &self.key_broker, issued_ts, component)
            .await?;
        Ok(post_url)
    }

    async fn file_storage_get_url_batch(
        &mut self,
        storage_ids: BTreeMap<BatchKey, FileStorageId>,
    ) -> BTreeMap<BatchKey, anyhow::Result<Option<String>>> {
        let component = match self.phase.component() {
            Ok(c) => c,
            Err(e) => {
                return storage_ids
                    .into_keys()
                    .map(|batch_key| (batch_key, Err(e.clone_error())))
                    .collect();
            },
        };
        let tx = match self.phase.tx() {
            Ok(tx) => tx,
            Err(e) => {
                return storage_ids
                    .into_keys()
                    .map(|batch_key| (batch_key, Err(e.clone_error())))
                    .collect();
            },
        };
        self.file_storage
            .get_url_batch(tx, component, storage_ids)
            .await
    }

    /// Writes the `_file_storage` entry for a file stored by this function and
    /// stages its contents for upload once the function finishes.
    async fn file_storage_store(
        &mut self,
        blob: Vec<u8>,
        content_type: Option<ContentType>,
        expected_sha256: Option<Sha256Digest>,
    ) -> anyhow::Result<DeveloperDocumentId> {
        let sha256 = Sha256::hash(&blob);
        if let Some(expected_sha256) = expected_sha256
            && expected_sha256 != sha256
        {
            anyhow::bail!(ErrorMetadata::bad_request(
                "Sha256Mismatch",
                format!(
                    "Sha256 mismatch. Expected: {} Actual: {}",
                    expected_sha256.as_base64(),
                    sha256.as_base64()
                )
            ));
        }
        let storage_key: ObjectKey = self.rt.new_uuid_v4().to_string().try_into()?;
        let storage_id = StorageUuid::from(self.rt.new_uuid_v4());

        let component = self.phase.component()?;
        let namespace: TableNamespace = component.into();
        let tx = self.phase.tx()?;
        let max_files = tx.transaction_limits().files_written;
        let max_bytes = tx.transaction_limits().file_write_bytes;
        let written = tx.execution_size().file_storage_write_size;
        anyhow::ensure!(
            written.num_writes < max_files,
            ErrorMetadata::bad_request(
                "TooManyFilesWritten",
                format!("Too many files stored by this mutation (limit: {max_files})")
            )
        );
        anyhow::ensure!(
            written.size + blob.len() <= max_bytes,
            ErrorMetadata::bad_request(
                "FilesWrittenTooLarge",
                format!(
                    "Too large total size of the files stored by this mutation (limit: \
                     {max_bytes} bytes)"
                )
            )
        );
        let entry = FileStorageEntry {
            storage_id,
            storage_key,
            sha256,
            size: blob.len().try_into()?,
            content_type: content_type.map(|ct| ct.to_string()),
        };
        let component_path = tx.must_component_path(component)?;
        let (id, virtual_id) = self
            .file_storage
            .store_file_entry(tx, namespace, entry)
            .await?;
        tx.add_pending_file_upload(
            id,
            PendingFileUpload {
                component_path,
                bytes: blob.into(),
            },
        )?;
        Ok(virtual_id)
    }

    async fn file_storage_delete(&mut self, storage_id: FileStorageId) -> anyhow::Result<()> {
        let component = self.phase.component()?;
        let tx = self.phase.tx()?;
        let id = self
            .file_storage
            .delete(tx, component.into(), storage_id)
            .await?;
        tx.remove_pending_file_upload(&id)?;
        Ok(())
    }

    async fn file_storage_get_entry(
        &mut self,
        storage_id: FileStorageId,
    ) -> anyhow::Result<Option<FileStorageEntry>> {
        let component = self.phase.component()?;
        self.file_storage
            .get_file_entry(self.phase.tx()?, component.into(), storage_id)
            .await
    }

    async fn file_storage_get_batch(
        &mut self,
        storage_ids: BTreeMap<BatchKey, FileStorageId>,
    ) -> BTreeMap<BatchKey, anyhow::Result<Option<(Bytes, Option<String>)>>> {
        match self.try_file_storage_get_batch(&storage_ids).await {
            Ok(results) => results,
            Err(e) => storage_ids
                .into_keys()
                .map(|batch_key| (batch_key, Err(e.clone_error())))
                .collect(),
        }
    }

    async fn try_file_storage_get_batch(
        &mut self,
        storage_ids: &BTreeMap<BatchKey, FileStorageId>,
    ) -> anyhow::Result<BTreeMap<BatchKey, anyhow::Result<Option<(Bytes, Option<String>)>>>> {
        if self.udf_type != UdfType::Mutation {
            anyhow::bail!(ErrorMetadata::bad_request(
                "StorageGetNotSupported",
                "Getting files from storage is only supported in mutations and actions",
            ));
        }
        let component = self.phase.component()?;
        let tx = self.phase.tx()?;
        let component_path = tx.must_component_path(component)?;
        let entries = FileStorageModel::new(tx, component.into())
            .get_file_batch(storage_ids.clone())
            .await;

        let mut results = BTreeMap::new();
        let mut from_downloads = BTreeMap::new();
        let mut to_download = BTreeMap::new();
        for (batch_key, entry) in entries {
            let entry = match entry {
                Ok(Some(entry)) => entry,
                Ok(None) => {
                    results.insert(batch_key, Ok(None));
                    continue;
                },
                Err(e) => {
                    results.insert(batch_key, Err(e));
                    continue;
                },
            };
            let content_type = entry.content_type.clone();
            if let Some(upload) = tx.get_pending_file_upload(&entry.id())? {
                results.insert(batch_key, Ok(Some((upload.bytes, content_type))));
                continue;
            }
            let storage_key = entry.storage_key.clone();
            if self.file_downloads.get(&storage_key).is_none()
                && !to_download.contains_key(&storage_key)
            {
                if let Err(e) = check_file_read_limits(tx, entry.size.try_into()?) {
                    results.insert(batch_key, Err(e));
                    continue;
                }
                tx.add_file_storage_read(entry.size.try_into()?);
                to_download.insert(
                    storage_key.clone(),
                    (component_path.clone(), entry.into_value()),
                );
            }
            from_downloads.insert(batch_key, (storage_key, content_type));
        }
        if !to_download.is_empty() {
            let usage_tracker = tx.usage_tracker.clone();
            self.file_storage
                .download_files(
                    &self.file_downloads,
                    to_download.into_values().collect(),
                    usage_tracker,
                )
                .await?;
        }
        for (batch_key, (storage_key, content_type)) in from_downloads {
            let bytes = self
                .file_downloads
                .get(&storage_key)
                .context("Downloaded file missing from file_downloads")?;
            results.insert(batch_key, Ok(Some((bytes, content_type))));
        }
        Ok(results)
    }

    #[fastrace::trace]
    async fn run_udf(
        &mut self,
        nested_udf_type: NestedUdfType,
        path: ResolvedComponentFunctionPath,
        args: PendingValue,
        transaction_limits: Option<TransactionLimits>,
        udf_callback: impl UdfCallback<RT>,
    ) -> anyhow::Result<PendingValue> {
        match (self.udf_type, nested_udf_type) {
            // Queries can call other queries, but not snapshot queries.
            (UdfType::Query, NestedUdfType::Query) => (),
            // Mutations can call queries (including snapshot queries) or mutations.
            (
                UdfType::Mutation,
                NestedUdfType::Query | NestedUdfType::SnapshotQuery | NestedUdfType::Mutation,
            ) => (),
            _ => {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "InvalidFunctionCall",
                    format!(
                        "Cannot call a {} function from a {} function",
                        nested_udf_type, self.udf_type
                    )
                ));
            },
        }
        let tx = self.phase.tx()?;
        let called_component_id = path.component;

        let execution_type = nested_udf_type.execution_type();
        let pending_args_policy = match self.udf_type {
            UdfType::Mutation => PendingArgsPolicy::Allow,
            _ => PendingArgsPolicy::Reject,
        };
        let path_and_args_result = ValidatedPathAndArgs::new_with_returns_validator(
            AllowedVisibility::All,
            tx,
            PublicFunctionPath::ResolvedComponent(path.clone()),
            SerializedArgs::from_raw(serde_json::value::to_raw_value(&[
                args.to_uncommitted_json_serializable()
            ])?),
            execution_type,
            pending_args_policy,
        )
        .await?;
        // We don't need to store visibility_info for non-queries
        let (path_and_args, returns_validator, _visibility_info) = match path_and_args_result {
            Ok(r) => r,
            Err(e) => {
                // TODO: Propagate this JsError to user space correctly.
                anyhow::bail!(ErrorMetadata::bad_request("InvalidArgs", e.message));
            },
        };

        // NB: Since this is a user error, we need to do this check before we take the
        // transaction below.
        if self.reactor_depth >= *MAX_REACTOR_CALL_DEPTH {
            anyhow::bail!(ErrorMetadata::bad_request(
                "MaximumCallDepthExceeded",
                "Cross component call depth limit exceeded. Do you have an infinite loop in your \
                 app?"
            ));
        }
        let new_reactor_depth = self.reactor_depth + 1;

        let (initial_tx, rng_seed, unix_timestamp) = self.phase.start_nested_udf()?;
        let (mut nested_tx, saved_tx) = match nested_udf_type {
            NestedUdfType::SnapshotQuery => {
                (initial_tx.clone_for_snapshot_query(), Some(initial_tx))
            },
            _ => (initial_tx, None),
        };

        let tokens = nested_tx.begin_subtransaction();

        if let Some(limits) = transaction_limits {
            nested_tx.set_transaction_limits(limits);
        }

        let share_query_journal =
            self.udf_type == UdfType::Query && nested_udf_type == NestedUdfType::Query;
        let query_journal = if share_query_journal {
            self.prev_journal.clone()
        } else {
            QueryJournal::new()
        };
        let (mut result_tx, outcome) = udf_callback
            .execute_nested_udf(
                self.client_id.clone(),
                UdfRequest {
                    udf_type: execution_type,
                    path_and_args,
                    transaction: nested_tx,
                    unix_timestamp,
                    journal: query_journal,
                    context: self.context.clone(),
                    environment_data: EnvironmentData {
                        key_broker: self.key_broker.clone(),
                        default_system_env_vars: self.phase.default_system_env_vars().clone(),
                        file_storage: self.file_storage.clone(),
                        module_loader: self.phase.module_loader().clone(),
                        deployment: self.deployment.clone(),
                    },
                    file_downloads: self.file_downloads.clone(),
                },
                rng_seed,
                new_reactor_depth,
            )
            .await
            .map_err(|mut e| {
                e = remove_rejected_before_execution(e);
                if let Some(em) = e.downcast_mut::<ErrorMetadata>()
                    && em.is_deterministic_user_error()
                {
                    // This is a bit gross, but at this layer, "deterministic
                    // user errors" get converted into catchable JS exceptions.
                    // However, if there is an error at this point, we cannot
                    // return to JS because the transaction has been lost.
                    //
                    // So upgrade such an error to a non-catchable system error.
                    // However this should only be happening for nested system
                    // timeouts and so it's likely that this error will again be
                    // replaced with a higher error.
                    em.code = ErrorCode::OperationalInternalServerError;
                    tracing::warn!("Upgrading error from nested UDF: {e:#}");
                }
                e
            })?;
        match nested_udf_type {
            NestedUdfType::Mutation if outcome.result.is_err() => {
                result_tx.rollback_subtransaction(tokens)?
            },
            _ => result_tx.commit_subtransaction(tokens)?,
        }
        if let Some(tx) = saved_tx {
            self.phase.put_tx(tx)?;
        } else {
            self.phase.put_tx(result_tx)?;
        }

        let NestedUdfOutcome {
            result,
            observed_identity,
            observed_rng,
            observed_time,
            syscall_trace,
            audit_log_lines,
            log_lines,
            journal,
        } = outcome;

        log_run_udf(
            self.udf_type,
            execution_type,
            self.phase.observed_identity(),
            observed_identity,
        );

        if observed_identity {
            self.phase.observe_identity()?;
        }

        if observed_rng {
            self.phase.observe_rng();
        }

        if observed_time {
            self.phase.unix_timestamp()?;
        }

        self.syscall_trace.merge(&syscall_trace);

        if share_query_journal && result.is_ok() && journal.end_cursor.is_some() {
            anyhow::ensure!(
                self.next_journal.end_cursor.is_none(),
                ErrorMetadata::bad_request(
                    "MultiplePaginatedDatabaseQueries",
                    "This query function ran multiple paginated queries. Convex only supports a \
                     single paginated query in each query function.",
                )
            );
            self.next_journal = journal;
        }

        // TODO(ENG-7663): restrict log lines within subfunctions instead of
        // limiting them only when they are returned to the parent.
        self.emit_sub_function_log_lines(path.for_logging(), log_lines);

        for audit_log_line in audit_log_lines {
            self.emit_audit_log_line(audit_log_line)?;
        }

        // TODO: How do we want to propagate stack traces between component calls?
        let result = result?;
        let tx = self.phase.tx()?;
        let table_mapping = tx.table_mapping().namespace(called_component_id.into());
        if let Some(e) = returns_validator.check_pending_output(
            &result,
            &table_mapping,
            virtual_system_mapping(),
        )? {
            anyhow::bail!(ErrorMetadata::bad_request("InvalidReturnValue", e.message));
        }
        Ok(result)
    }

    async fn create_function_handle(
        &mut self,
        path: CanonicalizedComponentFunctionPath,
    ) -> anyhow::Result<FunctionHandle> {
        let tx = self.phase.tx()?;
        FunctionHandlesModel::new(tx)
            .get_with_component_path(path)
            .await
    }

    async fn resolve(&mut self, reference: Reference) -> anyhow::Result<Resource> {
        let current_component_id = self.phase.component()?;
        let current_udf_path = self.path.udf_path.clone().into();

        let tx = self.phase.tx()?;

        ComponentsModel::new(tx)
            .resolve(current_component_id, Some(current_udf_path), &reference)
            .await
    }

    async fn lookup_function_handle(
        &mut self,
        handle: FunctionHandle,
    ) -> anyhow::Result<CanonicalizedComponentFunctionPath> {
        FunctionHandlesModel::new(self.phase.tx()?)
            .lookup(handle)
            .await
    }
}

#[fastrace::trace]
pub(super) async fn run_async_syscall_batch<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    batch: AsyncSyscallBatch,
    udf_callback: impl UdfCallback<RT>,
) -> Vec<anyhow::Result<String>> {
    let start = provider.phase.rt.monotonic_now();
    let batch_name = batch.name().to_string();
    let timer = async_syscall_timer(&batch_name);
    // Outer error is a system error that encompases the whole batch, while
    // inner errors are for individual batch items that may be system or developer
    // errors.
    let results = match batch {
        AsyncSyscallBatch::Reads(batch_args) => query_batch(provider, batch_args).await,
        AsyncSyscallBatch::StorageGetUrls(batch_args) => {
            storage_get_url_batch(provider, batch_args).await
        },
        AsyncSyscallBatch::StorageGets(batch_args) => storage_get_batch(provider, batch_args).await,
        AsyncSyscallBatch::Unbatched { name, args } => {
            let result = match &name[..] {
                // Database
                "1.0/count" => Box::pin(count(provider, args)).await,
                "1.0/insert" => Box::pin(insert(provider, args)).await,
                "1.0/shallowMerge" => Box::pin(shallow_merge(provider, args)).await,
                "1.0/replace" => Box::pin(replace(provider, args)).await,
                "1.0/remove" => Box::pin(remove(provider, args)).await,
                "1.0/queryPage" => Box::pin(query_page(provider, args)).await,
                "1.0/getTransactionMetrics" => tx_metrics(provider),
                "1.0/getFunctionMetadata" => function_metadata(provider),
                "1.0/getDeploymentMetadata" => deployment_metadata(provider),
                "1.0/getRequestMetadata" => request_metadata(provider),
                // Auth
                "1.0/getUserIdentity" => Box::pin(get_user_identity(provider, args)).await,
                "1.0/signJwt" => Box::pin(sign_deployment_jwt(provider, args)).await,
                // Storage
                "1.0/storageDelete" => Box::pin(storage_delete(provider, args)).await,
                "1.0/storageGetMetadata" => Box::pin(storage_get_metadata(provider, args)).await,
                "1.0/storageGenerateUploadUrl" => {
                    Box::pin(storage_generate_upload_url(provider, args)).await
                },
                "1.0/storageStore" => Box::pin(storage_store(provider, args)).await,
                // Scheduling
                "1.0/schedule" => Box::pin(schedule(provider, args)).await,
                "1.0/cancel_job" => Box::pin(cancel_job(provider, args)).await,

                // Audit logging
                "1.0/auditLog" => Box::pin(audit_log(provider, args)).await,
                // Audit logging (system UDFs only)
                "1.0/writeDeploymentAuditLog" => {
                    Box::pin(write_deployment_audit_log(provider, args)).await
                },

                // Components
                "1.0/runUdf" => Box::pin(run_udf(provider, args, udf_callback)).await,
                "1.0/createFunctionHandle" => {
                    Box::pin(create_function_handle(provider, args)).await
                },

                _ => Err(ErrorMetadata::bad_request(
                    "UnknownAsyncOperation",
                    format!("Unknown async operation {name}"),
                )
                .into()),
            };
            vec![result]
        },
    };
    provider.syscall_trace.log_async_syscall(
        batch_name,
        start.elapsed(),
        results.iter().all(|result| result.is_ok()),
    );
    timer.finish();
    results
        .into_iter()
        .map(|r| r.map(|json| <Box<str>>::from(json).into_string()))
        .collect()
}

/// Returns the remaining headroom for this transaction before hitting
/// limits.
fn tx_metrics<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
) -> anyhow::Result<Box<RawValue>> {
    let tx = provider.phase.tx()?;
    let s = tx.execution_size();
    let limits = tx.transaction_limits();
    #[derive(Serialize)]
    struct LimitedValue {
        used: usize,
        remaining: isize,
    }
    let limit_value = |limit: usize, used: usize| {
        let remaining = limit as isize - used as isize;
        LimitedValue { used, remaining }
    };
    #[allow(non_snake_case)]
    #[derive(Serialize)]
    struct TxMetricsJson {
        bytesRead: LimitedValue,
        bytesWritten: LimitedValue,
        databaseQueries: LimitedValue,
        documentsRead: LimitedValue,
        documentsWritten: LimitedValue,
        functionsScheduled: LimitedValue,
        scheduledFunctionArgsBytes: LimitedValue,
        filesWritten: LimitedValue,
        fileWriteBytes: LimitedValue,
        filesRead: LimitedValue,
        fileReadBytes: LimitedValue,
    }
    Ok(serde_json::value::to_raw_value(&TxMetricsJson {
        bytesRead: limit_value(limits.bytes_read, s.read_size.total_document_size),
        bytesWritten: limit_value(limits.bytes_written, s.write_size.size),
        databaseQueries: limit_value(limits.database_queries, s.num_intervals),
        documentsRead: limit_value(limits.documents_read, s.read_size.total_document_count),
        documentsWritten: limit_value(limits.documents_written, s.write_size.num_writes),
        functionsScheduled: limit_value(limits.functions_scheduled, s.scheduled_size.num_writes),
        scheduledFunctionArgsBytes: limit_value(
            limits.scheduled_function_args_bytes,
            s.scheduled_size.size,
        ),
        filesWritten: limit_value(limits.files_written, s.file_storage_write_size.num_writes),
        fileWriteBytes: limit_value(limits.file_write_bytes, s.file_storage_write_size.size),
        filesRead: limit_value(limits.files_read, s.file_storage_read_size.num_reads),
        fileReadBytes: limit_value(limits.file_read_bytes, s.file_storage_read_size.size),
    })?)
}

/// Returns metadata about the currently executing function.
fn function_metadata<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
) -> anyhow::Result<Box<RawValue>> {
    let udf_path = &provider.path.udf_path;
    let component_path = &provider.path.component_path;
    #[allow(non_snake_case)]
    #[derive(Serialize)]
    struct FunctionMetadataJson {
        name: String,
        componentPath: String,
    }
    Ok(serde_json::value::to_raw_value(&FunctionMetadataJson {
        name: udf_path.clone().strip().to_string(),
        componentPath: component_path.to_string(),
    })?)
}

/// Returns metadata about the deployment this function is running on.
fn deployment_metadata<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
) -> anyhow::Result<Box<RawValue>> {
    let deployment = &provider.deployment;
    #[allow(non_snake_case)]
    #[derive(Serialize)]
    struct DeploymentMetadataJson<'a> {
        name: &'a String,
        region: &'a Option<RegionName>,
        class: &'a DeploymentClass,
    }
    Ok(serde_json::value::to_raw_value(&DeploymentMetadataJson {
        name: &deployment.name,
        region: &deployment.region,
        class: &deployment.class,
    })?)
}

/// Returns metadata about the originating HTTP request.
fn request_metadata<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
) -> anyhow::Result<Box<RawValue>> {
    anyhow::ensure!(
        provider.udf_type == UdfType::Mutation,
        ErrorMetadata::bad_request(
            "RequestMetadataNotAllowed",
            format!("Cannot get request metadata in a {}", provider.udf_type)
        )
    );
    // Expose the raw auth JWT the request was authenticated with, if any. Only
    // `User` identities carry a JWT (an OIDC or custom JWT); admin keys and
    // logged-out requests have no token.
    provider.phase.observe_identity()?;
    let auth_token = match provider.phase.tx()?.authentication_token() {
        AuthenticationToken::User(token) => Some(token),
        AuthenticationToken::Admin(..) | AuthenticationToken::None => None,
    };
    let context = &provider.context;
    let metadata = &context.request_metadata;
    // The top-level scheduled function and all of its descendants (e.g. a
    // mutation called by a scheduled action) report the scheduled function's
    // id, since `parent_scheduled_job` is propagated down the call tree. It
    // is `None` when the function was not scheduled.
    let scheduled_function_id = context
        .parent_scheduled_job
        .map(|(_, job_id)| job_id.encode());
    #[allow(non_snake_case)]
    #[derive(Serialize)]
    struct RequestMetadataJson<'a> {
        ip: Option<&'a str>,
        userAgent: Option<&'a str>,
        requestId: &'a str,
        scheduledFunctionId: Option<String>,
        authToken: Option<String>,
    }
    Ok(serde_json::value::to_raw_value(&RequestMetadataJson {
        ip: metadata.ip.as_ref().map(|ip| ip.as_str()),
        userAgent: metadata.user_agent.as_ref().map(|ua| ua.as_str()),
        requestId: context.request_id.as_str(),
        scheduledFunctionId: scheduled_function_id,
        authToken: auth_token,
    })?)
}

#[convex_macro::instrument_future]
async fn count<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CountArgs {
        table: String,
    }
    let table = with_argument_error("db.count", || {
        let args: CountArgs = serde_json::from_value(args)?;
        args.table.parse().context(ArgName("table"))
    })?;
    let component = provider.phase.component()?;
    let tx = provider.phase.tx()?;
    let result = tx.count(component.into(), &table).await?;
    let Some(result) = result else {
        return Err(table_summary_bootstrapping_error(Some(
            "Table count unavailable while bootstrapping",
        )));
    };

    // Trim to u32 and check for overflow.
    let result = u32::try_from(result)?;
    // Return as f64, which converts to number type in JavaScript.
    let result = f64::from(result);
    Ok(serde_json::value::to_raw_value(
        &ConvexValue::from(result).to_internal_json_serializable(),
    )?)
}

#[convex_macro::instrument_future]
async fn get_user_identity<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    _args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    provider.phase.observe_identity()?;
    // TODO: Somehow make the Transaction aware of the dependency on the user.
    let component = provider.phase.component()?;
    let tx = provider.phase.tx()?;
    let user_identity = tx.user_identity();
    if !component.is_root() {
        log_component_get_user_identity(user_identity.is_some());
        if provider
            .rt
            .rng()
            .random_bool(*COMPONENT_GET_USER_IDENTITY_LOG_SAMPLE_RATIO)
        {
            let component_path = tx.get_component_path_untracked(component);
            tracing::info!(
                component_path = ?component_path,
                has_user_identity = user_identity.is_some(),
                "component called getUserIdentity()"
            );
        }
    }
    if let Some(user_identity) = user_identity {
        return Ok(serde_json::value::to_raw_value(&JsonValue::try_from(
            user_identity,
        )?)?);
    }

    Ok(RawValue::NULL.to_owned())
}

/// Syscall for JWT signing in mutations.
///
/// Not supported in queries (JWT creation involves the current time and query
/// caching makes it too likely that an incorrect value would later be served).
#[convex_macro::instrument_future]
async fn sign_deployment_jwt<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    if provider.udf_type != UdfType::Mutation {
        anyhow::bail!(ErrorMetadata::bad_request(
            "JwtSignInQuery",
            "ctx.auth.signJwt() can't be called from a query. Call it from a mutation.",
        ));
    }
    let default_system_env_vars = provider.phase.default_system_env_vars().clone();
    let component = provider.phase.component()?;
    let tx = provider.phase.tx()?;
    let origin = udf::deployment_jwt::deployment_origin(tx, &default_system_env_vars).await?;
    let signer = tx
        .get_component_path(component)
        .context("Calling component not found")?;
    let keys = provider
        .key_broker
        .deployment_jwt_keys
        .as_ref()
        .context("Function runner caller didn't send the deployment JWT signing key")?;
    let token = udf::deployment_jwt::mint(
        keys,
        &origin,
        signer,
        args,
        provider.phase.unix_timestamp()?.as_system_time(),
    )?;
    Ok(serde_json::value::to_raw_value(&token)?)
}

#[convex_macro::instrument_future]
async fn storage_generate_upload_url<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    _args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    let post_url = provider.file_storage_generate_upload_url().await?;
    Ok(serde_json::value::to_raw_value(&post_url)?)
}

#[convex_macro::instrument_future]
async fn storage_get_url_batch<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    batch_args: Vec<JsonValue>,
) -> Vec<anyhow::Result<Box<RawValue>>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GetUrlArgs {
        storage_id: String,
    }
    let batch_size = batch_args.len();
    let mut results = BTreeMap::new();
    let mut storage_ids = BTreeMap::new();
    for (idx, args) in batch_args.into_iter().enumerate() {
        let storage_id_result = with_argument_error("storage.getUrl", || {
            let GetUrlArgs { storage_id } = serde_json::from_value(args)?;
            storage_id.parse().context(ArgName("storageId"))
        });
        match storage_id_result {
            Ok(storage_id) => {
                storage_ids.insert(idx, storage_id);
            },
            Err(e) => {
                assert!(results.insert(idx, Err(e)).is_none());
            },
        }
    }
    let urls = provider.file_storage_get_url_batch(storage_ids).await;
    for (batch_key, url) in urls {
        assert!(results
            .insert(
                batch_key,
                url.and_then(|url| Ok(serde_json::value::to_raw_value(&url)?))
            )
            .is_none());
    }
    assert_eq!(results.len(), batch_size);
    results.into_values().collect()
}

#[convex_macro::instrument_future]
async fn storage_delete<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StorageDeleteArgs {
        storage_id: String,
    }
    let storage_id: FileStorageId = with_argument_error("storage.delete", || {
        let StorageDeleteArgs { storage_id } = serde_json::from_value(args)?;
        storage_id.parse().context(ArgName("storageId"))
    })?;

    // Synchronously delete the file from storage
    provider.file_storage_delete(storage_id).await?;

    Ok(RawValue::NULL.to_owned())
}

#[convex_macro::instrument_future]
async fn storage_store<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StorageStoreArgs {
        /// Base64-encoded file contents.
        blob: String,
        content_type: Option<String>,
        /// Base64-encoded sha256 of the contents, to check them against.
        sha256: Option<String>,
    }
    let (blob, content_type, expected_sha256) = with_argument_error("storage.store", || {
        let StorageStoreArgs {
            blob,
            content_type,
            sha256,
        } = serde_json::from_value(args)?;
        let blob = base64::decode(&blob).context(ArgName("blob"))?;
        let content_type = content_type
            .filter(|ct| !ct.is_empty())
            .map(|ct| mime::Mime::from_str(&ct).map(ContentType::from))
            .transpose()
            .context(ArgName("contentType"))?;
        let expected_sha256 = sha256
            .as_deref()
            .map(Sha256Digest::from_base64)
            .transpose()
            .context(ArgName("sha256"))?;
        Ok((blob, content_type, expected_sha256))
    })?;

    let storage_id = provider
        .file_storage_store(blob, content_type, expected_sha256)
        .await?;
    Ok(serde_json::value::to_raw_value(&storage_id.encode())?)
}

fn check_file_read_limits<RT: Runtime>(tx: &Transaction<RT>, size: usize) -> anyhow::Result<()> {
    let max_files = tx.transaction_limits().files_read;
    let max_bytes = tx.transaction_limits().file_read_bytes;
    let read = &tx.file_storage_read_size;
    anyhow::ensure!(
        read.num_reads < max_files,
        ErrorMetadata::bad_request(
            "TooManyFilesRead",
            format!("Too many files read by this mutation (limit: {max_files})")
        )
    );
    anyhow::ensure!(
        read.size + size <= max_bytes,
        ErrorMetadata::bad_request(
            "FilesReadTooLarge",
            format!(
                "Too large total size of the files read by this mutation (limit: {max_bytes} \
                 bytes)"
            )
        )
    );
    Ok(())
}

#[convex_macro::instrument_future]
async fn storage_get_batch<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    batch_args: Vec<JsonValue>,
) -> Vec<anyhow::Result<Box<RawValue>>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GetArgs {
        storage_id: String,
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct GetResult {
        /// Base64-encoded file contents.
        blob: String,
        content_type: Option<String>,
    }
    let batch_size = batch_args.len();
    let mut results = BTreeMap::new();
    let mut storage_ids = BTreeMap::new();
    for (idx, args) in batch_args.into_iter().enumerate() {
        let storage_id_result = with_argument_error("storage.get", || {
            let GetArgs { storage_id } = serde_json::from_value(args)?;
            storage_id.parse().context(ArgName("storageId"))
        });
        match storage_id_result {
            Ok(storage_id) => {
                storage_ids.insert(idx, storage_id);
            },
            Err(e) => {
                assert!(results.insert(idx, Err(e)).is_none());
            },
        }
    }
    let files = provider.file_storage_get_batch(storage_ids).await;
    for (batch_key, file) in files {
        let result = file.and_then(|file| {
            let result = file.map(|(contents, content_type)| GetResult {
                blob: base64::encode(&contents),
                content_type,
            });
            Ok(serde_json::value::to_raw_value(&result)?)
        });
        assert!(results.insert(batch_key, result).is_none());
    }
    assert_eq!(results.len(), batch_size);
    results.into_values().collect()
}

#[convex_macro::instrument_future]
async fn storage_get_metadata<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StorageGetMetadataArgs {
        storage_id: String,
    }
    let storage_id: FileStorageId = with_argument_error("storage.getMetadata", || {
        let StorageGetMetadataArgs { storage_id } = serde_json::from_value(args)?;
        storage_id.parse().context(ArgName("storageId"))
    })?;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct FileMetadataJson {
        storage_id: String,
        sha256: String,
        size: i64,
        content_type: Option<String>,
    }
    let file_metadata = provider.file_storage_get_entry(storage_id).await?.map(
        |FileStorageEntry {
             storage_id,
             storage_key: _, // internal field that we shouldn't return in syscalls
             sha256,
             size,
             content_type,
         }| {
            FileMetadataJson {
                storage_id: storage_id.to_string(),
                // TODO(CX-5533) use base64 for consistency.
                sha256: sha256.as_hex(),
                size,
                content_type,
            }
        },
    );
    Ok(serde_json::value::to_raw_value(&file_metadata)?)
}

#[convex_macro::instrument_future]
async fn schedule<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ScheduleArgs {
        name: Option<String>,
        reference: Option<String>,
        function_handle: Option<String>,
        ts: f64,
        args: UdfArgsJson,
    }

    let ScheduleArgs {
        name,
        reference,
        function_handle,
        ts,
        args,
    }: ScheduleArgs = with_argument_error("scheduler", || Ok(serde_json::from_value(args)?))?;

    let path = match function_handle {
        Some(h) => {
            let handle: FunctionHandle = with_argument_error("scheduler", || h.parse())?;
            provider.lookup_function_handle(handle).await?
        },
        None => {
            let reference = parse_name_or_reference("scheduler", name, reference)?;
            match provider.resolve(reference).await? {
                Resource::Value(v) => {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidResource",
                        format!(
                            "Only functions can be scheduled. {} is not a function",
                            v.to_internal_json()
                        ),
                    ));
                },
                Resource::Function(p) => p,
                Resource::ResolvedSystemUdf { .. } => {
                    anyhow::bail!("Cannot schedule function by component id");
                },
            }
        },
    };

    let scheduling_component = provider.phase.component()?;

    let scheduled_ts = with_argument_error("ts", || UnixTimestamp::from_secs_f64(ts))?;
    let (path, udf_args) = provider
        .validate_schedule_args(
            path,
            args.into_serialized_args()?.into_args()?,
            scheduled_ts,
        )
        .await?;

    let context = provider.context.clone();
    let tx = provider.phase.tx()?;
    let virtual_id = VirtualSchedulerModel::new(tx, scheduling_component.into())
        .schedule(path, udf_args, scheduled_ts, context)
        .await?;

    Ok(serde_json::value::to_raw_value(&String::from(virtual_id))?)
}

#[convex_macro::instrument_future]
async fn cancel_job<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CancelJobArgs {
        id: String,
    }
    let component = provider.phase.component()?;

    let virtual_id_v6 = with_argument_error("db.cancel_job", || {
        let args: CancelJobArgs = serde_json::from_value(args)?;
        let id = DeveloperDocumentId::decode(&args.id).context(ArgName("id"))?;
        Ok(id)
    })?;

    // A scheduled mutation observes its own record as `inProgress` via a pending
    // write, so canceling itself would conflict with that write. Clients on
    // MIN_NPM_VERSION_MUTATION_SELF_CANCEL or newer get an error; older clients
    // keep the historical no-op behavior but are warned that it will change.
    if let Some((_, self_job_id)) = provider.context.parent_scheduled_job
        && self_job_id == virtual_id_v6
    {
        if provider
            .udf_server_version
            .as_ref()
            .is_some_and(|version| *version >= *MIN_NPM_VERSION_MUTATION_SELF_CANCEL)
        {
            anyhow::bail!(ErrorMetadata::bad_request(
                "ScheduledFunctionCancelingItself",
                "A mutation cannot cancel itself",
            ));
        }
        provider.emit_warning(
            "A scheduled mutation canceling itself is deprecated and will throw an error in a \
             future version of Convex."
                .to_string(),
        )?;
        return Ok(RawValue::NULL.to_owned());
    }

    let tx = provider.phase.tx()?;
    VirtualSchedulerModel::new(tx, component.into())
        .cancel(virtual_id_v6)
        .await?;

    Ok(RawValue::NULL.to_owned())
}

async fn audit_log<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AuditLogArgs {
        body: JsonValue,
    }
    let args: AuditLogArgs = with_argument_error("auditLog", || Ok(serde_json::from_value(args)?))?;
    provider.emit_audit_log_line(AuditLogLine { body: args.body })?;
    Ok(RawValue::NULL.to_owned())
}

async fn write_deployment_audit_log<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    if !provider.is_system() {
        anyhow::bail!(ErrorMetadata::bad_request(
            "Unauthorized",
            "writeDeploymentAuditLog is only available in system UDFs"
        ));
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AuditLogArgs {
        action: String,
        metadata: serde_json::Map<String, JsonValue>,
    }
    let args: AuditLogArgs = serde_json::from_value(args)?;

    let component_id = provider.phase.component()?;
    let request_metadata = provider.context.request_metadata.clone();
    let tx = provider.phase.tx()?;
    let component_path = tx.must_component_path(component_id)?;

    // Inject component_id and component into metadata
    let mut metadata = args.metadata;
    metadata.insert(
        "component_id".to_string(),
        component_id
            .serialize_to_string()
            .map_or(JsonValue::Null, JsonValue::String),
    );
    metadata.insert(
        "component".to_string(),
        component_path
            .serialize()
            .map_or(JsonValue::Null, JsonValue::String),
    );

    let metadata_value: JsonValue = JsonValue::Object(metadata);
    let metadata: ConvexObject = metadata_value.try_into()?;

    let event_obj = obj!("action" => args.action, "metadata" => metadata)?;
    DeploymentAuditLogModel::new(tx)
        .insert(
            vec![DeploymentAuditLogEvent::try_from(event_obj)?],
            &request_metadata,
        )
        .await?;

    Ok(RawValue::NULL.to_owned())
}

#[fastrace::trace]
#[convex_macro::instrument_future]
async fn insert<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct InsertArgs {
        table: String,
        value: JsonValue,
    }
    let (table, value) = with_argument_error("db.insert", || {
        let args: InsertArgs = serde_json::from_value(args)?;
        let value = PendingValue::from_uncommitted_json(args.value).context(ArgName("value"))?;
        if !value.is_object() {
            return Err(anyhow::anyhow!("Value must be an Object").context(ArgName("value")));
        }
        Ok((
            args.table.parse::<TableName>().context(ArgName("table"))?,
            value,
        ))
    })?;

    system_table_guard(&table, false)?;
    let component = provider.phase.component()?;
    let tx = provider.phase.tx()?;
    let document_id = UserFacingModel::new(tx, component.into())
        .insert(table, value)
        .await?;
    let id_str = document_id.encode();
    #[allow(non_snake_case)]
    #[derive(Serialize)]
    struct InsertResult {
        _id: String,
    }
    Ok(serde_json::value::to_raw_value(&InsertResult {
        _id: id_str,
    })?)
}

#[fastrace::trace]
#[convex_macro::instrument_future]
async fn shallow_merge<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateArgs {
        #[serde(default)]
        table: Option<String>,
        id: String,
        value: JsonValue,
    }
    let table_filter = provider.table_filter();
    let component = provider.phase.component()?;

    let tx = provider.phase.tx()?;
    let (id, value, table_name) = with_argument_error("db.patch", || {
        let args: UpdateArgs = serde_json::from_value(args)?;

        let id = DeveloperDocumentId::decode(&args.id).context(ArgName("id"))?;
        let actual_table_name = tx
            .resolve_idv6(id, component.into(), table_filter)
            .context(ArgName("id"))?;
        check_table_name(&args.table, &actual_table_name)?;

        let value = PatchValue::from_uncommitted_json(args.value).context(ArgName("value"))?;
        Ok((id, value, actual_table_name))
    })?;

    system_table_guard(&table_name, false)?;

    let document = UserFacingModel::new(tx, component.into())
        .patch(id, value)
        .await?;
    Ok(serde_json::value::to_raw_value(
        &document.to_uncommitted_internal_json(),
    )?)
}

#[fastrace::trace]
#[convex_macro::instrument_future]
async fn replace<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ReplaceArgs {
        #[serde(default)]
        table: Option<String>,
        id: String,
        value: JsonValue,
    }
    let table_filter = provider.table_filter();
    let component = provider.phase.component()?;
    let tx = provider.phase.tx()?;
    let (id, value, table_name) = with_argument_error("db.replace", || {
        let args: ReplaceArgs = serde_json::from_value(args)?;

        let id = DeveloperDocumentId::decode(&args.id).context(ArgName("id"))?;
        let actual_table_name = tx
            .resolve_idv6(id, component.into(), table_filter)
            .context(ArgName("id"))?;
        check_table_name(&args.table, &actual_table_name)?;

        let value = PendingValue::from_uncommitted_json(args.value).context(ArgName("value"))?;
        if !value.is_object() {
            return Err(anyhow::anyhow!("Value must be an Object").context(ArgName("value")));
        }
        Ok((id, value, actual_table_name))
    })?;

    system_table_guard(&table_name, false)?;

    let document = UserFacingModel::new(tx, component.into())
        .replace(id, value)
        .await?;
    Ok(serde_json::value::to_raw_value(
        &document.to_uncommitted_internal_json(),
    )?)
}

#[fastrace::trace]
#[convex_macro::instrument_future]
async fn query_batch<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    batch_args: Vec<AsyncRead>,
) -> Vec<anyhow::Result<Box<RawValue>>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GetArgs {
        #[serde(default)]
        table: Option<String>,
        id: String,
        #[serde(default)]
        is_system: bool,
        #[serde(default)]
        version: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct QueryStreamNextArgs {
        query_id: u32,
    }

    let table_filter = provider.table_filter();
    let mut queries_to_fetch = BTreeMap::new();
    let mut results = BTreeMap::new();
    let batch_size = batch_args.len();
    for (idx, args) in batch_args.into_iter().enumerate() {
        let result: anyhow::Result<_> = try_anyhow!({
            match args {
                AsyncRead::QueryStreamNext(args) => {
                    let query_id = with_argument_error("queryStreamNext", || {
                        let args: QueryStreamNextArgs = serde_json::from_value(args)?;
                        Ok(args.query_id)
                    })?;
                    let managed_query = ManagedQuery::Active(
                        provider.query_manager.take_developer(query_id).context(
                            ErrorMetadata::bad_request(
                                "QueryNotFound",
                                "in-progress query not found",
                            ),
                        )?,
                    );
                    let local_query = match managed_query {
                        ManagedQuery::Pending { query, version } => {
                            let component = provider.phase.component()?;
                            DeveloperQuery::new_with_version(
                                provider.phase.tx()?,
                                component.into(),
                                query,
                                version,
                                table_filter,
                            )?
                        },
                        ManagedQuery::Active(local_query) => local_query,
                    };
                    Some((Some(query_id), local_query))
                },
                AsyncRead::Get(args) => {
                    let component = provider.phase.component()?;
                    let tx = provider.phase.tx()?;

                    let args = with_argument_error("db.get", || {
                        Ok(serde_json::from_value::<GetArgs>(args)?)
                    })?;
                    let method_name = if args.is_system {
                        "db.system.get"
                    } else {
                        "db.get"
                    };

                    let (id, is_system, version) = with_argument_error(method_name, || {
                        let id = DeveloperDocumentId::decode(&args.id).context(ArgName("id"))?;
                        let version = parse_version(args.version)?;
                        Ok((id, args.is_system, version))
                    })?;
                    let name: Result<TableName, anyhow::Error> =
                        tx.all_tables_number_to_name(component.into(), table_filter)(id.table());
                    if name.is_ok() {
                        system_table_guard(&name?, is_system)?;
                    }
                    match tx.resolve_idv6(id, component.into(), table_filter) {
                        Ok(table_name) => {
                            with_argument_error(method_name, || {
                                check_table_name(&args.table, &table_name)
                            })?;

                            let query = Query::get(table_name, id);
                            Some((
                                None,
                                DeveloperQuery::new_with_version(
                                    tx,
                                    component.into(),
                                    query,
                                    version,
                                    table_filter,
                                )?,
                            ))
                        },
                        Err(_) => {
                            // Get on a non-existent table should return
                            // null.
                            None
                        },
                    }
                },
            }
        });
        match result {
            Err(e) => {
                assert!(results.insert(idx, Err(e)).is_none());
            },
            Ok(Some((query_id, query_to_fetch))) => {
                assert!(queries_to_fetch
                    .insert(idx, (query_id, query_to_fetch))
                    .is_none());
            },
            Ok(None) => {
                assert!(results.insert(idx, Ok(RawValue::NULL.to_owned())).is_none());
            },
        }
    }

    let tx = match provider.phase.tx() {
        Ok(tx) => tx,
        Err(e) => {
            return (0..batch_size).map(|_| Err(e.clone_error())).collect_vec();
        },
    };

    let mut fetch_results = query_batch_next(
        queries_to_fetch
            .iter_mut()
            .map(|(idx, (_, local_query))| (*idx, (local_query, None)))
            .collect(),
        tx,
    )
    .await;

    #[derive(Serialize)]
    struct QueryStreamNextResult {
        value: Box<RawValue>,
        done: bool,
    }

    for (batch_key, (query_id, local_query)) in queries_to_fetch {
        let result: anyhow::Result<_> = try_anyhow!({
            if let Some(query_id) = query_id {
                provider
                    .query_manager
                    .insert_developer(query_id, local_query);
            }
            let maybe_next = fetch_results
                .remove(&batch_key)
                .context("batch_key missing")??;

            let done = maybe_next.is_none();
            let component = provider.phase.component()?;
            let tx = provider.phase.tx()?;
            let value = match maybe_next {
                Some((doc, ts)) => developer_document_to_json(tx, component.into(), &doc, ts)?,
                None => RawValue::NULL.to_owned(),
            };

            if let Some(query_id) = query_id {
                if done {
                    provider.query_manager.cleanup_developer(query_id);
                }
                serde_json::value::to_raw_value(&QueryStreamNextResult { value, done })?
            } else {
                value
            }
        });
        results.insert(batch_key, result);
    }
    assert_eq!(results.len(), batch_size);
    results.into_values().collect()
}

#[fastrace::trace]
#[convex_macro::instrument_future]
async fn remove<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RemoveArgs {
        #[serde(default)]
        table: Option<String>,
        id: String,
    }

    let table_filter = provider.table_filter();
    let component = provider.phase.component()?;
    let tx = provider.phase.tx()?;
    let (id, table_name) = with_argument_error("db.delete", || {
        let args: RemoveArgs = serde_json::from_value(args)?;
        let id = DeveloperDocumentId::decode(&args.id).context(ArgName("id"))?;
        let actual_table_name = tx
            .resolve_idv6(id, component.into(), table_filter)
            .context(ArgName("id"))?;
        check_table_name(&args.table, &actual_table_name)?;
        Ok((id, actual_table_name))
    })?;

    system_table_guard(&table_name, false)?;

    let document = UserFacingModel::new(tx, component.into())
        .delete(id)
        .await?;
    Ok(serde_json::value::to_raw_value(
        &document.to_internal_json_serializable(),
    )?)
}

#[convex_macro::instrument_future]
async fn run_udf<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
    udf_callback: impl UdfCallback<RT>,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RunUdfArgs {
        udf_type: String,
        name: Option<String>,
        reference: Option<String>,
        function_handle: Option<String>,
        args: JsonValue,
        transaction_limits: Option<TransactionLimits>,
    }
    let RunUdfArgs {
        udf_type,
        name,
        reference,
        function_handle,
        args,
        transaction_limits,
    } = with_argument_error("runUdf", || Ok(serde_json::from_value(args)?))?;
    let caller_udf_type = provider.udf_type;
    let (udf_type, args) = with_argument_error("runUdf", || {
        let udf_type: NestedUdfType = udf_type.parse().context(ArgName("udfType"))?;
        // Only a mutation can hold an unresolved commit timestamp to pass
        // along; queries keep rejecting the `$commitTs` token.
        let args = match caller_udf_type {
            UdfType::Mutation => {
                let args = PendingValue::from_uncommitted_json(args).context(ArgName("args"))?;
                if !args.is_object() {
                    return Err(anyhow::anyhow!("Value must be an Object").context(ArgName("args")));
                }
                args
            },
            UdfType::Query | UdfType::Action | UdfType::HttpAction => {
                let args: ConvexObject = ConvexValue::try_from(args)
                    .context(ArgName("args"))?
                    .try_into()
                    .context(ArgName("args"))?;
                args.into()
            },
        };
        Ok((udf_type, args))
    })?;
    let path = match function_handle {
        Some(function_handle) => {
            let handle: FunctionHandle = with_argument_error("runUdf", || function_handle.parse())?;
            let path = provider.lookup_function_handle(handle).await?;
            let tx = provider.phase.tx()?;
            let (_, component) =
                BootstrapComponentsModel::new(tx).must_component_path_to_ids(&path.component)?;
            ResolvedComponentFunctionPath {
                component,
                udf_path: path.udf_path,
                component_path: path.component,
            }
        },
        None => {
            let reference = parse_name_or_reference("runUdf", name, reference)?;
            let resource = provider.resolve(reference).await?;
            match resource {
                Resource::ResolvedSystemUdf(path) => path,
                Resource::Value(_) => {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidResource",
                        "Cannot execute a value resource"
                    ));
                },
                Resource::Function(path) => {
                    let tx = provider.phase.tx()?;
                    let (_, component) = BootstrapComponentsModel::new(tx)
                        .must_component_path_to_ids(&path.component)?;
                    ResolvedComponentFunctionPath {
                        component,
                        udf_path: path.udf_path,
                        component_path: path.component,
                    }
                },
            }
        },
    };
    let value = provider
        .run_udf(udf_type, path, args, transaction_limits, udf_callback)
        .await?;
    Ok(serde_json::value::to_raw_value(
        &value.to_uncommitted_json_serializable(),
    )?)
}

async fn create_function_handle<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateFunctionHandleArgs {
        name: Option<String>,
        function_handle: Option<String>,
        reference: Option<String>,
    }
    let CreateFunctionHandleArgs {
        name,
        function_handle,
        reference,
    } = with_argument_error("createFunctionHandle", || Ok(serde_json::from_value(args)?))?;
    let function_path = match function_handle {
        Some(function_handle) => {
            return Ok(serde_json::value::to_raw_value(&function_handle)?);
        },
        None => {
            let reference = parse_name_or_reference("createFunctionHandle", name, reference)?;
            match provider.resolve(reference).await? {
                Resource::Function(path) => path,
                Resource::ResolvedSystemUdf { .. } => {
                    anyhow::bail!("Cannot create function handle for system UDF");
                },
                Resource::Value(_) => {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        "InvalidResource",
                        "Cannot create a function handle for a value resource"
                    ));
                },
            }
        },
    };
    let handle = provider.create_function_handle(function_path).await?;
    Ok(serde_json::value::to_raw_value(&String::from(handle))?)
}

/// As pages of results are commonly returned directly from UDFs, a page should
/// be convertible to Value::Array, which has a size limit of MAX_ARRAY_LEN.
/// If there are more results than 75% of that, we recommend splitting the page.
const SOFT_MAX_PAGE_LEN: usize = soft_data_limit(8192);

#[derive(Debug, Copy, Clone)]
enum QueryPageStatus {
    SplitRequired,
    SplitRecommended,
}

impl QueryPageStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SplitRecommended => "SplitRecommended",
            Self::SplitRequired => "SplitRequired",
        }
    }
}

struct QueryPageMetadata {
    cursor: Cursor,
    split_cursor: Option<Cursor>,
    page_status: Option<QueryPageStatus>,
}

/// Serialize a queried document for JS, emitting `{"$commitTs": null}` at each
/// unresolved commit timestamp when the document is one of this transaction's
/// own staged writes.
fn developer_document_to_json<RT: Runtime>(
    tx: &mut Transaction<RT>,
    namespace: TableNamespace,
    document: &DeveloperDocument,
    ts: WriteTimestamp,
) -> anyhow::Result<Box<RawValue>> {
    if ts == WriteTimestamp::Pending {
        let id = document.id();
        // Virtual-table reads are also tagged pending when they hit a staged
        // system-table write, but virtual tables aren't in the user table
        // mapping and their documents never contain unresolved commit
        // timestamps.
        if tx
            .table_mapping()
            .namespace(namespace)
            .table_number_exists()(id.table())
        {
            let id = tx.resolve_developer_id(&id, namespace)?;
            if let Some(update) = tx.pending_write(&id)
                && !update.is_resolved()
            {
                return Ok(serde_json::value::to_raw_value(
                    &update
                        .new_document_internal_json()
                        .context("Staged write for a returned document has no new document")?,
                )?);
            }
        }
    }
    Ok(serde_json::value::to_raw_value(
        &document.to_internal_json_serializable(),
    )?)
}

async fn read_page_from_query<RT: Runtime>(
    mut query: DeveloperQuery<RT>,
    tx: &mut Transaction<RT>,
    page_size: usize,
) -> anyhow::Result<(Vec<(DeveloperDocument, WriteTimestamp)>, QueryPageMetadata)> {
    let end_cursor = query.end_cursor();
    let has_end_cursor = end_cursor.is_some();
    let mut page = Vec::with_capacity(page_size);
    let mut page_status = None;
    // If we don't have an end cursor, collect results until we hit our page size.
    // If we do have an end cursor, ignore the page size and collect everything
    while has_end_cursor || (page.len() < page_size) {
        // If we don't have an end cursor, we really have no idea
        // how many results we need to prefetch, but we can
        // use the original page size as a hint.
        let prefetch_hint = if has_end_cursor {
            Some(page_size)
        } else {
            Some(page_size - page.len())
        };

        let next_value = match query.next_with_ts(tx, prefetch_hint).await {
            Ok(Some(v)) => v,
            Ok(None) => {
                break;
            },
            Err(e) => {
                if e.is_pagination_limit() {
                    page_status = Some(QueryPageStatus::SplitRequired);
                    if query.cursor().is_none() {
                        // Intentionally drop ErrorMetadata because this should
                        // be impossible, so we want to throw a system error instead.
                        anyhow::bail!(
                            "This should be impossible. Hit pagination limit before setting query \
                             cursor: {e:?}"
                        );
                    }
                    break;
                }
                anyhow::bail!(e);
            },
        };
        page.push(next_value)
    }
    if page_status.is_none()
        && (query.is_approaching_data_limit() || page.len() > SOFT_MAX_PAGE_LEN)
    {
        page_status = Some(QueryPageStatus::SplitRecommended);
    }
    let cursor = end_cursor.or_else(|| query.cursor()).context(
        "Cursor was None. This should be impossible if `.next` was called on the query.",
    )?;
    Ok((
        page,
        QueryPageMetadata {
            cursor,
            split_cursor: query.split_cursor(),
            page_status,
        },
    ))
}

#[fastrace::trace]
#[convex_macro::instrument_future]
async fn query_page<RT: Runtime>(
    provider: &mut DatabaseUdfSyscallProvider<RT>,
    args: JsonValue,
) -> anyhow::Result<Box<RawValue>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct QueryPageArgs {
        query: JsonValue,
        cursor: Option<String>,
        end_cursor: Option<String>,
        page_size: usize,
        maximum_rows_read: Option<usize>,
        maximum_bytes_read: Option<usize>,
        #[serde(default)]
        version: Option<String>,
    }
    let args: QueryPageArgs =
        with_argument_error("queryPage", || Ok(serde_json::from_value(args)?))?;
    let parsed_query = with_argument_error("queryPage", || {
        Query::try_from(args.query).context(ArgName("query"))
    })?;
    let version = parse_version(args.version)?;
    let table_filter = provider.table_filter();

    let page_size = args.page_size;
    if page_size == 0 {
        anyhow::bail!(ErrorMetadata::bad_request(
            "NoDocumentsForPagination",
            "Must request at least 1 document while paginating"
        ));
    }
    if page_size > *TRANSACTION_MAX_READ_SIZE_ROWS {
        anyhow::bail!(ErrorMetadata::bad_request(
            "PageSizeTooLarge",
            format!("Requested too many items: {page_size}")
        ));
    }
    if args.maximum_rows_read == Some(0) || args.maximum_bytes_read == Some(0) {
        anyhow::bail!(ErrorMetadata::bad_request(
            "InvalidPaginationLimit",
            "maximumRowsRead and maximumBytesRead must be greater than 0"
        ));
    }

    let start_cursor = args
        .cursor
        .map(|c| provider.key_broker.decrypt_cursor(c))
        .transpose()?;

    let end_cursor = match args.end_cursor {
        Some(end_cursor) => Some(provider.key_broker.decrypt_cursor(end_cursor)?),
        None => provider.prev_journal.end_cursor.clone(),
    };

    let component = provider.phase.component()?;
    let tx = provider.phase.tx()?;

    let (
        page,
        QueryPageMetadata {
            cursor,
            split_cursor,
            page_status,
        },
    ) = {
        let query = DeveloperQuery::new_bounded(
            tx,
            component.into(),
            parsed_query,
            PaginationOptions::ReactivePagination {
                start_cursor,
                end_cursor,
                maximum_rows_read: args.maximum_rows_read,
                maximum_bytes_read: args.maximum_bytes_read,
            },
            version,
            table_filter,
        )?;
        let (page, metadata) = read_page_from_query(query, tx, page_size).await?;
        let page = page
            .into_iter()
            .map(|(doc, ts)| developer_document_to_json(tx, component.into(), &doc, ts))
            .collect::<anyhow::Result<_>>()?;
        (page, metadata)
    };

    let page_status = page_status.map(|s| s.as_str());

    // Place split_cursor in the middle.
    let split_cursor = split_cursor.map(|split| provider.key_broker.encrypt_cursor(&split));

    let continue_cursor = provider.key_broker.encrypt_cursor(&cursor);

    let is_done = matches!(
        cursor,
        Cursor {
            position: CursorPosition::End,
            ..
        }
    );

    if provider.udf_type == UdfType::Query {
        anyhow::ensure!(
            provider.next_journal.end_cursor.is_none(),
            ErrorMetadata::bad_request(
                "MultiplePaginatedDatabaseQueries",
                "This query function ran multiple paginated queries. Convex only supports a \
                 single paginated query in each query function.",
            )
        );
        provider.next_journal.end_cursor = Some(cursor);
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct QueryPageResult {
        page: Vec<Box<RawValue>>,
        is_done: bool,
        continue_cursor: String,
        split_cursor: Option<String>,
        page_status: Option<&'static str>,
    }
    let result = QueryPageResult {
        page,
        is_done,
        continue_cursor,
        split_cursor,
        page_status,
    };
    Ok(serde_json::value::to_raw_value(&result)?)
}
