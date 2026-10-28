use std::collections::{
    BTreeMap,
    BTreeSet,
    HashMap,
    HashSet,
};

use anyhow::Context;
use errors::{
    ErrorMetadata,
    ErrorMetadataAnyhowExt,
};
use itertools::Itertools;
use json_trait::JsonForm;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as JsonValue;
use value::{
    ConvexValue,
    FieldPath,
    IdentifierFieldName,
    TableName,
};

use super::{
    validator::{
        FieldValidator,
        FunctionHandleType,
        FunctionValidator,
        LiteralValidator,
        ObjectValidator,
        Validator,
    },
    DatabaseSchema,
    DocumentSchema,
    IndexSchema,
    VectorIndexSchema,
};
use crate::{
    bootstrap_model::index::{
        index_validation_error::{
            self,
            index_not_unique,
            search_field_not_unique,
            vector_field_not_unique,
        },
        vector_index::VectorDimensions,
    },
    schemas::{
        invalid_top_level_type_in_schema,
        TableDefinition,
        TextIndexSchema,
        MAX_INDEXES_PER_TABLE,
    },
    types::{
        IndexDescriptor,
        IndexName,
    },
};

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseSchemaJson {
    tables: Vec<TableDefinitionJson>,
    schema_validation: Option<bool>,
}

impl JsonForm for DatabaseSchema {
    type Json = DatabaseSchemaJson;
}

impl TryFrom<DatabaseSchemaJson> for DatabaseSchema {
    type Error = anyhow::Error;

    fn try_from(j: DatabaseSchemaJson) -> Result<Self, Self::Error> {
        let tables = j
            .tables
            .into_iter()
            .map(|v| {
                let s = TableDefinition::try_from(v)?;
                Ok((s.table_name.clone(), s))
            })
            .collect::<anyhow::Result<BTreeMap<_, _>>>()?;

        // Schemas written before schema validation was introduced don't include
        // this. Default to false.
        let schema_validation = j.schema_validation.unwrap_or(false);
        Ok(DatabaseSchema {
            tables,
            schema_validation,
        })
    }
}

impl TryFrom<DatabaseSchema> for DatabaseSchemaJson {
    type Error = anyhow::Error;

    fn try_from(
        DatabaseSchema {
            tables,
            schema_validation,
        }: DatabaseSchema,
    ) -> anyhow::Result<Self> {
        Ok(DatabaseSchemaJson {
            tables: tables
                .into_values()
                .map(TableDefinitionJson::try_from)
                .collect::<anyhow::Result<Vec<_>>>()?,
            schema_validation: Some(schema_validation),
        })
    }
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TableDefinitionJson {
    table_name: String,
    indexes: Vec<IndexSchemaJson>,
    staged_db_indexes: Option<Vec<IndexSchemaJson>>,
    search_indexes: Option<Vec<TextIndexSchemaJson>>,
    staged_search_indexes: Option<Vec<TextIndexSchemaJson>>,
    vector_indexes: Option<Vec<VectorIndexSchemaJson>>,
    staged_vector_indexes: Option<Vec<VectorIndexSchemaJson>>,
    document_type: Option<ValidatorJson>,
    staged_document_type: Option<ValidatorJson>,
}

impl JsonForm for TableDefinition {
    type Json = TableDefinitionJson;
}

// Collect the index names separately from the deduplicating map so that we can
// complain complain about duplicate names
fn parse_names_and_indexes<T: TryFrom<U, Error = anyhow::Error>, U>(
    table_name: &TableName,
    indexes: Vec<U>,
    descriptor: impl Fn(&T) -> &IndexDescriptor,
) -> anyhow::Result<(Vec<IndexDescriptor>, BTreeMap<IndexDescriptor, T>)> {
    itertools::process_results(
        indexes.into_iter().map(T::try_from).map_ok(|idx| {
            let index_name = descriptor(&idx);
            (index_name.clone(), (index_name.clone(), idx))
        }),
        |iter| iter.unzip(),
    )
    .map_err(|e: anyhow::Error| e.wrap_error_message(|s| format!("In table \"{table_name}\": {s}")))
}

fn validate_unique_index_fields<'a, T: 'a, Y: Clone + Eq + std::hash::Hash>(
    indexes: impl Iterator<Item = (&'a IndexDescriptor, &'a T)>,
    unique_index_field: impl Fn(&T) -> Y,
    non_unique_error: impl Fn(&IndexDescriptor, &IndexDescriptor) -> ErrorMetadata,
) -> anyhow::Result<()> {
    let index_fields: BTreeMap<_, _> = indexes
        .map(|(name, fields)| (name, unique_index_field(fields)))
        .collect();

    let mut seen: HashMap<_, &IndexDescriptor> = HashMap::new();
    for (name, fields) in index_fields.into_iter() {
        if let Some(other_name) = seen.insert(fields, name) {
            anyhow::bail!(non_unique_error(name, other_name));
        }
    }
    Ok(())
}

impl TryFrom<TableDefinitionJson> for TableDefinition {
    type Error = anyhow::Error;

    fn try_from(j: TableDefinitionJson) -> Result<Self, Self::Error> {
        let staged_db_indexes = j.staged_db_indexes.unwrap_or_default();
        let text_indexes = j.search_indexes.unwrap_or_default();
        let staged_text_indexes = j.staged_search_indexes.unwrap_or_default();
        let vector_indexes = j.vector_indexes.unwrap_or_default();
        let staged_vector_indexes = j.staged_vector_indexes.unwrap_or_default();

        let document_type = j.document_type.map(|t| t.try_into()).transpose()?;
        let staged_document_type = j.staged_document_type.map(|t| t.try_into()).transpose()?;

        let table_name: TableName = j
            .table_name
            .parse()
            .with_context(|| index_validation_error::invalid_table_name(&j.table_name))?;
        anyhow::ensure!(
            !table_name.is_system(),
            index_validation_error::table_name_reserved(&table_name)
        );

        if j.indexes.len()
            + vector_indexes.len()
            + text_indexes.len()
            + staged_db_indexes.len()
            + staged_text_indexes.len()
            + staged_vector_indexes.len()
            > MAX_INDEXES_PER_TABLE
        {
            anyhow::bail!(index_validation_error::too_many_indexes(
                &table_name,
                MAX_INDEXES_PER_TABLE
            ));
        }

        let (index_names, indexes) =
            parse_names_and_indexes(&table_name, j.indexes, |idx: &IndexSchema| {
                &idx.index_descriptor
            })?;
        for schema in indexes.values() {
            if schema.fields.is_empty() {
                anyhow::bail!(index_validation_error::empty_index(
                    &table_name,
                    schema,
                    false
                ));
            }
        }
        let (staged_db_index_names, staged_db_indexes) =
            parse_names_and_indexes(&table_name, staged_db_indexes, |idx: &IndexSchema| {
                &idx.index_descriptor
            })?;
        for schema in staged_db_indexes.values() {
            if schema.fields.is_empty() {
                anyhow::bail!(index_validation_error::empty_index(
                    &table_name,
                    schema,
                    true
                ));
            }
        }
        validate_unique_index_fields(
            indexes.iter().chain(staged_db_indexes.iter()),
            |idx: &IndexSchema| Vec::<FieldPath>::from(idx.fields.clone()),
            |index1, index2| index_not_unique(&table_name, index1, index2),
        )?;

        let (text_index_names, text_indexes) =
            parse_names_and_indexes(&table_name, text_indexes, |idx: &TextIndexSchema| {
                &idx.index_descriptor
            })?;
        let (staged_text_index_names, staged_text_indexes) =
            parse_names_and_indexes(&table_name, staged_text_indexes, |idx: &TextIndexSchema| {
                &idx.index_descriptor
            })?;
        validate_unique_index_fields(
            text_indexes.iter().chain(staged_text_indexes.iter()),
            |idx: &TextIndexSchema| (idx.search_field.clone(), idx.filter_fields.clone()),
            |index1, index2| search_field_not_unique(&table_name, index1, index2),
        )?;

        let (vector_index_names, vector_indexes): (Vec<_>, BTreeMap<_, _>) =
            parse_names_and_indexes(&table_name, vector_indexes, |idx: &VectorIndexSchema| {
                &idx.index_descriptor
            })?;
        let (staged_vector_index_names, staged_vector_indexes): (Vec<_>, BTreeMap<_, _>) =
            parse_names_and_indexes(
                &table_name,
                staged_vector_indexes,
                |idx: &VectorIndexSchema| &idx.index_descriptor,
            )?;
        validate_unique_index_fields(
            vector_indexes.iter().chain(staged_vector_indexes.iter()),
            |idx: &VectorIndexSchema| (idx.vector_field.clone(), idx.dimension),
            |index1, index2| vector_field_not_unique(&table_name, index1, index2),
        )?;

        let all_index_names: Vec<_> = index_names
            .into_iter()
            .chain(staged_db_index_names)
            .chain(text_index_names)
            .chain(staged_text_index_names)
            .chain(staged_vector_index_names)
            .chain(vector_index_names)
            .collect();

        let mut seen: HashSet<_> = HashSet::new();
        for index_name in all_index_names.into_iter() {
            // Validate the name
            if index_name.starts_with("_fivetran") {
                // Allow fivetran system fields to be used as index names
                IndexName::new_reserved(table_name.clone(), index_name.clone())?;
            } else {
                IndexName::new(table_name.clone(), index_name.clone())?;
            }

            if !seen.insert(index_name.clone()) {
                anyhow::bail!(index_validation_error::names_not_unique(
                    &table_name,
                    &index_name
                ));
            }
        }

        Ok(Self {
            table_name,
            indexes,
            staged_db_indexes,
            text_indexes,
            staged_text_indexes,
            vector_indexes,
            staged_vector_indexes,
            document_type,
            staged_document_type,
        })
    }
}

impl TryFrom<TableDefinition> for TableDefinitionJson {
    type Error = anyhow::Error;

    fn try_from(
        TableDefinition {
            table_name,
            indexes,
            staged_db_indexes,
            text_indexes: search_indexes,
            staged_text_indexes: staged_search_indexes,
            vector_indexes,
            staged_vector_indexes,
            document_type,
            staged_document_type,
        }: TableDefinition,
    ) -> anyhow::Result<Self> {
        let table_name = String::from(table_name);
        let indexes = indexes
            .into_values()
            .map(IndexSchemaJson::try_from)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let staged_db_indexes = Some(
            staged_db_indexes
                .into_values()
                .map(IndexSchemaJson::try_from)
                .collect::<anyhow::Result<Vec<_>>>()?,
        );
        let search_indexes = Some(
            search_indexes
                .into_values()
                .map(TextIndexSchemaJson::try_from)
                .collect::<anyhow::Result<Vec<_>>>()?,
        );
        let staged_search_indexes = Some(
            staged_search_indexes
                .into_values()
                .map(TextIndexSchemaJson::try_from)
                .collect::<anyhow::Result<Vec<_>>>()?,
        );
        let document_type = document_type.map(ValidatorJson::try_from).transpose()?;
        let staged_document_type = staged_document_type
            .map(ValidatorJson::try_from)
            .transpose()?;
        let vector_indexes = Some(
            vector_indexes
                .into_values()
                .map(VectorIndexSchemaJson::try_from)
                .collect::<anyhow::Result<Vec<_>>>()?,
        );
        let staged_vector_indexes = Some(
            staged_vector_indexes
                .into_values()
                .map(VectorIndexSchemaJson::try_from)
                .collect::<anyhow::Result<Vec<_>>>()?,
        );
        Ok(TableDefinitionJson {
            table_name,
            indexes,
            staged_db_indexes,
            search_indexes,
            staged_search_indexes,
            vector_indexes,
            staged_vector_indexes,
            document_type,
            staged_document_type,
        })
    }
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IndexSchemaJson {
    index_descriptor: String,
    fields: Vec<String>,
}

impl JsonForm for IndexSchema {
    type Json = IndexSchemaJson;
}

impl TryFrom<IndexSchemaJson> for IndexSchema {
    type Error = anyhow::Error;

    fn try_from(j: IndexSchemaJson) -> Result<Self, Self::Error> {
        let index_descriptor = IndexDescriptor::new(j.index_descriptor)?;
        let fields = j
            .fields
            .into_iter()
            .map(|p| {
                p.parse().with_context(|| {
                    index_validation_error::invalid_index_field(&index_descriptor, &p)
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?
            .try_into()
            .map_err(|e: anyhow::Error| {
                e.wrap_error_message(|s| format!("In index \"{index_descriptor}\": {s}"))
            })?;
        Ok(Self {
            index_descriptor,
            fields,
        })
    }
}

impl TryFrom<IndexSchema> for IndexSchemaJson {
    type Error = anyhow::Error;

    fn try_from(
        IndexSchema {
            index_descriptor,
            fields,
        }: IndexSchema,
    ) -> anyhow::Result<Self> {
        Ok(IndexSchemaJson {
            index_descriptor: String::from(index_descriptor),
            fields: Vec::<FieldPath>::from(fields)
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        })
    }
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VectorIndexSchemaJson {
    index_descriptor: String,
    vector_field: String,
    dimensions: Option<u32>,
    dimension: Option<u32>,
    filter_fields: Vec<String>,
}

impl JsonForm for VectorIndexSchema {
    type Json = VectorIndexSchemaJson;
}

impl TryFrom<VectorIndexSchemaJson> for VectorIndexSchema {
    type Error = anyhow::Error;

    fn try_from(j: VectorIndexSchemaJson) -> Result<Self, Self::Error> {
        let index_descriptor = IndexDescriptor::new(j.index_descriptor)?;
        let vector_field = j.vector_field.parse().with_context(|| {
            index_validation_error::invalid_index_field(&index_descriptor, &j.vector_field)
        })?;
        let filter_fields = j
            .filter_fields
            .into_iter()
            .map(|f| {
                f.parse().with_context(|| {
                    index_validation_error::invalid_index_field(&index_descriptor, &f)
                })
            })
            .collect::<anyhow::Result<BTreeSet<_>>>()?;
        let dimension: VectorDimensions = match j.dimensions {
            Some(d) => d.try_into()?,
            // Support legacy alpha users
            None => match j.dimension {
                Some(d) => d.try_into()?,
                None => anyhow::bail!("Missing dimensions field"),
            },
        };
        Self::new(index_descriptor, vector_field, dimension, filter_fields)
    }
}

impl TryFrom<VectorIndexSchema> for VectorIndexSchemaJson {
    type Error = anyhow::Error;

    fn try_from(
        VectorIndexSchema {
            index_descriptor,
            vector_field,
            dimension,
            filter_fields,
            ..
        }: VectorIndexSchema,
    ) -> anyhow::Result<Self> {
        Ok(VectorIndexSchemaJson {
            index_descriptor: String::from(index_descriptor),
            vector_field: String::from(vector_field),
            dimensions: Some(dimension.into()),
            dimension: None,
            filter_fields: filter_fields
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        })
    }
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TextIndexSchemaJson {
    index_descriptor: String,
    search_field: String,
    filter_fields: BTreeSet<String>,
}

impl JsonForm for TextIndexSchema {
    type Json = TextIndexSchemaJson;
}

impl TryFrom<TextIndexSchemaJson> for TextIndexSchema {
    type Error = anyhow::Error;

    fn try_from(j: TextIndexSchemaJson) -> Result<Self, Self::Error> {
        let index_descriptor = IndexDescriptor::new(j.index_descriptor)?;
        let search_field = j.search_field.parse().with_context(|| {
            index_validation_error::invalid_index_field(&index_descriptor, &j.search_field)
        })?;
        let filter_fields = j
            .filter_fields
            .into_iter()
            .map(|f| {
                f.parse().with_context(|| {
                    index_validation_error::invalid_index_field(&index_descriptor, &f)
                })
            })
            .collect::<anyhow::Result<BTreeSet<_>>>()?;

        Self::new(index_descriptor, search_field, filter_fields)
    }
}

impl TryFrom<TextIndexSchema> for TextIndexSchemaJson {
    type Error = anyhow::Error;

    fn try_from(
        TextIndexSchema {
            index_descriptor,
            search_field,
            filter_fields,
            ..
        }: TextIndexSchema,
    ) -> anyhow::Result<Self> {
        Ok(TextIndexSchemaJson {
            index_descriptor: index_descriptor.to_string(),
            search_field: String::from(search_field),
            filter_fields: filter_fields
                .into_iter()
                .map(String::from)
                .collect::<BTreeSet<_>>(),
        })
    }
}

impl TryFrom<ValidatorJson> for DocumentSchema {
    type Error = anyhow::Error;

    fn try_from(json: ValidatorJson) -> Result<Self, Self::Error> {
        let schema_type = Validator::try_from(json)?;
        match schema_type {
            Validator::Any => Ok(DocumentSchema::Any),
            Validator::Union(value) => {
                let schemas: Vec<_> = value
                    .into_iter()
                    .map(|s| {
                        if let Validator::Object(object_schema) = s {
                            // TODO(sarah) Change this to error on system fields at the top level
                            // once data has been migrated
                            Ok(object_schema.filter_system_fields())
                        } else {
                            Err(anyhow::anyhow!(invalid_top_level_type_in_schema(&s)))
                        }
                    })
                    .collect::<anyhow::Result<_>>()?;
                Ok(DocumentSchema::Union(schemas))
            },
            Validator::Object(object_schema) => {
                // TODO(sarah) Change this to error on system fields at the top level
                // once data has been migrated
                let filtered_schema = object_schema.filter_system_fields();
                Ok(DocumentSchema::Union(vec![filtered_schema]))
            },
            _ => Err(anyhow::anyhow!(invalid_top_level_type_in_schema(
                &schema_type
            ))),
        }
    }
}

impl DocumentSchema {
    /// Stable content hash used to carry validation state into fresh attempts.
    ///
    /// Hashes the canonical JSON serialization: object fields are sorted
    /// (`BTreeMap`), and union member order is significant — reordering union
    /// members counts as a different validator.
    pub fn content_hash(&self) -> anyhow::Result<String> {
        let json = ValidatorJson::try_from(self.clone())?;
        let bytes = serde_json::to_vec(&json)?;
        Ok(value::sha256::Sha256::hash(&bytes).as_hex())
    }
}

impl TryFrom<DocumentSchema> for ValidatorJson {
    type Error = anyhow::Error;

    fn try_from(d: DocumentSchema) -> anyhow::Result<ValidatorJson> {
        match d {
            DocumentSchema::Any => ValidatorJson::try_from(Validator::Any),
            DocumentSchema::Union(mut object_schemas) => {
                if object_schemas.len() == 1 {
                    let single_schema = object_schemas.pop().unwrap();
                    ValidatorJson::try_from(Validator::Object(single_schema))
                } else {
                    ValidatorJson::try_from(Validator::Union(
                        object_schemas.into_iter().map(Validator::Object).collect(),
                    ))
                }
            },
        }
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldTypeJson {
    field_type: ValidatorJson,
    optional: bool,
}

impl JsonForm for FieldValidator {
    type Json = FieldTypeJson;
}

impl TryFrom<FieldTypeJson> for FieldValidator {
    type Error = anyhow::Error;

    fn try_from(field_type_json: FieldTypeJson) -> anyhow::Result<Self> {
        Ok(FieldValidator {
            validator: field_type_json.field_type.try_into()?,
            optional: field_type_json.optional,
        })
    }
}

impl TryFrom<FieldValidator> for FieldTypeJson {
    type Error = anyhow::Error;

    fn try_from(f: FieldValidator) -> anyhow::Result<FieldTypeJson> {
        Ok(FieldTypeJson {
            field_type: ValidatorJson::try_from(f.validator)?,
            optional: f.optional,
        })
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum ValidatorJson {
    Null,
    Number,
    Bigint,
    CommitTs,
    Boolean,
    String,
    Bytes,
    #[serde(rename_all = "camelCase")]
    Function {
        #[serde(skip_serializing_if = "Option::is_none")]
        fn_type: Option<FunctionHandleType>,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<Box<ValidatorJson>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        returns: Option<Box<ValidatorJson>>,
    },
    #[serde(alias = "map")]
    #[serde(alias = "set")]
    Any,
    Literal {
        value: JsonValue,
    },
    #[serde(rename_all = "camelCase")]
    Id {
        table_name: String,
    },
    Array {
        value: Box<ValidatorJson>,
    },
    Record {
        keys: Box<ValidatorJson>,
        values: Box<FieldTypeJson>,
    },
    Object {
        value: BTreeMap<String, FieldTypeJson>,
    },
    Union {
        value: Vec<ValidatorJson>,
    },
}

impl JsonForm for Validator {
    type Json = ValidatorJson;
}

impl TryFrom<ValidatorJson> for Validator {
    type Error = anyhow::Error;

    fn try_from(s: ValidatorJson) -> anyhow::Result<Self> {
        match s {
            ValidatorJson::Null => Ok(Validator::Null),
            ValidatorJson::Number => Ok(Validator::Float64),
            ValidatorJson::Bigint => Ok(Validator::Int64),
            ValidatorJson::CommitTs => Ok(Validator::CommitTs),
            ValidatorJson::Boolean => Ok(Validator::Boolean),
            ValidatorJson::String => Ok(Validator::String),
            ValidatorJson::Bytes => Ok(Validator::Bytes),
            ValidatorJson::Function {
                fn_type,
                args,
                returns,
            } => Ok(Validator::Function(FunctionValidator {
                fn_type,
                args: args
                    .map(|a| Validator::try_from(*a).map(Box::new))
                    .transpose()?,
                returns: returns
                    .map(|r| Validator::try_from(*r).map(Box::new))
                    .transpose()?,
            })),
            ValidatorJson::Any => Ok(Validator::Any),
            ValidatorJson::Literal { value } => Ok(Validator::Literal(value.try_into()?)),
            ValidatorJson::Id { table_name } => Ok(Validator::Id(table_name.parse()?)),
            ValidatorJson::Array { value } => Ok(Validator::Array(Box::new((*value).try_into()?))),
            ValidatorJson::Record { keys, values } => {
                let error_short_code = "InvalidRecordType";
                let keys_validator = Validator::try_from(*keys)?;
                if !keys_validator.is_subset(&Validator::String) {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        error_short_code,
                        format!(
                            "Records can only have string keys. Your validator contains a record \
                             with key typed as `{keys_validator}`, which is not a subtype of \
                             `v.string()`"
                        )
                    ))
                }
                let values_validator = FieldValidator::try_from(*values)?;
                if keys_validator.is_string_subtype_with_string_literal() {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        error_short_code,
                        "Records cannot have string literal keys".to_string()
                    ));
                }
                if values_validator.optional {
                    anyhow::bail!(ErrorMetadata::bad_request(
                        error_short_code,
                        "Records cannot have optional values".to_string()
                    ));
                }
                Ok(Validator::Record(
                    Box::new(keys_validator),
                    Box::new(values_validator.validator),
                ))
            },
            ValidatorJson::Object { value } => Ok(Validator::Object(value.try_into()?)),
            ValidatorJson::Union { value } => {
                let schemas = value
                    .into_iter()
                    .map(Validator::try_from)
                    .collect::<anyhow::Result<_>>()?;
                Ok(Validator::Union(schemas))
            },
        }
    }
}

impl TryFrom<Validator> for ValidatorJson {
    type Error = anyhow::Error;

    fn try_from(s: Validator) -> anyhow::Result<ValidatorJson> {
        Ok(match s {
            Validator::Id(table_name) => ValidatorJson::Id {
                table_name: table_name.to_string(),
            },
            Validator::Null => ValidatorJson::Null,
            Validator::Float64 => ValidatorJson::Number,
            Validator::Int64 => ValidatorJson::Bigint,
            Validator::CommitTs => ValidatorJson::CommitTs,
            Validator::Boolean => ValidatorJson::Boolean,
            Validator::String => ValidatorJson::String,
            Validator::Bytes => ValidatorJson::Bytes,
            Validator::Function(FunctionValidator {
                fn_type,
                args,
                returns,
            }) => ValidatorJson::Function {
                fn_type,
                args: args
                    .map(|a| ValidatorJson::try_from(*a).map(Box::new))
                    .transpose()?,
                returns: returns
                    .map(|r| ValidatorJson::try_from(*r).map(Box::new))
                    .transpose()?,
            },
            Validator::Literal(literal) => ValidatorJson::Literal {
                value: literal.try_into()?,
            },
            Validator::Array(t) => ValidatorJson::Array {
                value: Box::new(ValidatorJson::try_from(*t)?),
            },
            Validator::Record(k, v) => ValidatorJson::Record {
                keys: Box::new(ValidatorJson::try_from(*k)?),
                values: Box::new(FieldTypeJson::try_from(FieldValidator {
                    optional: false,
                    validator: *v,
                })?),
            },
            Validator::Object(o) => ValidatorJson::Object {
                value: o.try_into()?,
            },
            Validator::Union(v) => ValidatorJson::Union {
                value: v
                    .into_iter()
                    .map(ValidatorJson::try_from)
                    .collect::<anyhow::Result<Vec<_>>>()?,
            },
            Validator::Any => ValidatorJson::Any,
        })
    }
}

impl TryFrom<JsonValue> for LiteralValidator {
    type Error = anyhow::Error;

    fn try_from(v: JsonValue) -> anyhow::Result<Self> {
        let value: ConvexValue = v.try_into()?;
        value.try_into()
    }
}

impl TryFrom<LiteralValidator> for JsonValue {
    type Error = anyhow::Error;

    fn try_from(s: LiteralValidator) -> anyhow::Result<JsonValue> {
        let v = match s {
            LiteralValidator::Float64(f) => {
                let f: f64 = f.into();
                let n = serde_json::Number::from_f64(f).context(ErrorMetadata::bad_request(
                    "UnsupportedFloatLiteral",
                    "Infinite or NaN values are not supported in literal validators.",
                ))?;
                JsonValue::Number(n)
            },
            LiteralValidator::Int64(i) => JsonValue::from(ConvexValue::Int64(i)),
            LiteralValidator::Boolean(b) => JsonValue::Bool(b),
            LiteralValidator::String(s) => JsonValue::String(s.to_string()),
        };
        Ok(v)
    }
}

impl TryFrom<BTreeMap<String, FieldTypeJson>> for ObjectValidator {
    type Error = anyhow::Error;

    fn try_from(value: BTreeMap<String, FieldTypeJson>) -> Result<Self, Self::Error> {
        let schema = ObjectValidator(
            value
                .into_iter()
                .map(|(k, v)| {
                    let field_name = k.parse::<IdentifierFieldName>()?;
                    let field_value = FieldValidator::try_from(v).map_err(|e| {
                        e.wrap_error_message(|msg| {
                            format!("Invalid validator for key `{field_name}`: {msg}")
                        })
                    })?;
                    Ok((field_name, field_value))
                })
                .collect::<anyhow::Result<_>>()?,
        );
        Ok(schema)
    }
}

impl TryFrom<ObjectValidator> for BTreeMap<String, FieldTypeJson> {
    type Error = anyhow::Error;

    fn try_from(o: ObjectValidator) -> anyhow::Result<BTreeMap<String, FieldTypeJson>> {
        let mut map = BTreeMap::new();
        for (field, field_type) in o.0 {
            map.insert(field.to_string(), FieldTypeJson::try_from(field_type)?);
        }
        Ok(map)
    }
}
