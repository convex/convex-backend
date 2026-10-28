pub mod types;

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        LazyLock,
    },
};

use common::{
    bootstrap_model::schema::SchemaState,
    document::{
        ParseDocument,
        ParsedDocument,
        CREATION_TIME_FIELD_PATH,
    },
    runtime::Runtime,
    schemas::{
        validator::Validator,
        DatabaseSchema,
        DocumentSchema,
    },
};
use value::{
    FieldPath,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

use self::types::ValidationState;
use crate::{
    system_tables::{
        SystemIndex,
        SystemTable,
    },
    SchemaValidationMetadata,
    SchemaValidationProgress,
    SchemaValidationProgressModel,
    SystemMetadataModel,
    Transaction,
};

pub const SCHEMA_VALIDATIONS_TABLE: TableName = TableName::const_new("_schema_validations");

pub static SCHEMA_VALIDATIONS_BY_SCHEMA_ID_AND_TABLE_NAME: LazyLock<
    SystemIndex<SchemaValidationTable>,
> = LazyLock::new(|| {
    SystemIndex::new(
        "by_schema_id_and_table_name",
        [
            &SCHEMA_ID_FIELD,
            &TABLE_NAME_FIELD,
            &CREATION_TIME_FIELD_PATH,
        ],
    )
    .unwrap()
});

static SCHEMA_ID_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "schemaId".parse().expect("invalid schemaId field"));

static TABLE_NAME_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "tableName".parse().expect("invalid tableName field"));

pub struct SchemaValidationTable;

impl SystemTable for SchemaValidationTable {
    type Metadata = types::SchemaValidationMetadata;

    const TABLE_NAME: TableName = SCHEMA_VALIDATIONS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![SCHEMA_VALIDATIONS_BY_SCHEMA_ID_AND_TABLE_NAME.clone()]
    }
}

/// Worker updates apply only to the pending attempt identified by its document
/// ID.
#[derive(Clone)]
pub enum ValidationAttemptUpdate {
    StartWalk {
        total_docs: Option<u64>,
    },
    RecordProgress {
        additional_docs_validated: u64,
        total_docs: Option<u64>,
    },
    MarkValid,
    MarkFailed {
        error: String,
    },
}

pub struct SchemaValidationWithProgress {
    pub validation: SchemaValidationMetadata,
    pub progress: SchemaValidationProgress,
}

/// A validation paired with the schema validator whose hash it tracks.
pub struct StagedValidationWithProgress {
    pub validator: DocumentSchema,
    pub attempt: SchemaValidationWithProgress,
}

impl StagedValidationWithProgress {
    pub fn can_reuse_for(&self, next: &DocumentSchema) -> bool {
        match self.attempt.validation.state {
            ValidationState::Pending => self.validator == *next,
            ValidationState::Valid => {
                Validator::from(self.validator.clone()).is_subset(&Validator::from(next.clone()))
            },
            ValidationState::Failed { .. } => false,
        }
    }
}

pub struct SchemaValidationModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
    namespace: TableNamespace,
}

impl<'a, RT: Runtime> SchemaValidationModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>, namespace: TableNamespace) -> Self {
        Self { tx, namespace }
    }

    pub async fn validations_for_schema(
        &mut self,
        schema_id: ResolvedDocumentId,
    ) -> anyhow::Result<Vec<ParsedDocument<SchemaValidationMetadata>>> {
        Ok(self
            .tx
            .query_system(
                self.namespace,
                &*SCHEMA_VALIDATIONS_BY_SCHEMA_ID_AND_TABLE_NAME,
            )?
            .eq(&[schema_id.developer_id.encode_into(&mut Default::default())])?
            .all()
            .await?
            .into_iter()
            .map(|validation| (*validation).clone())
            .collect())
    }

    /// The validation for one table under `schema_id`, if any.
    pub async fn validation_metadata_for_table(
        &mut self,
        schema_id: ResolvedDocumentId,
        table_name: &TableName,
    ) -> anyhow::Result<Option<ParsedDocument<SchemaValidationMetadata>>> {
        Ok(self
            .tx
            .query_system(
                self.namespace,
                &*SCHEMA_VALIDATIONS_BY_SCHEMA_ID_AND_TABLE_NAME,
            )?
            .eq(&[
                schema_id.developer_id.encode_into(&mut Default::default()),
                table_name,
            ])?
            .unique()
            .await?
            .map(Arc::unwrap_or_clone))
    }

    /// Create (or reset) the validation for one table under `schema_id`, in
    /// state `Pending` with zero progress. Its fresh document ID fences
    /// workers holding a previous attempt's snapshot.
    pub async fn start_table_validation(
        &mut self,
        schema_id: ResolvedDocumentId,
        table_name: TableName,
        validator_hash: Option<String>,
        total_docs: Option<u64>,
    ) -> anyhow::Result<ResolvedDocumentId> {
        if let Some(existing) = self
            .validation_metadata_for_table(schema_id, &table_name)
            .await?
        {
            self.delete_attempt(existing.id()).await?;
        }
        self.insert_validation(
            schema_id,
            table_name,
            validator_hash,
            ValidationState::Pending,
            0,
            total_docs,
        )
        .await
    }

    /// Every `_schema_validations` insert goes through here. A schema has at
    /// most one validation per table: readers look the pair up with
    /// `.unique()`, and a duplicate would leave one of them unreachable.
    async fn insert_validation(
        &mut self,
        schema_id: ResolvedDocumentId,
        table_name: TableName,
        validator_hash: Option<String>,
        state: ValidationState,
        num_docs_validated: u64,
        total_docs: Option<u64>,
    ) -> anyhow::Result<ResolvedDocumentId> {
        anyhow::ensure!(
            self.validation_metadata_for_table(schema_id, &table_name)
                .await?
                .is_none(),
            "Schema {schema_id} already has a validation for table {table_name}"
        );
        let metadata = SchemaValidationMetadata {
            schema_id: schema_id.developer_id,
            table_name,
            validator_hash,
            state,
        };
        let id = SystemMetadataModel::new(self.tx, self.namespace)
            .insert(&SCHEMA_VALIDATIONS_TABLE, metadata.try_into()?)
            .await?;
        SchemaValidationProgressModel::new(self.tx, self.namespace)
            .create(id, num_docs_validated, total_docs)
            .await?;
        Ok(id)
    }

    /// The document ID binds the update to the snapshot captured by the worker.
    /// A retry replaces the validation, so stale workers return false. Counter
    /// flushes read this validation and write only progress: a committed
    /// failure cancels an in-flight flush without counter updates
    /// invalidating failure writes.
    pub async fn update_attempt(
        &mut self,
        attempt_id: ResolvedDocumentId,
        update: ValidationAttemptUpdate,
    ) -> anyhow::Result<bool> {
        let Some(doc) = self.tx.get(attempt_id).await? else {
            return Ok(false);
        };
        let validation: ParsedDocument<SchemaValidationMetadata> = doc.parse()?;
        let (id, mut metadata) = validation.into_id_and_value();
        match metadata.state {
            ValidationState::Pending => {},
            ValidationState::Valid | ValidationState::Failed { .. } => return Ok(false),
        }
        match update {
            ValidationAttemptUpdate::StartWalk { total_docs } => {
                SchemaValidationProgressModel::new(self.tx, self.namespace)
                    .reset(id, total_docs)
                    .await?;
                return Ok(true);
            },
            ValidationAttemptUpdate::RecordProgress {
                additional_docs_validated,
                total_docs,
            } => {
                SchemaValidationProgressModel::new(self.tx, self.namespace)
                    .record(id, additional_docs_validated, total_docs)
                    .await?;
                return Ok(true);
            },
            ValidationAttemptUpdate::MarkValid => metadata.state = ValidationState::Valid,
            ValidationAttemptUpdate::MarkFailed { error } => {
                metadata.state = ValidationState::Failed { error }
            },
        }
        SystemMetadataModel::new(self.tx, self.namespace)
            .replace(id, metadata.try_into()?)
            .await?;
        Ok(true)
    }

    pub async fn progress(
        &mut self,
        id: ResolvedDocumentId,
    ) -> anyhow::Result<SchemaValidationProgress> {
        Ok(SchemaValidationProgressModel::new(self.tx, self.namespace)
            .must_get(id)
            .await?
            .into_value())
    }

    pub async fn validations_with_progress(
        &mut self,
        schema_id: ResolvedDocumentId,
    ) -> anyhow::Result<Vec<SchemaValidationWithProgress>> {
        let mut result = vec![];
        for validation in self.validations_for_schema(schema_id).await? {
            let counters = self.progress(validation.id()).await?;
            result.push(SchemaValidationWithProgress {
                validation: validation.into_value(),
                progress: counters,
            });
        }
        Ok(result)
    }

    async fn delete_attempt(&mut self, id: ResolvedDocumentId) -> anyhow::Result<()> {
        SchemaValidationProgressModel::new(self.tx, self.namespace)
            .delete(id)
            .await?;
        SystemMetadataModel::new(self.tx, self.namespace)
            .delete(id)
            .await?;
        Ok(())
    }

    /// Pair staged attempts with their hash-checked validators before an
    /// outgoing schema is overwritten and its attempts are deleted.
    pub async fn staged_validations_with_progress(
        &mut self,
        schema_id: ResolvedDocumentId,
        schema: &DatabaseSchema,
    ) -> anyhow::Result<Vec<StagedValidationWithProgress>> {
        if !schema.has_staged_validators() {
            return Ok(vec![]);
        }
        let mut staged = vec![];
        for attempt in self.validations_with_progress(schema_id).await? {
            let Some(validator) = schema.staged_schema_for_table(&attempt.validation.table_name)
            else {
                continue;
            };
            if attempt.validation.validator_hash.as_deref() == Some(&validator.content_hash()?) {
                staged.push(StagedValidationWithProgress {
                    validator: validator.clone(),
                    attempt,
                });
            }
        }
        Ok(staged)
    }

    /// Reuse completed proofs for wider staged validators and pending progress
    /// for unchanged validators, preferring a finished proof, then the most
    /// progress. Failed attempts start over as `Pending`.
    pub async fn initialize_staged_validators(
        &mut self,
        schema_id: ResolvedDocumentId,
        schema: &DatabaseSchema,
        carry_over: Vec<StagedValidationWithProgress>,
    ) -> anyhow::Result<()> {
        for (table_name, table_def) in &schema.tables {
            let Some(staged_validator) = &table_def.staged_document_type else {
                continue;
            };
            let validator_hash = staged_validator.content_hash()?;
            let previous = carry_over
                .iter()
                .filter(|candidate| {
                    candidate.attempt.validation.table_name == *table_name
                        && candidate.can_reuse_for(staged_validator)
                })
                .map(|candidate| &candidate.attempt)
                .max_by_key(|candidate| {
                    (
                        matches!(candidate.validation.state, ValidationState::Valid),
                        candidate.progress.num_docs_validated,
                    )
                });
            let (state, num_docs_validated, total_docs) = match previous {
                Some(previous) => (
                    previous.validation.state.clone(),
                    previous.progress.num_docs_validated,
                    previous.progress.total_docs,
                ),
                None => (ValidationState::Pending, 0, None),
            };
            self.insert_validation(
                schema_id,
                table_name.clone(),
                Some(validator_hash),
                state,
                num_docs_validated,
                total_docs,
            )
            .await?;
        }

        Ok(())
    }

    /// Reset `Failed` staged validations for `schema_id` back to `Pending`
    /// where `schema` still declares an identical staged validator. Called
    /// when a push re-submits an unchanged schema (which never reaches
    /// `initialize_staged_validators`), so that deploy also retries failed
    /// staged validation.
    pub async fn retry_failed_staged_validators(
        &mut self,
        schema_id: ResolvedDocumentId,
        schema: &DatabaseSchema,
    ) -> anyhow::Result<()> {
        // Check the schema first so pushes without staged validators take no
        // read dependency on the validations.
        if !schema.has_staged_validators() {
            return Ok(());
        }
        for validation in self.validations_for_schema(schema_id).await? {
            if !matches!(validation.state, ValidationState::Failed { .. }) {
                continue;
            }
            let Some(staged_validator) = schema.staged_schema_for_table(&validation.table_name)
            else {
                continue;
            };
            if validation.validator_hash.as_deref() != Some(&staged_validator.content_hash()?) {
                continue;
            }
            self.start_table_validation(
                schema_id,
                validation.table_name.clone(),
                validation.validator_hash.clone(),
                None,
            )
            .await?;
        }
        Ok(())
    }

    /// Removing an active table also invalidates `v.id` checks on unchanged
    /// documents. An enforced schema rejects the deletion outright (see
    /// `DatabaseSchema::check_delete_table`), but a staged validator is not
    /// enforced, so the deletion goes through and its proof is failed here
    /// instead. The shared deletion path covers import replacements, and
    /// failing pending attempts fences workers using the old table mapping.
    /// Every schema that can hold staged validations is covered: a push still
    /// in flight has them too, and its proof would otherwise survive into a
    /// promotion.
    pub async fn invalidate_table_references(
        &mut self,
        table_name: &TableName,
    ) -> anyhow::Result<()> {
        // Orphaned-namespace cleanup deletes the system tables along with the
        // rest; once the validations table is gone there is nothing to fail.
        if !self
            .tx
            .table_mapping()
            .namespace(self.namespace)
            .name_exists(&SCHEMA_VALIDATIONS_TABLE)
        {
            return Ok(());
        }
        for state in [
            SchemaState::Active,
            SchemaState::Validated,
            SchemaState::Pending,
        ] {
            let Some((schema_id, schema)) = crate::SchemaModel::new(self.tx, self.namespace)
                .get_by_state(state)
                .await?
            else {
                continue;
            };
            for (referencing_table, table) in &schema.tables {
                if let Some(staged) = &table.staged_document_type
                    && staged
                        .foreign_keys()
                        .any(|referenced| referenced == table_name)
                {
                    self.mark_failed(
                        schema_id,
                        referencing_table,
                        format!(
                            "Table {table_name} is referenced by the staged validator for \
                             {referencing_table} but was deleted or replaced; redeploy to \
                             revalidate {referencing_table}."
                        ),
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    /// Write-time invalidation applies to both pending and valid attempts.
    /// An already-failed validation keeps its first error; a missing validation
    /// is canceled.
    pub async fn mark_failed(
        &mut self,
        schema_id: ResolvedDocumentId,
        table_name: &TableName,
        error: String,
    ) -> anyhow::Result<bool> {
        let Some(validation) = self
            .validation_metadata_for_table(schema_id, table_name)
            .await?
        else {
            return Ok(false);
        };
        let (id, mut metadata) = validation.into_id_and_value();
        match metadata.state {
            ValidationState::Failed { .. } => {},
            ValidationState::Pending | ValidationState::Valid => {
                metadata.state = ValidationState::Failed { error };
                SystemMetadataModel::new(self.tx, self.namespace)
                    .replace(id, metadata.try_into()?)
                    .await?;
            },
        }
        Ok(true)
    }

    /// The active schema (if any) together with its staged validators whose
    /// validations are `Valid`, for validation fast paths.
    pub async fn active_schema_with_valid_staged_validators(
        &mut self,
    ) -> anyhow::Result<(
        Option<Arc<DatabaseSchema>>,
        BTreeMap<TableName, DocumentSchema>,
    )> {
        let Some((active_id, active_schema)) = crate::SchemaModel::new(self.tx, self.namespace)
            .get_by_state(SchemaState::Active)
            .await?
        else {
            return Ok((None, BTreeMap::new()));
        };
        let valid = self
            .valid_staged_validators(active_id, &active_schema)
            .await?;
        Ok((Some(active_schema), valid))
    }

    /// The schema's staged validators whose validations are `Valid`
    /// (hash-checked against the schema), i.e. proven to hold for every
    /// current document and kept true by write checks. A push whose validator
    /// is a superset of one of these can skip walking the table.
    pub async fn valid_staged_validators(
        &mut self,
        schema_id: ResolvedDocumentId,
        schema: &DatabaseSchema,
    ) -> anyhow::Result<BTreeMap<TableName, DocumentSchema>> {
        let mut valid = BTreeMap::new();
        // Check the schema first so schemas without staged validators take no
        // read dependency on the validations.
        if !schema.has_staged_validators() {
            return Ok(valid);
        }
        for validation in self.validations_for_schema(schema_id).await? {
            if !matches!(validation.state, ValidationState::Valid) {
                continue;
            }
            let Some(staged_schema) = schema.staged_schema_for_table(&validation.table_name) else {
                continue;
            };
            if Some(staged_schema.content_hash()?) == validation.validator_hash {
                valid.insert(validation.table_name.clone(), staged_schema.clone());
            }
        }
        Ok(valid)
    }

    pub async fn delete_validations_for_schema(
        &mut self,
        schema_id: ResolvedDocumentId,
    ) -> anyhow::Result<()> {
        for validation in self.validations_for_schema(schema_id).await? {
            self.delete_attempt(validation.id()).await?;
        }
        Ok(())
    }

    /// Delete the enforced walk's validations (those without a validator
    /// hash), leaving staged ones in place.
    pub async fn delete_enforced_validations_for_schema(
        &mut self,
        schema_id: ResolvedDocumentId,
    ) -> anyhow::Result<()> {
        for validation in self.validations_for_schema(schema_id).await? {
            if validation.validator_hash.is_none() {
                self.delete_attempt(validation.id()).await?;
            }
        }
        Ok(())
    }
}
