use std::{
    collections::{
        BTreeMap,
        BTreeSet,
        HashSet,
    },
    fmt::Display,
    iter,
    marker::PhantomData,
};

use errors::ErrorMetadata;
use itertools::{
    Either,
    Itertools,
};
use serde::Serialize;
use shape_inference::{
    Shape,
    ShapeConfig,
    ShapeCounter,
};
use value::{
    id_v6::DeveloperDocumentId,
    ConvexObject,
    ConvexValue,
    IdentifierFieldName,
    Namespace,
    NamespacedTableMapping,
};

use self::validator::{
    ObjectValidator,
    ValidationError,
    Validator,
};
use crate::{
    bootstrap_model::index::{
        database_index::IndexedFields,
        index_validation_error,
        vector_index::VectorDimensions,
        MAX_TEXT_INDEX_FILTER_FIELDS_SIZE,
        MAX_VECTOR_INDEX_FILTER_FIELDS_SIZE,
    },
    document::ResolvedDocument,
    paths::FieldPath,
    types::{
        IndexDescriptor,
        TableName,
    },
    virtual_system_mapping::VirtualSystemMapping,
};

pub mod json;
pub mod validator;

pub const MAX_INDEXES_PER_TABLE: usize = 64;
#[derive(derive_more::Display, Debug, Clone, PartialEq)]
pub enum SchemaValidationError {
    #[display(
        "Document with ID \"{id}\" in table \"{table_name}\" does not match the schema: \
         {validation_error}"
    )]
    ExistingDocument {
        validation_error: ValidationError,
        table_name: TableName,
        id: DeveloperDocumentId,
    },

    // TODO: Figure out if it's possible to surface the document ID here,
    // this is a concurrent write condition
    #[display(
        "New document in table \"{table_name}\" does not match the schema: {validation_error}"
    )]
    NewDocument {
        validation_error: ValidationError,
        table_name: TableName,
    },

    #[display("Failed to delete table \"{table_name}\" because it appears in the schema")]
    TableCannotBeDeleted { table_name: TableName },
    #[display(
        "Failed to delete table \"{table_name}\" because `v.id(\"{table_name}\")` appears in the \
         schema of table \"{table_in_schema}\""
    )]
    ReferencedTableCannotBeDeleted {
        table_in_schema: TableName,
        table_name: TableName,
    },
}

#[derive(derive_more::Display, Debug, Clone, PartialEq)]
pub enum SchemaEnforcementError {
    #[display(
        "Failed to insert or update a document in table \"{table_name}\" because it does not \
         match the schema: {validation_error}"
    )]
    Document {
        validation_error: ValidationError,
        table_name: TableName,
    },
    #[display("Failed to delete table \"{table_name}\" because it appears in the schema")]
    TableCannotBeDeleted { table_name: TableName },
    #[display(
        "Failed to delete table \"{table_name}\" because `v.id(\"{table_name}\")` appears in the \
         schema of table \"{table_in_schema}\""
    )]
    ReferencedTableCannotBeDeleted {
        table_in_schema: TableName,
        table_name: TableName,
    },
}

impl SchemaEnforcementError {
    pub fn to_error_metadata(self) -> ErrorMetadata {
        ErrorMetadata::bad_request("SchemaEnforcementError", self.to_string())
    }
}

impl From<SchemaEnforcementError> for SchemaValidationError {
    fn from(value: SchemaEnforcementError) -> Self {
        match value {
            SchemaEnforcementError::Document {
                validation_error,
                table_name,
            } => Self::NewDocument {
                validation_error,
                table_name,
            },
            SchemaEnforcementError::TableCannotBeDeleted { table_name } => {
                Self::TableCannotBeDeleted { table_name }
            },
            SchemaEnforcementError::ReferencedTableCannotBeDeleted {
                table_in_schema,
                table_name,
            } => Self::ReferencedTableCannotBeDeleted {
                table_in_schema,
                table_name,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseSchema {
    pub tables: BTreeMap<TableName, TableDefinition>,
    pub schema_validation: bool,
}

#[macro_export]
/// Create a DatabaseSchema from TableNames and DocumentSchemas.
macro_rules! db_schema {
    ($($table:expr => $document_schema:expr),* $(,)?) => {
        {
            use std::collections::BTreeMap;
            #[allow(unused)]
            use $crate::types::TableName;
            use $crate::schemas::DatabaseSchema;
            #[allow(unused)]
            let mut tables = BTreeMap::new();
            {
                $(
                    let table_name: TableName = $table.to_string().parse()?;
                    let table_def = $crate::schemas::TableDefinition {
                        table_name: table_name.clone(),
                        indexes: Default::default(),
                        staged_db_indexes: Default::default(),
                        text_indexes: Default::default(),
                        staged_text_indexes: Default::default(),
                        vector_indexes: Default::default(),
                        staged_vector_indexes: Default::default(),
                        document_type: Some($document_schema),
                        staged_document_type: None,
                    };
                    tables.insert(table_name, table_def);
                )*
            }
            DatabaseSchema {
                tables,
                schema_validation: true,
            }
        }
    };
}

#[macro_export]
/// Creates a `[DatabaseSchema]` that is not validated.
macro_rules! db_schema_not_validated {
    ($($table:expr => $document_schema:expr),* $(,)?) => {
        {
            use std::collections::BTreeMap;
            #[allow(unused)]
            use $crate::types::TableName;
            use $crate::schemas::DatabaseSchema;
            #[allow(unused)]
            let mut tables = BTreeMap::new();
            {
                $(
                    let table_name: TableName = $table.to_string().parse()?;
                    let table_def = $crate::schemas::TableDefinition {
                        table_name: table_name.clone(),
                        indexes: Default::default(),
                        staged_db_indexes: Default::default(),
                        text_indexes: Default::default(),
                        staged_text_indexes: Default::default(),
                        vector_indexes: Default::default(),
                        staged_vector_indexes: Default::default(),
                        document_type: Some($document_schema),
                        staged_document_type: None,
                    };
                    tables.insert(table_name, table_def);
                )*
            }
            DatabaseSchema {
                tables,
                schema_validation: false,
            }
        }
    };
}

pub const VECTOR_DIMENSIONS: u32 = 1536;

/// Why schema validation will or won't walk a table's documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TableValidationOutcome {
    /// The new schema has `schemaValidation: false`; nothing is walked.
    NotValidated,
    /// The new validator is a superset of the enforced (active, validating)
    /// schema for this table, so all existing documents already conform.
    SupersetOfEnforced,
    /// The new validator is a superset of a staged validator the
    /// schema-validation rows have proven valid (and kept valid by checking
    /// every write since), so all existing documents already conform.
    SupersetOfStagedValidated,
    /// The new validator is a superset of the table's inferred shape.
    SupersetOfShape,
    /// Documents must be walked.
    MustWalk,
}

impl DatabaseSchema {
    pub fn tables_to_validate<'a, C: ShapeConfig, S: ShapeCounter, F>(
        new_schema: &'a DatabaseSchema,
        active_schema: Option<&DatabaseSchema>,
        table_mapping: &NamespacedTableMapping,
        virtual_system_mapping: &VirtualSystemMapping,
        shape_provider: &F,
        valid_staged_validators: &BTreeMap<TableName, DocumentSchema>,
    ) -> anyhow::Result<BTreeSet<&'a TableName>>
    where
        F: Fn(&TableName) -> anyhow::Result<Option<Shape<C, S>>>,
    {
        Ok(Self::table_validation_outcomes(
            new_schema,
            active_schema,
            table_mapping,
            virtual_system_mapping,
            shape_provider,
            valid_staged_validators,
        )?
        .into_iter()
        .filter_map(|(table_name, outcome)| {
            matches!(outcome, TableValidationOutcome::MustWalk).then_some(table_name)
        })
        .collect())
    }

    pub fn table_validation_outcomes<'a, C: ShapeConfig, S: ShapeCounter, F>(
        new_schema: &'a DatabaseSchema,
        active_schema: Option<&DatabaseSchema>,
        table_mapping: &NamespacedTableMapping,
        virtual_system_mapping: &VirtualSystemMapping,
        shape_provider: &F,
        // Must come from the active schema's `Valid` validation rows; see
        // `validation_outcome_for_validator`.
        valid_staged_validators: &BTreeMap<TableName, DocumentSchema>,
    ) -> anyhow::Result<Vec<(&'a TableName, TableValidationOutcome)>>
    where
        F: Fn(&TableName) -> anyhow::Result<Option<Shape<C, S>>>,
    {
        if !new_schema.schema_validation {
            tracing::info!("Schema validation is disabled, no tables to check");
            return Ok(new_schema
                .tables
                .keys()
                .map(|table_name| (table_name, TableValidationOutcome::NotValidated))
                .collect());
        }

        new_schema
            .tables
            .iter()
            .map(|(table_name, table_definition)| {
                let table_shape = shape_provider(table_name)?;
                Self::validation_outcome_for_validator(
                    table_name,
                    table_definition.document_type.clone(),
                    active_schema,
                    table_mapping,
                    virtual_system_mapping,
                    &table_shape,
                    valid_staged_validators.get(table_name),
                )
                .map(|outcome| (table_name, outcome))
            })
            .try_collect()
    }

    /// Whether existing documents must be walked to check they conform to
    /// `next_schema` for this table, or whether a subset relation (against the
    /// enforced validator or the table's shape) proves they already do.
    ///
    /// `valid_staged_validator` must be the table's staged validator from the
    /// active schema whose validation row is `Valid` (i.e. from
    /// `SchemaValidationModel::valid_staged_validators`): treating it
    /// as a superset proof is only sound because every document was checked
    /// against it, and every write since is checked on the commit path.
    #[allow(clippy::too_many_arguments)]
    pub fn validation_outcome_for_validator<C: ShapeConfig, S: ShapeCounter>(
        table_name: &TableName,
        next_schema: Option<DocumentSchema>,
        active_schema: Option<&DatabaseSchema>,
        table_mapping: &NamespacedTableMapping,
        virtual_system_mapping: &VirtualSystemMapping,
        table_shape: &Option<Shape<C, S>>,
        valid_staged_validator: Option<&DocumentSchema>,
    ) -> anyhow::Result<TableValidationOutcome> {
        let next_schema_validator: Validator = next_schema.into();

        // Can skip validation thanks to the schema diff?
        let enforced_schema = match active_schema {
            Some(active_schema) if active_schema.schema_validation => {
                active_schema.schema_for_table(table_name).cloned()
            },
            _ => None,
        };
        let enforced_schema_validator: Validator = enforced_schema.into();
        if enforced_schema_validator.is_subset(&next_schema_validator) {
            tracing::debug!(
                "Skipping validation for table {} because its schema is a subset of the enforced \
                 schema",
                table_name
            );
            return Ok(TableValidationOutcome::SupersetOfEnforced);
        }

        // Can skip validation thanks to a staged validator already proven valid?
        if let Some(staged_validator) = valid_staged_validator {
            let staged_validator = Validator::from(staged_validator.clone());
            if staged_validator.is_subset(&next_schema_validator) {
                tracing::debug!(
                    "Skipping validation for table {} because its schema is a superset of a \
                     validated staged validator",
                    table_name
                );
                return Ok(TableValidationOutcome::SupersetOfStagedValidated);
            }
        }

        if let Some(table_shape) = table_shape {
            // Can skip validation thanks to the saved shape?
            let validator_from_shape =
                Validator::from_shape(table_shape, table_mapping, virtual_system_mapping);
            if validator_from_shape
                .filter_top_level_system_fields()
                .is_subset(&next_schema_validator)
            {
                tracing::debug!(
                    "Skipping validation for table {} because its shape matches the schema
                     ",
                    table_name
                );
                return Ok(TableValidationOutcome::SupersetOfShape);
            }
        }

        Ok(TableValidationOutcome::MustWalk)
    }

    fn schema_for_table(&self, table_name: &TableName) -> Option<&DocumentSchema> {
        self.tables
            .get(table_name)
            .and_then(|table_definition| table_definition.document_type.as_ref())
    }

    pub fn staged_schema_for_table(&self, table_name: &TableName) -> Option<&DocumentSchema> {
        self.tables
            .get(table_name)
            .and_then(|table_definition| table_definition.staged_document_type.as_ref())
    }

    pub fn has_staged_validators(&self) -> bool {
        self.tables
            .values()
            .any(|table| table.staged_document_type.is_some())
    }

    /// Check a document against the staged validator for its table, if any.
    /// Unlike `check_value`, this ignores the `schema_validation` flag: staged
    /// validation asks whether documents satisfy the *proposed* validator,
    /// independent of whether the current schema is enforced.
    fn check_value_against_staged(
        &self,
        doc: &ResolvedDocument,
        table_name: &TableName,
        table_mapping: &NamespacedTableMapping,
        virtual_system_mapping: &VirtualSystemMapping,
    ) -> Result<(), ValidationError> {
        match self.staged_schema_for_table(table_name) {
            Some(staged_schema) => {
                staged_schema.check_value(&doc.value().0, table_mapping, virtual_system_mapping)
            },
            None => Ok(()),
        }
    }

    pub fn check_existing_document_against_staged(
        &self,
        doc: &ResolvedDocument,
        table_name: TableName,
        table_mapping: &NamespacedTableMapping,
        virtual_system_mapping: &VirtualSystemMapping,
    ) -> Result<(), SchemaValidationError> {
        self.check_value_against_staged(doc, &table_name, table_mapping, virtual_system_mapping)
            .map_err(|validation_error| SchemaValidationError::ExistingDocument {
                validation_error,
                table_name,
                id: doc.developer_id(),
            })
    }

    pub fn check_new_document_against_staged(
        &self,
        doc: &ResolvedDocument,
        table_name: TableName,
        table_mapping: &NamespacedTableMapping,
        virtual_system_mapping: &VirtualSystemMapping,
    ) -> Result<(), SchemaEnforcementError> {
        self.check_value_against_staged(doc, &table_name, table_mapping, virtual_system_mapping)
            .map_err(|validation_error| SchemaEnforcementError::Document {
                validation_error,
                table_name,
            })
    }

    fn check_value(
        &self,
        doc: &ResolvedDocument,
        table_mapping: &NamespacedTableMapping,
        virtual_system_mapping: &VirtualSystemMapping,
    ) -> Result<(), ValidationError> {
        if self.schema_validation
            && let Ok(table_name) = table_mapping.tablet_name(doc.id().tablet_id)
            && let Some(document_schema) = self.schema_for_table(&table_name)
        {
            return document_schema.check_value(
                &doc.value().0,
                table_mapping,
                virtual_system_mapping,
            );
        }
        Ok(())
    }

    pub fn check_existing_document(
        &self,
        doc: &ResolvedDocument,
        table_name: TableName,
        table_mapping: &NamespacedTableMapping,
        virtual_system_mapping: &VirtualSystemMapping,
    ) -> Result<(), SchemaValidationError> {
        self.check_value(doc, table_mapping, virtual_system_mapping)
            .map_err(|validation_error| SchemaValidationError::ExistingDocument {
                validation_error,
                table_name,
                id: doc.developer_id(),
            })
    }

    pub fn check_new_document(
        &self,
        doc: &ResolvedDocument,
        table_name: TableName,
        table_mapping: &NamespacedTableMapping,
        virtual_system_mapping: &VirtualSystemMapping,
    ) -> Result<(), SchemaEnforcementError> {
        self.check_value(doc, table_mapping, virtual_system_mapping)
            .map_err(|validation_error| SchemaEnforcementError::Document {
                validation_error,
                table_name,
            })
    }

    fn contains_table_as_reference(&self, table_name: &TableName) -> Option<TableName> {
        for table_schema in self.tables.values() {
            if let Some(document_schema) = &table_schema.document_type
                && document_schema.foreign_keys().contains(table_name)
            {
                return Some(table_schema.table_name.clone());
            }
        }
        None
    }

    pub fn check_delete_table(
        &self,
        active_table_to_delete: TableName,
    ) -> Result<(), SchemaEnforcementError> {
        if self.schema_for_table(&active_table_to_delete).is_some() {
            Err(SchemaEnforcementError::TableCannotBeDeleted {
                table_name: active_table_to_delete,
            })
        } else if let Some(table_in_schema) =
            self.contains_table_as_reference(&active_table_to_delete)
        {
            Err(SchemaEnforcementError::ReferencedTableCannotBeDeleted {
                table_in_schema,
                table_name: active_table_to_delete,
            })
        } else {
            Ok(())
        }
    }

    /// Checks whether the indexes are correctly defined (if the schema is
    /// enforced, all field names referenced by indexes must exist)
    pub fn check_index_references(&self) -> anyhow::Result<()> {
        if !self.schema_validation {
            return Ok(());
        }

        for (table_name, table_definition) in &self.tables {
            if let Some((index_descriptor, field_path)) = table_definition
                .fields_referenced_in_indexes()
                .find(|(_, field_path)| {
                    table_definition
                        .document_type
                        .as_ref()
                        .map(|document_schema| !document_schema.can_contain_field(field_path))
                        .unwrap_or(false)
                })
            {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "SchemaDefinitionError",
                    format!(
                        "In table \"{table_name}\" the index \"{index_descriptor}\" is invalid \
                         because it references the field {field_path} that does not exist.",
                    )
                ));
            }

            if let Some((index_descriptor, field_path)) =
                table_definition.vector_fields().find(|(_, vector_field)| {
                    !Self::is_vector_index_eligible(&table_definition.document_type, vector_field)
                })
            {
                anyhow::bail!(ErrorMetadata::bad_request(
                    "SchemaDefinitionError",
                    format!(
                        "In table \"{table_name}\" the vector index \"{index_descriptor}\" is \
                         invalid because it references the field {field_path} that is neither an \
                         array of float64 or optional array of float64.",
                    )
                ));
            }
        }

        Ok(())
    }

    fn is_vector_index_eligible(
        document_schema: &Option<DocumentSchema>,
        vector_field: &FieldPath,
    ) -> bool {
        let Some(document_schema) = document_schema else {
            // If there's no schema, hope the user knows what they're doing and
            // let them use the field.
            return true;
        };
        document_schema.is_vector_index_eligible(vector_field)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableDefinition {
    pub table_name: TableName,
    pub indexes: BTreeMap<IndexDescriptor, IndexSchema>,
    pub staged_db_indexes: BTreeMap<IndexDescriptor, IndexSchema>,
    pub text_indexes: BTreeMap<IndexDescriptor, TextIndexSchema>,
    pub staged_text_indexes: BTreeMap<IndexDescriptor, TextIndexSchema>,
    pub vector_indexes: BTreeMap<IndexDescriptor, VectorIndexSchema>,
    pub staged_vector_indexes: BTreeMap<IndexDescriptor, VectorIndexSchema>,
    pub document_type: Option<DocumentSchema>, /* FIXME: `Option` could be removed here, since
                                                * `None` is handled the same way as
                                                * `Some(DocumentSchema::Any)`. */
    /// The proposed next validator for this table, validated in the
    /// background while `document_type` remains the enforced validator.
    /// Valid without a `document_type`: staging a table's first validator
    /// checks existing documents while writes stay unenforced.
    pub staged_document_type: Option<DocumentSchema>,
}

impl TableDefinition {
    pub fn fields_referenced_in_indexes(
        &self,
    ) -> impl Iterator<Item = (&IndexDescriptor, &FieldPath)> {
        let index_fields = self
            .indexes
            .iter()
            .chain(self.staged_db_indexes.iter())
            .flat_map(|(index_descriptor, index_schema)| {
                index_schema
                    .fields
                    .iter()
                    .map(move |field_path| (index_descriptor, field_path))
            });

        let text_index_fields = self
            .text_indexes
            .iter()
            .chain(self.staged_text_indexes.iter())
            .map(|(index_descriptor, search_index_schema)| {
                (index_descriptor, (&search_index_schema.search_field))
            });

        let text_index_filter_fields = self
            .text_indexes
            .iter()
            .chain(self.staged_text_indexes.iter())
            .flat_map(|(index_descriptor, search_index_schema)| {
                search_index_schema
                    .filter_fields
                    .iter()
                    .map(move |field_path| (index_descriptor, field_path))
            });

        let vector_index_fields = self.vector_fields();

        index_fields
            .chain(text_index_fields)
            .chain(text_index_filter_fields)
            .chain(vector_index_fields)
    }

    pub fn vector_fields(&self) -> impl Iterator<Item = (&IndexDescriptor, &FieldPath)> {
        self.vector_indexes
            .iter()
            .chain(self.staged_vector_indexes.iter())
            .map(|(index_descriptor, vector_index_schema)| {
                (index_descriptor, (&vector_index_schema.vector_field))
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexSchema {
    pub index_descriptor: IndexDescriptor,
    pub fields: IndexedFields,
}

impl Display for IndexSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.index_descriptor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextIndexSchema {
    pub index_descriptor: IndexDescriptor,
    pub search_field: FieldPath,
    pub filter_fields: BTreeSet<FieldPath>,

    // Private field to force all creations to go through the constructor.
    _pd: PhantomData<()>,
}

impl TextIndexSchema {
    pub fn new(
        index_descriptor: IndexDescriptor,
        search_field: FieldPath,
        filter_fields: BTreeSet<FieldPath>,
    ) -> anyhow::Result<Self> {
        if filter_fields.len() > MAX_TEXT_INDEX_FILTER_FIELDS_SIZE {
            anyhow::bail!(index_validation_error::too_many_filter_fields(
                MAX_TEXT_INDEX_FILTER_FIELDS_SIZE
            ));
        }
        Ok(Self {
            index_descriptor,
            search_field,
            filter_fields,
            _pd: PhantomData,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorIndexSchema {
    pub index_descriptor: IndexDescriptor,
    pub vector_field: FieldPath,
    pub dimension: VectorDimensions,
    pub filter_fields: BTreeSet<FieldPath>,

    // Private field to force all creations to go through the constructor.
    _pd: PhantomData<()>,
}

impl VectorIndexSchema {
    pub fn new(
        index_descriptor: IndexDescriptor,
        vector_field: FieldPath,
        dimension: VectorDimensions,
        filter_fields: BTreeSet<FieldPath>,
    ) -> anyhow::Result<Self> {
        if filter_fields.len() > MAX_VECTOR_INDEX_FILTER_FIELDS_SIZE {
            anyhow::bail!(index_validation_error::too_many_filter_fields(
                MAX_VECTOR_INDEX_FILTER_FIELDS_SIZE
            ));
        }
        Ok(Self {
            index_descriptor,
            vector_field,
            dimension,
            filter_fields,
            _pd: PhantomData,
        })
    }
}

/// [`DocumentSchema`] corresponds to the `DocumentSchema` TS type in
/// `TableDefinition`. `Any` means no schema will be enforced.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DocumentSchema {
    Any,

    Union(Vec<ObjectValidator>),
}

impl DocumentSchema {
    fn check_value(
        &self,
        value: &ConvexObject,
        table_mapping: &NamespacedTableMapping,
        virtual_system_mapping: &VirtualSystemMapping,
    ) -> Result<(), ValidationError> {
        match self {
            DocumentSchema::Any => {},
            DocumentSchema::Union(t) => {
                let value = value.clone().filter_system_fields();
                let schema_type = t
                    .iter()
                    .map(|obj_schema| Validator::Object(obj_schema.clone()))
                    .collect();
                Validator::Union(schema_type).check_value(
                    &ConvexValue::Object(value),
                    table_mapping,
                    virtual_system_mapping,
                )?;
            },
        }
        Ok(())
    }

    /// Returns `true` when it is sometimes possible to have a field with the
    /// given path on the document if this table definition is enforced, or
    /// `false` when it is never possible.
    pub fn can_contain_field(&self, field_path: &FieldPath) -> bool {
        // Allow system fields even if they are not in the table definition
        if matches!(&field_path.fields(), [single_field] if single_field.is_system()) {
            return true;
        }

        match &self {
            DocumentSchema::Any => true,
            DocumentSchema::Union(validators) => validators.iter().any(|root_validator| {
                Validator::Object(root_validator.clone()).can_contain_field(field_path)
            }),
        }
    }

    pub fn has_validator_for_system_field(&self) -> bool {
        match &self {
            DocumentSchema::Any => false,
            DocumentSchema::Union(validators) => validators
                .iter()
                .any(|root_validator| root_validator.has_validator_for_system_field()),
        }
    }

    pub fn is_vector_index_eligible(&self, field_path: &FieldPath) -> bool {
        match &self {
            DocumentSchema::Any => true,
            DocumentSchema::Union(validators) => validators.iter().any(|root_validator| {
                Validator::Object(root_validator.clone()).overlaps_with_array_float64(field_path)
            }),
        }
    }

    /// Returns the field names from top level objects in the schema that are
    /// optional.
    pub fn optional_top_level_fields(&self) -> HashSet<IdentifierFieldName> {
        match self {
            DocumentSchema::Any => HashSet::default(),
            DocumentSchema::Union(validators) => validators
                .iter()
                .flat_map(|validator| {
                    validator
                        .0
                        .iter()
                        .filter_map(|(field_name, field_validator)| {
                            if field_validator.optional {
                                Some(field_name.clone())
                            } else {
                                None
                            }
                        })
                })
                .collect(),
        }
    }

    pub fn foreign_keys(&self) -> impl Iterator<Item = &TableName> {
        match self {
            Self::Any => Either::Left(iter::empty()),
            Self::Union(options) => {
                Either::Right(options.iter().flat_map(|option| option.foreign_keys()))
            },
        }
    }
}

const SEE_SCHEMA_DOCS: &str =
    "To learn more, see the schema documentation at https://docs.convex.dev/database/schemas.";

fn invalid_top_level_type_in_schema(validator: &Validator) -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "InvalidTopLevelTypeInSchemaError",
        format!(
            "The document validator in a schema must be an object, a union of objects, or \
             `v.any()`. Found {validator}. {SEE_SCHEMA_DOCS}"
        ),
    )
}

pub fn missing_schema_export_error() -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "MissingSchemaExportError",
        format!("Schema file missing default export. {SEE_SCHEMA_DOCS}"),
    )
}

pub fn invalid_schema_export_error() -> ErrorMetadata {
    ErrorMetadata::bad_request(
        "InvalidSchemaExport",
        format!("Default export from schema file isn't a Convex schema. {SEE_SCHEMA_DOCS}"),
    )
}
