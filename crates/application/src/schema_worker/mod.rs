use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    num::NonZeroU64,
    sync::Arc,
    time::Duration,
};

use ::metrics::StatusTimer;
use anyhow::Context;
use common::{
    backoff::Backoff,
    bootstrap_model::schema::SchemaState,
    document::ResolvedDocument,
    errors::report_error,
    knobs::{
        SCHEMA_WORKER_PAGE_SIZE,
        SCHEMA_WORKER_WALK_CONCURRENCY,
    },
    persistence::LatestDocument,
    runtime::{
        try_join,
        Runtime,
    },
    schemas::{
        DatabaseSchema,
        DocumentSchema,
        SchemaValidationError,
        TableValidationOutcome,
    },
    types::{
        IndexRef,
        RepeatableTimestamp,
    },
    virtual_system_mapping::VirtualSystemMapping,
};
use database::{
    Database,
    IndexModel,
    SchemaModel,
    SchemaValidationModel,
    Snapshot,
    TableShape,
    TableShapes,
    Token,
    Transaction,
    ValidationAttemptUpdate,
    ValidationState,
    SCHEMAS_TABLE,
};
use errors::ErrorMetadataAnyhowExt;
use futures::{
    pin_mut,
    stream,
    Future,
    FutureExt,
    StreamExt,
    TryStreamExt,
};
use keybroker::Identity;
use metrics::{
    log_document_bytes,
    log_document_validated,
    log_walk_started,
    log_walk_ts_lag,
    schema_validation_timer,
    TableWalkMetrics,
};
use shape_inference::{
    CountedShape,
    ProdConfig,
};
use usage_tracking::FunctionUsageTracker;
use value::{
    NamespacedTableMapping,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
    TabletId,
};

use crate::metrics::log_worker_starting;

mod metrics;

const INITIAL_BACKOFF: Duration = Duration::from_millis(10);
const MAX_BACKOFF: Duration = Duration::from_secs(5);
const MAX_OCC_FAILURES: u32 = 3;
const PROGRESS_UPDATE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy)]
struct WalkConfig {
    concurrency: usize,
}

impl WalkConfig {
    fn new(concurrency: usize) -> Self {
        Self {
            concurrency: concurrency.max(1),
        }
    }
}

#[derive(Clone)]
pub struct SchemaWorker<RT: Runtime> {
    runtime: RT,
    database: Database<RT>,
    walk_config: WalkConfig,
}

pub struct PendingSchemaValidation {
    namespace: TableNamespace,
    id: ResolvedDocumentId,
    timer: StatusTimer,
    table_mapping: NamespacedTableMapping,
    virtual_system_mapping: VirtualSystemMapping,
    db_schema: Arc<DatabaseSchema>,
    active_schema: Option<Arc<DatabaseSchema>>,
    /// Staged validators of the active schema with `Valid` validation
    /// documents, usable as a fast path when validating this pending
    /// schema.
    valid_staged_validators: BTreeMap<TableName, DocumentSchema>,
    by_id_indexes: BTreeMap<TabletId, IndexRef>,
}

pub struct StagedSchemaValidation {
    namespace: TableNamespace,
    table_mapping: NamespacedTableMapping,
    virtual_system_mapping: VirtualSystemMapping,
    db_schema: Arc<DatabaseSchema>,
    /// Tables whose validation documents are `Pending`.
    pending_tables: BTreeMap<TableName, ResolvedDocumentId>,
    by_id_indexes: BTreeMap<TabletId, IndexRef>,
}

struct TableWalk {
    namespace: TableNamespace,
    table_mapping: Arc<NamespacedTableMapping>,
    /// The timestamp the validation was read at. Every page of the walk is
    /// read at or after it, so the walk observes every write that committed
    /// before the validation was read; later writes are checked against the
    /// schema in their own transactions.
    min_ts: RepeatableTimestamp,
    tablet_id: TabletId,
    by_id: IndexRef,
    validation_id: ResolvedDocumentId,
    total_docs: Option<u64>,
}

type DocumentChecker = Arc<
    dyn Fn(
            &ResolvedDocument,
            TableName,
            &NamespacedTableMapping,
        ) -> Result<(), SchemaValidationError>
        + Send
        + Sync,
>;

enum WalkOutcome {
    Complete,
    Violation(SchemaValidationError),
    Canceled,
}

pub struct SchemaValidationResult {
    pub token: Token,
    /// Tables scanned for enforced or staged validation, grouped by namespace.
    pub walked_tables: BTreeMap<TableNamespace, BTreeSet<TableName>>,
}

impl<RT: Runtime> SchemaWorker<RT> {
    pub fn start(runtime: RT, database: Database<RT>) -> impl Future<Output = ()> + Send {
        let worker = Self {
            runtime,
            database,
            walk_config: WalkConfig::new(*SCHEMA_WORKER_WALK_CONCURRENCY),
        };
        async move {
            tracing::info!("Starting SchemaWorker");
            let mut backoff = Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF);
            loop {
                let result: anyhow::Result<()> = async {
                    let SchemaValidationResult {
                        token,
                        walked_tables,
                    } = Box::pin(worker.run()).await?;
                    let num_walked: usize = walked_tables.values().map(|tables| tables.len()).sum();
                    if !walked_tables.is_empty() {
                        tracing::info!(
                            "SchemaWorker validated {} pending schema(s), walking {num_walked} \
                             table(s)",
                            walked_tables.len()
                        );
                    }
                    worker
                        .database
                        .subscribe_and_wait_for_invalidation(token)
                        .await?;
                    Ok(())
                }
                .await;
                if let Err(e) = result {
                    let delay = backoff.fail(&mut worker.runtime.rng());
                    report_error(&mut e.context("SchemaWorker died")).await;
                    tracing::error!("Schema worker failed, sleeping {delay:?}");
                    worker.runtime.wait(delay).await;
                } else {
                    backoff.reset();
                }
            }
        }
    }

    pub(crate) async fn pending_schema_validations(
        tx: &mut Transaction<RT>,
    ) -> anyhow::Result<Vec<PendingSchemaValidation>> {
        let mut pending_schema_work = Vec::new();
        let namespaces: Vec<_> = tx.table_mapping().namespaces_for_name(&SCHEMAS_TABLE);
        for namespace in namespaces {
            if let Some((id, db_schema)) = SchemaModel::new(tx, namespace)
                .get_by_state(SchemaState::Pending)
                .await?
            {
                tracing::debug!("SchemaWorker found a pending schema and is validating it...");
                let timer = schema_validation_timer();
                let table_mapping = tx.table_mapping().namespace(namespace);
                let virtual_system_mapping = tx.virtual_system_mapping().clone();

                let (active_schema, valid_staged_validators) =
                    SchemaValidationModel::new(tx, namespace)
                        .active_schema_with_valid_staged_validators()
                        .await?;
                let by_id_indexes = IndexModel::new(tx).by_id_indexes().await?;
                pending_schema_work.push(PendingSchemaValidation {
                    namespace,
                    id,
                    timer,
                    table_mapping,
                    virtual_system_mapping,
                    db_schema,
                    active_schema,
                    valid_staged_validators,
                    by_id_indexes,
                });
            }
        }
        Ok(pending_schema_work)
    }

    pub(crate) async fn staged_schema_validations(
        tx: &mut Transaction<RT>,
    ) -> anyhow::Result<Vec<StagedSchemaValidation>> {
        let mut staged_work = Vec::new();
        let namespaces: Vec<_> = tx.table_mapping().namespaces_for_name(&SCHEMAS_TABLE);
        for namespace in namespaces {
            // Staged validation starts when a schema is submitted, so a push
            // still in flight has validations to walk alongside the active
            // schema's.
            for state in [
                SchemaState::Active,
                SchemaState::Validated,
                SchemaState::Pending,
            ] {
                let Some((schema_id, db_schema)) =
                    SchemaModel::new(tx, namespace).get_by_state(state).await?
                else {
                    continue;
                };
                // Check the schema before reading validation documents so
                // deployments without staged validators take no read
                // dependency on the progress.
                if !db_schema.has_staged_validators() {
                    continue;
                }
                // A pending schema's enforced walk records its validations in
                // the same table without a validator hash; only validations
                // for the schema's current staged validators are staged work.
                let mut pending_tables: BTreeMap<TableName, ResolvedDocumentId> = BTreeMap::new();
                for validation in SchemaValidationModel::new(tx, namespace)
                    .validations_for_schema(schema_id)
                    .await?
                {
                    if !matches!(validation.state, ValidationState::Pending) {
                        continue;
                    }
                    let Some(staged_validator) =
                        db_schema.staged_schema_for_table(&validation.table_name)
                    else {
                        continue;
                    };
                    if validation.validator_hash.as_deref()
                        != Some(&staged_validator.content_hash()?)
                    {
                        continue;
                    }
                    pending_tables.insert(validation.table_name.clone(), validation.id());
                }
                if pending_tables.is_empty() {
                    continue;
                }
                staged_work.push(StagedSchemaValidation {
                    namespace,
                    table_mapping: tx.table_mapping().namespace(namespace),
                    virtual_system_mapping: tx.virtual_system_mapping().clone(),
                    db_schema,
                    pending_tables,
                    by_id_indexes: IndexModel::new(tx).by_id_indexes().await?,
                });
            }
        }
        Ok(staged_work)
    }

    pub async fn run(&self) -> anyhow::Result<SchemaValidationResult> {
        let status = log_worker_starting("SchemaWorker");
        let mut tx: Transaction<RT> = self.database.begin(Identity::system()).await?;
        let ts = tx.begin_timestamp();
        let pending_validations = SchemaWorker::pending_schema_validations(&mut tx).await?;
        let staged_validations = SchemaWorker::staged_schema_validations(&mut tx).await?;
        let token = tx.into_token()?;

        let mut walked_tables: BTreeMap<TableNamespace, BTreeSet<TableName>> = BTreeMap::new();
        if pending_validations.is_empty() && staged_validations.is_empty() {
            drop(status);
            tracing::debug!("SchemaWorker waiting...");
            return Ok(SchemaValidationResult {
                token,
                walked_tables,
            });
        }
        let snapshot = self.database.snapshot(ts)?;
        let table_shapes = self.database.table_shapes_at(ts).await?;

        for pending_validation in pending_validations {
            let outcomes = DatabaseSchema::table_validation_outcomes(
                &pending_validation.db_schema,
                pending_validation.active_schema.as_deref(),
                &pending_validation.table_mapping,
                &pending_validation.virtual_system_mapping,
                &table_shape_provider(&table_shapes, &pending_validation.table_mapping, ts),
                &pending_validation.valid_staged_validators,
            )?;
            tracing::info!(
                "SchemaWorker: table validation outcomes for {:?}: {:?}",
                pending_validation.namespace,
                outcomes,
            );
            let per_table_totals = outcomes
                .iter()
                .filter(|(_, outcome)| matches!(outcome, TableValidationOutcome::MustWalk))
                .map(|(table_name, _)| {
                    let total =
                        count_total_docs(&snapshot, table_name, pending_validation.namespace)?;
                    Ok(((*table_name).clone(), total))
                })
                .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
            walked_tables
                .entry(pending_validation.namespace)
                .or_default()
                .extend(per_table_totals.keys().cloned());
            self.validate_tables(pending_validation, ts, per_table_totals)
                .await?;
        }

        // Staged validators are validated after pending schemas: a blocked
        // push always takes priority over background staged walks.
        for staged_validation in staged_validations {
            let namespace = staged_validation.namespace;
            let walked = self
                .validate_staged_tables(staged_validation, ts, &snapshot, &table_shapes)
                .await?;
            walked_tables.entry(namespace).or_default().extend(walked);
        }

        drop(status);
        tracing::debug!("SchemaWorker waiting...");
        Ok(SchemaValidationResult {
            token,
            walked_tables,
        })
    }

    async fn validate_tables(
        &self,
        pending_validation: PendingSchemaValidation,
        // The timestamp `pending_validation` was read at.
        ts: RepeatableTimestamp,
        // The tables to walk, with each table's approximate document count.
        per_table_totals: BTreeMap<TableName, Option<u64>>,
    ) -> anyhow::Result<()> {
        let PendingSchemaValidation {
            namespace,
            id,
            timer,
            table_mapping,
            virtual_system_mapping,
            db_schema,
            active_schema: _,
            valid_staged_validators: _,
            by_id_indexes,
        } = pending_validation;

        let table_mapping = Arc::new(table_mapping.clone());
        let mut tx = self.database.begin_system().await?;
        let mut progress = SchemaValidationModel::new(&mut tx, namespace);
        let mut walks = Vec::new();
        for (table_name, total_docs) in per_table_totals {
            let tablet_id = table_mapping.name_to_tablet()(table_name.clone())?;
            let by_id = *by_id_indexes
                .get(&tablet_id)
                .context("Missing by_id index")?;
            let validation_id = progress
                .start_table_validation(id, table_name, None, total_docs)
                .await?;
            walks.push(TableWalk {
                namespace,
                table_mapping: table_mapping.clone(),
                min_ts: ts,
                tablet_id,
                by_id,
                validation_id,
                total_docs,
            });
        }
        self.database
            .commit_with_write_source(tx, "schema_validation_tracker_initialized")
            .await?;
        walks.sort_by_key(|walk| std::cmp::Reverse(walk.total_docs.unwrap_or(u64::MAX)));
        let check_document: DocumentChecker = Arc::new(move |doc, name, mapping| {
            db_schema.check_existing_document(doc, name, mapping, &virtual_system_mapping)
        });
        let worker = self.clone();
        let mut outcomes = stream::iter(walks.into_iter().map(move |walk| {
            let worker = worker.clone();
            let check_document = check_document.clone();
            async move {
                try_join("schema_worker_walk_table", async move {
                    worker.walk_table(walk, check_document).await
                })
                .await
            }
        }))
        .buffer_unordered(self.walk_config.concurrency);
        while let Some(outcome) = outcomes.try_next().await? {
            match outcome {
                WalkOutcome::Complete => {},
                WalkOutcome::Canceled => return Ok(()),
                WalkOutcome::Violation(schema_error) => {
                    self.database
                        .execute_with_occ_retries(
                            Identity::system(),
                            FunctionUsageTracker::new(),
                            MAX_OCC_FAILURES,
                            "schema_worker_mark_failed",
                            |tx| {
                                let schema_error = schema_error.clone();
                                async move {
                                    SchemaModel::new(tx, namespace)
                                        .mark_failed(id, schema_error)
                                        .await
                                }
                                .boxed()
                                .into()
                            },
                        )
                        .await?;
                    timer.finish_developer_error();
                    return Ok(());
                },
            }
        }
        let mut tx = self.database.begin(Identity::system()).await?;
        if let Err(error) = SchemaModel::new(&mut tx, namespace)
            .mark_validated(id)
            .await
        {
            if error.is_bad_request() {
                timer.finish_developer_error();
            }
            tracing::info!("Schema not marked valid");
            return Err(error);
        }
        self.database
            .commit_with_write_source(tx, "schema_worker_mark_valid")
            .await?;
        tracing::info!("Schema is valid");
        timer.finish();
        Ok(())
    }

    /// Validate the staged validators of an Active schema, one table at a
    /// time. Each table resolves independently: a violation fails only that
    /// table's validation document, and the remaining tables keep validating.
    /// Returns the tables that were actually walked.
    async fn validate_staged_tables(
        &self,
        staged_validation: StagedSchemaValidation,
        // The timestamp `staged_validation` was read at, which `snapshot` and
        // `table_shapes` must match.
        ts: RepeatableTimestamp,
        snapshot: &Snapshot,
        table_shapes: &Option<Arc<TableShapes>>,
    ) -> anyhow::Result<Vec<TableName>> {
        let StagedSchemaValidation {
            namespace,
            table_mapping,
            virtual_system_mapping,
            db_schema,
            pending_tables,
            by_id_indexes,
        } = staged_validation;
        let shape_provider = table_shape_provider(table_shapes, &table_mapping, ts);
        let mut to_walk = Vec::new();
        for (table_name, validation_id) in pending_tables {
            let Some(staged_schema) = db_schema.staged_schema_for_table(&table_name) else {
                // Documents are reconciled with the schema at activation, so a document
                // without a staged validator shouldn't happen; skip it.
                continue;
            };
            // Never-written tables have no tablet or documents, so their staged
            // validators hold vacuously.
            if !table_mapping.name_exists(&table_name) {
                self.commit_progress_write(
                    namespace,
                    validation_id,
                    ValidationAttemptUpdate::MarkValid,
                    "schema_worker_staged_valid",
                )
                .await?;
                continue;
            }
            let table_shape = shape_provider(&table_name)?;
            let outcome = DatabaseSchema::validation_outcome_for_validator(
                &table_name,
                Some(staged_schema.clone()),
                Some(&db_schema),
                &table_mapping,
                &virtual_system_mapping,
                &table_shape,
                None,
            )?;
            tracing::info!(
                "SchemaWorker: staged validator outcome for {table_name} in {namespace:?}: \
                 {outcome:?}"
            );
            if !matches!(outcome, TableValidationOutcome::MustWalk) {
                // A subset relation proves every existing document conforms.
                self.commit_progress_write(
                    namespace,
                    validation_id,
                    ValidationAttemptUpdate::MarkValid,
                    "schema_worker_staged_valid",
                )
                .await?;
                continue;
            }

            let tablet_id = table_mapping.name_to_tablet()(table_name.clone())?;
            to_walk.push((table_name, tablet_id, validation_id));
        }
        let walked: Vec<TableName> = to_walk
            .iter()
            .map(|(table_name, ..)| table_name.clone())
            .collect();
        let table_mapping = Arc::new(table_mapping.clone());
        let mut walks = Vec::new();
        for (table_name, tablet_id, validation_id) in to_walk {
            let total_docs = count_total_docs(snapshot, &table_name, namespace)?;
            if !self
                .commit_progress_write(
                    namespace,
                    validation_id,
                    ValidationAttemptUpdate::StartWalk { total_docs },
                    "schema_worker_staged_start",
                )
                .await?
            {
                continue;
            }
            let by_id = *by_id_indexes
                .get(&tablet_id)
                .context("Missing by_id index")?;
            walks.push((
                table_name,
                TableWalk {
                    namespace,
                    table_mapping: table_mapping.clone(),
                    min_ts: ts,
                    tablet_id,
                    by_id,
                    validation_id,
                    total_docs,
                },
            ));
        }
        walks.sort_by_key(|(_, walk)| std::cmp::Reverse(walk.total_docs.unwrap_or(u64::MAX)));
        let check_document: DocumentChecker = Arc::new(move |doc, name, mapping| {
            db_schema.check_existing_document_against_staged(
                doc,
                name,
                mapping,
                &virtual_system_mapping,
            )
        });
        let worker = self.clone();
        stream::iter(walks.into_iter().map(move |(table_name, walk)| {
            let worker = worker.clone();
            let check_document = check_document.clone();
            let validation_id = walk.validation_id;
            async move {
                try_join("schema_worker_walk_staged_table", async move {
                    let outcome = worker.walk_table(walk, check_document).await?;
                    match outcome {
                        WalkOutcome::Complete => {
                            tracing::info!("Staged validator for {table_name} is valid")
                        },
                        WalkOutcome::Canceled => {},
                        WalkOutcome::Violation(error) => {
                            tracing::info!("Staged validator for {table_name} is invalid: {error}");
                            worker
                                .commit_progress_write(
                                    namespace,
                                    validation_id,
                                    ValidationAttemptUpdate::MarkFailed {
                                        error: error.to_string(),
                                    },
                                    "schema_worker_staged_failed",
                                )
                                .await?;
                        },
                    }
                    anyhow::Ok(())
                })
                .await
            }
        }))
        .buffer_unordered(self.walk_config.concurrency)
        .try_collect::<Vec<_>>()
        .await?;
        Ok(walked)
    }

    /// Documents unchanged since the proposal are covered by this scan; later
    /// writes validate against the proposal in their writing transaction.
    /// A violation fails the schema or staged attempt, fencing completion.
    /// This lets pages use fresh timestamps and avoid reconstructing old
    /// documents from the log as concurrent write traffic grows.
    ///
    /// Table mappings refresh with each page to account for imports. Progress
    /// updates target the original attempt ID even if its table is renamed;
    /// pending-only updates fence cancellation and concurrent invalidation.
    async fn walk_table(
        &self,
        walk: TableWalk,
        check_document: DocumentChecker,
    ) -> anyhow::Result<WalkOutcome> {
        let TableWalk {
            namespace,
            table_mapping,
            min_ts,
            tablet_id,
            by_id,
            validation_id,
            mut total_docs,
        } = walk;
        log_walk_started();
        let update_threshold = progress_update_threshold(total_docs);
        let stream = self
            .database
            .latest_table_iterator(min_ts, *SCHEMA_WORKER_PAGE_SIZE)
            .stream_documents_in_table(tablet_id, by_id);
        pin_mut!(stream);
        let mut walk_metrics = TableWalkMetrics::start(self.runtime.clone());
        let mut last_progress_update = self.runtime.monotonic_now();
        let mut current_page_ts = None;
        let mut fresh_mapping = (*table_mapping).clone();
        let mut table_name = fresh_mapping.tablet_name(tablet_id)?;
        let mut docs_since_flush = 0;
        while let Some((LatestDocument { value: doc, .. }, page_ts)) = stream.try_next().await? {
            if current_page_ts != Some(page_ts) {
                current_page_ts = Some(page_ts);
                fresh_mapping = self
                    .database
                    .latest_snapshot()?
                    .table_mapping()
                    .namespace(namespace);
                match fresh_mapping.tablet_name(tablet_id) {
                    Ok(name) => table_name = name,
                    // Replacement documents are covered by write-time checks.
                    Err(_) => break,
                }
            }
            log_document_validated();
            log_document_bytes(doc.size());
            walk_metrics.docs += 1;
            if let Err(error) = check_document(&doc, table_name.clone(), &fresh_mapping) {
                return Ok(WalkOutcome::Violation(error));
            }
            docs_since_flush += 1;
            let now = self.runtime.monotonic_now();
            if docs_since_flush >= update_threshold.get()
                && now - last_progress_update >= PROGRESS_UPDATE_INTERVAL
            {
                if total_docs.is_none() {
                    total_docs = count_total_docs(
                        &self.database.latest_snapshot()?,
                        &table_name,
                        namespace,
                    )?;
                }
                if !self
                    .commit_progress_write(
                        namespace,
                        validation_id,
                        ValidationAttemptUpdate::RecordProgress {
                            additional_docs_validated: docs_since_flush,
                            total_docs,
                        },
                        "schema_validation_progress_updated",
                    )
                    .await?
                {
                    return Ok(WalkOutcome::Canceled);
                }
                docs_since_flush = 0;
                last_progress_update = now;
            }
        }
        if docs_since_flush > 0
            && !self
                .commit_progress_write(
                    namespace,
                    validation_id,
                    ValidationAttemptUpdate::RecordProgress {
                        additional_docs_validated: docs_since_flush,
                        total_docs,
                    },
                    "schema_validation_progress_updated",
                )
                .await?
        {
            return Ok(WalkOutcome::Canceled);
        }
        log_walk_ts_lag(Duration::from_nanos(
            (i64::from(*current_page_ts.unwrap_or(min_ts)) - i64::from(*min_ts)).max(0) as u64,
        ));
        let marked = self
            .commit_progress_write(
                namespace,
                validation_id,
                ValidationAttemptUpdate::MarkValid,
                "schema_validation_progress_finished",
            )
            .await?;
        Ok(if marked {
            WalkOutcome::Complete
        } else {
            WalkOutcome::Canceled
        })
    }

    /// Apply one progress write, retrying on OCC conflicts (the commit path
    /// and schema activations also write these documents). Returns the write's
    /// own result: false means the document is gone or the transition didn't
    /// apply.
    async fn commit_progress_write(
        &self,
        namespace: TableNamespace,
        validation_id: ResolvedDocumentId,
        update: ValidationAttemptUpdate,
        write_source: &'static str,
    ) -> anyhow::Result<bool> {
        let (_, applied, _) = self
            .database
            .execute_with_occ_retries(
                Identity::system(),
                FunctionUsageTracker::new(),
                MAX_OCC_FAILURES,
                write_source,
                |tx| {
                    let update = update.clone();
                    async move {
                        SchemaValidationModel::new(tx, namespace)
                            .update_attempt(validation_id, update)
                            .await
                    }
                    .boxed()
                    .into()
                },
            )
            .await?;
        Ok(applied)
    }
}

/// Flush progress to the table's document every 5% of the table or 500
/// documents, whichever is smaller, so progress stays fresh without slowing
/// validation down with writes.
fn progress_update_threshold(total_docs: Option<u64>) -> NonZeroU64 {
    NonZeroU64::new(
        total_docs
            .map(|total| std::cmp::min(500, (total as f64 * 0.05).ceil() as u64))
            .unwrap_or(500),
    )
    .unwrap_or(NonZeroU64::MIN)
}

/// Shape provider for [`DatabaseSchema::tables_to_validate`] and
/// [`DatabaseSchema::table_validation_outcomes`]: a table whose shape at the
/// given timestamp is already a subset of the schema being validated can skip
/// the document walk. Returning `None` means "shape unavailable" and the table
/// gets walked. `table_shapes` must be caught up to exactly `ts`, the
/// timestamp `table_mapping` is from.
pub(crate) fn table_shape_provider<'a>(
    table_shapes: &'a Option<Arc<TableShapes>>,
    table_mapping: &'a NamespacedTableMapping,
    ts: RepeatableTimestamp,
) -> impl Fn(&TableName) -> anyhow::Result<Option<CountedShape<ProdConfig>>> + 'a {
    move |table_name| {
        let Some(table_shapes) = table_shapes.as_ref() else {
            return Ok(None);
        };
        let Ok(table_id) = table_mapping.id(table_name) else {
            // Nonexistent tables have no documents to validate, so an
            // empty shape lets them skip validation.
            return Ok(Some(TableShape::empty().inferred_type().clone()));
        };
        // Every tablet in the table mapping must have a shape because the
        // shapes are caught up to exactly the mapping's timestamp.
        let shape = table_shapes
            .tablet_shape(&table_id.tablet_id)
            .with_context(|| {
                format!(
                    "table {table_name} (tablet {}) is in the table mapping at ts {} but has no \
                     shape in the table shapes at ts {}",
                    table_id.tablet_id, *ts, table_shapes.ts,
                )
            })?;
        Ok(Some(shape.inferred_type().clone()))
    }
}

/// Number of documents in the table at the snapshot, or `None` if table counts
/// haven't been bootstrapped yet.
fn count_total_docs(
    snapshot: &Snapshot,
    table_name: &TableName,
    namespace: TableNamespace,
) -> anyhow::Result<Option<u64>> {
    if snapshot.table_counts.is_none() {
        return Ok(None);
    }
    let total_docs = snapshot
        .table_count(namespace, table_name)
        .context("Failed to retrieve table count when table counts were present")?
        .num_values();
    Ok(Some(total_docs))
}
