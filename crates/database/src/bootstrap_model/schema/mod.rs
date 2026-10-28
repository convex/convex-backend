pub mod types;

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        LazyLock,
    },
    time::Duration,
};

use anyhow::Context;
use async_recursion::async_recursion;
use common::{
    self,
    bootstrap_model::schema::{
        SchemaMetadata,
        SchemaState,
    },
    document::ResolvedDocument,
    runtime::Runtime,
    schemas::{
        DatabaseSchema,
        SchemaValidationError,
        TableValidationOutcome,
    },
};
use errors::ErrorMetadata;
use shape_inference::{
    CountedShape,
    ProdConfig,
};
use value::{
    FieldPath,
    NamespacedTableMapping,
    ResolvedDocumentId,
    TableName,
    TableNamespace,
};

use self::types::SchemaDiff;
use crate::{
    patch_value,
    system_tables::{
        SystemIndex,
        SystemTable,
    },
    SchemaValidationModel,
    SystemMetadataModel,
    TableModel,
    TableShape,
    Transaction,
    ValidationState,
};

pub const SCHEMAS_TABLE: TableName = TableName::const_new("_schemas");

pub static SCHEMAS_STATE_INDEX: LazyLock<SystemIndex<SchemasTable>> =
    LazyLock::new(|| SystemIndex::new("by_state", [&SCHEMA_STATE_FIELD]).unwrap());

pub static SCHEMA_STATE_FIELD: LazyLock<FieldPath> =
    LazyLock::new(|| "state".parse().expect("invalid state field"));

const MAX_TIME_TO_KEEP_FAILED_AND_OVERWRITTEN_SCHEMAS: Duration = Duration::from_secs(60 * 60); // 1 hour

pub struct SchemasTable;
impl SystemTable for SchemasTable {
    type Metadata = SchemaMetadata;

    const TABLE_NAME: TableName = SCHEMAS_TABLE;

    fn indexes() -> Vec<SystemIndex<Self>> {
        vec![SCHEMAS_STATE_INDEX.clone()]
    }
}

pub struct SchemaModel<'a, RT: Runtime> {
    tx: &'a mut Transaction<RT>,
    namespace: TableNamespace,
}

impl<'a, RT: Runtime> SchemaModel<'a, RT> {
    pub fn new(tx: &'a mut Transaction<RT>, namespace: TableNamespace) -> Self {
        Self { tx, namespace }
    }

    #[fastrace::trace]
    pub async fn apply(
        &mut self,
        schema_id: Option<ResolvedDocumentId>,
    ) -> anyhow::Result<(Option<SchemaDiff>, Option<DatabaseSchema>)> {
        let previous_schema = self
            .get_by_state(SchemaState::Active)
            .await?
            .map(|(_id, schema)| schema);
        let next_schema = if let Some(schema_id) = schema_id {
            Some(
                self.get_validated_or_active(schema_id)
                    .await?
                    .database_schema()?,
            )
        } else {
            None
        };
        let schema_diff: Option<SchemaDiff> = (previous_schema.as_deref() != next_schema.as_ref())
            .then_some(SchemaDiff {
                previous_schema: previous_schema.map(Arc::unwrap_or_clone),
                next_schema: next_schema.clone(),
            });
        if let Some(schema_id) = schema_id {
            self.mark_active(schema_id).await?;
        } else {
            self.clear_active().await?;
        }

        Ok((schema_diff, next_schema))
    }

    #[fastrace::trace]
    pub async fn enforce(&mut self, document: &ResolvedDocument) -> anyhow::Result<()> {
        let schema_table_mapping = self.tx.table_mapping().namespace(self.namespace);
        if schema_table_mapping.is_system_tablet(document.id().tablet_id) {
            // System tables are not subject to schema validation.
            return Ok(());
        }
        self.enforce_with_table_mapping(document, &schema_table_mapping)
            .await
    }

    pub async fn enforce_table_deletion(
        &mut self,
        active_table_to_delete: TableName,
    ) -> anyhow::Result<()> {
        if let Some((_id, active_schema)) = self.get_by_state(SchemaState::Active).await?
            && let Err(schema_error) =
                active_schema.check_delete_table(active_table_to_delete.clone())
        {
            anyhow::bail!(schema_error.to_error_metadata());
        }
        let pending_schema = self.get_by_state(SchemaState::Pending).await?;
        let validated_schema = self.get_by_state(SchemaState::Validated).await?;
        match (pending_schema, validated_schema) {
            (None, None) => {},
            (Some((id, in_progress_schema)), None) | (None, Some((id, in_progress_schema))) => {
                if let Err(enforcement_error) =
                    in_progress_schema.check_delete_table(active_table_to_delete)
                {
                    self.mark_failed(id, enforcement_error.into()).await?;
                }
            },
            (Some(_), Some(_)) => {
                anyhow::bail!("Invalid schema state: both pending and validated schemas exist")
            },
        }

        Ok(())
    }

    /// You probably want to use `enforce`.
    /// enforce_with_table_mapping allows schema validation to use a custom
    /// TableMapping for validating foreign references, which is useful for
    /// snapshot imports where hidden tables can have foreign references to
    /// other hidden tables in the same import.
    pub async fn enforce_with_table_mapping(
        &mut self,
        document: &ResolvedDocument,
        table_mapping_for_schema: &NamespacedTableMapping,
    ) -> anyhow::Result<()> {
        let table_name = table_mapping_for_schema.tablet_name(document.id().tablet_id)?;
        if let Some((active_id, active_schema)) = self.get_by_state(SchemaState::Active).await? {
            if let Err(schema_error) = active_schema.check_new_document(
                document,
                table_name.clone(),
                table_mapping_for_schema,
                self.tx.virtual_system_mapping(),
            ) {
                anyhow::bail!(schema_error.to_error_metadata());
            }
            self.check_write_against_staged(
                active_id,
                &active_schema,
                document,
                &table_name,
                table_mapping_for_schema,
            )
            .await?;
        }
        let pending_schema = self.get_by_state(SchemaState::Pending).await?;
        let validated_schema = self.get_by_state(SchemaState::Validated).await?;
        match (pending_schema, validated_schema) {
            (None, None) => {},
            (Some((id, in_progress_schema)), None) | (None, Some((id, in_progress_schema))) => {
                if let Err(enforcement_error) = in_progress_schema.check_new_document(
                    document,
                    table_name.clone(),
                    table_mapping_for_schema,
                    self.tx.virtual_system_mapping(),
                ) {
                    self.mark_failed(id, enforcement_error.into()).await?;
                }
                // Staged validation runs from the moment a schema is
                // submitted, so its validations need the same write checks
                // as the active schema's.
                self.check_write_against_staged(
                    id,
                    &in_progress_schema,
                    document,
                    &table_name,
                    table_mapping_for_schema,
                )
                .await?;
            },
            (Some(_), Some(_)) => {
                anyhow::bail!("Invalid schema state: both pending and validated schemas exist")
            },
        }

        Ok(())
    }

    /// A staged violation fails `schema_id`'s validation for the table in the
    /// document transaction. Conforming writes avoid validation reads.
    /// Failures touch only the validation, so worker counter flushes cannot
    /// invalidate the document transaction.
    #[async_recursion]
    async fn check_write_against_staged(
        &mut self,
        schema_id: ResolvedDocumentId,
        schema: &DatabaseSchema,
        document: &ResolvedDocument,
        table_name: &TableName,
        table_mapping_for_schema: &NamespacedTableMapping,
    ) -> anyhow::Result<()> {
        let Err(enforcement_error) = schema.check_new_document_against_staged(
            document,
            table_name.clone(),
            table_mapping_for_schema,
            self.tx.virtual_system_mapping(),
        ) else {
            return Ok(());
        };
        let mut validations = SchemaValidationModel::new(self.tx, self.namespace);
        let Some(validation) = validations
            .validation_metadata_for_table(schema_id, table_name)
            .await?
        else {
            return Ok(());
        };
        if matches!(validation.state, ValidationState::Failed { .. }) {
            return Ok(());
        }
        let error: SchemaValidationError = enforcement_error.into();
        tracing::info!(
            "Write to table {table_name} violates its staged validator; marking its validation \
             failed: {error}"
        );
        validations
            .mark_failed(schema_id, table_name, error.to_string())
            .await?;
        Ok(())
    }

    pub async fn get_by_state(
        &mut self,
        state: SchemaState,
    ) -> anyhow::Result<Option<(ResolvedDocumentId, Arc<DatabaseSchema>)>> {
        anyhow::ensure!(
            state.is_unique(),
            "Getting schema by state is only permitted for Pending, Validated, or Active states, \
             since Failed or Overwritten states may have multiple documents."
        );
        self.tx.get_schema_by_state(self.namespace, state)
    }

    #[fastrace::trace]
    pub async fn submit_pending(
        &mut self,
        schema: DatabaseSchema,
    ) -> anyhow::Result<(ResolvedDocumentId, SchemaState)> {
        let active_schema = self.get_by_state(SchemaState::Active).await?;
        if schema.has_staged_validators() {
            // The enforced walk and a staged validation both record under the
            // `(schema, table)` key, so a table cannot have both. Only tables
            // that stage a validator are constrained, and only when their
            // enforced change needs a walk: a change the active validator
            // already proves goes through. Table shapes are not available
            // here, so a change only the shape would prove counts as a walk.
            let table_mapping = self.tx.table_mapping().namespace(self.namespace);
            let shape_provider =
                |table_name: &TableName| -> anyhow::Result<Option<CountedShape<ProdConfig>>> {
                    // A table that does not exist yet has nothing to walk.
                    Ok(table_mapping
                        .id(table_name)
                        .is_err()
                        .then(|| TableShape::empty().inferred_type().clone()))
                };
            // A staged validator the active schema has already proven can
            // stand in for a walk here, exactly as it does for the worker.
            let valid_staged_validators = match &active_schema {
                Some((active_id, active)) => {
                    SchemaValidationModel::new(self.tx, self.namespace)
                        .valid_staged_validators(*active_id, active)
                        .await?
                },
                None => BTreeMap::new(),
            };
            let walked_staged_tables = DatabaseSchema::table_validation_outcomes(
                &schema,
                active_schema.as_ref().map(|(_, active)| active.as_ref()),
                &table_mapping,
                self.tx.virtual_system_mapping(),
                &shape_provider,
                &valid_staged_validators,
            )?
            .into_iter()
            .filter(|(table_name, outcome)| {
                matches!(outcome, TableValidationOutcome::MustWalk)
                    && schema
                        .tables
                        .get(*table_name)
                        .is_some_and(|table| table.staged_document_type.is_some())
            })
            .map(|(table_name, _)| table_name.to_string())
            .collect::<Vec<_>>();
            if !walked_staged_tables.is_empty() {
                let walked_staged_tables = walked_staged_tables.join(", ");
                return Err(ErrorMetadata::bad_request(
                    "StagedSchemaWithEnforcedValidatorChanges",
                    format!(
                        "Cannot stage validators on tables whose enforced validator change needs \
                         their documents walked: {walked_staged_tables}. Put the whole change in \
                         the staged validator instead, so the table is walked once, in the \
                         background."
                    ),
                )
                .into());
            }
        }
        let mut table_model = TableModel::new(self.tx);
        for name in schema.tables.keys() {
            if !table_model.table_exists(self.namespace, name) {
                table_model
                    .insert_table_metadata(self.namespace, name)
                    .await?;
            }
        }
        if let Some((id, active_schema)) = &active_schema
            && **active_schema == schema
        {
            let id = *id;
            if let Some((id, _pending_schema)) = self.get_by_state(SchemaState::Pending).await? {
                self.mark_overwritten(id).await?;
            }
            if let Some((id, _validated_schema)) = self.get_by_state(SchemaState::Validated).await?
            {
                self.mark_overwritten(id).await?;
            }
            // Re-pushing the active schema retries any failed staged
            // validation: fixing the offending documents and redeploying
            // revalidates without an unstage/restage cycle.
            SchemaValidationModel::new(self.tx, self.namespace)
                .retry_failed_staged_validators(id, &schema)
                .await?;
            return Ok((id, SchemaState::Active));
        }
        // Staged validation starts with the push rather than at activation, so
        // it runs while indexes build. Compatible staged validators inherit the
        // state of the outgoing schemas' validations: the active schema's and,
        // below, an overwritten in-progress schema's. Pushes without staged
        // validators take no read dependency on the validations.
        let mut carry_over = vec![];
        if schema.has_staged_validators()
            && let Some((active_id, active)) = &active_schema
        {
            carry_over = SchemaValidationModel::new(self.tx, self.namespace)
                .staged_validations_with_progress(*active_id, active)
                .await?;
        }
        let pending_schema = self.get_by_state(SchemaState::Pending).await?;
        let validated_schema = self.get_by_state(SchemaState::Validated).await?;
        let in_progress = match (pending_schema, validated_schema) {
            (Some(_), Some(_)) => {
                anyhow::bail!("Invalid schema state: both pending and validated schemas exist")
            },
            (Some((id, existing_schema)), None) => {
                Some((id, existing_schema, SchemaState::Pending))
            },
            (None, Some((id, existing_schema))) => {
                Some((id, existing_schema, SchemaState::Validated))
            },
            (None, None) => None,
        };
        if let Some((id, existing_schema, state)) = in_progress {
            if *existing_schema == schema {
                SchemaValidationModel::new(self.tx, self.namespace)
                    .retry_failed_staged_validators(id, &schema)
                    .await?;
                return Ok((id, state));
            }
            if schema.has_staged_validators() {
                carry_over.extend(
                    SchemaValidationModel::new(self.tx, self.namespace)
                        .staged_validations_with_progress(id, &existing_schema)
                        .await?,
                );
            }
            self.mark_overwritten(id).await?;
        }

        // `SchemaMetadata::new` takes the schema; keep a copy only when there
        // are staged validators to initialize against it.
        let staged_schema = schema.has_staged_validators().then(|| schema.clone());
        let schema_metadata = SchemaMetadata::new(SchemaState::Pending, schema)?;
        let id = SystemMetadataModel::new(self.tx, self.namespace)
            .insert(&SCHEMAS_TABLE, schema_metadata.try_into()?)
            .await?;
        if let Some(staged_schema) = staged_schema {
            SchemaValidationModel::new(self.tx, self.namespace)
                .initialize_staged_validators(id, &staged_schema, carry_over)
                .await?;
        }
        Ok((id, SchemaState::Pending))
    }

    pub async fn mark_validated(&mut self, document_id: ResolvedDocumentId) -> anyhow::Result<()> {
        let doc = self
            .tx
            .get(document_id)
            .await?
            .context("Schema to mark as validated must exist.")?;
        let schema = SchemaMetadata::try_from(doc.into_value().into_value())?;
        match schema.state {
            SchemaState::Pending => {
                SystemMetadataModel::new(self.tx, self.namespace)
                    .patch(
                        document_id,
                        patch_value!("state" => Some(SchemaState::Validated.try_into()?))?,
                    )
                    .await?;
                tracing::info!("Marked pending schema as validated");
                Ok(())
            },
            SchemaState::Validated => Err(anyhow::anyhow!("Schema is already validated.")),
            SchemaState::Active => Err(anyhow::anyhow!("Schema is already active.")),
            SchemaState::Failed { error, .. } => Err(ErrorMetadata::bad_request(
                "SchemaAlreadyFailed",
                format!("Schema has already been failed with error: {error}"),
            )
            .into()),
            SchemaState::Overwritten => Err(ErrorMetadata::bad_request(
                "SchemaAlreadyOverwritten",
                "Schema has already been overwritten.",
            )
            .into()),
        }
    }

    pub async fn get_validated_or_active(
        &mut self,
        schema_id: ResolvedDocumentId,
    ) -> anyhow::Result<SchemaMetadata> {
        let doc = self
            .tx
            .get(schema_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No document found for schema ID {schema_id}"))?;
        let schema = SchemaMetadata::try_from(doc.into_value().into_value())?;
        match schema.state {
            SchemaState::Pending => {
                anyhow::bail!("Expected schema to be Validated, but it's Pending {schema_id}")
            },
            SchemaState::Validated => Ok(schema),
            SchemaState::Active => Ok(schema),
            SchemaState::Failed { error, .. } => Err(ErrorMetadata::bad_request(
                "SchemaAlreadyFailed",
                format!("Schema has already been failed with error: {error}"),
            )
            .into()),
            SchemaState::Overwritten => Err(ErrorMetadata::bad_request(
                "SchemaAlreadyOverwritten",
                "Schema has already been overwritten.",
            )
            .into()),
        }
    }

    pub async fn mark_active(&mut self, document_id: ResolvedDocumentId) -> anyhow::Result<()> {
        // Make sure it's already Validated or Active.
        let schema = self.get_validated_or_active(document_id).await?;
        match schema.state {
            // Already active: no-op
            SchemaState::Active => Ok(()),
            // If it's validated, mark as active.
            SchemaState::Validated => {
                // The enforced walk is over; its validations go. Staged
                // validations were created when the schema was submitted and
                // keep running under the same schema id.
                SchemaValidationModel::new(self.tx, self.namespace)
                    .delete_enforced_validations_for_schema(document_id)
                    .await?;
                self.clear_active().await?;
                SystemMetadataModel::new(self.tx, self.namespace)
                    .patch(
                        document_id,
                        patch_value!("state" => Some(SchemaState::Active.try_into()?))?,
                    )
                    .await?;
                Ok(())
            },
            SchemaState::Overwritten | SchemaState::Pending | SchemaState::Failed { .. } => {
                anyhow::bail!("expected validated or active schema")
            },
        }
    }

    #[async_recursion]
    /// Mark pending or validated schemas as failed. Error if the schema is
    /// already active, and do nothing if it is already overwritten or failed.
    pub async fn mark_failed(
        &mut self,
        document_id: ResolvedDocumentId,
        error: SchemaValidationError,
    ) -> anyhow::Result<()> {
        let doc = self
            .tx
            .get(document_id)
            .await?
            .context("Schema to mark as failed must exist.")?;
        let schema = SchemaMetadata::try_from(doc.into_value().into_value())?;
        match schema.state {
            SchemaState::Pending | SchemaState::Validated => {
                let error_message = error.to_string();
                let table_name = match error {
                    SchemaValidationError::ExistingDocument { table_name, .. } => table_name,
                    SchemaValidationError::NewDocument { table_name, .. } => table_name,
                    SchemaValidationError::TableCannotBeDeleted { table_name } => table_name,
                    SchemaValidationError::ReferencedTableCannotBeDeleted {
                        table_name, ..
                    } => table_name,
                };
                SystemMetadataModel::new(self.tx, self.namespace)
                    .patch(
                        document_id,
                        patch_value!(
                            "state" => Some(
                                SchemaState::Failed {
                                    error: error_message,
                                    table_name: Some(table_name.to_string())
                                }.try_into()?
                            )
                        )?,
                    )
                    .await?;
            },
            SchemaState::Active => {
                anyhow::bail!("Active schemas cannot be marked as failed.")
            },
            SchemaState::Failed { .. } | SchemaState::Overwritten => {},
        }
        self.delete_old_failed_and_overwritten_schemas().await?;
        let mut model = SchemaValidationModel::new(self.tx, self.namespace);
        model.delete_validations_for_schema(document_id).await?;
        Ok(())
    }

    pub async fn overwrite_all(&mut self) -> anyhow::Result<bool> {
        let mut is_any_schema_overwritten = false;
        for state in [
            SchemaState::Pending,
            SchemaState::Active,
            SchemaState::Validated,
        ] {
            is_any_schema_overwritten =
                self.overwrite_by_state(state).await? || is_any_schema_overwritten;
        }
        Ok(is_any_schema_overwritten)
    }

    pub async fn clear_active(&mut self) -> anyhow::Result<()> {
        self.overwrite_by_state(SchemaState::Active)
            .await
            .map(|_| ())
    }

    async fn overwrite_by_state(&mut self, state: SchemaState) -> anyhow::Result<bool> {
        if let Some((id, _schema)) = self.get_by_state(state).await? {
            self.mark_overwritten(id).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Deletes failed and overwritten schemas older than an hour, returning the
    /// number of documents deleted. Keeps schemas table small.
    async fn delete_old_failed_and_overwritten_schemas(&mut self) -> anyhow::Result<usize> {
        let mut num_deleted = 0;
        for schema_doc in self
            .tx
            .query_system(
                self.namespace,
                &SystemIndex::<SchemasTable>::by_creation_time(),
            )?
            .all()
            .await?
        {
            // Only delete failed and overwritten schemas
            match schema_doc.state {
                SchemaState::Failed { .. } | SchemaState::Overwritten => {},
                SchemaState::Active | SchemaState::Pending | SchemaState::Validated => continue,
            }
            // Break if the schemas are not old enough to be deleted
            if schema_doc.creation_time()
                > (*self
                    .tx
                    .begin_timestamp()
                    .sub(MAX_TIME_TO_KEEP_FAILED_AND_OVERWRITTEN_SCHEMAS)
                    .context("Should be able to subtract an hour from creation time")?)
                .try_into()?
            {
                break;
            }
            SystemMetadataModel::new(self.tx, self.namespace)
                .delete(schema_doc.id())
                .await?;
            num_deleted += 1;
        }
        Ok(num_deleted)
    }

    async fn mark_overwritten(&mut self, id: ResolvedDocumentId) -> anyhow::Result<()> {
        SystemMetadataModel::new(self.tx, self.namespace)
            .patch(
                id,
                patch_value!("state" => Some(SchemaState::Overwritten.try_into()?))?,
            )
            .await?;
        self.delete_old_failed_and_overwritten_schemas().await?;
        let mut model = SchemaValidationModel::new(self.tx, self.namespace);
        model.delete_validations_for_schema(id).await?;
        Ok(())
    }
}
